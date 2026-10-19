//! The agent loop: stream a turn, run the tools the model asks for on the shared session, send
//! the results back, and repeat until the model ends its turn.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use qiyas_core::{Session, Source};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::host::{changes_drawing, is_host_tool, run_host, DrawingHost, NO_HOST};
use crate::log::{clip, AgentLog};
use crate::prompt::{state_block, system_prompt, DrawingFacts, Lang, LiveState};
use crate::provider::{
    png_base64, AiError, ContentBlock, Message, Provider, ResultPart, Role, StopReason,
    StreamEvent, TurnRequest, Usage, INVALID_JSON,
};
use crate::{catalogue, execute, ToolOutput, ToolSpec};

/// Most tool calls in one user turn.
pub const MAX_TOOL_CALLS: usize = 40;
/// Longest conversation sent. The history is append-only (the API ties the model's thinking to
/// everything before it), so instead of trimming, a longer one asks for a new conversation.
pub const MAX_MESSAGES: usize = 400;
/// Why the agent stops at `MAX_MESSAGES`.
pub const TOO_LONG: &str = "conversation is long, start a new one";
/// Most base64 bytes of pictures in one conversation; every picture stays in the history and is
/// sent again with each request.
pub const IMAGE_BUDGET: usize = 15 * 1024 * 1024;
/// Most `remove_measurement` (and, apart, `resolve_conflict`) calls in one user turn.
pub const MAX_DESTRUCTIVE: usize = 10;
const NO_IMAGE_BUDGET: &str =
    "Image budget for this conversation is used up; start a new conversation to look again";
const CANCELLED: &str = "Not run: cancelled";
/// Why a call is not run when another drawing came on screen after the turn began.
pub const DRAWING_CHANGED: &str = "Not run: the drawing changed. Another drawing is on screen now.";

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSettings {
    pub model: String,
    pub effort: String,
    pub max_tokens: u32,
    pub language: Lang,
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings {
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            max_tokens: 16_000,
            language: Lang::En,
        }
    }
}

/// What the agent tells the window while a turn runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
        /// The tool changes the take-off or the BOQ work (see `ToolSpec::mutating`).
        mutating: bool,
    },
    ToolResult {
        id: String,
        /// One line for the user.
        summary: String,
        /// Entities the result is about, to highlight.
        handles: Vec<u64>,
        /// What the window should do (`show_sheet`, `zoom`, `redraw`, ...), from the tool.
        ui: Option<Value>,
        is_error: bool,
    },
    /// The model or a safety classifier declined; `category` is the API's `stop_details`.
    Refused {
        category: Option<String>,
        text: String,
    },
    /// The turn stopped early: "cancelled", "stopped after 40 steps", "max_tokens" or another
    /// stop reason.
    Stopped {
        reason: String,
    },
    /// The model finished its answer.
    Done {
        usage: Usage,
    },
}

pub struct Agent {
    provider: Box<dyn Provider>,
    session: Arc<Mutex<Option<Session>>>,
    /// The app's drawings (the file tools); without one they answer `NO_HOST`.
    host: Option<Arc<dyn DrawingHost>>,
    /// The user's message of the turn running: a path written in it counts as named by the user.
    turn_text: String,
    settings: AgentSettings,
    /// Built at the first turn and kept, byte for byte, until `clear()`.
    system: Option<String>,
    /// Append-only: earlier messages are never changed or dropped.
    history: Vec<Message>,
    /// Base64 bytes of the pictures in `history`, and the most it may hold.
    image_bytes: usize,
    image_budget: usize,
    log: Option<AgentLog>,
}

/// What the tool calls of one user turn have used so far.
#[derive(Default)]
struct TurnCounts {
    calls: usize,
    removals: usize,
    resolutions: usize,
}

impl TurnCounts {
    /// Counts a removal or resolution; `Some(why)` when the turn has had its share of them.
    fn over_cap(&mut self, name: &str) -> Option<&'static str> {
        let (n, why) = match name {
            "remove_measurement" => (
                &mut self.removals,
                "Too many removals in one turn; ask the user to confirm",
            ),
            "resolve_conflict" => (
                &mut self.resolutions,
                "Too many resolutions in one turn; ask the user to confirm",
            ),
            _ => return None,
        };
        if *n >= MAX_DESTRUCTIVE {
            return Some(why);
        }
        *n += 1;
        None
    }
}

