//! `qiyas-cli`: Qiyas without a window. Converts drawings between DWG and DXF, runs command
//! scripts (`qiyas_core::script`), counts blocks into a take-off workbook, and runs one script on
//! every drawing of a folder or a sheet set with a line per drawing.
//!
//! It opens drawings as the app does (the failsafe reader, the `.qiyas` project file beside a
//! drawing when there is one) and writes only what it is asked to: a converted drawing (alone,
//! no project file), a script's SAVE, SAVEAS and EXPORT, a take-off workbook. It never opens a
//! window. Only explicitly approved `eval` runs access provider settings, keychain and AI.
//! Ordinary commands never touch these, or the autosaves and
//! lock files beside drawings. Exit codes: 0 done; 1 an error (one line on stderr,
//! `qiyas-cli: ...`, a mistake in the arguments too); 2 done, but a drawing opened with problems
//! the reader repaired (a warning, one line on stderr).

mod bench;
mod eval;
mod eval_agent;
mod headless;

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use clap::error::ErrorKind;
use clap::{ArgGroup, Parser, Subcommand};
use opencadcodec::{DwgWriter, DxfVersion, DxfWriter};
use qiyas_core::script::{self, Action, Context, Host, Run, Script};
use qiyas_core::{Boq, Command, Drawing, Outcome, Session, SheetSet, Source};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "qiyas-cli",
    version,
    about = "Qiyas without a window: convert drawings, run command scripts, take off, batch."
)]
struct Cli {
    /// Print nothing but errors and warnings (on stderr), and a batch's summary.
    #[arg(long, global = true)]
    quiet: bool,
    /// Print the result as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Export quantities from a drawing or a saved project using a data-defined layout.
    Export {
        source: PathBuf,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        format: String,
        #[arg(long)]
        path: String,
        #[arg(long)]
        layout: Option<String>,
        #[arg(long, default_value = "en")]
        language: String,
    },
    /// Run a project take-off and write its workbook and morning report.
    Project {
        #[command(subcommand)]
        command: ProjectCli,
    },
    /// Carry A's knowledge onto B, remeasure, cloud changes and export variation quantities.
    Variation {
        a: PathBuf,
        b: PathBuf,
        #[arg(long)]
        export: PathBuf,
        /// Save B with its knowledge and clouds to a new drawing.
        #[arg(long)]
        drawing_output: Option<PathBuf>,
    },
    /// Generate, run and score the local development QS benchmark.
    Bench(bench::BenchArgs),
    /// Run Mizan headlessly on one dev case, with explicit approval and a daily budget.
    Eval(eval::EvalArgs),
    /// Serve the three Qiyas MCP tools over stdio without a window or credentials.
    Mcp(headless::McpArgs),
    /// Evaluate a subscription agent through headless MCP, without paid API calls.
    EvalAgent(eval_agent::EvalAgentArgs),
    /// Converts a drawing between DWG and DXF, by the output's extension: the drawing alone, as
    /// read (no project file, no take-off embedded).
    Convert {
        input: PathBuf,
        output: PathBuf,
        /// The output's version: 2000, 2004, 2007, 2010, 2013 or 2018 (default: the input's).
        #[arg(long, value_parser = parse_version)]
        version: Option<DxfVersion>,
    },
    /// Runs a command script (.scr) on the drawing, or on what the script OPENs.
    Run {
        script: PathBuf,
        drawing: Option<PathBuf>,
        /// Let SAVE write over a drawing the reader repaired when it opened it.
        #[arg(long)]
        allow_repaired_save: bool,
    },
    /// Counts blocks and exports the drawing's take-off to Excel. Writes nothing else.
    #[command(group(ArgGroup::new("what").args(["counts", "export"]).required(true).multiple(true)))]
    Takeoff {
        drawing: PathBuf,
        /// A block to count; repeat for more.
        #[arg(long = "count", value_name = "BLOCK")]
        counts: Vec<String>,
        /// The workbook to write the take-off to.
        #[arg(long, value_name = "XLSX")]
        export: Option<PathBuf>,
    },
    /// Runs a script on every drawing (.dwg, .dxf) of a folder or a sheet set (.qss).
    Batch {
        /// A folder (its own drawings, not its subfolders) or a sheet set (.qss).
        source: PathBuf,
        #[arg(long)]
        script: PathBuf,
        /// Where the scripts' written files go (default: the script's folder).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Drawings at a time.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=64))]
        jobs: u16,
        /// Let SAVE write over a drawing the reader repaired when it opened it.
        #[arg(long)]
        allow_repaired_save: bool,
    },
}

