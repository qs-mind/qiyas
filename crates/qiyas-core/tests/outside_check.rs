//! Independent facts remain available when classifications and generated quantities are wrong.
use qiyas_core::{knowledge, outside_check::OutsideQuery, Outcome, Session, Source};
use serde_json::{json, Value};

fn run(s: &mut Session, value: Value) -> Outcome {
    s.run_input(&value, Source::User).unwrap().outcome
}
fn add(s: &mut Session, value: Value) -> u64 {
    match run(s, value) {
        Outcome::Added { handles } => handles[0],
        o => panic!("{o:?}"),
    }
}
fn rectangle(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> u64 {
    add(s, json!({"type":"draw","kind":"rectangle","a":a,"b":b}))
}
fn rows(s: &Session, mut query: OutsideQuery) -> Vec<Value> {
    let mut rows = Vec::new();
    loop {
        let page = s.outside_check(&query).unwrap();
        rows.extend(page["entries"].as_array().unwrap().iter().cloned());
        match page["next_offset"].as_u64() {
            Some(n) => {
                assert!(n as usize > query.offset);
                query.offset = n as usize;
            }
            None => break,
        }
    }
    rows
}
fn line(handles: &[u64], quantity: f64, unit: &str) -> Value {
    json!({"label":"Measured / مقاس","operation":"add","timesing":1,"factors":[quantity],
        "source_expressions":[],"expression":quantity.to_string(),"value":quantity,"unit":unit,"method":"test",
        "citations":[],"sources":handles.iter().map(|h| json!({"handle":h,"sheet":"*Model_Space","grid":null,"evidence":[]})).collect::<Vec<_>>(),
        "included":true,"decision":null,"operands":[]})
}
fn item(code: &str, handles: &[u64], quantity: f64, unit: &str) -> Value {
    json!({"code":code,"description_en":"Test","description_ar":"اختبار","section":"","trade":"","unit":unit,
        "quantity":quantity,"mapping_citations":[],"coverage":[],"lines":[line(handles,quantity,unit)]})
}
fn report(s: &mut Session, items: Vec<Value>) {
    let mut data =
        serde_json::to_value(qiyas_core::measurement::MeasurementReport::default()).unwrap();
    data["items"] = json!(items);
    knowledge::put(
        &mut s.drawing.doc,
        knowledge::Kind::Measurement,
        "result",
        &data,
    )
    .unwrap();
}

#[test]
fn raw_blocks_recount_twenty_eight_doors_even_when_boq_loses_the_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("doors.dxf"), 6);
    run(
        &mut s,
        json!({"type":"define_block","name":"D","base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":1,"y":0,"z":0}}}]}),
    );
    let handles: Vec<_> = (0..28)
        .map(|i| add(&mut s, json!({"type":"insert_block","name":"D","at":[i,0]})))
        .collect();
    report(
        &mut s,
        vec![
            item("D1", &handles[..14], 14.0, "nr"),
            item("D2", &handles[14..], 14.0, "pcs"),
        ],
    );
    let correct = rows(&s, Default::default());
    let raw = correct
        .iter()
        .find(|v| v["kind"] == "raw_recount" && v["group"]["kind"] == "blocks")
        .unwrap();
    assert_eq!(raw["raw_count"], 28);
    assert_eq!(raw["boq_count"], 28.0);
    assert!(!correct
        .iter()
        .any(|v| v["kind"] == "quantity_mismatch" && v["group"]["kind"] == "blocks"));
    report(&mut s, vec![]);
    let incorrect = rows(&s, Default::default());
    let mismatch = incorrect
        .iter()
        .find(|v| v["kind"] == "quantity_mismatch" && v["group"]["kind"] == "blocks")
        .unwrap();
    assert_eq!(mismatch["raw_quantity"], 28.0);
    assert_eq!(mismatch["boq_quantity"], 0.0);
    assert_eq!(mismatch["handles"].as_array().unwrap().len(), 28);
    assert_eq!(mismatch["boq_handles"], json!([]));
}

