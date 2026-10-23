//! The in-app assistant's state: one `Agent` per conversation, built at its first message from the
//! settings and the keychain, and lent to the background task that runs each turn.
//!
//! A new conversation saves the current one before dropping its agent. A shared checkpoint keeps
//! the last complete model step available while the background task owns the agent.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Why `ai_send` refused a turn when a new conversation began while it was starting; the window
/// shows `ai.restarted`.
pub const RESTARTED: &str = "restarted";
/// The error of `ai:done` when the turn's task panicked; the window shows `ai.crashed`.
pub const CRASHED: &str = "crashed";

/// Makes the provider instead of the settings and keychain, in tests.
#[cfg(test)]
pub type TestProvider = Arc<dyn Fn() -> Box<dyn Provider> + Send + Sync>;

use qiyas_ai::anthropic::Anthropic;
use qiyas_ai::conversation::Conversation;
use qiyas_ai::openai::OpenAiCompatible;
use qiyas_ai::prompt::Lang;
use qiyas_ai::{Agent, AgentSettings, Provider};
use qiyas_core::Session;
use tokio_util::sync::CancellationToken;

use crate::settings::{self, Settings};

#[derive(Default)]
pub struct AiRun {
    /// The conversation; `None` before its first message, and while a turn runs (the task holds
    /// it).
    agent: Option<Agent>,
    /// Cancels the running turn.
    cancel: Option<CancellationToken>,
    busy: bool,
    /// Bumped by every new conversation. A turn started in an older one drops its agent and its
    /// events.
    generation: u64,
    /// The language of the system prompt, set by the window.
    language: Lang,
    /// Messages in the conversation after the last finished turn.
    messages: usize,
    /// The id and latest recoverable conversation, including while the task holds the agent.
    checkpoint: Arc<Mutex<Option<Conversation>>>,
    store: Option<crate::conversations::Store>,
    #[cfg(test)]
    pub test_provider: Option<TestProvider>,
}

/// A turn handed to the background task.
pub struct Turn {
    pub agent: Agent,
    pub cancel: CancellationToken,
    pub generation: u64,
}

impl AiRun {
    pub fn new(config_dir: &Path) -> Self {
        let store = crate::conversations::Store::new(config_dir);
        if let Err(error) = store.recover() {
            eprintln!("The conversation index could not be recovered: {error}");
        }
        Self {
            store: Some(store),
            ..Self::default()
        }
    }