impl Agent {
    pub fn new(
        provider: Box<dyn Provider>,
        session: Arc<Mutex<Option<Session>>>,
        settings: AgentSettings,
    ) -> Agent {
        Agent {
            provider,
            session,
            host: None,
            turn_text: String::new(),
            settings,
            system: None,
            history: Vec::new(),
            image_bytes: 0,
            image_budget: IMAGE_BUDGET,
            log: None,
        }
    }

    /// The agent with the app's drawings behind its file tools (`crate::host`).
    pub fn with_host(mut self, host: Arc<dyn DrawingHost>) -> Agent {
        self.host = Some(host);
        self
    }

    /// The agent writing every turn, request, answer, tool run and error to `log`.
    pub fn with_log(mut self, log: AgentLog) -> Agent {
        self.log = Some(log);
        self
    }

    /// Adds an entry to the log, if there is one; `body` is only made then.
    fn log(&self, title: &str, body: impl FnOnce() -> String) {
        if let Some(log) = &self.log {
            log.write(title, &body());
        }
    }

    /// The agent with another picture budget (base64 bytes per conversation), for tests.
    pub fn with_image_budget(mut self, bytes: usize) -> Agent {
        self.image_budget = bytes;
        self
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Starts a new conversation: the next turn builds a new system prompt.
    pub fn clear(&mut self) {
        self.history.clear();
        self.system = None;
        self.image_bytes = 0;
    }

    /// The new settings apply to the next request; the language of the system prompt changes
    /// only with a new conversation.
    pub fn set_settings(&mut self, settings: AgentSettings) {
        self.settings = settings;
    }

    /// Answers `user_text`, running tools on the session as the model asks, until it ends its
    /// turn, hits a limit, or `cancel` fires. Events go to `events`; a closed receiver is fine.
    pub async fn run_turn(
        &mut self,
        user_text: String,
        events: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<(), AiError> {
        self.log("USER", || user_text.clone());
        if self.history.len() >= MAX_MESSAGES {
            return self.stop(&events, TOO_LONG).await;
        }
        let system = match &self.system {
            Some(s) => s.clone(),
            None => {
                let s = self.build_system();
                self.system = Some(s.clone());
                s
            }
        };
        let (state, mut epoch) = self.state();
        self.turn_text = user_text.clone();
        self.push_user(vec![
            ContentBlock::Text { text: state },
            ContentBlock::Text { text: user_text },
        ]);
        let tools = catalogue();
        let mut counts = TurnCounts::default();
        let mut usage = Usage::default();
        let mut requests = 0;
        loop {
            if cancel.is_cancelled() {
                return self.stop(&events, "cancelled").await;
            }
            if self.history.len() > MAX_MESSAGES {
                return self.stop(&events, TOO_LONG).await;
            }
            let req = TurnRequest {
                system: system.clone(),
                messages: self.history.clone(),
                tools: tools.clone(),
                model: self.settings.model.clone(),
                effort: self.settings.effort.clone(),
                max_tokens: self.settings.max_tokens,
            };
            requests += 1;
            self.log(&format!("REQUEST {requests}"), || {
                format!(
                    "{} messages, {} tools, model {}, effort {}",
                    req.messages.len(),
                    req.tools.len(),
                    req.model,
                    req.effort
                )
            });
            let started = Instant::now();
            let Some(turn) = self.stream(req, &events, &cancel).await else {
                // The partial answer is dropped; the next user message joins this turn's.
                return self.stop(&events, "cancelled").await;
            };
            let turn = match turn {
                Ok(turn) => turn,
                Err(e) => {
                    self.log("ERROR", || match &e {
                        AiError::Api { body, request, .. } => {
                            let mut text = format!("{e}\nThe service's answer: {body}");
                            if let Some(request) = request {
                                text.push_str(&format!("\nThe request: {request}"));
                            }
                            text
                        }
                        _ => e.to_string(),
                    });
                    return Err(e);
                }
            };
            self.log(
                &format!(
                    "ANSWER {requests} ({:.1} s)",
                    started.elapsed().as_secs_f64()
                ),
                || answer_entry(&turn),
            );
            usage.add(turn.usage);
            match turn.stop_reason {
                StopReason::Refusal { category, text } => {
                    // A refusal can cut a tool call off; nothing of it is kept or run.
                    self.log("REFUSED", || {
                        format!(
                            "category: {}\n{text}",
                            category.as_deref().unwrap_or("none")
                        )
                    });
                    let _ = events.send(AgentEvent::Refused { category, text }).await;
                    return Ok(());
                }
                // With no call to answer (all were in a declined attempt before a fallback), an
                // empty user message would be refused, so the turn ends below instead.
                StopReason::ToolUse if has_calls(&turn.content) => {
                    let assistant = Message::assistant(turn.content);
                    let results = self
                        .run_tools(
                            &assistant,
                            &tools,
                            &mut counts,
                            &events,
                            &cancel,
                            &mut epoch,
                        )
                        .await;
                    self.history.push(assistant);
                    self.history.push(Message::user(results));
                    if counts.calls >= MAX_TOOL_CALLS {
                        let why = format!("stopped after {MAX_TOOL_CALLS} steps");
                        return self.stop(&events, &why).await;
                    }
                }
                reason => {
                    let assistant = Message::assistant(turn.content);
                    // Tool calls cut off at max_tokens are answered, never run.
                    let unrun: Vec<ContentBlock> = assistant
                        .tool_uses()
                        .map(|(id, _, _)| error_result(id, "Not run: the answer was cut off."))
                        .collect();
                    // An empty assistant message would be refused on the next request.
                    if !assistant.content.is_empty() {
                        self.history.push(assistant);
                    }
                    if !unrun.is_empty() {
                        self.history.push(Message::user(unrun));
                    }
                    return match reason {
                        StopReason::EndTurn | StopReason::ToolUse => {
                            self.log("DONE", || tokens(&usage));
                            let _ = events.send(AgentEvent::Done { usage }).await;
                            Ok(())
                        }
                        StopReason::MaxTokens => self.stop(&events, "max_tokens").await,
                        StopReason::Other(r) => self.stop(&events, &r).await,
                        _ => Ok(()),
                    };
                }
            }
        }
    }

    /// Ends the turn early for `reason`, in the log too.
    async fn stop(&self, events: &mpsc::Sender<AgentEvent>, reason: &str) -> Result<(), AiError> {
        self.log("STOPPED", || reason.to_string());
        stopped(events, reason).await
    }

    /// Streams one turn, forwarding text and thinking as they arrive. `None` when cancelled:
    /// the HTTP request is dropped mid-stream.
    async fn stream(
        &self,
        req: TurnRequest,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Option<Result<crate::provider::TurnResult, AiError>> {
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let turn = async move {
            let mut sink = move |e: StreamEvent| {
                let _ = tx.send(e);
            };
            // `sink` (and the sender in it) is dropped when the turn ends, which ends `forward`.
            self.provider.stream_turn(req, &mut sink).await
        };
        let forward = async {
            while let Some(e) = rx.recv().await {
                let event = match e {
                    StreamEvent::TextDelta(text) => AgentEvent::Text { text },
                    StreamEvent::Thinking(text) => AgentEvent::Thinking { text },
                    _ => continue,
                };
                let _ = events.send(event).await;
            }
        };
        tokio::select! {
            (result, ()) = async { tokio::join!(turn, forward) } => Some(result),
            () = cancel.cancelled() => None,
        }
    }

    /// Runs the tool calls of `assistant` in order and returns one result block for each, all
    /// for one user message. Once `cancel` fires, past `MAX_TOOL_CALLS`, and past
    /// `MAX_DESTRUCTIVE` removals or resolutions, the calls are answered but not run. `epoch` is
    /// the session's at the start of the turn (see `run_tool`), moved on when a file tool of the
    /// turn put another drawing on screen.
    async fn run_tools(
        &mut self,
        assistant: &Message,
        tools: &[ToolSpec],
        counts: &mut TurnCounts,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
        epoch: &mut Option<u64>,
    ) -> Vec<ContentBlock> {
        let mut results = Vec::new();
        for (id, name, input) in assistant.tool_uses() {
            let _ = events
                .send(AgentEvent::ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: input.clone(),
                    mutating: tools.iter().any(|t| t.name == name && t.mutating),
                })
                .await;
            let started = Instant::now();
            // Every answered call counts, bad input included.
            let (block, event) = if cancel.is_cancelled() {
                (error_result(id, CANCELLED), error_event(id, CANCELLED))
            } else if counts.calls >= MAX_TOOL_CALLS {
                let why = "Not run: the limit of tool calls for this message was reached.";
                (error_result(id, why), error_event(id, why))
            } else if !input.is_object() {
                counts.calls += 1;
                (
                    error_result(id, INVALID_JSON),
                    error_event(id, INVALID_JSON),
                )
            } else if let Some(why) = counts.over_cap(name) {
                counts.calls += 1;
                (error_result(id, why), error_event(id, why))
            } else if is_host_tool(name) {
                counts.calls += 1;
                let ran = self.run_host_tool(id, name, input).await;
                // The calls after it work on the drawing on screen now.
                if changes_drawing(name) && !is_error(&ran.0) {
                    *epoch = self.epoch_now();
                }
                ran
            } else {
                counts.calls += 1;
                self.run_tool(id, name, input, cancel, *epoch).await
            };
            self.log(
                &format!("TOOL {name} ({} ms)", started.elapsed().as_millis()),
                || tool_entry(id, input, &block),
            );
            results.push(block);
            let _ = events.send(event).await;
        }
        results
    }

    /// Runs one tool off the async workers (a picture takes a while to draw), holding the
    /// session's lock only while it runs. A picture past the conversation's budget is refused.
    /// Once it has the lock, the call is not run when the turn was cancelled meanwhile, or when
    /// the session is no longer the one of the turn (`epoch`): a tab switch swaps the session and
    /// cancels the turn while the call waits for the lock.
    async fn run_tool(
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        cancel: &CancellationToken,
        epoch: Option<u64>,
    ) -> (ContentBlock, AgentEvent) {
        if name == "view_image" && self.image_bytes >= self.image_budget {
            return (
                error_result(id, NO_IMAGE_BUDGET),
                error_event(id, NO_IMAGE_BUDGET),
            );
        }
        let session = self.session.clone();
        let (tool, args) = (name.to_string(), input.clone());
        let cancel = cancel.clone();
        let ran = tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            if cancel.is_cancelled() {
                return Err(CANCELLED);
            }
            match guard.as_mut() {
                None => Err("No drawing is open. Ask the user to open one."),
                Some(s) if epoch.is_some_and(|e| e != s.epoch()) => Err(DRAWING_CHANGED),
                Some(s) => Ok(execute(s, &tool, &args, Source::Ai)),
            }
        })
        .await;
        let output = match ran {
            Ok(Ok(output)) => output,
            Ok(Err(why)) => return (error_result(id, why), error_event(id, why)),
            Err(e) => {
                let why = format!("The tool {name} crashed: {e}");
                return (error_result(id, &why), error_event(id, &why));
            }
        };
        match output {
            Err(e) => {
                let why = e.to_string();
                (error_result(id, &why), error_event(id, &why))
            }
            Ok(ToolOutput::Json(value)) => json_result(id, &value),
            Ok(ToolOutput::Image { png, caption }) => {
                let data = png_base64(&png);
                if self.image_bytes + data.len() > self.image_budget {
                    return (
                        error_result(id, NO_IMAGE_BUDGET),
                        error_event(id, NO_IMAGE_BUDGET),
                    );
                }
                self.image_bytes += data.len();
                let event = AgentEvent::ToolResult {
                    id: id.to_string(),
                    summary: caption.clone(),
                    handles: Vec::new(),
                    ui: None,
                    is_error: false,
                };
                let block = ContentBlock::ToolResult {
                    tool_use_id: id.to_string(),
                    content: vec![ResultPart::Text(caption), ResultPart::Png(data)],
                    is_error: false,
                };
                (block, event)
            }
        }
    }

