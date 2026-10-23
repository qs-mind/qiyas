//! Command scripts: a `.scr` file run without the window (`qiyas-cli run` and `batch`; the
//! ribbon's Run script later). One command per line; a command's answers (its points) on the same
//! line or the lines after it; a line starting with `;` is a comment; blank lines are skipped.
//! The words are AutoCAD-style with typed points as the window's command line takes them (`x,y`,
//! `@dx,dy`, `@d<angle` in degrees, drawing units, world coordinates, Arabic digits accepted),
//! and a line starting with `{` is a command in its wire form (`{"type":"draw","kind":"line",
//! ...}`, exactly what `Command` reads), so every command of the bus can be scripted without a
//! second parser. Words outside this file's legacy grammar use `cmdline::Machine`, the same
//! schema-driven prompts as the window and AI.
//!
//! The whole script is read and checked before anything runs: words, points, handles and paths.
//! A relative path is taken from the script's folder (a written one from `Context::out_dir` when
//! given) and must stay inside the script's folder, the drawing's folder or the out folder, as
//! the AI tools and WBLOCK refuse files elsewhere; a JSON line's paths (`path`, `from_path`) go
//! through the same rule. `{name}` in a path is the drawing's file name without its extension and
//! `{ext}` its extension, so one script exports `{name}.{ext}.xlsx` (a bare EXPORT) for every
//! drawing of a folder. A UTF-8 byte order mark at the start is skipped.
//! Nothing is written except by SAVE and SAVEAS (which save the project file beside the drawing
//! as the app does; SAVE keeps a `.bak`, SAVEAS to a new path has nothing to keep), EXPORT and a
//! JSON `write_block`. SAVE refuses a drawing the failsafe reader repaired on open unless the host
//! allows it. Headless runs never touch settings, the keychain, MCP, the AI, or the app's
//! autosaves and lock files (`Session::save_as_headless`).

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::{Boq, Command, Current, Kind, LayerProp, Outcome, Reported, Session, Source};

/// Where a script runs: the script file (relative paths are taken from its folder), the drawing
/// it starts on when there is one, and where its written files go.
pub struct Context<'a> {
    pub script: &'a Path,
    /// The drawing open when the script starts; `None` when the script must OPEN one.
    pub drawing: Option<&'a Path>,
    /// Where relative written paths (SAVEAS, EXPORT) go; the script's folder when `None`.
    pub out_dir: Option<&'a Path>,
}

/// One thing a script line asks for.
#[derive(Debug, Clone)]
pub enum Action {
    Open(PathBuf),
    SaveAs(PathBuf),
    Save,
    /// Ends the run; the lines after it are checked but never run.
    Quit,
    /// Waits this many milliseconds.
    Delay(u64),
    /// LAYER CURRENT: makes the layer current, keeping the other current properties.
    CurrentLayer(String),
    /// The take-off to an Excel workbook (`Host::export_takeoff`).
    Export(PathBuf),
    /// A BOQ from an Excel workbook (`Host::import_boq`), then `Command::SetBoq`.
    ImportBoq(PathBuf),
    Compare,
    /// ZOOM: nothing to do without a window.
    Zoom,
    Echo(String),
    /// A command of the bus: a word turned into one, or a JSON line.
    Command(Command),
}

#[derive(Debug, Clone)]
pub struct Step {
    /// The line of the script it came from (the command's line), from 1.
    pub line: usize,
    pub action: Action,
    /// Original command input after resolving file paths, before serde supplies defaults.
    pub input: Option<serde_json::Value>,
}

/// A script read and checked whole.
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub steps: Vec<Step>,
}

/// What stopped a script, and on which line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

