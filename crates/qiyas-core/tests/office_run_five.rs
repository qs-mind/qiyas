//! The office design run of 3 November 2026 (run 5): its four refused inputs, reduced.
use opencadcodec::{EntityType, Handle};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> qiyas_core::Reported {
    s.run_input(&command, Source::Ai).unwrap()
}

fn added(s: &mut Session, command: Value) -> Vec<u64> {
    let report = run(s, command);
    match report.outcome {
        Outcome::Added { handles } => handles,
        Outcome::Generated { added, .. } => added,
        other => panic!("added: {other:?}"),
    }
}

fn text_of(s: &Session, h: u64) -> String {
    s.entity_info(h, &[]).unwrap().plain_text.unwrap()
}

#[test]
fn a_range_without_step_counts_by_one_towards_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    // The office run's floor joints: from 0 to 6, no step.
    let lines = added(
        &mut s,
        json!({"type":"generate","loops":[{"from":0,"to":6,"var":"i"}],"commands":[
            {"type":"draw","kind":"line","a":["=8.5+i*0.6",3.49],"b":["=8.5+i*0.6",5.3]}]}),
    );
    assert_eq!(lines.len(), 7);
    let down = added(
        &mut s,
        json!({"type":"generate","loops":[{"from":3,"to":1,"var":"i"}],"commands":[
            {"type":"draw","kind":"line","a":["=i",0],"b":["=i",1]}]}),
    );
    assert_eq!(down.len(), 3);
}

#[test]
fn record_updates_of_a_text_value_are_text_edits_in_the_same_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    let note = added(
        &mut s,
        json!({"type":"mtext","at":[30,8],"width":6,"value":"DESIGN NOTES\nFootprint 125 m²"}),
    )[0];
    let label = added(
        &mut s,
        json!({"type":"text","at":[1,1],"value":"OPEN OFFICE"}),
    )[0];
    let before = s
        .drawing
        .doc
        .get_entity(Handle::new(label))
        .unwrap()
        .common()
        .layer
        .clone();
    run(
        &mut s,
        json!({"type":"set_records","collection":"entities","updates":[
            {"key":label,"path":"/common/layer","value":"A-ANNO"},
            {"key":note,"path":"/value","value":"DESIGN & MATERIAL NOTES\nFootprint 125.44 m²"},
            {"key":label,"path":"/value","value":"01 OPEN OFFICE","expected":"OPEN OFFICE"}]}),
    );
    assert_eq!(
        text_of(&s, note),
        "DESIGN & MATERIAL NOTES\nFootprint 125.44 m²"
    );
    assert_eq!(text_of(&s, label), "01 OPEN OFFICE");
    let EntityType::Text(text) = s.drawing.doc.get_entity(Handle::new(label)).unwrap() else {
        panic!("text")
    };
    assert_eq!(text.common.layer, "A-ANNO");
    // A stale `expected` refuses the whole command.
    assert!(s
        .run_input(
            &json!({"type":"set_records","collection":"entities","updates":[
                {"key":label,"path":"/value","value":"X","expected":"OPEN OFFICE"}]}),
            Source::Ai,
        )
        .is_err());
    assert_eq!(text_of(&s, label), "01 OPEN OFFICE");
    assert!(s.undo());
    assert_eq!(text_of(&s, label), "OPEN OFFICE");
    assert_eq!(text_of(&s, note), "DESIGN NOTES\nFootprint 125 m²");
    let EntityType::Text(text) = s.drawing.doc.get_entity(Handle::new(label)).unwrap() else {
        panic!("text")
    };
    assert_eq!(text.common.layer, before);
}

#[test]
fn a_dimension_edit_and_a_flat_decision_read_as_meant() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    let dimension = added(
        &mut s,
        json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[12.8,0],"line_point":[6.4,-1]}),
    )[0];
    run(
        &mut s,
        json!({"type":"dim_edit","target":{"handle":dimension},"op":{"text_position":[6.4,-1.6]}}),
    );
    // The ceiling decision, its fields beside kind.
    let out = run(
        &mut s,
        json!({"type":"decision","kind":"set","name":"ceiling_concept","value":{"downlights":11,"linear_lights":7},
            "reason":"Concept fixture counts","source":"mizan_design","status":"answered",
            "citation":"E-101 ceiling drawing"}),
    );
    let defaults: Vec<_> = out.defaults_used.iter().map(|d| d.field.clone()).collect();
    assert!(
        defaults.iter().any(|f| f.ends_with("/unit")),
        "{defaults:?}"
    );
    assert!(
        defaults.iter().any(|f| f.ends_with("/set_by")),
        "{defaults:?}"
    );
    assert!(
        !defaults.iter().any(|f| f.ends_with("/reason")),
        "{defaults:?}"
    );
}

#[test]
fn making_a_sheet_twice_says_how_to_change_the_one_there() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("office.dwg"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[12.8,9.8]}),
    );
    let sheet = json!({"type":"make_sheet","name":"A-101 Office plan","paper":"A1","orientation":"landscape",
        "model_bounds":[[-2.1,-2.6],[15,12]],"scale":"1:50"});
    run(&mut s, sheet.clone());
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| sh.name == "A-101 Office plan")
        .unwrap()
        .block;
    let view = s.paper_views_of(&block)[0].handle;
    let error = s.run_input(&sheet, Source::Ai).unwrap_err().to_string();
    assert!(
        error.contains("mview") && error.contains(&view.to_string()),
        "{error}"
    );
}
