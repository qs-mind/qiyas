//! The user's unmodified Claude Code process owns authentication, inference and its tool loop.
//! Qiyas only supplies a prompt and its own private MCP server over the documented CLI.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use qiyas_core::Session;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::provider::{BoxFuture, Sink};
use crate::{
    AgentEvent, AiError, ContentBlock, DrawingHost, Provider, StopReason, StreamEvent, TurnRequest,
    TurnResult, Usage,
};

pub const INSTALL_URL: &str = "https://code.claude.com/docs/en/setup";
const TOOLS: [&str; 3] = [
    "mcp__qiyas__qiyas_read",
    "mcp__qiyas__qiyas_execute",
    "mcp__qiyas__qiyas_capture",
];
const REQUIRED: [&str; 9] = [
    "--input-format",
    "--output-format",
    "--verbose",
    "--include-partial-messages",
    "--mcp-config",
    "--strict-mcp-config",
    "--allowedTools",
    "--disallowedTools",
    "--append-system-prompt",
];
// Shared by status checks and every conversation. Only one executable may run at a time.
static PROCESS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));
// Cancellation may finish the agent before its child has been reaped. Wait for the actual
// process permit too, so the next drawing cannot fail or start a second Claude process.
async fn process_permit() -> Result<Arc<OwnedSemaphorePermit>, AiError> {
    Ok(Arc::new(
        PROCESS
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| error("claude_busy"))?,
    ))
}

fn error(code: &'static str) -> AiError {
    AiError::Subscription(code)
}

/// Read only ordinary process settings, never credential variables, even in the parent.
fn command(executable: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(executable);
    cmd.current_dir(cwd).env_clear();
    for name in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "SYSTEMROOT",
        "WINDIR",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "HOMEDRIVE",
        "HOMEPATH",
        "TMP",
        "TEMP",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Child diagnostics can contain account information. They never enter Qiyas's logs.
        .stderr(Stdio::null())
        .kill_on_drop(true);
    cmd
}

/// Resolve only a user selection or PATH; never install or discover credential directories.
pub fn executable(selected: Option<&str>) -> Result<PathBuf, AiError> {
    if let Some(selected) = selected.filter(|s| !s.trim().is_empty()) {
        let path = PathBuf::from(selected);
        if !path.is_absolute() || !is_executable(&path) {
            return Err(error("claude_missing"));
        }
        return Ok(path);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        });
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    Err(error("claude_missing"))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// A dedicated empty folder, with no drawing/project configuration to pick up.
pub fn work_directory(home: &Path) -> Result<PathBuf, AiError> {
    let directory = home.join("claude-work");
    if directory
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(error("claude_work_directory"));
    }
    std::fs::create_dir_all(&directory).map_err(|_| error("claude_work_directory"))?;
    if std::fs::read_dir(&directory)
        .map_err(|_| error("claude_work_directory"))?
        .next()
        .is_some()
    {
        return Err(error("claude_work_directory"));
    }
    directory
        .canonicalize()
        .map_err(|_| error("claude_work_directory"))
}

struct McpConfig {
    path: PathBuf,
}

impl McpConfig {
    fn create(cwd: &Path, config: &Value) -> Result<Self, AiError> {
        let path = cwd.join(format!("mcp-{}.json", uuid::Uuid::new_v4().simple()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| error("claude_work_directory"))?;
        let guard = Self { path };
        let result = serde_json::to_writer(&mut file, config);
        drop(file);
        result.map_err(|_| error("claude_work_directory"))?;
        Ok(guard)
    }
}

impl Drop for McpConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Clone)]
struct Capabilities {
    help: String,
}

impl Capabilities {
    fn has(&self, flag: &str) -> bool {
        self.help
            .split(|c: char| c.is_whitespace() || [',', '=', '[', ']'].contains(&c))
            .any(|word| word == flag)
    }

    fn description(&self, flag: &str) -> String {
        let mut matching = false;
        let mut lines = Vec::new();
        for line in self.help.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('-') {
                matching = trimmed
                    .split_whitespace()
                    .any(|word| word.trim_end_matches(',') == flag);
            } else if trimmed.is_empty() || !line.starts_with(char::is_whitespace) {
                matching = false;
            }
            if matching {
                lines.push(trimmed);
            }
        }
        lines.join(" ")
    }

    fn effort_levels(&self) -> Vec<String> {
        if !self.has("--effort") {
            return Vec::new();
        }
        let description = self.description("--effort");
        let words: HashSet<_> = description
            .split(|c: char| !c.is_ascii_alphanumeric())
            .collect();
        ["low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .filter(|level| words.contains(level))
            .map(str::to_owned)
            .collect()
    }

    fn model_aliases(&self) -> Vec<String> {
        if !self.has("--model") {
            return Vec::new();
        }
        let description = self.description("--model");
        let Some((_, examples)) = description.split_once("e.g.") else {
            return Vec::new();
        };
        let examples = examples.split(')').next().unwrap_or(examples);
        let mut aliases = Vec::new();
        let mut quoted = None;
        let mut candidate = String::new();
        for c in examples.chars() {
            if let Some(quote) = quoted {
                if c == quote {
                    if valid_option(&candidate) && !aliases.contains(&candidate) {
                        aliases.push(std::mem::take(&mut candidate));
                    }
                    candidate.clear();
                    quoted = None;
                } else {
                    candidate.push(c);
                }
            } else if ['\'', '"', '`'].contains(&c) {
                quoted = Some(c);
            }
        }
        aliases
    }

    fn validate(&self) -> Result<(), AiError> {
        // Without strict MCP and settings isolation, running could load unrelated tools/hooks.
        if REQUIRED.iter().any(|flag| !self.has(flag))
            || !self.has("--permission-mode")
            || !self.help.contains("dontAsk")
            || !self.has("--setting-sources")
            || !self.has("--settings")
            || !self.has("--tools")
        {
            return Err(error("claude_flags"));
        }
        Ok(())
    }
}

fn valid_option(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
}

async fn output(executable: &Path, cwd: &Path, args: &[&str]) -> Result<String, AiError> {
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        command(executable, cwd).args(args).output(),
    )
    .await
    .map_err(|_| error("claude_unavailable"))?
    .map_err(|_| error("claude_missing"))?;
    if (!result.status.success() && args != ["auth", "status"]) || result.stdout.len() > 256 * 1024
    {
        return Err(error("claude_unavailable"));
    }
    String::from_utf8(result.stdout).map_err(|_| error("claude_unavailable"))
}

#[derive(Serialize)]
pub struct Status {
    pub installed: bool,
    pub compatible: bool,
    pub signed_in: Option<bool>,
    pub version: Option<String>,
    pub code: Option<String>,
    pub supports_model: bool,
    pub supports_effort: bool,
    pub model_aliases: Vec<String>,
    pub effort_levels: Vec<String>,
}

/// Explicit Settings action only. No timer, polling, login or inference.
pub async fn status(selected: Option<&str>, home: &Path) -> Result<Status, AiError> {
    let _permit = PROCESS
        .clone()
        .try_acquire_owned()
        .map_err(|_| error("claude_busy"))?;
    let path = match executable(selected) {
        Ok(path) => path,
        Err(_) => {
            return Ok(Status {
                installed: false,
                compatible: false,
                signed_in: None,
                version: None,
                code: Some("claude_missing".into()),
                supports_model: false,
                supports_effort: false,
                model_aliases: Vec::new(),
                effort_levels: Vec::new(),
            })
        }
    };
    let cwd = work_directory(home)?;
    let help = output(&path, &cwd, &["--help"]).await?;
    let capabilities = Capabilities { help };
    let version = output(&path, &cwd, &["--version"]).await?;
    // Keep only the version number, not arbitrary stdout or an account identifier.
    let version = version
        .split_whitespace()
        .next()
        .filter(|s| s.len() <= 32 && s.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map(str::to_owned);
    let compatible = capabilities.validate().is_ok();
    let mut signed_in = None;
    // Check the documented subcommand help before calling it. A missing auth status never
    // triggers a paid probe from Settings; the next user message can report authentication.
    if capabilities
        .help
        .lines()
        .any(|line| line.trim_start().starts_with("auth"))
    {
        if let Ok(help) = output(&path, &cwd, &["auth", "--help"]).await {
            if help
                .lines()
                .any(|line| line.trim_start().starts_with("status"))
            {
                if let Ok(text) = output(&path, &cwd, &["auth", "status"]).await {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        signed_in = value.get("loggedIn").and_then(Value::as_bool);
                    }
                }
            }
        }
    }
    Ok(Status {
        installed: true,
        compatible,
        signed_in,
        version,
        code: (!compatible).then(|| "claude_flags".into()),
        supports_model: capabilities.has("--model"),
        supports_effort: capabilities.has("--effort"),
        model_aliases: capabilities.model_aliases(),
        effort_levels: capabilities.effort_levels(),
    })
}

pub struct ClaudeCode {
    path: PathBuf,
    home: PathBuf,
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    session_id: Mutex<Option<String>>,
    evidence: crate::mcp::EvidenceCache,
    user_text: Mutex<String>,
    image_input: Mutex<Option<bool>>,
    reported_model: Mutex<Option<String>>,
    request_limit: Option<u32>,
}

impl ClaudeCode {
    pub fn new(
        selected: Option<&str>,
        home: PathBuf,
        session: Arc<Mutex<Option<Session>>>,
    ) -> Result<Self, AiError> {
        if !cfg!(unix) {
            return Err(error("claude_platform"));
        }
        Ok(Self {
            path: executable(selected)?,
            home,
            session,
            host: None,
            session_id: Mutex::new(None),
            evidence: crate::mcp::EvidenceCache::default(),
            user_text: Mutex::new(String::new()),
            image_input: Mutex::new(None),
            reported_model: Mutex::new(None),
            request_limit: None,
        })
    }

    /// Bound headless runs through the CLI's own agent loop.
    pub fn with_request_limit(mut self, limit: u32) -> Self {
        self.request_limit = Some(limit.max(1));
        self
    }