/// The Excel and comparison steps, which live outside this crate (`qiyas-boq`, `qiyas-takeoff`):
/// the program running the script supplies them.
pub trait Host {
    fn deliverable(&mut self, _export: &crate::deliverables::Export) -> Result<(), String> {
        Err("This script host does not support deliverables".into())
    }
    /// Writes the session's take-off to the workbook at `path`.
    fn export_takeoff(&mut self, session: &Session, path: &Path) -> Result<(), String>;
    /// Reads the BOQ in the workbook at `path`.
    fn import_boq(&mut self, path: &Path) -> Result<Boq, String>;
    /// Compares the session's BOQ and take-off with the drawing and returns how many conflicts.
    fn compare(&mut self, session: &mut Session) -> Result<usize, String>;
    /// Whether SAVE (or SAVEAS onto the drawing's own file) may write over a drawing the failsafe
    /// reader repaired when it opened it (`qiyas-cli --allow-repaired-save`). No by default: a
    /// run never puts what the reader made of a damaged file over the original and its `.bak`.
    fn allow_repaired_save(&self) -> bool {
        false
    }
}

/// One step run, and what it did in a line.
#[derive(Debug, Clone, Serialize)]
pub struct Done {
    pub line: usize,
    pub text: String,
}

/// What a run did: each step's line, the block counts (COUNT, by block name), the drawings it
/// worked on that the failsafe reader had to repair (`<path> opened with N problems`) or whose
/// project file could not be read (`<path>: Project file could not be read …`) and the
/// session as the script left it (`None` when it never opened a drawing).
#[derive(Serialize)]
pub struct Run {
    pub steps: Vec<Done>,
    pub counts: BTreeMap<String, f64>,
    pub warnings: Vec<String>,
    #[serde(skip)]
    pub session: Option<Session>,
}

/// Reads and checks the whole of `text`; the first bad line is the error.
pub fn parse(text: &str, cx: &Context) -> Result<Script, ScriptError> {
    let mut parser = Parser::new(cx);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = i + 1;
        let t = lines[i].trim();
        i += 1;
        if t.is_empty() || t.starts_with(';') {
            continue;
        }
        parser.line(t, line, &lines, &mut i)?;
    }
    Ok(Script {
        steps: parser.steps,
    })
}

/// Resolves a command's paths, including commands in a batch, through the script's folder rule.
/// Paths become absolute. `write_block` uses the output folder when one is given.
/// Errors carry `line`, as they do when parsing a script.
pub fn resolve_command_paths(
    value: &mut serde_json::Value,
    cx: &Context,
    line: usize,
) -> Result<(), ScriptError> {
    Parser::new(cx).json_paths(line, value)
}

