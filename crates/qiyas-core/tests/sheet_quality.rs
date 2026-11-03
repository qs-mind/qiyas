//! Small fixtures rewritten from the office run's model window and annotation inputs.
use opencadcodec::{EntityType, Handle};
use qiyas_core::{check::CheckScope, Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}
fn add(s: &mut Session, command: Value) -> u64 {
    let Outcome::Added { handles } = run(s, command) else {
        panic!("added")
    };
    handles[0]
}
fn layout(s: &mut Session) {
    run(
        s,
        json!({"type":"new_layout","name":"A1 Review","paper":"ISO A1","orientation":"landscape"}),
    );
    let sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| sh.name == "A1 Review")
        .unwrap();
    s.set_sheet(&sheet.block).unwrap();
}

#[test]
fn office_window_fills_a1_at_the_largest_standard_scale_in_each_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("office-window.dwg"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        let bounds = [[0.0, 0.0], [12.0 * u, 9.0 * u]];
        run(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":bounds[0],"b":bounds[1]}),
        );
        let Outcome::Sheet { report } = run(
            &mut s,
            json!({"type":"make_sheet","name":"A-101","paper":"ISO A1","orientation":"landscape","model_bounds":bounds}),
        ) else {
            panic!("sheet")
        };
        assert_eq!(report.scale, "1:25");
        let sheet = s
            .drawing
            .sheets()
            .into_iter()
            .find(|sh| sh.name == "A-101")
            .unwrap();
        let view = &s.paper_views_of(&sheet.block)[0];
        let size = [
            view.rect[1][0] - view.rect[0][0],
            view.rect[1][1] - view.rect[0][1],
        ];
        assert!((size[0] - 480.0).abs() < 1e-7 && (size[1] - 360.0).abs() < 1e-7);
        let zone = report.zones.plans;
        let usable = (zone[1][0] - zone[0][0] - 10.0) * (zone[1][1] - zone[0][1] - 10.0);
        assert!(size[0] * size[1] / usable > 0.4);
        // The next larger standard scale cannot fit the available height.
        assert!(9000.0 / 20.0 > zone[1][1] - zone[0][1] - 10.0);

        layout(&mut s);
        let result = s
            .run_input(&json!({"type":"mview","model_bounds":bounds}), Source::User)
            .unwrap();
        assert!(result
            .defaults_used
            .iter()
            .any(|d| d.field == "/scale" || d.field == "scale"));
        let view = &s.paper_views()[0];
        assert!((view.scale * u - 40.0).abs() < 1e-7);
        assert!((view.rect[1][0] - view.rect[0][0] - 480.0).abs() < 1e-7);
    }
}

#[test]
fn explicit_office_scale_is_retained_with_bilingual_underfill_advice() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("scale.dwg"), 6);
    layout(&mut s);
    let result = s
        .run_input(
            &json!({"type":"mview","model_bounds":[[0,0],[12,9]],"scale":"1:100"}),
            Source::User,
        )
        .unwrap();
    assert_eq!(s.paper_views()[0].scale, 10.0);
    let advice = result
        .advice
        .iter()
        .find(|a| a["code"] == "viewport_underfilled")
        .unwrap();
    assert_eq!(advice["suggested_scale"], "1:25");
    assert!(advice["fill_fraction"].as_f64().unwrap() < 0.4);
    assert!(advice["message_ar"].as_str().unwrap().contains("1:25"));
    let result = s.run_input(&json!({"type":"make_sheet","name":"Explicit","paper":"ISO A1","orientation":"landscape","model_bounds":[[0,0],[12,9]],"scale":"1:100"}), Source::User).unwrap();
    assert!(result.advice.iter().any(|a| a["suggested_scale"] == "1:25"));
}