fn parse_version(text: &str) -> Result<DxfVersion, String> {
    Ok(match text {
        "2000" => DxfVersion::AC1015,
        "2004" => DxfVersion::AC1018,
        "2007" => DxfVersion::AC1021,
        "2010" => DxfVersion::AC1024,
        "2013" => DxfVersion::AC1027,
        "2018" => DxfVersion::AC1032,
        _ => return Err("give 2000, 2004, 2007, 2010, 2013 or 2018".to_string()),
    })
}

/// How a command finished without an error: cleanly, or with a warning for stderr (exit 2).
type Done = Result<Option<String>, String>;

#[derive(Subcommand)]
enum ProjectCli {
    Run {
        /// Deliverable kinds, comma separated; all exports the five kinds.
        #[arg(long)]
        deliverables: Option<String>,
        #[arg(long, default_value = "excel,pdf")]
        formats: String,
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long)]
        layout: Option<String>,
        project: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    // A panic (the reader on a damaged drawing) becomes a one-line error below, not a backtrace.
    std::panic::set_hook(Box::new(|_| {}));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            e.exit()
        }
        Err(e) => {
            eprintln!("qiyas-cli: {}", usage_error(&e));
            return ExitCode::FAILURE;
        }
    };
    let out = Out {
        quiet: cli.quiet,
        json: cli.json,
    };
    match catch_unwind(AssertUnwindSafe(|| dispatch(&cli.command, &out))) {
        Ok(Ok(None)) => ExitCode::SUCCESS,
        Ok(Ok(Some(warning))) => {
            eprintln!("qiyas-cli: {warning}");
            ExitCode::from(2)
        }
        Ok(Err(message)) => {
            eprintln!("qiyas-cli: {message}");
            ExitCode::FAILURE
        }
        Err(panic) => {
            eprintln!("qiyas-cli: crashed: {}", panic_text(&*panic));
            ExitCode::FAILURE
        }
    }
}

/// Clap's message in one line: its text before the usage and the pointer to --help, without
/// "error: ".
fn usage_error(e: &clap::Error) -> String {
    if e.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
        return "give a command: convert, run, takeoff or batch (see --help)".to_string();
    }
    let text = e.to_string();
    let text = text.split("\n\nUsage:").next().unwrap_or(&text);
    let text = text
        .split("\n\nFor more information")
        .next()
        .unwrap_or(text);
    let text = text.strip_prefix("error: ").unwrap_or(text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn panic_text(panic: &(dyn Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".to_string())
}

/// What goes on stdout: text, JSON or nothing.
struct Out {
    quiet: bool,
    json: bool,
}

impl Out {
    fn text(&self, line: impl std::fmt::Display) {
        if !self.quiet && !self.json {
            println!("{line}");
        }
    }

    fn json(&self, value: &impl Serialize) {
        if !self.quiet && self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(value).expect("plain data")
            );
        }
    }
}

