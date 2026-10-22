use std::collections::BTreeMap;

use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::json;

use super::model::{Finding, Item, Rule, Submission, Truth};
use super::*;

fn item(id: &str, kind: &str, quantity: f64, tolerance: &str) -> Item {
    Item {
        id: id.into(),
        description: format!("Item {id}"),
        kind: kind.into(),
        unit: match kind {
            "count" => "pcs",
            "length" => "m",
            "volume" => "m3",
            _ => "m2",
        }
        .into(),
        quantity,
        basis: "geometric".into(),
        tolerance_class: tolerance.into(),
        source_objects: vec!["10".into()],
        ..Default::default()
    }
}
fn truth(items: Vec<Item>) -> Truth {
    Truth {
        schema_version: 1,
        id: "test".into(),
        project_type: "test".into(),
        drawing: "drawing.dxf".into(),
        items,
        rules: BTreeMap::new(),
        seeded_errors: Vec::new(),
    }
}

#[test]
fn strict_bands_units_missing_and_duplicate_submissions_are_not_hidden() {
    let mut rule = item("rule", "area", 100.0, "rule");
    rule.tolerance_percent = Some(5.0);
    let truth = truth(vec![
        item("count", "count", 4.0, "count"),
        item("area", "area", 100.0, "geometry"),
        rule,
        item("judgement", "volume", 100.0, "judgement"),
        item("missing", "length", 10.0, "geometry"),
    ]);
    let mut proposed = truth.items[..4].to_vec();
    proposed[0].quantity = 5.0;
    proposed[1].quantity = 100.5;
    proposed[2].quantity = 105.0;
    proposed[3].quantity = 110.0;
    let report = score::score(
        &truth,
        &Submission {
            items: proposed.clone(),
            ..Default::default()
        },
        None,
    );
    assert_eq!((report.metrics.matched, report.metrics.passed), (4, 3));
    assert_eq!(report.metrics.within_25_percent, 4);
    assert_eq!(report.metrics.traceable, 0);
    proposed[1].unit = "m".into();
    let report = score::score(
        &truth,
        &Submission {
            items: proposed,
            ..Default::default()
        },
        None,
    );
    assert!(!report.items[1].passed);
    let mut duplicate = truth.items.clone();
    duplicate.push(duplicate[0].clone());
    let report = score::score(
        &truth,
        &Submission {
            items: duplicate,
            ..Default::default()
        },
        None,
    );
    assert!(report.items[0].submitted.is_none());
    assert_eq!(report.metrics.extras, 2);
}

#[test]
fn zero_truth_does_not_get_a_free_relative_error_pass() {
    let truth = truth(vec![item("zero", "area", 0.0, "geometry")]);
    let mut items = truth.items.clone();
    items[0].quantity = 1.0;
    let report = score::score(
        &truth,
        &Submission {
            items,
            ..Default::default()
        },
        None,
    );
    assert!(!report.items[0].passed);
    assert_eq!(report.metrics.within_25_percent, 0);
}

#[test]
fn matching_uses_handles_or_unique_description_and_position_never_quantity() {
    let mut gold = item("area", "area", 20.0, "geometry");
    gold.position = Some([1.0, 2.0]);
    let mut submission = gold.clone();
    submission.id = "random-uuid".into();
    submission.quantity = 999.0;
    let truth = truth(vec![gold.clone()]);
    let report = score::score(
        &truth,
        &Submission {
            items: vec![submission.clone()],
            ..Default::default()
        },
        None,
    );
    assert_eq!(report.items[0].matched_by.as_deref(), Some("handles"));
    submission.source_objects.clear();
    submission.position = None;
    let report = score::score(
        &truth,
        &Submission {
            items: vec![submission.clone()],
            ..Default::default()
        },
        None,
    );
    assert_eq!(
        report.items[0].matched_by.as_deref(),
        Some("description_position")
    );
    let mut other = gold;
    other.id = "another".into();
    other.position = Some([10.0, 20.0]);
    let mut duplicate_truth = truth;
    duplicate_truth.items.push(other);
    assert_eq!(
        score::score(
            &duplicate_truth,
            &Submission {
                items: vec![submission],
                ..Default::default()
            },
            None
        )
        .metrics
        .matched,
        0
    );
}

