use qiyas_core::{
    knowledge::{self, Kind},
    revisions::ChangeKind,
    Session, Source,
};
use serde_json::{json, Value};
fn run(s: &mut Session, v: Value) -> Value {
    serde_json::to_value(s.run_input(&v, Source::Ai).unwrap()).unwrap()
}
fn line(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    run(s, json!({"type":"draw","kind":"line","a":a,"b":b}))["handles"][0]
        .as_u64()
        .unwrap()
}
fn knowledge(s: &mut Session) {
    let c = json!({"source":"Test brief","clause":"Path length"});
    run(
        s,
        json!({"type":"define_object_type","definition":{"name":"Observed","category":"any drawing","geometry":"path","quantities":{"measured":"geometry.length"}}}),
    );
    run(
        s,
        json!({"type":"recognition","kind":"define","rule":{"name":"paths","object_type":"Observed","match":{"entity_type":"Line"},"confidence":1.0}}),
    );
    run(s, json!({"type":"recognition","kind":"apply"}));
    run(
        s,
        json!({"type":"measurement","kind":"set_method","scope":"project","method":{"name":"user","title":"User","based_on":"brief","source_note":"Exact geometry","rules":[{"id":"L","element_type":"Observed","unit":"m","terms":[{"label":"Length","operation":"add","factors":["measured"],"citation":c}],"citation":c}]}}),
    );
    run(
        s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[{"rule":"L","code":"L","description_en":"Length","description_ar":"الطول","section":"","trade":"","unit":"m","citation":c}]}),
    );
    run(
        s,
        json!({"type":"define_check","name":"relation","rule":{"citation":c,"question_en":"Related?","question_ar":"مرتبط؟","test":{"kind":"relation","left":{},"right":{},"join":"host","minimum":0}}}),
    );
}
#[test]
fn carries_reapplies_matches_renumbered_exports_and_round_trips_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["dxf", "dwg"] {
        let ap = dir.path().join(format!("a.{ext}"));
        let bp = dir.path().join(format!("b.{ext}"));
        let mut a = Session::new_blank(&ap, 6);
        let changed = line(&mut a, [0., 0.], [5., 0.]);
        let removed = line(&mut a, [20., 0.], [23., 0.]);
        let matched = line(&mut a, [30., 0.], [32., 0.]);
        knowledge(&mut a);
        a.save_as(&ap).unwrap();
        let mut b = Session::new_blank(&bp, 6);
        for _ in 0..100 {
            b.drawing.doc.allocate_handle();
        }
        let bc = line(&mut b, [0., 0.], [6., 0.]);
        let bm = line(&mut b, [30., 0.], [32., 0.]);
        let added = line(&mut b, [40., 0.], [47., 0.]);
        let before = b.history_steps().0.len();
        let doc = b.drawing.doc.clone();
        let cmp = b
            .compare_revisions(ap.to_str().unwrap(), Some(0.0001))
            .unwrap();
        assert_eq!(doc, b.drawing.doc);
        let changes = &cmp
            .sheets
            .iter()
            .find(|s| s.sheet == "Model")
            .unwrap()
            .entities;
        assert!(changes.iter().any(|c| c.kind == ChangeKind::Matched
            && c.before.as_ref().unwrap().handle == matched
            && c.after.as_ref().unwrap().handle == bm));
        assert!(changes.iter().any(|c| c.kind == ChangeKind::Changed
            && c.before.as_ref().unwrap().handle == changed
            && c.after.as_ref().unwrap().handle == bc));
        assert!(
            changes
                .iter()
                .any(|c| c.kind == ChangeKind::Removed
                    && c.before.as_ref().unwrap().handle == removed)
        );
        assert!(changes
            .iter()
            .any(|c| c.kind == ChangeKind::Added && c.after.as_ref().unwrap().handle == added));
        run(&mut b, json!({"type":"carry_knowledge","from_path":ap}));
        assert_eq!(b.history_steps().0.len(), before + 1);
        assert_eq!(b.revision_identity(bm), a.revision_identity(matched));
        assert_eq!(b.measurement_report().unwrap().items[0].quantity, 15.);
        assert_eq!(
            knowledge::list(&b.drawing.doc, Kind::Checks).unwrap().len(),
            1
        );
        assert!(b.undo());
        assert!(b.object_types().unwrap().is_empty());
        assert!(b.redo());
        let before = b.history_steps().0.len();
        run(
            &mut b,
            json!({"type":"variation_report","layer":"Revision B changes"}),
        );
        let report = b.variation().unwrap().unwrap();
        assert_eq!(b.history_steps().0.len(), before + 1);
        let item = &report.items[0];
        assert_eq!(
            (item.a_quantity, item.b_quantity, item.delta),
            (10., 15., 5.)
        );
        assert_eq!(item.elements.len(), 3);
        assert_eq!(
            item.elements.iter().map(|e| e.delta).sum::<f64>(),
            item.delta
        );
        assert_eq!(b.list_comments().len(), 3);
        for c in b.list_comments() {
            assert_eq!(
                b.drawing
                    .doc
                    .get_entity(opencadcodec::Handle::new(c.handle))
                    .unwrap()
                    .common()
                    .layer,
                "Revision B changes"
            );
        }
        assert!(b.undo());
        assert!(b.list_comments().is_empty());
        assert!(b.variation().unwrap().is_none());
        assert!(b.redo());
        run(&mut b, json!({"type":"variation_report"}));
        assert_eq!(b.list_comments().len(), 3);
        let report = b.variation().unwrap().unwrap();
        b.save_as(&bp).unwrap();
        let reopened = Session::open_standalone(&bp).unwrap();
        assert_eq!(reopened.variation().unwrap().unwrap(), report);
        assert_eq!(reopened.list_comments().len(), 3);
        assert_eq!(reopened.revision_identity(bm), a.revision_identity(matched));
    }
}
#[test]
fn stable_handles_geometry_tolerance_and_sheet_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.dxf");
    let mut a = Session::new_blank(&path, 6);
    let h = line(&mut a, [1., 2.], [5., 2.]);
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    run(
        &mut b,
        json!({"type":"modify","kind":"move","targets":[{"handle":h}],"delta":[0.00001,0.]}),
    );
    let cmp = b
        .compare_revisions(path.to_str().unwrap(), Some(0.001))
        .unwrap();
    assert!(cmp
        .sheets
        .iter()
        .flat_map(|s| &s.entities)
        .all(|c| c.kind == ChangeKind::Matched));
    let cmp = b
        .compare_revisions(path.to_str().unwrap(), Some(0.000001))
        .unwrap();
    assert!(cmp
        .sheets
        .iter()
        .flat_map(|s| &s.entities)
        .any(|c| c.kind == ChangeKind::Changed));
    assert!(cmp.sheets.iter().any(|s| s.bounds.is_some()));
    let before = b.drawing.doc.clone();
    assert!(b
        .run_input(
            &json!({"type":"carry_knowledge","from_path":"missing.dxf"}),
            Source::Ai
        )
        .is_err());
    assert_eq!(before, b.drawing.doc);
}
#[test]
fn block_definition_changes_are_visible_on_every_instance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.dxf");
    let mut a = Session::new_blank(&path, 6);
    let def = run(
        &mut a,
        json!({"type":"define_block","name":"symbol","base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":2,"y":0,"z":0}}}]}),
    );
    let h = def["handles"][0].as_u64().unwrap();
    run(
        &mut a,
        json!({"type":"insert_block","name":"symbol","at":[0,0]}),
    );
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    if let Some(opencadcodec::EntityType::Line(l)) =
        b.drawing.doc.get_entity_mut(opencadcodec::Handle::new(h))
    {
        l.end.x = 3.;
    }
    let cmp = b.compare_revisions(path.to_str().unwrap(), None).unwrap();
    assert!(cmp
        .sheets
        .iter()
        .flat_map(|s| &s.entities)
        .any(|c| c.kind == ChangeKind::Changed
            && c.after.as_ref().unwrap().block.as_deref() == Some("symbol")));
}

