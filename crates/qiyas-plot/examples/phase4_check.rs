//! The Phase 4 check on one drawing, without a window: plots to PDF (model extents on A1 fitted
//! and at 1:100, the first layout on A1, and an Arabic label added in memory), a purge dry run,
//! an audit without fixes, and a width constraint on a rectangle drawn in memory.
//!
//! `cargo run --release -p qiyas-plot --example phase4_check -- <drawing> <out folder>`
//! Read only: the drawing is never saved; the PDFs go in the out folder.

use std::path::{Path, PathBuf};
use std::time::Instant;

use opencadcodec::{EntityType, Handle};
use qiyas_core::{AutoSettings, Command, ConstraintKind, DrawSpec, Outcome, Session, Target};
use qiyas_plot::{
    paper_catalog, plot_snapshot, write_pdf, Area, Orientation, Paper, PlotJob, Scale,
};

const MODEL: &str = "*Model_Space";

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn a(name: &str) -> Paper {
    paper_catalog()
        .into_iter()
        .find(|p| p.name == name)
        .expect("in the catalog")
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn plot(s: &Session, label: &str, job: PlotJob, path: &Path) {
    let t0 = Instant::now();
    let snap = match plot_snapshot(s, &job) {
        Ok(snap) => snap,
        Err(e) => {
            println!("plot {label:<14} refused: {e}");
            return;
        }
    };
    let snap_ms = ms(t0);
    let t1 = Instant::now();
    match write_pdf(&snap, path) {
        Ok(out) => {
            let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            println!(
                "plot {label:<14} snapshot {snap_ms:.0} ms, write {:.0} ms, {bytes} bytes, skipped {}: {}",
                ms(t1),
                out.skipped.len(),
                path.display()
            );
            for why in out.skipped.iter().take(3) {
                println!("  skipped {why}");
            }
        }
        Err(e) => println!("plot {label:<14} failed: {e}"),
    }
}

fn job(sheet: &str, area: Area, paper: Paper, scale: Scale) -> PlotJob {
    PlotJob {
        sheet: sheet.to_string(),
        area,
        paper,
        orientation: Orientation::Landscape,
        scale,
        centre: true,
        style: None,
        lineweights: true,
        output: None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("usage: phase4_check <drawing> <out>"));
    let out = PathBuf::from(args.next().expect("usage: phase4_check <drawing> <out>"));
    std::fs::create_dir_all(&out).unwrap();
    let stem = path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .replace(' ', "_");

    let t0 = Instant::now();
    let mut s = Session::open(&path).expect("open");
    println!("file {} (open {:.0} ms)", path.display(), ms(t0));
    let info = s.info();
    println!("units {}, problems {}", info.units, info.problems.len());
    let sheets = s.drawing.sheets();
    println!(
        "sheets {}: {}",
        sheets.len(),
        sheets
            .iter()
            .map(|sh| sh.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    s.set_sheet(MODEL).unwrap();
    let [[x0, y0], [x1, y1]] = s.batch().bounds;
    println!(
        "model extents {:.2} x {:.2} units from ({x0:.2}, {y0:.2})",
        x1 - x0,
        y1 - y0
    );

    // Model space: fitted on A1, and at 1:100 (centred, what falls off the page is cut).
    plot(
        &s,
        "model A1 fit",
        job(MODEL, Area::Extents, a("ISO A1"), Scale::Fit),
        &out.join(format!("{stem}-model-A1-fit.pdf")),
    );
    plot(
        &s,
        "model A1 1:100",
        job(MODEL, Area::Extents, a("ISO A1"), Scale::Ratio(1.0, 100.0)),
        &out.join(format!("{stem}-model-A1-1to100.pdf")),
    );
    // The first layout, as its page setup gives it, fitted on A1.
    if let Some(layout) = sheets.iter().find(|sh| !sh.is_model) {
        plot(
            &s,
            "layout A1 fit",
            job(&layout.block, Area::Layout, a("ISO A1"), Scale::Fit),
            &out.join(format!("{stem}-layout-A1-fit.pdf")),
        );
    }

    // Purge dry run and audit, timed.
    let t0 = Instant::now();
    match s.run(Command::Purge {
        kinds: Vec::new(),
        dry_run: true,
    }) {
        Ok(Outcome::Purged { report }) => println!(
            "purge dry run {:.0} ms: total {}, layers {}, blocks {}, styles {}, linetypes {}, dimstyles {}, groups {}, group members {}, limited {:?}",
            ms(t0),
            report.total,
            report.layers.len(),
            report.blocks.len(),
            report.styles.len(),
            report.linetypes.len(),
            report.dimstyles.len(),
            report.groups.len(),
            report.group_members,
            report.limited_reason
        ),
        other => println!("purge dry run: {other:?}"),
    }
    let t0 = Instant::now();
    match s.run(Command::Audit { fix: false }) {
        Ok(Outcome::Audited { report }) => {
            println!(
                "audit {:.0} ms: {} kinds, {:?}",
                ms(t0),
                report.counts.len(),
                report.counts
            );
        }
        other => println!("audit: {other:?}"),
    }

    // A 4 x 3 unit rectangle beyond the extents, auto-constrained, its width 4 -> 6.
    let (rx, ry) = (x1 + 10.0, y0);
    let rect = match s
        .run(Command::Draw(DrawSpec::Rectangle {
            a: [rx, ry],
            b: [rx + 4.0, ry + 3.0],
        }))
        .unwrap()
    {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    };
    let t0 = Instant::now();
    let auto = s
        .run(Command::AutoConstrain {
            targets: vec![t(rect)],
            settings: AutoSettings::default(),
        })
        .unwrap();
    let auto_ms = ms(t0);
    let t0 = Instant::now();
    let dim = s.run(Command::AddDimConstraint {
        kind: ConstraintKind::Linear,
        targets: vec![t(rect)],
        points: vec![[rx + 2.0, ry]].into(),
        line_point: [rx + 2.0, ry - 1.0],
        expression: None,
    });
    let dim_ms = ms(t0);
    let t0 = Instant::now();
    let set = s.run(Command::SetParameter {
        name: "d1".into(),
        expression: "6".into(),
    });
    let set_ms = ms(t0);
    let corners: Vec<[f64; 2]> = match s.drawing.doc.get_entity(Handle::new(rect)) {
        Some(EntityType::LwPolyline(p)) => p
            .vertices
            .iter()
            .map(|v| [v.location.x - rx, v.location.y - ry])
            .collect(),
        _ => Vec::new(),
    };
    println!(
        "constraint auto {auto_ms:.0} ms ({:?}), width {dim_ms:.0} ms ({}), d1=6 {set_ms:.0} ms ({}), corners from its first: {corners:.3?}",
        match auto {
            Outcome::Constrained { ids } => ids.len(),
            _ => 0,
        },
        if dim.is_ok() { "ok" } else { "refused" },
        match &set {
            Ok(_) => "ok".to_string(),
            Err(e) => e.to_string(),
        }
    );

    // An Arabic label added in memory, plotted on A4: its text must come out of the PDF.
    let height = ((x1 - x0).max(y1 - y0) / 200.0).max(0.1);
    let at = [x0 + (x1 - x0) / 2.0, y0 + (y1 - y0) / 2.0];
    s.run(Command::Text {
        at,
        height,
        rotation: 0.0,
        value: "غرفة المعيشة ٥٨٫٢٩ م²".to_string(),
        style: None,
    })
    .unwrap();
    let w = height * 30.0;
    plot(
        &s,
        "arabic A4 fit",
        job(
            MODEL,
            Area::Window([[at[0] - w, at[1] - w / 2.0], [at[0] + w, at[1] + w / 2.0]]),
            a("ISO A4"),
            Scale::Fit,
        ),
        &out.join(format!("{stem}-arabic-A4.pdf")),
    );
}