/// Runs `script` on `session` (none: the script must OPEN a drawing first), stopping at QUIT or
/// at the first step that fails.
pub fn run(
    script: &Script,
    session: Option<Session>,
    host: &mut dyn Host,
) -> Result<Run, ScriptError> {
    let mut run = Run {
        steps: Vec::new(),
        counts: BTreeMap::new(),
        warnings: Vec::new(),
        session,
    };
    // The drawing open is one the reader repaired: SAVE would put that over the original.
    let mut repaired = false;
    if let Some(session) = &run.session {
        repaired = warn_if_repaired(session, &mut run.warnings);
    }
    for step in &script.steps {
        let line = step.line;
        let fail = |message: String| ScriptError { line, message };
        let text = match &step.action {
            Action::Quit => {
                run.steps.push(Done {
                    line,
                    text: "quit".to_string(),
                });
                break;
            }
            Action::Open(path) => {
                let session = Session::open(path).map_err(|e| fail(e.to_string()))?;
                repaired = warn_if_repaired(&session, &mut run.warnings);
                let problems = read_problems(&session);
                run.session = Some(session);
                match problems {
                    0 => format!("opened {}", path.display()),
                    n => format!("opened {} ({})", path.display(), problems_reading(n)),
                }
            }
            Action::Delay(ms) => {
                std::thread::sleep(Duration::from_millis(*ms));
                format!("waited {ms} ms")
            }
            Action::Echo(text) => text.clone(),
            Action::Zoom => "zoom: nothing to do without a window".to_string(),
            Action::SaveAs(path) => {
                let session = open(&mut run.session, line)?;
                if repaired && *path == real_or_lexical(&session.drawing.path) {
                    refuse_repaired(host, line)?;
                }
                session
                    .save_as_headless(path)
                    .map_err(|e| fail(e.to_string()))?;
                repaired = false;
                format!("saved {}", path.display())
            }
            Action::Save => {
                let session = open(&mut run.session, line)?;
                if repaired {
                    refuse_repaired(host, line)?;
                }
                let path = session.drawing.path.clone();
                session
                    .save_as_headless(&path)
                    .map_err(|e| fail(e.to_string()))?;
                format!("saved {}", path.display())
            }
            Action::CurrentLayer(name) => {
                let session = open(&mut run.session, line)?;
                let current = Current {
                    layer: name.clone(),
                    ..session.current.clone()
                };
                session
                    .run(Command::SetCurrent { current })
                    .map_err(|e| fail(e.to_string()))?;
                format!("current layer {name}")
            }
            Action::Export(path) => {
                let session = open(&mut run.session, line)?;
                host.export_takeoff(session, path).map_err(fail)?;
                format!("exported {}", path.display())
            }
            Action::ImportBoq(path) => {
                let session = open(&mut run.session, line)?;
                let boq = host.import_boq(path).map_err(fail)?;
                let items = boq.items.len();
                session
                    .run(Command::SetBoq { boq })
                    .map_err(|e| fail(e.to_string()))?;
                format!("imported {items} BOQ items from {}", path.display())
            }
            Action::Compare => {
                let session = open(&mut run.session, line)?;
                let conflicts = host.compare(session).map_err(fail)?;
                format!("{conflicts} conflicts")
            }
            Action::Command(cmd) => {
                let session = open(&mut run.session, line)?;
                let input = step.input.clone().unwrap_or_else(|| serde_json::json!(cmd));
                let outcome = session
                    .run_input(&input, Source::User)
                    .map_err(|e| fail(e.to_string()))?;
                if let Outcome::Deliverable { export } = &outcome.outcome {
                    host.deliverable(export).map_err(fail)?;
                }
                describe(cmd, outcome, &mut run.counts)
            }
        };
        run.steps.push(Done { line, text });
    }
    Ok(run)
}

/// The open session, or why the step on `line` can't run.
fn open(session: &mut Option<Session>, line: usize) -> Result<&mut Session, ScriptError> {
    session.as_mut().ok_or_else(|| ScriptError {
        line,
        message: "no drawing is open: OPEN one first".to_string(),
    })
}

/// How many records the failsafe reader dropped or repaired reading the session's drawing (its
/// diagnostics; the project file's problems are not counted).
pub fn read_problems(session: &Session) -> usize {
    session.drawing.report.dropped.iter().map(|d| d.count).sum()
}

/// "1 problem reading it", "3 problems reading it".
pub fn problems_reading(n: usize) -> String {
    match n {
        1 => "1 problem reading it".to_string(),
        n => format!("{n} problems reading it"),
    }
}

/// The session's problems with its project file: one that is there but could not be read (it
/// opened with an empty take-off; `Session::open` backed the file up).
pub fn project_problems(session: &Session) -> Vec<String> {
    session
        .drawing
        .problems
        .iter()
        .filter(|p| p.starts_with("Project file could not be read"))
        .cloned()
        .collect()
}

/// Notes in `warnings` that the session's drawing was repaired on open, or that its project file
/// could not be read; whether the drawing was repaired (only that stops SAVE).
fn warn_if_repaired(session: &Session, warnings: &mut Vec<String>) -> bool {
    for problem in project_problems(session) {
        warnings.push(format!("{}: {problem}", session.drawing.path.display()));
    }
    let n = read_problems(session);
    if n > 0 {
        let s = if n == 1 { "" } else { "s" };
        warnings.push(format!(
            "{} opened with {n} problem{s}",
            session.drawing.path.display()
        ));
    }
    n > 0
}