#[test]
fn drawing_unit_tolerance_does_not_hide_colour_or_angle_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.dxf");
    let mut a = Session::new_blank(&path, 4);
    let h = line(&mut a, [0., 0.], [100., 0.]);
    run(
        &mut a,
        json!({"type":"set_property","target":{"handle":h},"prop":"color","value":"ACI 1"}),
    );
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    run(
        &mut b,
        json!({"type":"set_property","target":{"handle":h},"prop":"color","value":"ACI 2"}),
    );
    let cmp = b
        .compare_revisions(path.to_str().unwrap(), Some(10.))
        .unwrap();
    assert_eq!(cmp.sheets[0].entities[0].kind, ChangeKind::Changed);
    let left = a.revision_snapshot();
    let mut right = left.clone();
    right[0].data = json!({"rotation":0.0,"point":{"x":0.0,"y":0.0}});
    let mut left = right.clone();
    left[0].data["rotation"] = json!(0.01);
    assert_eq!(
        qiyas_core::revisions::compare(&left, &right, 10.).sheets[0].entities[0].kind,
        ChangeKind::Changed
    );
}

#[test]
fn moved_elements_have_local_clouds_with_revision_triangles_and_replace_together() {
    use opencadcodec::{EntityType, Handle};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("revision-a.dxf");
    let mut a = Session::new_blank(&path, 6);
    run(&mut a, json!({"type":"annotation_scale","name":"1:50"}));
    let moved = line(&mut a, [0.0, 0.0], [2.0, 0.0]);
    let unchanged = line(&mut a, [40.0, 0.0], [42.0, 0.0]);
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    run(&mut b, json!({"type":"carry_knowledge","from_path":path}));
    run(
        &mut b,
        json!({"type":"modify","kind":"move","targets":[{"handle":moved}],"delta":[80,0]}),
    );
    let before = b.history_steps().0.len();
    run(
        &mut b,
        json!({"type":"variation_report","layer":"REV-C","revision":"C03"}),
    );
    assert_eq!(b.history_steps().0.len(), before + 1);
    let clouds = b.list_comments();
    assert_eq!(clouds.len(), 2);
    for cloud in &clouds {
        assert_eq!(cloud.text, "C03");
        assert!(cloud.bounds[1][0] - cloud.bounds[0][0] < 2.3);
        assert!(cloud.bounds[1][0] < 40.0 || cloud.bounds[0][0] > 42.0);
        let EntityType::MText(label) = b
            .drawing
            .doc
            .get_entity(Handle::new(cloud.label.unwrap()))
            .unwrap()
        else {
            panic!("label")
        };
        assert!((label.height - 0.125).abs() < 1e-8);
    }
    let revision_parts = |s: &Session| {
        s.drawing
            .doc
            .entities_in_block("*Model_Space")
            .filter(|e| e.common().layer == "REV-C")
            .map(|e| e.common().handle.value())
            .collect::<Vec<_>>()
    };
    let old = revision_parts(&b);
    assert_eq!(old.len(), 6);
    assert_eq!(b.drawing.doc.entities_in_block("*Model_Space").filter(|e| {
        matches!(e, EntityType::LwPolyline(p) if p.common.layer == "REV-C" && p.vertices.len() == 3)
    }).count(), 2);
    assert!(b.drawing.doc.get_entity(Handle::new(unchanged)).is_some());
    run(&mut b, json!({"type":"variation_report"}));
    assert_eq!(revision_parts(&b).len(), 6);
    assert!(old
        .iter()
        .all(|h| b.drawing.doc.get_entity(Handle::new(*h)).is_none()));
    assert!(b.list_comments().iter().all(|c| c.text == "C03"));
    assert!(b.undo());
    assert_eq!(revision_parts(&b), old);
}

