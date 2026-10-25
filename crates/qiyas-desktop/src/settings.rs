//! App settings in `<app config dir>/settings.json`. The AI provider's API key is not here: it
//! lives in the OS keychain (see `secrets`).
//!
//! Plain functions over a directory, so tests run without Tauri.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::ribbon::RibbonLayout;

const FILE: &str = "settings.json";

/// Drawings kept in the Open menu's recent list.
pub const MAX_RECENTS: usize = 10;

pub const EFFORTS: [&str; 6] = ["none", "low", "medium", "high", "xhigh", "max"];

/// Buttons the tool palette keeps at most.
pub const MAX_PALETTE: usize = 100;

/// The longest time between autosaves, in minutes.
pub const MAX_AUTOSAVE_MINUTES: u32 = 120;

/// The snap aperture and the pick box, in screen pixels: at least and at most.
pub const MIN_PX: u32 = 1;
pub const MAX_PX: u32 = 50;

/// Aliases and shortcuts kept at most, each.
pub const MAX_ALIASES: usize = 500;

/// The keys a shortcut may end in besides letters, digits and F1 to F24.
const NAMED_KEYS: [&str; 15] = [
    "Escape",
    "Enter",
    "Tab",
    "Space",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
];

/// What a tool palette button does: insert a block of the drawing, or run a command line.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaletteKind {
    Block,
    Command,
}

/// One button of the user's tool palette: its label, and the block name or command line.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub kind: PaletteKind,
    pub label: String,
    pub value: String,
}

/// Who answers the in-app assistant.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    #[default]
    Anthropic,
    OpenaiCompatible,
    ChatgptPlan,
    ClaudeCode,
}