#[test]
fn citations_are_remeasured_and_rule_citation_is_required() {
    let mut session = Session::new_blank(Path::new("trace.dxf"), 6);
    let outcome = session
        .run_input(
            &json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,5]}),
            Source::User,
        )
        .unwrap()
        .outcome;
    let Outcome::Added { handles } = outcome else {
        panic!("expected geometry")
    };
    let mut area = item("area", "area", 20.0, "geometry");
    area.source_objects = vec![format!("{:X}", handles[0])];
    let mut volume = item("volume", "volume", 60.0, "rule");
    volume.source_objects = area.source_objects.clone();
    volume.basis = "rule:height".into();
    let mut truth = truth(vec![area, volume]);
    truth.rules.insert(
        "height".into(),
        Rule {
            operation: "area_times_height".into(),
            clause: "project note: 3 m".into(),
            height_m: Some(3.0),
            factor: None,
            deduction: None,
        },
    );
    truth.validate().unwrap();
    let mut proposal = Submission {
        items: truth.items.clone(),
        ..Default::default()
    };
    assert_eq!(
        score::score(&truth, &proposal, Some(&session))
            .metrics
            .traceable,
        2
    );
    proposal.items[1].basis = "geometric".into();
    assert_eq!(
        score::score(&truth, &proposal, Some(&session))
            .metrics
            .traceable,
        1
    );
    assert!(!score::score(&truth, &proposal, Some(&session)).items[1].passed);
    session
        .run(Command::DeleteEntities {
            targets: vec![qiyas_core::Target {
                handle: handles[0],
                chain: vec![],
            }],
        })
        .unwrap();
    assert_eq!(
        score::score(&truth, &proposal, Some(&session))
            .metrics
            .traceable,
        0
    );
}

#[test]
fn error_scoring_requires_correct_location_and_class_and_counts_duplicates() {
    let mut truth = truth(vec![item("a", "area", 10.0, "geometry")]);
    truth.seeded_errors = vec![Finding {
        item_id: "a".into(),
        kind: "wrong_unit".into(),
    }];
    let hit = truth.seeded_errors[0].clone();
    let wrong = Finding {
        item_id: "elsewhere".into(),
        kind: "wrong_unit".into(),
    };
    let submission = Submission {
        findings: vec![hit.clone(), hit, wrong],
        ..Default::default()
    };
    let report = score::score(&truth, &submission, None);
    assert_eq!(
        (
            report.metrics.true_positives,
            report.metrics.false_positives,
            report.metrics.false_negatives
        ),
        (1, 2, 0)
    );
    truth.seeded_errors.clear();
    assert_eq!(
        score::score(&truth, &submission, None)
            .metrics
            .clean_false_alarms,
        3
    );
}

#[test]
fn portable_native_takeoff_and_boq_json_are_supported() {
    let native = json!({"items":[{"id":"uuid","boq_code":"A1","kind":"area","shown":12.5,"value":12500000,"unit":"m2","handles":[255],"description":"Floor"}]});
    let proposal = submission::from_json(native).unwrap();
    assert_eq!(proposal.items[0].id, "A1");
    assert_eq!(proposal.items[0].quantity, 12.5);
    assert_eq!(proposal.items[0].source_objects, ["FF"]);
    let boq = submission::from_json(json!({"items":[{"code":"A1","unit":"m2","quantity":12.5}]}))
        .unwrap();
    assert_eq!(boq.items[0].kind, "area");
}

#[test]
fn qiyas_excel_reads_only_items_not_the_totals_below_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("a.dxf"), 6);
    session
        .run_input(
            &json!({"type":"measure_area","points":[[0,0],[4,0],[4,5],[0,5]]}),
            Source::User,
        )
        .unwrap();
    let path = dir.path().join("takeoff.xlsx");
    qiyas_boq::export_takeoff(
        &path,
        &session.takeoff.items,
        &session.takeoff.totals(),
        "test",
    )
    .unwrap();
    let parsed = submission::read(&path).unwrap();
    assert_eq!(parsed.items.len(), 1);
    assert_eq!(parsed.items[0].quantity, 20.0);
    assert!(parsed.items[0].source_objects.is_empty());
}

