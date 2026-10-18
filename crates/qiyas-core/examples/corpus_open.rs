//! Read-only corpus probe. One drawing per process; JSON on stdout, diagnostics on stderr.
use qiyas_core::Session;
use serde_json::json;
use std::{path::PathBuf, time::Instant};

fn main() {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("usage: corpus_open <drawing>"),
    );
    let start = Instant::now();
    let session = match Session::open(&path) {
        Ok(session) => session,
        Err(error) => {
            println!(
                "{}",
                json!({"opened": false, "error": error.to_string(),
                "open_ms": start.elapsed().as_secs_f64() * 1000.0})
            );
            std::process::exit(1);
        }
    };
    let open_ms = start.elapsed().as_secs_f64() * 1000.0;
    let batch = session.batch();
    let ready_ms = start.elapsed().as_secs_f64() * 1000.0;
    let info = session.info();
    println!(
        "{}",
        json!({
            "opened": true, "open_ms": open_ms, "ready_ms": ready_ms,
            "report": session.drawing.report, "warnings": info.problems,
            "layers": info.layers.len(), "sheets": info.sheets.len(),
            "segments": batch.segments.len() / 4, "ranges": batch.ranges.len(),
        })
    );
}
