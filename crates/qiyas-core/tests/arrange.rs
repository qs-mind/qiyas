use opencadcodec::{EntityType, Handle};
use qiyas_core::{check::CheckScope, Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}
fn added(s: &mut Session, command: Value) -> u64 {
    match run(s, command) {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}
fn check(s: &Session, handles: Vec<u64>, rule: &str) -> qiyas_core::check::CheckReport {
    s.check_drawing(&CheckScope {
        handles,
        rules: vec![rule.into()],
        ..Default::default()
    })
}
fn points(s: &Session, h: u64) -> Vec<[f64; 2]> {
    let e = s.drawing.doc.get_entity(Handle::new(h)).unwrap();
    qiyas_geom::flatten(&s.drawing.doc, e, &qiyas_geom::Xform::identity(), 0.001)
        .into_paths()
        .into_iter()
        .flat_map(|p| p.points)
        .collect()
}
fn fixture(s: &mut Session, k: f64) -> (u64, u64, u64) {
    let plan = added(
        s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[10.0*k,8.0*k]}),
    );
    let label = added(
        s,
        json!({"type":"text","at":[2.0*k,2.0*k],"height":0.25*k,"value":"Ground floor"}),
    );
    let table = added(
        s,
        json!({"type":"table","at":[2.0*k,7.0*k],"rows":2,"cols":2,
        "row_height":0.5*k,"col_width":1.5*k,"data":[["Room","Area"],["Living","20"]]}),
    );
    (plan, label, table)
}

#[test]
fn given_parts_keep_geometry_and_order_and_undo_as_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Villa.dwg"), 6);
    let (plan, label, table) = fixture(&mut s, 1.0);
    let before: Vec<_> = [plan, label, table].map(|h| points(&s, h)).into();
    let steps = s.history_steps().0.len();
    let Outcome::Arranged { report } = run(
        &mut s,
        json!({"type":"arrange",
        "items":[{"handles":[plan,label],"label":"Ground floor"},{"handles":[table]}],
        "origin":[20,20],"columns":2,"gap_mm":15}),
    ) else {
        panic!("arrangement")
    };
    assert_eq!(report.moves.len(), 2);
    assert_eq!(report.moves[0].handles, vec![plan, label]);
    assert!(report.extents.is_some());
    assert_eq!(s.history_steps().0.len(), steps + 1);
    for (i, h) in [plan, label, table].into_iter().enumerate() {
        let m = &report.moves[usize::from(i == 2)];
        let after = points(&s, h);
        assert_eq!(after.len(), before[i].len());
        for (a, b) in after.iter().zip(&before[i]) {
            for axis in 0..2 {
                assert!((a[axis] - b[axis] - (m.to[axis] - m.from[axis])).abs() < 1e-8);
            }
        }
    }
    assert!(check(&s, vec![], "part_overlap").findings.is_empty());
    assert!(s.undo());
    for (i, h) in [plan, label, table].into_iter().enumerate() {
        assert_eq!(points(&s, h), before[i]);
    }
    assert_eq!(s.history_steps().0.len(), steps);
}

#[test]
fn automatic_parts_find_a_table_inside_a_plan_and_separate_it_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("Villa.dwg"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
        let (plan, label, table) = fixture(&mut s, k);
        for touched in [plan, label, table] {
            let report = check(&s, vec![touched], "part_overlap");
            assert_eq!(report.findings.len(), 1, "units {units}, touched {touched}");
            assert!(report.findings[0].handles.contains(&plan));
            assert!(report.findings[0].handles.contains(&table));
            assert!(report.findings[0].message.contains("arrange"));
        }
        let Outcome::Arranged { report } = run(&mut s, json!({"type":"arrange","columns":2}))
        else {
            panic!("arrangement")
        };
        assert_eq!(report.moves.len(), 2);
        assert!(report
            .moves
            .iter()
            .any(|m| m.handles.contains(&plan) && m.handles.contains(&label)));
        let m = &report.moves[1];
        assert!((m.to[0] - report.moves[0].to[0] - 11.5 * k).abs() < 1e-7);
        assert!(check(&s, vec![], "part_overlap").findings.is_empty());
        // A later edit must bring the finding back even when only the table was touched.
        run(
            &mut s,
            json!({"type":"modify","kind":"move","targets":[{"handle":table}],
            "delta":[m.from[0]-m.to[0],m.from[1]-m.to[1]]}),
        );
        assert!(!check(&s, vec![table], "part_overlap").findings.is_empty());
    }
}

#[test]
fn arranging_only_a_table_finds_space_without_moving_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("Villa.dwg"), 6);
    let (plan, label, table) = fixture(&mut s, 1.0);
    let before = [points(&s, plan), points(&s, label)];
    run(
        &mut s,
        json!({"type":"arrange","items":[{"handles":[table]}]}),
    );
    assert_eq!([points(&s, plan), points(&s, label)], before);
    assert!(check(&s, vec![], "part_overlap").findings.is_empty());
}