    async fn execute_recovering(
        &self,
        mut req: TurnRequest,
        cwd: &Path,
        capabilities: &Capabilities,
        config: Value,
        permit: Arc<OwnedSemaphorePermit>,
        sink: Sink<'_>,
    ) -> Result<TurnResult, AiError> {
        let mut backoff = crate::RetryBackoff::default();
        let mut started = None;
        let mut messages = HashMap::<String, Usage>::new();
        let mut retained = Vec::new();
        let mut seen_activity = HashSet::new();
        let mut output_retried = false;
        let mut empty = 0;
        loop {
            let known_messages = messages.keys().cloned().collect();
            let remaining = self
                .request_limit
                .map(|limit| limit.saturating_sub(messages.len() as u32));
            let result = self
                .execute_once(
                    req.clone(),
                    cwd,
                    capabilities,
                    config.clone(),
                    permit.clone(),
                    &known_messages,
                    remaining,
                    &mut |event| {
                        if let StreamEvent::Diagnostic { kind, text } = &event {
                            if kind == "CLAUDE REQUEST USAGE" {
                                if let Ok(v) = serde_json::from_str::<Value>(text) {
                                    if let (Some(id), Ok(usage)) = (
                                        v["id"].as_str(),
                                        serde_json::from_value::<Usage>(v["usage"].clone()),
                                    ) {
                                        messages.insert(id.into(), usage);
                                    }
                                }
                            }
                        }
                        if let StreamEvent::Activity(activity) = &event {
                            let key = match activity.as_ref() {
                                AgentEvent::ToolCall { id, .. } => Some((false, id.clone())),
                                AgentEvent::ToolResult { id, .. } => Some((true, id.clone())),
                                _ => None,
                            };
                            if key.is_some_and(|key| !seen_activity.insert(key)) {
                                return;
                            }
                        }
                        sink(event);
                    },
                )
                .await;
            let (mut partial, broken) = match result {
                Ok(turn) if turn.stop_reason == StopReason::MaxTokens => (turn, false),
                Err(AiError::Interrupted { partial, .. }) => (*partial, true),
                Ok(mut turn) => {
                    crate::stream_recovery::retain(retained, &mut turn);
                    if !messages.is_empty() {
                        turn.usage = Usage::default();
                        for usage in messages.values() {
                            turn.usage.add(*usage);
                        }
                    }
                    return Ok(turn);
                }
                Err(error) => return Err(error),
            };
            if req.cache_key == "picture-support"
                || self
                    .session_id
                    .lock()
                    .map_err(|_| error("claude_unavailable"))?
                    .is_none()
            {
                return Err(AiError::Interrupted {
                    provider: "claude_code",
                    partial: Box::new(partial),
                });
            }
            if self
                .request_limit
                .is_some_and(|limit| messages.len() >= limit as usize)
            {
                return Err(AiError::Incomplete {
                    provider: "claude_code".into(),
                    reason: "request_limit".into(),
                });
            }
            let delay = if broken {
                let Some(delay) = backoff.next_delay(
                    None,
                    started
                        .get_or_insert_with(tokio::time::Instant::now)
                        .elapsed(),
                ) else {
                    return Err(AiError::Interrupted {
                        provider: "claude_code",
                        partial: Box::new(partial),
                    });
                };
                delay
            } else {
                if partial.content.is_empty()
                    || partial.content.iter().all(|block| retained.contains(block))
                {
                    empty += 1;
                } else {
                    empty = 0;
                }
                if empty > 1 {
                    return Err(AiError::Incomplete {
                        provider: "claude_code".into(),
                        reason: "max_tokens".into(),
                    });
                }
                if !output_retried {
                    req.max_tokens = crate::agent::MAX_TOKENS_CAP;
                    output_retried = true;
                }
                Duration::ZERO
            };
            retained.append(&mut partial.content);
            sink(StreamEvent::Diagnostic {
                kind: "CLAUDE RESUME".into(),
                text: if broken {
                    "claude_stream: resuming the existing session"
                } else {
                    "max_tokens: resuming with the larger output budget"
                }
                .into(),
            });
            sink(StreamEvent::Retrying {
                seconds: delay.as_secs(),
                code: if broken {
                    "ai.error.claude_stream"
                } else {
                    "ai.error.output_limit"
                }
                .into(),
                diagnostic: "Resume the existing Claude Code session; retain completed work."
                    .into(),
                discard_partial: false,
            });
            tokio::time::sleep(delay).await;
            req.messages = vec![crate::Message::user(vec![ContentBlock::Text {
                text: crate::stream_recovery::continuation(false),
            }])];
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute(
        &self,
        req: TurnRequest,
        cwd: &Path,
        capabilities: &Capabilities,
        config: Value,
        permit: Arc<OwnedSemaphorePermit>,
        sink: Sink<'_>,
    ) -> Result<TurnResult, AiError> {
        self.execute_once(
            req,
            cwd,
            capabilities,
            config,
            permit,
            &HashSet::new(),
            self.request_limit,
            sink,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_once(
        &self,
        req: TurnRequest,
        cwd: &Path,
        capabilities: &Capabilities,
        config: Value,
        permit: Arc<OwnedSemaphorePermit>,
        known_messages: &HashSet<String>,
        request_limit: Option<u32>,
        sink: Sink<'_>,
    ) -> Result<TurnResult, AiError> {
        capabilities.validate()?;
        let probe = req.cache_key == "picture-support";
        let config = McpConfig::create(cwd, &config)?;
        let allowed_tools = if probe {
            String::new()
        } else {
            TOOLS.join(",")
        };
        let mut cmd = command(&self.path, cwd);
        cmd.args([
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--strict-mcp-config",
            "--permission-mode",
            "dontAsk",
            // Exclude user/project settings and hooks, without touching authentication.
            "--setting-sources",
            "",
            "--settings",
            r#"{"disableAllHooks":true}"#,
            // No built-in tools. Only the three named MCP tools are permitted.
            "--tools",
            "",
            "--disallowedTools",
            "Bash,Edit,Write,Read,Glob,Grep,WebFetch,WebSearch,Agent,Task,NotebookEdit,Skill",
            "--allowedTools",
            allowed_tools.as_str(),
            "--mcp-config",
        ])
        .arg(&config.path)
        .args(["--append-system-prompt", &req.system]);
        if let Some(limit) = request_limit.filter(|_| !probe) {
            if !capabilities.has("--max-turns") {
                return Err(error("claude_flags"));
            }
            cmd.arg("--max-turns").arg(limit.to_string());
        }
        if !probe && req.max_tokens > 0 {
            cmd.env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", req.max_tokens.to_string());
        }
        if probe {
            cmd.env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", "1")
                .env("MAX_THINKING_TOKENS", "0")
                .env("CLAUDE_CODE_MAX_RETRIES", "0");
            if capabilities.has("--max-turns") {
                cmd.args(["--max-turns", "1"]);
            }
        }
        if capabilities.has("--permission-prompts") {
            cmd.args(["--permission-prompts", "none"]);
        }
        if !req.model.is_empty() {
            if !valid_option(&req.model) || !capabilities.has("--model") {
                return Err(error("claude_flags"));
            }
            cmd.args(["--model", &req.model]);
        }
        if !req.effort.is_empty() && req.effort != "none" {
            if !valid_option(&req.effort) || !capabilities.effort_levels().contains(&req.effort) {
                return Err(error("claude_flags"));
            }
            cmd.args(["--effort", &req.effort]);
        }
        let resume = self
            .session_id
            .lock()
            .map_err(|_| error("claude_unavailable"))?
            .clone();
        if let Some(id) = resume.as_ref().filter(|_| !probe) {
            if !capabilities.has("--resume") {
                return Err(error("claude_flags"));
            }
            cmd.args(["--resume", id]);
        }
        #[cfg(not(unix))]
        return Err(error("claude_platform"));
        #[cfg(unix)]
        {
            cmd.stdin(Stdio::piped());
            let child = cmd.spawn().map_err(|_| error("claude_missing"))?;
            let mut running = Running {
                child: Some(child),
                config: Some(config),
                permit: Some(permit),
                probe,
            };
            let child = running.child.as_mut().expect("spawned child");
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| error("claude_unavailable"))?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| error("claude_unavailable"))?;
            let content = user_content(&req)?;
            let line = json!({"type":"user","message":{"role":"user","content":content}});
            stdin
                .write_all(format!("{line}\n").as_bytes())
                .await
                .map_err(|_| error("claude_unavailable"))?;
            stdin
                .shutdown()
                .await
                .map_err(|_| error("claude_unavailable"))?;
            drop(stdin);
            sink(StreamEvent::RequestSent(
                "Claude Code; one user turn".into(),
            ));
            let mut reader = BufReader::new(stdout);
            let mut state = Events {
                report_usage: true,
                known_messages: known_messages.clone(),
                ..Events::default()
            };
            loop {
                let mut line = Vec::new();
                // A bounded read prevents a malformed child stream exhausting memory.
                let count = match tokio::io::AsyncReadExt::take(&mut reader, 32 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .await
                {
                    Ok(count) => count,
                    Err(_) => break,
                };
                if count == 0 {
                    break;
                }
                if line.len() > 32 * 1024 * 1024 {
                    return Err(AiError::Parse {
                        provider: "claude_code",
                        detail: "oversized stream item",
                    });
                }
                if !line.ends_with(b"\n") {
                    break;
                }
                let value = serde_json::from_slice(&line).map_err(|_| AiError::Parse {
                    provider: "claude_code",
                    detail: "invalid event JSON",
                })?;
                if let Err(error) = state.read(value, sink) {
                    // Do not resume a transcript that still contains the rejected image.
                    if !probe && crate::pictures::image_refusal(&error) {
                        *self
                            .session_id
                            .lock()
                            .map_err(|_| self::error("claude_unavailable"))? = None;
                    }
                    return Err(error);
                }
                if let Some(model) = &state.model {
                    *self
                        .reported_model
                        .lock()
                        .map_err(|_| error("claude_unavailable"))? = Some(model.clone());
                }
                if let Some(id) = state.session_id.as_ref().filter(|_| !probe) {
                    *self
                        .session_id
                        .lock()
                        .map_err(|_| error("claude_unavailable"))? = Some(id.clone());
                }
                if state.finished {
                    break;
                }
            }
            if !state.finished {
                return Err(AiError::Interrupted {
                    provider: "claude_code",
                    partial: Box::new(state.turn()),
                });
            }
            // A terminal result remains valid even if the child fails while shutting down.
            let status = tokio::time::timeout(
                Duration::from_secs(5),
                running.child.as_mut().expect("running child").wait(),
            )
            .await;
            if matches!(status, Ok(Ok(_))) {
                running.child.take();
            }
            sink(StreamEvent::Done);
            Ok(state.turn())
        }
    }
}

/// Drop also handles cancellation by Agent::stream: interrupt now, terminate after five seconds.
struct Running {
    probe: bool,
    child: Option<Child>,
    config: Option<McpConfig>,
    permit: Option<Arc<OwnedSemaphorePermit>>,
}

#[cfg(unix)]
fn positive_pid(pid: i64) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0)
}

impl Drop for Running {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        if self.probe {
            let _ = child.start_kill();
        }
        #[cfg(unix)]
        if let Some(pid) = child
            .id()
            .filter(|_| !self.probe)
            .and_then(|pid| positive_pid(i64::from(pid)))
        {
            // SAFETY: kill takes no pointers. A positive pid_t targets one process;
            // SIGINT is valid, and this owned child has not been reaped.
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
        }
        let config = self.config.take();
        let permit = self.permit.take();
        tokio::spawn(async move {
            let _permit = permit;
            // Remove the config before another turn can acquire the permit.
            let _config = config;
            if tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_err()
            {
                #[cfg(unix)]
                if let Some(pid) = child.id().and_then(|pid| positive_pid(i64::from(pid))) {
                    // SAFETY: kill takes no pointers. A positive pid_t targets one process;
                    // SIGTERM is valid, and this owned child has not been reaped.
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
                if tokio::time::timeout(Duration::from_secs(5), child.wait())
                    .await
                    .is_err()
                {
                    let _ = child.kill().await;
                }
            }
        });
    }
}

struct ServerLifetime(CancellationToken);
impl Drop for ServerLifetime {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl Provider for ClaudeCode {
    fn handles_temporary_retries(&self) -> bool {
        true
    }
    fn image_input(&self, _model: &str) -> Option<bool> {
        self.image_input.lock().ok().and_then(|value| *value)
    }
    fn resolved_model(&self) -> Option<String> {
        self.reported_model
            .lock()
            .ok()
            .and_then(|model| model.clone())
    }
    fn set_image_input(&self, supported: Option<bool>) {
        if let Ok(mut value) = self.image_input.lock() {
            *value = supported;
        }
    }
    fn probe_image_input<'a>(&'a self, model: &'a str) -> BoxFuture<'a, Result<bool, AiError>> {
        Box::pin(async move {
            let permit = process_permit().await?;
            let cwd = work_directory(&self.home)?;
            let capabilities = Capabilities {
                help: output(&self.path, &cwd, &["--help"]).await?,
            };
            self.execute(
                crate::pictures::probe_request(model),
                &cwd,
                &capabilities,
                json!({"mcpServers":{}}),
                permit,
                &mut |_| {},
            )
            .await
            .map(|_| true)
        })
    }
    fn owns_turn(&self) -> bool {
        true
    }
    fn set_host(&mut self, host: Arc<dyn DrawingHost>) {
        self.host = Some(host);
    }
    fn begin_turn(&self, text: &str) {
        if let Ok(mut user_text) = self.user_text.lock() {
            *user_text = text.into();
        }
    }
    fn save_state(&self) -> Value {
        {
            let mut state =
                json!({"session_id": self.session_id.lock().ok().and_then(|id| id.clone())});
            let evidence = self.evidence.state();
            if evidence["contracts"]
                .as_object()
                .is_some_and(|m| !m.is_empty())
                || evidence["picture"].is_string()
            {
                state["evidence"] = evidence;
            }
            state
        }
    }
    fn restore_state(&self, state: Value) {
        self.evidence.restore(&state["evidence"]);
        let id = state["session_id"]
            .as_str()
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            .map(str::to_owned);
        if let Ok(mut current) = self.session_id.lock() {
            *current = id;
        }
    }
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let permit = process_permit().await?;
            let cwd = work_directory(&self.home)?;
            let capabilities = Capabilities {
                help: output(&self.path, &cwd, &["--help"]).await?,
            };
            output(&self.path, &cwd, &["--version"]).await?;
            capabilities.validate()?;
            let shutdown = ServerLifetime(CancellationToken::new());
            let token = uuid::Uuid::new_v4().simple().to_string();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut progress = crate::progress::Bridge::default();
            let user_text = self
                .user_text
                .lock()
                .map_err(|_| error("claude_unavailable"))?
                .clone();
            let addr = crate::mcp::serve_agent(
                self.session.clone(),
                self.host.clone(),
                user_text,
                (self.evidence.clone(), self.image_input(&req.model)),
                token.clone(),
                shutdown.0.clone(),
                (
                    Arc::new(move |ui| {
                        let _ = tx.send(ui);
                    }),
                    Arc::new(move |key, step| {
                        let _ = progress_tx.send((key, step));
                    }),
                ),
            )
            .await?;
            let config = json!({"mcpServers":{"qiyas":{
                "type":"http","url":format!("http://{addr}/mcp"),
                "headers":{"Authorization":format!("Bearer {token}")}
            }}});
            let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();
            let mut forward = move |event| {
                let _ = events_tx.send(event);
            };
            let execute =
                self.execute_recovering(req, &cwd, &capabilities, config, permit, &mut forward);
            tokio::pin!(execute);
            loop {
                tokio::select! {
                    result = &mut execute => {
                        while let Ok(event) = events_rx.try_recv() { forward_progress_event(event, &mut progress, &mut progress_rx, sink); }
                        while let Ok(ui) = rx.try_recv() { forward_progress_event(ui_event(ui), &mut progress, &mut progress_rx, sink); }
                        return result;
                    }
                    Some(event) = events_rx.recv() => forward_progress_event(event, &mut progress, &mut progress_rx, sink),
                    Some(ui) = rx.recv() => forward_progress_event(ui_event(ui), &mut progress, &mut progress_rx, sink),
                    Some((key, step)) = progress_rx.recv() => {
                        if let Some(event) = progress.step(key, step) { sink(StreamEvent::Activity(Box::new(event))); }
                    }
                }
            }
        })
    }
}

