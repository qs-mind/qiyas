//! The validated command and later opening commands use the same per-run vertical defaults.

use qiyas_core::{Session, Source};
use serde_json::{json, Value};

fn run(session: &mut Session, input: Value) -> Value {
    serde_json::to_value(session.run_input(&input, Source::User).unwrap().outcome).unwrap()
}

#[test]
fn per_run_heights_set_opening_heads_before_drawing_and_when_adding_later() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        let mut session = Session::new_blank(&dir.path().join("defaults.dxf"), units);
        let wall = run(
            &mut session,
            json!({
                "type":"walls", "thickness":0.2/factor, "height":3.0,
                "runs":[{"points":[[0,0],[6.0/factor,0]],"height":1.8}],
                "openings":[{"run":0,"at":1.0/factor,"width":0.8/factor,"kind":"door"}]
            }),
        );
        let groups = run(&mut session, json!({"type":"wall_groups"}));
        let first = &groups["groups"][0]["runs"][0]["openings"][0];
        assert_eq!(first["sill"], 0.0);
        assert_eq!(first["head"], 1.8);
        run(
            &mut session,
            json!({
                "type":"add_wall_openings", "centreline":wall["centrelines"][0],
                "openings":[{"at":4.0/factor,"width":1.0/factor,"kind":"window"}]
            }),
        );
        let groups = run(&mut session, json!({"type":"wall_groups"}));
        let later = &groups["groups"][0]["runs"][0]["openings"][1];
        assert_eq!(later["sill"], 0.9);
        assert_eq!(later["head"], 1.8);
        assert_eq!(session.history_steps().0.len(), 2);
    }
}
