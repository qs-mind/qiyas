#![no_main]

use libfuzzer_sys::fuzz_target;
use qiyas_core::{registry::parse_command, Command, DrawSpec, Session, Source};
use serde_json::Value;

fn in_memory(command: &Command) -> bool {
    match command {
        Command::Draw(DrawSpec::Line { .. } | DrawSpec::Circle { .. })
        | Command::Text { .. }
        | Command::Records { .. } => true,
        Command::Batch { commands, .. } => commands.iter().all(in_memory),
        _ => false,
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    let Ok(raw) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let dir = tempfile::tempdir().expect("temporary drawing folder");
    let mut session = Session::new_blank(&dir.path().join("input.dxf"), 6);
    let report = session.validate_input(&raw);
    // Valid file commands stop at validation; invalid inputs reach the rejection path.
    if !report.valid || parse_command(raw.clone()).is_ok_and(|command| in_memory(&command)) {
        let source = match data.len() % 3 {
            0 => Source::User,
            1 => Source::Ai,
            _ => Source::Mcp,
        };
        let _ = session.run_input(&raw, source);
    }
});