/// SAVE over a drawing the reader repaired, unless the host allows it.
fn refuse_repaired(host: &dyn Host, line: usize) -> Result<(), ScriptError> {
    if host.allow_repaired_save() {
        return Ok(());
    }
    Err(ScriptError {
        line,
        message: "the drawing was repaired on open; use SAVEAS to a new path or pass \
                  --allow-repaired-save"
            .to_string(),
    })
}

/// What `cmd` did, in a line; a count goes into `counts` by block name.
fn describe(cmd: &Command, outcome: Reported, counts: &mut BTreeMap<String, f64>) -> String {
    let notes = outcome.input_notes();
    let text = match outcome.outcome {
        Outcome::Measured { measurement: m } => {
            let shown = match m.kind {
                Kind::Count => format!("{:.0}", m.shown),
                _ => format!("{:.3}", m.shown),
            };
            if m.kind == Kind::Count {
                let name = m
                    .block_name
                    .clone()
                    .unwrap_or_else(|| m.description.clone());
                counts.insert(name, m.shown);
            }
            format!("{} = {shown} {}", m.description, m.unit)
        }
        Outcome::Changed => match cmd {
            Command::SetLayerHidden { name, hidden } => {
                format!("layer {name} {}", if *hidden { "off" } else { "on" })
            }
            Command::SetLayer {
                name,
                prop: LayerProp::Frozen,
                value,
            } => format!(
                "layer {name} {}",
                if value.as_bool() == Some(true) {
                    "frozen"
                } else {
                    "thawed"
                }
            ),
            _ => "done".to_string(),
        },
        Outcome::Added { handles } => format!("added {} entities", handles.len()),
        Outcome::TitleBlock {
            handles,
            empty_fields,
        } => format!(
            "title block: {} entities; empty fields: {}",
            handles.len(),
            empty_fields.join(", ")
        ),
        Outcome::Batch { outcomes, added } => format!(
            "ran {} commands as one step, added {} entities",
            outcomes.len(),
            added.len()
        ),
        Outcome::Nothing => "nothing to do".to_string(),
        other => format!("{other:?}"),
    };
    if notes.is_empty() {
        text
    } else {
        format!("{text}; {notes}")
    }
}

struct Parser {
    script_dir: PathBuf,
    out_dir: Option<PathBuf>,
    /// The drawing open at this point of the script: the one it started on, then the last OPEN
    /// or SAVEAS.
    drawing: Option<PathBuf>,
    /// The last point typed, for `@`.
    last: Option<[f64; 2]>,
    steps: Vec<Step>,
}

fn at(line: usize, message: impl Into<String>) -> ScriptError {
    ScriptError {
        line,
        message: message.into(),
    }
}

impl Parser {
    fn new(cx: &Context) -> Self {
        let script_dir = cx
            .script
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        Self {
            script_dir: real_or_lexical(script_dir),
            out_dir: cx.out_dir.map(real_or_lexical),
            drawing: cx.drawing.map(real_or_lexical),
            last: None,
            steps: Vec::new(),
        }
    }

