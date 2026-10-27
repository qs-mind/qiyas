//! W5 regression fixtures: the founder's villa, a coherent alternative and real symbol
//! entities in mm, cm, metres and inches. Included by the single integration-test binary.

use opencadcodec::{EntityType, Handle};
use qiyas_core::drafting_standards::{check_register, limits};
use qiyas_core::registry::{effect_of, Effect};
use qiyas_core::space_planning::{run_checks, PlanModel};
use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::{json, Value};

const UNITS: [(i16, f64); 4] = [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)];

fn bad_plan() -> PlanModel {
    serde_json::from_value(json!({"floors":[{
        "name":"Ground floor",
        "rooms":[
            {"name":"Lobby","area_m2":35,"width_m":5},
            {"name":"Corridor","area_m2":18,"width_m":1.2,"length_m":15},
            {"name":"Kitchen","area_m2":30,"width_m":5},
            {"name":"Living","area_m2":20,"width_m":4},
            {"name":"Master bedroom","area_m2":20,"width_m":4},
            {"name":"Bedroom 2","area_m2":16,"width_m":4},
            {"name":"Bedroom 3","area_m2":16,"width_m":4},
            {"name":"Bathroom","area_m2":4,"width_m":2,"opens_to":["Corridor"]}
        ],
        "doors":[{"name":"Entrance","width_m":0.9,"leaves":1,"main_entrance":true}],
        "stairs":[{"name":"Stair","located_in":"Corridor","enclosed":false,
            "width_m":0.9,"passage_beside_m":0.5,"riser_m":0.2,"going_m":0.22,
            "steps_per_flight":18,"headroom_m":1.9,"door_distance_m":0.2}]
    }]}))
    .unwrap()
}