    pub fn conversation_id(&self) -> Option<String> {
        self.checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|conversation| conversation.id.clone())
    }

    /// Called with the turn lock held; never takes the session lock (tab changes hold it).
    pub fn save(&self) -> Result<(), String> {
        let checkpoint = self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let (Some(store), Some(conversation)) = (&self.store, checkpoint.as_ref()) {
            store.save(conversation)?;
        }
        Ok(())
    }

    pub fn save_logged(&self) {
        if let Err(error) = self.save() {
            eprintln!("The conversation could not be saved: {error}");
        }
    }

    /// A panic hook must not wait on a lock held by the panicking thread.
    pub fn save_if_available(&self) {
        if let (Some(store), Ok(checkpoint)) = (&self.store, self.checkpoint.try_lock()) {
            if let Some(conversation) = checkpoint.as_ref() {
                if let Err(error) = store.save(conversation) {
                    eprintln!("The conversation could not be saved after the error: {error}");
                }
            }
        }
    }

    pub fn drawing_saved_as(&mut self, drawing: PathBuf) {
        if let Some(agent) = &mut self.agent {
            agent.drawing_saved_as(drawing.clone());
        }
        if let Some(conversation) = self
            .checkpoint
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            conversation.phase_state.drawing = Some(drawing.clone());
            conversation.drawing = Some(drawing);
        }
        self.save_logged();
    }

    pub fn resume(&mut self, agent: Agent) {
        self.busy = false;
        self.cancel = None;
        self.messages = agent.history().len();
        self.checkpoint = Arc::new(Mutex::new(Some(agent.snapshot())));
        self.agent = Some(agent.with_checkpoint(self.checkpoint.clone()));
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    pub fn message_count(&self) -> usize {
        self.messages
    }

    pub fn language(&self) -> Lang {
        self.language
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Cancels a running turn and forgets the conversation.
    pub fn new_conversation(&mut self) {
        self.save_logged();
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.agent = None;
        self.generation += 1;
        self.messages = 0;
        // The old task keeps its own checkpoint; later steps cannot replace the new chat.
        self.checkpoint = Arc::new(Mutex::new(None));
    }

    /// Sets the prompt language; a change starts a new conversation. True when it changed.
    pub fn set_language(&mut self, lang: Lang) -> bool {
        if lang == self.language {
            return false;
        }
        self.language = lang;
        self.new_conversation();
        true
    }

    /// Claims the next turn: `Err("busy")` while one runs. Gives the conversation's agent (`None`
    /// when one must be built) and the generation to pass to `start`.
    pub fn reserve(&mut self) -> Result<(Option<Agent>, u64), String> {
        if self.busy {
            return Err("busy".into());
        }
        self.busy = true;
        Ok((self.agent.take(), self.generation))
    }

    /// Gives up a claimed turn that could not start (no key, say).
    pub fn release(&mut self, agent: Option<Agent>, generation: u64) {
        self.busy = false;
        if generation == self.generation {
            self.agent = agent;
        }
    }

    /// Starts the claimed turn with `agent`, unless a new conversation began since `reserve`.
    pub fn start(&mut self, agent: Agent, generation: u64) -> Result<Turn, String> {
        if generation != self.generation {
            self.busy = false;
            return Err(RESTARTED.into());
        }
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        Ok(Turn {
            agent: agent.with_checkpoint(self.checkpoint.clone()),
            cancel,
            generation,
        })
    }

    /// Takes the agent back after a turn; it is dropped when a new conversation began meanwhile.
    pub fn finish(&mut self, agent: Agent, generation: u64) {
        self.busy = false;
        self.cancel = None;
        if generation == self.generation {
            let snapshot = agent.snapshot();
            *self
                .checkpoint
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
            self.save_logged();
            self.messages = agent.history().len();
            self.agent = Some(agent);
        }
    }

    /// A panicking task leaves its last complete checkpoint recoverable from History.
    pub fn crashed(&mut self) {
        self.new_conversation();
        self.busy = false;
    }

    /// Cancels the running turn, if any.
    pub fn cancel(&self) {
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

/// What the agent needs from the settings, in the window's language.
pub fn agent_settings(s: &Settings, language: Lang) -> AgentSettings {
    AgentSettings {
        provider: provider_identity(s),
        model: s.profile().model.clone(),
        effort: s.effort.clone(),
        language,
        autonomy: s.autonomy,
        measurement_method: crate::methods::selected(s.measurement_method.as_deref()),
        ..AgentSettings::default()
    }
}

/// Signatures belong to one service as well as one model. Keep a deterministic endpoint
/// fingerprint in conversations without copying the address or its optional credentials.
/// FNV-1a is an identity checksum here, not a security hash.
fn provider_identity(settings: &Settings) -> String {
    if settings.provider == settings::Provider::Anthropic {
        return settings.provider.name().into();
    }
    let endpoint = settings
        .profile()
        .base_url
        .as_deref()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/');
    let hash = endpoint.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{}:{hash:016x}", settings.provider.name())
}

/// The agent log, beside `settings.json`: `logs/agent.log` in the app's config folder.
pub fn log_path(config_dir: &Path) -> PathBuf {
    config_dir.join("logs").join("agent.log")
}

/// Most of the log the window is sent: its newest part.
const LOG_TAIL: u64 = 256 * 1024;

/// The last `LOG_TAIL` bytes of the log at `path`, from the start of an entry where one begins in
/// them; empty when there is no log yet.
pub fn log_tail(path: &Path) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if start == 0 {
        return Ok(text);
    }
    // Entries start at a line that is not indented.
    let first = text
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .find(|&i| text[i..].chars().next().is_some_and(|c| c != ' '));
    Ok(first.map_or(text.clone(), |i| text[i..].to_string()))
}

/// The first log entry of a conversation: who answers it. Never the key.
pub fn conversation_entry(provider: &str, s: &Settings, language: Lang) -> String {
    let mut lines = vec![
        format!("provider: {provider}"),
        format!("model: {}", s.profile().model),
    ];
    if let (settings::Provider::OpenaiCompatible, Some(url)) = (s.provider, &s.profile().base_url) {
        lines.push(format!("address: {url}"));
    }
    lines.push(format!("effort: {}", s.effort));
    lines.push(format!("language: {language:?}"));
    lines.join("\n")
}

/// Whether going from `old` to `new` changes who answers (provider, address or model), which needs
/// a new conversation. Effort changes apply to the next message.
pub fn answerer_changed(old: &Settings, new: &Settings) -> bool {
    (old.provider, old.profile()) != (new.provider, new.profile())
}

/// The provider the settings name, with `key` from the keychain. `Err("no_api_key")` for Claude
/// without a key, `Err("no_base_url")` without an OpenAI-compatible service address, or
/// `Err("no_model")` until its model is chosen. The window opens Settings for all three.
pub fn build_provider(s: &Settings, key: Option<String>) -> Result<Box<dyn Provider>, String> {
    match s.provider {
        settings::Provider::Anthropic => {
            let key = key.ok_or("no_api_key")?;
            Ok(Box::new(Anthropic::new(key)))
        }
        settings::Provider::OpenaiCompatible => {
            let url = s
                .profile()
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .ok_or("no_base_url")?;
            qiyas_ai::endpoint::normalize(url)?;
            if s.profile().model.trim().is_empty() {
                return Err("no_model".into());
            }
            Ok(Box::new(OpenAiCompatible::new(url, key)))
        }
    }
}

/// The canned provider of `QIYAS_AI_MOCK=1`, in debug builds only.
pub fn mock_provider(session: &Arc<Mutex<Option<Session>>>) -> Option<Box<dyn Provider>> {
    #[cfg(debug_assertions)]
    if std::env::var("QIYAS_AI_MOCK").as_deref() == Ok("1") {
        return Some(Box::new(crate::mock::Mock::new(session.clone())));
    }
    let _ = session;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_tail_starts_at_an_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.log");
        assert_eq!(log_tail(&path).unwrap(), "", "no log yet");
        let entry = "2026-10-09 12:00:00.000  TOOL x\n    result: done\n";
        std::fs::write(&path, entry.repeat(LOG_TAIL as usize / entry.len() + 10)).unwrap();
        let tail = log_tail(&path).unwrap();
        assert!(tail.starts_with("2026-10-09"), "{}", &tail[..40]);
        assert!(tail.len() as u64 <= LOG_TAIL);
    }

    #[test]
    fn a_conversation_entry_names_who_answers_and_never_the_key() {
        let s = Settings {
            provider: settings::Provider::OpenaiCompatible,
            providers: settings::ProviderProfiles {
                openai_compatible: settings::ProviderProfile {
                    base_url: Some("https://api.runware.ai/v1".into()),
                    model: "google:gemini@3.8-flash".into(),
                },
                ..Default::default()
            },
            ..Settings::default()
        };
        let entry = conversation_entry("openai_compatible", &s, Lang::Ar);
        assert!(entry.contains("model: google:gemini@3.8-flash"));
        assert!(entry.contains("address: https://api.runware.ai/v1"));
        assert!(entry.contains("language: Ar"));
    }

    fn agent() -> Agent {
        let provider = build_provider(&Settings::default(), Some("k".into())).unwrap();
        Agent::new(
            provider,
            Arc::new(Mutex::new(None)),
            AgentSettings::default(),
        )
    }

    #[test]
    fn one_turn_at_a_time_and_the_agent_comes_back() {
        let mut run = AiRun::default();
        let (none, generation) = run.reserve().unwrap();
        assert!(none.is_none(), "the first message builds the agent");
        assert_eq!(run.reserve().err().as_deref(), Some("busy"));
        let turn = run.start(agent(), generation).unwrap();
        assert!(run.busy());
        run.finish(turn.agent, turn.generation);
        assert!(!run.busy());
        let (kept, _) = run.reserve().unwrap();
        assert!(kept.is_some(), "the conversation is kept between turns");
    }

    #[test]
    fn save_as_updates_the_agent_before_the_next_turn_is_claimed() {
        let mut run = AiRun::default();
        let (_, generation) = run.reserve().unwrap();
        let turn = run.start(agent(), generation).unwrap();
        run.finish(turn.agent, generation);

        let drawing = PathBuf::from("named.dxf");
        run.drawing_saved_as(drawing.clone());
        let (kept, generation) = run.reserve().unwrap();
        assert_eq!(
            kept.as_ref().unwrap().snapshot().drawing,
            Some(drawing.clone())
        );
        run.release(kept, generation);
        let (kept, generation) = run.reserve().unwrap();
        let turn = run.start(kept.unwrap(), generation).unwrap();
        assert_eq!(turn.agent.snapshot().drawing, Some(drawing.clone()));
        assert_eq!(
            run.checkpoint.lock().unwrap().as_ref().unwrap().drawing,
            Some(drawing)
        );
        run.finish(turn.agent, generation);
    }

    #[test]
    fn a_new_conversation_cancels_the_turn_and_drops_its_agent() {
        let mut run = AiRun::default();
        let (_, generation) = run.reserve().unwrap();
        let turn = run.start(agent(), generation).unwrap();
        run.new_conversation();
        assert!(turn.cancel.is_cancelled());
        run.finish(turn.agent, turn.generation);
        assert!(!run.busy());
        let (agent, _) = run.reserve().unwrap();
        assert!(agent.is_none(), "the next message starts afresh");
    }

    #[test]
    fn a_reset_between_reserve_and_start_refuses_the_turn() {
        let mut run = AiRun::default();
        let (_, generation) = run.reserve().unwrap();
        run.new_conversation();
        assert!(run.start(agent(), generation).is_err());
        assert!(!run.busy());
    }

    #[test]
    fn only_a_new_language_starts_a_new_conversation() {
        let mut run = AiRun::default();
        assert!(!run.set_language(Lang::En));
        let before = run.generation();
        assert!(run.set_language(Lang::Ar));
        assert_eq!(run.generation(), before + 1);
        assert_eq!(run.language(), Lang::Ar);
    }

    #[test]
    fn providers_need_a_key_or_an_address() {
        let mut s = Settings::default();
        assert_eq!(
            build_provider(&s, None).err().as_deref(),
            Some("no_api_key")
        );
        s.provider = settings::Provider::OpenaiCompatible;
        assert_eq!(
            build_provider(&s, None).err().as_deref(),
            Some("no_base_url")
        );
        s.profile_mut().base_url = Some("http://localhost:11434/v1".into());
        assert_eq!(build_provider(&s, None).err().as_deref(), Some("no_model"));
        s.profile_mut().model = "local-model".into();
        assert!(
            build_provider(&s, None).is_ok(),
            "a local server may take no key"
        );
    }

    #[test]
    fn effort_keeps_the_conversation_and_model_does_not() {
        let old = Settings::default();
        let mut new = old.clone();
        new.effort = "low".into();
        assert!(!answerer_changed(&old, &new));
        new.providers.openai_compatible.model = "other-provider-model".into();
        assert!(!answerer_changed(&old, &new));
        new.profile_mut().model = "claude-sonnet-5-5".into();
        assert!(answerer_changed(&old, &new));
    }

    #[test]
    fn compatible_endpoint_changes_the_saved_provider_identity() {
        let mut settings = Settings::default();
        assert_eq!(agent_settings(&settings, Lang::En).provider, "anthropic");
        settings.provider = settings::Provider::OpenaiCompatible;
        settings.profile_mut().model = "same-model".into();
        settings.profile_mut().base_url = Some("https://first.invalid/v1".into());
        let first = agent_settings(&settings, Lang::En);
        assert!(!first.provider.contains("first.invalid"));
        settings.profile_mut().base_url = Some(" https://first.invalid/v1/ ".into());
        assert_eq!(agent_settings(&settings, Lang::En).provider, first.provider);
        settings.profile_mut().base_url = Some("https://second.invalid/v1".into());
        let second = agent_settings(&settings, Lang::En);
        assert_eq!(first.model, second.model);
        assert_ne!(first.provider, second.provider);
        assert!(!second.provider.contains("second.invalid"));
    }
}
