//! Full offline clinic replay, including readbacks and exact rollback assertions.
include!("../examples/clinic_performance.rs");
#[path = "../../../tests/support/performance.rs"]
mod performance;

#[test]
#[ignore = "full clinic replay; run explicitly while changing the engine"]
fn clinic_replay() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(replay(Some(dir.path())).len(), 85);
}

#[test]
#[ignore = "release speed guard: scripts/check.sh"]
#[allow(clippy::assertions_on_constants)]
fn clinic_speed() {
    assert!(!cfg!(debug_assertions), "run the speed guard in release");
    let reference = performance::reference();
    let dir = tempfile::tempdir().unwrap();
    let timings = replay(Some(dir.path()));
    assert_eq!(timings.len(), 85);
    let maximum = timings.into_iter().max().unwrap();
    // Guards today's speed (4.5x the fixed CPU reference on this laptop, 6.2x on the GitHub
    // runner, 25 October 2026; every call under a second in release here). The goal of 2x, as
    // the villa guard allows, is not met yet: broad batch deferral (Goal P5/P6) is still
    // open. Lower this when that lands.
    performance::assert_relative(maximum, reference, 8.0, "largest clinic call");
}

#[test]
fn step_references_read_completed_opening_groups_before_editing() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("references.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        let wall = json!(s
            .run_input(
                &json!({"type":"walls","thickness":0.2*k,
            "runs":[{"points":[[0,0],[12.0*k,0]]}]}),
                Source::User
            )
            .unwrap());
        let host = wall["centrelines"][0].as_u64().unwrap();
        let result = surface::run_session(&mut s,surface::EXECUTE,"batch",&json!({"commands":[
            {"type":"add_wall_openings","centreline":host,"openings":[{"at":2.0*k,"width":0.8*k,"kind":"door"}]},
            {"type":"add_wall_openings","centreline":host,"openings":[{"at":5.0*k,"width":0.8*k,"kind":"door"}]},
            {"type":"edit_wall_opening","insert":{"$step":1,"pointer":"/outcome/inserts/1"},"width":1.0*k}
        ]}),Source::Ai).unwrap();
        let ToolOutput::Json(result) = result else {
            panic!("JSON outcome");
        };
        let door = result["outcome"]["steps"][1]["outcome"]["inserts"][1]
            .as_u64()
            .unwrap();
        let groups = s.wall_groups(None).unwrap();
        let openings = &groups[0].runs[0].openings;
        assert_eq!(openings.len(), 2);
        assert_eq!(
            openings
                .iter()
                .find(|o| o.insert == door)
                .unwrap()
                .opening
                .width,
            1.0 * k
        );
        assert!(s.undo());
        assert!(s.wall_groups(None).unwrap()[0].runs[0].openings.is_empty());
        assert!(s.redo());
        assert_eq!(s.wall_groups(None).unwrap()[0].runs[0].openings.len(), 2);
    }
}