fn dispatch(cmd: &Cmd, out: &Out) -> Done {
    match cmd {
        Cmd::Mcp(args) => headless::run(args),
        Cmd::EvalAgent(args) => {
            let result = eval_agent::run(args)?;
            out.text(serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
            out.json(&result);
            Ok(None)
        }
        Cmd::Bench(args) => {
            let result = bench::run(args)?;
            out.text(serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
            out.json(&result);
            Ok(None)
        }
        Cmd::Eval(args) => {
            let result = eval::run(args)?;
            out.text(serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?);
            out.json(&result);
            Ok(None)
        }
        Cmd::Convert {
            input,
            output,
            version,
        } => convert(out, input, output, *version),
        Cmd::Run {
            script,
            drawing,
            allow_repaired_save,
        } => run(out, script, drawing.as_deref(), *allow_repaired_save),
        Cmd::Export {
            source,
            kind,
            format,
            path,
            layout,
            language,
        } => {
            export_deliverable(source, kind, format, path, layout.as_deref(), language)?;
            out.json(&serde_json::json!({"written":path}));
            Ok(None)
        }
        Cmd::Project {
            command:
                ProjectCli::Run {
                    project,
                    out: folder,
                    deliverables,
                    formats,
                    language,
                    layout,
                },
        } => {
            let path = qiyas_boq::project::resolve(project)?;
            let result = qiyas_boq::project::run(
                &mut qiyas_boq::project::session()?,
                &path.to_string_lossy(),
                folder.as_deref(),
                Source::User,
            )?;
            if let Some(kinds) = deliverables {
                let names = if kinds == "all" {
                    "dimensions,boq,queries,variation,register"
                } else {
                    kinds.as_str()
                };
                for kind in names.split(',') {
                    for format in formats.split(',') {
                        let folder = folder
                            .clone()
                            .unwrap_or_else(|| path.with_extension("output"));
                        let folder = folder.canonicalize().map_err(|e| e.to_string())?;
                        let name = folder.join(format!(
                            "{kind}-{language}.{}",
                            if format == "pdf" { "pdf" } else { "xlsx" }
                        ));
                        export_deliverable(
                            &path,
                            kind,
                            format,
                            &name.to_string_lossy(),
                            layout.as_deref(),
                            language,
                        )?;
                    }
                }
            }
            out.json(&result);
            out.text(format!(
                "Project workbook: {}. Morning report: {}",
                result["workbook"].as_str().unwrap_or(""),
                result["morning_report"].as_str().unwrap_or("")
            ));
            if result["failed"].as_u64().unwrap_or(0) > 0 {
                Err("Project has failed drawings; see the morning report".into())
            } else if result["checked"] == false {
                Ok(Some(
                    "Project checks incomplete; see the morning report".into(),
                ))
            } else {
                Ok(None)
            }
        }
        Cmd::Variation {
            a,
            b,
            export,
            drawing_output,
        } => {
            let mut session = Session::open_standalone(b).map_err(|e| e.to_string())?;
            let a = a.canonicalize().map_err(|e| e.to_string())?;
            session
                .run(Command::CarryKnowledge {
                    from_path: a.to_string_lossy().into(),
                    tolerance: None,
                })
                .map_err(|e| e.to_string())?;
            session
                .run(Command::VariationReport { layer: None })
                .map_err(|e| e.to_string())?;
            Excel {
                allow_repaired_save: false,
            }
            .export_takeoff(&session, export)?;
            if let Some(path) = drawing_output {
                session.save_as_headless(path).map_err(|e| e.to_string())?;
            }
            if out.json {
                out.json(&session.variation().map_err(|e| e.to_string())?);
            } else if !out.quiet {
                println!("Variation written to {}", export.display());
            }
            Ok(None)
        }
        Cmd::Takeoff {
            drawing,
            counts,
            export,
        } => takeoff(out, drawing, counts, export.as_deref()),
        Cmd::Batch {
            source,
            script,
            out: out_dir,
            jobs,
            allow_repaired_save,
        } => batch(
            out,
            source,
            script,
            out_dir.as_deref(),
            usize::from(*jobs),
            *allow_repaired_save,
        ),
    }
}

/// The Excel and comparison steps of a script, as the desktop does them.
struct Excel {
    allow_repaired_save: bool,
}

impl Host for Excel {
    fn deliverable(&mut self, export: &qiyas_core::deliverables::Export) -> Result<(), String> {
        qiyas_boq::deliverables::write(export)
    }
    fn export_takeoff(&mut self, session: &Session, path: &Path) -> Result<(), String> {
        let title = match session.drawing.path.file_name() {
            Some(name) => format!("{} take-off", name.to_string_lossy()),
            None => "Take-off".to_string(),
        };
        qiyas_boq::export_takeoff_with_queries(
            path,
            &session.takeoff.items,
            &session.takeoff.totals(),
            &title,
            &session.measurement_report().map_err(|e| e.to_string())?,
            &session.conflicts,
        )
        .map_err(|e| e.to_string())
    }

    fn import_boq(&mut self, path: &Path) -> Result<Boq, String> {
        qiyas_boq::import_boq(path).map_err(|e| e.to_string())
    }

    fn compare(&mut self, session: &mut Session) -> Result<usize, String> {
        qiyas_takeoff::compare_session(session, qiyas_takeoff::DEFAULT_TOLERANCE, Source::User)
            .map(|conflicts| conflicts.len())
            .map_err(|e| e.to_string())
    }

    fn allow_repaired_save(&self) -> bool {
        self.allow_repaired_save
    }
}

/// `path` opened, or the error naming it.
fn open(path: &Path) -> Result<Session, String> {
    Session::open(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_script(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| {
        format!(
            "{}: not UTF-8 text: save the script as UTF-8",
            path.display()
        )
    })
}

/// The warning for a drawing the reader repaired: "<path> opened with N problems: check ...".
fn opened_with(path: &Path, problems: usize, check: &str) -> Option<String> {
    let s = if problems == 1 { "" } else { "s" };
    (problems > 0).then(|| {
        format!(
            "{} opened with {problems} problem{s}: check {check}",
            path.display()
        )
    })
}

/// Writes the drawing as the reader read it, in the output's format and the chosen version:
/// no project file, no take-off embedded, no autosave touched. An output already there is kept
/// as `<output>.bak`; the new file goes in through a temp file. Refused while a running Qiyas
/// has the output open.
fn convert(out: &Out, input: &Path, output: &Path, version: Option<DxfVersion>) -> Done {
    let mut drawing = Drawing::open(input).map_err(|e| format!("{}: {e}", input.display()))?;
    if let Some(version) = version {
        drawing.doc.version = version;
    }
    let version = drawing.doc.version;
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let bytes = match ext.as_deref() {
        Some("dwg") => DwgWriter::write_to_vec(&drawing.doc),
        Some("dxf") => DxfWriter::new(&drawing.doc).write_to_vec(),
        _ => return Err(format!("{}: save as .dwg or .dxf", output.display())),
    }
    .map_err(|e| format!("{}: {e}", output.display()))?;
    qiyas_core::refuse_if_open_elsewhere(output).map_err(|e| e.to_string())?;
    write_with_backup(output, &bytes).map_err(|e| format!("{}: {e}", output.display()))?;
    let problems = drawing.problems.len();
    out.text(format!(
        "converted {} to {} ({})",
        input.display(),
        output.display(),
        version.as_str(),
    ));
    out.json(&serde_json::json!({
        "input": input,
        "output": output,
        "version": version.as_str(),
        "problems": problems,
    }));
    Ok(opened_with(input, problems, "the converted drawing"))
}

/// `bytes` to `path` through `<path>.tmp`; a file already there is first copied to `<path>.bak`.
fn write_with_backup(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let with = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    let tmp = with(".tmp");
    let written = std::fs::write(&tmp, bytes).and_then(|()| {
        if path.is_file() {
            std::fs::copy(path, with(".bak"))?;
        }
        std::fs::rename(&tmp, path)
    });
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[derive(Serialize)]
struct RunReport<'a> {
    /// What the reader had to repair in the drawing given on the command line.
    problems: usize,
    #[serde(flatten)]
    run: &'a Run,
}

/// The warning line for a run's repaired drawings, if any.
fn run_warning(warnings: &[String]) -> Option<String> {
    match warnings {
        [] => None,
        [one] => Some(format!("{one}: check the results")),
        many => Some(format!(
            "{} drawings opened with problems: check the results",
            many.len()
        )),
    }
}

fn run(out: &Out, script_path: &Path, drawing: Option<&Path>, allow: bool) -> Done {
    let text = read_script(script_path)?;
    let cx = Context {
        script: script_path,
        drawing,
        out_dir: None,
    };
    let in_script = |e: script::ScriptError| format!("{}: {e}", script_path.display());
    let script = script::parse(&text, &cx).map_err(in_script)?;
    let session = drawing.map(open).transpose()?;
    let problems = session.as_ref().map_or(0, script::read_problems);
    let mut host = Excel {
        allow_repaired_save: allow,
    };
    let run = script::run(&script, session, &mut host).map_err(in_script)?;
    for step in &run.steps {
        out.text(format!("line {}: {}", step.line, step.text));
    }
    out.json(&RunReport {
        problems,
        run: &run,
    });
    Ok(run_warning(&run.warnings))
}

fn takeoff(out: &Out, drawing: &Path, blocks: &[String], export: Option<&Path>) -> Done {
    let mut session = open(drawing)?;
    let problems = script::read_problems(&session);
    let project = script::project_problems(&session);
    let mut counts = BTreeMap::new();
    for name in blocks {
        let outcome = session
            .run(Command::CountBlock {
                block_name: name.clone(),
                layer: None,
            })
            .map_err(|e| format!("COUNT {name}: {e}"))?;
        if let Outcome::Measured { measurement: m } = outcome {
            out.text(format!("{} = {:.0} {}", m.description, m.shown, m.unit));
            counts.insert(name.clone(), m.shown);
        }
    }
    if let Some(path) = export {
        let mut host = Excel {
            allow_repaired_save: false,
        };
        host.export_takeoff(&session, path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        out.text(format!("exported {}", path.display()));
    }
    out.json(&serde_json::json!({
        "drawing": drawing,
        "problems": problems,
        "warnings": project,
        "counts": counts,
        "export": export,
    }));
    if let Some(problem) = project.first() {
        return Ok(Some(format!("{}: {problem}", drawing.display())));
    }
    Ok(opened_with(drawing, problems, "its counts"))
}

/// One drawing of a batch: whether its script ran, with how many problems reading it, its block
/// counts, or why it failed.
#[derive(Serialize)]
struct FileReport {
    path: PathBuf,
    /// "ok", "warning" (it ran, but the reader repaired the drawing) or "error".
    status: &'static str,
    problems: usize,
    /// Why it is a warning: `Run::warnings` (the reader's repairs, a project file not read).
    warnings: Vec<String>,
    counts: BTreeMap<String, f64>,
    error: Option<String>,
}

impl FileReport {
    fn line(&self) -> String {
        let path = self.path.display();
        if let Some(error) = &self.error {
            return format!("{path} error {error}");
        }
        let mut line = format!("{path} {}", self.status);
        let mut why = Vec::new();
        if self.problems > 0 {
            why.push(script::problems_reading(self.problems));
        }
        // A project file not read: its message, without the drawing's path in front.
        let prefix = format!("{path}: ");
        why.extend(
            self.warnings
                .iter()
                .filter_map(|w| w.strip_prefix(&prefix))
                .filter(|w| w.starts_with("Project file"))
                .map(str::to_string),
        );
        if !why.is_empty() {
            line.push_str(&format!(" ({})", why.join("; ")));
        }
        for (name, count) in &self.counts {
            line.push_str(&format!(" {name}={count}"));
        }
        line
    }

    fn failed(path: &Path, error: String) -> FileReport {
        FileReport {
            path: path.to_path_buf(),
            status: "error",
            problems: 0,
            warnings: Vec::new(),
            counts: BTreeMap::new(),
            error: Some(error),
        }
    }
}

#[derive(Serialize)]
struct BatchReport {
    files: Vec<FileReport>,
    total: usize,
    ok: usize,
    warnings: usize,
    failed: usize,
}

/// The drawings of a folder (sorted by name) or of a sheet set (in its order).
fn drawings_of(source: &Path) -> Result<Vec<PathBuf>, String> {
    let is_drawing = |p: &Path| {
        p.is_file()
            && p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("dwg") || e.eq_ignore_ascii_case("dxf"))
    };
    let files: Vec<PathBuf> = if source.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(source)
            .map_err(|e| format!("{}: {e}", source.display()))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| is_drawing(p))
            .collect();
        files.sort();
        files
    } else if source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("qss"))
    {
        let set = SheetSet::load(source).map_err(|e| format!("{}: {e}", source.display()))?;
        qiyas_core::sheet_set_entries(source, &set)
            .into_iter()
            .map(|entry| PathBuf::from(entry.full))
            .collect()
    } else {
        return Err(format!(
            "{} is neither a folder nor a sheet set (.qss)",
            source.display()
        ));
    };
    if files.is_empty() {
        return Err(format!("no drawings in {}", source.display()));
    }
    Ok(files)
}

