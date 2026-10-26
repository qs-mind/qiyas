use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use qiyas_core::{DrawingInfo, Session};

use crate::ai::AiRun;
use crate::mcp::McpRun;
use crate::pdf::PictureCache;
use crate::settings::{self, Settings};

pub fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A drawing and its Mizan stay here for their whole lifetime. The window only changes which
/// workspace it displays. Session references held by agents never follow the visible tab.
#[derive(Default)]
pub struct PublishedRender {
    pub version: u64,
    pub batch: Option<Vec<u8>>,
    pub atlas: Option<Vec<u8>>,
}
pub struct DrawingWorkspace {
    pub id: u32,
    pub view: Mutex<crate::workspace_view::WorkspaceView>,
    pub context: Mutex<qiyas_ai::TurnContext>,
    pub render: Mutex<PublishedRender>,
    pub reads: Mutex<BTreeMap<&'static str, serde_json::Value>>,
    pub session: Arc<Mutex<Option<Session>>>,
    pub ai: Mutex<AiRun>,
    /// External sheet changes exclude a new turn without holding its status lock during work.
    pub sheet_authority: Mutex<()>,
    pub ai_window: crate::ai_window::AiWindow,
    pub pictures: Mutex<PictureCache>,
    pub provider: Mutex<Settings>,
    /// Last committed metadata, available even while a tool holds the drawing lock.
    pub published: Mutex<Option<DrawingInfo>>,
    publication: Mutex<Option<serde_json::Value>>,
    pub dirty: std::sync::atomic::AtomicBool,
    pub closed: std::sync::atomic::AtomicBool,
}

impl DrawingWorkspace {
    fn new(id: u32, config_dir: &std::path::Path, settings: Settings) -> Self {
        Self {
            id,
            view: Mutex::default(),
            context: Mutex::default(),
            render: Mutex::default(),
            reads: Mutex::default(),
            session: Arc::new(Mutex::new(None)),
            ai: Mutex::new(AiRun::new(config_dir)),
            sheet_authority: Mutex::new(()),
            ai_window: crate::ai_window::AiWindow::new(id),
            pictures: Mutex::new(PictureCache::default()),
            provider: Mutex::new(settings),
            published: Mutex::new(None),
            publication: Mutex::new(None),
            dirty: std::sync::atomic::AtomicBool::new(false),
            closed: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn publish(&self, session: &Session) {
        let stamp = serde_json::json!([
            session.revision(),
            session.batch_version(),
            session.sheet,
            session.is_dirty()
        ]);
        let mut previous = lock(&self.publication);
        if previous.as_ref() == Some(&stamp) {
            return;
        }
        *previous = Some(stamp);
        self.dirty
            .store(session.is_dirty(), std::sync::atomic::Ordering::Relaxed);
        *lock(&self.published) = Some(session.info());
        let mut reads = lock(&self.reads);
        reads.insert("paper_views", serde_json::json!(session.paper_views()));
        reads.insert("drafting", serde_json::json!(session.drafting));
        reads.insert("takeoff", serde_json::json!(session.takeoff.items));
        reads.insert("totals", serde_json::json!(session.takeoff.totals()));
        reads.insert("boq", serde_json::json!(session.boq));
        reads.insert("conflicts", serde_json::json!(session.conflicts));
    }
}

#[derive(Default)]
pub struct TabList {
    pub order: Vec<u32>,
    pub active: Option<u32>,
    pub next_id: u32,
    pub lobby: Option<Arc<DrawingWorkspace>>,
    pub workspaces: BTreeMap<u32, Arc<DrawingWorkspace>>,
}

/// Managed by Tauri. Each IPC resolves an optional workspace once with `scoped`. Clones share
/// app services but keep a fixed drawing binding. The root's workspace is the no-drawing panel.
/// Lock order: copy registry references and release its lock, then sheet authority, AI, session,
/// published metadata. Never acquire the registry or AI lock while holding a session lock.
#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<DrawingWorkspace>,
    bound: bool,
    pub projects: Arc<Mutex<crate::projects::Workspace>>,
    pub tabs: Arc<Mutex<TabList>>,
    pub settings: Arc<Mutex<Settings>>,
    pub config_dir: PathBuf,
    pub mcp: Arc<Mutex<McpRun>>,
    pub turn_queue: Arc<crate::turn_queue::TurnQueue>,
    #[cfg(test)]
    pub available_memory: Option<u64>,
    pub clipboard: Arc<Mutex<Option<arboard::Clipboard>>>,
}

impl Deref for AppState {
    type Target = DrawingWorkspace;
    fn deref(&self) -> &Self::Target {
        &self.workspace
    }
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let settings = settings::load(&config_dir);
        let workspace = Arc::new(DrawingWorkspace::new(0, &config_dir, settings.clone()));
        Self {
            workspace: workspace.clone(),
            bound: false,
            projects: Arc::default(),
            tabs: Arc::new(Mutex::new(TabList {
                next_id: 1,
                lobby: Some(workspace),
                ..Default::default()
            })),
            settings: Arc::new(Mutex::new(settings)),
            config_dir,
            mcp: Arc::default(),
            clipboard: Arc::default(),
            turn_queue: Arc::default(),
            #[cfg(test)]
            available_memory: None,
        }
    }

