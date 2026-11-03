//! The office design run of 3 November 2026 (run 4): inputs Mizan sent that were refused,
//! reduced to what each needs. Each now does what Mizan meant, and says what it inferred.
use opencadcodec::EntityType;
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> qiyas_core::Reported {
    s.run_input(&command, Source::Ai).unwrap()
}

fn added(s: &mut Session, command: Value) -> u64 {
    let Outcome::Added { handles } = run(s, command).outcome else {
        panic!("added")
    };
    handles[0]
}

/// A sized A1 sheet, selected, and its block.
fn sheet(s: &mut Session, name: &str) -> String {
    run(
        s,
        json!({"type":"new_layout","name":name,"paper":"ISO A1","orientation":"landscape"}),
    );
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| sh.name == name)
        .unwrap()
        .block;
    s.set_sheet(&block).unwrap();
    block
}

fn overall_viewport(s: &Session, block: &str) -> u64 {
    s.drawing
        .doc
        .entities_in_block(block)
        .find_map(|e| match e {
            EntityType::Viewport(v) if v.id == 1 => Some(v.common.handle.value()),
            _ => None,
        })
        .expect("a layout has its overall viewport")
}

#[test]
fn paper_corners_with_a_model_window_frame_it_on_paper_and_centre_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[-1.8,-2.3],"b":[21,11.1]}),
    );
    sheet(&mut s, "A-101 Furnished office");
    let out = run(
        &mut s,
        json!({"type":"mview","corner1":[25,90],"corner2":[579,410],
            "model_bounds":[[-1.8,-2.3],[21,11.1]],"scale":"1:50"}),
    );
    let Outcome::Added { .. } = out.outcome else {
        panic!("a viewport")
    };
    let view = &s.paper_views()[0];
    assert_eq!(view.rect, [[25.0, 90.0], [579.0, 410.0]]);
    assert!((view.centre[0] - 9.6).abs() < 1e-9 && (view.centre[1] - 4.4).abs() < 1e-9);
    assert!((view.scale - 20.0).abs() < 1e-9, "1:50 in metres");
    assert!(out.defaults_used.iter().any(|d| d.field == "/centre"));

    // Without a scale, the largest one that shows the whole window.
    let out = run(
        &mut s,
        json!({"type":"mview","corner1":[25,90],"corner2":[282,410],"model_bounds":[[24,-1.15],[35.5,9.85]]}),
    );
    let view = s.paper_views().into_iter().last().unwrap();
    assert!((view.scale - 257.0 / 11.5).abs() < 1e-9, "{}", view.scale);
    assert!(out.defaults_used.iter().any(|d| d.field == "/scale"));
}

#[test]
fn the_overall_viewport_as_target_means_the_sheets_model_viewport() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[20,10]}),
    );
    let block = sheet(&mut s, "A-102 Interior details");
    let overall = overall_viewport(&s, &block);

    // No model viewport yet: one is made.
    let out = run(
        &mut s,
        json!({"type":"mview","target":{"handle":overall},"corner1":[25,90],"corner2":[282,410],"centre":[10,5],"scale":"1:50"}),
    );
    let Outcome::Added { handles } = out.outcome else {
        panic!("a new viewport")
    };
    assert!(out
        .advice
        .iter()
        .any(|a| a["code"] == "overall_viewport_redirected"));
    // One model viewport: that one is resized, keeping its handle.
    let out = run(
        &mut s,
        json!({"type":"mview","target":{"handle":overall},"corner1":[25,90],"corner2":[579,410],"centre":[10,5],"scale":"1:50"}),
    );
    assert!(matches!(out.outcome, Outcome::Changed));
    let views = s.paper_views();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].handle, handles[0]);
    assert_eq!(views[0].rect, [[25.0, 90.0], [579.0, 410.0]]);
    // Two: Mizan must say which, and is told their handles.
    added(
        &mut s,
        json!({"type":"mview","corner1":[600,90],"corner2":[800,300]}),
    );
    let error = s
        .run_input(
            &json!({"type":"mview","target":{"handle":overall},"corner1":[25,90],"corner2":[579,410]}),
            Source::Ai,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains(&handles[0].to_string()), "{error}");
}

#[test]
fn an_edit_of_a_text_on_another_sheet_runs_on_that_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    sheet(&mut s, "A-101 Furnished office");
    let scale_text = added(
        &mut s,
        json!({"type":"text","at":[500,40],"height":5,"value":"1:100"}),
    );
    s.set_sheet("*Model_Space").unwrap();
    let out = run(
        &mut s,
        json!({"type":"set_property","target":{"handle":scale_text},"prop":"text","value":"1:50"}),
    );
    assert!(out
        .defaults_used
        .iter()
        .any(|d| d.field == "/sheet" && d.value == json!("A-101 Furnished office")));
    assert_eq!(s.sheet, "*Model_Space", "the view stays where it was");
    assert_eq!(
        s.entity_info(scale_text, &[]).unwrap().text.as_deref(),
        Some("1:50")
    );
    // Inside a batch too.
    run(
        &mut s,
        json!({"type":"batch","commands":[{"type":"set_property","target":{"handle":scale_text},"prop":"text","value":"Scale 1:50"}]}),
    );
    assert_eq!(
        s.entity_info(scale_text, &[]).unwrap().text.as_deref(),
        Some("Scale 1:50")
    );
}

#[test]
fn loops_named_x_and_y_are_ordinary_names_unless_a_loop_runs_along_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    run(
        &mut s,
        json!({"type":"define_block","name":"LIGHT_LINEAR_120","base":[0,0],"entities":[
            {"type":"Line","data":{"start":{"x":-0.56,"y":0,"z":0},"end":{"x":0.56,"y":0,"z":0}}}]}),
    );
    // The ceiling lights of the office run, a 3 x 2 grid.
    let out = run(
        &mut s,
        json!({"type":"generate","loops":[{"count":3,"var":"x"},{"count":2,"var":"y"}],
            "commands":[{"type":"insert_block","name":"LIGHT_LINEAR_120","at":["=1.7+x*1.55","=24.5+y*1.68"]}]}),
    );
    let Outcome::Generated { added, .. } = out.outcome else {
        panic!("generated")
    };
    assert_eq!(added.len(), 6);
    // With `along`, x, y and angle are the station's and still taken.
    let error = s
        .run_input(
            &json!({"type":"generate","loops":[{"var":"x","along":{"points":[[0,0],[10,0]],"count":3}}],
                "commands":[{"type":"insert_block","name":"LIGHT_LINEAR_120","at":["=x","=y"]}]}),
            Source::Ai,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("along"), "{error}");
}