    /// Reads the command on `text` (line `line`, not blank, not a comment); a measure with its
    /// points on the lines after it reads on through `lines`, moving `i` past them.
    fn line(
        &mut self,
        text: &str,
        line: usize,
        lines: &[&str],
        i: &mut usize,
    ) -> Result<(), ScriptError> {
        if text.starts_with('{') {
            let mut value: serde_json::Value =
                serde_json::from_str(text).map_err(|e| at(line, e.to_string()))?;
            self.json_paths(line, &mut value)?;
            let cmd = crate::registry::parse_command(value.clone()).map_err(|e| at(line, e))?;
            self.push_input(line, cmd, value);
            return Ok(());
        }
        let (first, rest) = match text.find(char::is_whitespace) {
            Some(n) => (&text[..n], text[n..].trim()),
            None => (text, ""),
        };
        let word = first.to_ascii_uppercase();
        let args = tokenize(rest);
        let nothing_after = |what: &str| -> Result<(), ScriptError> {
            if args.is_empty() {
                Ok(())
            } else {
                Err(at(line, format!("{what} takes nothing after it")))
            }
        };
        match word.as_str() {
            "OPEN" => {
                let path = self.path(line, &one_name(line, rest, "OPEN takes a path")?, false)?;
                self.drawing = Some(path.clone());
                self.push(line, Action::Open(path));
            }
            "SAVEAS" => {
                let path = self.save_as_path(line, &args)?;
                self.drawing = Some(path.clone());
                self.push(line, Action::SaveAs(path));
            }
            "SAVE" => {
                nothing_after("SAVE")?;
                self.push(line, Action::Save);
            }
            "QUIT" => {
                nothing_after("QUIT")?;
                self.push(line, Action::Quit);
            }
            "DELAY" => {
                let ms = match args.as_slice() {
                    [ms] => number(ms).filter(|n| *n >= 0.0 && n.fract() == 0.0),
                    _ => None,
                }
                .ok_or_else(|| at(line, "DELAY takes a whole number of milliseconds"))?;
                self.push(line, Action::Delay(ms as u64));
            }
            "LAYOFF" | "LAYON" => {
                if args.is_empty() {
                    return Err(at(line, format!("{word} takes one or more layer names")));
                }
                for name in args {
                    let hidden = word == "LAYOFF";
                    self.push(
                        line,
                        Action::Command(Command::SetLayerHidden { name, hidden }),
                    );
                }
            }
            "LAYFRZ" | "LAYTHW" => {
                if args.is_empty() {
                    return Err(at(line, format!("{word} takes one or more layer names")));
                }
                for name in args {
                    let frozen = word == "LAYFRZ";
                    self.push(
                        line,
                        Action::Command(Command::SetLayer {
                            name,
                            prop: LayerProp::Frozen,
                            value: serde_json::Value::Bool(frozen),
                        }),
                    );
                }
            }
            "LAYER" => {
                let name = match args.first() {
                    Some(how) if how.eq_ignore_ascii_case("CURRENT") => one_name(
                        line,
                        rest[how.len()..].trim(),
                        "LAYER CURRENT takes a layer name",
                    ),
                    _ => Err(at(line, "LAYER takes CURRENT <name>")),
                }?;
                self.push(line, Action::CurrentLayer(name));
            }
            "COUNT" => {
                let block_name = one_name(line, rest, "COUNT takes a block name")?;
                self.push(
                    line,
                    Action::Command(Command::CountBlock {
                        block_name,
                        layer: None,
                    }),
                );
            }
            "DI" | "DIST" => {
                let points = self.points(line, &args, lines, i, Some(2))?;
                if points.len() != 2 {
                    return Err(at(
                        line,
                        format!("DIST takes 2 points, {} given", points.len()),
                    ));
                }
                self.push(
                    line,
                    Action::Command(Command::MeasureDistance {
                        points: points.into(),
                    }),
                );
            }
            "AA" | "AREA" => {
                let points = self.points(line, &args, lines, i, None)?;
                if points.len() < 3 {
                    return Err(at(
                        line,
                        format!(
                            "AREA needs at least 3 points, {} given (a blank line ends them)",
                            points.len()
                        ),
                    ));
                }
                self.push(
                    line,
                    Action::Command(Command::MeasureArea {
                        points: points.into(),
                    }),
                );
            }
            "MEASURE" => {
                let handle = match args.as_slice() {
                    [h] => u64::from_str_radix(&normalize_digits(h), 16).ok(),
                    _ => None,
                }
                .ok_or_else(|| at(line, "MEASURE takes an entity handle in hex"))?;
                self.push(
                    line,
                    Action::Command(Command::MeasureEntity {
                        handle,
                        chain: Vec::new(),
                    }),
                );
            }
            "EXPORT" => {
                let text = if rest.is_empty() {
                    if self.drawing.is_none() {
                        return Err(at(
                            line,
                            "EXPORT without a path is {name}.{ext}.xlsx: no drawing is open yet",
                        ));
                    }
                    "{name}.{ext}.xlsx".to_string()
                } else {
                    one_name(line, rest, "EXPORT takes the workbook's path")?
                };
                let path = self.path(line, &text, true)?;
                self.push(line, Action::Export(path));
            }
            "IMPORTBOQ" => {
                let path = self.path(
                    line,
                    &one_name(line, rest, "IMPORTBOQ takes the workbook's path")?,
                    false,
                )?;
                self.push(line, Action::ImportBoq(path));
            }
            "COMPARE" => {
                nothing_after("COMPARE")?;
                self.push(line, Action::Compare);
            }
            "ZOOM" => {
                let how = args.first().map(|a| a.to_ascii_uppercase());
                match how.as_deref() {
                    Some("E" | "EXTENTS" | "IN" | "OUT") if args.len() == 1 => {
                        self.push(line, Action::Zoom)
                    }
                    _ => return Err(at(line, "ZOOM takes E (extents), IN or OUT")),
                }
            }
            "ECHO" => self.push(line, Action::Echo(rest.to_string())),
            _ => {
                if crate::registry::registry().by_word(first).is_none() {
                    return Err(at(line, format!("unknown command {first}")));
                }
                self.typed(text, line, lines, i)?;
            }
        }
        Ok(())
    }