/// MCP callbacks precede the CLI result; drain them before closing its card.
fn forward_progress_event(
    event: StreamEvent,
    progress: &mut crate::progress::Bridge,
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<(String, qiyas_core::batch_progress::Step)>,
    sink: Sink<'_>,
) {
    while let Ok((key, step)) = receiver.try_recv() {
        if let Some(event) = progress.step(key, step) {
            sink(StreamEvent::Activity(Box::new(event)));
        }
    }
    let extra = if let StreamEvent::Activity(ref event) = event {
        progress.event(event)
    } else {
        Vec::new()
    };
    sink(event);
    for event in extra {
        sink(StreamEvent::Activity(Box::new(event)));
    }
}

fn ui_event(ui: Value) -> StreamEvent {
    StreamEvent::Activity(Box::new(AgentEvent::ToolResult {
        id: "claude-ui".into(),
        activity: None,
        summary: String::new(),
        count: None,
        handles: Vec::new(),
        ui: Some(ui),
        is_error: false,
    }))
}

fn user_content(req: &TurnRequest) -> Result<Vec<Value>, AiError> {
    let message = req
        .messages
        .last()
        .filter(|m| m.role == crate::Role::User)
        .ok_or_else(|| error("claude_failed"))?;
    Ok(message
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(json!({"type":"text","text":text})),
            ContentBlock::Image { media_type, data } => Some(json!({
                "type":"image","source":{"type":"base64","media_type":media_type,"data":data}
            })),
            _ => None,
        })
        .collect())
}

#[derive(Default)]
struct Events {
    report_usage: bool,
    known_messages: HashSet<String>,
    replayed: bool,
    text: String,
    message_streamed: bool,
    session_id: Option<String>,
    model: Option<String>,
    calls: HashMap<String, (String, Value)>,
    partial_calls: HashMap<u64, PartialCall>,
    completed: HashSet<String>,
    usage: Usage,
    message_usage: HashMap<String, Usage>,
    stop_reason: Option<String>,
    finished: bool,
}

struct PartialCall {
    id: String,
    name: String,
    input: Value,
    arguments: String,
}

fn terminal_error(category: &str, detail: &Value) -> Option<AiError> {
    let code = match category {
        "authentication_failed" => "claude_auth",
        "rate_limit" => "claude_limit",
        "billing_error" => "claude_billing",
        "account_on_hold" => "claude_hold",
        _ => return None,
    };
    let resets_at = crate::provider_error::reset_timestamp(detail);
    Some(if resets_at.is_some() {
        AiError::SubscriptionFailure {
            code,
            resets_at,
            retry_after: None,
        }
    } else {
        error(code)
    })
}

impl Events {
    fn turn(self) -> TurnResult {
        TurnResult {
            content: if self.text.is_empty() {
                Vec::new()
            } else {
                vec![ContentBlock::Text { text: self.text }]
            },
            stop_reason: match self.stop_reason.as_deref() {
                Some("max_tokens" | "max_output_tokens") => StopReason::MaxTokens,
                Some("content_filter" | "refusal") => crate::stream_recovery::stop(
                    "claude_code",
                    self.stop_reason.as_deref().unwrap(),
                ),
                _ => StopReason::EndTurn,
            },
            usage: self.usage,
        }
    }

    fn call(
        &mut self,
        id: String,
        name: &str,
        input: Value,
        sink: Sink<'_>,
    ) -> Result<(), AiError> {
        if !TOOLS.contains(&name) {
            return Err(error("claude_tool"));
        }
        let name = name.trim_start_matches("mcp__qiyas__");
        if self.calls.contains_key(&id) {
            return Ok(());
        }
        self.calls.insert(id.clone(), (name.into(), input.clone()));
        sink(StreamEvent::Activity(Box::new(AgentEvent::ToolCall {
            id,
            name: name.into(),
            label: crate::surface::label(name, &input),
            mutating: crate::surface::mutating(name, &input),
            activity: Some(crate::activity::describe(name, &input)),
            input,
        })));
        Ok(())
    }

