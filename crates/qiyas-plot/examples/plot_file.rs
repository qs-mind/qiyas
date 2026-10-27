//! `cargo run -p qiyas-plot --example plot_file -- <drawing> <out.pdf> [sheet]`: plots a drawing's
//! sheet (model space by default) to fit an A1 sheet, as the Plot dialog would: text, fills and
//! all. For looking at a drawing from the command line (pdftoppm turns the PDF into a picture).
//! A layout (a sheet made by make_sheet) plots as its own page setup at 1:1, as the app prints it.
//! A fourth argument is a `make_sheet` command as JSON, run on the drawing first (nothing is
//! saved); its sheet is the one plotted.

use std::path::Path;

use qiyas_core::Session;
use qiyas_plot::{plot_pdf_to, Area, Orientation, Paper, PlotJob, Scale};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: plot_file <drawing> <out.pdf> [sheet] [make_sheet json]");
        std::process::exit(2);
    };
    let mut sheet = args.next().unwrap_or_else(|| "*Model_Space".to_string());
    let mut session = Session::open(Path::new(&input)).expect("open the drawing");
    if let Some(command) = args.next() {
        let command: serde_json::Value = serde_json::from_str(&command).expect("make_sheet JSON");
        let outcome = session
            .run_input(&command, qiyas_core::Source::User)
            .expect("make the sheet");
        println!("{}", serde_json::to_string(&outcome.outcome).unwrap());
        let name = command["name"].as_str().expect("the sheet's name");
        sheet = session
            .drawing
            .sheets()
            .into_iter()
            .find(|s| s.name == name)
            .expect("the new sheet")
            .block;
    }
    let layout = !sheet.eq_ignore_ascii_case("*Model_Space");
    let paper = if layout {
        session
            .run(qiyas_core::Command::SetSheet {
                block: sheet.clone(),
            })
            .expect("show the layout");
        session.paper().expect("a sized layout")
    } else {
        Paper {
            name: "ISO A1".into(),
            w_mm: 594.0,
            h_mm: 841.0,
        }
    };
    let job = PlotJob {
        sheet,
        area: if layout { Area::Layout } else { Area::Extents },
        paper,
        orientation: if layout {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        },
        scale: if layout {
            Scale::Ratio(1.0, 1.0)
        } else {
            Scale::Fit
        },
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