/// The files `script` writes when run on `drawing`: SAVEAS and EXPORT paths, the drawing SAVE
/// writes over, a JSON `write_block`'s path; up to QUIT.
fn written_by(script: &Script, drawing: &Path) -> Vec<PathBuf> {
    let mut current = drawing
        .canonicalize()
        .unwrap_or_else(|_| drawing.to_path_buf());
    let mut written = Vec::new();
    for step in &script.steps {
        match &step.action {
            Action::Quit => break,
            Action::Open(path) => current = path.clone(),
            Action::SaveAs(path) => {
                written.push(path.clone());
                current = path.clone();
            }
            Action::Save => written.push(current.clone()),
            Action::Export(path) => written.push(path.clone()),
            Action::Command(Command::WriteBlock { path, .. }) => {
                written.push(PathBuf::from(path));
            }
            _ => {}
        }
    }
    written.sort();
    written.dedup();
    written
}

/// Each drawing's script, read for it, or why it can't run: a bad line, or a file it would write
/// that another drawing of the batch writes too (both refused, never one over the other).
fn prepare(files: &[PathBuf], scripts: Vec<Result<Script, String>>) -> Vec<Result<Script, String>> {
    // Keyed in lower case on every system: Windows and macOS take `Plan.xlsx` and `plan.xlsx` for
    // one file, and a batch should mean the same everywhere.
    let mut writers: HashMap<String, (PathBuf, Vec<usize>)> = HashMap::new();
    for (i, script) in scripts.iter().enumerate() {
        if let Ok(script) = script {
            for path in written_by(script, &files[i]) {
                let key = path.to_string_lossy().to_lowercase();
                let entry = writers.entry(key).or_insert_with(|| (path, Vec::new()));
                if !entry.1.contains(&i) {
                    entry.1.push(i);
                }
            }
        }
    }
    let mut collisions: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (path, who) in writers.values() {
        if who.len() < 2 {
            continue;
        }
        for &i in who {
            let others: Vec<String> = who
                .iter()
                .filter(|&&j| j != i)
                .map(|&j| files[j].display().to_string())
                .collect();
            collisions.entry(i).or_default().push(format!(
                "output {} collides with {}",
                path.display(),
                others.join(", ")
            ));
        }
    }
    scripts
        .into_iter()
        .enumerate()
        .map(|(i, script)| match collisions.remove(&i) {
            Some(mut why) => {
                why.sort();
                Err(why.join("; "))
            }
            None => script,
        })
        .collect()
}