#[test]
fn holdout_is_unread_even_when_poisoned_and_cannot_be_selected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("holdout")).unwrap();
    std::fs::write(dir.path().join("holdout/DO-NOT-READ"), b"not JSON or CAD").unwrap();
    assert!(load_case(dir.path(), "../holdout").is_err());
    assert!(load_case(dir.path(), "holdout").is_err());
    assert!(load_case(dir.path(), "/holdout").is_err());
    assert!(load_case(&dir.path().join("holdout"), "case").is_err());
    assert_eq!(seal(dir.path()).unwrap()["status"], "not generated");
    #[cfg(unix)]
    {
        std::fs::create_dir(dir.path().join("cases")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("holdout"), dir.path().join("cases/alias"))
            .unwrap();
        assert!(load_case(dir.path(), "alias")
            .unwrap_err()
            .contains("symlink"));
        assert!(dev_ids(dir.path(), false).is_err());
    }
}

#[test]
fn rejects_unsupported_schema_and_unbounded_rule_tolerances() {
    let mut truth = truth(vec![item("a", "area", 1.0, "rule")]);
    truth.schema_version = 99;
    assert!(truth.validate().is_err());
    truth.schema_version = 1;
    truth.items[0].tolerance_percent = Some(25.0);
    assert!(truth.validate().is_err());
}

#[test]
fn recognition_recipes_compute_quantities_without_a_truth_file() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = dir.path().join("drawing.dxf");
    let mut session = Session::new_blank(&drawing, 4);
    let Outcome::Added { handles } = session
        .run(
            serde_json::from_value(json!({"type":"draw","kind":"line","a":[0,0],"b":[3000,4000]}))
                .unwrap(),
        )
        .unwrap()
    else {
        panic!("line")
    };
    session.save_as(&drawing).unwrap();
    let plan = dir.path().join("recognition.json");
    write_json(&plan,&json!({"commands":[
        {"type":"define_object_type","definition":{"name":"Route","category":"example","geometry":"path","quantities":{"measured":"length"}}},
        {"type":"recognition","kind":"define","rule":{"name":"route","object_type":"Route","match":{"entity_type":"Line"},"confidence":0.99}},
        {"type":"recognition","kind":"apply"}
    ],"measurements":[{"object_type":"Route","quantity":"measured","kind":"length","unit":"m"}]})).unwrap();
    let submission = recognition::measure(&drawing, &plan).unwrap();
    assert_eq!(submission.items.len(), 1);
    assert_eq!(submission.items[0].quantity, 5.0);
    assert_eq!(
        submission.items[0].source_objects,
        [format!("{:X}", handles[0])]
    );
    assert!(!dir.path().join("truth.json").exists());
    assert!(Session::open_standalone(&drawing)
        .unwrap()
        .object_types()
        .unwrap()
        .is_empty());
}

#[test]
fn pdf_recognition_produces_scored_submission_with_page_evidence() {
    if !qiyas_pdf::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let drawing = qiyas_pdf::fixtures::write_wall(dir.path());
    let plan = dir.path().join("recognition.json");
    write_json(&plan,&json!({"commands":[
        {"type":"recognition","kind":"pdf","path":drawing.file_name().unwrap().to_string_lossy(),"page":1,"calibration":{"a":[0,0],"b":[100,0],"distance_m":1}},
        {"type":"define_object_type","definition":{"name":"Outline","category":"example","geometry":"region","quantities":{"measured_area":"area"}}},
        {"type":"recognition","kind":"define","rule":{"name":"outline","object_type":"Outline","match":{"closed_region":true,"layer":"PDF-FF0000"},"confidence":0.99}},
        {"type":"recognition","kind":"apply"}
    ],"measurements":[{"object_type":"Outline","quantity":"measured_area","kind":"area","unit":"m2","id":"area-1"}]})).unwrap();
    let submission = recognition::measure(&drawing, &plan).unwrap();
    assert_eq!(submission.items.len(), 1);
    assert!((submission.items[0].quantity - 0.5).abs() < 1e-6);
    assert!(submission.items[0].source_objects[0].contains("#page=1"));
    let report = score::score(
        &truth(vec![item("area-1", "area", 0.5, "geometry")]),
        &submission,
        None,
    );
    assert_eq!(report.metrics.passed, 1);
    assert_eq!(
        report.metrics.traceable, 0,
        "PDF evidence does not pretend to be the source CAD handle"
    );
    // The default benchmark root is relative. The shared surface must not resolve it twice.
    let cwd = std::env::current_dir().unwrap();
    if let Some(common) = cwd.ancestors().find(|base| drawing.starts_with(base)) {
        let mut relative = PathBuf::new();
        for _ in cwd.strip_prefix(common).unwrap().components() {
            relative.push("..");
        }
        relative.push(drawing.strip_prefix(common).unwrap());
        let from_relative = recognition::measure(&relative, &plan).unwrap();
        assert_eq!(from_relative.items.len(), 1);
        assert_eq!(
            from_relative.items[0].quantity,
            submission.items[0].quantity
        );
        assert_eq!(
            from_relative.items[0].source_objects,
            submission.items[0].source_objects
        );
    }
}