#[test]
fn arrange_refits_an_automatic_viewport_and_preserves_an_explicit_scale() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("arrange-scale.dwg"), 6);
    run(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[12,9]}),
    );
    layout(&mut s);
    let h = add(
        &mut s,
        json!({"type":"mview","corner1":[100,200],"corner2":[240,310]}),
    );
    let before = s.paper_views()[0].scale;
    run(&mut s, json!({"type":"arrange"}));
    assert_eq!(s.paper_views()[0].handle, h);
    assert_eq!(s.paper_views()[0].scale, 40.0);
    assert!(before < 40.0);
    assert!(s.undo());
    assert_eq!(s.paper_views()[0].scale, before);
    run(
        &mut s,
        json!({"type":"mview","target":{"handle":h},"model_bounds":[[0,0],[12,9]],"scale":"1:100"}),
    );
    run(&mut s, json!({"type":"arrange"}));
    assert_eq!(s.paper_views()[0].scale, 10.0);
}

#[test]
fn office_notes_are_checked_in_both_rule_names_and_separated_with_leaders_in_one_undo() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("notes.dwg"), 6);
    // The failed batch's mtext form, reduced to two colliding note blocks.
    let a = add(
        &mut s,
        json!({"type":"mtext","at":[36,3.7],"height":0.16,"width":9.8,"value":"MATERIAL PALETTE\nF1: warm oak-tone resilient planks."}),
    );
    let b = add(
        &mut s,
        json!({"type":"mtext","at":[36,3.7],"height":0.16,"width":9,"value":"OPENING NOTES\nDimensions in metres."}),
    );
    let leader = add(
        &mut s,
        json!({"type":"leader","points":[[35,2],[36,3.7]],"text":""}),
    );
    if let Some(EntityType::Leader(l)) = s.drawing.doc.get_entity_mut(Handle::new(leader)) {
        l.annotation_handle = Handle::new(a);
    } else {
        panic!("leader")
    }
    for rule in ["text_over_text", "text_overlaps_text"] {
        let check = s.check_drawing(&CheckScope {
            rules: vec![rule.into()],
            ..Default::default()
        });
        assert!(check
            .findings
            .iter()
            .any(|f| f.rule == rule && f.handles.contains(&a) && f.handles.contains(&b)));
    }
    let old_a = s.drawing.doc.get_entity(Handle::new(a)).unwrap().clone();
    let old_leader = s
        .drawing
        .doc
        .get_entity(Handle::new(leader))
        .unwrap()
        .clone();
    let steps = s.history_steps().0.len();
    // One item preserves the pair's initial overlap so the annotation repair must resolve it.
    run(
        &mut s,
        json!({"type":"arrange","items":[{"handles":[a,b]}],"gap_mm":0}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    assert!(s
        .check_drawing(&CheckScope {
            rules: vec!["text_over_text".into()],
            ..Default::default()
        })
        .findings
        .is_empty());
    let EntityType::Leader(old) = &old_leader else {
        panic!("leader")
    };
    let Some(EntityType::Leader(new)) = s.drawing.doc.get_entity(Handle::new(leader)) else {
        panic!("leader")
    };
    assert_eq!(new.vertices[0], old.vertices[0]);
    assert_ne!(new.vertices.last(), old.vertices.last());
    assert_eq!(new.annotation_handle, Handle::new(a));
    assert!(s.undo());
    assert_eq!(s.drawing.doc.get_entity(Handle::new(a)), Some(&old_a));
    assert_eq!(
        s.drawing.doc.get_entity(Handle::new(leader)),
        Some(&old_leader)
    );
}

#[test]
fn wall_area_bases_are_distinct_in_measurement_and_bilingual_exports() {
    use qiyas_core::deliverables::{self, Kind, Language};
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("tower-wall-junction.dwg"), 6);
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[12,0]]},{"points":[[6,0],[6,9]]}],"thickness":0.25,"height":3,
        "openings":[{"run":0,"at":2,"width":0.9,"kind":"door","sill":0,"head":2.1}]}),
    );
    let cite = json!({"source":"Test drawing","clause":"Wall geometry"});
    let rule = |id: &str, factors: Value, deductions: Value| {
        json!({"id":id,"kind":"area","basis":"geometric","element_type":"Legacy wall","unit":"m2",
        "terms":[{"label":"Wall area","operation":"add","factors":factors,"citation":cite}],"deductions":deductions,"citation":cite})
    };
    let openings = json!([{"element_type":"Legacy door","when":"true","term":{"label":"Opening","operation":"deduct","factors":["void.width","void.host_width"],"citation":cite}}]);
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[
        {"rule":"openings","code":"W-O","description_en":"Wall area","description_ar":"مساحة الحوائط","section":"Walls","trade":"wall","unit":"m2","citation":cite},
        {"rule":"junctions","code":"W-J","description_en":"Wall area","description_ar":"مساحة الحوائط","section":"Walls","trade":"wall","unit":"m2","citation":cite}]}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{"version":1,"name":"Test areas","title":"Test areas","based_on":"drawing","source_note":"Geometry only",
        "rules":[rule("openings",json!(["geometry.length","width"]),openings),rule("junctions",json!(["net_plan_area"]),json!([]))]}}),
    );
    let report = s.measurement_report().unwrap();
    for q in &report.quantities {
        let basis = q.wall_area_basis.as_ref().unwrap();
        assert_eq!(
            basis.label_en,
            if q.rule == "openings" {
                "Net of openings"
            } else {
                "Net of openings and wall junctions"
            }
        );
    }
    let first = report.items.iter().find(|i| i.code == "W-O").unwrap();
    let joined = report.items.iter().find(|i| i.code == "W-J").unwrap();
    assert!(first.quantity > joined.quantity);
    let glossary: Value =
        serde_json::from_str(include_str!("../../../src/i18n/ar-glossary.json")).unwrap();
    for language in [Language::En, Language::Ar] {
        let table = deliverables::drawing(&s, Kind::Boq, language, None)
            .unwrap()
            .table()
            .unwrap();
        let text = serde_json::to_string(&table).unwrap();
        for key in ["net_of_openings", "net_of_openings_and_wall_junctions"] {
            let field = if language == Language::Ar {
                "preferred"
            } else {
                "en"
            };
            assert!(text.contains(glossary["concepts"][key][field].as_str().unwrap()));
        }
        let table = deliverables::drawing(&s, Kind::Dimensions, language, None)
            .unwrap()
            .table()
            .unwrap();
        let text = serde_json::to_string(&table).unwrap();
        let field = if language == Language::Ar {
            "preferred"
        } else {
            "en"
        };
        assert!(text.contains(
            glossary["concepts"]["net_of_openings_and_wall_junctions"][field]
                .as_str()
                .unwrap()
        ));
    }
}