    /// Runs a file tool through the host, off the async workers and outside the session's lock
    /// (the host locks the app's tabs first).
    async fn run_host_tool(
        &self,
        id: &str,
        name: &str,
        input: &Value,
    ) -> (ContentBlock, AgentEvent) {
        let Some(host) = self.host.clone() else {
            return (error_result(id, NO_HOST), error_event(id, NO_HOST));
        };
        let (tool, args, text) = (name.to_string(), input.clone(), self.turn_text.clone());
        let ran =
            tokio::task::spawn_blocking(move || run_host(&*host, &tool, &args, Some(&text))).await;
        match ran {
            Ok(Ok(value)) => json_result(id, &value),
            Ok(Err(why)) => (error_result(id, &why), error_event(id, &why)),
            Err(e) => {
                let why = format!("The tool {name} crashed: {e}");
                (error_result(id, &why), error_event(id, &why))
            }
        }
    }

    /// The epoch of the drawing on screen now (none without one).
    fn epoch_now(&self) -> Option<u64> {
        let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        guard.as_ref().map(Session::epoch)
    }

    /// The system prompt for a new conversation: the rules and the drawing as it is now.
    fn build_system(&self) -> String {
        let facts = {
            let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            guard
                .as_ref()
                .map_or_else(DrawingFacts::none, DrawingFacts::of)
        };
        system_prompt(self.settings.language, &facts)
    }