#[test]
fn item_precision_and_f1_penalize_extra_submissions_per_case_and_overall() {
    let truth = truth(vec![
        item("a", "area", 10.0, "geometry"),
        item("b", "length", 5.0, "geometry"),
    ]);
    let mut extra = item("extra", "count", 1.0, "count");
    extra.source_objects = vec!["FFF".into()];
    let report = score::score(
        &truth,
        &Submission {
            items: vec![truth.items[0].clone(), extra],
            ..Default::default()
        },
        None,
    );
    assert_eq!(report.metrics.extras, 1);
    assert_eq!(report.rates["coverage"], json!(0.5));
    assert_eq!(report.rates["item_precision"], json!(0.5));
    assert_eq!(report.rates["item_f1"], json!(0.5));
    let empty = score::score(&truth, &Submission::default(), None);
    assert!(empty.rates["item_precision"].is_null());
    assert_eq!(empty.rates["item_f1"], json!(0.0));
    let mut overall = report.metrics.clone();
    overall.merge(&empty.metrics);
    assert_eq!(overall.rates()["coverage"], json!(0.25));
    assert_eq!(overall.rates()["item_precision"], json!(0.5));
    assert_eq!(overall.rates()["item_f1"], json!(1.0 / 3.0));
}

#[test]
fn autonomy_never_opens_case_recipe_and_shared_rules_reject_case_handles() {
    let dir = tempfile::tempdir().unwrap();
    let drawing = dir.path().join("drawing.dxf");
    let mut s = Session::new_blank(&drawing, 6);
    s.run_input(
        &json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[3,4]}),
        Source::User,
    )
    .unwrap();
    s.save_as(&drawing).unwrap();
    // Deliberately unreadable: accessing a case recipe would fail this run.
    std::fs::write(dir.path().join("recognition.json"), "not JSON").unwrap();
    let baseline = general::measure(dir.path(), &drawing).unwrap();
    assert!(
        baseline.items.is_empty(),
        "Unmapped geometry stays a candidate"
    );
    std::fs::create_dir(dir.path().join("general")).unwrap();
    write_json(&dir.path().join("general/recognition.json"),&json!({"commands":[
        {"type":"define_object_type","definition":{"name":"Surface","category":"evidence","geometry":"region","quantities":{"measured":"area"}}},
        {"type":"recognition","kind":"define","rule":{"name":"surfaces","object_type":"Surface","match":{"entity_type":"LwPolyline","closed_region":true},"confidence":0.99}},
        {"type":"recognition","kind":"apply"}
    ],"measurements":[{"object_type":"Surface","quantity":"measured","kind":"area","unit":"m2"}]})).unwrap();
    let mapped = general::measure(dir.path(), &drawing).unwrap();
    assert!(mapped
        .items
        .iter()
        .any(|i| (i.quantity - 12.0).abs() < 1e-9));
    write_json(&dir.path().join("general/recognition.json"),&json!({"commands":[{"type":"recognition","kind":"define","rule":{"name":"case","match":{"handles":[123]}}}],"measurements":[]})).unwrap();
    assert!(general::measure(dir.path(), &drawing)
        .unwrap_err()
        .contains("case-specific handles"));
}