impl Provider {
    /// The name used in the settings file and as the keychain account.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenaiCompatible => "openai_compatible",
            Provider::ChatgptPlan => "chatgpt_plan",
            Provider::ClaudeCode => "claude_code",
        }
    }

    pub fn from_name(name: &str) -> Option<Provider> {
        [
            Provider::Anthropic,
            Provider::OpenaiCompatible,
            Provider::ChatgptPlan,
            Provider::ClaudeCode,
        ]
        .into_iter()
        .find(|p| p.name() == name)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderProfile {
    /// Independent effort for this provider; absent profiles use the provider default.
    pub effort: Option<String>,
    /// Used exactly as typed.
    pub model: String,
    /// Only OpenAI-compatible services use an address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Model capability reported by the service or confirmed in Settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
}

fn anthropic_profile() -> ProviderProfile {
    ProviderProfile {
        effort: None,
        model: "claude-opus-5-5".into(),
        base_url: None,
        image_input: Some(true),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderProfiles {
    pub anthropic: ProviderProfile,
    pub openai_compatible: ProviderProfile,
    pub chatgpt_plan: ProviderProfile,
    pub claude_code: ProviderProfile,
}

impl Default for ProviderProfiles {
    fn default() -> Self {
        Self {
            anthropic: anthropic_profile(),
            openai_compatible: ProviderProfile::default(),
            chatgpt_plan: ProviderProfile::default(),
            claude_code: ProviderProfile::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Maximum concurrent Mizan turns; Claude Code still has one process at a time.
    pub max_concurrent_turns: usize,
    pub provider: Provider,
    /// Empty uses the user-installed command on PATH; never downloaded by Qiyas.
    pub claude_executable: Option<String>,
    /// Each provider keeps its own model and address when the chosen provider changes.
    pub providers: ProviderProfiles,
    /// Legacy effort, migrated into the selected profile when loading older settings.
    pub effort: String,
    /// When Mizan waits for approval of its plan.
    pub autonomy: qiyas_ai::Autonomy,
    /// The selected measurement method name; local rules are optional. None means geometric quantities.
    pub measurement_method: Option<String>,
    /// Shows diagnostic disclosures in Mizan; ordinary surveying work keeps them hidden.
    pub show_technical_details: bool,
    pub mcp_enabled: bool,
    pub mcp_port: u16,
    /// 32 hex characters, made once and kept. Empty when missing from the file, so `load` makes
    /// and writes one instead of a new random one each start.
    #[serde(default)]
    pub mcp_token: String,
    /// "en" or "ar".
    pub language: String,
    /// "system", "light" or "dark".
    pub theme: String,
    /// Decimals lengths, areas and volumes are shown with: 2, 3 or 4 (as the drafting
    /// settings' precision).
    pub display_precision: u8,
    /// How far object snaps reach around the cursor, in screen pixels (AutoCAD's APERTURE):
    /// `MIN_PX` to `MAX_PX`.
    pub snap_aperture_px: u32,
    /// How far picking reaches around the cursor, in screen pixels (PICKBOX): `MIN_PX` to
    /// `MAX_PX`.
    pub pick_box_px: u32,
    /// The drawing area's background as "#rrggbb"; null is the theme's (Paper in light, Ink in
    /// dark).
    pub background_color: Option<String>,
    /// The crosshair's colour as "#rrggbb"; null is the theme's.
    pub crosshair_color: Option<String>,
    /// The colour picked and highlighted entities are drawn in, as "#rrggbb"; null is the
    /// theme's.
    pub highlight_color: Option<String>,
    /// The user's keyboard shortcuts, command to key ("Ctrl+Shift+C", "F3", "D"), over the
    /// built-in ones (`src/tools/shortcuts.ts`). Commands are the ribbon's (`ribbon`) and the
    /// shortcut actions (undo, redo, save, osnap...).
    pub shortcuts: BTreeMap<String, String>,
    /// The user's ribbon layout (tabs, groups, buttons; see `ribbon`); null is the default
    /// (`ribbon::default_ribbon`).
    pub ribbon: Option<RibbonLayout>,
    /// The user's command line aliases, alias to command (e.g. "L" to "LAYOFF"), over the
    /// built-in ones.
    pub aliases: BTreeMap<String, String>,
    /// Drawings opened or saved lately, most recent first, at most `MAX_RECENTS`. Kept by the app
    /// itself: the Settings dialog never changes it.
    pub recents: Vec<String>,
    /// The user's tool palette, in order. Changed only by the palette itself
    /// (`set_tool_palette`), never by the Settings dialog.
    pub tool_palette: Vec<PaletteItem>,
    /// Minutes between autosaves of drawings with unsaved changes (`<name>.sv$`); 0 turns
    /// autosave off. At most `MAX_AUTOSAVE_MINUTES`.
    pub autosave_minutes: u32,
    /// When updates were last checked, in seconds since 1970 (0: never). The launch check waits
    /// a day after it (`updater`). Kept by the app: the Settings dialog never changes it.
    pub last_update_check: u64,
}

impl Settings {
    pub fn effective_effort(&self) -> &str {
        self.profile()
            .effort
            .as_deref()
            .unwrap_or(match self.provider {
                Provider::Anthropic => "medium",
                Provider::OpenaiCompatible => "low",
                Provider::ChatgptPlan | Provider::ClaudeCode => "none",
            })
    }

    pub fn profile(&self) -> &ProviderProfile {
        match self.provider {
            Provider::Anthropic => &self.providers.anthropic,
            Provider::OpenaiCompatible => &self.providers.openai_compatible,
            Provider::ChatgptPlan => &self.providers.chatgpt_plan,
            Provider::ClaudeCode => &self.providers.claude_code,
        }
    }

    pub fn profile_mut(&mut self) -> &mut ProviderProfile {
        match self.provider {
            Provider::Anthropic => &mut self.providers.anthropic,
            Provider::OpenaiCompatible => &mut self.providers.openai_compatible,
            Provider::ChatgptPlan => &mut self.providers.chatgpt_plan,
            Provider::ClaudeCode => &mut self.providers.claude_code,
        }
    }

    /// Puts `path` first in the recent list, once (another spelling of the same file goes), and
    /// keeps the newest `MAX_RECENTS`.
    pub fn push_recent(&mut self, path: &str) {
        self.recents
            .retain(|p| !same_file(Path::new(p), Path::new(path)));
        self.recents.insert(0, path.to_string());
        self.recents.truncate(MAX_RECENTS);
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            max_concurrent_turns: 3,
            provider: Provider::Anthropic,
            claude_executable: None,
            providers: ProviderProfiles::default(),
            effort: "medium".into(),
            autonomy: qiyas_ai::Autonomy::default(),
            measurement_method: None,
            show_technical_details: false,
            mcp_enabled: false,
            mcp_port: 48123,
            mcp_token: new_token(),
            language: "en".into(),
            theme: "system".into(),
            display_precision: 3,
            snap_aperture_px: 10,
            pick_box_px: 6,
            background_color: None,
            crosshair_color: None,
            highlight_color: None,
            shortcuts: BTreeMap::new(),
            ribbon: None,
            aliases: BTreeMap::new(),
            recents: Vec::new(),
            tool_palette: Vec::new(),
            autosave_minutes: 5,
            last_update_check: 0,
        }
    }
}

/// Whether `a` and `b` name the same file: as written, or once resolved (`..`, links, case on
/// systems that ignore it).
pub fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (fs::canonicalize(a), fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

/// 16 random bytes as 32 lowercase hex characters.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the OS random number generator failed");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_token(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Checks settings coming from the window before they are kept.
pub fn validate(s: &Settings) -> Result<(), String> {
    if !(1..=8).contains(&s.max_concurrent_turns) {
        return Err("Mizan can run between 1 and 8 drawings at once".into());
    }

    if let Some(base) = s
        .providers
        .openai_compatible
        .base_url
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        qiyas_ai::endpoint::normalize(base)?;
    }
    if s.measurement_method
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err("The measurement method name is empty; choose None instead".into());
    }
    if s.mcp_port < 1024 {
        return Err(format!(
            "The MCP port must be between 1024 and 65535, not {}",
            s.mcp_port
        ));
    }
    if !EFFORTS.contains(&s.effort.as_str()) {
        return Err(format!("Unknown effort: {}", s.effort));
    }
    for (provider, profile) in [
        (Provider::Anthropic, &s.providers.anthropic),
        (Provider::OpenaiCompatible, &s.providers.openai_compatible),
    ] {
        if let Some(effort) = &profile.effort {
            let allowed = if provider == Provider::Anthropic {
                &EFFORTS[..]
            } else {
                &EFFORTS[..4]
            };
            if !allowed.contains(&effort.as_str()) {
                return Err("Choose a supported effort for this provider".into());
            }
        }
    }
    if s.provider == Provider::Anthropic && s.profile().model.trim().is_empty() {
        return Err("The model is empty".into());
    }
    if !valid_token(&s.mcp_token) {
        return Err("The MCP token must be 32 hex characters".into());
    }
    if !["en", "ar"].contains(&s.language.as_str()) {
        return Err(format!("Unknown language \"{}\"", s.language));
    }
    if !["system", "light", "dark"].contains(&s.theme.as_str()) {
        return Err(format!("Unknown theme \"{}\"", s.theme));
    }
    if s.autosave_minutes > MAX_AUTOSAVE_MINUTES {
        return Err(format!(
            "Autosave runs every {MAX_AUTOSAVE_MINUTES} minutes at most (0 turns it off)"
        ));
    }
    for (alias, command) in &s.aliases {
        if alias.is_empty() || alias.contains(char::is_whitespace) || command.trim().is_empty() {
            return Err(format!(
                "The alias \"{alias}\" needs one word and a command"
            ));
        }
    }
    validate_options(s)
}

/// The Options, Customize and Aliases dialogs' part of `validate`.
fn validate_options(s: &Settings) -> Result<(), String> {
    if !(2..=4).contains(&s.display_precision) {
        return Err("The display precision must be 2, 3 or 4 decimals".into());
    }
    for (what, px) in [
        ("snap aperture", s.snap_aperture_px),
        ("pick box", s.pick_box_px),
    ] {
        if !(MIN_PX..=MAX_PX).contains(&px) {
            return Err(format!(
                "The {what} must be {MIN_PX} to {MAX_PX} pixels, not {px}"
            ));
        }
    }
    for (what, color) in [
        ("background", &s.background_color),
        ("crosshair", &s.crosshair_color),
        ("highlight", &s.highlight_color),
    ] {
        if let Some(c) = color.as_deref().filter(|c| !valid_color(c)) {
            return Err(format!("The {what} colour \"{c}\" must be written #rrggbb"));
        }
    }
    if s.aliases.len() > MAX_ALIASES {
        return Err(format!("At most {MAX_ALIASES} aliases"));
    }
    let mut aliases = std::collections::HashSet::new();
    if let Some(alias) = s.aliases.keys().find(|a| !aliases.insert(a.to_uppercase())) {
        return Err(format!(
            "The alias \"{alias}\" is there twice (aliases ignore case)"
        ));
    }
    if s.shortcuts.len() > MAX_ALIASES {
        return Err(format!("At most {MAX_ALIASES} shortcuts"));
    }
    let mut keys = BTreeMap::new();
    for (command, key) in &s.shortcuts {
        if command.is_empty() || command.contains(char::is_whitespace) {
            return Err(format!(
                "The shortcut for \"{command}\" needs a command of one word"
            ));
        }
        let Some(chord) = shortcut_key(key) else {
            return Err(format!(
                "The shortcut \"{key}\" for {command} is not a key such as Ctrl+Shift+C, F3 or D"
            ));
        };
        if let Some(other) = keys.insert(chord, command) {
            return Err(format!(
                "{key} is the shortcut for both {other} and {command}"
            ));
        }
    }
    if let Some(layout) = &s.ribbon {
        crate::ribbon::validate(layout)?;
    }
    Ok(())
}

/// "#rrggbb".
fn valid_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// A shortcut such as "Ctrl+Shift+C" in one spelling (modifiers in the order Ctrl, Alt, Shift,
/// Meta; the key as `NAMED_KEYS` or upper case), so two spellings of one key compare equal;
/// `None` when it is not a key. Modifiers: Ctrl, Alt, Shift, Meta, each once; then a letter, a
/// digit, F1 to F24 or one of `NAMED_KEYS`, in any case.
pub fn shortcut_key(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last()?;
    let mut held = [false; 4];
    for m in modifiers {
        let i = ["ctrl", "alt", "shift", "meta"]
            .iter()
            .position(|n| m.eq_ignore_ascii_case(n))?;
        if std::mem::replace(&mut held[i], true) {
            return None;
        }
    }
    let upper = key.to_ascii_uppercase();
    let function = upper
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n) && !upper[1..].starts_with('0'));
    let key = if (key.len() == 1 && key.bytes().all(|b| b.is_ascii_alphanumeric())) || function {
        upper
    } else {
        NAMED_KEYS
            .iter()
            .find(|n| n.eq_ignore_ascii_case(key))?
            .to_string()
    };
    let mut chord: Vec<&str> = ["Ctrl", "Alt", "Shift", "Meta"]
        .iter()
        .zip(held)
        .filter(|(_, on)| *on)
        .map(|(n, _)| *n)
        .collect();
    chord.push(&key);
    Some(chord.join("+"))
}

/// Checks a tool palette before it is kept: at most `MAX_PALETTE` buttons, each with a label
/// and a block name or command.
pub fn validate_palette(items: &[PaletteItem]) -> Result<(), String> {
    if items.len() > MAX_PALETTE {
        return Err(format!(
            "A tool palette holds at most {MAX_PALETTE} buttons"
        ));
    }
    if items
        .iter()
        .any(|i| i.label.trim().is_empty() || i.value.trim().is_empty())
    {
        return Err("Each palette button needs a label and a block or command".into());
    }
    Ok(())
}

/// Reads the settings in `dir`. A missing file gives the defaults; a file that can't be read as
/// settings is kept as `settings.json.bak` and the defaults are used. Whenever the defaults or a
/// new MCP token were used, the result is written back so the token stays the same next time.
pub fn load(dir: &Path) -> Settings {
    let path = dir.join(FILE);
    let (mut settings, mut write) = match fs::read_to_string(&path) {
        Ok(text) => match read_settings(&text) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("settings.json could not be read ({e}); using the defaults");
                if let Err(e) = fs::rename(&path, dir.join(format!("{FILE}.bak"))) {
                    eprintln!("settings.json could not be backed up: {e}");
                }
                (Settings::default(), true)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Settings::default(), true),
        Err(e) => {
            eprintln!("settings.json could not be read ({e}); using the defaults");
            return Settings::default();
        }
    };
    if !valid_token(&settings.mcp_token) {
        settings.mcp_token = new_token();
        write = true;
    }
    // A ribbon this version can't draw (a button gone since) gives way to the default.
    if let Some(Err(e)) = settings.ribbon.as_ref().map(crate::ribbon::validate) {
        eprintln!("The saved ribbon layout is not used ({e}); using the default");
        settings.ribbon = None;
        write = true;
    }
    if write {
        if let Err(e) = save(dir, &settings) {
            eprintln!("settings.json could not be written: {e}");
        }
    }
    // Files from before they were written 0600 hold the token too.
    for name in [FILE.to_string(), format!("{FILE}.bak")] {
        if let Err(e) = owner_only(&dir.join(&name)) {
            eprintln!("{name} could not be made private: {e}");
        }
    }
    settings
}