    /// Resolve once, before awaiting or taking any drawing lock. An explicit missing tab never
    /// falls back to another drawing. Bound host calls retain their original workspace.
    pub fn scoped(&self, id: Option<u32>) -> Result<Self, String> {
        if id.is_none() && self.bound {
            return Ok(self.clone());
        }
        let tabs = lock(&self.tabs);
        let workspace = match id.or(tabs.active) {
            Some(id) => tabs
                .workspaces
                .get(&id)
                .or_else(|| tabs.lobby.as_ref().filter(|lobby| lobby.id == id))
                .cloned()
                .ok_or_else(|| format!("Drawing {id} is closed"))?,
            None => tabs.lobby.as_ref().unwrap_or(&self.workspace).clone(),
        };
        Ok(Self {
            workspace,
            bound: true,
            ..self.clone()
        })
    }

    /// The answerer belongs to the drawing; shared work rules still follow Settings.
    pub fn turn_settings(&self) -> Settings {
        let shared = lock(&self.settings).clone();
        let mut choice = lock(&self.provider).clone();
        choice.autonomy = shared.autonomy;
        choice.measurement_method = shared.measurement_method;
        choice.country = shared.country;
        choice
    }

    pub fn turn_limit(&self) -> usize {
        // Tests inject pressure explicitly; the other agents' builds must not make routing tests flaky.
        #[cfg(test)]
        let available = self.available_memory;
        #[cfg(not(test))]
        let available = crate::turn_queue::available_memory_kib();
        crate::turn_queue::effective_limit(lock(&self.settings).max_concurrent_turns, available)
    }

    pub fn workspaces(&self) -> Vec<Arc<DrawingWorkspace>> {
        let tabs = lock(&self.tabs);
        tabs.order
            .iter()
            .filter_map(|id| tabs.workspaces.get(id).cloned())
            .collect()
    }

    /// All chat owners, including the no-drawing panel, without duplicate references.
    pub fn conversations(&self) -> Vec<Arc<DrawingWorkspace>> {
        let mut all = self.workspaces();
        if let Some(lobby) = lock(&self.tabs).lobby.clone() {
            if !all.iter().any(|workspace| workspace.id == lobby.id) {
                all.push(lobby);
            }
        }
        all.sort_by_key(|workspace| workspace.id);
        all
    }

    pub fn make_workspace(&self, id: u32) -> Arc<DrawingWorkspace> {
        Arc::new(DrawingWorkspace::new(
            id,
            &self.config_dir,
            lock(&self.settings).clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_choice_keeps_shared_work_rules_current() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().join("config"));
        {
            let mut choice = lock(&state.provider);
            choice.profile_mut().model = "drawing-model".into();
            choice.profile_mut().effort = Some("high".into());
        }
        {
            let mut defaults = lock(&state.settings);
            defaults.provider = settings::Provider::OpenaiCompatible;
            defaults.autonomy = qiyas_ai::Autonomy::PerJob;
            defaults.measurement_method = Some("shared-method".into());
        }
        let turn = state.turn_settings();
        assert_eq!(turn.provider, settings::Provider::Anthropic);
        assert_eq!(turn.profile().model, "drawing-model");
        assert_eq!(turn.profile().effort.as_deref(), Some("high"));
        assert_eq!(turn.autonomy, qiyas_ai::Autonomy::PerJob);
        assert_eq!(turn.measurement_method.as_deref(), Some("shared-method"));
    }
}
