use qiyas_core::{Command, ConflictKind, Session, Source};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) {
    s.run_input(&v, Source::Ai).unwrap();
}
fn setup(path: &std::path::Path) -> Session {
    let mut s = Session::new_blank(path, 6);
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{"name":"Specimen","category":"arbitrary","geometry":"region","properties":{"mark":{"kind":"text","default":"X"},"grid":{"kind":"text","default":"A/1"}},"quantities":{"surface":"area"}}}),
    );
    run(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Specimen","points":[[0,0],[4,0],[4,3],[0,3]]}]}),
    );
    let c = json!({"source":"User measurement brief","clause":"Test arithmetic"});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":{"name":"sample","title":"Sample","based_on":"User","source_note":"Geometric","rules":[{"id":"surface","element_type":"Specimen","unit":"m2","tolerance":{"percent":2},"terms":[{"label":"Area","operation":"add","factors":["surface"],"citation":c}],"citation":c}]}}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[{"rule":"surface","code":"A","description_en":"Surface","description_ar":"مساحة","section":"Example","trade":"","unit":"{unit}","citation":c}]}),
    );
    s
}
fn client(s: &mut Session, items: Value) {
    run(
        s,
        json!({"type":"set_boq","boq":{"source":"client.xlsx","imported":"","items":items}}),
    );
}
fn check(s: &mut Session) {
    run(s, json!({"type":"check"}));
}
#[test]
fn mapped_missing_quantity_units_duplicates_and_no_evidence_are_checked() {
    let d = tempfile::tempdir().unwrap();
    let mut s = setup(&d.path().join("p.dxf"));
    client(&mut s, json!([]));
    check(&mut s);
    assert_eq!(s.conflicts.len(), 1);
    assert_eq!(s.conflicts[0].kind, ConflictKind::MissingFromBoq);
    let q = s.conflicts[0].query.as_ref().unwrap();
    assert_eq!(q.references[0].grid.as_deref(), Some("A/1"));
    assert!(!q.clouds.is_empty());
    client(
        &mut s,
        json!([{"code":"A","description":"Surface","unit":"m2","quantity":12.2,"row":1}]),
    );
    check(&mut s);
    assert!(s.conflicts.is_empty());
    client(
        &mut s,
        json!([{"code":"A","description":"Surface","unit":"m2","quantity":18,"row":1,"tolerance":90}]),
    );
    check(&mut s);
    assert_eq!(s.conflicts[0].kind, ConflictKind::QuantityMismatch);
    client(
        &mut s,
        json!([{"code":"A","description":"Surface","unit":"m","quantity":12,"row":1},{"code":"A","description":"Surface","unit":"m","quantity":12,"row":2},{"code":"Z","description":"Unseen","unit":"pcs","quantity":3,"row":3}]),
    );
    check(&mut s);
    for kind in [
        ConflictKind::UnitDoubt,
        ConflictKind::DuplicateItem,
        ConflictKind::NotInDrawings,
    ] {
        assert!(s.conflicts.iter().any(|c| c.kind == kind));
    }
}
#[test]
fn cited_rules_clouds_status_and_queries_round_trip_and_undo_together() {
    let d = tempfile::tempdir().unwrap();
    for extension in ["dwg", "dxf"] {
        let path = d.path().join(format!("p.{extension}"));
        let mut s = setup(&path);
        let c = json!({"source":"Supplied document","clause":"Table 2"});
        run(
            &mut s,
            json!({"type":"set_knowledge","kind":"tables","name":"schedule","data":{"citation":c,"rows":[["X","2","15"],["Y","1","5"]]}}),
        );
        for (name, test) in [
            (
                "count",
                json!({"kind":"schedule","table":{"kind":"document","name":"schedule"},"key_column":0,"value_column":1,"elements":{},"property":"mark"}),
            ),
            (
                "size",
                json!({"kind":"schedule","table":{"kind":"document","name":"schedule"},"key_column":0,"value_column":2,"elements":{},"property":"mark","measure":"surface","unit":"m2"}),
            ),
            (
                "host",
                json!({"kind":"relation","left":{},"right":{},"join":"host","minimum":1}),
            ),
            (
                "spec",
                json!({"kind":"document","table":{"kind":"document","name":"schedule"},"key_column":0,"elements":{},"property":"type"}),
            ),
            (
                "ratio",
                json!({"kind":"quantity_relation","left":{},"right":{},"left_measure":"surface","right_measure":"surface","factor":2}),
            ),
        ] {
            run(
                &mut s,
                json!({"type":"define_check","name":name,"rule":{"citation":c,"severity":"error","question_en":"Confirm the evidence?","question_ar":"يرجى تأكيد الدليل؟","test":test}}),
            );
        }
        let n = s.history_steps().0.len();
        check(&mut s);
        assert_eq!(s.history_steps().0.len(), n + 1);
        assert_eq!(s.conflicts.len(), 7);
        assert_eq!(s.list_comments().len(), 7);
        let findings = s.conflicts.clone();
        assert!(s.undo());
        assert!(s.conflicts.is_empty());
        assert!(s.list_comments().is_empty());
        assert!(s.redo());
        assert_eq!(s.conflicts, findings);
        run(
            &mut s,
            json!({"type":"resolve_conflict","id":findings[0].id,"status":"resolved","note":"Verified"}),
        );
        let queries = s.conflicts.clone();
        s.save_as(&path).unwrap();
        let mut reopened = Session::open_standalone(&path).unwrap();
        assert_eq!(reopened.conflicts, queries);
        assert_eq!(reopened.list_comments().len(), 7);
        check(&mut reopened);
        assert_eq!(reopened.list_comments().len(), 7);
        assert_eq!(reopened.conflicts[0].status, queries[0].status);
    }
}
#[test]
fn missing_evidence_and_invalid_rules_do_not_leave_partial_queries() {
    let d = tempfile::tempdir().unwrap();
    let mut s = setup(&d.path().join("p.dxf"));
    let n = s.history_steps().0.len();
    let bad = json!({"type":"set_knowledge","kind":"checks","name":"bad","data":{"citation":{"source":"","clause":""},"question_en":"?","question_ar":"؟","test":{"kind":"relation","left":{},"right":{},"join":"host","minimum":1}}});
    assert!(s.run_input(&bad, Source::Ai).is_err());
    assert_eq!(s.history_steps().0.len(), n);
    run(
        &mut s,
        json!({"type":"define_check","name":"missing","rule":{"citation":{"source":"Brief","clause":"1"},"question_en":"?","question_ar":"؟","test":{"kind":"document","table":{"kind":"document","name":"absent"},"key_column":0,"elements":{},"property":"type"}}}),
    );
    let n = s.history_steps().0.len();
    assert!(s
        .run_as(Command::Check { tolerance: None }, Source::Ai)
        .is_err());
    assert_eq!(s.history_steps().0.len(), n);
    assert!(s.conflicts.is_empty());
    assert!(s.list_comments().is_empty());
}