fn batch(
    out: &Out,
    source: &Path,
    script_path: &Path,
    out_dir: Option<&Path>,
    jobs: usize,
    allow: bool,
) -> Done {
    let files = drawings_of(source)?;
    let text = read_script(script_path)?;
    let parse = |file: &Path| {
        script::parse(
            &text,
            &Context {
                script: script_path,
                drawing: Some(file),
                out_dir,
            },
        )
    };
    // A bad script fails before any drawing is touched.
    parse(&files[0]).map_err(|e| format!("{}: {e}", script_path.display()))?;
    let scripts = files
        .iter()
        .map(|f| parse(f).map_err(|e| e.to_string()))
        .collect();
    let scripts = prepare(&files, scripts);
    if let Some(dir) = out_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    // Workers take the next drawing in turn, each opening its own session; the report is
    // printed in the folder's order as soon as the drawings before it are done.
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    let mut reports: Vec<FileReport> = Vec::with_capacity(files.len());
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(files.len()) {
            let tx = tx.clone();
            let (next, files, scripts) = (&next, &files, &scripts);
            std::thread::Builder::new()
                // The reader recurses on nested blocks: room for deep drawings.
                .stack_size(16 << 20)
                .spawn_scoped(scope, move || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = files.get(i) else { break };
                    let report = match &scripts[i] {
                        Ok(script) => run_one(file, script, allow),
                        Err(why) => FileReport::failed(file, why.clone()),
                    };
                    if tx.send((i, report)).is_err() {
                        break;
                    }
                })
                .expect("a worker thread");
        }
        drop(tx);
        let mut waiting = BTreeMap::new();
        for (i, report) in rx {
            waiting.insert(i, report);
            while let Some(report) = waiting.remove(&reports.len()) {
                out.text(report.line());
                reports.push(report);
            }
        }
    });

    let count = |status: &str| reports.iter().filter(|r| r.status == status).count();
    let (ok, warnings, failed) = (count("ok"), count("warning"), count("error"));
    let total = reports.len();
    let summary = format!("{total} drawings: {ok} ok, {warnings} with warnings, {failed} failed");
    if out.quiet {
        println!("{summary}");
    } else {
        out.text(summary);
    }
    out.json(&BatchReport {
        files: reports,
        total,
        ok,
        warnings,
        failed,
    });
    match (failed, warnings) {
        (0, 0) => Ok(None),
        (0, w) => Ok(Some(format!(
            "{w} of {total} drawings opened with problems: check their counts"
        ))),
        (f, _) => Err(format!("{f} of {total} drawings failed")),
    }
}