    /// Registry words keep the legacy script's whole-file validation and continuation lines.
    fn typed(
        &mut self,
        text: &str,
        line: usize,
        lines: &[&str],
        i: &mut usize,
    ) -> Result<(), ScriptError> {
        use crate::cmdline::{Machine, Step as TypedStep};

        let mut machine = Machine::default();
        machine.set_last(self.last);
        let mut on = line;
        let mut answer = machine.step(text).map_err(|e| at(on, e))?;
        loop {
            match answer {
                TypedStep::Command(_) => {
                    let mut value = machine
                        .take_input()
                        .expect("a completed command has wire input");
                    self.json_paths(line, &mut value)?;
                    let command =
                        crate::registry::parse_command(value.clone()).map_err(|e| at(line, e))?;
                    self.last = machine.last();
                    self.push_input(line, command, value);
                    return Ok(());
                }
                TypedStep::Cancelled => {
                    self.last = machine.last();
                    return Ok(());
                }
                TypedStep::Prompt(prompt) => {
                    while *i < lines.len() && lines[*i].trim().starts_with(';') {
                        *i += 1;
                    }
                    let eof = *i == lines.len();
                    let next = if eof { "" } else { lines[*i].trim() };
                    if !eof {
                        *i += 1;
                        on = *i;
                    }
                    answer = machine.step(next).map_err(|e| at(on, e))?;
                    if next.is_empty() && matches!(answer, TypedStep::Prompt(_)) {
                        return Err(at(line, format!("incomplete command: {prompt}")));
                    }
                }
            }
        }
    }

    /// A JSON line's paths (`path`, `from_path`, any `..._path`) through the folder rule, made
    /// absolute so they never depend on where the script is run from; `write_block`'s is written.
    fn json_paths(&self, line: usize, value: &mut serde_json::Value) -> Result<(), ScriptError> {
        let Some(fields) = value.as_object_mut() else {
            return Ok(());
        };
        let written = fields.get("type").and_then(|t| t.as_str()) == Some("write_block");
        for (key, field) in fields.iter_mut() {
            // A batch's commands go through the same rule.
            if key == "commands" {
                if let Some(commands) = field.as_array_mut() {
                    for command in commands {
                        self.json_paths(line, command)?;
                    }
                }
                continue;
            }
            if key != "path" && !key.ends_with("_path") {
                continue;
            }
            if let Some(text) = field.as_str() {
                let path = self.path(line, text, written)?;
                *field = serde_json::Value::String(path.to_string_lossy().into_owned());
            }
        }
        Ok(())
    }