#[test]
fn make_sheet_repairs_only_notes_inside_its_window_and_undo_restores_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("note-window.dwg"), 6);
    let mut notes = Vec::new();
    for x in [5.0, 36.0] {
        for value in [
            "MATERIAL PALETTE\nF1: resilient planks.",
            "OPENING NOTES\nDimensions in metres.",
        ] {
            notes.push(add(
                &mut s,
                json!({"type":"mtext","at":[x,3.7],"height":0.16,"width":3,"value":value}),
            ));
        }
    }
    let before: Vec<_> = notes
        .iter()
        .map(|h| s.drawing.doc.get_entity(Handle::new(*h)).unwrap().clone())
        .collect();
    let steps = s.history_steps().0.len();
    run(
        &mut s,
        json!({"type":"make_sheet","name":"A-101","paper":"ISO A1","model_bounds":[[0,0],[12,9]]}),
    );
    assert_eq!(s.history_steps().0.len(), steps + 1);
    let check = s.check_drawing(&CheckScope {
        sheet: Some("Model".into()),
        rules: vec!["text_over_text".into()],
        ..Default::default()
    });
    assert!(!check.findings.iter().any(|f| f.handles.contains(&notes[0])));
    assert!(check
        .findings
        .iter()
        .any(|f| f.handles.contains(&notes[2]) && f.handles.contains(&notes[3])));
    assert_eq!(
        s.drawing.doc.get_entity(Handle::new(notes[2])),
        Some(&before[2])
    );
    assert!(s.undo());
    for (h, old) in notes.iter().zip(&before) {
        assert_eq!(s.drawing.doc.get_entity(Handle::new(*h)), Some(old));
    }
}
