//! Inspect a drawing through qiyas_read, or capture a picture.
//! Usage: inspect <drawing> [op={args} ...]. Defaults to state and drawing queries.
//! A picture writes <drawing>.picture.png.

use std::path::Path;

use qiyas_ai::{surface, ToolOutput};
use qiyas_core::{Session, Source};
use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(drawing) = args.next() else {
        eprintln!("usage: inspect <drawing> [tool …]");
        std::process::exit(2);
    };
    let mut tools: Vec<String> = args.collect();
    if tools.is_empty() {
        tools = [
            "state",
            r#"query={"kind":"layers"}"#,
            r#"query={"kind":"dimensions"}"#,
            r#"query={"kind":"texts"}"#,
            r#"query={"kind":"blocks"}"#,
        ]
        .map(String::from)
        .to_vec();
    }
    let mut session = Session::open(Path::new(&drawing)).expect("open the drawing");
    let (scale, factor) = session.annotation_scale();
    println!(
        "units {} annotation {scale} ({factor})",
        session.info().units
    );
    for s in session.dim_styles() {
        println!(
            "dimstyle {}: {}",
            s.name,
            serde_json::to_string(&s.fields).unwrap()
        );
    }
    for spec in tools {
        // `tool` or `tool={json input}`.
        let (tool, input) = match spec.split_once('=') {
            Some((t, j)) => (
                t.to_string(),
                serde_json::from_str(j).expect("the input as JSON"),
            ),
            None => (spec.clone(), json!({})),
        };
        let name = if tool == "picture" {
            surface::CAPTURE
        } else {
            surface::READ
        };
        match surface::run_session(&mut session, name, &tool, &input, Source::User) {
            Ok(ToolOutput::Json(v)) => {
                println!("== {tool}\n{}", serde_json::to_string(&v).unwrap())
            }
            Ok(ToolOutput::Image { png, caption }) => {
                let out = format!("{drawing}.{tool}.png");
                std::fs::write(&out, png).expect("write the picture");
                println!("== {tool}: {caption}\n   written to {out}");
            }
            Err(e) => println!("== {tool}: error {e}"),
        }
    }
}