/// Runs the script on one drawing of a batch; a panic (the reader on a damaged file) is that
/// drawing's error, not the batch's end.
fn run_one(file: &Path, script: &Script, allow: bool) -> FileReport {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let session = Session::open(file).map_err(|e| e.to_string())?;
        let problems = script::read_problems(&session);
        let mut host = Excel {
            allow_repaired_save: allow,
        };
        let run = script::run(script, Some(session), &mut host).map_err(|e| e.to_string())?;
        Ok((problems, run))
    }))
    .unwrap_or_else(|panic| Err(format!("crashed: {}", panic_text(&*panic))));
    match result {
        Ok((problems, run)) => FileReport {
            path: file.to_path_buf(),
            status: if run.warnings.is_empty() {
                "ok"
            } else {
                "warning"
            },
            problems,
            warnings: run.warnings,
            counts: run.counts,
            error: None,
        },
        Err(error) => FileReport::failed(file, error),
    }
}

fn export_deliverable(
    source: &Path,
    kind: &str,
    format: &str,
    path: &str,
    layout: Option<&str>,
    language: &str,
) -> Result<(), String> {
    let project = source.extension().is_some_and(|e| e == "qss");
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let mut session = if project {
        qiyas_boq::project::session()?
    } else {
        Session::open(&source).map_err(|e| e.to_string())?
    };
    let input = serde_json::json!({"type":"deliverable.export","kind":kind,"format":format,"path":path,"layout":layout,"language":language,"project":if project {Some(source.to_string_lossy())} else {None}});
    let outcome = session
        .run_input(&input, Source::User)
        .map_err(|e| e.to_string())?;
    qiyas_boq::deliverables::complete(&outcome.outcome)
}