#[test]
fn schedule_sizes_compare_each_instance_and_method_bands_keep_their_units() {
    let d = tempfile::tempdir().unwrap();
    let mut s = setup(&d.path().join("p.dxf"));
    run(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Specimen","points":[[10,0],[14,0],[14,3],[10,3]]}]}),
    );
    let c = json!({"source":"User schedule","clause":"Surface size"});
    run(
        &mut s,
        json!({"type":"set_knowledge","kind":"tables","name":"sizes","data":{"citation":c,"rows":[["X","12"]]}}),
    );
    run(
        &mut s,
        json!({"type":"define_check","name":"sizes","rule":{"citation":c,"question_en":"Confirm the size?","question_ar":"يرجى تأكيد المقاس؟","test":{"kind":"schedule","table":{"kind":"document","name":"sizes"},"key_column":0,"value_column":1,"elements":{},"property":"mark","measure":"surface","each":true}}}),
    );
    check(&mut s);
    assert!(s.conflicts.is_empty());
    let mut method =
        serde_json::to_value(s.measurement_selection().unwrap().effective().unwrap()).unwrap();
    method["rules"][0]["bands"] = json!([{"when":"surface > 10","unit":"pcs","terms":[{"label":"One per instance","operation":"add","factors":["1"],"citation":c}],"citation":c}]);
    method["rules"][0]["tolerance"] = json!({"absolute":2});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":method}),
    );
    client(
        &mut s,
        json!([{"code":"A","description":"Surface","unit":"m2","quantity":24,"row":1}]),
    );
    check(&mut s);
    assert_eq!(s.conflicts.len(), 1);
    assert_eq!(s.conflicts[0].kind, ConflictKind::UnitDoubt);
    client(
        &mut s,
        json!([{"code":"A","description":"Surface","unit":"pcs","quantity":0,"row":1}]),
    );
    check(&mut s);
    assert!(s.conflicts.is_empty());
    method["rules"][0]["tolerance"] = json!({"absolute":1});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":method}),
    );
    check(&mut s);
    assert_eq!(s.conflicts[0].kind, ConflictKind::QuantityMismatch);
}
