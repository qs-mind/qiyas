//! Times what a click costs on a big drawing: snap, pick, window pick, then one drawn line and its
//! undo, each followed by the batch the window asks for.
//!
//! `cargo run --release -p qiyas-core --example bench_edit -- <drawing.dwg>`
//! Writes nothing to the drawing; the edits stay in memory.

use std::path::PathBuf;
use std::time::Instant;

use qiyas_core::{Session, Source};
use serde_json::json;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: bench_edit <drawing>"),
    );
    let mut session = Session::open(&path).expect("open");
    let t = Instant::now();
    let bytes = session.batch_bytes_if(None, None).expect("a batch").len();
    println!("first batch bytes  {:8.1} ms  ({bytes} bytes)", ms(t));
    let t = Instant::now();
    let again = session.batch_bytes_if(None, None).expect("a batch");
    println!(
        "same batch again   {:8.1} ms  ({} bytes)",
        ms(t),
        again.len()
    );
    let t = Instant::now();
    let atlas = session.atlas_bytes(None).len();
    println!("atlas bytes        {:8.1} ms  ({atlas} bytes)", ms(t));

    // What the window's one-second watch asks of the session on every beat.
    let t = Instant::now();
    for _ in 0..100 {
        std::hint::black_box((
            session.revision(),
            session.batch_version(),
            session.is_dirty(),
        ));
    }
    println!(
        "watch beat stamp   {:8.3} ms  (revision, version, dirty; avg of 100)",
        ms(t) / 100.0
    );
    let t = Instant::now();
    for _ in 0..20 {
        std::hint::black_box(session.info());
    }
    println!("session.info()     {:8.3} ms  (avg of 20)", ms(t) / 20.0);

    let [[x0, y0], [x1, y1]] = session.batch().bounds;
    let centre = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
    let tol = (x1 - x0) / 2000.0;
    session.drafting.osnap = true;
    let modes = session.drafting.snap_modes;
    let t = Instant::now();
    let mut found = 0;
    for i in 0..50 {
        let p = [centre[0] + i as f64 * tol, centre[1] + i as f64 * tol];
        found += usize::from(session.snap(p, tol * 10.0, &modes, None).is_some());
    }
    println!(
        "snap (avg of 50)   {:8.3} ms  ({found} found)",
        ms(t) / 50.0
    );
    let t = Instant::now();
    let hits = session.pick_window([centre, [centre[0] + 30.0, centre[1] + 20.0]], true);
    println!("window pick 30x20  {:8.1} ms  ({} hits)", ms(t), hits.len());
    let t = Instant::now();
    let hits = session.pick_window([[x0, y0], [x1, y1]], false);
    println!("window pick all    {:8.1} ms  ({} hits)", ms(t), hits.len());

    let line = json!({"type":"draw","kind":"line","a":[centre[0], centre[1]],"b":[centre[0] + 5.0, centre[1]]});
    for round in 0..3 {
        let t = Instant::now();
        session.run_input(&line, Source::User).expect("draw");
        let run = ms(t);
        let t = Instant::now();
        let version = session.batch_version();
        let version_ms = ms(t);
        let t = Instant::now();
        let bytes = session.batch_bytes_if(None, None).expect("a batch").len();
        let batch_ms = ms(t);
        let t = Instant::now();
        session.snap(centre, tol * 10.0, &modes, None);
        println!(
            "draw a line #{round}    {run:8.1} ms, version {version_ms:.2} ms, next batch {batch_ms:8.1} ms ({bytes} bytes), first snap after it {:.1} ms [{version}]",
            ms(t)
        );
        let t = Instant::now();
        session.undo();
        let undo = ms(t);
        let t = Instant::now();
        session.batch_bytes_if(None, None).expect("a batch");
        println!(
            "undo #{round}          {undo:8.1} ms, next batch {:8.1} ms",
            ms(t)
        );
    }
}
