//! The agent loop: stream a turn, run the tools the model asks for on the shared session, send
//! the results back, and repeat until the model ends its turn.

use crate::judgement::{self, Job};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use qiyas_core::{Session, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::context::{self, TurnContext, UserMessage};
use crate::conversation::{self, Conversation, PhaseState, TranscriptEntry};
use crate::host::{changes_drawing, run_host, DrawingHost, NO_HOST, NO_WINDOW};
use crate::log::{clip, AgentLog};
use crate::prompt::{state_block, system_prompt, DrawingFacts, Lang, LiveState};
use crate::provider::{
    png_base64, AiError, ContentBlock, Message, Provider, ResultPart, Role, StopReason,
    StreamEvent, TurnRequest, Usage, INVALID_JSON,
};
use crate::surface::{self, Route};
use crate::window::run_window;
use crate::{catalogue, ToolError, ToolOutput};

/// Most tool calls in one user turn.
pub const MAX_TOOL_CALLS: usize = 40;
/// Most calls once a tool of the turn has changed the drawing.
pub const MAX_DRAWING_CALLS: usize = 120;
/// Longest conversation sent. The history is append-only (the API ties the model's thinking to
/// everything before it; only old pictures give way, see `trim_pictures`), so instead of
/// trimming, a longer one asks for a new conversation.
pub const MAX_MESSAGES: usize = 400;
/// Why the agent stops at `MAX_MESSAGES`: only when compaction could not make room.
pub const TOO_LONG: &str = "conversation is long, start a new one";
/// Past this many messages the conversation is compacted (`compact`) down to `COMPACT_TO`.
pub const COMPACT_AT: usize = 300;
pub const COMPACT_TO: usize = 160;
/// The newest messages compaction always keeps whole (30 steps).
pub const KEEP_RECENT: usize = 60;
/// Marks the note compaction puts at the start of the history.
const COMPACTED: &str = "[earlier work, compacted to make room]";
/// Most base64 bytes of pictures Mizan draws in one conversation (the user's own count too).
pub const IMAGE_BUDGET: usize = 40 * 1024 * 1024;
/// The history keeps at most this many pictures, or this many base64 bytes of them; past
/// either, the oldest give way to a line of text until `KEEP_PICTURES` and `KEEP_PICTURE_BYTES`
/// are left. Cut seldom and in one go, so the requests between cuts share their cached start.
pub const MAX_PICTURES: usize = 12;
pub const MAX_PICTURE_BYTES: usize = 16 * 1024 * 1024;
pub const KEEP_PICTURES: usize = 8;
pub const KEEP_PICTURE_BYTES: usize = 10 * 1024 * 1024;
/// What an old picture becomes.
pub const PICTURE_REMOVED: &str = "[earlier picture removed; look again if you need it]";
/// Most commands marked destructive by the registry in one user turn.
pub const MAX_DESTRUCTIVE: usize = 10;
const NO_IMAGE_BUDGET: &str =
    "Image budget for this conversation is used up; start a new conversation to look again";
const CANCELLED: &str = "Not run: cancelled";
/// Why a call is not run when another drawing came on screen after the turn began.
pub const DRAWING_CHANGED: &str = "Not run: the drawing changed. Another drawing is on screen now.";

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSettings {
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub max_tokens: u32,
    pub language: Lang,
    pub autonomy: Autonomy,
    pub measurement_method: MeasurementMethod,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    PerJob,
    PerPhase,
    #[default]
    Autonomous,
}

/// Only the rule-set envelope is understood here; applying rules belongs to Phase 17.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeasurementMethod {
    pub name: Option<String>,
    pub rules_loaded: bool,
}

impl MeasurementMethod {
    /// Drawing knowledge is authoritative, including a project override or an explicit clear.
    pub fn for_session(session: &Session, fallback: &Self) -> Result<Self, qiyas_core::CoreError> {
        if qiyas_core::knowledge::get(
            &session.drawing.doc,
            qiyas_core::knowledge::Kind::Measurement,
            "selection",
        )?
        .is_none()
        {
            return Ok(fallback.clone());
        }
        let selection = session.measurement_selection()?;
        Ok(Self {
            name: selection.effective().map(|method| method.name.clone()),
            rules_loaded: selection
                .effective()
                .is_some_and(|method| !method.rules.is_empty()),
        })
    }

    pub fn instruction(&self) -> &'static str {
        if self.name.is_none() {
            "No method selected. Use cited geometric quantities."
        } else if self.rules_loaded {
            "Reuse the drawing's cited method rules. Preserve the project override."
        } else {
            "Write rules from your knowledge in your own words, with <standard> clause <n> citations. Save via measurement.set_method for reuse. List uncertain rules/references in the end report."
        }
    }

    pub fn state(&self) -> Value {
        serde_json::json!({"name":self.name,"rules_loaded":self.rules_loaded,"instruction":self.instruction()})
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        AgentSettings {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            max_tokens: 16_000,
            language: Lang::En,
            autonomy: Autonomy::Autonomous,
            measurement_method: MeasurementMethod::default(),
        }
    }
}

/// What the agent tells the window while a turn runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    ProviderStatus {
        code: String,
    },
    Retrying {
        seconds: u64,
        code: String,
        discard_partial: bool,
    },
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    /// The current task's checklist, also kept in the tool result in history.
    Plan {
        phases: Vec<surface::PlanPhase>,
    },
    AwaitingApproval {
        plan: surface::Plan,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
        /// The command or operation kept in the card's technical details.
        label: Option<String>,
        /// Translatable presentation metadata, absent in older saved conversations.
        #[serde(default)]
        activity: Option<crate::activity::Activity>,
        /// An execute operation other than planning, selecting or viewing.
        mutating: bool,
    },
    ToolResult {
        id: String,
        /// Diagnostic summary for the card's technical details and the log.
        summary: String,
        /// A count from the structured result, never from its English summary.
        #[serde(default)]
        count: Option<u64>,
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
    job: Job,
    chat_language: Lang,
    settings: AgentSettings,
    /// Built at the first turn and kept, byte for byte, until `clear()`.
    system: Option<String>,
    /// Recent results remain whole; older evidence becomes summaries with source identities.
    history: Vec<Message>,
    /// Base64 bytes of the pictures in `history`, and the most it may hold.
    image_bytes: usize,
    image_budget: usize,
    log: Option<AgentLog>,
    id: String,
    title: String,
    created: u64,
    transcript: Mutex<Vec<TranscriptEntry>>,
    checkpoint: Option<Arc<Mutex<Option<Conversation>>>>,
    resumed: bool,
    plan: Option<surface::Plan>,
    phase_state: PhaseState,
    /// Metadata captured while the session is locked on a blocking worker.
    phase_epoch: Option<u64>,
}

/// What a user turn starts from: the `[state]` block, the session's epoch (none without a
/// drawing), and a picture of a new selection or region with its caption.
struct Opening {
    state: String,
    epoch: Option<u64>,
    drawing: Option<PathBuf>,
    system: Option<String>,
    picture: Option<(Vec<u8>, String)>,
}

/// What the tool calls of one user turn have used so far.
#[derive(Default)]
struct TurnCounts {
    calls: usize,
    destructive: usize,
    changed_drawing: bool,
    plan_seen: bool,
    approved: bool,
    awaiting: Option<surface::Plan>,
    ineffective: BTreeMap<String, usize>,
}

impl TurnCounts {
    fn limit(&self) -> usize {
        if self.changed_drawing {
            MAX_DRAWING_CALLS
        } else {
            MAX_TOOL_CALLS
        }
    }