#[test]
fn general_pdf_measures_multiple_labels_using_its_own_scale_text_without_case_recipe() {
    if !qiyas_pdf::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("source.dxf"), 6);
    for command in [
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[3,4]}),
        json!({"type":"draw","kind":"rectangle","a":[5,0],"b":[7,4]}),
        json!({"type":"text","at":[0.1,0.1],"height":0.15,"value":"1 Alpha"}),
        json!({"type":"text","at":[5.1,0.1],"height":0.15,"value":"2 Beta"}),
        json!({"type":"draw","kind":"line","a":[0,-2],"b":[2,-2]}),
        json!({"type":"draw","kind":"line","a":[0,-2.1],"b":[0,-1.9]}),
        json!({"type":"draw","kind":"line","a":[2,-2.1],"b":[2,-1.9]}),
        json!({"type":"text","at":[0,-2.4],"height":0.15,"value":"Reference 2 m"}),
    ] {
        s.run_input(&command, Source::User).unwrap();
    }
    let drawing = dir.path().join("drawing.pdf");
    qiyas_plot::plot_pdf_to(
        &s,
        &qiyas_plot::PlotJob {
            sheet: s.sheet.clone(),
            area: qiyas_plot::Area::Extents,
            paper: qiyas_plot::Paper {
                name: "Test".into(),
                w_mm: 297.0,
                h_mm: 210.0,
            },
            orientation: qiyas_plot::Orientation::Landscape,
            scale: qiyas_plot::Scale::Fit,
            centre: true,
            style: None,
            lineweights: false,
            output: None,
        },
        &drawing,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("general")).unwrap();
    write_json(&dir.path().join("general/pdf.json"),&json!({"calibration":{"label_regex":"Reference (?P<distance>[0-9.]+) m","metres_per_label_unit":1,"search_heights":8},"area_label_regex":"^[0-9]+\\s+(?P<description>.+)$"})).unwrap();
    let result = general::measure(dir.path(), &drawing).unwrap();
    assert_eq!(result.items.len(), 2, "{:?}", result);
    for (name, area) in [("Alpha", 12.0), ("Beta", 8.0)] {
        let measured = result.items.iter().find(|i| i.description == name).unwrap();
        assert!(
            (measured.quantity - area).abs() / area < 0.005,
            "{measured:?}"
        );
        assert!(measured.source_objects[0].contains("#page=1;label="));
        assert_eq!(measured.confidence, Some(qiyas_core::trust::unscored()));
    }
    assert!(!dir.path().join("recognition.json").exists());
    assert!(!dir.path().join("truth.json").exists());
}

#[test]
fn quick_and_live_sets_are_frozen_deterministic_and_keep_holdout_sealed() {
    let dir = tempfile::tempdir().unwrap();
    for family in [
        "apartment",
        "office",
        "warehouse",
        "mep",
        "external",
        "part",
    ] {
        for i in 0..24 {
            let id = format!("{family}-{i:03}");
            let path = dir.path().join("cases").join(&id);
            std::fs::create_dir_all(&path).unwrap();
            let drawing = if i == 23 {
                "drawing.pdf"
            } else {
                "drawing.dxf"
            };
            std::fs::write(path.join(drawing), []).unwrap();
            write_json(&path.join("truth.json"),&json!({"schema_version":1,"id":id,"project_type":family,"drawing":drawing,"items":[{"id":"shape","description":"Shape","kind":"area","unit":"m2","quantity":1,"source_objects":["1"],"tolerance_class":"geometry"}]})).unwrap();
        }
    }
    let quick = sets::select(dir.path(), "quick").unwrap();
    assert_eq!(quick.len(), 120);
    assert_eq!(quick, sets::select(dir.path(), "quick").unwrap());
    for family in [
        "apartment",
        "office",
        "warehouse",
        "mep",
        "external",
        "part",
    ] {
        assert!(quick.iter().any(|id| id.starts_with(family)));
    }
    let live = sets::select(dir.path(), "live-dev").unwrap();
    assert_eq!(live.len(), 6);
    assert_eq!(live, sets::select(dir.path(), "live-dev").unwrap());
    assert_eq!(live[0], "apartment-000");
    assert_eq!(live[5], "apartment-023");
    assert!(!dir.path().join("holdout").exists());
}