#[test]
/// Line work whose extents overlap is one plan (a villa's outer and inner wall rings overlap
/// without one containing the other); a plan stacked on another can be split with items.
fn overlapping_line_work_is_one_part_and_items_split_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("plans.dwg"), 6);
    for a in [0.0, 3.0] {
        added(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[a,a],"b":[a+10.0,a+8.0]}),
        );
    }
    assert!(check(&s, vec![], "part_overlap").findings.is_empty());
    let handles: Vec<u64> = s
        .drawing
        .doc
        .entities_in_block("*Model_Space")
        .map(|e| e.common().handle.value())
        .collect();
    run(
        &mut s,
        json!({"type":"arrange","items":[{"handles":[handles[0]]},{"handles":[handles[1]]}]}),
    );
    assert!(check(&s, vec![], "part_overlap").findings.is_empty());
}

#[test]
fn bounds_items_missing_handles_and_batch_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("parts.dwg"), 6);
    let h = added(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
    );
    let before = points(&s, h);
    let error = s
        .run_input(
            &json!({"type":"arrange","items":[{"handles":[h]},{"handles":[u64::MAX]}]}),
            Source::User,
        )
        .unwrap_err();
    assert!(error.to_string().contains("does not exist"));
    assert_eq!(points(&s, h), before);
    let steps = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"batch","commands":[{"type":"arrange",
        "items":[{"bounds":[[-1,-1],[6,5]]}],"origin":[10,10]}]}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    assert!(s.undo());
    assert_eq!(points(&s, h), before);
}

#[test]
fn a_content_sheet_needs_a_title_block_and_arrange_keeps_it_clear() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("sheet.dwg"), 6);
    run(
        &mut s,
        json!({"type":"new_layout","name":"Review","paper":"ISO A3","orientation":"landscape"}),
    );
    let sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.name == "Review")
        .unwrap()
        .block;
    run(&mut s, json!({"type":"set_sheet","block":sheet}));
    assert!(check(&s, vec![], "sheet_without_title_block")
        .findings
        .is_empty());
    let note = added(
        &mut s,
        json!({"type":"mtext","at":[260,50],"width":40,"height":3,"value":"For review\nCheck dimensions"}),
    );
    assert_eq!(
        check(&s, vec![note], "sheet_without_title_block")
            .findings
            .len(),
        1
    );
    let title = match run(
        &mut s,
        json!({"type":"title_block","fields":{"title":"Villa plans"}}),
    ) {
        Outcome::TitleBlock { handles, .. } => handles,
        other => panic!("{other:?}"),
    };
    assert!(check(&s, vec![], "sheet_without_title_block")
        .findings
        .is_empty());
    let before: Vec<_> = title.iter().map(|h| points(&s, *h)).collect();
    assert!(!check(&s, vec![note], "part_overlap").findings.is_empty());
    let viewport = added(
        &mut s,
        json!({"type":"mview","corner1":[250,30],"corner2":[300,70],"centre":[0,0],"scale":"1:100"}),
    );
    let old = match s.drawing.doc.get_entity(Handle::new(viewport)).unwrap() {
        EntityType::Viewport(v) => v.clone(),
        _ => panic!("viewport"),
    };
    run(&mut s, json!({"type":"set_sheet","block":"*Model_Space"}));
    run(&mut s, json!({"type":"arrange","sheet":"Review"}));
    assert_eq!(s.sheet, "*Model_Space");
    run(&mut s, json!({"type":"set_sheet","block":sheet}));
    assert_eq!(s.sheet, sheet);
    assert!(check(&s, vec![], "part_overlap").findings.is_empty());
    for (h, before) in title.iter().zip(before) {
        assert_eq!(points(&s, *h), before);
    }
    let EntityType::Viewport(v) = s.drawing.doc.get_entity(Handle::new(viewport)).unwrap() else {
        panic!("viewport")
    };
    assert_eq!(v.view_center, old.view_center);
    assert_eq!(v.view_target, old.view_target);
    assert_eq!(v.scale(), old.scale());
    assert_eq!(v.width, old.width);
    assert_eq!(v.height, old.height);
    assert!(v.center.x >= 20.0 && v.center.y > 90.0);
}

#[test]
fn export_defaults_use_the_export_folder_and_keep_previous_names() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("exports");
    std::fs::create_dir_all(&folder).unwrap();
    let mut s = Session::new_blank(&dir.path().join("Villa plans.dwg"), 6);
    s.set_export_directory(folder.clone());
    let prepare = |s: &mut Session| match run(
        s,
        json!({"type":"deliverable","kind":"export","report":"register","format":"excel"}),
    ) {
        Outcome::Deliverable { export } => export,
        other => panic!("{other:?}"),
    };
    let first = prepare(&mut s);
    assert_eq!(first.path, folder.join("Villa plans - Register.xlsx"));
    std::fs::write(&first.path, "previous export").unwrap();
    let second = prepare(&mut s);
    assert_eq!(second.path, folder.join("Villa plans - Register 2.xlsx"));
    assert_eq!(
        std::fs::read_to_string(&first.path).unwrap(),
        "previous export"
    );
}
