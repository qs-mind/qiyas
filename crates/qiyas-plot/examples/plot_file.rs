//! `cargo run -p qiyas-plot --example plot_file -- <drawing> <out.pdf> [sheet]`: plots a drawing's
//! sheet (model space by default) to fit an A1 sheet, as the Plot dialog would: text, fills and
//! all. For looking at a drawing from the command line (pdftoppm turns the PDF into a picture).

use std::path::Path;

use qiyas_core::Session;
use qiyas_plot::{plot_pdf_to, Area, Orientation, Paper, PlotJob, Scale};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: plot_file <drawing> <out.pdf> [sheet]");
        std::process::exit(2);
    };
    let sheet = args.next().unwrap_or_else(|| "*Model_Space".to_string());
    let session = Session::open(Path::new(&input)).expect("open the drawing");
    let job = PlotJob {
        sheet,
        area: Area::Extents,
        paper: Paper {
            name: "ISO A1".into(),
            w_mm: 594.0,
            h_mm: 841.0,
        },
        orientation: Orientation::Landscape,
        scale: Scale::Fit,
        centre: true,
        style: None,
        lineweights: true,
        output: None,
    };
    let out = plot_pdf_to(&session, &job, Path::new(&output)).expect("plot");
    println!("{}", out.path.display());
    for why in out.skipped {
        println!("skipped: {why}");
    }
}