    /// The `[state]` block for the next user message, and the session's epoch (none without a
    /// drawing).
    fn state(&self) -> (String, Option<u64>) {
        let (state, epoch) = {
            let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
            let state = guard.as_ref().map_or_else(LiveState::none, LiveState::of);
            (state, guard.as_ref().map(Session::epoch))
        };
        (state_block(self.settings.language, &state), epoch)
    }

    /// Adds user content. A user message already last (after a cancel, an error or a refusal,
    /// or tool results at the step limit) is the latest turn, not yet answered; the new content
    /// goes after what it holds, so tool results stay first and roles keep alternating.
    fn push_user(&mut self, content: Vec<ContentBlock>) {
        match self.history.last_mut() {
            Some(last) if last.role == Role::User => last.content.extend(content),
            _ => self.history.push(Message::user(content)),
        }
    }
}

/// A tool's JSON answer: the block for the model and the event for the window.
fn json_result(id: &str, value: &Value) -> (ContentBlock, AgentEvent) {
    let event = AgentEvent::ToolResult {
        id: id.to_string(),
        summary: summary(value),
        handles: handles(value),
        ui: value.get("ui").cloned(),
        is_error: false,
    };
    let block = ContentBlock::ToolResult {
        tool_use_id: id.to_string(),
        content: vec![ResultPart::Text(value.to_string())],
        is_error: false,
    };
    (block, event)
}

