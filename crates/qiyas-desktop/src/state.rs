use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use qiyas_core::Session;

use crate::ai::AiRun;
use crate::mcp::McpRun;
use crate::pdf::PictureCache;
use crate::settings::{self, Settings};

/// An open drawing not on screen: its tab id and its session, waiting to be swapped back into
/// `AppState::session`.
pub struct ParkedTab {
    pub id: u32,
    pub session: Session,
}

/// The file tabs, in the order shown, and which one is on screen (its session is
/// `AppState::session`; the others are in `AppState::parked`).
#[derive(Default)]
pub struct TabList {
    pub order: Vec<u32>,
    pub active: Option<u32>,
    pub next_id: u32,
}

/// Managed by Tauri and shared by every command.
pub struct AppState {
    /// The file tabs. Lock order when several are needed: `tabs`, then `session`, then `parked`.
    pub tabs: Mutex<TabList>,
    /// The drawing on screen (the active tab), if any. Shared with the AI agent and the MCP
    /// server, which hold a clone of the `Arc`, so they always act on the active tab.
    pub session: Arc<Mutex<Option<Session>>>,
    /// The other open drawings.
    pub parked: Mutex<Vec<ParkedTab>>,
    /// The settings, loaded at startup and written back on every change.
    pub settings: Mutex<Settings>,
    /// Where `settings.json` lives.
    pub config_dir: PathBuf,
    /// The in-app assistant's conversation and running turn.
    pub ai: Mutex<AiRun>,
    /// The MCP server for outside AI apps.
    pub mcp: Mutex<McpRun>,
    /// Underlay pictures drawn lately.
    pub pictures: Mutex<PictureCache>,
    /// The system clipboard, opened on first use and kept: on X11 a dropped one takes what it
    /// copied with it.
    pub clipboard: Mutex<Option<arboard::Clipboard>>,
    /// The questions Mizan's window tools put to the window, waiting for its reply.
    pub ai_window: crate::ai_window::AiWindow,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        AppState {
            tabs: Mutex::new(TabList::default()),
            session: Arc::new(Mutex::new(None)),
            parked: Mutex::new(Vec::new()),
            settings: Mutex::new(settings::load(&config_dir)),
            config_dir,
            ai: Mutex::new(AiRun::default()),
            mcp: Mutex::new(McpRun::default()),
            pictures: Mutex::new(PictureCache::default()),
            clipboard: Mutex::new(None),
            ai_window: crate::ai_window::AiWindow::default(),
        }
    }
}