    fn read(&mut self, value: Value, sink: Sink<'_>) -> Result<(), AiError> {
        match value["type"].as_str().unwrap_or("") {
            "system" if value["subtype"] == "init" => {
                self.model = value["model"]
                    .as_str()
                    .filter(|model| valid_option(model))
                    .map(str::to_owned);
                self.session_id = value["session_id"]
                    .as_str()
                    .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                    .map(str::to_owned);
                sink(StreamEvent::Activity(Box::new(
                    AgentEvent::ProviderStatus {
                        code: "ai.claudeConnected".into(),
                    },
                )));
            }
            "system" if value["subtype"] == "api_retry" => {
                let category = value["error"].as_str().unwrap_or("");
                if let Some(error) = terminal_error(category, &value) {
                    return Err(error);
                }
                sink(StreamEvent::Diagnostic {
                    kind: "CLAUDE RETRY".into(),
                    text: "Claude Code is retrying.".into(),
                });
                sink(StreamEvent::Activity(Box::new(
                    AgentEvent::ProviderStatus {
                        code: "ai.claudeWaiting".into(),
                    },
                )));
            }
            "stream_event" => {
                let event = &value["event"];
                if event["type"] == "message_delta" {
                    self.stop_reason = event["delta"]["stop_reason"]
                        .as_str()
                        .map(|s| crate::stream_recovery::reason(Some(s)));
                }
                if event["type"] == "message_start" {
                    self.replayed = event["message"]["id"]
                        .as_str()
                        .is_some_and(|id| self.known_messages.contains(id));
                    self.stop_reason = None;
                    self.message_streamed = false;
                    self.partial_calls.clear();
                }
                if self.replayed {
                    return Ok(());
                }
                if event["type"] == "content_block_start"
                    && event["content_block"]["type"] == "tool_use"
                {
                    let block = &event["content_block"];
                    let name = block["name"].as_str().unwrap_or("");
                    if !TOOLS.contains(&name) {
                        return Err(error("claude_tool"));
                    }
                    let index = event["index"].as_u64().ok_or(AiError::Parse {
                        provider: "claude_code",
                        detail: "invalid tool item",
                    })?;
                    let id = block["id"].as_str().ok_or(AiError::Parse {
                        provider: "claude_code",
                        detail: "invalid tool item",
                    })?;
                    self.partial_calls.insert(
                        index,
                        PartialCall {
                            id: id.into(),
                            name: name.into(),
                            input: block["input"].clone(),
                            arguments: String::new(),
                        },
                    );
                }
                if event["type"] == "content_block_delta" {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        self.message_streamed = true;
                        self.text.push_str(text);
                        sink(StreamEvent::TextDelta(text.into()));
                    }
                    if let Some(delta) = event["delta"]["partial_json"].as_str() {
                        if let Some(call) = event["index"]
                            .as_u64()
                            .and_then(|index| self.partial_calls.get_mut(&index))
                        {
                            if call.arguments.len().saturating_add(delta.len()) > 32 * 1024 * 1024 {
                                return Err(AiError::Parse {
                                    provider: "claude_code",
                                    detail: "invalid tool item",
                                });
                            }
                            call.arguments.push_str(delta);
                        }
                    }
                    // Raw thinking is intentionally not forwarded to the visible panel or log.
                }
                if event["type"] == "content_block_stop" {
                    if let Some(call) = event["index"]
                        .as_u64()
                        .and_then(|index| self.partial_calls.remove(&index))
                    {
                        let input = if call.arguments.is_empty() {
                            call.input
                        } else {
                            serde_json::from_str(&call.arguments).unwrap_or(Value::Null)
                        };
                        if input.is_object() {
                            self.call(call.id, &call.name, input, sink)?;
                        }
                    }
                }
            }
            "assistant" => {
                if let Some(category) = value["error"].as_str() {
                    if terminal_error(category, &value).is_none()
                        && crate::pictures::unsupported_image_message(&value.to_string())
                    {
                        return Err(error("image_unsupported"));
                    }
                    return Err(
                        terminal_error(category, &value).unwrap_or_else(|| error("claude_failed"))
                    );
                }
                if let Some(reason) = value["message"]["stop_reason"].as_str() {
                    self.stop_reason = Some(crate::stream_recovery::reason(Some(reason)));
                }
                if let Some(id) = value["message"]["id"].as_str() {
                    let u = &value["message"]["usage"];
                    let usage = Usage {
                        input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                        cache_read_input_tokens: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                        cache_creation_input_tokens: u["cache_creation_input_tokens"]
                            .as_u64()
                            .unwrap_or(0),
                        ..Usage::default()
                    };
                    self.message_usage.insert(id.into(), usage);
                    self.usage = Usage::default();
                    for usage in self.message_usage.values() {
                        self.usage.add(*usage);
                    }
                }
                if self.report_usage {
                    if let (Some(id), Some(usage)) = (
                        value["message"]["id"].as_str(),
                        value["message"]["usage"].as_object(),
                    ) {
                        let number =
                            |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
                        sink(StreamEvent::Diagnostic {
                            kind: "CLAUDE REQUEST USAGE".into(),
                            text: json!({"id":id,"usage":Usage {
                                input_tokens: number("input_tokens"),
                                output_tokens: number("output_tokens"),
                                cache_read_input_tokens: number("cache_read_input_tokens"),
                                cache_creation_input_tokens: number("cache_creation_input_tokens"),
                                ..Usage::default()
                            }})
                            .to_string(),
                        });
                    }
                }
                if value["message"]["id"]
                    .as_str()
                    .is_some_and(|id| self.known_messages.contains(id))
                {
                    self.message_streamed = false;
                    return Ok(());
                }
                if let Some(id) = value["message"]["id"].as_str() {
                    self.known_messages.insert(id.into());
                }
                for block in value["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str().unwrap_or("") {
                        "text" if !self.message_streamed => {
                            let text = block["text"].as_str().unwrap_or("");
                            self.text.push_str(text);
                            sink(StreamEvent::TextDelta(text.into()));
                        }
                        "tool_use" => {
                            let name = block["name"].as_str().unwrap_or("");
                            let id = block["id"]
                                .as_str()
                                .ok_or(AiError::Parse {
                                    provider: "claude_code",
                                    detail: "invalid tool item",
                                })?
                                .to_owned();
                            let input = block["input"].clone();
                            self.call(id, name, input, sink)?;
                        }
                        _ => {}
                    }
                }
                self.message_streamed = false;
            }
            "user" => {
                for block in value["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] != "tool_result" {
                        continue;
                    }
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    if !self.calls.contains_key(id) || !self.completed.insert(id.into()) {
                        continue;
                    }
                    let text = block["content"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            block["content"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|part| part["text"].as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        });
                    let data: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if let Some(plan) = crate::artifact::resolve(&data, "/plan").value {
                        if let Ok(plan) =
                            serde_json::from_value::<crate::surface::Plan>(plan.clone())
                        {
                            sink(StreamEvent::Activity(Box::new(AgentEvent::Plan {
                                phases: plan.phases,
                                frame: plan.frame,
                            })));
                        }
                    }
                    sink(StreamEvent::Activity(Box::new(AgentEvent::ToolResult {
                        id: id.into(),
                        activity: self
                            .calls
                            .get(id)
                            .filter(|_| block["is_error"] != true)
                            .map(|(name, input)| crate::activity::refine(name, input, &data)),
                        summary: crate::output_files::summary(&data)
                            .unwrap_or_else(|| crate::log::clip(&text)),
                        count: crate::activity::count(&data),
                        handles: Vec::new(),
                        ui: None,
                        is_error: block["is_error"].as_bool().unwrap_or(false),
                    })));
                }
            }
            "result" => {
                if value["is_error"] == true || value["subtype"] != "success" {
                    if terminal_error(value["error"].as_str().unwrap_or(""), &value).is_none()
                        && crate::pictures::unsupported_image_message(&value.to_string())
                    {
                        return Err(error("image_unsupported"));
                    }
                    sink(StreamEvent::Diagnostic {
                        kind: "CLAUDE RESULT".into(),
                        text: "Claude Code stopped without completing the turn.".into(),
                    });
                    let category = value["error"].as_str().unwrap_or("");
                    return Err(
                        terminal_error(category, &value).unwrap_or_else(|| error("claude_failed"))
                    );
                }
                let u = &value["usage"];
                self.usage = Usage {
                    input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                    cache_read_input_tokens: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cache_creation_input_tokens: u["cache_creation_input_tokens"]
                        .as_u64()
                        .unwrap_or(0),
                    ..Usage::default()
                };
                if self.text.is_empty() {
                    if let Some(text) = value["result"].as_str() {
                        self.text = text.into();
                        sink(StreamEvent::TextDelta(text.into()));
                    }
                }
                self.finished = true;
                sink(StreamEvent::Diagnostic {
                    kind: "CLAUDE RESULT".into(),
                    text: format!(
                        "success; tokens: {} in, {} out, {} cache read, {} cache written",
                        self.usage.input_tokens,
                        self.usage.output_tokens,
                        self.usage.cache_read_input_tokens,
                        self.usage.cache_creation_input_tokens
                    ),
                });
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const SESSION: &str = "6d22c32a-00da-4d6b-86b4-f13482078290";
    static TEST_PROCESS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn truncated_cli_resumes_the_session_and_deduplicates_message_usage() {
        let _serial = TEST_PROCESS.lock().await;
        let (dir, provider) = fixture(vec![]);
        let first = json!({"type":"assistant","message":{"id":"first","stop_reason":"max_tokens","content":[{"type":"text","text":"Terminal plan ready."}],"usage":{"input_tokens":100,"output_tokens":23}}});
        std::fs::write(dir.path().join("scenario.json"), json!({"help":help(),"attempts":[
            {"events":[{"type":"system","subtype":"init","session_id":SESSION}, first, {"type":"result","subtype":"success","usage":{"input_tokens":100,"output_tokens":23}}]},
            {"events":[first, {"type":"assistant","message":{"id":"second","stop_reason":"end_turn","content":[{"type":"text","text":"Sheets ready."}],"usage":{"input_tokens":120,"output_tokens":25}}}, {"type":"result","subtype":"success","usage":{"input_tokens":220,"output_tokens":48}}]}
        ]}).to_string()).unwrap();
        let result = provider
            .execute_recovering(
                request(),
                dir.path(),
                &Capabilities { help: help() },
                config(),
                permit(),
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(result.usage.output_tokens, 48);
        assert_eq!(result.usage.total_input(), 220);
        let records = records(dir.path());
        let calls = records
            .iter()
            .filter(|r| {
                r["argv"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "-p"))
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(!calls[0]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "--resume"));
        assert!(calls[1]["argv"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "--resume"));
        assert_eq!(
            calls[1]["environment"]["CLAUDE_CODE_MAX_OUTPUT_TOKENS"],
            crate::agent::MAX_TOKENS_CAP.to_string()
        );
        let inputs = records
            .iter()
            .filter_map(|r| r.get("input"))
            .collect::<Vec<_>>();
        assert!(inputs[1]
            .to_string()
            .contains("Do not repeat completed operations"));
    }

    #[tokio::test]
    async fn broken_cli_stream_resumes_with_the_bounded_delay() {
        let _serial = TEST_PROCESS.lock().await;
        let (dir, provider) = fixture(vec![]);
        std::fs::write(dir.path().join("scenario.json"), json!({"help":help(),"attempts":[
            {"events":[{"type":"system","subtype":"init","session_id":SESSION},{"type":"assistant","message":{"id":"first","content":[{"type":"text","text":"Terminal plan ready."}],"usage":{"input_tokens":100,"output_tokens":23}}}]},
            {"events":[{"type":"assistant","message":{"id":"second","stop_reason":"end_turn","content":[{"type":"text","text":"Sheets ready."}],"usage":{"input_tokens":120,"output_tokens":25}}},{"type":"result","subtype":"success","usage":{"input_tokens":120,"output_tokens":25}}]}
        ]}).to_string()).unwrap();
        let mut seen = Vec::new();
        let result = provider
            .execute_recovering(
                request(),
                dir.path(),
                &Capabilities { help: help() },
                config(),
                permit(),
                &mut |e| seen.push(e),
            )
            .await
            .unwrap();
        assert_eq!(result.usage.output_tokens, 48);
        assert!(seen.iter().any(|e| matches!(
            e,
            StreamEvent::Retrying {
                seconds: 5,
                discard_partial: false,
                ..
            }
        )));
        assert_eq!(
            records(dir.path())
                .iter()
                .filter(|r| r.get("input").is_some())
                .count(),
            2
        );
    }

    #[test]
    fn repeated_cli_messages_count_usage_once_and_parser_errors_are_distinct() {
        let mut state = Events::default();
        let message = json!({"type":"assistant","message":{"id":"one","stop_reason":"max_tokens","content":[],"usage":{"input_tokens":100,"output_tokens":23}}});
        state.read(message.clone(), &mut |_| {}).unwrap();
        state.read(message, &mut |_| {}).unwrap();
        let turn = state.turn();
        assert_eq!(turn.stop_reason, StopReason::MaxTokens);
        assert_eq!(turn.usage.output_tokens, 23);
        let error = Events::default().read(json!({"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__qiyas__qiyas_read"}]}}), &mut |_| {}).unwrap_err();
        assert_eq!(error.code(), "stream_parse");
    }

    #[test]
    fn signal_pids_must_be_positive_and_fit_pid_t() {
        for pid in [i64::MIN, -1, 0, i64::from(u32::MAX), i64::MAX] {
            assert_eq!(positive_pid(pid), None, "{pid}");
        }
        assert_eq!(positive_pid(1), Some(1));
        assert_eq!(
            positive_pid(i64::from(libc::pid_t::MAX)),
            Some(libc::pid_t::MAX)
        );
    }

    fn help() -> String {
        format!("{} --permission-mode dontAsk --permission-prompts --setting-sources --settings --tools --model --resume\n auth Manage authentication", REQUIRED.join(" "))
    }
    fn request() -> TurnRequest {
        TurnRequest {
            cache_key: "offline-test-conversation".into(),
            system: "You are Mizan. Use Qiyas only.".into(),
            messages: vec![crate::Message::user(vec![ContentBlock::Text {
                text: "Measure this drawing".into(),
            }])],
            tools: crate::catalogue(),
            model: String::new(),
            effort: "none".into(),
            max_tokens: 1024,
        }
    }
    fn fixture(events: Vec<Value>) -> (tempfile::TempDir, ClaudeCode) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude");
        std::fs::write(&path, include_str!("../tests/fixtures/claude.py")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":help(),"events":events}).to_string(),
        )
        .unwrap();
        let provider = ClaudeCode::new(
            path.to_str(),
            dir.path().join("qiyas"),
            Arc::new(Mutex::new(None)),
        )
        .unwrap();
        (dir, provider)
    }
    #[tokio::test]
    async fn headless_limit_uses_cli_turn_cap_and_reports_only_numeric_usage() {
        let _serial = TEST_PROCESS.lock().await;
        let events = vec![
            json!({"type":"assistant","message":{"id":"request-a","content":[],"usage":{"input_tokens":100,"output_tokens":20,"api_key":"credential-sentinel"}}}),
            json!({"type":"result","subtype":"success","usage":{"input_tokens":100,"output_tokens":20}}),
        ];
        let (dir, provider) = fixture(events.clone());
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":format!("{} --max-turns", help()),"events":events}).to_string(),
        )
        .unwrap();
        let provider = provider.with_request_limit(7);
        let capabilities = Capabilities {
            help: format!("{} --max-turns", help()),
        };
        let mut diagnostics = Vec::new();
        provider
            .execute(
                request(),
                dir.path(),
                &capabilities,
                config(),
                permit(),
                &mut |event| {
                    if let StreamEvent::Diagnostic { kind, text } = event {
                        if kind == "CLAUDE REQUEST USAGE" {
                            diagnostics.push(text);
                        }
                    }
                },
            )
            .await
            .unwrap();
        let calls = records(dir.path());
        assert!(calls
            .iter()
            .filter_map(|call| call["argv"].as_array())
            .any(|args| args
                .windows(2)
                .any(|pair| pair[0] == "--max-turns" && pair[1] == "7")));
        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics[0].contains("credential-sentinel"));
        let usage: Value = serde_json::from_str(&diagnostics[0]).unwrap();
        assert_eq!(usage["usage"]["input_tokens"], 100);
        assert_eq!(usage["usage"]["output_tokens"], 20);
        let missing = Capabilities { help: help() };
        assert!(provider
            .execute(
                request(),
                dir.path(),
                &missing,
                config(),
                permit(),
                &mut |_| {}
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn picture_probe_uses_stream_input_without_tools_or_a_resumed_session() {
        let _serial = TEST_PROCESS.lock().await;
        let (dir, provider) = fixture(vec![
            json!({"type":"system","subtype":"init","model":"claude-sonnet-5-5","session_id":SESSION}),
            json!({"type":"result","subtype":"success","result":"OK"}),
        ]);
        *provider.session_id.lock().unwrap() = Some(SESSION.into());
        assert!(provider.probe_image_input("sonnet").await.unwrap());
        assert_eq!(
            provider.resolved_model().as_deref(),
            Some("claude-sonnet-5-5")
        );
        let calls = records(dir.path());
        let invocation = calls
            .iter()
            .find(|call| {
                call["argv"]
                    .as_array()
                    .is_some_and(|args| args.iter().any(|a| a == "-p"))
            })
            .unwrap();
        let args = invocation["argv"].as_array().unwrap();
        let allowed = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(args[allowed + 1], "");
        assert!(!args.iter().any(|a| a == "--resume"));
        assert_eq!(invocation["mcp_config"], json!({"mcpServers":{}}));
        assert_eq!(
            invocation["environment"]["CLAUDE_CODE_MAX_OUTPUT_TOKENS"],
            "1"
        );
        let input = calls.iter().find_map(|call| call.get("input")).unwrap();
        assert_eq!(
            input["message"]["content"][0]["source"]["data"],
            crate::pictures::PNG
        );
        assert_eq!(input["message"]["content"][1]["text"], "Describe");
    }

    #[tokio::test]
    async fn cli_image_refusal_is_distinct_from_authentication_or_limits() {
        let _serial = TEST_PROCESS.lock().await;
        for (category, message, unsupported) in [
            (
                "invalid_request",
                "This model does not support images",
                true,
            ),
            (
                "authentication_failed",
                "image input is not supported",
                false,
            ),
            ("rate_limit", "image input is not supported", false),
        ] {
            let (dir, provider) = fixture(vec![
                json!({"type":"result","subtype":"error_during_execution","is_error":true,"error":category,"result":message}),
            ]);
            let error = provider.probe_image_input("sonnet").await.unwrap_err();
            assert_eq!(crate::pictures::image_refusal(&error), unsupported);
            assert_eq!(
                records(dir.path())
                    .iter()
                    .filter(|call| call.get("input").is_some())
                    .count(),
                1
            );
        }
    }

    #[tokio::test]
    async fn refused_user_image_is_resent_once_as_text() {
        let _serial = TEST_PROCESS.lock().await;
        let (dir, inner) = fixture(success());
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":help(),"events":success(),"refuse_images":true}).to_string(),
        )
        .unwrap();
        let cache = Arc::new(crate::pictures::Cache::new(
            dir.path().join("pictures.json"),
        ));
        let key = crate::pictures::Key {
            provider: "claude_code".into(),
            scope: "fixture".into(),
            model: "sonnet".into(),
        };
        let provider = crate::pictures::Detected::new(Box::new(inner), cache.clone(), key.clone());
        let mut req = request();
        req.model = "sonnet".into();
        req.messages[0].content.push(ContentBlock::Image {
            media_type: "image/png".into(),
            data: crate::pictures::PNG.into(),
        });
        provider.stream_turn(req, &mut |_| {}).await.unwrap();
        let calls = records(dir.path());
        let inputs: Vec<_> = calls.iter().filter_map(|call| call.get("input")).collect();
        assert_eq!(inputs.len(), 2);
        let invocations: Vec<_> = calls
            .iter()
            .filter_map(|call| call["argv"].as_array())
            .filter(|args| args.iter().any(|arg| arg == "-p"))
            .collect();
        assert!(!invocations[1].iter().any(|arg| arg == "--resume"));
        assert!(inputs[0].to_string().contains(crate::pictures::PNG));
        assert!(!inputs[1].to_string().contains(crate::pictures::PNG));
        assert!(inputs[1].to_string().contains("Pictures are unavailable"));
        assert_eq!(cache.get(&key), Some(false));
    }

    fn records(dir: &Path) -> Vec<Value> {
        std::fs::read_to_string(dir.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn permit() -> Arc<OwnedSemaphorePermit> {
        Arc::new(Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap())
    }
    fn config() -> Value {
        json!({"mcpServers":{"qiyas":{"type":"http","url":"http://127.0.0.1:9/mcp","headers":{"Authorization":"Bearer fake-qiyas-only"}}}})
    }
    fn success() -> Vec<Value> {
        vec![
            json!({"type":"system","subtype":"init","session_id":SESSION}),
            json!({"type":"stream_event","event":{"type":"message_start"}}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Measured."}}}),
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"Measured."}]}}),
            json!({"type":"result","subtype":"success","is_error":false,"result":"Measured.","usage":{"input_tokens":14,"output_tokens":3,"cache_read_input_tokens":2}}),
        ]
    }

    fn option_help() -> String {
        format!(
            "{}\n  --effort <level> Effort level (low, medium, high, xhigh, max)\n  --model <model> Model for this session. An alias for the latest model (e.g. 'fable', 'opus', or 'sonnet') or a model's full name.\n",
            help()
        )
    }

    #[tokio::test]
    async fn queued_fake_claude_waits_until_cancelled_process_is_reaped() {
        let _guard = TEST_PROCESS.lock().await;
        let (dir, first) = fixture(Vec::new());
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":help(),"wait":true}).to_string(),
        )
        .unwrap();
        let (second_dir, second) = fixture(success());
        let cwd = work_directory(&first.home).unwrap();
        let capabilities = Capabilities { help: help() };
        let mut sink = |_| {};
        let mut running = Box::pin(first.execute(
            request(),
            &cwd,
            &capabilities,
            config(),
            process_permit().await.unwrap(),
            &mut sink,
        ));
        let ready = async {
            while !records(dir.path())
                .iter()
                .any(|entry| entry["ready"] == true)
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::select! {
            _ = &mut running => panic!("first fake must remain running"),
            result = tokio::time::timeout(Duration::from_secs(5), ready) => result.unwrap(),
        }
        let mut waiting = Box::pin(async {
            let permit = process_permit().await.unwrap();
            second
                .execute(
                    request(),
                    &work_directory(&second.home).unwrap(),
                    &capabilities,
                    config(),
                    permit,
                    &mut |_| {},
                )
                .await
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut waiting)
                .await
                .is_err()
        );
        assert!(
            records(second_dir.path()).is_empty(),
            "queued fake has not spawned"
        );
        drop(running);
        tokio::time::timeout(Duration::from_secs(8), waiting)
            .await
            .unwrap()
            .unwrap();
        assert!(records(dir.path())
            .iter()
            .any(|entry| entry["signal"].is_number()));
        assert_eq!(
            records(second_dir.path())
                .iter()
                .filter(|entry| entry.get("input").is_some())
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn help_status_exposes_only_documented_options() {
        let _guard = TEST_PROCESS.lock().await;
        let (dir, provider) = fixture(success());
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":option_help()}).to_string(),
        )
        .unwrap();
        let result = status(provider.path.to_str(), &provider.home)
            .await
            .unwrap();
        let result = serde_json::to_value(result).unwrap();
        if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("claude-status-with-options.json"),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            result["effort_levels"],
            json!(["low", "medium", "high", "xhigh", "max"])
        );
        assert_eq!(result["model_aliases"], json!(["fable", "opus", "sonnet"]));
        assert_eq!(result["supports_model"], true);
        assert_eq!(result["supports_effort"], true);
        assert!(records(dir.path()).iter().all(|record| !record["argv"]
            .as_array()
            .is_some_and(|args| args.contains(&json!("-p")))));
        for (help, aliases, efforts) in [
            (help().replace("--model", ""), json!([]), json!([])),
            (format!("{}\n  --effort <level> Choices: low, high\n  --model <model> Alias (e.g. 'opus') or full name", help()), json!(["opus"]), json!(["low", "high"])),
        ] {
            std::fs::write(dir.path().join("scenario.json"), json!({"help":help}).to_string()).unwrap();
            let result = serde_json::to_value(status(provider.path.to_str(), &provider.home).await.unwrap()).unwrap();
            assert_eq!(result["model_aliases"], aliases);
            assert_eq!(result["effort_levels"], efforts);
            assert_eq!(result["supports_effort"], !efforts.as_array().unwrap().is_empty());
            assert_eq!(result["supports_model"], !aliases.as_array().unwrap().is_empty());
        }
        // The fixture can independently advertise the optional flags, just like two CLI versions.
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":help().replace("--model", ""),"option_help":true}).to_string(),
        )
        .unwrap();
        let result = serde_json::to_value(
            status(provider.path.to_str(), &provider.home)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["model_aliases"], json!(["fable", "opus", "sonnet"]));
        assert_eq!(
            result["effort_levels"],
            json!(["low", "medium", "high", "xhigh", "max"])
        );
    }

    #[tokio::test]
    async fn selected_effort_and_model_are_separate_argv_items() {
        let (dir, provider) = fixture(success());
        let cwd = work_directory(&provider.home).unwrap();
        let mut req = request();
        req.model = "claude-model_2.1:latest".into();
        req.effort = "xhigh".into();
        provider
            .execute(
                req,
                &cwd,
                &Capabilities {
                    help: option_help(),
                },
                config(),
                permit(),
                &mut |_| {},
            )
            .await
            .unwrap();
        let recorded = records(dir.path());
        let argv = recorded[0]["argv"].as_array().unwrap();
        for (flag, value) in [
            ("--effort", "xhigh"),
            ("--model", "claude-model_2.1:latest"),
        ] {
            let index = argv
                .iter()
                .position(|arg| arg == flag)
                .expect("selected flag must be passed");
            assert_eq!(argv[index + 1], value);
        }
        provider
            .execute(
                request(),
                &cwd,
                &Capabilities {
                    help: help().replace("--model", ""),
                },
                config(),
                permit(),
                &mut |_| {},
            )
            .await
            .unwrap();
        let calls = records(dir.path());
        let defaults = calls
            .iter()
            .rev()
            .find_map(|entry| entry["argv"].as_array())
            .unwrap();
        assert!(!defaults
            .iter()
            .any(|arg| arg == "--model" || arg == "--effort"));
    }

    #[tokio::test]
    async fn option_values_reject_unsupported_flags_and_non_identifier_characters_before_spawn() {
        let (dir, provider) = fixture(success());
        let cwd = work_directory(&provider.home).unwrap();
        for invalid in [
            " ",
            "two words",
            "a/b",
            "a\\b",
            "a\nb",
            "a\tb",
            "a;b",
            "a'b",
            "a\"b",
            "a$b",
            "a`b",
            "a=b",
            "عالي",
            "--injected",
        ] {
            for field in ["model", "effort"] {
                let mut req = request();
                if field == "model" {
                    req.model = invalid.into();
                } else {
                    req.effort = invalid.into();
                }
                let result = provider
                    .execute(
                        req,
                        &cwd,
                        &Capabilities {
                            help: option_help(),
                        },
                        config(),
                        permit(),
                        &mut |_| {},
                    )
                    .await;
                assert!(result.is_err(), "accepted {field} value {invalid:?}");
            }
        }
        for (model, effort, help) in [
            ("sonnet", "none", help().replace("--model", "")),
            ("", "high", help()),
            (
                "",
                "max",
                format!("{}\n --effort <level> low, medium", help()),
            ),
        ] {
            let mut req = request();
            req.model = model.into();
            req.effort = effort.into();
            assert!(provider
                .execute(
                    req,
                    &cwd,
                    &Capabilities { help },
                    config(),
                    permit(),
                    &mut |_| {}
                )
                .await
                .is_err());
        }
        assert!(
            records(dir.path()).is_empty(),
            "invalid options must not launch the child"
        );
    }

    #[test]
    fn wrapped_help_does_not_invent_aliases_or_effort_levels() {
        let caps = Capabilities { help: "  --model <model> Model for this session. An alias\n      (e.g. 'fable', 'opus') or the full name (e.g. 'not-an-alias')\n  --effort <level> Select effort:\n      low, high\n  --another <value> medium max sonnet".into() };
        assert_eq!(caps.model_aliases(), ["fable", "opus"]);
        assert_eq!(caps.effort_levels(), ["low", "high"]);
    }

    #[test]
    fn partial_tool_stream_shows_activity_before_assistant_summary() {
        let mut state = Events::default();
        let mut events = Vec::new();
        for event in [
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"partial-call","name":TOOLS[0],"input":{}}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"op\":\"state\",\"args\":\"{}\"}"}}),
            json!({"type":"content_block_stop","index":0}),
        ] {
            state
                .read(json!({"type":"stream_event","event":event}), &mut |event| {
                    events.push(event)
                })
                .unwrap();
        }
        assert!(events.iter().any(|event| matches!(event, StreamEvent::Activity(activity) if matches!(activity.as_ref(), AgentEvent::ToolCall { id, input, .. } if id == "partial-call" && input["op"] == "state"))));
        state.read(json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"partial-call","name":TOOLS[0],"input":{"op":"state","args":"{}"}}]}}), &mut |event| events.push(event)).unwrap();
        assert_eq!(events.iter().filter(|event| matches!(event, StreamEvent::Activity(activity) if matches!(activity.as_ref(), AgentEvent::ToolCall { .. }))).count(), 1, "the final assistant item must not create another card");
    }

    /// Exercises the real child protocol and Agent log without opening a local MCP socket.
    /// The separate private-MCP test below covers actual commands and their readbacks.
    struct FakeCliWithoutMcp(ClaudeCode);

    impl Provider for FakeCliWithoutMcp {
        fn owns_turn(&self) -> bool {
            true
        }

        fn stream_turn<'a>(
            &'a self,
            req: TurnRequest,
            sink: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            Box::pin(async move {
                self.0
                    .execute(
                        req,
                        &work_directory(&self.0.home)?,
                        &Capabilities {
                            help: option_help(),
                        },
                        config(),
                        permit(),
                        sink,
                    )
                    .await
            })
        }
    }

    #[tokio::test]
    async fn cancelled_fake_cli_turn_keeps_received_activity_in_agent_log() {
        let _guard = TEST_PROCESS.lock().await;
        let (dir, provider) = fixture(Vec::new());
        let session = provider.session.clone();
        let before_wait = vec![
            json!({"type":"system","subtype":"init","session_id":SESSION,"apiKey":"fake-secret-should-not-appear"}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Reading the drawing."}}}),
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"visible-call","name":TOOLS[0],"input":{"op":"state","args":"{}"}}]}}),
            json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"visible-call","content":"{\"total\":2}"}]}}),
            json!({"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"private-hidden-thinking"}}}),
            json!({"type":"system","subtype":"api_retry","error":"overloaded","account":"fake-private-account"}),
        ];
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":option_help(),"events_before_wait":before_wait,"wait":true}).to_string(),
        )
        .unwrap();
        let log_path = dir.path().join("agent.log");
        let settings = crate::AgentSettings {
            provider: "claude_code".into(),
            model: String::new(),
            effort: "none".into(),
            ..crate::AgentSettings::default()
        };
        let mut agent = crate::Agent::new(Box::new(FakeCliWithoutMcp(provider)), session, settings)
            .with_log(crate::AgentLog::new(log_path.clone()));
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let cancel = CancellationToken::new();
        let running = agent.run_turn("Read the drawing.".into(), tx, cancel.clone());
        tokio::pin!(running);
        let mut events = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    result = &mut running => panic!("fake must wait: {result:?}"),
                    event = rx.recv() => {
                        let event = event.expect("agent stays open");
                        let waiting = matches!(&event, AgentEvent::ProviderStatus { code } if code == "ai.claudeWaiting");
                        events.push(event);
                        if waiting { break; }
                    }
                }
            }
        }).await.unwrap();
        // Check before cancelling: an incomplete turn must already have useful diagnostics.
        let live_log = std::fs::read_to_string(&log_path).unwrap();
        cancel.cancel();
        running.await.unwrap();
        if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("claude-live-before-cancel.log"), &live_log).unwrap();
            std::fs::write(
                path.join("claude-live-events.json"),
                serde_json::to_vec_pretty(&events).unwrap(),
            )
            .unwrap();
            std::fs::copy(&log_path, path.join("claude-cancelled-agent.log")).unwrap();
        }
        assert!(live_log.contains("Reading the drawing."));
        assert!(live_log.contains("visible-call"));
        assert!(live_log.contains("qiyas_read"));
        assert!(live_log.contains("total"));
        assert!(live_log.contains("ai.claudeWaiting"));
        for secret in [
            "fake-secret-should-not-appear",
            "private-hidden-thinking",
            "fake-private-account",
            "Bearer",
            "Authorization",
            SESSION,
        ] {
            // The message names no value: a leaked one must not reach the test log either.
            assert!(
                !live_log.contains(secret),
                "the agent log holds a value it must never hold"
            );
        }
    }

    #[tokio::test]
    async fn completed_fake_cli_agent_turn_logs_text_and_usage() {
        let (dir, provider) = fixture(success());
        let session = provider.session.clone();
        let path = dir.path().join("agent.log");
        let settings = crate::AgentSettings {
            provider: "claude_code".into(),
            model: "opus".into(),
            effort: "max".into(),
            ..crate::AgentSettings::default()
        };
        let mut agent = crate::Agent::new(Box::new(FakeCliWithoutMcp(provider)), session, settings)
            .with_log(crate::AgentLog::new(path.clone()));
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        tokio::time::timeout(
            Duration::from_secs(10),
            agent.run_turn("Read the drawing.".into(), tx, CancellationToken::new()),
        )
        .await
        .unwrap()
        .unwrap();
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("CLAUDE RESULT"));
        assert!(text.contains("14 in, 3 out"));
        assert!(text.contains("Measured."));
        for forbidden in ["Bearer", "Authorization", SESSION] {
            assert!(!text.contains(forbidden));
        }
        assert!(events.iter().any(|event| matches!(event, AgentEvent::Done { usage } if usage.input_tokens == 14 && usage.output_tokens == 3)));
        if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::copy(
                dir.path().join("agent.log"),
                path.join("claude-complete-agent.log"),
            )
            .unwrap();
            std::fs::write(
                path.join("claude-complete-agent-events.json"),
                serde_json::to_vec_pretty(&events).unwrap(),
            )
            .unwrap();
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn fake_cli_enforces_argv_environment_directory_and_resume() {
        // A separate test process gives HOME a sentinel without racing other tests or
        // exposing the developer's environment to the fake recorder.
        if std::env::var_os("QIYAS_FAKE_CLAUDE_CHILD").is_none() {
            let home = tempfile::tempdir().unwrap();
            let credential_dir = home.path().join(".claude");
            std::fs::create_dir(&credential_dir).unwrap();
            let sentinel = credential_dir.join(".credentials.json");
            std::fs::write(&sentinel, "sentinel-must-never-be-read").unwrap();
            let watched = std::ffi::CString::new(sentinel.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: inotify_init1 takes only valid flags, with no pointers.
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            assert!(fd >= 0);
            // SAFETY: fd is open; watched is a live NUL-terminated CString; IN_OPEN is valid.
            assert!(unsafe { libc::inotify_add_watch(fd, watched.as_ptr(), libc::IN_OPEN) } >= 0);
            let mut child_test = std::process::Command::new(std::env::current_exe().unwrap());
            child_test
                .args([
                    "--exact",
                    "claude_code::tests::fake_cli_enforces_argv_environment_directory_and_resume",
                    "--nocapture",
                ])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", home.path())
                .env("QIYAS_FAKE_CLAUDE_CHILD", "1")
                .env("ANTHROPIC_API_KEY", "fake-api")
                .env("ANTHROPIC_AUTH_TOKEN", "fake-auth")
                .env("ANTHROPIC_BASE_URL", "https://untrusted.invalid")
                .env("ANTHROPIC_CUSTOM_HEADERS", "fake-header")
                .env("CLAUDE_CODE_OAUTH_TOKEN", "fake-oauth")
                .env("CLAUDE_CODE_USE_BEDROCK", "1")
                .env("HTTP_PROXY", "https://untrusted.invalid");
            if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
                child_test.env("QIYAS_SUBSCRIPTION_EVIDENCE", path);
            }
            let result = child_test.output().unwrap();
            let mut events = [0u8; 512];
            // SAFETY: fd is open and events is writable for exactly events.len() bytes.
            let count = unsafe { libc::read(fd, events.as_mut_ptr().cast(), events.len()) };
            // SAFETY: fd is open, owned here, and is not used or closed again.
            unsafe {
                libc::close(fd);
            }
            assert!(count < 0, "the sentinel credential file was opened");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stdout)
            );
            assert!(
                !String::from_utf8_lossy(&result.stdout).contains("sentinel-must-never-be-read")
            );
            assert!(
                !String::from_utf8_lossy(&result.stderr).contains("sentinel-must-never-be-read")
            );
            if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
                let path = PathBuf::from(path);
                std::fs::create_dir_all(&path).unwrap();
                std::fs::write(
                    path.join("sentinel.json"),
                    json!({"opened":false,"child_exit":result.status.code()}).to_string(),
                )
                .unwrap();
            }
            return;
        }
        let (dir, provider) = fixture(success());
        let cwd = work_directory(&provider.home).unwrap();
        let capabilities = Capabilities {
            help: output(&provider.path, &cwd, &["--help"]).await.unwrap(),
        };
        assert_eq!(
            output(&provider.path, &cwd, &["--version"])
                .await
                .unwrap()
                .trim(),
            "2.1.309 (Claude Code)"
        );
        let mut events = Vec::new();
        let result = provider
            .execute(
                request(),
                &cwd,
                &capabilities,
                config(),
                permit(),
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(
            result.content,
            vec![ContentBlock::Text {
                text: "Measured.".into()
            }]
        );
        assert_eq!(result.usage.input_tokens, 14);
        assert_eq!(result.usage.cache_read_input_tokens, 2);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, StreamEvent::TextDelta(_)))
                .count(),
            1
        );
        assert_eq!(provider.save_state(), json!({"session_id":SESSION}));
        provider
            .execute(
                request(),
                &cwd,
                &capabilities,
                config(),
                permit(),
                &mut |_| {},
            )
            .await
            .unwrap();
        let records = records(dir.path());
        let calls: Vec<_> = records.iter().filter(|v| v["argv"].is_array()).collect();
        assert_eq!(calls.len(), 4);
        let turns: Vec<_> = calls
            .iter()
            .filter(|v| v["argv"].as_array().unwrap().contains(&json!("-p")))
            .collect();
        assert_eq!(turns.len(), 2);
        for call in &calls {
            assert_eq!(call["cwd"], cwd.to_str().unwrap());
            let env = call["environment"].as_object().unwrap();
            assert!(!env.keys().any(|k| k.starts_with("ANTHROPIC_")
                || k.starts_with("CLAUDE_")
                || k.ends_with("_PROXY")));
        }
        for call in &turns {
            let argv = call["argv"].as_array().unwrap();
            let arg = |flag: &str| &argv[argv.iter().position(|v| v == flag).unwrap() + 1];
            assert!(argv.contains(&json!("--strict-mcp-config")));
            assert_eq!(arg("--allowedTools"), &json!(TOOLS.join(",")));
            assert_eq!(arg("--tools"), "");
            for name in [
                "Bash",
                "Edit",
                "Write",
                "WebFetch",
                "WebSearch",
                "Agent",
                "Read",
            ] {
                assert!(arg("--disallowedTools")
                    .as_str()
                    .unwrap()
                    .split(',')
                    .any(|n| n == name));
            }
            for flag in [
                "--bare",
                "--dangerously-skip-permissions",
                "--max-budget-usd",
            ] {
                assert!(!argv.contains(&json!(flag)));
            }
            assert_eq!(arg("--permission-mode"), "dontAsk");
            assert_eq!(arg("--permission-prompts"), "none");
            assert!(argv.iter().all(|value| {
                let value = value.as_str().unwrap();
                !value.contains("Bearer") && !value.contains("fake-qiyas-only")
            }));
            let config_path = Path::new(arg("--mcp-config").as_str().unwrap());
            assert_eq!(config_path.parent(), Some(cwd.as_path()));
            assert_eq!(call["mcp_config_mode"], 0o600);
            assert_eq!(call["mcp_has_bearer"], true);
            assert!(
                !config_path.exists(),
                "completed turn must remove its config"
            );
            let mcp = &call["mcp_config"];
            let mut expected = config();
            expected["mcpServers"]["qiyas"]["headers"]["Authorization"] = json!("[redacted]");
            assert_eq!(mcp, &expected);
            assert_eq!(
                mcp["mcpServers"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .collect::<Vec<_>>(),
                vec!["qiyas"]
            );
            assert_eq!(arg("--setting-sources"), "");
            assert_eq!(arg("--settings"), r#"{"disableAllHooks":true}"#);
        }
        let resumed = turns[1]["argv"].as_array().unwrap();
        let first_argv = turns[0]["argv"].as_array().unwrap();
        let config_index = first_argv.iter().position(|v| v == "--mcp-config").unwrap() + 1;
        assert_ne!(first_argv[config_index], resumed[config_index]);
        assert_eq!(
            resumed[resumed.iter().position(|v| v == "--resume").unwrap() + 1],
            SESSION
        );
        assert!(std::fs::read_dir(cwd).unwrap().next().is_none());
        assert!(!format!("{events:?}").contains("fake-oauth"));
        assert!(!records
            .iter()
            .any(|v| v.to_string().contains("sentinel-must-never-be-read")));
        if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::copy(
                dir.path().join("calls.jsonl"),
                path.join("claude-argv-environment.jsonl"),
            )
            .unwrap();
            std::fs::write(path.join("claude-events.txt"), format!("{events:#?}")).unwrap();
        }
    }

    #[test]
    fn rejects_missing_isolation_and_untrusted_session_ids() {
        for flag in REQUIRED
            .into_iter()
            .chain(["--tools", "--settings", "--setting-sources"])
        {
            assert!(
                Capabilities {
                    help: help().replace(flag, "")
                }
                .validate()
                .is_err(),
                "{flag}"
            );
        }
        let (_dir, provider) = fixture(success());
        provider.restore_state(json!({"session_id":"../../untrusted"}));
        assert_eq!(provider.save_state(), json!({"session_id":null}));
        let cwd = work_directory(&provider.home).unwrap();
        std::fs::write(cwd.join(".mcp.json"), "{}").unwrap();
        assert!(work_directory(&provider.home).is_err());
    }

    #[tokio::test]
    async fn fake_cli_drives_private_mcp_with_readback_and_undo() {
        let _guard = TEST_PROCESS.lock().await;
        for (unit, unit_code, units_per_metre) in [
            ("mm", 4, 1000.0),
            ("cm", 5, 100.0),
            ("m", 6, 1.0),
            ("in", 1, 1.0 / 0.0254),
        ] {
            let (dir, provider) = fixture(success());
            *provider.session.lock().unwrap() = Some(Session::new_blank(
                &dir.path().join("drawing.dxf"),
                unit_code,
            ));
            std::fs::write(
                dir.path().join("scenario.json"),
                json!({"help":option_help(),"events":success(),"mcp":true,"stream_tools":true,"units_per_metre":units_per_metre}).to_string(),
            )
            .unwrap();
            let session = provider.session.clone();
            let log_path = dir.path().join("agent.log");
            let settings = crate::AgentSettings {
                provider: "claude_code".into(),
                model: "sonnet".into(),
                effort: "high".into(),
                ..crate::AgentSettings::default()
            };
            let mut agent = crate::Agent::new(Box::new(provider), session.clone(), settings)
                .with_log(crate::AgentLog::new(log_path.clone()));
            let (tx, mut rx) = tokio::sync::mpsc::channel(100);
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                agent.run_turn(
                    "Draw a line, check it, then undo it and check again.".into(),
                    tx,
                    CancellationToken::new(),
                ),
            )
            .await
            .unwrap();
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
                let path = PathBuf::from(path).join(format!("claude-private-mcp-{unit}"));
                std::fs::create_dir_all(&path).unwrap();
                std::fs::copy(
                    dir.path().join("calls.jsonl"),
                    path.join("transcript.jsonl"),
                )
                .unwrap();
                std::fs::write(
                    path.join("events.json"),
                    serde_json::to_vec_pretty(&events).unwrap(),
                )
                .unwrap();
                std::fs::copy(&log_path, path.join("agent.log")).unwrap();
                std::fs::write(path.join("result.txt"), format!("{result:?}")).unwrap();
            }
            result.unwrap();
            assert!(session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .history_steps()
                .0
                .is_empty());
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, AgentEvent::ToolCall { .. }))
                    .count(),
                5
            );
            assert_eq!(events.iter().filter(|event| matches!(event, AgentEvent::ToolResult { id, .. } if id != "claude-ui")).count(), 5);
            assert!(events.iter().any(|event| matches!(event, AgentEvent::Done { usage } if usage.input_tokens == 14 && usage.output_tokens == 3)));
            let calls = records(dir.path());
            assert_eq!(
                calls
                    .iter()
                    .filter(|value| value["argv"]
                        .as_array()
                        .is_some_and(|args| args.contains(&json!("-p"))))
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|value| value["mcp_request"].is_object())
                    .count(),
                7
            );
            let log = std::fs::read_to_string(log_path).unwrap();
            for expected in [
                "TOOL CALL",
                "TOOL RESULT",
                "qiyas_execute",
                "qiyas_read",
                "Measured.",
                "14 in",
                "3 out",
            ] {
                assert!(log.contains(expected), "missing {expected} in {unit} log");
            }
            for forbidden in ["Bearer", "Authorization", SESSION] {
                assert!(!log.contains(forbidden));
            }
        }
    }

    #[test]
    fn source_does_not_access_claude_credentials_or_use_adapters() {
        let production = include_str!("claude_code.rs")
            .split("#[cfg(all(test, unix))]")
            .next()
            .unwrap();
        for forbidden in [
            ".claude",
            ".claude.json",
            ".config/claude",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "keyring",
            "agent_sdk",
            "acp",
        ] {
            // User-facing translation keys are not file access.
            let production = production
                .replace("ai.claude", "ai.provider")
                .replace("https://code.claude.com", "https://official.invalid");
            assert!(!production.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn claude_limit_preserves_only_explicit_reset_metadata() {
        let failure = Events::default()
            .read(
                json!({"type":"system","subtype":"api_retry",
            "error":"rate_limit", "resets_at":1800000000}),
                &mut |_| {},
            )
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(failure.public_error()).unwrap()["resets_at"],
            1800000000_u64
        );
    }

    #[test]
    fn subscription_failures_use_shared_kinds_without_automatic_retries() {
        for (category, kind) in [
            ("rate_limit", "plan_limit"),
            ("billing_error", "spend_blocked"),
            ("authentication_failed", "auth"),
            ("account_on_hold", "policy"),
        ] {
            let failure = Events::default()
                .read(
                    json!({"type":"system","subtype":"api_retry","error":category}),
                    &mut |_| {},
                )
                .unwrap_err();
            let public = serde_json::to_value(failure.public_error()).unwrap();
            assert_eq!(public["kind"], kind);
            assert_eq!(public["provider"], "claude_code");
            assert!(!failure.retryable());
            if matches!(category, "rate_limit" | "billing_error") {
                assert!(public["actions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["page"] == "claude_usage"));
            }
        }
    }

    #[test]
    fn events_show_tools_and_terminal_categories_never_retry() {
        let mut state = Events::default();
        let mut events = Vec::new();
        state.read(json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"call-1","name":TOOLS[0],"input":{"op":"state","args":"{}"}}]}}), &mut |e| events.push(e)).unwrap();
        state.read(json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call-1","content":[{"type":"text","text":"{\"total\":2}"}]}]}}), &mut |e| events.push(e)).unwrap();
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Activity(a) if matches!(a.as_ref(), AgentEvent::ToolCall { activity: Some(_), .. }))));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Activity(a) if matches!(a.as_ref(), AgentEvent::ToolResult { count: Some(2), .. }))));
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Activity(a) if matches!(a.as_ref(),
            AgentEvent::ToolResult { activity: Some(activity), .. }
                if activity.detail.as_ref().is_some_and(|detail| detail.key == "activity.line.read" && detail.params["n"] == 2)
        ))));
        for category in [
            "authentication_failed",
            "rate_limit",
            "billing_error",
            "account_on_hold",
        ] {
            let e = state
                .read(
                    json!({"type":"system","subtype":"api_retry","error":category}),
                    &mut |_| panic!("terminal errors never retry"),
                )
                .unwrap_err();
            assert!(!e.retryable());
            assert!(!e.public_error().message_ar.is_empty());
        }
        assert!(state.read(json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"bad","name":"Bash","input":{}}]}}), &mut |_| {}).is_err());
        assert!(state.read(json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"bad-partial","name":"Bash","input":{}}}}), &mut |_| panic!("unavailable tools must not produce an activity card")).is_err());
        let mut failed = Vec::new();
        assert!(state.read(json!({"type":"result","subtype":"error_during_execution","is_error":true,"result":"fake-secret-terminal-detail"}), &mut |event| failed.push(event)).is_err());
        assert!(failed.iter().any(
            |event| matches!(event, StreamEvent::Diagnostic { kind, .. } if kind == "CLAUDE RESULT")
        ));
        assert!(!format!("{failed:?}").contains("fake-secret-terminal-detail"));
    }

    #[tokio::test]
    async fn failed_fake_cli_turns_remove_mcp_config() {
        for fail_spawn in [false, true] {
            let (dir, provider) = fixture(Vec::new());
            let cwd = work_directory(&provider.home).unwrap();
            if fail_spawn {
                std::fs::remove_file(&provider.path).unwrap();
            }
            let semaphore = Arc::new(Semaphore::new(1));
            let permit = Arc::new(semaphore.clone().try_acquire_owned().unwrap());
            assert!(provider
                .execute(
                    request(),
                    &cwd,
                    &Capabilities { help: help() },
                    config(),
                    permit,
                    &mut |_| {},
                )
                .await
                .is_err());
            let _released = tokio::time::timeout(Duration::from_secs(8), semaphore.acquire())
                .await
                .unwrap()
                .unwrap();
            if !fail_spawn {
                let calls = records(dir.path());
                let call = calls.iter().find(|call| call["argv"].is_array()).unwrap();
                assert_eq!(call["mcp_config_mode"], 0o600);
            }
            assert!(std::fs::read_dir(&cwd).unwrap().next().is_none());
        }
    }

    #[tokio::test]
    async fn cancellation_interrupts_then_terminates_and_holds_the_process_permit() {
        for ignore_interrupt in [false, true] {
            let (dir, provider) = fixture(Vec::new());
            std::fs::write(
                dir.path().join("scenario.json"),
                json!({"help":help(),"wait":true,"ignore_interrupt":ignore_interrupt}).to_string(),
            )
            .unwrap();
            let cwd = work_directory(&provider.home).unwrap();
            let semaphore = Arc::new(Semaphore::new(1));
            let permit = Arc::new(semaphore.clone().try_acquire_owned().unwrap());
            let capabilities = Capabilities { help: help() };
            let mut sink = |_| {};
            let mut running = Box::pin(provider.execute(
                request(),
                &cwd,
                &capabilities,
                config(),
                permit,
                &mut sink,
            ));
            let ready = async {
                loop {
                    if records(dir.path()).iter().any(|v| v["ready"] == true) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            };
            tokio::select! {
                _ = &mut running => panic!("fake must wait"),
                _ = tokio::time::timeout(Duration::from_secs(5), ready) => {}
            }
            assert!(records(dir.path()).iter().any(|v| v["ready"] == true));
            let calls = records(dir.path());
            let argv = calls
                .iter()
                .find_map(|call| call["argv"].as_array())
                .unwrap();
            assert!(argv.iter().all(|value| {
                let value = value.as_str().unwrap();
                !value.contains("Bearer") && !value.contains("fake-qiyas-only")
            }));
            let config_path = PathBuf::from(
                argv[argv
                    .iter()
                    .position(|value| value == "--mcp-config")
                    .unwrap()
                    + 1]
                .as_str()
                .unwrap(),
            );
            assert_eq!(config_path.parent(), Some(cwd.as_path()));
            assert_eq!(
                config_path.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
            let contents: Value =
                serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
            assert_eq!(contents, config());
            let stopped = std::time::Instant::now();
            drop(running);
            assert!(semaphore.clone().try_acquire_owned().is_err());
            let _released = tokio::time::timeout(Duration::from_secs(8), semaphore.acquire())
                .await
                .unwrap()
                .unwrap();
            assert!(
                !config_path.exists(),
                "cancelled turn must remove its config"
            );
            assert!(std::fs::read_dir(&cwd).unwrap().next().is_none());
            if ignore_interrupt {
                assert!(stopped.elapsed() >= Duration::from_secs(5));
            }
            if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
                let path = PathBuf::from(path);
                std::fs::create_dir_all(&path).unwrap();
                let name = if ignore_interrupt {
                    "claude-sigterm.jsonl"
                } else {
                    "claude-sigint.jsonl"
                };
                std::fs::copy(dir.path().join("calls.jsonl"), path.join(name)).unwrap();
            }
            let signals: Vec<_> = records(dir.path())
                .into_iter()
                .filter_map(|v| v["signal"].as_i64())
                .collect();
            assert_eq!(
                signals,
                if ignore_interrupt {
                    vec![libc::SIGINT as i64, libc::SIGTERM as i64]
                } else {
                    vec![libc::SIGINT as i64]
                }
            );
        }
    }
}