#[test]
fn manual_geometry_without_recognition_mapping_is_never_submitted() {
    let mut session = Session::new_blank(Path::new("unused.dxf"), 6);
    session
        .run_input(
            &json!({"type":"measure_area","points":[[0,0],[3,0],[3,2],[0,2]]}),
            qiyas_core::Source::Ai,
        )
        .unwrap();
    assert_eq!(session.takeoff.items.len(), 1);
    assert!(submission::from_session(&session).unwrap().items.is_empty());
}

#[test]
fn dimension_submission_preserves_rules_handles_and_totals_without_truth() {
    let mut s = Session::new_blank(Path::new("unused.dxf"), 6);
    let citation = json!({"source":"drawing","clause":"note: sample-factor"});
    s.run_input(&json!({"type":"batch","commands":[
        {"type":"draw","kind":"rectangle","a":[0,0],"b":[3,2]},
        {"type":"draw","kind":"rectangle","a":[5,0],"b":[7,2]},
        {"type":"define_object_type","definition":{"name":"Observed","geometry":"region","category":"test"}},
        {"type":"recognition","kind":"define","rule":{"name":"observed","object_type":"Observed","match":{"entity_type":"LwPolyline","closed_region":true},"confidence":0.99}},
        {"type":"recognition","kind":"apply"},
        {"type":"measurement","kind":"set_method","scope":"project","method":{"name":"test","title":"Test","based_on":"drawing","source_note":"Visible dimensions","rules":[
            {"id":"A","element_type":"Observed","unit":"m2","terms":[{"label":"Area","operation":"add","factors":["geometry.area"],"citation":citation}],"citation":citation},
            {"id":"V","element_type":"Observed","unit":"m3","terms":[{"label":"Volume","operation":"add","factors":["geometry.area","0.25"],"citation":citation}],"citation":citation},
            {"id":"R","element_type":"Observed","kind":"area","basis":"rule:sample-factor","unit":"m2","terms":[{"label":"Derived","operation":"add","factors":["geometry.length","2"],"citation":citation}],"citation":citation}
        ]}},
        {"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"A","code":"TOTAL","description_en":"Area","description_ar":"Area","section":"Test","trade":"","unit":"m2","citation":citation}
        ]}
    ]}), Source::Ai).unwrap();
    let submitted = submission::from_session(&s).unwrap();
    let measured = s.measurement_report().unwrap();
    assert_eq!(submitted.items.len(), 7);
    for q in &measured.quantities {
        let item = submitted
            .items
            .iter()
            .find(|i| {
                i.rule.as_deref() == Some(&q.rule)
                    && i.source_objects == [format!("{:X}", q.object)]
            })
            .unwrap();
        assert_eq!(item.quantity, q.quantity);
        assert_eq!(item.dimension_lines, q.lines);
        assert_eq!(item.citation.as_ref(), Some(&q.citation));
        assert_eq!(
            item.basis,
            if q.rule == "R" {
                "rule:sample-factor"
            } else {
                "geometric"
            }
        );
        if q.rule == "V" {
            assert_eq!(item.height_m, Some(0.25));
        }
    }
    let total = submitted.items.iter().find(|i| i.id == "TOTAL").unwrap();
    assert_eq!(total.quantity, 10.0);
    assert_eq!(total.aggregate_of.len(), 2);
    let gold: Vec<_> = submitted
        .items
        .iter()
        .filter(|i| i.rule.as_deref() == Some("A"))
        .cloned()
        .collect();
    let intact = score::score(&truth(gold.clone()), &submitted, None);
    assert_eq!(intact.reported_aggregates, ["TOTAL"]);
    let mut broken = submitted.clone();
    broken
        .items
        .iter_mut()
        .find(|i| i.id == "TOTAL")
        .unwrap()
        .dimension_lines
        .pop();
    let broken = score::score(&truth(gold), &broken, None);
    assert!(broken.reported_aggregates.is_empty());
    assert_eq!(broken.metrics.extras, intact.metrics.extras + 1);
    let mut report = measured.clone();
    report.selection.project.as_mut().unwrap().rules[2].basis = None;
    let legacy = submission::from_report(&report, &s.recognition_evidence().unwrap()).unwrap();
    assert!(legacy
        .items
        .iter()
        .filter(|i| i.rule.as_deref() == Some("R"))
        .all(|i| i.basis == "rule:R"));
    assert!(!legacy.warnings.is_empty());
    let mut evidence = s.recognition_evidence().unwrap();
    evidence[0].state = qiyas_core::recognition::ReviewState::Rejected;
    let partial = submission::from_report(&report, &evidence).unwrap();
    assert!(!partial.items.iter().any(|i| i.id == "TOTAL"));
    assert_eq!(partial.items.len(), 3);
}

