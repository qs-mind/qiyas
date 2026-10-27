//! Plotting: a sheet of the open drawing to a one-page PDF as the screen draws it (plot style
//! applied as a colour and lineweight map, lineweights in paper millimetres, text as real PDF
//! text in embedded font subsets or else outlines, fills as paths, pictures embedded), plot
//! styles (CTB and STB files), and handing a PDF to the operating system to print.

mod content;
mod ctb;
mod font;
mod look;
mod page;
mod plotter;
mod print;

use std::path::{Path, PathBuf};

use qiyas_core::Session;
use serde::{Deserialize, Serialize};

pub use ctb::{aci_of, object_pen, Pen, PlotStyle, StyleEntry, StyleKind, DEFAULT_LINEWEIGHTS};
pub use look::DEFAULT_LINEWEIGHT_MM;
pub use page::{paper_catalog, Paper, FIT_MARGIN_MM, PT_PER_MM};
pub use plotter::{write_pdf, PlotSnapshot, PLOT_TOL_MM};
pub use print::{
    default_printer, hands_off_a_copy, list_printers, parse_default_printer, parse_printers,
    plain_path, print_dry_run, print_pdf, print_plan, print_spooled, PrintPlan,
};

/// What part of the sheet is plotted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    /// Everything the sheet draws.
    Extents,
    /// Two corners in sheet coordinates.
    Window([[f64; 2]; 2]),
    /// A layout's limits (its printable area as AutoCAD keeps it), else its extents; model
    /// space's extents.
    Layout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    /// The paper turned: its width and height swapped.
    Landscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    /// As large as fits inside a 5 mm margin.
    Fit,
    /// `num` paper millimetres per `den` sheet millimetres (1:100 is `Ratio(1, 100)`). Model
    /// space is in the drawing's units; a layout's units are taken as millimetres.
    Ratio(f64, f64),
}

/// One plot: which sheet, area, paper, scale and plot style, and where the PDF goes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlotJob {
    /// The sheet's block name: `*Model_Space`, `*Paper_Space`, ...
    pub sheet: String,
    pub area: Area,
    pub paper: Paper,
    pub orientation: Orientation,
    pub scale: Scale,
    /// Centre the area on the page; else its lower left goes to the page's.
    pub centre: bool,
    /// A CTB or STB to plot with; `None` plots the colours as drawn.
    #[serde(default)]
    pub style: Option<PlotStyle>,
    /// Plot the entities' lineweights; else every line is as thin as the device draws (a plot
    /// style's lineweights still apply).
    pub lineweights: bool,
    /// The PDF's name in the drawing's folder (or a subfolder); `None` for
    /// `<drawing>-<sheet>.pdf` beside the drawing.
    #[serde(default)]
    pub output: Option<String>,
}

/// A plot written: the PDF, and what was left out of it (pictures not found, ...).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlotOutput {
    pub path: PathBuf,
    pub skipped: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlotError {
    #[error("Nothing to plot: the area is empty")]
    Nothing,
    #[error("No sheet {0} in this drawing")]
    NoSuchSheet(String),
    #[error("{0}")]
    Paper(String),
    #[error("{0}")]
    Output(String),
    #[error("Can't write the PDF: {0}")]
    Write(String),
    #[error("{0}")]
    Style(String),
    #[error("{0}")]
    Print(String),
}

/// Plots `job` to its PDF in the drawing's folder (see `PlotJob::output`), written over a PDF
/// of that name.
pub fn plot_pdf(session: &Session, job: &PlotJob) -> Result<PlotOutput, PlotError> {
    let path = output_path(session, job)?;
    plot_pdf_to(session, job, &path)
}

/// Plots `job` to the PDF at `path`, anywhere (a Save dialog's choice, a print spool file).
pub fn plot_pdf_to(session: &Session, job: &PlotJob, path: &Path) -> Result<PlotOutput, PlotError> {
    write_pdf(&plot_snapshot(session, job)?, path)
}

/// What plotting `job` needs from `session`, as plain data: the only step that needs the
/// session (hold its lock for this, then write the PDF with `write_pdf` without it).
pub fn plot_snapshot(session: &Session, job: &PlotJob) -> Result<PlotSnapshot, PlotError> {
    plotter::snapshot(session, job)
}

