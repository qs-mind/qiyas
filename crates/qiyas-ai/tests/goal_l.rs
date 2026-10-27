//! Scripted provider replays and revision readbacks; no model or network access.
use crate::scripted;
use qiyas_ai::surface::{CAPTURE, EXECUTE, READ};
use qiyas_ai::{Agent, AgentEvent, AgentSettings, ContentBlock, ResultPart};
use qiyas_core::Session;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn villa_replay_finishes_with_all_openings_and_visible_findings() {
    let dir = tempfile::tempdir().unwrap();
    let mut drawing = Session::new_blank(&dir.path().join("villa.dxf"), 6);
    drawing
        .apply_settings_method(Some(&qiyas_core::measurement::RuleSet::named(
            "POMI (RICS)",
        )))
        .unwrap();
    let session = Arc::new(Mutex::new(Some(drawing)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let fixture: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/villa-openings.json")).unwrap();
    let mut calls = Vec::new();
    for input in fixture {
        calls.push((EXECUTE, input["input"].clone()));
        calls.push((
            READ,
            json!({"op":"records","args":{"collection":"entities","limit":200}}),
        ));
        calls.push((
            READ,
            json!({"op":"run","args":{"command":{"type":"wall_groups"}}}),
        ));
    }
    calls.push((READ, json!({"op":"check"})));
    calls.push((CAPTURE, json!({"source":"picture","args":{"width":1200}})));
    let mut agent = Agent::new(
        Box::new(scripted::Script::new(calls, requests.clone())),
        session.clone(),
        AgentSettings::default(),
    );
    let (tx, mut rx) = mpsc::channel(512);
    agent
        .run_turn(
            "Complete the villa openings in this drawing and report the remaining findings.".into(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    assert!(
        matches!(events.last(), Some(AgentEvent::Done { .. })),
        "{events:?}"
    );
    let requests = requests.lock().unwrap();
    let messages = scripted::observed_messages(&requests);
    let mut errors = Vec::new();
    for message in &messages {
        for block in &message.content {
            if let ContentBlock::ToolResult {
                content, is_error, ..
            } = block
            {
                for part in content {
                    if let ResultPart::Text(text) = part {
                        if let Ok(value) = serde_json::from_str::<Value>(text) {
                            assert!(value.get("revision").is_some(), "{value}");
                            if *is_error {
                                errors.push(value);
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        !errors.is_empty(),
        "the logged overlapping retries still fail"
    );
    assert!(
        errors
            .iter()
            .all(|e| e["message"].as_str().is_some_and(|m| m.contains("fit")
                && (m.contains("overlap")
                    || m.contains("junction")
                    || m.contains("does not fit")))),
        "{errors:?}"
    );
    let guard = session.lock().unwrap();
    let drawing = guard.as_ref().unwrap();
    let groups = drawing.wall_groups(None).unwrap();
    assert_eq!(
        groups
            .iter()
            .flat_map(|g| &g.runs)
            .map(|r| r.openings.len())
            .sum::<usize>(),
        44
    );
    for group in &groups {
        for handle in &group.handles {
            assert!(drawing
                .drawing
                .doc
                .get_entity(opencadcodec::Handle::new(*handle))
                .is_some());
        }
    }
    if let Some(root) = std::env::var_os("QIYAS_GOAL_L_EVIDENCE") {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("villa-events.json"),
            serde_json::to_vec_pretty(&events).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("villa-transcript.json"),
            serde_json::to_vec_pretty(&messages).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("villa-final-groups.json"),
            serde_json::to_vec_pretty(&groups).unwrap(),
        )
        .unwrap();
        for message in &messages {
            for block in &message.content {
                if let ContentBlock::ToolResult { content, .. } = block {
                    for part in content {
                        if let ResultPart::Png(data) = part {
                            std::fs::write(
                                root.join("villa-final.png"),
                                qiyas_ai::from_base64(data).unwrap(),
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }
}