#[test]
fn geometry_facts_include_partial_outside_dangling_no_opening_and_overlap_area() {
    for units in [4, 6] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("geometry.dxf"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        let outline = rectangle(&mut s, [0., 0.], [10. * u, 10. * u]);
        let a = rectangle(&mut s, [u, u], [5. * u, 5. * u]);
        let b = rectangle(&mut s, [4. * u, 4. * u], [7. * u, 7. * u]);
        let outside = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[9.*u,8.*u],"b":[11.*u,8.*u]}),
        );
        let wall = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[2.*u,8.*u],"b":[4.*u,8.*u]}),
        );
        let query = OutsideQuery {
            boundary: Some(outline),
            space_handles: vec![a, b],
            wall_handles: vec![wall],
            ..Default::default()
        };
        let found = rows(&s, query.clone());
        assert!(found
            .iter()
            .any(|v| v["kind"] == "outside_outline" && v["handles"] == json!([outside])));
        assert_eq!(
            found
                .iter()
                .filter(|v| v["kind"] == "dangling_wall" && v["handles"] == json!([wall]))
                .count(),
            2
        );
        assert_eq!(
            found
                .iter()
                .filter(|v| v["kind"] == "space_without_opening")
                .count(),
            2
        );
        let overlap = found.iter().find(|v| v["kind"] == "space_overlap").unwrap()["area_m2"]
            .as_f64()
            .unwrap();
        assert!((overlap - 1.).abs() < 1e-8);
        let opening = add(
            &mut s,
            json!({"type":"draw","kind":"line","a":[2.*u,0.5*u],"b":[2.*u,1.5*u]}),
        );
        let found = rows(
            &s,
            OutsideQuery {
                opening_handles: vec![opening],
                ..query
            },
        );
        assert!(!found
            .iter()
            .any(|v| v["kind"] == "space_without_opening" && v["handles"] == json!([a])));
        assert!(found
            .iter()
            .any(|v| v["kind"] == "space_without_opening" && v["handles"] == json!([b])));
    }
}

#[test]
fn answer_units_arabic_digits_and_decision_review_are_facts_not_edits() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("review.dxf"), 6);
    let h = rectangle(&mut s, [0., 0.], [3., 4.]);
    run(
        &mut s,
        json!({"type":"decision","kind":"set","entries":{"name":"rise","value":3,"unit":"m","reason":"Span calculation / حساب البحر",
        "source":"mizan_design","citation":"Calculation / الحساب","set_by":"mizan","status":"provisional"}}),
    );
    let mut generated = item("area", &[h], 12., "m2");
    generated["lines"][0]["operands"] = json!([{"label":"rise","expression":"rise","value":3,"unit":"m","sources":[{"name":"rise","kind":"mizan","decision":"rise"}]}]);
    report(&mut s, vec![generated]);
    let before = s.measurement_report().unwrap();
    let found = rows(
        &s,
        OutsideQuery {
            answer: Some(
                "Area 12 m²; height 3000 mm; المساحة ١٢ م²؛ الطول ٩٩٩ م؛ mass 12 kg".into(),
            ),
            ..Default::default()
        },
    );
    let unsupported: Vec<_> = found
        .iter()
        .filter(|v| v["kind"] == "unmatched_answer_number")
        .collect();
    assert_eq!(unsupported.len(), 2);
    assert!(unsupported.iter().any(|v| v["value"] == 999.));
    assert!(unsupported.iter().any(|v| v["unit"] == "kg"));
    assert!(found.iter().any(|v| v["kind"] == "decision_dependency"
        && v["review_only"] == true
        && v["handles"] == json!([h])));
    assert!(found.iter().any(|v| v["kind"] == "design_decision"
        && v["decision"]["reason"] == "Span calculation / حساب البحر"));
    assert_eq!(s.measurement_report().unwrap(), before);
}

