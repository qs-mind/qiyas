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
// Shared by status checks and every conversation. Never queue another subscription session.
static PROCESS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(1)));

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
    })
}

pub struct ClaudeCode {
    path: PathBuf,
    home: PathBuf,
    session: Arc<Mutex<Option<Session>>>,
    host: Option<Arc<dyn DrawingHost>>,
    session_id: Mutex<Option<String>>,
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
        if !req.model.trim().is_empty() {
            if req.model.starts_with('-') || req.model.chars().any(char::is_control) {
                return Err(error("claude_flags"));
            }
            if !capabilities.has("--model") {
                return Err(error("claude_flags"));
            }
            cmd.args(["--model", &req.model]);
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
        json!({"session_id": self.session_id.lock().ok().and_then(|id| id.clone())})
    }
    fn restore_state(&self, state: Value) {
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
            let permit = Arc::new(
                PROCESS
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| error("claude_busy"))?,
            );
            let cwd = work_directory(&self.home)?;
            let capabilities = Capabilities {
                help: output(&self.path, &cwd, &["--help"]).await?,
            };
            output(&self.path, &cwd, &["--version"]).await?;
            capabilities.validate()?;
            let shutdown = ServerLifetime(CancellationToken::new());
            let token = uuid::Uuid::new_v4().simple().to_string();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let user_text = self
                .user_text
                .lock()
                .map_err(|_| error("claude_unavailable"))?
                .clone();
            let addr = crate::mcp::serve_agent(
                self.session.clone(),
                self.host.clone(),
                user_text,
                token.clone(),
                shutdown.0.clone(),
                Arc::new(move |ui| {
                    let _ = tx.send(ui);
                }),
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
                        while let Ok(event) = events_rx.try_recv() { sink(event); }
                        while let Ok(ui) = rx.try_recv() { sink(ui_event(ui)); }
                        return result;
                    }
                    Some(event) = events_rx.recv() => sink(event),
                    Some(ui) = rx.recv() => sink(ui_event(ui)),
                }
            }
        })
    }
}

fn ui_event(ui: Value) -> StreamEvent {
    StreamEvent::Activity(Box::new(AgentEvent::ToolResult {
        id: "claude-ui".into(),
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
    calls: HashMap<String, String>,
    completed: HashSet<String>,
    usage: Usage,
    finished: bool,
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
                }
                if event["type"] == "content_block_delta" {
                    if let Some(text) = event["delta"]["text"].as_str() {
                        self.message_streamed = true;
                        self.text.push_str(text);
                        sink(StreamEvent::TextDelta(text.into()));
                    }
                    // Raw thinking is intentionally not forwarded to the visible panel or log.
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
                            if !TOOLS.contains(&name) {
                                return Err(error("claude_tool"));
                            }
                            let id = block["id"]
                                .as_str()
                                .ok_or_else(|| error("claude_stream"))?
                                .to_owned();
                            let name = name.trim_start_matches("mcp__qiyas__");
                            if self.calls.insert(id.clone(), name.into()).is_some() {
                                continue;
                            }
                            let input = block["input"].clone();
                            sink(StreamEvent::Activity(Box::new(AgentEvent::ToolCall {
                                id,
                                name: name.into(),
                                label: crate::surface::label(name, &input),
                                mutating: crate::surface::mutating(name, &input),
                                activity: Some(crate::activity::describe(name, &input)),
                                input,
                            })));
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
                    if let Some(phases) = data.pointer("/plan/phases") {
                        if let Ok(phases) = serde_json::from_value(phases.clone()) {
                            sink(StreamEvent::Activity(Box::new(AgentEvent::Plan { phases })));
                        }
                    }
                    sink(StreamEvent::Activity(Box::new(AgentEvent::ToolResult {
                        id: id.into(),
                        summary: crate::log::clip(&text),
                        count: crate::activity::count(&data),
                        handles: Vec::new(),
                        ui: None,
                        is_error: block["is_error"].as_bool().unwrap_or(false),
                    })));
                }
            }
            "result" => {
                if value["is_error"] == true || value["subtype"] != "success" {
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

    fn help() -> String {
        format!("{} --permission-mode dontAsk --permission-prompts --setting-sources --settings --tools --model --resume\n auth Manage authentication", REQUIRED.join(" "))
    }
    fn request() -> TurnRequest {
        TurnRequest {
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
        let (dir, provider) = fixture(success());
        *provider.session.lock().unwrap() =
            Some(Session::new_blank(&dir.path().join("drawing.dxf"), 6));
        std::fs::write(
            dir.path().join("scenario.json"),
            json!({"help":help(),"events":success(),"mcp":true}).to_string(),
        )
        .unwrap();
        provider.begin_turn("Draw a line, check it, then undo it and check again.");
        let mut events = Vec::new();
        let result = provider
            .stream_turn(request(), &mut |e| events.push(e))
            .await
            .unwrap();
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert!(provider
            .session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .history_steps()
            .0
            .is_empty());
        assert_eq!(events.iter().filter(|e| matches!(e,
            StreamEvent::Activity(activity) if matches!(activity.as_ref(), AgentEvent::ToolCall { .. })
        )).count(), 5);
        let calls = records(dir.path());
        assert_eq!(
            calls
                .iter()
                .filter(|v| v["argv"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!("-p"))))
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|v| v["mcp_request"].is_object())
                .count(),
            7
        );
        if let Some(path) = std::env::var_os("QIYAS_SUBSCRIPTION_EVIDENCE") {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::copy(
                dir.path().join("calls.jsonl"),
                path.join("claude-private-mcp.jsonl"),
            )
            .unwrap();
            std::fs::write(
                path.join("claude-private-mcp-events.txt"),
                format!("{events:#?}"),
            )
            .unwrap();
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