fn good_plan() -> PlanModel {
    serde_json::from_value(json!({"floors":[{
        "name":"Ground floor",
        "rooms":[
            {"name":"Lobby","area_m2":8,"width_m":2},
            {"name":"Living","area_m2":30,"width_m":5,"adjacent":["Dining"]},
            {"name":"Dining","area_m2":16,"width_m":4,"adjacent":["Living","Kitchen"]},
            {"name":"Kitchen","area_m2":16,"width_m":4,"adjacent":["Dining"]},
            {"name":"Stair room","area_m2":8,"width_m":2.5}
        ],
        "doors":[{"width_m":1.5,"leaves":2,"main_entrance":true}],
        "stairs":[{"located_in":"Stair room","enclosed":true,"width_m":1.2,
            "riser_m":0.16,"going_m":0.3,"steps_per_flight":10,"headroom_m":2.4,"door_distance_m":1.5}]
    },{
        "name":"First floor",
        "rooms":[
            {"name":"Lobby","area_m2":5,"width_m":2},
            {"name":"Master bedroom","area_m2":20,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Bedroom 2","area_m2":16,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Bedroom 3","area_m2":16,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Master bathroom","area_m2":5,"width_m":2,"opens_to":["Master bedroom"]},
            {"name":"Bathroom 2","area_m2":4,"width_m":2,"opens_to":["Lobby"]},
            {"name":"Bathroom 3","area_m2":4,"width_m":2,"opens_to":["Lobby"]}
        ]
    }]})).unwrap()
}

fn added(s: &mut Session, value: Value) -> Vec<u64> {
    match s.run_input(&value, Source::Ai).unwrap().outcome {
        Outcome::Added { handles } => handles,
        other => panic!("Expected symbol handles, got {other:?}"),
    }
}

fn texts(s: &Session) -> Vec<String> {
    s.drawing
        .doc
        .entities()
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            EntityType::MText(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect()
}

fn snapshot(s: &Session) -> Value {
    let mut entities: Vec<_> = s.drawing.doc.entities().collect();
    entities.sort_by_key(|e| e.common().handle.value());
    json!({"entities":entities,"layers":s.drawing.doc.layers.iter().collect::<Vec<_>>(),
        "blocks":s.drawing.doc.block_records.iter().collect::<Vec<_>>(),
        "linetypes":s.drawing.doc.line_types.iter().collect::<Vec<_>>(),"current":s.current})
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

#[test]
fn corridor_villa_reports_the_founders_complaints() {
    let report = run_checks(&bad_plan(), Some("egypt")).unwrap();
    for rule in [
        "circulation_share",
        "corridor_length",
        "kitchen_larger_than_living",
        "master_without_ensuite",
        "bedroom_without_wardrobe",
        "stair_not_enclosed",
        "stair_blocks_corridor",
        "entrance_door_narrow",
    ] {
        assert!(report.has(rule), "Missing {rule}: {report:#?}");
    }
    assert!(report
        .findings
        .iter()
        .all(|f| !f.message_ar.is_empty() && !f.fix_ar.is_empty()));
    assert_eq!(report.status, "For review");
}

#[test]
fn a_coherent_villa_has_no_findings_in_each_market() {
    for market in [None, Some("egypt"), Some("ksa"), Some("uae")] {
        let report = run_checks(&good_plan(), market).unwrap();
        assert!(report.findings.is_empty(), "{market:?}: {report:#?}");
        for rule in [
            "circulation_share",
            "kitchen_larger_than_living",
            "master_without_ensuite",
            "bedroom_without_wardrobe",
            "stair_not_enclosed",
            "entrance_door_narrow",
        ] {
            assert!(
                report.checked.contains(&rule),
                "Good plan never checked {rule}"
            );
        }
    }
}

#[test]
fn unknown_bathroom_access_is_not_a_failed_ensuite_check() {
    let mut plan = bad_plan();
    let mut unknown = plan.floors[0].rooms.last().unwrap().clone();
    unknown.name = "Bathroom with unknown door".into();
    unknown.opens_to.clear();
    plan.floors[0].rooms.push(unknown);
    let report = run_checks(&plan, None).unwrap();
    assert!(!report.has("master_without_ensuite"));
    assert!(report
        .not_checked
        .iter()
        .any(|v| v["rule"] == "master_without_ensuite"));
    plan.floors[0].stairs[0].riser_m = Some(f64::NAN);
    assert!(run_checks(&plan, None).is_err());
}

#[test]
fn knowledge_and_space_check_are_registered_reads_without_undo_or_document_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("reads.dxf"), 6);
    let before = snapshot(&s);
    let history = s.history_steps().0.len();
    for value in [
        json!({"type":"drafting_knowledge","topic":"sheet_set","project_type":"villa","storeys":2}),
        json!({"type":"space_check","plan":bad_plan(),"language":"ar"}),
    ] {
        let command: Command = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(effect_of(&command), Effect::Read);
        let Outcome::Measurement { result } = s.run_input(&value, Source::Ai).unwrap().outcome
        else {
            panic!("Expected measurement read");
        };
        assert!(result.is_object());
        assert_eq!(snapshot(&s), before);
        assert_eq!(s.history_steps().0.len(), history);
    }
}

#[test]
fn doors_have_real_leaves_and_swings_in_every_unit_and_reuse_their_block() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        for leaves in [1, 2] {
            let mut s = Session::new_blank(&dir.path().join("door.dxf"), units);
            let width = if leaves == 1 { 0.9 } else { 1.5 };
            let before = snapshot(&s);
            let steps = s.history_steps().0.len();
            let value = json!({"type":"symbol","kind":"door","a":[0,0],"b":[width/k,0],"side_point":[0,1.0/k],"leaves":leaves});
            let handles = added(&mut s, value.clone());
            let EntityType::Insert(insert) =
                s.drawing.doc.get_entity(Handle::new(handles[0])).unwrap()
            else {
                panic!("Door must be a block");
            };
            let block = insert.block_name.clone();
            let parts: Vec<_> = s.drawing.doc.entities_in_block(&block).collect();
            assert_eq!(
                parts
                    .iter()
                    .filter(|e| matches!(e, EntityType::Line(_)))
                    .count(),
                leaves
            );
            assert_eq!(
                parts
                    .iter()
                    .filter(|e| matches!(e, EntityType::Arc(_)))
                    .count(),
                leaves
            );
            for e in parts {
                if let EntityType::Line(l) = e {
                    near(
                        (l.end.x - l.start.x).hypot(l.end.y - l.start.y) * k,
                        width / leaves as f64,
                    );
                }
            }
            assert_eq!(s.history_steps().0.len(), steps + 1);
            assert_eq!(s.current.layer, "0");
            assert!(s.undo());
            assert_eq!(snapshot(&s), before);
            assert!(s.redo());
            let mut second = value;
            // Use identical local width arithmetic to exercise recipe reuse without relying
            // on subtracting two translated floating point coordinates to be bit-identical.
            second["a"] = json!([0, 3.0 / k]);
            second["b"] = json!([width / k, 3.0 / k]);
            second["side_point"] = json!([0, 4.0 / k]);
            let next = added(&mut s, second);
            let EntityType::Insert(insert) =
                s.drawing.doc.get_entity(Handle::new(next[0])).unwrap()
            else {
                panic!("Door must be a block");
            };
            assert_eq!(insert.block_name, block);
        }
    }
}