#[test]
fn paging_keeps_every_handle_and_totals_and_takeoff_check_keeps_existing_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("pages.dxf"), 6);
    let boundary = rectangle(&mut s, [0., 0.], [1., 1.]);
    let outside: Vec<_> = (0..33)
        .map(|i| {
            add(
                &mut s,
                json!({"type":"draw","kind":"line","a":[2+i,2],"b":[2+i,3]}),
            )
        })
        .collect();
    let query = OutsideQuery {
        boundary: Some(boundary),
        limit: 2,
        ..Default::default()
    };
    let first = s.outside_check(&query).unwrap();
    assert_eq!(first["totals"]["outside_outline"], 33);
    let found = rows(&s, query);
    for h in outside {
        assert!(found
            .iter()
            .any(|v| v["kind"] == "outside_outline" && v["handles"] == json!([h])));
    }
    assert_eq!(
        found.len(),
        first["total_entries"].as_u64().unwrap() as usize
    );
    let check = s.takeoff_check(0, &[]).unwrap();
    assert!(check.get("measurement_errors").is_some());
    assert!(check.get("passed").is_some());
    assert_eq!(check["outside_check"]["totals"]["outside_outline"], 33);
}

#[test]
fn closed_wall_union_supplies_outline_without_model_rules_or_a_room_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("walls.dxf"), 6);
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[8,0],[8,6],[0,6]],"closed":true}],"thickness":0.2,"height":3}),
    );
    let found = rows(&s, Default::default());
    assert!(found
        .iter()
        .any(|v| v["kind"] == "outline" && v["basis"] == "wall_union"));
    assert!(found.iter().any(|v| v["kind"] == "space_without_opening"));
    assert!(!found.iter().any(|v| v["kind"] == "dangling_wall"));
}

#[test]
fn comparison_does_not_allocate_cross_group_lines_or_double_negate_deductions() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("groups.dxf"), 6);
    let a = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[3,0]}),
    );
    let b = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,1],"b":[5,1]}),
    );
    // Change the fixture's layer directly, independently of the object and mapping engines.
    s.drawing
        .doc
        .get_entity_mut(opencadcodec::Handle::new(b))
        .unwrap()
        .common_mut()
        .layer = "other".into();
    report(&mut s, vec![item("combined", &[a, b], 8., "m")]);
    let found = rows(&s, Default::default());
    assert_eq!(
        found
            .iter()
            .filter(|v| v["kind"] == "unallocated_boq")
            .count(),
        2
    );
    assert!(!found.iter().any(|v| v["kind"] == "quantity_mismatch"));
    let mut split = item("split", &[a, b], 8., "m");
    let mut deduction = line(&[a], -1., "m");
    deduction["operation"] = json!("deduct");
    split["lines"] = json!([line(&[a], 4., "m"), deduction, line(&[b], 5., "m")]);
    report(&mut s, vec![split]);
    let found = rows(&s, Default::default());
    assert!(!found
        .iter()
        .any(|v| v["kind"] == "quantity_mismatch" || v["kind"] == "unallocated_boq"));
}

#[test]
fn raw_recount_is_per_sheet_and_reports_lost_boq_source_handles() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("sheets.dxf"), 6);
    let a = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[3,0]}),
    );
    run(
        &mut s,
        json!({"type":"new_layout","name":"Sheet A","paper":"ISO A3"}),
    );
    let sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sheet| sheet.name == "Sheet A")
        .unwrap()
        .block;
    run(&mut s, json!({"type":"set_sheet","block":sheet}));
    let b = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
    );
    report(&mut s, vec![item("lost", &[999999], 4., "m")]);
    let found = rows(&s, Default::default());
    let raw: Vec<_> = found
        .iter()
        .filter(|v| v["kind"] == "raw_recount" && v["group"]["kind"] == "length")
        .collect();
    assert_eq!(raw.len(), 2);
    assert!(raw
        .iter()
        .any(|v| v["handles"] == json!([a]) && v["raw_quantity"] == 3.));
    assert!(raw
        .iter()
        .any(|v| v["handles"] == json!([b]) && v["raw_quantity"] == 5.));
    assert!(found
        .iter()
        .any(|v| v["kind"] == "unlocated_boq_source" && v["handles"] == json!([999999])));
}

