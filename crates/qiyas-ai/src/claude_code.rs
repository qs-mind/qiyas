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
        })
    }

    async fn execute(
        &self,
        req: TurnRequest,
        cwd: &Path,
        capabilities: &Capabilities,
        config: Value,
        permit: Arc<OwnedSemaphorePermit>,
        sink: Sink<'_>,
    ) -> Result<TurnResult, AiError> {
        capabilities.validate()?;
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
            &TOOLS.join(","),
            "--mcp-config",
            &config.to_string(),
            "--append-system-prompt",
            &req.system,
        ]);
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
        if let Some(id) = &resume {
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
                permit: Some(permit),
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
            let mut state = Events::default();
            loop {
                let mut line = Vec::new();
                // A bounded read prevents a malformed child stream exhausting memory.
                let count = tokio::io::AsyncReadExt::take(&mut reader, 32 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line)
                    .await
                    .map_err(|_| error("claude_stream"))?;
                if count == 0 {
                    break;
                }
                if line.len() > 32 * 1024 * 1024 {
                    return Err(error("claude_stream"));
                }
                let value = serde_json::from_slice(&line).map_err(|_| error("claude_stream"))?;
                state.read(value, sink)?;
                if let Some(id) = &state.session_id {
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
                return Err(error("claude_stream"));
            }
            // Completion is a terminal protocol event; do not wait forever on a broken child.
            let status = tokio::time::timeout(
                Duration::from_secs(5),
                running.child.as_mut().expect("running child").wait(),
            )
            .await
            .map_err(|_| error("claude_stream"))?
            .map_err(|_| error("claude_stream"))?;
            running.child.take();
            if !status.success() {
                return Err(error("claude_failed"));
            }
            sink(StreamEvent::Done);
            Ok(TurnResult {
                content: vec![ContentBlock::Text { text: state.text }],
                stop_reason: StopReason::EndTurn,
                usage: state.usage,
            })
        }
    }
}

/// Drop also handles cancellation by Agent::stream: interrupt now, terminate after five seconds.
struct Running {
    child: Option<Child>,
    permit: Option<Arc<OwnedSemaphorePermit>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        #[cfg(unix)]
        if let Some(pid) = child.id() {
            // SAFETY: signal only this owned, unreaped child. No shell or process-name matching.
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
            }
        }
        let permit = self.permit.take();
        tokio::spawn(async move {
            let _permit = permit;
            if tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .is_err()
            {
                #[cfg(unix)]
                if let Some(pid) = child.id() {
                    // SAFETY: the child is still owned and has not been reaped.
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
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
    fn image_input(&self, model: &str) -> Option<bool> {
        crate::models::CLAUDE_MODELS
            .iter()
            .any(|(id, _)| *id == model)
            .then_some(true)
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
            let execute = self.execute(req, &cwd, &capabilities, config, permit, &mut forward);
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
    text: String,
    message_streamed: bool,
    session_id: Option<String>,
    calls: HashMap<String, (String, Value)>,
    partial_calls: HashMap<u64, PartialCall>,
    completed: HashSet<String>,
    usage: Usage,
    finished: bool,
}

struct PartialCall {
    id: String,
    name: String,
    input: Value,
    arguments: String,
}

fn terminal_error(category: &str) -> Option<AiError> {
    Some(error(match category {
        "authentication_failed" => "claude_auth",
        "rate_limit" => "claude_limit",
        "billing_error" => "claude_billing",
        "account_on_hold" => "claude_hold",
        _ => return None,
    }))
}

impl Events {
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
                if let Some(error) = terminal_error(category) {
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
                if event["type"] == "message_start" {
                    self.message_streamed = false;
                    self.partial_calls.clear();
                }
                if event["type"] == "content_block_start"
                    && event["content_block"]["type"] == "tool_use"
                {
                    let block = &event["content_block"];
                    let name = block["name"].as_str().unwrap_or("");
                    if !TOOLS.contains(&name) {
                        return Err(error("claude_tool"));
                    }
                    let index = event["index"]
                        .as_u64()
                        .ok_or_else(|| error("claude_stream"))?;
                    let id = block["id"].as_str().ok_or_else(|| error("claude_stream"))?;
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
                                return Err(error("claude_stream"));
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
                            serde_json::from_str(&call.arguments)
                                .map_err(|_| error("claude_stream"))?
                        };
                        self.call(call.id, &call.name, input, sink)?;
                    }
                }
            }
            "assistant" => {
                if let Some(category) = value["error"].as_str() {
                    return Err(terminal_error(category).unwrap_or_else(|| error("claude_failed")));
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
                                .ok_or_else(|| error("claude_stream"))?
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
                    if let Some(plan) = data.get("plan") {
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
                    sink(StreamEvent::Diagnostic {
                        kind: "CLAUDE RESULT".into(),
                        text: "Claude Code stopped without completing the turn.".into(),
                    });
                    let category = value["error"].as_str().unwrap_or("");
                    return Err(terminal_error(category).unwrap_or_else(|| error("claude_failed")));
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
            // SAFETY: these calls watch a test-owned path; descriptors are checked and closed.
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            assert!(fd >= 0);
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
            let count = unsafe { libc::read(fd, events.as_mut_ptr().cast(), events.len()) };
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
            let mcp: Value = serde_json::from_str(arg("--mcp-config").as_str().unwrap()).unwrap();
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
            let stopped = std::time::Instant::now();
            drop(running);
            assert!(semaphore.clone().try_acquire_owned().is_err());
            let _released = tokio::time::timeout(Duration::from_secs(8), semaphore.acquire())
                .await
                .unwrap()
                .unwrap();
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
