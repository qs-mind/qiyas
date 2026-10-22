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