#[test]
fn a_concave_outline_checks_whole_segments_not_just_their_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("concave.dxf"), 6);
    let boundary = add(
        &mut s,
        json!({"type":"draw","kind":"polyline","closed":true,"points":[[0,0],[6,0],[6,6],[4,6],[4,2],[2,2],[2,6],[0,6]]}),
    );
    let crossing = add(
        &mut s,
        json!({"type":"draw","kind":"line","a":[1,5],"b":[5,5]}),
    );
    let found = rows(
        &s,
        OutsideQuery {
            boundary: Some(boundary),
            ..Default::default()
        },
    );
    assert!(found
        .iter()
        .any(|v| v["kind"] == "outside_outline" && v["handles"] == json!([crossing])));
}

#[test]
fn reasoned_exclusions_close_coverage_by_handle_type_and_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("exclusions.dxf"), 6);
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{
        "name":"Fixture","category":"illustration","geometry":"point"}}),
    );
    let h = add(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Fixture","points":[[1,1]]}]}),
    );
    run(
        &mut s,
        json!({"type":"define_object_type","definition":{
        "name":"Observed","category":"measured","geometry":"point"}}),
    );
    add(
        &mut s,
        json!({"type":"place_objects","objects":[{"type":"Observed","points":[[3,3]]}]}),
    );
    let expectations =
        [
            serde_json::from_value(json!({"kind":"count","entity_type":"Point","value":2}))
                .unwrap(),
        ];
    let kit = qiyas_core::takeoff_kit::kit();
    for phase in ["method", "mappings"] {
        let mut batch = kit["phases"][phase].clone();
        batch["type"] = json!("batch");
        run(&mut s, batch);
    }
    assert_eq!(
        s.takeoff_check(0, &expectations).unwrap()["unmeasured_elements"],
        1
    );
    for target in [json!({"handles":[h]}), json!({"object_type":"Fixture"})] {
        let mut command = json!({"type":"decision","kind":"set","entries":{
            "name":"Illustration scope","value":"excluded","unit":"","reason":"Illustration only",
            "source":"mizan_design","citation":"Drawing scope","set_by":"mizan","status":"answered",
            "takeoff_exclusion":target}});
        command["entries"]["reason"] = json!(" ");
        assert!(!qiyas_core::validation::validate_command(&command, None).valid);
        assert!(s.run_input(&command, Source::Ai).is_err());
        command["entries"]["reason"] = json!("Illustration only");
        run(&mut s, command);
        let check = s.takeoff_check(0, &expectations).unwrap();
        assert_eq!(check["unmeasured_elements"], 0);
        assert_eq!(check["excluded"], 1);
        assert_eq!(check["passed"], true, "{check}");
        assert!(check["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["section"] == "excluded"
                && r["handle"] == h
                && r["reason"] == "Illustration only"));
        assert!(s.undo());
    }
    let check = s
        .takeoff_check_with_report(0, &expectations, &format!("Exclude {h}: Illustration only"))
        .unwrap();
    assert_eq!(check["unmeasured_elements"], 0);
    assert_eq!(check["excluded"], 1);
    assert_eq!(check["passed"], true, "{check}");
    assert_eq!(
        s.takeoff_check_with_report(0, &expectations, &format!("Exclude {h}: "))
            .unwrap()["unmeasured_elements"],
        1
    );
}