/// `<drawing>-<sheet name>.pdf`, the sheet named as its tab is, with characters file names
/// can't hold made `_`.
pub fn default_file_name(session: &Session, sheet: &str) -> Result<String, PlotError> {
    let name = session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.block.eq_ignore_ascii_case(sheet))
        .ok_or_else(|| PlotError::NoSuchSheet(sheet.to_string()))?
        .name;
    let stem = session
        .drawing
        .path
        .file_stem()
        .map_or("drawing".into(), |s| s.to_string_lossy().into_owned());
    let clean: String = format!("{stem}-{name}")
        .chars()
        .map(|c| {
            // Characters file names can't hold, and quotes and signs a shell reads (defence in
            // depth: no name is ever put inside a command).
            if c.is_control() || r#"/\:*?"<>|’‘‚‛$`"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    Ok(format!("{clean}.pdf"))
}

/// Where `job` goes by command: a `.pdf` inside the drawing's folder or one of its existing
/// subfolders (relative names from that folder), or beside the drawing.
pub fn output_path(session: &Session, job: &PlotJob) -> Result<PathBuf, PlotError> {
    let outside = || {
        PlotError::Output(
            "A plot goes in the drawing's folder this way: choose another place in the Save dialog"
                .into(),
        )
    };
    // The drawing's folder, or Documents for a drawing that has no file yet: the same place as
    // every other export (never the app's hidden placeholder folder).
    let folder = session
        .export_folder()
        .canonicalize()
        .map_err(|_| outside())?;
    let name = match &job.output {
        Some(name) => name.clone(),
        None => default_file_name(session, &job.sheet)?,
    };
    let full = folder.join(&name);
    if !full
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        return Err(PlotError::Output("A plot is a .pdf file".into()));
    }
    let file = full.file_name().ok_or_else(outside)?;
    let dir = full.parent().ok_or_else(outside)?;
    let real = dir.canonicalize().map_err(|_| outside())?;
    if !real.starts_with(&folder) {
        return Err(outside());
    }
    Ok(print::plain_path(&real.join(file)))
}

/// A PDF in the user's print spool folder (`spool_dir`) for printing sheet `sheet`
/// (`<drawing>-<sheet>.pdf`).
pub fn spool_path(session: &Session, sheet: &str) -> Result<PathBuf, PlotError> {
    Ok(spool_dir()?.join(default_file_name(session, sheet)?))
}

/// How long a spool PDF the print system did not take a copy of (the PDF viewer's, Windows')
/// is kept for its program to read.
pub const SPOOL_KEEP: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// The user's own folder for print spool PDFs, made when missing: `<temp>/qiyas-plot-<uid>`
/// with mode 0700 on Unix, where the temporary folder is shared (a folder of that name that is
/// not this user's, or is a link, is refused: another user could read the plots or plant links
/// there); `<temp>/qiyas-plot` on Windows, whose temporary folder is the user's. Spool PDFs
/// older than `SPOOL_KEEP` are removed.
pub fn spool_dir() -> Result<PathBuf, PlotError> {
    let write = |e: std::io::Error| PlotError::Write(e.to_string());
    #[cfg(unix)]
    let dir = {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        // SAFETY: getuid has no preconditions and cannot fail.
        let uid = unsafe { libc::getuid() };
        let dir = std::env::temp_dir().join(format!("qiyas-plot-{uid}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(write(e)),
            _ => {}
        }
        let meta = std::fs::symlink_metadata(&dir).map_err(write)?;
        if !meta.is_dir() || meta.uid() != uid {
            return Err(PlotError::Write(format!(
                "{} is not this user's own folder: remove it, then print again",
                dir.display()
            )));
        }
        if meta.mode() & 0o077 != 0 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(write)?;
        }
        dir
    };
    #[cfg(not(unix))]
    let dir = {
        let dir = std::env::temp_dir().join("qiyas-plot");
        std::fs::create_dir_all(&dir).map_err(write)?;
        dir
    };
    let now = std::time::SystemTime::now();
    for entry in std::fs::read_dir(&dir).map_err(write)?.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > SPOOL_KEEP));
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(dir)
}

pub mod report;

/// Marked drawing sheets supporting the generated BOQ.
pub use plotter::evidence;