/// Whether a result block is an error.
fn is_error(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::ToolResult { is_error: true, .. })
}

/// Whether `content` holds a tool call to answer.
fn has_calls(content: &[ContentBlock]) -> bool {
    Message::assistant(content.to_vec())
        .tool_uses()
        .next()
        .is_some()
}

async fn stopped(events: &mpsc::Sender<AgentEvent>, reason: &str) -> Result<(), AiError> {
    let _ = events
        .send(AgentEvent::Stopped {
            reason: reason.to_string(),
        })
        .await;
    Ok(())
}

/// The log entry of one answer: why it stopped, its tokens, its text and the calls it made.
fn answer_entry(turn: &crate::provider::TurnResult) -> String {
    let mut lines = vec![format!("stop: {:?}", turn.stop_reason), tokens(&turn.usage)];
    for block in &turn.content {
        match block {
            ContentBlock::Text { text } => lines.push(format!("text: {}", clip(text))),
            ContentBlock::Thinking { thinking, .. } => {
                lines.push(format!("thinking: {}", clip(thinking)));
            }
            ContentBlock::ToolUse { id, name, input } => match input {
                Value::String(raw) => lines.push(format!(
                    "call {name} ({id}), input not valid JSON: {}",
                    clip(raw)
                )),
                v => lines.push(format!("call {name} ({id}): {}", clip(&v.to_string()))),
            },
            _ => {}
        }
    }
    lines.join("\n")
}