#[test]
fn stairs_show_up_and_a_cut_below_but_dn_void_and_handrails_above() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        for floor in ["lower", "upper"] {
            let mut s = Session::new_blank(&dir.path().join("stair.dxf"), units);
            added(
                &mut s,
                json!({"type":"symbol","kind":"stair","origin":[0,0],
                "width":1.2/k,"length_m":3,"rise_m":1.7,"steps":10,"floor":floor}),
            );
            let text = texts(&s);
            assert!(text
                .iter()
                .any(|t| t == if floor == "lower" { "UP" } else { "DN" }));
            let rails = s
                .drawing
                .doc
                .entities()
                .filter(|e| e.common().layer == "A-FLOR-HRAL")
                .count();
            let hidden = s
                .drawing
                .doc
                .entities()
                .filter(|e| e.common().linetype == "HIDDEN")
                .count();
            if floor == "lower" {
                assert!(hidden > 0);
                assert!(s.drawing.doc.entities().any(|e| matches!(e, EntityType::LwPolyline(p) if p.vertices.len() == 4 && !p.is_closed)));
                assert!(!text.iter().any(|t| t == "DN" || t == "VOID"));
            } else {
                assert!(rails > 0);
                assert_eq!(hidden, 0);
                assert!(text.iter().any(|t| t == "VOID"));
                assert!(!text.iter().any(|t| t == "UP"));
            }
            let max_y = s
                .drawing
                .doc
                .entities()
                .filter_map(|e| match e {
                    EntityType::Line(l) => Some(l.start.y.max(l.end.y) * k),
                    _ => None,
                })
                .fold(f64::NEG_INFINITY, f64::max);
            near(max_y, 3.0);
        }
    }
}

#[test]
fn elevations_have_signed_levels_and_a_ground_line_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        let mut s = Session::new_blank(&dir.path().join("elevation.dxf"), units);
        added(
            &mut s,
            json!({"type":"symbol","kind":"levels","origin":[0,0],"levels":[
            {"label":"FFL","elevation_m":0},{"label":"Roof","elevation_m":3.2},{"label":"Parapet","elevation_m":4.3}]}),
        );
        for text in ["FFL +0.00 m", "Roof +3.20 m", "Parapet +4.30 m"] {
            assert!(texts(&s).iter().any(|t| t == text));
        }
        assert_eq!(
            s.drawing
                .doc
                .entities()
                .filter(|e| matches!(e, EntityType::LwPolyline(_)))
                .count(),
            3
        );
        let ground = added(
            &mut s,
            json!({"type":"symbol","kind":"ground_line","a":[0,0],"b":[8.0/k,0]}),
        );
        assert!(ground.len() > 1);
        let EntityType::Line(line) = s.drawing.doc.get_entity(Handle::new(ground[0])).unwrap()
        else {
            panic!("Ground line missing");
        };
        near((line.end.x - line.start.x) * k, 8.0);
        assert_eq!(line.common.layer, "A-ELEV-GRND");
        added(
            &mut s,
            json!({"type":"symbol","kind":"foundation_level","a":[0,-1.2/k],"b":[8.0/k,-1.2/k],"elevation_m":-1.2}),
        );
        assert!(texts(&s)
            .iter()
            .any(|t| t.contains("indicative") && t.contains("-1.20")));
        assert!(s
            .drawing
            .doc
            .entities()
            .any(|e| e.common().layer == "A-SECT-FNDN" && e.common().linetype == "HIDDEN"));
    }
}

#[test]
fn sections_and_details_have_material_cut_hatch_and_leader_labels_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        for (view, material, pattern) in [
            ("section", "masonry", "ANSI31"),
            ("detail", "concrete", "ANSI37"),
        ] {
            let mut s = Session::new_blank(&dir.path().join("cut.dxf"), units);
            added(
                &mut s,
                json!({"type":"symbol","kind":"material_hatch","material":material,"view":view,
                "points":[[0,0],[0.3/k,0],[0.3/k,3.0/k],[0,3.0/k]],"point":[0.15/k,1.5/k],"at":[2.0/k,2.0/k]}),
            );
            let hatch = s
                .drawing
                .doc
                .entities()
                .find_map(|e| match e {
                    EntityType::Hatch(h) => Some(h),
                    _ => None,
                })
                .expect("Cut hatch");
            assert_eq!(hatch.pattern.name, pattern);
            near(
                hatch.pattern_scale * k,
                if view == "section" {
                    0.25 / 3.175
                } else {
                    0.2 / 3.175
                },
            );
            assert!(s
                .drawing
                .doc
                .entities()
                .any(|e| matches!(e, EntityType::Leader(_))));
            assert!(s
                .drawing
                .doc
                .entities()
                .any(|e| e.common().layer == "A-SECT-CUT"));
            assert!(texts(&s)
                .iter()
                .any(|t| t.contains(if material == "masonry" {
                    "Masonry"
                } else {
                    "concrete"
                })));
        }
    }
}

