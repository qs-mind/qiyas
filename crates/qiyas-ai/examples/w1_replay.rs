//! `cargo run -p qiyas-ai --example w1_replay -- <drawing> <reads.jsonl> [--summary]`
//!
//! Offline replay of a logged sequence of `qiyas_read` calls (one JSON object per line:
//! `{"tool": "qiyas_read", "input": {...}}`) on a copy of a drawing, through the same route,
//! session run and model-facing formatter the agent uses. It prints, for each call, the bytes
//! the model would receive and whether the result was shortened to an artifact pointer, then
//! totals. It reads only: calls that are not reads are skipped. Used for the W1 before/after
//! evidence (`docs/2026-10-26-qiyas-W1.md`); no model is involved.

use std::path::Path;

use qiyas_ai::surface::{self, Route};
use qiyas_ai::ToolOutput;
use qiyas_core::{Session, Source};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(drawing), Some(calls)) = (args.next(), args.next()) else {
        eprintln!("usage: w1_replay <drawing> <reads.jsonl> [--summary]");
        std::process::exit(2);
    };
    let rest: Vec<String> = args.collect();
    let summary_only = rest.iter().any(|a| a == "--summary");
    let show: usize = rest
        .iter()
        .position(|a| a == "--show")
        .and_then(|i| rest.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let mut session = Session::open(Path::new(&drawing)).expect("open the drawing");
    let text = std::fs::read_to_string(&calls).expect("read the call list");
    let (mut total, mut shortened, mut count, mut errors) = (0usize, 0usize, 0usize, 0usize);
    let mut by_op = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let call: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
        let tool = call["tool"].as_str().unwrap_or("qiyas_read");
        let input = &call["input"];
        if tool != surface::READ {
            continue;
        }
        let op = input["op"].as_str().unwrap_or("?").to_string();
        let routed = surface::route(tool, input);
        let mut shown = String::new();
        let (bytes, cut, err) = match routed {
            Ok(Route::Session {
                tool,
                op: route_op,
                args,
            }) => {
                let out = surface::run_session(&mut session, &tool, &route_op, &args, Source::Ai)
                    .map_err(|e| e.json().to_string());
                match out {
                    Ok(output) => {
                        match surface::format_result(&tool, &route_op, &args, output) {
                            Ok(ToolOutput::Json(v)) => {
                                let text = v.to_string();
                                shown = text.chars().take(show).collect();
                                (text.len(), text.contains("Full evidence is in the artifact"), false)
                            }
                            Ok(_) => (0, false, false),
                            Err(e) => {
                                shown = e.json().to_string().chars().take(show).collect();
                                (e.json().to_string().len(), false, true)
                            }
                        }
                    }
                    Err(e) => {
                        shown = e.chars().take(show).collect();
                        (e.len(), false, true)
                    }
                }
            }
            Ok(_) => continue,
            Err(e) => {
                shown = e.chars().take(show).collect();
                (e.len(), false, true)
            }
        };
        total += bytes;
        count += 1;
        shortened += usize::from(cut);
        errors += usize::from(err);
        let slot = by_op.entry(op.clone()).or_default();
        slot.0 += 1;
        slot.1 += bytes;
        slot.2 += usize::from(cut);
        if !summary_only {
            println!(
                "{:>8} bytes{}{}  {} {}",
                bytes,
                if cut { "  SHORTENED" } else { "" },
                if err { "  ERROR" } else { "" },
                op,
                input["args"].to_string().chars().take(100).collect::<String>()
            );
            if show > 0 {
                println!("      {shown}");
            }
        }
    }
    for (op, (n, bytes, cut)) in &by_op {
        println!("{op:<14} calls {n:>3}  bytes {bytes:>8}  shortened {cut:>3}");
    }
    println!("TOTAL calls {count}  bytes {total}  shortened {shortened}  errors {errors}");
}