    fn push(&mut self, line: usize, action: Action) {
        let input = match &action {
            Action::Command(command) => Some(serde_json::json!(command)),
            _ => None,
        };
        self.steps.push(Step {
            line,
            action,
            input,
        });
    }

    fn push_input(&mut self, line: usize, command: Command, input: serde_json::Value) {
        self.steps.push(Step {
            line,
            action: Action::Command(command),
            input: Some(input),
        });
    }

    /// The points of a measure: `first` are the words after the command on its line, then the
    /// lines after it. With `exactly`, reading stops once that many are read; without, a blank
    /// line or the end of the script ends them. A word that is no point is an error on its line.
    fn points(
        &mut self,
        line: usize,
        first: &[String],
        lines: &[&str],
        i: &mut usize,
        exactly: Option<usize>,
    ) -> Result<Vec<[f64; 2]>, ScriptError> {
        let mut points = Vec::new();
        let mut words: Vec<String> = first.to_vec();
        let mut on = line;
        loop {
            for word in &words {
                if exactly.is_some_and(|n| points.len() >= n) {
                    return Err(at(
                        on,
                        format!("{word}: more points than the command takes"),
                    ));
                }
                let p = self
                    .point(word)
                    .map_err(|e| at(on, e))?
                    .ok_or_else(|| at(on, format!("{word} is not a point")))?;
                points.push(p);
            }
            if exactly.is_some_and(|n| points.len() >= n) || *i >= lines.len() {
                return Ok(points);
            }
            let t = lines[*i].trim();
            *i += 1;
            on = *i;
            if t.starts_with(';') {
                words.clear();
                continue;
            }
            if t.is_empty() {
                return Ok(points);
            }
            words = tokenize(t);
        }
    }