fn shop_plan(s: &mut Session, missing_door: bool, gap: bool) {
    run(
        s,
        json!({"type":"walls","join":false,"thickness":0.2,"height":3,
        "layers":{"outline":"A-EXT","centreline":"A-EXT-C"},
        "runs":[{"points":[[0,0],[12,0],[12,8],[0,8]],"closed":true}],
        "openings":[{"run":0,"at":2,"width":1,"kind":"door"},
                    {"run":0,"at":29,"width":1,"kind":"window"}]}),
    );
    let doors = if missing_door {
        vec![json!({"run":0,"at":2,"width":1,"kind":"door"})]
    } else {
        vec![
            json!({"run":0,"at":2,"width":1,"kind":"door"}),
            json!({"run":0,"at":8,"width":1,"kind":"door"}),
        ]
    };
    run(
        s,
        json!({"type":"walls","join":false,"thickness":0.2,"height":3,
        "layers":{"outline":"A-SHOP","centreline":"A-SHOP-C","doors":"SHOP-DOORS"},
        "runs":[{"points":[[0,3],[12,3]]}],"openings":doors}),
    );
    run(
        s,
        json!({"type":"walls","join":false,"thickness":0.15,"height":3,
        "layers":{"outline":"A-PART","centreline":"A-PART-C"},
        "runs":[{"points":[[6,if gap {3.5} else {3.0}],[6,8]]}]}),
    );
    // An L in the corridor, meeting the exterior and shop wall at T junctions.
    run(
        s,
        json!({"type":"walls","join":false,"thickness":0.15,"height":3,
        "layers":{"outline":"A-CORE","centreline":"A-CORE-C"},
        "runs":[{"points":[[10,0],[10,1.5],[12,1.5]]}],
        "openings":[{"run":0,"at":0.7,"width":0.6,"kind":"door"}]}),
    );
}

#[test]
fn qiyas_shop_plan_has_no_false_geometric_findings() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("shops.dxf"), 6);
    shop_plan(&mut s, false, false);
    let found = rows(&s, Default::default());
    let faults: Vec<_> = found
        .iter()
        .filter(|r| {
            [
                "dangling_wall",
                "space_without_opening",
                "space_overlap",
                "outside_outline",
            ]
            .contains(&r["kind"].as_str().unwrap())
        })
        .collect();
    assert!(faults.is_empty(), "{faults:#?}");
}

#[test]
fn qiyas_shop_plan_still_reports_missing_door_gap_and_outside_element() {
    let dir = tempfile::tempdir().unwrap();
    for (missing, gap) in [(true, false), (false, true)] {
        let mut s = Session::new_blank(&dir.path().join("faults.dxf"), 6);
        shop_plan(&mut s, missing, gap);
        let site = rectangle(&mut s, [-1., -1.], [13., 9.]);
        let outside = rectangle(&mut s, [15., 1.], [16., 2.]);
        let found = rows(
            &s,
            OutsideQuery {
                boundary: Some(site),
                ..Default::default()
            },
        );
        assert!(found
            .iter()
            .any(|r| r["kind"] == "outside_outline" && r["handles"] == json!([outside])));
        assert!(
            found.iter().any(|r| r["kind"]
                == if missing {
                    "space_without_opening"
                } else {
                    "dangling_wall"
                }),
            "{found:#?}"
        );
    }
}

#[test]
fn wall_graphics_are_not_extra_quantities_and_different_dimensions_are_not_mismatches() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("wall-recount.dxf"), 6);
    run(
        &mut s,
        json!({"type":"walls","thickness":0.2,"height":3,
        "runs":[{"points":[[0,0],[8,0],[8,6],[0,6]],"closed":true}],
        "openings":[{"run":0,"at":2,"width":1,"kind":"door"}]}),
    );
    let mut items = Vec::new();
    for e in s.drawing.doc.entities() {
        let Some(tag) = qiyas_core::xdata::read_json::<Value>(e.common(), "QIYAS_WALL") else {
            continue;
        };
        let h = e.common().handle.value();
        if tag["role"] == "run" {
            items.push(item("wall-area", &[h], 80., "m2"));
        }
        if tag["role"] == "opening" {
            items.push(item("door", &[h], 1., "nr"));
        }
    }
    report(&mut s, items);
    let found = rows(&s, Default::default());
    assert!(
        !found
            .iter()
            .any(|r| r["kind"] == "quantity_mismatch" || r["kind"] == "unallocated_boq"),
        "{found:#?}"
    );
    let raw: Vec<_> = found
        .iter()
        .filter(|r| r["kind"] == "raw_recount")
        .collect();
    assert_eq!(raw.len(), 2); // source path length and door insert, not enclosed GFA or graphics
    let length = raw.iter().find(|r| r["unit"] == "m").unwrap();
    assert_eq!(length["comparison_applicable"], false);
    assert_eq!(length["raw_quantity"], 28.);
}
