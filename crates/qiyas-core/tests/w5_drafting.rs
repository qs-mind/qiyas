//! W5 regression fixtures: the founder's villa, a coherent alternative and real symbol
//! entities in mm, cm, metres and inches. Included by the single integration-test binary.

use opencadcodec::{EntityType, Handle};
use qiyas_core::drafting_standards::{check_register, limit_for, limits};
use qiyas_core::registry::{effect_of, Effect};
use qiyas_core::space_planning::{run_checks, PlanModel};
use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::{json, Value};

const UNITS: [(i16, f64); 4] = [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)];

fn bad_plan() -> PlanModel {
    serde_json::from_value(json!({"floors":[{
        "name":"Ground floor",
        "rooms":[
            {"name":"Lobby","kind":"lobby","area_m2":35,"width_m":5},
            {"name":"Corridor","kind":"corridor","area_m2":18,"width_m":1.2,"length_m":15},
            {"name":"Kitchen","kind":"kitchen","area_m2":30,"width_m":5},
            {"name":"Living","kind":"living","area_m2":20,"width_m":4},
            {"name":"Master bedroom","kind":"bedroom","master":true,"area_m2":20,"width_m":4},
            {"name":"Bedroom 2","kind":"bedroom","area_m2":16,"width_m":4},
            {"name":"Bedroom 3","kind":"bedroom","area_m2":16,"width_m":4},
            {"name":"Bathroom","kind":"bathroom","area_m2":4,"width_m":2,"opens_to":["Corridor"]}
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
            {"name":"Lobby","kind":"lobby","area_m2":8,"width_m":2},
            {"name":"Living","kind":"living","area_m2":30,"width_m":5,"adjacent":["Dining"]},
            {"name":"Dining","kind":"dining","area_m2":16,"width_m":4,"adjacent":["Living","Kitchen"]},
            {"name":"Kitchen","kind":"kitchen","area_m2":16,"width_m":4,"adjacent":["Dining"]},
            {"name":"Stair room","kind":"stair","area_m2":8,"width_m":2.5}
        ],
        "doors":[{"width_m":1.5,"leaves":2,"main_entrance":true}],
        "stairs":[{"located_in":"Stair room","enclosed":true,"width_m":1.2,
            "riser_m":0.16,"going_m":0.3,"steps_per_flight":10,"headroom_m":2.4,"door_distance_m":1.5}]
    },{
        "name":"First floor",
        "rooms":[
            {"name":"Lobby","kind":"lobby","area_m2":5,"width_m":2},
            {"name":"Master bedroom","kind":"bedroom","master":true,"area_m2":20,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Bedroom 2","kind":"bedroom","area_m2":16,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Bedroom 3","kind":"bedroom","area_m2":16,"width_m":4,"fixtures":["wardrobe"]},
            {"name":"Master bathroom","kind":"bathroom","area_m2":5,"width_m":2,"opens_to":["Master bedroom"]},
            {"name":"Bathroom 2","kind":"bathroom","area_m2":4,"width_m":2,"opens_to":["Lobby"]},
            {"name":"Bathroom 3","kind":"bathroom","area_m2":4,"width_m":2,"opens_to":["Lobby"]}
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
    let report = run_checks(&bad_plan(), Some("egypt"), Some("villa"), Some("villa")).unwrap();
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
        let report = run_checks(&good_plan(), market, Some("villa"), Some("villa")).unwrap();
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
    let report = run_checks(&plan, None, Some("villa"), Some("villa")).unwrap();
    assert!(!report.has("master_without_ensuite"));
    assert!(report
        .not_checked
        .iter()
        .any(|v| v["rule"] == "master_without_ensuite"));
    plan.floors[0].stairs[0].riser_m = Some(f64::NAN);
    assert!(run_checks(&plan, None, Some("villa"), Some("villa")).is_err());
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
        // The amendments checked, and the one not read, travel with the finding.
        assert!(limit["note"].as_str().unwrap().contains("67/2014"), "{id}");
        assert!(limit.get("reference_to_confirm").is_none(), "{id}");
    }
    let law = &register["markets"]["egypt"]["codes"][0];
    assert_eq!(law["id"], "egypt_building_law_119");
    assert_eq!(law["evidence_read"], true);
    assert!(check_register().is_empty(), "{:?}", check_register());
}

#[test]
fn practice_defaults_are_labelled_and_never_report_code_errors() {
    // Egypt, KSA and UAE carry code values since 27 October 2026 (see the tests below).
    {
        let market: Option<&str> = None;
        let report = run_checks(&bad_plan(), market, Some("villa"), Some("villa")).unwrap();
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
fn selected_profiles_are_practice_preferences_in_every_jurisdiction() {
    for market in ["Egypt", "Dubai", "Abu Dhabi", "UK"] {
        let report = run_checks(&bad_plan(), Some(market), Some("villa"), Some("villa")).unwrap();
        assert_eq!(report.market.as_deref(), Some(market));
        assert_eq!(report.profile.as_deref(), Some("villa"));
        assert!(!report.thresholds_applied.is_empty());
        assert!(report
            .findings
            .iter()
            .all(|f| f.basis == "practice_default" && f.source.is_none()));
        assert_eq!(report.errors, 0);
    }
}

#[test]
fn jurisdiction_results_keep_the_request_and_every_scoped_alternative() {
    use qiyas_core::drafting_standards::{knowledge, KnowledgeTopic};
    use qiyas_core::rooms::LabelLanguage;
    for market in ["Dubai", "Abu Dhabi", "UAE"] {
        let result = knowledge(
            KnowledgeTopic::Limits,
            None,
            Some(market),
            None,
            None,
            LabelLanguage::En,
            &[],
        )
        .unwrap();
        assert_eq!(result["market"], market);
        let widths: Vec<_> = result["limits"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| l["id"] == "stair_width_min_m")
            .collect();
        assert_eq!(widths.len(), 3);
        assert!(widths
            .iter()
            .any(|l| l["value"] == 1.0 && l["scope"]["jurisdiction"] == "Dubai"));
        assert!(widths
            .iter()
            .any(|l| l["value"] == 1.2 && l["scope"]["jurisdiction"] == "Abu Dhabi"));
        assert!(widths.iter().any(
            |l| l["value"] == 1.2 && l["scope"]["building_classes"][0] == "apartment_building"
        ));
        assert!(widths.iter().all(|l| l["asked"] == market));
    }
    for topic in [KnowledgeTopic::Limits, KnowledgeTopic::Codes] {
        let result =
            knowledge(topic, None, Some("UK"), None, None, LabelLanguage::En, &[]).unwrap();
        assert_eq!(result["market"], "UK");
        assert_eq!(result["supported"], false);
        assert!(result["note"].as_str().unwrap().contains("UK"));
        assert!(result["note_ar"].as_str().unwrap().contains("UK"));
        assert!(result
            .get("limits")
            .is_none_or(|l| l.as_array().unwrap().is_empty()));
        assert!(result
            .get("markets")
            .is_none_or(|l| l.as_object().unwrap().is_empty()));
    }
    assert!(limit_for("stair_width_min_m", Some("UK")).is_none());
}

#[test]
fn missing_and_incompatible_profiles_return_no_judgements() {
    for (profile, kind) in [
        (None, None),
        (Some("villa"), None),
        (Some("villa"), Some("mall")),
        (Some("missing"), Some("villa")),
    ] {
        let report = run_checks(&bad_plan(), Some("Dubai"), profile, kind).unwrap();
        assert!(report.findings.is_empty());
        assert!(report.checked.is_empty());
        assert!(report.limits_used.is_empty());
        assert!(report.thresholds_applied.is_empty());
        assert_eq!(report.profile, None);
        assert!(report.available_profiles.contains(&"villa".into()));
    }
}

#[test]
fn packages_do_not_invent_a_project_type_or_storeys() {
    use qiyas_core::drafting_standards::{knowledge, sheet_set, KnowledgeTopic};
    use qiyas_core::rooms::LabelLanguage;
    let omitted = knowledge(
        KnowledgeTopic::SheetSet,
        None,
        None,
        None,
        None,
        LabelLanguage::En,
        &[],
    )
    .unwrap();
    assert_eq!(omitted["sheet_count"], 0);
    let mall = sheet_set("Mall", None).unwrap();
    assert_eq!(mall["project_type"], "Mall");
    assert_eq!(mall["sheet_count"], 0);
    assert!(mall["note"]
        .as_str()
        .unwrap()
        .contains("build the package from the brief"));
    let villa = sheet_set("villa", None).unwrap();
    assert!(villa["storeys"].is_null());
    assert!(!villa["conditional_sheets"].as_array().unwrap().is_empty());
    assert!(villa["sheets"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s.get("when").is_none()));
    assert!(
        sheet_set("residential_unit", None).unwrap()["sheet_count"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn explicit_space_kinds_win_and_names_are_only_uncertain_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("kinds.dxf"), 6);
    let plan = json!({"floors":[{"name":"Mall","rooms":[
        {"name":"Bedroom shop","kind":"retail unit","area_m2":30},
        {"name":"Store","area_m2":20},{"name":"Hall","area_m2":40},
        {"name":"Bedroom","area_m2":15}]}]});
    let Outcome::Measurement { result } = s
        .run_input(&json!({"type":"space_check","plan":plan}), Source::Ai)
        .unwrap()
        .outcome
    else {
        panic!("measurement")
    };
    let spaces = result["floors"][0]["spaces"].as_array().unwrap();
    assert_eq!(spaces[0]["kind"], "retail unit");
    assert!(spaces[0].get("suggested_kind").is_none());
    for space in &spaces[1..3] {
        assert!(space["kind"].is_null());
        assert!(space["suggested_kind"].is_null());
    }
    assert_eq!(spaces[3]["suggested_kind"], "bedroom");
    assert_eq!(spaces[3]["uncertain"], true);
    assert!(result["floors"][0]["circulation_share"].is_null());
    assert!(result["findings"].as_array().unwrap().is_empty());
}

#[test]
fn concave_space_facts_use_polygons_without_retaining_or_mutating_rooms() {
    let dir = tempfile::tempdir().unwrap();
    for (units, k) in UNITS {
        let mut s = Session::new_blank(&dir.path().join("facts.dxf"), units);
        let points: Vec<_> = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [1.0, 1.0],
            [1.0, 4.0],
            [0.0, 4.0],
        ]
        .map(|p| [p[0] / k, p[1] / k])
        .into();
        added(
            &mut s,
            json!({"type":"draw","kind":"polyline","closed":true,"points":points}),
        );
        added(
            &mut s,
            json!({"type":"text","at":[0.5/k,2.0/k],"height":0.1/k,"value":"Space A"}),
        );
        let inside = added(
            &mut s,
            json!({"type":"draw","kind":"point","p":[0.5/k,3.0/k]}),
        )[0];
        let outside = added(
            &mut s,
            json!({"type":"draw","kind":"point","p":[2.0/k,2.0/k]}),
        )[0];
        let before = snapshot(&s);
        let Outcome::Measurement { result } = s
            .run_input(
                &json!({"type":"space_check","floors":[{"name":"Floor"}]}),
                Source::Ai,
            )
            .unwrap()
            .outcome
        else {
            panic!("measurement")
        };
        assert_eq!(snapshot(&s), before);
        let space = &result["floors"][0]["spaces"][0];
        near(space["area_m2"].as_f64().unwrap(), 7.0);
        near(space["perimeter_m"].as_f64().unwrap(), 16.0);
        assert!(space["clear_widths_m"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| (w["width_m"].as_f64().unwrap() - 1.0).abs() < 1e-8));
        let fixtures = space["fixtures"].as_array().unwrap();
        assert!(fixtures.iter().any(|f| f["handle"] == inside));
        assert!(!fixtures.iter().any(|f| f["handle"] == outside));
        assert!(result["findings"].as_array().unwrap().is_empty());
    }
}

#[test]
fn each_symbol_can_use_a_caller_layer_and_default_leaves_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    for mut input in [
        json!({"type":"symbol","kind":"door","a":[0,0],"b":[1.5,0],"side_point":[0,1]}),
        json!({"type":"symbol","kind":"stair","origin":[0,0],"width":1.2,"length_m":3,"rise_m":1.7,"steps":10,"floor":"upper"}),
        json!({"type":"symbol","kind":"levels","origin":[0,0],"levels":[{"label":"FFL","elevation_m":0}]}),
        json!({"type":"symbol","kind":"ground_line","a":[0,0],"b":[8,0]}),
        json!({"type":"symbol","kind":"foundation_level","a":[0,0],"b":[8,0],"elevation_m":-1.2}),
        json!({"type":"symbol","kind":"material_hatch","points":[[0,0],[1,0],[1,2],[0,2]],"material":"concrete","point":[0.5,1],"at":[3,2]}),
    ] {
        input["layer"] = json!("Chosen layer");
        let mut s = Session::new_blank(&dir.path().join("layer.dxf"), 6);
        let report = s.run_input(&input, Source::User).unwrap();
        let Outcome::Added { handles } = report.outcome else {
            panic!("added")
        };
        assert!(handles.iter().all(|h| {
            s.drawing
                .doc
                .get_entity(Handle::new(*h))
                .unwrap()
                .common()
                .layer
                == "Chosen layer"
        }));
        assert_eq!(s.current.layer, "0");
        if input["kind"] == "door" {
            assert!(report
                .defaults_used
                .iter()
                .any(|d| d.field == "/leaves" && d.value == 2));
            assert!(s
                .takeoff_check(0, &[])
                .unwrap()
                .to_string()
                .contains("type_default"));
        }
    }
}