#[test]
fn failed_symbol_rolls_back_geometry_layers_and_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("rollback.dxf"), 6);
    let before = snapshot(&s);
    let steps = s.history_steps().0.len();
    let result = s.run_input(
        &json!({"type":"symbol","kind":"levels","origin":[0,0],"levels":[
        {"label":"FFL","elevation_m":0},{"label":"","elevation_m":3}]}),
        Source::Ai,
    );
    assert!(result.is_err());
    assert_eq!(snapshot(&s), before);
    assert_eq!(s.history_steps().0.len(), steps);
}

#[test]
fn code_register_never_presents_an_unsourced_value_as_code() {
    assert!(check_register().is_empty(), "{:?}", check_register());
    for limit in limits() {
        if limit.basis == "code" {
            assert!(limit
                .source_url
                .as_deref()
                .is_some_and(|s| s.starts_with("https://")));
            assert!(limit.code.as_deref().is_some_and(|s| !s.trim().is_empty()));
            assert!(limit
                .clause
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty()));
        } else {
            assert_eq!(limit.basis, "practice_default");
            assert!(limit
                .rationale
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty()));
        }
    }
}

#[test]
fn egypt_limits_are_code_values_quoting_the_arabic_article() {
    let register = qiyas_core::drafting_standards::register();
    let raw: Vec<_> = register["limits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["market"] == "egypt")
        .collect();
    assert_eq!(raw.len(), 22);
    for limit in raw {
        let id = limit["id"].as_str().unwrap();
        assert_eq!(limit["basis"], "code", "{id}");
        assert_eq!(limit["evidence_read"], true, "{id}");
        assert!(
            limit["clause"].as_str().unwrap().starts_with("Article "),
            "{id}"
        );
        assert!(
            limit["source_url"]
                .as_str()
                .unwrap()
                .starts_with("https://"),
            "{id}"
        );
        // Each value quotes the Arabic article it comes from, figure included.
        let quote = limit["quote_ar"].as_str().unwrap();
        assert!(quote.chars().count() > 10, "{id}");
        // The caveat about the edition travels with the finding.
        assert!(limit["note"].as_str().unwrap().contains("774/2014"), "{id}");
        assert!(limit.get("reference_to_confirm").is_none(), "{id}");
    }
    let law = &register["markets"]["egypt"]["codes"][0];
    assert_eq!(law["id"], "egypt_building_law_119");
    assert_eq!(law["evidence_read"], true);
    assert!(check_register().is_empty(), "{:?}", check_register());
}

#[test]
fn practice_defaults_are_labelled_and_never_report_code_errors() {
    // Egypt's limits are code values since 27 October 2026 (see the test below).
    for market in [None, Some("ksa"), Some("uae")] {
        let report = run_checks(&bad_plan(), market).unwrap();
        assert!(!report.findings.is_empty());
        assert_eq!(report.errors, 0);
        for finding in &report.findings {
            assert_eq!(finding.basis, "practice_default");
            assert_ne!(finding.severity, "error");
            assert!(finding.message.contains("practice default"));
            assert!(finding.message_ar.contains("قيمة ممارسة معتادة"));
            assert!(finding.source.is_none() && finding.source_url.is_none());
        }
        let circulation = report
            .findings
            .iter()
            .find(|f| f.rule == "circulation_share")
            .unwrap();
        assert_eq!(circulation.severity, "warning");
    }
}

#[test]
fn egypt_code_findings_cite_the_article_its_source_and_the_edition_caveat() {
    let report = run_checks(&bad_plan(), Some("egypt")).unwrap();
    let code: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.basis == "code")
        .collect();
    assert!(!code.is_empty(), "{report:#?}");
    for finding in &code {
        assert!(finding
            .source_url
            .as_deref()
            .is_some_and(|u| u.starts_with("https://")));
        assert!(finding
            .caveat
            .as_deref()
            .is_some_and(|c| c.contains("774/2014")));
    }
    for finding in report.findings.iter().filter(|f| f.basis != "code") {
        assert_eq!(finding.basis, "practice_default");
        assert_ne!(finding.severity, "error");
    }
}