    /// Reserve the destructive commands before running any part of a call.
    fn over_cap(&mut self, name: &str, input: &Value) -> Option<&'static str> {
        let count = surface::destructive_count(name, input);
        if self.destructive.saturating_add(count) > MAX_DESTRUCTIVE {
            return Some("Too many destructive commands in one turn; ask the user to confirm");
        }
        self.destructive += count;
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
            job: Job::Drawing,
            chat_language: settings.language,
            settings,
            system: None,
            history: Vec::new(),
            image_bytes: 0,
            image_budget: IMAGE_BUDGET,
            log: None,
            id: String::new(),
            title: String::new(),
            created: 0,
            transcript: Mutex::new(Vec::new()),
            checkpoint: None,
            resumed: false,
            plan: None,
            phase_state: PhaseState::default(),
            phase_epoch: None,
        }
    }

    pub fn with_checkpoint(mut self, checkpoint: Arc<Mutex<Option<Conversation>>>) -> Self {
        self.checkpoint = Some(checkpoint);
        self.update_checkpoint();
        self
    }

    pub fn snapshot(&self) -> Conversation {
        // Streaming and desktop resets call this while another thread may own the session.
        // Only use metadata returned by the blocking opening/tool steps; never lock it here.
        let drawing = self.phase_state.drawing.clone();
        let phase_state = self.phase_state.clone();
        let mut history = conversation::history_without_pictures(&self.history);
        // A crash during a group of calls must still leave one result for every call.
        if let Some(index) = history.iter().rposition(|m| m.role == Role::Assistant) {
            let ids: Vec<String> = history[index]
                .tool_uses()
                .map(|(id, _, _)| id.to_string())
                .collect();
            if !ids.is_empty() {
                if index + 1 == history.len() {
                    history.push(Message::user(Vec::new()));
                }
                for id in ids {
                    if !history[index + 1].content.iter().any(|b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == &id)) {
                        history[index + 1].content.push(error_result(&id, "Interrupted before the result was saved; it may have run. Read the drawing before retrying."));
                    }
                }
            }
        }
        Conversation {
            id: self.id.clone(),
            title: self.title.clone(),
            created: self.created,
            updated: conversation::now(),
            drawing,
            provider: self.settings.provider.clone(),
            model: self.settings.model.clone(),
            language: self.settings.language,
            system: self.system.clone(),
            history,
            image_bytes: self.image_bytes,
            provider_state: self.provider.save_state(),
            transcript: self
                .transcript
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            phase_state,
        }
        .without_pictures()
    }

    pub fn resume(
        mut conv: Conversation,
        provider: Box<dyn Provider>,
        session: Arc<Mutex<Option<Session>>>,
        settings: AgentSettings,
    ) -> Self {
        // Old saved prompts injected the quantity workflow into every drawing chat. Rebuild
        // those prompts and discard their tied provider signatures, as for a model change.
        let old_workflow = conv
            .system
            .as_ref()
            .is_some_and(|s| s.contains("\nTake-off workflow:"));
        if old_workflow {
            conv.system = None;
        }
        if !old_workflow && conv.provider == settings.provider && conv.model == settings.model {
            provider.restore_state(conv.provider_state);
        } else {
            for message in &mut conv.history {
                let boundary = message.fallback_boundary();
                message.content = std::mem::take(&mut message.content)
                    .into_iter()
                    .enumerate()
                    .filter(|(index, block)| {
                        (*index >= boundary || matches!(block, ContentBlock::Text { .. }))
                            && !matches!(
                                block,
                                ContentBlock::Thinking { .. } | ContentBlock::Raw(_)
                            )
                    })
                    .map(|(_, block)| block)
                    .collect();
            }
            conv.history.retain(|message| !message.content.is_empty());
            let mut history: Vec<Message> = Vec::with_capacity(conv.history.len());
            for message in conv.history {
                if let Some(last) = history.last_mut().filter(|last| last.role == message.role) {
                    last.content.extend(message.content);
                } else {
                    history.push(message);
                }
            }
            conv.history = history;
        }
        let plan = conv.transcript.iter().rev().find_map(|entry| match entry {
            TranscriptEntry::Event {
                event: AgentEvent::Plan { phases },
            } => Some(surface::Plan {
                phases: phases.clone(),
            }),
            _ => None,
        });
        let mut agent = Self::new(provider, session, settings);
        agent.id = conv.id;
        agent.title = conv.title;
        agent.created = conv.created;
        agent.system = conv.system;
        agent.history = conv.history;
        let mut pending = BTreeMap::new();
        for entry in &conv.transcript {
            match entry {
                TranscriptEntry::User { text, .. } => agent.job = Job::request(text, agent.job),
                TranscriptEntry::Event {
                    event:
                        AgentEvent::ToolCall {
                            id, name, input, ..
                        },
                } => {
                    pending.insert(id.as_str(), (name, input));
                }
                TranscriptEntry::Event {
                    event:
                        AgentEvent::ToolResult {
                            id,
                            is_error: false,
                            ..
                        },
                } if pending
                    .get(id.as_str())
                    .is_some_and(|(name, input)| Job::observes(name, input)) =>
                {
                    agent.job = Job::Takeoff;
                }
                _ => {}
            }
        }
        agent.image_bytes = conv.image_bytes;
        agent.transcript = Mutex::new(conv.transcript);
        agent.resumed = true;
        agent.plan = plan;
        // The first blocking opening validates this saved attribution against its drawing.
        // Epochs are process-local, so a resumed phase is matched by path once.
        agent.phase_state = conv.phase_state;
        agent.phase_state.drawing = conv.drawing;
        agent
    }

    /// Save As changes a drawing's path without changing its epoch or phase attribution.
    pub fn drawing_saved_as(&mut self, drawing: PathBuf) {
        self.phase_state.drawing = Some(drawing);
    }

    fn update_checkpoint(&self) {
        if let Some(checkpoint) = &self.checkpoint {
            let snapshot = self.snapshot();
            *checkpoint.lock().unwrap_or_else(PoisonError::into_inner) = Some(snapshot);
        }
    }

    async fn emit(&self, events: &mpsc::Sender<AgentEvent>, event: AgentEvent) {
        conversation::append(
            &mut self
                .transcript
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            TranscriptEntry::Event {
                event: event.clone(),
            },
        );
        // Text deltas append in place. Copy the full conversation only at a step boundary.
        if !matches!(event, AgentEvent::Text { .. } | AgentEvent::Thinking { .. }) {
            self.update_checkpoint();
        }
        let _ = events.send(event).await;
    }

    /// The agent with the app's drawings behind its file tools (`crate::host`).
    pub fn with_host(mut self, host: Arc<dyn DrawingHost>) -> Agent {
        self.provider.set_host(host.clone());
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
        self.job = Job::Drawing;
        self.system = None;
        self.image_bytes = 0;
        self.id.clear();
        self.title.clear();
        self.created = 0;
        self.transcript
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.plan = None;
        self.resumed = false;
        self.phase_state = PhaseState::default();
        self.phase_epoch = None;
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
        self.run_turn_with(UserMessage::text(user_text), events, cancel)
            .await
    }

    /// `run_turn` for a message with what the user points at (the selection, a marked region,
    /// the view: written into the `[state]` block, with a picture of a new selection or region)
    /// and the pictures they attached (sent before their words).
    pub async fn run_turn_with(
        &mut self,
        message: UserMessage,
        events: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<(), AiError> {
        if self.id.is_empty() {
            self.id = uuid::Uuid::new_v4().to_string();
            self.created = conversation::now();
            self.title = message.text.chars().take(60).collect();
        }
        conversation::append(
            &mut self
                .transcript
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
            TranscriptEntry::User {
                text: message.text.clone(),
                pointed: message.pointed.clone(),
                images: vec![conversation::PICTURE_REMOVED.to_string(); message.images.len()],
            },
        );
        let result = self.run_turn_inner(message, events, cancel).await;
        self.update_checkpoint();
        result
    }

    async fn run_turn_inner(
        &mut self,
        message: UserMessage,
        events: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<(), AiError> {
        // A native CLI owns its tool loop. Never silently bypass Qiyas's phase approvals.
        if self.provider.owns_turn() && self.settings.autonomy != Autonomy::Autonomous {
            return Err(AiError::Subscription("claude_approval"));
        }
        let UserMessage {
            text: user_text,
            context: pointed,
            images,
            pointed: _,
        } = message;
        self.log("USER", || {
            let mut entry = user_text.clone();
            if !pointed.selection.is_empty() {
                entry.push_str(&format!(
                    "\n(selection: {} entities)",
                    pointed.selection.len()
                ));
            }
            if pointed.region.is_some() {
                entry.push_str("\n(a marked region)");
            }
            if !images.is_empty() {
                entry.push_str(&format!("\n({} pictures attached)", images.len()));
            }
            entry
        });
        self.compact();
        if self.history.len() >= MAX_MESSAGES {
            return self.stop(&events, TOO_LONG).await;
        }
        if let Some(why) = images.iter().find_map(context::UserImage::refused) {
            return self.stop(&events, &why).await;
        }
        let Opening {
            state,
            mut epoch,
            drawing,
            system,
            picture,
        } = match self.opening(pointed, &cancel).await {
            Ok(opening) => opening,
            Err(why) => return self.stop(&events, &why).await,
        };
        self.refresh_phase_drawing(drawing, epoch);
        if self.system.is_none() {
            self.system = system;
        }
        let system = self.system.clone().unwrap_or_default();
        self.job = Job::request(&user_text, self.job);
        self.chat_language = judgement::language(&user_text, self.settings.language);
        // Guidance belongs to this job and language; preserve the user's actual requests.
        for message in &mut self.history {
            message.content.retain(|block| {
                !matches!(block, ContentBlock::Text { text }
                if text == crate::prompt::takeoff_workflow(Lang::En)
                    || text == crate::prompt::takeoff_workflow(Lang::Ar))
            });
        }
        self.turn_text = user_text.clone();
        self.provider.begin_turn(&user_text);
        let mut content = vec![ContentBlock::Text { text: state }];
        if std::mem::take(&mut self.resumed) {
            content.push(ContentBlock::Text { text: "[resumed] This conversation continues after a pause; the drawing may have changed: read it before acting.".into() });
        }
        if let Some((png, caption)) = picture {
            let data = png_base64(&png);
            if self.image_bytes + data.len() <= self.image_budget {
                self.image_bytes += data.len();
                content.push(ContentBlock::Text {
                    text: format!("[picture of what the user points at] {caption}"),
                });
                content.push(ContentBlock::Image {
                    media_type: "image/png".into(),
                    data,
                });
            }
        }
        for image in images.into_iter().take(context::MAX_USER_IMAGES) {
            self.image_bytes += image.data.len();
            content.push(ContentBlock::Image {
                media_type: image.media_type,
                data: image.data,
            });
        }
        // A picture alone needs no words; an empty text block would be refused.
        let pictures_only = content
            .iter()
            .any(|b| matches!(b, ContentBlock::Image { .. }))
            && user_text.trim().is_empty();
        if !pictures_only {
            content.push(ContentBlock::Text { text: user_text });
        }
        self.push_user(content);
        self.trim_pictures();
        self.update_checkpoint();
        let tools = catalogue();
        let mut counts = TurnCounts {
            approved: matches!(
                self.turn_text.trim(),
                "Approved, continue." | "موافق، تابع." | "موافق، أكمل."
            ),
            ..TurnCounts::default()
        };
        let mut workflow_sent = false;
        let mut correction_requests = 0;
        let mut usage = Usage::default();
        let mut requests = 0;
        let mut context_retried = false;
        // Grows when an answer spends all its room thinking (see `MAX_TOKENS_CAP`).
        let mut max_tokens = self.settings.max_tokens;
        loop {
            if cancel.is_cancelled() {
                return self.stop(&events, "cancelled").await;
            }
            self.compact();
            crate::history::trim(&mut self.history);
            if self.history.len() > MAX_MESSAGES {
                return self.stop(&events, TOO_LONG).await;
            }
            let mut messages = self.history.clone();
            if self.job == Job::Takeoff && !workflow_sent {
                let workflow = ContentBlock::Text {
                    text: crate::prompt::takeoff_workflow(self.chat_language).into(),
                };
                if let Some(last) = self.history.last_mut() {
                    last.content.push(workflow.clone());
                }
                if let Some(last) = messages.last_mut() {
                    last.content.push(workflow);
                }
                workflow_sent = true;
            }
            let provider_notice = self.provider.budget_notice();
            let near_limit = counts.calls * 5 >= counts.limit() * 4
                || provider_notice
                    .as_ref()
                    .is_some_and(|n| n.contains("[finish now]"));
            let notice = near_limit.then(|| self.job.budget(self.chat_language).to_string());
            if let (Some(last), Some(text)) = (messages.last_mut(), notice) {
                last.content.push(ContentBlock::Text { text });
            }
            let req = TurnRequest {
                system: system.clone(),
                messages,
                tools: tools.clone(),
                model: self.settings.model.clone(),
                effort: self.settings.effort.clone(),
                max_tokens,
            };
            requests += 1;
            let started = Instant::now();
            let Some(turn) = self.stream(req, requests, &events, &cancel).await else {
                // The partial answer is dropped; the next user message joins this turn's.
                return self.stop(&events, "cancelled").await;
            };
            let turn = match turn {
                Ok(turn) => turn,
                Err(e) => {
                    self.log("ERROR", || match &e {
                        AiError::Api { body, request, .. } => {
                            let mut text =
                                format!("{e}\nThe service's answer: {}", crate::log::clip(body));
                            if let Some(request) = request {
                                text.push_str(&format!("\nThe request: {request}"));
                            }
                            text
                        }
                        _ => e.to_string(),
                    });
                    if !self.provider.owns_turn()
                        && e.code() == "context_too_long"
                        && !context_retried
                    {
                        context_retried = true;
                        if self.make_context_room() {
                            self.log("COMPACTED (context limit)", || {
                                "Large results and older pictures shortened; retrying once".into()
                            });
                            self.emit(
                                &events,
                                AgentEvent::Retrying {
                                    seconds: 0,
                                    code: "ai.error.context_retry".into(),
                                    discard_partial: false,
                                },
                            )
                            .await;
                            continue;
                        }
                    }
                    // Provider errors are displayed once by the window, in its language.
                    // A failed drawing request must never produce a take-off end report.
                    self.update_checkpoint();
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
                    self.emit(&events, AgentEvent::Refused { category, text })
                        .await;
                    return Ok(());
                }
                // With no call to answer (all were in a declined attempt before a fallback), an
                // empty user message would be refused, so the turn ends below instead.
                StopReason::ToolUse if has_calls(&turn.content) => {
                    let assistant = Message::assistant(turn.content);
                    self.history.push(assistant.clone());
                    self.history.push(Message::user(Vec::new()));
                    if let Err(why) = self
                        .run_tools(&assistant, &mut counts, &events, &cancel, &mut epoch)
                        .await
                    {
                        return self.stop(&events, &why).await;
                    }
                    self.trim_pictures();
                    if let Some(plan) = counts.awaiting.take() {
                        self.emit(&events, AgentEvent::AwaitingApproval { plan })
                            .await;
                        return Ok(());
                    }
                    if counts.calls >= counts.limit() {
                        let why = format!("stopped after {} steps", counts.limit());
                        return self.stop(&events, &why).await;
                    }
                }
                // Cut off before it said or called anything: its room went on thinking (Gemini
                // through Runware once thought for 94 s and wrote nothing). Nothing ran, so the
                // same request goes again with twice the room; the cut-off answer is dropped.
                StopReason::MaxTokens
                    if !self.provider.owns_turn()
                        && says_nothing(&turn.content)
                        && max_tokens < MAX_TOKENS_CAP =>
                {
                    max_tokens = (max_tokens.max(1) * 2).min(MAX_TOKENS_CAP);
                    self.log("RETRY", || {
                        format!("the answer was cut off before it said anything; asking again with max_tokens {max_tokens}")
                    });
                    self.emit(
                        &events,
                        AgentEvent::Retrying {
                            seconds: 0,
                            code: "ai.error.output_limit".into(),
                            discard_partial: true,
                        },
                    )
                    .await;
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
                            let errors = match self.unfixed_errors(&cancel, epoch).await {
                                Ok(errors) => errors,
                                Err(why) => return self.stop(&events, &why).await,
                            };
                            if !errors.is_empty() && self.settings.autonomy != Autonomy::Autonomous
                            {
                                let plan = Self::error_approval_plan(
                                    self.approval_plan(),
                                    &errors,
                                    self.chat_language,
                                );
                                self.emit(&events, AgentEvent::AwaitingApproval { plan })
                                    .await;
                            } else {
                                if !errors.is_empty() {
                                    if correction_requests < 2 && counts.calls < counts.limit() {
                                        correction_requests += 1;
                                        self.push_user(vec![ContentBlock::Text {text: format!(
                                            "[drawing check] Work is not complete. Fix the new errors and warnings before finishing; read the affected objects and check again. Findings: {}",
                                            errors.iter().cloned().collect::<Vec<_>>().join("; "))}]);
                                        continue;
                                    }
                                    let text = match self.chat_language {
                                        Lang::En => "The drawing still has issues from my changes. I could not finish correcting them; the work is incomplete.",
                                        Lang::Ar => "ما زالت هناك مشكلات نتجت عن تعديلاتي على الرسم. لم أتمكن من تصحيحها كلها؛ العمل غير مكتمل.",
                                    };
                                    self.emit(&events, AgentEvent::Text { text: text.into() })
                                        .await;
                                    return self.stop(&events, "drawing_checks_unresolved").await;
                                }
                                if self.job == Job::Takeoff {
                                    self.report_confidence(&events).await;
                                }
                                self.emit(&events, AgentEvent::Done { usage }).await;
                            }
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
        // Tool results name an unrun call; the turn event keeps its existing cancellation code.
        let reason = if reason == CANCELLED {
            "cancelled"
        } else {
            reason
        };
        self.log("STOPPED", || reason.to_string());
        if reason.contains("steps") || reason.contains("budget") || reason.contains("limit") {
            self.report_interrupted(events, reason).await;
        }
        self.emit(
            events,
            AgentEvent::Stopped {
                reason: reason.to_string(),
            },
        )
        .await;
        Ok(())
    }

    async fn report_confidence(&self, events: &mpsc::Sender<AgentEvent>) {
        let session = self.session.clone();
        let report = tokio::task::spawn_blocking(move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            guard.as_ref()?.measurement_report().ok()
        })
        .await
        .ok()
        .flatten();
        if let Some(report) = report.filter(|r| !r.items.is_empty()) {
            let summary = &report.confidence_summary;
            let items = report
                .items
                .iter()
                .map(|i| format!("{}: {:.0}%", i.code, i.confidence * 100.))
                .collect::<Vec<_>>()
                .join("; ");
            let text = if self.chat_language == Lang::Ar {
                format!("\nالثقة في الكميات، من الأقل: {items}. منخفضة: {}، متوسطة: {}، عالية: {}. المراجعة اختيارية.", summary.low, summary.medium, summary.high)
            } else {
                format!("\nConfidence report (lowest first): {items}. Bands: low {}, medium {}, high {}. Review is optional.", summary.low, summary.medium, summary.high)
            };
            self.emit(events, AgentEvent::Text { text }).await;
        }
    }

    /// A hard cap may leave no paid request for a final answer. Report the persisted state
    /// locally; never fabricate measurements or discard an already generated BOQ.
    async fn report_interrupted(&self, events: &mpsc::Sender<AgentEvent>, reason: &str) {
        if self.job != Job::Takeoff {
            return;
        }
        self.report_confidence(events).await;
        let session = self.session.clone();
        let arabic = self.chat_language == Lang::Ar;
        let report = tokio::task::spawn_blocking(move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let s = guard.as_ref()?;
            let report = s.measurement_report().ok()?;
            let recognised = s.recognition_evidence().ok()?.iter()
                .filter(|c| c.state == qiyas_core::recognition::ReviewState::Accepted).count();
            Some(if arabic {
                format!("احتُفظ بـ {} بند كميات من {recognised} عنصر مقبول. توجد {} ملاحظة للمراجعة. النطاق غير المرتبط أو غير المؤكد لم يكتمل؛ لم تُختلق كميات.", report.items.len(), report.warnings.len())
            } else {
                format!("{} generated BOQ items retained from {recognised} accepted recognised elements. {} Unmapped or uncertain scope remains incomplete; no quantities were invented.", report.items.len(), report.warnings.join("; "))
            })
        }).await.ok().flatten().unwrap_or_else(|| if arabic {
            "الحصر غير مكتمل؛ لم يُرسل طلب آخر إلى خدمة المساعد.".into()
        } else {
            "The take-off is incomplete; no further provider request was made.".into()
        });
        self.emit(
            events,
            AgentEvent::Text {
                text: if arabic {
                    format!("\nملخص العمل المتوقف: {report}")
                } else {
                    format!("\nEnd report: {reason}. {report}")
                },
            },
        )
        .await;
    }

    /// Streams one turn, forwarding text and thinking as they arrive. `None` when cancelled:
    /// the HTTP request is dropped mid-stream.
    async fn stream(
        &self,
        req: TurnRequest,
        request_number: usize,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) -> Option<Result<crate::provider::TurnResult, AiError>> {
        let request_summary = format!(
            "{} messages, {} tools, model {}, effort {}",
            req.messages.len(),
            req.tools.len(),
            req.model,
            req.effort
        );
        let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
        let turn = async move {
            let mut sink = move |e: StreamEvent| {
                let _ = tx.send(e);
            };
            // `sink` (and the sender in it) is dropped when the turn ends, which ends `forward`.
            self.provider.stream_turn(req, &mut sink).await
        };
        let forward = async {
            let mut reported = false;
            let mut partial = String::new();
            let mut streamed = false;
            while let Some(e) = rx.recv().await {
                let event = match e {
                    StreamEvent::Activity(event) => {
                        // Local providers own their tool loop; these calls never pass through
                        // run_tools. Log each received activity while it is still running.
                        match event.as_ref() {
                            AgentEvent::ToolCall {
                                id, name, input, ..
                            } => self.log("TOOL CALL", || {
                                format!("{id}; {name}\n{}", clip(&input.to_string()))
                            }),
                            AgentEvent::ToolResult {
                                id,
                                summary,
                                is_error,
                                ..
                            } => self.log("TOOL RESULT", || {
                                format!("{id}; error: {is_error}\n{}", clip(summary))
                            }),
                            AgentEvent::ProviderStatus { code } => {
                                self.log("PROVIDER STATUS", || code.clone())
                            }
                            AgentEvent::Plan { phases } => self.log("PLAN", || {
                                clip(&serde_json::to_string(phases).unwrap_or_default())
                            }),
                            _ => {}
                        }
                        *event
                    }
                    StreamEvent::Diagnostic { kind, text } => {
                        self.log(&kind, || clip(&text));
                        continue;
                    }
                    StreamEvent::RequestSent(fields) => {
                        reported = true;
                        self.log(&format!("REQUEST {request_number}; sent: {fields}"), || {
                            request_summary.clone()
                        });
                        continue;
                    }
                    StreamEvent::Retrying {
                        seconds,
                        code,
                        diagnostic,
                        discard_partial,
                    } => {
                        self.log("RETRY", || {
                            format!(
                                "{code}; {diagnostic}; waiting {seconds} s; partial answer: {}",
                                crate::log::clip(&partial)
                            )
                        });
                        let discard_partial = discard_partial && streamed;
                        partial.clear();
                        streamed = false;
                        AgentEvent::Retrying {
                            seconds,
                            code,
                            discard_partial,
                        }
                    }
                    StreamEvent::TextDelta(text) => {
                        if self.provider.owns_turn() {
                            self.log("TEXT", || clip(&text));
                        }
                        streamed = true;
                        if partial.len() < crate::log::MAX_TEXT {
                            partial.push_str(&text);
                        }
                        AgentEvent::Text { text }
                    }
                    StreamEvent::Thinking(text) => {
                        streamed = true;
                        AgentEvent::Thinking { text }
                    }
                    _ => continue,
                };
                self.emit(events, event).await;
            }
            if !reported {
                self.log(
                    &format!("REQUEST {request_number}; sent: not reported by provider"),
                    || request_summary.clone(),
                );
            }
        };
        let result = tokio::select! {
            (result, ()) = async { tokio::join!(turn, forward) } => Some(result),
            () = cancel.cancelled() => None,
        };
        self.update_checkpoint();
        result
    }

    /// Runs the tool calls of `assistant` in order and returns one result block for each, all
    /// for one user message. Once `cancel` fires, past `MAX_TOOL_CALLS`, and past
    /// `MAX_DESTRUCTIVE` destructive commands, the calls are answered but not run. `epoch` is
    /// the session's at the start of the turn (see `run_tool`), moved on when a file tool of the
    /// turn put another drawing on screen.
    async fn run_tools(
        &mut self,
        assistant: &Message,
        counts: &mut TurnCounts,
        events: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
        epoch: &mut Option<u64>,
    ) -> Result<(), String> {
        let mut stopped = None;
        for (id, name, input) in assistant.tool_uses() {
            self.emit(
                events,
                AgentEvent::ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    input: input.clone(),
                    label: surface::label(name, input),
                    mutating: surface::mutating(name, input),
                    activity: Some(crate::activity::describe(name, input)),
                },
            )
            .await;
            let started = Instant::now();
            // Every answered call counts, bad input included.
            let fingerprint = edit_fingerprint(name, input);
            let repeated = fingerprint
                .as_ref()
                .is_some_and(|key| counts.ineffective.get(key).copied().unwrap_or(0) >= 2);
            let (mut block, mut event) = if let Some(why) = stopped.as_deref() {
                (error_result(id, why), error_event(id, why))
            } else if repeated {
                let why = match self.chat_language {
                    Lang::En => "I stopped repeating this edit because two attempts made no change. The requested change is still incomplete.",
                    Lang::Ar => "توقفت عن تكرار هذا التعديل لأن محاولتين لم تغيّرا شيئًا. التعديل المطلوب لم يكتمل.",
                };
                stopped = Some("repeated_ineffective_edit".into());
                self.emit(events, AgentEvent::Text { text: why.into() })
                    .await;
                (error_result(id, why), error_event(id, why))
            } else if counts.awaiting.is_some() {
                let why = "Not run: waiting for the user's approval.";
                (error_result(id, why), error_event(id, why))
            } else if cancel.is_cancelled() {
                (error_result(id, CANCELLED), error_event(id, CANCELLED))
            } else if counts.calls >= counts.limit() {
                let why = "Not run: the limit of tool calls for this message was reached.";
                (error_result(id, why), error_event(id, why))
            } else {
                counts.calls += 1;
                if !input.is_object() {
                    (
                        error_result(id, INVALID_JSON),
                        error_event(id, INVALID_JSON),
                    )
                } else if let Some(why) = counts.over_cap(name, input) {
                    (error_result(id, why), error_event(id, why))
                } else {
                    let op = input[if name == surface::CAPTURE {
                        "source"
                    } else {
                        "op"
                    }]
                    .as_str()
                    .unwrap_or_default();
                    let routed = surface::route(name, input).and_then(|route| {
                        // The drawing need is checked where the session is locked, off this
                        // thread (`run_tool`): locking here would stall the turn's
                        // events behind a long command.
                        surface::check_need(
                            name,
                            op,
                            true,
                            self.host
                                .as_deref()
                                .is_some_and(crate::DrawingHost::has_window),
                            self.host.is_some(),
                        )
                        .map_err(|error| error.json().to_string())?;
                        Ok(route)
                    });
                    match routed {
                        Err(why) => (error_result(id, &why), error_event(id, &why)),
                        Ok(Route::Window { tool, input }) => {
                            self.run_window_tool(id, tool, &input, cancel, *epoch).await
                        }
                        Ok(Route::Host { tool, input }) => {
                            let before = *epoch;
                            let ran = self.run_host_tool(id, tool, &input, cancel, *epoch).await;
                            if changes_drawing(tool) && !is_error(&ran.0) {
                                *epoch = self.phase_epoch;
                                counts.changed_drawing |= before != *epoch;
                            }
                            ran
                        }
                        Ok(Route::Session { tool, op, args }) => {
                            let (block, event, changed) = self
                                .run_tool(
                                    id,
                                    (&tool, &op),
                                    &args,
                                    cancel,
                                    *epoch,
                                    surface::mutating(name, input),
                                )
                                .await;
                            counts.changed_drawing |= changed;
                            (block, event)
                        }
                    }
                }
            };
            if let Some(key) = fingerprint {
                if ineffective(&block) {
                    *counts.ineffective.entry(key).or_default() += 1;
                } else {
                    counts.ineffective.clear();
                }
            }
            if !is_error(&block) && Job::observes(name, input) {
                self.job = Job::Takeoff;
            }
            let left = counts.limit().saturating_sub(counts.calls);
            if left <= 10 {
                add_budget(&mut block, left);
            }
            self.log(
                &format!("TOOL {name} ({} ms)", started.elapsed().as_millis()),
                || tool_entry(id, input, &block),
            );
            if name == surface::EXECUTE && input["op"] == "plan" && !is_error(&block) {
                if let Ok(plan) = surface::args_of(input)
                    .map_err(ToolError::BadInput)
                    .and_then(|args| surface::read_plan(&args))
                {
                    let completed = plan.phases.iter().any(|phase| {
                        phase.status == surface::PlanStatus::Done
                            && !self.plan.as_ref().is_some_and(|old| {
                                old.phases.iter().any(|p| {
                                    p.id == phase.id && p.status == surface::PlanStatus::Done
                                })
                            })
                    });
                    let switched = self.plan.as_ref().and_then(active_phase) != active_phase(&plan);
                    let errors = if completed || switched {
                        match self.unfixed_errors(cancel, *epoch).await {
                            Ok(errors) => errors,
                            Err(why) => {
                                // Finish answering this batch so every tool use still has a result.
                                stopped = Some(why);
                                BTreeSet::new()
                            }
                        }
                    } else {
                        BTreeSet::new()
                    };
                    if !errors.is_empty() && self.settings.autonomy == Autonomy::Autonomous {
                        let why = format!("Fix the new drawing errors and warnings before completing this phase: {}", errors.iter().cloned().collect::<Vec<_>>().join("; "));
                        block = error_result(id, &why);
                        event = error_event(id, &why);
                        if let Some(last) = self.history.last_mut() {
                            last.content.push(block);
                        }
                        self.emit(events, event).await;
                        continue;
                    }
                    let first = !counts.plan_seen;
                    counts.plan_seen = true;
                    if stopped.is_some() {
                        counts.awaiting = None;
                    } else if !errors.is_empty() && self.settings.autonomy != Autonomy::Autonomous {
                        counts.awaiting = Some(Self::error_approval_plan(
                            plan.clone(),
                            &errors,
                            self.chat_language,
                        ));
                    } else if self.settings.autonomy != Autonomy::Autonomous
                        && ((first && plan.phases.len() >= 2 && !counts.approved)
                            || (self.settings.autonomy == Autonomy::PerPhase && completed))
                    {
                        counts.awaiting = Some(plan.clone());
                    }
                    if stopped.is_none() && (completed || switched) && errors.is_empty() {
                        self.phase_state.baseline_errors = None;
                        self.phase_state.touched.clear();
                    }
                    self.plan = Some(plan.clone());
                    self.emit(
                        events,
                        AgentEvent::Plan {
                            phases: plan.phases,
                        },
                    )
                    .await;
                }
            }
            if let Some(last) = self.history.last_mut() {
                last.content.push(block);
            }
            self.emit(events, event).await;
        }
        stopped.map_or(Ok(()), Err)
    }

    /// Check on a blocking worker, against the same drawing, once per phase boundary or end.
    /// The returned set also supplies the approval text without a second drawing check.
    async fn unfixed_errors(
        &mut self,
        cancel: &CancellationToken,
        epoch: Option<u64>,
    ) -> Result<BTreeSet<String>, String> {
        if self.phase_state.touched.is_empty() {
            return Ok(BTreeSet::new());
        }
        let Some(baseline) = self.phase_state.baseline_errors.clone() else {
            return Ok(BTreeSet::new());
        };
        let handles = self.phase_state.touched.iter().copied().collect::<Vec<_>>();
        let session = self.session.clone();
        let cancel = cancel.clone();
        let (drawing, errors) = tokio::task::spawn_blocking(move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            check_turn(guard.as_ref(), &cancel, epoch)?;
            let drawing = guard.as_ref().map(|s| s.drawing.path.clone());
            let errors = guard.as_ref().map_or_else(BTreeSet::new, |s| {
                drawing_errors(s, &handles)
                    .difference(&baseline)
                    .cloned()
                    .collect()
            });
            check_turn(guard.as_ref(), &cancel, epoch)?;
            Ok::<_, String>((drawing, errors))
        })
        .await
        .map_err(|e| format!("The drawing check crashed: {e}"))??;
        self.refresh_phase_drawing(drawing, epoch);
        Ok(errors)
    }

    fn error_approval_plan(
        mut plan: surface::Plan,
        errors: &BTreeSet<String>,
        lang: Lang,
    ) -> surface::Plan {
        for phase in &mut plan.phases {
            if phase.status == surface::PlanStatus::Active {
                phase.status = surface::PlanStatus::Pending;
            }
        }
        plan.phases.push(surface::PlanPhase {
            id: "drawing-check-review".into(),
            title: if lang == Lang::Ar {
                format!("تصحيح مشكلات الرسم ({})", errors.len())
            } else {
                format!("Fix drawing issues ({})", errors.len())
            },
            status: surface::PlanStatus::Active,
        });
        plan
    }

    /// Apply metadata captured off-thread. Resume validates by path once; Save As keeps the
    /// epoch, while an ordinary tab switch starts fresh error attribution.
    fn refresh_phase_drawing(&mut self, drawing: Option<PathBuf>, epoch: Option<u64>) {
        let same_drawing = if self.resumed {
            self.phase_state.drawing == drawing
        } else {
            self.phase_epoch == epoch
        };
        if !same_drawing {
            self.phase_state = PhaseState::default();
        }
        self.phase_state.drawing = drawing;
        self.phase_epoch = epoch;
    }

    fn approval_plan(&self) -> surface::Plan {
        self.plan.clone().unwrap_or_else(|| surface::Plan {
            phases: vec![surface::PlanPhase {
                id: "drawing-check".into(),
                title: if self.chat_language == Lang::Ar {
                    "تصحيح مشكلات الرسم"
                } else {
                    "Fix drawing issues"
                }
                .into(),
                status: surface::PlanStatus::Active,
            }],
        })
    }

    /// Runs one tool off the async workers (a picture takes a while to draw), holding the
    /// session's lock only while it runs. A picture past the conversation's budget is refused.
    /// Once it has the lock, the call is not run when the turn was cancelled meanwhile, or when
    /// the session is no longer the one of the turn (`epoch`): a tab switch swaps the session and
    /// cancels the turn while the call waits for the lock.
    async fn run_tool(
        &mut self,
        id: &str,
        operation: (&str, &str),
        input: &Value,
        cancel: &CancellationToken,
        epoch: Option<u64>,
        mutating: bool,
    ) -> (ContentBlock, AgentEvent, bool) {
        let (name, op) = operation;
        if name == surface::CAPTURE && self.image_bytes >= self.image_budget {
            return (
                error_result(id, NO_IMAGE_BUDGET),
                error_event(id, NO_IMAGE_BUDGET),
                false,
            );
        }
        let session = self.session.clone();
        let (tool, op, args) = (name.to_string(), op.to_string(), input.clone());
        let method = self.settings.measurement_method.clone();
        let cancel = cancel.clone();
        let mut phase = self.phase_state.clone();
        let ran = tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            check_turn(guard.as_ref(), &cancel, epoch)?;
            phase.drawing = guard.as_ref().map(|s| s.drawing.path.clone());
            if guard.is_none() {
                if let Some(output) = surface::run_without_drawing(&tool, &op, &args, None) {
                    return Ok((output, false, phase));
                }
            }
            let s = guard
                .as_mut()
                .ok_or("No drawing is open. Ask the user to open one.")?;
            // Baseline and mutation share this critical section: no tab switch or other edit
            // can slip between them, and rejected/cancelled calls never establish a baseline.
            if mutating && phase.baseline_errors.is_none() {
                phase.baseline_errors = Some(drawing_errors(s, &[]));
            }
            check_turn(Some(s), &cancel, epoch)?;
            let before = s.document_revision();
            let output =
                surface::run_session_with_method(s, &tool, &op, &args, Source::Ai, &method);
            let changed = before != s.document_revision();
            if mutating {
                if let Ok(ToolOutput::Json(value)) = &output {
                    for field in ["added", "changed", "removed"] {
                        phase.touched.extend(
                            value[field]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(Value::as_u64),
                        );
                    }
                }
            }
            Ok::<_, String>((output, changed, phase))
        })
        .await;
        let (result, changed) = match ran {
            Ok(Ok((output, changed, phase))) => {
                self.phase_state = phase;
                self.phase_epoch = epoch;
                (
                    self.output_result(id, output.map_err(|e| e.json().to_string())),
                    changed,
                )
            }
            Ok(Err(why)) => ((error_result(id, &why), error_event(id, &why)), false),
            Err(e) => {
                let why = format!("The tool {name} crashed: {e}");
                ((error_result(id, &why), error_event(id, &why)), false)
            }
        };
        (result.0, result.1, changed)
    }

    /// Runs a window tool (see `crate::window`) off the async workers: the window first, then
    /// the session.
    async fn run_window_tool(
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        cancel: &CancellationToken,
        epoch: Option<u64>,
    ) -> (ContentBlock, AgentEvent) {
        if name == "view_screen" && self.image_bytes >= self.image_budget {
            return (
                error_result(id, NO_IMAGE_BUDGET),
                error_event(id, NO_IMAGE_BUDGET),
            );
        }
        let Some(host) = self.host.clone() else {
            return (error_result(id, NO_WINDOW), error_event(id, NO_WINDOW));
        };
        let session = self.session.clone();
        let (tool, args, lang) = (name.to_string(), input.clone(), self.settings.language);
        let cancel = cancel.clone();
        let ran = tokio::task::spawn_blocking(move || {
            {
                let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
                check_turn(guard.as_ref(), &cancel, epoch)?;
            }
            // Window requests must run outside the session lock: rendering takes it too.
            let output = run_window(&*host, &session, &tool, &args, lang)?;
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            check_turn(guard.as_ref(), &cancel, epoch)?;
            Ok::<_, String>((output, guard.as_ref().map(|s| s.drawing.path.clone())))
        })
        .await;
        match ran {
            Ok(Ok((output, drawing))) => {
                self.refresh_phase_drawing(drawing, epoch);
                self.output_result(id, Ok(output))
            }
            Ok(Err(why)) => (error_result(id, &why), error_event(id, &why)),
            Err(e) => {
                let why = format!("The tool {name} crashed: {e}");
                (error_result(id, &why), error_event(id, &why))
            }
        }
    }

    /// A tool's output as the block for the model and the event for the window; a picture past
    /// the conversation's budget is refused.
    fn output_result(
        &mut self,
        id: &str,
        output: Result<ToolOutput, String>,
    ) -> (ContentBlock, AgentEvent) {
        match output {
            Err(why) => (error_result(id, &why), error_event(id, &why)),
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
                    count: None,
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
        &mut self,
        id: &str,
        name: &str,
        input: &Value,
        cancel: &CancellationToken,
        epoch: Option<u64>,
    ) -> (ContentBlock, AgentEvent) {
        let Some(host) = self.host.clone() else {
            return (error_result(id, NO_HOST), error_event(id, NO_HOST));
        };
        let (tool, args, text) = (name.to_string(), input.clone(), self.turn_text.clone());
        let session = self.session.clone();
        let cancel = cancel.clone();
        let ran = tokio::task::spawn_blocking(move || {
            {
                let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
                check_turn(guard.as_ref(), &cancel, epoch)?;
            }
            // The host takes tabs before session. Never hold session while calling it.
            let value = run_host(&*host, &tool, &args, Some(&text))?;
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let next_epoch = guard.as_ref().map(Session::epoch);
            check_turn(
                guard.as_ref(),
                &cancel,
                if changes_drawing(&tool) {
                    next_epoch
                } else {
                    epoch
                },
            )?;
            let drawing = guard.as_ref().map(|s| s.drawing.path.clone());
            Ok::<_, String>((value, drawing, next_epoch))
        })
        .await;
        match ran {
            Ok(Ok((value, drawing, epoch))) => {
                self.refresh_phase_drawing(drawing, epoch);
                json_result(id, &value)
            }
            Ok(Err(why)) => (error_result(id, &why), error_event(id, &why)),
            Err(e) => {
                let why = format!("The tool {name} crashed: {e}");
                (error_result(id, &why), error_event(id, &why))
            }
        }
    }

    /// Capture prompt facts, live state, picture and metadata under one off-thread lock.
    /// Checkpoints only use the returned values and never wait for the drawing themselves.
    async fn opening(
        &self,
        pointed: TurnContext,
        cancel: &CancellationToken,
    ) -> Result<Opening, String> {
        let session = self.session.clone();
        let settings = self.settings.clone();
        let build_system = self.system.is_none();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            let guard = session.lock().unwrap_or_else(PoisonError::into_inner);
            let epoch = guard.as_ref().map(Session::epoch);
            check_turn(guard.as_ref(), &cancel, epoch)?;
            let lang = settings.language;
            let system = build_system.then(|| {
                let facts = guard
                    .as_ref()
                    .map_or_else(DrawingFacts::none, DrawingFacts::of);
                system_prompt(lang, &facts)
            });
            let state = guard.as_ref().map_or_else(LiveState::none, LiveState::of);
            let mut text = state_block(lang, &state);
            let method = guard.as_ref().map_or_else(
                || Ok(settings.measurement_method.clone()),
                |session| MeasurementMethod::for_session(session, &settings.measurement_method),
            ).map_err(|e| e.to_string())?;
            text.push_str(&format!(
                "Measurement method: {}; rules loaded: {}.\nApproval mode: {}.\n",
                method.name
                    .as_deref()
                    .unwrap_or("None (geometric quantities)"),
                method.rules_loaded,
                serde_json::to_value(settings.autonomy)
                    .unwrap_or_default()
                    .as_str()
                    .unwrap_or("autonomous")
            ));
            text.push_str(method.instruction());
            text.push('\n');
            let mut picture = None;
            if let Some(session) = guard.as_ref() {
                if let Some(working) = &session.automation_sheet {
                    text.push_str(&format!("Automation working sheet: {working}. Window sheet: {}. User browsing does not change the working sheet.\n", session.sheet));
                }
                text.push_str(&context::describe(session, &pointed, lang));
                picture = context::picture(session, &pointed);
            }
            check_turn(guard.as_ref(), &cancel, epoch)?;
            Ok(Opening {
                state: text,
                epoch,
                drawing: guard.as_ref().map(|s| s.drawing.path.clone()),
                system,
                picture,
            })
        })
        .await
        .map_err(|e| format!("The drawing state could not be read: {e}"))?
    }

    /// Replaces the oldest pictures in the history with `PICTURE_REMOVED` once it holds more
    /// than `MAX_PICTURES` of them or `MAX_PICTURE_BYTES`, keeping the newest `KEEP_PICTURES`
    /// within `KEEP_PICTURE_BYTES` (the newest always).
    fn trim_pictures(&mut self) {
        // Where each picture is, oldest first: message, block, part of a tool result, bytes.
        let mut found: Vec<(usize, usize, Option<usize>, usize)> = Vec::new();
        for (m, message) in self.history.iter().enumerate() {
            for (b, block) in message.content.iter().enumerate() {
                match block {
                    ContentBlock::Image { data, .. } => found.push((m, b, None, data.len())),
                    ContentBlock::ToolResult { content, .. } => {
                        for (p, part) in content.iter().enumerate() {
                            if let ResultPart::Png(data) = part {
                                found.push((m, b, Some(p), data.len()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let bytes: usize = found.iter().map(|f| f.3).sum();
        if found.len() <= MAX_PICTURES && bytes <= MAX_PICTURE_BYTES {
            return;
        }
        let (mut kept, mut kept_bytes, mut cut) = (0, 0, found.len());
        for (k, f) in found.iter().enumerate().rev() {
            let first = kept == 0;
            if !first && (kept + 1 > KEEP_PICTURES || kept_bytes + f.3 > KEEP_PICTURE_BYTES) {
                break;
            }
            kept += 1;
            kept_bytes += f.3;
            cut = k;
        }
        for &(m, b, part, _) in &found[..cut] {
            let block = &mut self.history[m].content[b];
            match (block, part) {
                (ContentBlock::ToolResult { content, .. }, Some(p)) => {
                    content[p] = ResultPart::Text(PICTURE_REMOVED.to_string());
                }
                (block, None) => {
                    *block = ContentBlock::Text {
                        text: PICTURE_REMOVED.to_string(),
                    };
                }
                _ => {}
            }
        }
        self.log("PICTURES", || {
            format!("{cut} older pictures removed from the conversation, {kept} kept")
        });
    }

    /// A context refusal gets one smaller request. User words, tool ids and result pairing
    /// stay intact. The ordinary history policy belongs to the conversation workflow.
    fn make_context_room(&mut self) -> bool {
        let mut changed = false;
        for message in &mut self.history {
            for block in &mut message.content {
                match block {
                    ContentBlock::ToolResult { content, .. } => {
                        for part in content {
                            match part {
                                ResultPart::Png(_) => {
                                    *part = ResultPart::Text(
                                        "[Picture removed to make room. Capture again if needed.]"
                                            .into(),
                                    );
                                    changed = true;
                                }
                                ResultPart::Text(text) if text.len() > 1000 => {
                                    *text = format!("{} [Result shortened to make room. Read the drawing for full details.]", text.chars().take(400).collect::<String>());
                                    changed = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    ContentBlock::Image { .. } => {
                        *block = ContentBlock::Text {
                            text: "[Picture removed to make room. Ask for it again if needed.]"
                                .into(),
                        };
                        changed = true;
                    }
                    _ => {}
                }
            }
        }
        changed
    }

    /// Keep the newest 160 messages and complete call/result pairs. Older requests survive
    /// word for word in a memory note, alongside outcomes and discovered command contracts.
    fn compact(&mut self) {
        if self.history.len() <= COMPACT_AT {
            return;
        }
        let before = self.history.len();
        let contracts = crate::history::contracts(&self.history);
        let (mut requests, mut steps) = take_note(&mut self.history);
        let mut cut = self.history.len().saturating_sub(COMPACT_TO);
        // Never orphan tool results or cut the tail below the promised recent window.
        while cut > 0
            && (self.history[cut].role != Role::Assistant
                || self.history[cut - 1].role != Role::User)
        {
            cut -= 1;
        }
        if cut == 0 {
            return;
        }
        for message in &self.history[..cut] {
            requests.extend(user_words(message));
        }
        steps.extend(step_lines(&self.history[..cut]));
        self.history.drain(..cut);
        let mut note = vec![ContentBlock::Text {
            text: compaction_note(&requests, &steps),
        }];
        if !contracts.is_empty() {
            note.push(ContentBlock::Text {
                text: format!(
                    "[known contracts]\n{}",
                    serde_json::to_string(&contracts).unwrap()
                ),
            });
        }
        self.history.insert(0, Message::user(note));
        self.log("COMPACTED", || {
            format!(
                "{before} messages became {}; {} steps noted",
                self.history.len(),
                steps.len()
            )
        });
    }

    /// Adds user content. A user message already last (after a cancel, an error or a refusal,
    /// or tool results at the step limit) is the latest turn, not yet answered; the new content
    /// goes after what it holds, so tool results stay first and roles keep alternating.
    fn push_user(&mut self, content: Vec<ContentBlock>) {
        match self.history.last_mut() {
            Some(last) if last.role == Role::User && !self.provider.owns_turn() => {
                last.content.extend(content)
            }
            _ => self.history.push(Message::user(content)),
        }
    }
}

pub(crate) fn edit_fingerprint(name: &str, input: &Value) -> Option<String> {
    if !surface::mutating(name, input) {
        return None;
    }
    if crate::activity::describe(name, input).effect != "edit" {
        return None;
    }
    let args = surface::args_of(input).ok()?;
    let op = input["op"].as_str()?;
    let mut command = match op {
        "run" => match &args["command"] {
            Value::String(text) => serde_json::from_str(text).ok()?,
            other => other.clone(),
        },
        "batch" | "set_records" | "create_entities" => {
            let mut c = args;
            c["type"] = serde_json::json!(op);
            c
        }
        _ => serde_json::json!({"op":op,"args":args}),
    };
    qiyas_core::registry::canonicalize(&mut command);
    Some(command.to_string())
}

fn ineffective(block: &ContentBlock) -> bool {
    if is_error(block) {
        return true;
    }
    let ContentBlock::ToolResult { content, .. } = block else {
        return false;
    };
    content.iter().any(|part| match part {
        ResultPart::Text(text) => {
            serde_json::from_str::<Value>(text).is_ok_and(|v| v["no_change"] == true)
        }
        _ => false,
    })
}

/// A tool's JSON answer: the block for the model and the event for the window.
fn json_result(id: &str, value: &Value) -> (ContentBlock, AgentEvent) {
    let event = AgentEvent::ToolResult {
        id: id.to_string(),
        summary: summary(value),
        count: crate::activity::count(value),
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

/// Remind the model of the calls left, including in errors and picture captions.
fn add_budget(block: &mut ContentBlock, left: usize) {
    let ContentBlock::ToolResult {
        content, is_error, ..
    } = block
    else {
        return;
    };
    let budget = format!("{left} calls left");
    if let Some(ResultPart::Text(text)) = content.first_mut() {
        let mut value = serde_json::from_str::<Value>(text).unwrap_or_else(|_| {
            if *is_error {
                serde_json::json!({ "error": text })
            } else {
                serde_json::json!({ "caption": text })
            }
        });
        if !value.is_object() {
            value = serde_json::json!({ "result": value });
        }
        value["budget"] = Value::String(budget);
        *text = value.to_string();
    }
}

/// Whether a result block is an error.
fn is_error(block: &ContentBlock) -> bool {
    matches!(block, ContentBlock::ToolResult { is_error: true, .. })
}

/// Whether `content` holds a tool call to answer.
/// The most room one answer gets when it ran out thinking: Gemini's output limit (65,536)
/// with a margin; Claude's is higher.
pub const MAX_TOKENS_CAP: u32 = 64_000;

/// Whether an answer holds no text and no tool call (thinking at most).
fn says_nothing(content: &[ContentBlock]) -> bool {
    content.iter().all(|b| match b {
        ContentBlock::Text { text } => text.trim().is_empty(),
        ContentBlock::ToolUse { .. } => false,
        _ => true,
    })
}

/// Every user request and correction, without clipping generated state or picture captions in.
fn user_words(message: &Message) -> Vec<String> {
    if message.role != Role::User {
        return Vec::new();
    }
    message
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text }
                if !text.starts_with("[state]")
                    && !text.starts_with("[resumed]")
                    && !text.starts_with("[picture of")
                    && !text.starts_with("[known contracts]")
                    && !text.starts_with("[drawing check]")
                    && !text.starts_with(COMPACTED)
                    && !text.trim().is_empty()
                    && text != crate::prompt::TAKEOFF_WORKFLOW
                    && text != crate::prompt::takeoff_workflow(Lang::Ar) =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// One line per call in an assistant message: the tool, its operation and the command type.
fn step_lines(messages: &[Message]) -> Vec<String> {
    let results: BTreeMap<_, _> = messages
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|b| match b {
            ContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => Some((tool_use_id.as_str(), *is_error)),
            _ => None,
        })
        .collect();
    messages
        .iter()
        .flat_map(Message::tool_uses)
        .map(|(id, name, input)| {
            let args = surface::args_of(input).unwrap_or_default();
            let what = args["command"]["type"]
                .as_str()
                .or_else(|| args["commands"].as_array().map(|_| "several commands"))
                .or_else(|| args["name"].as_str())
                .unwrap_or("");
            let op = input["op"]
                .as_str()
                .or(input["source"].as_str())
                .unwrap_or("");
            let status = match results.get(id) {
                Some(false) => "returned successfully; read back to verify",
                Some(true) => "failed",
                None => "not completed",
            };
            format!("{name} {op} {what}: {status}")
        })
        .collect()
}

/// Takes an earlier compaction's note off the first message: its requests and steps.
fn take_note(history: &mut [Message]) -> (Vec<String>, Vec<String>) {
    let Some(first) = history.first_mut() else {
        return (Vec::new(), Vec::new());
    };
    let Some(ContentBlock::Text { text }) = first.content.first() else {
        return (Vec::new(), Vec::new());
    };
    if !text.starts_with(COMPACTED) {
        return (Vec::new(), Vec::new());
    }
    let (mut requests, mut steps) = (Vec::new(), Vec::new());
    let mut in_steps = false;
    for line in text.lines().skip(1) {
        if matches!(line, "Steps and outcomes:" | "Steps already done:") {
            in_steps = true;
        } else if let Some(r) = line.strip_prefix("Request: ") {
            requests.push(serde_json::from_str(r).unwrap_or_else(|_| r.replace("\\n", "\n")));
        } else if let Some(step) = line.strip_prefix("- ") {
            if in_steps {
                steps.push(step.to_string());
            }
        }
    }
    first.content.remove(0);
    (requests, steps)
}

/// The note in place of what compaction dropped. At most 80 step lines: older ones are
/// counted.
fn compaction_note(requests: &[String], steps: &[String]) -> String {
    let mut note = format!(
        "{COMPACTED}\nThe start of this conversation was removed to make room. The drawing \
         holds everything done so far: read it (qiyas_read state, records, wall_groups, check) \
         instead of relying on memory, and carry on with the user's latest request.\n"
    );
    for r in requests {
        note.push_str(&format!("Request: {}\n", serde_json::to_string(r).unwrap()));
    }
    note.push_str("Steps and outcomes:\n");
    let skip = steps.len().saturating_sub(80);
    if skip > 0 {
        note.push_str(&format!("- ({skip} earlier steps)\n"));
    }
    for step in &steps[skip..] {
        note.push_str(&format!("- {step}\n"));
    }
    note
}

fn has_calls(content: &[ContentBlock]) -> bool {
    Message::assistant(content.to_vec())
        .tool_uses()
        .next()
        .is_some()
}

fn active_phase(plan: &surface::Plan) -> Option<&str> {
    plan.phases
        .iter()
        .find(|p| p.status == surface::PlanStatus::Active)
        .map(|p| p.id.as_str())
}

/// Called only with the session locked on a blocking worker, before reading or changing it.
fn check_turn(
    session: Option<&Session>,
    cancel: &CancellationToken,
    epoch: Option<u64>,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        Err(CANCELLED.into())
    } else if session.map(Session::epoch) != epoch {
        Err(DRAWING_CHANGED.into())
    } else {
        Ok(())
    }
}

/// Stable error identities allow a phase to repair its own changes before asking for review.
fn drawing_errors(session: &Session, handles: &[u64]) -> BTreeSet<String> {
    let report = session.check_drawing_all(&qiyas_core::check::CheckScope::default());
    report
        .findings
        .into_iter()
        .filter(|f| handles.is_empty() || f.handles.iter().any(|h| handles.contains(h)))
        .filter(|f| {
            matches!(
                f.severity,
                qiyas_core::check::Severity::Error | qiyas_core::check::Severity::Warning
            )
        })
        .map(|mut f| {
            f.handles.sort_unstable();
            format!("{}:{}:{:?}", report.sheet, f.rule, f.handles)
        })
        .collect()
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
        "tokens: {} in, {} out ({} thinking), {} cache read, {} cache written",
        usage.input_tokens,
        usage.output_tokens,
        usage.reasoning_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens
    )
}

fn error_result(id: &str, why: &str) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: id.to_string(),
        content: vec![ResultPart::Text(crate::errors::json_error(why).to_string())],
        is_error: true,
    }
}

fn error_event(id: &str, why: &str) -> AgentEvent {
    AgentEvent::ToolResult {
        id: id.to_string(),
        summary: crate::errors::message(why),
        count: None,
        handles: Vec::new(),
        ui: None,
        is_error: true,
    }
}

/// One line about a tool's JSON result.
fn summary(value: &Value) -> String {
    let value = value
        .get("outcome")
        .filter(|v| v.is_object())
        .unwrap_or(value);
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
    } else if value["outcome"]["measurement"]["handles"].is_array() {
        &value["outcome"]["measurement"]["handles"]
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

    #[derive(Default)]
    struct Recording {
        requests: Arc<Mutex<Vec<TurnRequest>>>,
        state: Arc<Mutex<Value>>,
    }

    #[tokio::test]
    async fn a_local_agent_is_not_retried_or_sent_the_cancelled_request_again() {
        struct LocalAgent(Arc<Mutex<Vec<TurnRequest>>>);
        impl Provider for LocalAgent {
            fn owns_turn(&self) -> bool {
                true
            }
            fn stream_turn<'a>(
                &'a self,
                req: TurnRequest,
                _sink: Sink<'a>,
            ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
                self.0.lock().unwrap().push(req);
                Box::pin(async {
                    Ok(TurnResult {
                        content: Vec::new(),
                        stop_reason: StopReason::MaxTokens,
                        usage: Usage::default(),
                    })
                })
            }
        }
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = Agent::new(
            Box::new(LocalAgent(requests.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        record_turn(&mut agent, "Previous unfinished request").await;
        assert_eq!(requests.lock().unwrap().len(), 1);
        record_turn(&mut agent, "New independent request").await;
        {
            let recorded = requests.lock().unwrap();
            assert_eq!(recorded.len(), 2);
            let newest = serde_json::to_string(recorded[1].messages.last().unwrap()).unwrap();
            assert!(newest.contains("New independent request"));
            assert!(!newest.contains("Previous unfinished request"));
        }
        for autonomy in [Autonomy::PerJob, Autonomy::PerPhase] {
            agent.settings.autonomy = autonomy;
            let (tx, _rx) = mpsc::channel(10);
            let error = agent
                .run_turn("Wait for approval".into(), tx, CancellationToken::new())
                .await
                .unwrap_err();
            assert_eq!(error.code(), "claude_approval");
            assert_eq!(
                requests.lock().unwrap().len(),
                2,
                "no native process starts"
            );
        }
    }

    impl Provider for Recording {
        fn save_state(&self) -> Value {
            self.state.lock().unwrap().clone()
        }
        fn restore_state(&self, state: Value) {
            *self.state.lock().unwrap() = state;
        }
        fn stream_turn<'a>(
            &'a self,
            req: TurnRequest,
            sink: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            self.requests.lock().unwrap().push(req);
            sink(StreamEvent::TextDelta("An".into()));
            sink(StreamEvent::TextDelta(" answer".into()));
            Box::pin(async {
                Ok(TurnResult {
                    content: vec![ContentBlock::Text {
                        text: "An answer".into(),
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                })
            })
        }
    }

    async fn record_turn(agent: &mut Agent, text: &str) {
        let (tx, _rx) = mpsc::channel(100);
        agent
            .run_turn(text.into(), tx, CancellationToken::new())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn snapshot_resume_keeps_the_request_bytes_and_provider_memory() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = Arc::new(Mutex::new(
            serde_json::json!({"extras":{"c1":{"google":{"thought_signature":"sig+/=\r\n"}}}}),
        ));
        let session = Arc::new(Mutex::new(None));
        let mut agent = Agent::new(
            Box::new(Recording {
                requests: requests.clone(),
                state: state.clone(),
            }),
            session.clone(),
            AgentSettings::default(),
        );
        record_turn(&mut agent, "First message").await;
        agent.history.last_mut().unwrap().content.extend([
            ContentBlock::Thinking {
                thinking: "Thought\r\n".into(),
                signature: "sig+/=\r\n".into(),
            },
            ContentBlock::Raw(
                serde_json::json!({"type":"redacted_thinking","data":"unchanged+/="}),
            ),
        ]);
        let saved = agent.snapshot();
        assert_eq!(saved.title, "First message");
        assert!(uuid::Uuid::parse_str(&saved.id).is_ok());
        assert!(
            matches!(&saved.transcript[1], TranscriptEntry::Event { event: AgentEvent::Text { text } } if text == "An answer")
        );
        let bytes = serde_json::to_vec(&saved).unwrap();
        let saved: Conversation = serde_json::from_slice(&bytes).unwrap();
        record_turn(&mut agent, "Continue").await;
        let restored_state = Arc::new(Mutex::new(Value::Null));
        let mut resumed = Agent::resume(
            saved,
            Box::new(Recording {
                requests: requests.clone(),
                state: restored_state.clone(),
            }),
            session,
            AgentSettings::default(),
        );
        assert_eq!(*restored_state.lock().unwrap(), *state.lock().unwrap());
        record_turn(&mut resumed, "Continue").await;
        record_turn(&mut resumed, "Again").await;
        let requests = requests.lock().unwrap();
        assert_eq!(requests[1].system.as_bytes(), requests[2].system.as_bytes());
        let mut after = requests[2].messages.clone();
        let last = after.last_mut().unwrap();
        assert!(
            matches!(&last.content[1], ContentBlock::Text { text } if text.starts_with("[resumed]"))
        );
        last.content.remove(1);
        assert_eq!(
            serde_json::to_vec(&requests[1].messages).unwrap(),
            serde_json::to_vec(&after).unwrap()
        );
        assert!(!requests[3]
            .messages
            .last()
            .unwrap()
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::Text { text } if text.starts_with("[resumed]"))));
    }

    #[tokio::test]
    async fn another_model_or_provider_discards_signatures_and_extras() {
        for change_provider in [false, true] {
            let session = Arc::new(Mutex::new(None));
            let mut agent = Agent::new(
                Box::<Recording>::default(),
                session.clone(),
                AgentSettings::default(),
            );
            record_turn(&mut agent, "First").await;
            agent.history.last_mut().unwrap().content.extend([
                ContentBlock::Thinking {
                    thinking: "thought".into(),
                    signature: "signature".into(),
                },
                ContentBlock::Raw(serde_json::json!({"type":"redacted_thinking","data":"opaque"})),
            ]);
            let mut settings = AgentSettings::default();
            if change_provider {
                settings.provider = "another service".into();
            } else {
                settings.model = "another model".into();
            }
            let requests = Arc::new(Mutex::new(Vec::new()));
            let state = Arc::new(Mutex::new(Value::Null));
            let mut resumed = Agent::resume(
                agent.snapshot(),
                Box::new(Recording {
                    requests: requests.clone(),
                    state: state.clone(),
                }),
                session,
                settings,
            );
            record_turn(&mut resumed, "Continue").await;
            assert_eq!(*state.lock().unwrap(), Value::Null);
            for message in &requests.lock().unwrap()[0].messages {
                assert!(!message.content.is_empty());
                assert!(!message
                    .content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Thinking { .. } | ContentBlock::Raw(_))));
            }
        }
    }

    #[tokio::test]
    async fn changing_model_does_not_resurrect_calls_before_a_fallback() {
        let session = Arc::new(Mutex::new(None));
        let mut agent = Agent::new(
            Box::<Recording>::default(),
            session.clone(),
            AgentSettings::default(),
        );
        record_turn(&mut agent, "Start").await;
        agent.history.push(Message::user(vec![ContentBlock::Text {
            text: "Read".into(),
        }]));
        agent.history.push(Message::assistant(vec![
            ContentBlock::ToolUse {
                id: "declined".into(),
                name: surface::READ.into(),
                input: serde_json::json!({"op":"state"}),
            },
            ContentBlock::Raw(serde_json::json!({"type":"fallback"})),
            ContentBlock::ToolUse {
                id: "accepted".into(),
                name: surface::READ.into(),
                input: serde_json::json!({"op":"state"}),
            },
        ]));
        agent
            .history
            .push(Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: "accepted".into(),
                content: vec![ResultPart::Text("ok".into())],
                is_error: false,
            }]));
        let mut resumed = Agent::resume(
            agent.snapshot(),
            Box::<Recording>::default(),
            session,
            AgentSettings {
                model: "another model".into(),
                ..Default::default()
            },
        );
        record_turn(&mut resumed, "Continue").await;
        let calls: Vec<_> = resumed
            .history
            .iter()
            .flat_map(Message::tool_uses)
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(calls, ["accepted"]);
        assert_well_formed(&resumed.history);
    }

    #[tokio::test]
    async fn snapshot_removes_every_picture_but_keeps_accounting_and_pointed_labels() {
        let session = Arc::new(Mutex::new(None));
        let mut agent = Agent::new(
            Box::<Recording>::default(),
            session.clone(),
            AgentSettings::default(),
        );
        let (tx, _rx) = mpsc::channel(100);
        agent
            .run_turn_with(
                UserMessage {
                    text: "Picture".into(),
                    pointed: vec!["Selected: 2".into()],
                    images: vec![context::UserImage {
                        media_type: "image/png".into(),
                        data: "c2VjcmV0LXNjcmVlbnNob3Q=".into(),
                    }],
                    ..Default::default()
                },
                tx,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        step(&mut agent.history, 1);
        let ContentBlock::ToolResult { content, .. } =
            &mut agent.history.last_mut().unwrap().content[0]
        else {
            panic!("result");
        };
        content.push(ResultPart::Png("dG9vbC1zY3JlZW5zaG90".into()));
        let saved = agent.snapshot();
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("c2VjcmV0LXNjcmVlbnNob3Q="));
        assert!(!json.contains("dG9vbC1zY3JlZW5zaG90"));
        assert_eq!(json.matches(conversation::PICTURE_REMOVED).count(), 3);
        assert!(
            matches!(&saved.transcript[0], TranscriptEntry::User { pointed, images, .. } if pointed == &["Selected: 2"] && images == &[conversation::PICTURE_REMOVED])
        );
        assert_eq!(saved.image_bytes, agent.image_bytes);
        let resumed = Agent::resume(
            saved,
            Box::<Recording>::default(),
            session,
            AgentSettings::default(),
        );
        assert_eq!(resumed.image_bytes, agent.image_bytes);
    }

    #[tokio::test]
    async fn state_reports_the_selected_method_without_changing_the_system() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = Agent::new(
            Box::new(Recording {
                requests: requests.clone(),
                ..Default::default()
            }),
            Arc::new(Mutex::new(None)),
            AgentSettings {
                measurement_method: MeasurementMethod {
                    name: Some("office-method".into()),
                    rules_loaded: true,
                },
                ..Default::default()
            },
        );
        record_turn(&mut agent, "Measure").await;
        let requests = requests.lock().unwrap();
        assert!(
            matches!(&requests[0].messages[0].content[0], ContentBlock::Text { text } if text.contains("Measurement method: office-method; rules loaded: true"))
        );
    }

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

    /// One step: an assistant call and its result.
    fn step(history: &mut Vec<Message>, i: usize) {
        let id = format!("call_{i}");
        history.push(Message::assistant(vec![ContentBlock::ToolUse {
            id: id.clone(),
            name: "qiyas_execute".into(),
            input: serde_json::json!({"op": "run", "args": format!(r#"{{"command":{{"type":"walls","n":{i}}}}}"#)}),
        }]));
        history.push(Message::user(vec![ContentBlock::ToolResult {
            tool_use_id: id,
            content: vec![ResultPart::Text(format!("ok {i}"))],
            is_error: false,
        }]));
    }

    /// Roles alternate from a user message, and every result follows its call.
    fn assert_well_formed(history: &[Message]) {
        assert_eq!(history[0].role, Role::User);
        for (i, w) in history.windows(2).enumerate() {
            assert_ne!(w[0].role, w[1].role, "roles alternate at {i}");
        }
        for (i, m) in history.iter().enumerate() {
            for b in &m.content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = b {
                    assert!(
                        i > 0
                            && history[i - 1]
                                .tool_uses()
                                .any(|(id, _, _)| id == tool_use_id),
                        "result {tool_use_id} follows its call"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn a_long_conversation_is_compacted_and_goes_on() {
        let sent = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::new(
            Box::new(Counting(sent.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        // The founder's villa: one request, 150 steps and an answer (302 messages).
        agent.history.push(Message::user(vec![
            ContentBlock::Text {
                text: "[state] units: metres".into(),
            },
            ContentBlock::Text {
                text: "A villa on a 20 by 25 m plot, about 220 m².".into(),
            },
        ]));
        for i in 0..150 {
            step(&mut agent.history, i);
        }
        agent
            .history
            .push(text(Role::Assistant, "The plan is done."));
        let (tx, _rx) = mpsc::channel(100);
        agent
            .run_turn(
                "Now complete the design package".into(),
                tx,
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            sent.load(Ordering::SeqCst),
            1,
            "the request is sent, not refused"
        );
        assert!(
            agent.history.len() <= COMPACT_TO + 4,
            "{}",
            agent.history.len()
        );
        assert_well_formed(&agent.history);
        let ContentBlock::Text { text: note } = &agent.history[0].content[0] else {
            panic!("a note first");
        };
        assert!(note.starts_with(COMPACTED), "{note}");
        assert!(note.contains("- qiyas_execute run walls"), "{note}");
        let has = |words: &str| {
            agent.history.iter().any(|m| {
                m.content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::Text { text } if text.contains(words)))
            })
        };
        assert!(
            has("A villa on a 20 by 25 m plot, about 220 m²."),
            "the brief is kept whole"
        );
        assert!(
            has("Now complete the design package"),
            "the new request is kept whole"
        );
    }

    #[test]
    fn one_long_turn_loses_its_oldest_steps_and_keeps_the_newest_whole() {
        let mut agent = Agent::new(
            Box::new(Counting(Arc::new(AtomicUsize::new(0)))),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        agent
            .history
            .push(text(Role::User, "Draw the whole package"));
        for i in 0..200 {
            step(&mut agent.history, i);
        }
        let newest = agent.history[agent.history.len() - KEEP_RECENT..].to_vec();
        agent.compact();
        assert!(
            agent.history.len() <= COMPACT_TO + 1,
            "{}",
            agent.history.len()
        );
        assert_eq!(
            &agent.history[agent.history.len() - KEEP_RECENT..],
            &newest[..]
        );
        assert_well_formed(&agent.history);
        // Compacting again keeps the earlier note's steps and adds the new ones.
        for i in 200..400 {
            step(&mut agent.history, i);
        }
        agent.compact();
        assert_well_formed(&agent.history);
        let ContentBlock::Text { text: note } = &agent.history[0].content[0] else {
            panic!("a note first");
        };
        assert_eq!(note.matches(COMPACTED).count(), 1, "one note, merged");
        assert!(note.contains("earlier steps)"), "{note}");
        assert!(agent.history.iter().any(|m| m.content.iter().any(
            |b| matches!(b, ContentBlock::Text { text } if text.contains("Draw the whole package"))
        )));
    }
    #[test]
    fn compaction_keeps_160_recent_messages_all_requests_and_failed_outcomes() {
        let mut agent = Agent::new(
            Box::new(Counting(Arc::new(AtomicUsize::new(0)))),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        let brief = format!(
            "[Important] {}\nKeep millimetres; do not save. Literal \\n stays literal.",
            "Long requirement ".repeat(400)
        );
        agent.history.push(Message::user(vec![
            ContentBlock::Text {
                text: brief.clone(),
            },
            ContentBlock::Text {
                text: "Correction: keep the original layer.".into(),
            },
        ]));
        for i in 0..150 {
            step(&mut agent.history, i);
        }
        if let ContentBlock::ToolResult { is_error, .. } = &mut agent.history[2].content[0] {
            *is_error = true;
        }
        agent.history.push(text(Role::Assistant, "Earlier answer"));
        agent
            .history
            .push(text(Role::User, "Continue the same work"));
        let recent = agent.history[agent.history.len() - COMPACT_TO..].to_vec();
        agent.compact();
        assert!(agent.history.len() >= COMPACT_TO);
        assert_eq!(&agent.history[agent.history.len() - COMPACT_TO..], &recent);
        assert_well_formed(&agent.history);
        let (requests, steps) = take_note(&mut agent.history);
        assert!(requests.contains(&brief));
        assert!(requests.contains(&"Correction: keep the original layer.".into()));
        assert!(steps[0].ends_with("failed"), "{steps:?}");
    }

    #[tokio::test]
    async fn legacy_quantity_prompt_is_removed_on_resume_without_losing_user_requests() {
        let mut first = Agent::new(
            Box::<Recording>::default(),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        record_turn(&mut first, "Draw a bracket").await;
        let mut saved = first.snapshot();
        saved.system = Some("Old rules\nTake-off workflow:\nAlways finish BOQ".into());
        saved
            .history
            .last_mut()
            .unwrap()
            .content
            .push(ContentBlock::Thinking {
                thinking: "old".into(),
                signature: "old signature".into(),
            });
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut resumed = Agent::resume(
            saved,
            Box::new(Recording {
                requests: requests.clone(),
                ..Default::default()
            }),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        record_turn(&mut resumed, "Continue").await;
        let requests = requests.lock().unwrap();
        assert!(!requests[0].system.contains("Take-off workflow:"));
        let messages = serde_json::to_string(&requests[0].messages).unwrap();
        assert!(messages.contains("Draw a bracket"));
        assert!(!messages.contains("old signature"));
    }
    #[test]
    fn loop_guard_matches_object_and_encoded_command_aliases() {
        let object = serde_json::json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[0,0],"b":[1,1]}}});
        let encoded = serde_json::json!({"op":"run","args":serde_json::json!({"command":serde_json::json!({"type":"draw.line","a":[0,0],"b":[1,1]}).to_string()}).to_string()});
        assert_eq!(
            edit_fingerprint(crate::surface::EXECUTE, &object),
            edit_fingerprint(crate::surface::EXECUTE, &encoded)
        );
        assert!(edit_fingerprint(crate::surface::EXECUTE, &object).is_some());
    }
}

#[cfg(test)]
mod provider_failure_tests {
    use super::*;
    use crate::provider::{BoxFuture, Sink, TurnResult};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ContextProvider(Arc<AtomicUsize>);
    impl Provider for ContextProvider {
        fn stream_turn<'a>(
            &'a self,
            req: TurnRequest,
            _: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            let attempt = self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    Err(AiError::Api {
                        status: 400,
                        message: "context_length_exceeded".into(),
                        body: String::new(),
                        request: None,
                    })
                } else {
                    assert!(serde_json::to_string(&req.messages).unwrap().len() < 15_000);
                    assert!(req
                        .messages
                        .iter()
                        .any(|m| m.tool_uses().any(|(id, _, _)| id == "earlier")));
                    Ok(TurnResult {
                        content: vec![],
                        stop_reason: StopReason::EndTurn,
                        usage: Usage::default(),
                    })
                }
            })
        }
    }

    #[tokio::test]
    async fn context_refusal_shortens_large_results_once_without_losing_call_pairing() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::new(
            Box::new(ContextProvider(attempts.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        );
        agent.history = vec![
            Message::user(vec![ContentBlock::Text {
                text: "Keep my drawing unchanged.".into(),
            }]),
            Message::assistant(vec![ContentBlock::ToolUse {
                id: "earlier".into(),
                name: "qiyas_read".into(),
                input: serde_json::json!({"op":"records"}),
            }]),
            Message::user(vec![ContentBlock::ToolResult {
                tool_use_id: "earlier".into(),
                content: vec![ResultPart::Text("evidence ".repeat(5000))],
                is_error: false,
            }]),
        ];
        let (tx, mut rx) = mpsc::channel(100);
        agent
            .run_turn("Continue".into(), tx, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let mut retry = false;
        while let Ok(event) = rx.try_recv() {
            retry |= matches!(event, AgentEvent::Retrying { code, .. } if code == "ai.error.context_retry");
        }
        assert!(retry);
        assert!(serde_json::to_string(agent.history())
            .unwrap()
            .contains("Keep my drawing unchanged."));
    }

    struct ThinkingLimit(Arc<AtomicUsize>);
    impl Provider for ThinkingLimit {
        fn stream_turn<'a>(
            &'a self,
            req: TurnRequest,
            sink: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            let attempt = self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    sink(StreamEvent::Thinking("unfinished thinking".into()));
                    Ok(TurnResult {
                        content: vec![ContentBlock::Thinking {
                            thinking: "unfinished thinking".into(),
                            signature: String::new(),
                        }],
                        stop_reason: StopReason::MaxTokens,
                        usage: Usage::default(),
                    })
                } else {
                    assert_eq!(req.max_tokens, 200);
                    Ok(TurnResult {
                        content: vec![],
                        stop_reason: StopReason::EndTurn,
                        usage: Usage::default(),
                    })
                }
            })
        }
    }

    #[tokio::test]
    async fn retry_after_only_thinking_is_visible_and_discards_the_unfinished_thought() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let mut agent = Agent::new(
            Box::new(ThinkingLimit(attempts.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings {
                max_tokens: 100,
                ..AgentSettings::default()
            },
        );
        let (tx, mut rx) = mpsc::channel(100);
        agent
            .run_turn("Continue".into(), tx, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let mut retry = false;
        while let Ok(event) = rx.try_recv() {
            retry |= matches!(event, AgentEvent::Retrying { code, discard_partial: true, .. } if code == "ai.error.output_limit");
        }
        assert!(retry);
        assert!(!serde_json::to_string(&agent.transcript)
            .unwrap()
            .contains("unfinished thinking"));
    }

    struct Tokens(Arc<AtomicUsize>);
    impl Provider for Tokens {
        fn save_state(&self) -> Value {
            self.0.fetch_add(1, Ordering::SeqCst);
            Value::Null
        }
        fn stream_turn<'a>(
            &'a self,
            _: TurnRequest,
            sink: Sink<'a>,
        ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
            Box::pin(async move {
                for _ in 0..1000 {
                    sink(StreamEvent::TextDelta("word ".into()));
                }
                Ok(TurnResult {
                    content: vec![ContentBlock::Text {
                        text: "word ".repeat(1000),
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                })
            })
        }
    }

    #[tokio::test]
    async fn a_thousand_tokens_do_not_make_a_thousand_full_checkpoints() {
        let saves = Arc::new(AtomicUsize::new(0));
        let checkpoint = Arc::new(Mutex::new(None));
        let mut agent = Agent::new(
            Box::new(Tokens(saves.clone())),
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        )
        .with_checkpoint(checkpoint.clone());
        let (tx, _rx) = mpsc::channel(2000);
        agent
            .run_turn("Tell me".into(), tx, CancellationToken::new())
            .await
            .unwrap();
        assert!(saves.load(Ordering::SeqCst) < 10);
        let saved = checkpoint.lock().unwrap();
        assert!(saved.as_ref().unwrap().transcript.iter().any(|entry| matches!(entry, TranscriptEntry::Event { event: AgentEvent::Text { text } } if text == &"word ".repeat(1000))));
    }
}