#[test]
fn overlapping_cloud_footprints_keep_concave_boundaries_and_unchanged_islands() {
    let rectangle = |x0, y0, x1, y1| vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let shapes = vec![
        rectangle(0.0, 0.0, 4.0, 1.0),
        rectangle(0.0, 0.0, 1.0, 4.0),
        rectangle(20.0, 0.0, 21.0, 1.0),
    ];
    let regions = qiyas_geom::walls::union_footprints(&shapes).unwrap();
    assert_eq!(regions.len(), 2);
    assert!((regions.iter().map(|r| r.area()).sum::<f64>() - 8.0).abs() < 1e-8);
    let ring = vec![
        rectangle(0.0, 0.0, 4.0, 1.0),
        rectangle(0.0, 3.0, 4.0, 4.0),
        rectangle(0.0, 0.0, 1.0, 4.0),
        rectangle(3.0, 0.0, 4.0, 4.0),
    ];
    let regions = qiyas_geom::walls::union_footprints(&ring).unwrap();
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].holes.len(), 1);
    assert!((regions[0].area() - 12.0).abs() < 1e-8);
}

#[test]
fn a_ring_of_changes_excludes_its_unchanged_centre_without_an_extra_triangle() {
    use opencadcodec::EntityType;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty-a.dxf");
    let mut a = Session::new_blank(&path, 6);
    run(&mut a, json!({"type":"annotation_scale","name":"1:50"}));
    a.save_as(&path).unwrap();
    let mut b = Session::open_standalone(&path).unwrap();
    run(&mut b, json!({"type":"carry_knowledge","from_path":path}));
    for (a, c) in [
        ([0.0, 0.0], [4.0, 0.0]),
        ([4.0, 0.0], [4.0, 4.0]),
        ([4.0, 4.0], [0.0, 4.0]),
        ([0.0, 4.0], [0.0, 0.0]),
    ] {
        line(&mut b, a, c);
    }
    run(
        &mut b,
        json!({"type":"variation_report","layer":"REV-RING","revision":"R1"}),
    );
    assert_eq!(b.list_comments().len(), 1);
    let parts: Vec<_> = b
        .drawing
        .doc
        .entities_in_block("*Model_Space")
        .filter(|e| e.common().layer == "REV-RING")
        .collect();
    assert_eq!(parts.len(), 4);
    assert_eq!(
        parts
            .iter()
            .filter(|e| matches!(e, EntityType::LwPolyline(p) if p.vertices.len() == 3))
            .count(),
        1
    );
    assert!(parts.iter().any(
        |e| matches!(e, EntityType::LwPolyline(p) if p.vertices.iter().all(|v| v.bulge < 0.0))
    ));
    run(&mut b, json!({"type":"variation_report"}));
    assert_eq!(
        b.drawing
            .doc
            .entities_in_block("*Model_Space")
            .filter(|e| e.common().layer == "REV-RING")
            .count(),
        4
    );
}