/// Moves the old flat model and address into the selected provider's profile. No keychain
/// access is involved. A profile already present wins over obsolete flat fields.
fn read_settings(text: &str) -> Result<(Settings, bool), serde_json::Error> {
    let mut value: serde_json::Value = serde_json::from_str(text)?;
    let mut migrated = false;
    if let Some(object) = value.as_object_mut() {
        let model = object.remove("model");
        let base_url = object.remove("base_url");
        migrated = model.is_some() || base_url.is_some();
        if migrated {
            let provider = object
                .get("provider")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("anthropic")
                .to_string();
            let profiles = object
                .entry("providers")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(profiles) = profiles.as_object_mut() {
                profiles.entry(provider.clone()).or_insert_with(|| {
                    let mut profile = serde_json::Map::new();
                    profile.insert(
                        "model".into(),
                        model.unwrap_or_else(|| {
                            if provider == "anthropic" {
                                anthropic_profile().model.into()
                            } else {
                                String::new().into()
                            }
                        }),
                    );
                    if provider == "openai_compatible" {
                        if let Some(base_url) = base_url {
                            profile.insert("base_url".into(), base_url);
                        }
                    }
                    serde_json::Value::Object(profile)
                });
            }
        }
    }
    if let Some(legacy) = value["effort"].as_str().map(str::to_string) {
        let provider = value["provider"]
            .as_str()
            .unwrap_or("anthropic")
            .to_string();
        // The legacy effort belonged to API providers. A subscription uses its own
        // default until the user explicitly chooses a supported level.
        if matches!(provider.as_str(), "anthropic" | "openai_compatible")
            && value["providers"][&provider]
                .as_object()
                .is_none_or(|p| !p.contains_key("effort"))
        {
            if value["providers"].is_null() {
                value["providers"] = serde_json::json!({});
            }
            if value["providers"][&provider].is_null() {
                value["providers"][&provider] = serde_json::to_value(if provider == "anthropic" {
                    anthropic_profile()
                } else {
                    ProviderProfile::default()
                })
                .expect("provider profile");
            }
            let effort =
                if provider == "openai_compatible" && matches!(legacy.as_str(), "xhigh" | "max") {
                    "high".into()
                } else {
                    legacy
                };
            value["providers"][&provider]["effort"] = serde_json::json!(effort);
            migrated = true;
        }
    }
    serde_json::from_value(value).map(|settings| (settings, migrated))
}

/// On Unix, lets only the user read and write `path`, if it exists.
fn owner_only(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(meta) if meta.permissions().mode() & 0o777 != 0o600 => {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            }
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Writes the settings to `dir` atomically: a temporary file, then a rename over the old one. On
/// Unix only the user can read the file, since it holds the MCP token.
pub fn save(dir: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    let text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    // A temporary file left by a crash keeps its old mode; set it again.
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    file.write_all(text.as_bytes())?;
    drop(file);
    fs::rename(&tmp, dir.join(FILE))
}