/// The log entry of one tool call: its input and what it gave back.
fn tool_entry(id: &str, input: &Value, block: &ContentBlock) -> String {
    let mut lines = vec![
        format!("id: {id}"),
        format!("input: {}", clip(&input.to_string())),
    ];
    if let ContentBlock::ToolResult {
        content, is_error, ..
    } = block
    {
        let label = if *is_error { "error" } else { "result" };
        for part in content {
            lines.push(match part {
                ResultPart::Text(t) => format!("{label}: {}", clip(t)),
                ResultPart::Png(data) => format!("{label}: a picture, {} KB", data.len() / 1024),
            });
        }
    }
    lines.join("\n")
}

fn tokens(usage: &Usage) -> String {
    format!(
        "tokens: {} in, {} out, {} cache read, {} cache written",
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens
    )
}

fn error_result(id: &str, why: &str) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: id.to_string(),
        content: vec![ResultPart::Text(why.to_string())],
        is_error: true,
    }
}

fn error_event(id: &str, why: &str) -> AgentEvent {
    AgentEvent::ToolResult {
        id: id.to_string(),
        summary: why.to_string(),
        handles: Vec::new(),
        ui: None,
        is_error: true,
    }
}

/// One line about a tool's JSON result.
fn summary(value: &Value) -> String {
    if let Some(m) = value.get("measurement").filter(|m| m.is_object()) {
        let shown = m["shown"].as_f64().unwrap_or(0.0);
        return format!(
            "{shown:.3} {}: {}",
            m["unit"].as_str().unwrap_or(""),
            m["description"].as_str().unwrap_or("")
        );
    }
    if let Some(total) = value.get("total").and_then(Value::as_u64) {
        return format!("{total} found");
    }
    if let Some(removed) = value.get("removed").and_then(Value::as_str) {
        return format!("Removed {removed}");
    }
    // The file tools say what they did in a line of their own.
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.to_string();
    }
    "Done".to_string()
}

/// The entity handles a result is about: a measurement's, or a zoom's.
fn handles(value: &Value) -> Vec<u64> {
    let list = if value["measurement"]["handles"].is_array() {
        &value["measurement"]["handles"]
    } else {
        &value["ui"]["handles"]
    };
    list.as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{BoxFuture, Sink, TurnResult};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts the requests it is asked to send, and answers each with a short text.
    struct Counting(Arc<AtomicUsize>);
    impl Provider for Counting {
        fn stream_turn<'a>(
            &'a self,
            _: TurnRequest,
            _: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(TurnResult {
                    content: vec![ContentBlock::Text { text: "ok".into() }],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                })
            })
        }
    }

    fn text(role: Role, t: &str) -> Message {
        Message {
            role,
            content: vec![ContentBlock::Text { text: t.into() }],
        }
    }

    #[tokio::test]
    async fn the_401st_message_is_refused_and_history_is_left_alone() {
        let sent = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::new(
            Box::new(Counting(sent.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        // 398 messages: one more turn (user + answer) fits exactly.
        for i in 0..199 {
            agent.history.push(text(Role::User, &format!("q{i}")));
            agent.history.push(text(Role::Assistant, &format!("a{i}")));
        }
        let (tx, mut rx) = mpsc::channel(100);
        agent
            .run_turn("fits".into(), tx.clone(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(agent.history.len(), MAX_MESSAGES);
        assert_eq!(sent.load(Ordering::SeqCst), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(AgentEvent::Text { .. }) | Ok(AgentEvent::Done { .. })
        ));

        let before = agent.history.clone();
        agent
            .run_turn("too many".into(), tx, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(sent.load(Ordering::SeqCst), 1, "no request is sent");
        assert_eq!(agent.history, before, "the history is not touched");
        let mut last = None;
        while let Ok(e) = rx.try_recv() {
            last = Some(e);
        }
        assert_eq!(
            last,
            Some(AgentEvent::Stopped {
                reason: "conversation is long, start a new one".into()
            })
        );
    }
}
