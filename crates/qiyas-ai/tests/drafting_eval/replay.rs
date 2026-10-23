//! Reproduce old drafting faults without passing them through today's text normalisation.

use opencadcodec::{EntityType, Line, Text, Vector3};
use qiyas_ai::surface::{self, READ};
use qiyas_ai::ToolOutput;
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn insert(session: &mut Session, mut entity: EntityType) -> u64 {
    entity.common_mut().owner_handle = session
        .drawing
        .doc
        .block_records
        .get("*Model_Space")
        .unwrap()
        .handle;
    entity.common_mut().layer = "0".into();
    session.drawing.doc.add_entity(entity).unwrap().value()
}

fn line(session: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    insert(
        session,
        EntityType::Line(Line::from_coords(a[0], a[1], 0.0, b[0], b[1], 0.0)),
    )
}

fn text(session: &mut Session, at: [f64; 2], value: &str) -> u64 {
    insert(
        session,
        EntityType::Text(Text::with_value(value, Vector3::new(at[0], at[1], 0.0)).with_height(0.2)),
    )
}

fn check(session: &mut Session, packs: &[&str]) -> Value {
    let input = json!({"op": "check", "args": json!({"packs": packs}).to_string()});
    let surface::Route::Session { tool, op, args } = surface::route(READ, &input).unwrap() else {
        panic!("check must route to the drawing session")
    };
    let ToolOutput::Json(report) =
        surface::run_session(session, &tool, &op, &args, Source::Ai).unwrap()
    else {
        panic!("check must return a JSON report")
    };
    report
}

#[test]
fn old_mizan_faults_are_reported_through_the_generic_read_surface() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("old-mizan.dwg"), 6);

    // These are deliberately raw entities: the replay preserves the old mistakes instead of
    // letting the current drafting commands repair them as they are created.
    let wall = line(&mut session, [0.0, 0.0], [5.0, 0.0]);
    let short = line(&mut session, [2.0, 3.0], [2.0, 0.1]);
    let crossing = line(&mut session, [4.0, 3.0], [4.0, -0.05]);

    // A closed 4 x 3 m room, with an incorrect 5.25 x 3.25 m label.
    for (a, b) in [
        ([10.0, 0.0], [14.0, 0.0]),
        ([14.0, 0.0], [14.0, 3.0]),
        ([14.0, 3.0], [10.0, 3.0]),
        ([10.0, 3.0], [10.0, 0.0]),
    ] {
        line(&mut session, a, b);
    }
    let wrong_size = text(&mut session, [10.5, 1.5], "BED 5.25m x 3.25m");
    let literal_newline = text(&mut session, [0.0, 5.0], r"KITCHEN\nGROUND FLOOR");

    let report = check(&mut session, &["general", "construction"]);
    let findings = report["findings"].as_array().unwrap();
    for (rule, pack, handle) in [
        ("dangling_end", "general", short),
        ("overshoot", "general", crossing),
        ("literal_newline", "general", literal_newline),
        ("room_size_text_differs", "construction", wrong_size),
        ("layer_zero", "general", wall),
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding["rule"] == rule
                    && finding["pack"] == pack
                    && finding["handles"]
                        .as_array()
                        .unwrap()
                        .contains(&json!(handle))
            }),
            "missing {rule} on handle {handle}: {report:#}"
        );
        assert!(report["counts"][rule].as_u64().unwrap() > 0, "{report:#}");
    }
    assert!(report["errors"].as_u64().unwrap() > 0, "{report:#}");
    assert!(!session.can_undo(), "checking must not alter the drawing");
}