#[test]
fn aggregate_accounting_requires_complete_unique_components_and_never_reuses_matches() {
    let a = item("a", "area", 6.0, "geometry");
    let mut b = item("b", "area", 4.0, "geometry");
    b.source_objects = vec!["20".into()];
    let mut total = item("total", "area", 10.0, "geometry");
    total.source_objects = vec!["10".into(), "20".into()];
    total.aggregate_of = vec!["a".into(), "b".into()];
    let submission = Submission {
        items: vec![a.clone(), b.clone(), total.clone()],
        ..Default::default()
    };
    let per_object = score::score(&truth(vec![a.clone(), b.clone()]), &submission, None);
    assert_eq!(
        (per_object.metrics.matched, per_object.metrics.extras),
        (2, 0)
    );
    assert_eq!(per_object.reported_aggregates, ["total"]);
    let grouped = score::score(&truth(vec![total.clone()]), &submission, None);
    assert_eq!((grouped.metrics.matched, grouped.metrics.extras), (1, 0));
    for gold in [
        vec![a.clone(), total.clone()],
        vec![total.clone(), a.clone()],
    ] {
        assert_eq!(
            score::score(&truth(gold), &submission, None)
                .metrics
                .matched,
            1
        );
    }
    for mutation in 0..5 {
        let mut broken = submission.clone();
        match mutation {
            0 => {
                broken.items.remove(1);
            }
            1 => broken.items[2].quantity = 11.0,
            2 => broken.items[2].basis = "rule:other".into(),
            3 => broken.items[2].aggregate_of.push("a".into()),
            _ => broken.items.push(a.clone()),
        }
        let scored = score::score(&truth(vec![a.clone(), b.clone()]), &broken, None);
        assert!(scored.reported_aggregates.is_empty(), "mutation {mutation}");
        assert!(scored.metrics.extras > 0);
    }
    let mut wrong_basis = submission.clone();
    wrong_basis.items[0].id = "unrelated".into();
    wrong_basis.items[0].description = "Unrelated".into();
    wrong_basis.items[0].basis = "rule:different".into();
    assert_eq!(
        score::score(&truth(vec![a]), &wrong_basis, None)
            .metrics
            .matched,
        0
    );
}

#[test]
fn confidence_calibration_counts_wrong_and_extra_items_without_inventing_empty_rates() {
    let gold = truth(vec![
        item("A", "count", 2., "count"),
        item("B", "count", 3., "count"),
    ]);
    let mut predicted = gold.items.clone();
    predicted[0].confidence = Some(0.95);
    predicted[1].confidence = Some(0.6);
    predicted[1].quantity = 10.;
    let mut extra = item("extra", "length", 5., "geometry");
    extra.confidence = Some(0.95);
    predicted.push(extra);
    let scored = score::score(
        &gold,
        &Submission {
            items: predicted,
            ..Default::default()
        },
        None,
    );
    assert_eq!(scored.calibration["high"]["submitted"], 2);
    assert_eq!(scored.calibration["high"]["pass_rate"], 0.5);
    assert_eq!(scored.calibration["medium"]["pass_rate"], 0.);
    assert!(scored.calibration["low"]["pass_rate"].is_null());
    assert_eq!(scored.items[0].confidence, Some(0.95));
    let mut merged = score::Metrics::default();
    merged.merge(&scored.metrics);
    merged.merge(&scored.metrics);
    assert_eq!(merged.calibration_rates()["high"]["submitted"], 4);
    assert_eq!(merged.calibration_rates()["high"]["pass_rate"], 0.5);
}