    /// `word` as a typed point: `None` when it doesn't look like one, an error when it is a
    /// broken one. Moves `last` on.
    fn point(&mut self, word: &str) -> Result<Option<[f64; 2]>, String> {
        let t = normalize_digits(word.trim());
        if !t.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | '@')) {
            return Ok(None);
        }
        let relative = t.starts_with('@');
        let body = t.strip_prefix('@').unwrap_or(&t).trim();
        let last = match (relative, self.last) {
            (false, _) => [0.0, 0.0],
            (true, Some(last)) => last,
            (true, None) => return Err(format!("{word}: no point before it for @")),
        };
        let bad = || format!("{word} is not a point");
        let p = if relative && body.contains('<') {
            let (d, a) = body.split_once('<').ok_or_else(bad)?;
            let (d, a) = (number(d).ok_or_else(bad)?, number(a).ok_or_else(bad)?);
            let rad = a.to_radians();
            [last[0] + d * rad.cos(), last[1] + d * rad.sin()]
        } else {
            let (x, y) = body.split_once(',').ok_or_else(bad)?;
            let (x, y) = (number(x).ok_or_else(bad)?, number(y).ok_or_else(bad)?);
            [last[0] + x, last[1] + y]
        };
        self.last = Some(p);
        Ok(Some(p))
    }

    /// SAVEAS's path: `<path>` with a .dwg or .dxf extension, or `<path> dwg|dxf` giving it one.
    fn save_as_path(&self, line: usize, args: &[String]) -> Result<PathBuf, ScriptError> {
        let usage = || {
            at(line, "SAVEAS takes a path and, when the path has no extension, dwg or dxf; quote a path with spaces")
        };
        let is_format = |w: &str| w.eq_ignore_ascii_case("dwg") || w.eq_ignore_ascii_case("dxf");
        let (text, format) = match args {
            [p] => (p.as_str(), None),
            [p, f] if is_format(f) => (p.as_str(), Some(f.to_ascii_lowercase())),
            _ => return Err(usage()),
        };
        let mut path = self.path(line, text, true)?;
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase());
        match (ext.as_deref(), format) {
            (Some(e), Some(f)) if e != f => {
                return Err(at(
                    line,
                    format!("{text} is a .{e} file: leave the format out or save it as .{f}"),
                ))
            }
            (None, Some(f)) => {
                path.set_extension(f);
            }
            (Some("dwg" | "dxf"), _) => {}
            _ => {
                return Err(at(
                    line,
                    "SAVEAS writes .dwg or .dxf: give the extension or the format",
                ))
            }
        }
        Ok(path)
    }

    /// `text` as a real path inside the script's folder, the drawing's folder or the out folder
    /// (relative from the script's folder; a written file from the out folder when there is one),
    /// with `{name}` replaced by the drawing's file name without extension and `{ext}` by its
    /// extension.
    fn path(&self, line: usize, text: &str, written: bool) -> Result<PathBuf, ScriptError> {
        let text = if text.contains("{name}") || text.contains("{ext}") {
            let drawing = self.drawing.as_deref().ok_or_else(|| {
                at(
                    line,
                    "{name} and {ext} have no value: no drawing is open yet",
                )
            })?;
            let part = |p: Option<&std::ffi::OsStr>| {
                p.map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            text.replace("{name}", &part(drawing.file_stem()))
                .replace("{ext}", &part(drawing.extension()))
        } else {
            text.to_string()
        };
        if text.is_empty() {
            return Err(at(line, "a path is missing"));
        }
        let base = match (&self.out_dir, written) {
            (Some(out), true) => out,
            _ => &self.script_dir,
        };
        let full = real_or_lexical(&base.join(&text));
        let drawing_dir = self.drawing.as_deref().and_then(Path::parent);
        let roots = [
            Some(self.script_dir.as_path()),
            drawing_dir,
            self.out_dir.as_deref(),
        ];
        if roots
            .into_iter()
            .flatten()
            .any(|root| full.starts_with(root))
        {
            Ok(full)
        } else {
            Err(at(
                line,
                format!("{text} is outside the script's folder and the drawing's folder: a script only touches files there"),
            ))
        }
    }
}

/// `path` with links followed where it exists; where it doesn't yet (a file to be written), its
/// folder is followed and the name kept; else `.` and `..` worked out from the text alone.
fn real_or_lexical(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
        if let Ok(real) = dir.canonicalize() {
            return real.join(name);
        }
    }
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Western digits, `.` and `,` for Arabic-Indic and Eastern Arabic-Indic digits, `٫` and `،`.
fn normalize_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '٠'..='٩' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            '۰'..='۹' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
            '٫' => '.',
            '،' => ',',
            c => c,
        })
        .collect()
}

/// A plain decimal number in either script (no exponent, no inf or nan).
fn number(text: &str) -> Option<f64> {
    let t = normalize_digits(text.trim());
    let body = t.strip_prefix(['-', '+']).unwrap_or(&t);
    let plain = body.chars().any(|c| c.is_ascii_digit())
        && body.chars().all(|c| c.is_ascii_digit() || c == '.')
        && body.matches('.').count() <= 1;
    plain.then(|| t.parse().ok()).flatten()
}

/// Splits on spaces; double quotes keep a name with spaces in one word.
fn tokenize(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' {
            chars.next();
            let mut word = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                word.push(c);
            }
            words.push(word);
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
            words.push(word);
        }
    }
    words
}

/// The rest of a line as one name: unquoted when the whole of it is quoted; `usage` when empty.
fn one_name(line: usize, rest: &str, usage: &str) -> Result<String, ScriptError> {
    let t = rest.trim();
    let name = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    };
    if name.is_empty() {
        return Err(at(line, usage));
    }
    Ok(name.to_string())
}
