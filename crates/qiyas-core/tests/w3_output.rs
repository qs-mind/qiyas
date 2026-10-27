//! W3: what Qiyas hands a client. Areas at two decimals with their unit, no internal ids on a
//! sheet, tagged openings, per-kind totals, an itemised BOQ, one export folder, a real sheet at
//! a true scale and a method that does not refuse a short wall. Every unit the app draws in.

use opencadcodec::{EntityType, Handle};
use qiyas_core::check::CheckScope;
use qiyas_core::deliverables::{self, Kind, Language};
use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

#[path = "support/measurement_fixture.rs"]
mod fixture;

const UNITS: [i16; 4] = [4, 5, 6, 1];

fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}
fn added(s: &mut Session, command: Value) -> Vec<u64> {
    match run(s, command) {
        Outcome::Added { handles } => handles,
        other => panic!("expected handles, got {other:?}"),
    }
}
fn text_of(s: &Session, h: u64) -> String {
    s.entity_info(h, &[]).unwrap().text.unwrap()
}
fn findings(s: &Session, rule: &str) -> usize {
    s.check_drawing(&CheckScope {
        rules: vec![rule.into()],
        ..Default::default()
    })
    .counts
    .get(rule)
    .copied()
    .unwrap_or(0)
}

/// A 4 x 3 m room labelled, in this unit: (session, boundary handle).
fn room(units: i16) -> (tempfile::TempDir, Session, u64, f64) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("w3.dwg"), units);
    let u = 1.0 / s.drawing.units.to_metres();
    added(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4.0*u,3.0*u]}),
    );
    let Outcome::Rooms { report } = run(
        &mut s,
        json!({"type":"label_spaces","query":{"seeds":[{"name":"Room A","point":[u,u]}]}}),
    ) else {
        panic!("rooms")
    };
    let boundary = report.rooms[0].boundary_handle.unwrap();
    (dir, s, boundary, u)
}

#[test]
fn an_area_in_text_shows_two_decimals_and_its_unit_in_every_unit() {
    for units in UNITS {
        let (_dir, mut s, boundary, u) = room(units);
        let code = format!(r#"%<\AcVar QiyasObject "{boundary:X}" "net_floor_area">%"#);
        // The plain field a model writes (3 decimals and no unit before W3).
        let h = added(&mut s, json!({"type":"text","at":[0,-u],"value":code}))[0];
        assert_eq!(text_of(&s, h), "12.00 m²", "units {units}");
        // The unit the sentence gives is not repeated.
        let h = added(
            &mut s,
            json!({"type":"text","at":[0,-2.0*u],"value":format!("Floor {code} m²")}),
        )[0];
        assert_eq!(text_of(&s, h), "Floor 12.00 m²");
        // Arabic text: Arabic digits and separator, and the Arabic unit.
        let h = added(
            &mut s,
            json!({"type":"text","at":[0,-3.0*u],"value":format!("المساحة {code}")}),
        )[0];
        assert_eq!(text_of(&s, h), "المساحة ١٢٫٠٠ م²");
        // An explicit precision is kept.
        let three = format!(r#"%<\AcVar QiyasObject "{boundary:X}" "net_floor_area" 3>%"#);
        let h = added(&mut s, json!({"type":"text","at":[0,-4.0*u],"value":three}))[0];
        assert_eq!(text_of(&s, h), "12.000 m²");
        // A perimeter is a length.
        let perimeter = format!(r#"%<\AcVar QiyasObject "{boundary:X}" "boundary_length">%"#);
        let h = added(
            &mut s,
            json!({"type":"text","at":[0,-5.0*u],"value":perimeter}),
        )[0];
        assert_eq!(text_of(&s, h), "14.00 m");
    }
}

#[test]
fn internal_ids_on_a_sheet_are_a_finding_and_a_writer_is_advised() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("ids.dwg"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        added(
            &mut s,
            json!({"type":"text","at":[0,0],"value":"Plan basis: 12.0 x 15.0 m"}),
        );
        assert_eq!(findings(&s, "internal_id_in_text"), 0);
        for (i, bad) in [
            "OPENING SCHEDULE - MEASUREMENT REFERENCES takeoff:34f348f7-1234-4abc-8def-0123456789ab",
            "see boq-row:7",
            "34f348f7-1234-4abc-8def-0123456789ab",
            "handle 1234",
        ]
        .into_iter()
        .enumerate()
        {
            added(
                &mut s,
                json!({"type":"text","at":[0,(i as f64 + 1.0)*2.0*u],"value":bad}),
            );
        }
        assert_eq!(findings(&s, "internal_id_in_text"), 4, "units {units}");
        // What does not plot is not on the sheet.
        s.run_input(
            &json!({"type":"set_layer","name":"0","prop":"plot","value":false}),
            Source::User,
        )
        .ok();
        // A writer through the AI door gets advice at once.
        let reported = s
            .run_input(
                &json!({"type":"text","at":[0,-u],"value":"takeoff:abc"}),
                Source::Ai,
            )
            .unwrap();
        assert!(reported
            .advice
            .iter()
            .any(|a| a["code"] == "internal_id_in_text"));
    }
}

#[test]
fn doors_and_windows_without_a_tag_are_a_finding_until_object_tags_tags_them() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("tags.dwg"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[12.0*k,0]]}],"thickness":0.2*k,
                "openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"},
                            {"run":0,"at":5.0*k,"width":1.2*k,"kind":"window"},
                            {"run":0,"at":8.0*k,"width":0.9*k,"kind":"door"}]}),
        );
        // No schedule yet: nothing to be tagged for.
        assert_eq!(findings(&s, "opening_untagged"), 0);
        run(&mut s, json!({"type":"set_sheet","block":"*Paper_Space"}));
        run(&mut s, json!({"type":"opening_schedule","at":[10,100]}));
        run(&mut s, json!({"type":"set_sheet","block":"*Model_Space"}));
        assert_eq!(findings(&s, "opening_untagged"), 1, "units {units}");
        let report = s.check_drawing(&CheckScope {
            rules: vec!["opening_untagged".into()],
            ..Default::default()
        });
        assert_eq!(report.findings[0].handles.len(), 3);
        assert!(report.findings[0].message.contains("object_tags"));
        let tags = added(
            &mut s,
            json!({"type":"object_tags","category":"opening","property":"mark"}),
        );
        assert_eq!(tags.len(), 3);
        assert_eq!(findings(&s, "opening_untagged"), 0);
        assert!(tags.iter().all(|t| text_of(&s, *t).contains("-")));
    }
}

#[test]
fn the_opening_schedule_totals_each_kind_and_speaks_arabic_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("ar.dwg"), 6);
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[12,0]]}],"thickness":0.2,
            "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"},
                        {"run":0,"at":5,"width":1.2,"kind":"window"},
                        {"run":0,"at":8,"width":0.9,"kind":"door"}]}),
    );
    run(&mut s, json!({"type":"set_sheet","block":"*Paper_Space"}));
    let table = added(
        &mut s,
        json!({"type":"opening_schedule","at":[10,100],"language":"ar"}),
    )[0];
    let EntityType::Table(t) = s.drawing.doc.get_entity(Handle::new(table)).unwrap() else {
        panic!("table")
    };
    let rows: Vec<Vec<String>> = (0..t.rows.len())
        .map(|r| {
            (0..t.columns.len())
                .map(|c| t.cell_text(r, c).unwrap_or("").to_string())
                .collect()
        })
        .collect();
    assert_eq!(
        rows[0],
        ["العلامة", "النوع", "العرض × الارتفاع (مم)", "العدد"]
    );
    assert_eq!(rows[1][1], "باب");
    assert_eq!(rows[1][2], "٩٠٠ × ٢١٠٠");
    assert_eq!(rows[1][3], "٢");
    // One total per kind, never one for doors and windows together.
    let totals: Vec<_> = rows
        .iter()
        .filter(|r| r[0].starts_with("المجموع"))
        .collect();
    assert_eq!(totals.len(), 2, "{rows:?}");
    assert_eq!(totals[0][3], "٢");
    assert_eq!(totals[1][3], "١");
}

#[test]
fn the_boq_totals_only_like_with_like_and_is_not_one_sum_of_different_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("totals.dwg"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("client method", false));
    let mut report = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    // Two different items in the same unit, and a layout that asks for a total.
    let mut second = report.rows[0].clone();
    second["code"] = json!("S-OTHER");
    second["description"] = json!("Another surface");
    report.rows.push(second);
    report.layout.columns.iter_mut().for_each(|c| {
        c.total = c.field == "quantity";
    });
    let table = report.table().unwrap();
    assert!(
        !table
            .iter()
            .any(|r| r.cells.first().is_some_and(|c| c == "Total")),
        "different items are never summed: {table:?}"
    );
    // The shipped BOQ layout asks for no quantity total at all.
    let shipped = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    assert!(shipped.layout.columns.iter().all(|c| !c.total));
}

/// Doors of three sizes in a wall, a static "various sizes" mapping and a gross floor area.
fn doors_and_floor(units: i16) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("boq.dwg"), units);
    let k = 1.0 / s.drawing.units.to_metres();
    run(
        &mut s,
        json!({"type":"walls","runs":[{"points":[[0,0],[20.0*k,0]]}],"thickness":0.2*k,
            "openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"},
                        {"run":0,"at":5.0*k,"width":0.9*k,"kind":"door"},
                        {"run":0,"at":8.0*k,"width":1.0*k,"kind":"door"},
                        {"run":0,"at":12.0*k,"width":1.2*k,"kind":"window"}]}),
    );
    let cite = json!({"source":"Test method","clause":"1"});
    let count = |id: &str, element: &str| {
        json!({"id":id,"kind":"count","basis":"geometric","element_type":element,"unit":"pcs",
            "terms":[{"label":"Count","operation":"add","factors":["pieces"],"citation":cite}],
            "citation":cite})
    };
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"doors","code":"A-D01","description_en":"Doors, various sizes","description_ar":"أبواب بمقاسات متنوعة","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
            {"rule":"windows","code":"A-W01","description_en":"Windows, various sizes","description_ar":"شبابيك بمقاسات متنوعة","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
    );
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
            "version":1,"name":"Test method","title":"Test","based_on":"test","source_note":"test",
            "rules":[count("doors","Legacy door"),count("windows","Legacy window")]}}),
    );
    (dir, s)
}

#[test]
fn the_boq_lists_doors_and_windows_by_size_even_when_the_mapping_says_various() {
    for units in UNITS {
        let (_dir, s) = doors_and_floor(units);
        let report = s.measurement_report().unwrap();
        let mut doors: Vec<(String, f64)> = report
            .items
            .iter()
            .filter(|i| i.code.starts_with("A-D01"))
            .map(|i| (i.description_en.clone(), i.quantity))
            .collect();
        doors.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            doors,
            [
                ("Doors, various sizes — 1000 × 2100 mm".to_string(), 1.0),
                ("Doors, various sizes — 900 × 2100 mm".to_string(), 2.0)
            ],
            "units {units}"
        );
        let windows: Vec<_> = report
            .items
            .iter()
            .filter(|i| i.code.starts_with("A-W01"))
            .collect();
        assert_eq!(windows.len(), 1);
        assert!(windows[0].description_ar.contains("1200 × 1200 mm"));
        // The printed BOQ has a section row per section and no total across doors and windows.
        let boq = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
        let table = boq.table().unwrap();
        assert!(table.iter().any(|r| r.section && r.cells[0] == "Openings"));
        assert!(!table.iter().any(|r| r.cells[0] == "Total"));
    }
}

#[test]
fn a_mapping_that_already_says_the_size_is_left_as_it_is() {
    let (_dir, mut s) = doors_and_floor(6);
    let cite = json!({"source":"Test method","clause":"1"});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"doors","code":"A-D-{width}","description_en":"Door {width} m wide","description_ar":"باب {width}","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
            {"rule":"windows","code":"A-W01","description_en":"Windows","description_ar":"شبابيك","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
    );
    let report = s.measurement_report().unwrap();
    let mut doors: Vec<_> = report
        .items
        .iter()
        .filter(|i| i.code.starts_with("A-D-"))
        .map(|i| i.code.clone())
        .collect();
    doors.sort();
    assert_eq!(doors, ["A-D-0.9", "A-D-1"]);
}

#[test]
fn a_gross_floor_area_is_information_not_a_boq_item() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("gfa.dwg"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("client method", false));
    let cite = json!({"source":"Test","clause":"1"});
    run(
        &mut s,
        json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"surface","code":"GFA-1","description_en":"Gross floor area (GFA)","description_ar":"إجمالي المسطح","section":"Informational","trade":"","unit":"{unit}","citation":cite}]}),
    );
    let measured = s.measurement_report().unwrap();
    assert!(measured.items.iter().all(|i| i.informational));
    assert!(measured
        .warnings
        .iter()
        .any(|w| w.contains("informational")));
    let boq = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    assert!(boq.rows.is_empty(), "left out of the BOQ: {:?}", boq.rows);
    let dims = deliverables::drawing(&s, Kind::Dimensions, Language::En, None).unwrap();
    assert!(!dims.rows.is_empty(), "the dimension sheets keep it");
    assert!(measured.comparison_measurements(s.drawing.units).is_empty());
}

#[test]
fn an_arabic_boq_leaves_out_english_method_and_source_columns() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("lang.dwg"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("client method", false));
    let arabic = deliverables::drawing(&s, Kind::Boq, Language::Ar, None).unwrap();
    assert!(arabic
        .layout
        .columns
        .iter()
        .all(|c| c.field != "method" && c.field != "citations"));
    assert!(arabic
        .warnings
        .iter()
        .any(|w| w.contains("not in the report's language")));
    let english = deliverables::drawing(&s, Kind::Boq, Language::En, None).unwrap();
    assert!(english.layout.columns.iter().any(|c| c.field == "method"));
    // The dimension sheets keep the full citations in both languages.
    let dims = deliverables::drawing(&s, Kind::Dimensions, Language::Ar, None).unwrap();
    assert!(dims.layout.columns.iter().any(|c| c.field == "citations"));
}

#[test]
fn deductions_beyond_a_short_walls_gross_limit_it_to_nothing_and_say_why() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("short.dwg"), units);
        let (host, _) = fixture::fixture(&mut s);
        let u = 1.0 / s.drawing.units.to_metres();
        // A 0.3 m stub (gross 3 x 0.3 x 2.8 = 2.52) with a 1.2 x 3.0 opening (3.6).
        let stub = added(
            &mut s,
            json!({"type":"place_objects","objects":[{"type":"Surface","points":[[10.0*u,0],[10.3*u,0]]}]}),
        )[0];
        added(
            &mut s,
            json!({"type":"place_objects","objects":[{"type":"Void","host":stub,"points":[[10.1*u,0]],"props":{"width":1.2,"rise":3.0}}]}),
        );
        let _ = host;
        fixture::apply(&mut s, fixture::method("client method", false));
        let report = s.measurement_report().unwrap();
        let q = report
            .quantities
            .iter()
            .find(|q| q.object == stub)
            .expect("the stub is measured");
        assert_eq!(q.quantity, 0.0, "units {units}");
        assert!(q.lines.iter().any(|l| l.label.contains("limited")));
        let warning = report
            .warnings
            .iter()
            .find(|w| w.contains(&stub.to_string()))
            .unwrap();
        assert!(
            warning.contains("gross_volume") && warning.contains("exceed"),
            "{warning}"
        );
    }
}

#[test]
fn the_same_enclosure_seeded_twice_is_one_retained_room_and_never_plots() {
    for units in UNITS {
        let (_dir, mut s, boundary, u) = room(units);
        let Outcome::Rooms { report } = run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[{"name":"Another name","point":[2.0*u,2.0*u]}]}}),
        ) else {
            panic!("rooms")
        };
        assert_eq!(report.rooms[0].boundary_handle, Some(boundary));
        let polylines = s
            .drawing
            .doc
            .entities()
            .filter(|e| e.common().layer == "A-ROOM-BOUNDARY")
            .count();
        assert_eq!(polylines, 1, "units {units}");
        assert!(
            !s.drawing
                .doc
                .layers
                .get("A-ROOM-BOUNDARY")
                .unwrap()
                .is_plottable
        );
        assert_eq!(findings(&s, "duplicate"), 0);
    }
}

/// Text on a layout, as (value, height in paper mm).
fn layout_texts(s: &Session, block: &str) -> Vec<(String, f64)> {
    s.drawing
        .doc
        .entities_in_block(block)
        .filter_map(|e| match e {
            EntityType::MText(t) => Some((t.value.clone(), t.height)),
            EntityType::Text(t) => Some((t.value.clone(), t.height)),
            _ => None,
        })
        .collect()
}

#[test]
fn make_sheet_makes_a_real_sheet_at_a_true_scale_in_every_unit() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("sheet.dwg"), units);
        let u = 1.0 / s.drawing.units.to_metres();
        added(
            &mut s,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[12.0*u,15.0*u]}),
        );
        let before = s.history_steps().0.len();
        let Outcome::Sheet { report } = run(
            &mut s,
            json!({"type":"make_sheet","name":"A-101","project":"Villa","title":"Ground floor plan",
                "drawing_no":"A-101","date":"2026-10-27","notes":["All dimensions in metres.","Assumptions are for review."]}),
        ) else {
            panic!("sheet")
        };
        assert_eq!(s.history_steps().0.len(), before + 1, "one undo step");
        // Metric drawings get the smallest ISO sheet that holds them; inches get ANSI.
        let (paper_name, den, paper_w, paper_h) = if units == 1 {
            ("ANSI B", 75.0, 279.4, 431.8)
        } else {
            ("ISO A3", 50.0, 297.0, 420.0)
        };
        assert_eq!(report.paper, paper_name, "units {units}");
        assert_eq!(report.scale, format!("1:{den}"));
        assert!(report.notes_fit);
        assert!(report.empty_fields.is_empty(), "{:?}", report.empty_fields);
        // Text that reads on paper: 2.5 mm at least; 3.5 mm or more for titles.
        let block = s
            .drawing
            .sheets()
            .into_iter()
            .find(|x| x.name == "A-101")
            .unwrap()
            .block;
        let texts = layout_texts(&s, &block);
        for (value, height) in &texts {
            assert!(*height >= 2.5, "{value} is {height} mm");
        }
        let all: String = texts
            .iter()
            .map(|t| t.0.as_str())
            .collect::<Vec<_>>()
            .join("|");
        for must in [
            "Ground floor plan",
            "Villa",
            "A-101",
            "Qiyas",
            "For review",
            "P01",
            &report.scale,
            "2026-10-27",
            "Scale",
            "General notes",
        ] {
            assert!(all.contains(must), "{must} missing in {all}");
        }
        assert!(texts
            .iter()
            .any(|(v, h)| v.contains("Ground floor plan") && *h >= 3.5));
        // The viewport is at exactly the scale (paper mm per model unit).
        let view = s.paper_views_of(&block);
        assert_eq!(view.len(), 1);
        let expected = 1000.0 * s.drawing.units.to_metres() / den;
        assert!(
            (view[0].scale - expected).abs() < 1e-9 * expected.max(1.0),
            "{} vs {expected}",
            view[0].scale
        );
        // It sits inside the frame and clear of the title strip.
        let [[ax, ay], [bx, by]] = view[0].rect;
        assert!(ax >= 20.0 && bx <= paper_w - 10.0 && ay >= 10.0 + 78.0 && by <= paper_h - 10.0);
        // The model's 12 x 15 m on paper at that scale.
        assert!(
            ((bx - ax) - 12000.0 / den).abs() < 1e-6 && ((by - ay) - 15000.0 / den).abs() < 1e-6
        );
        assert!(report.min_text_height > 0.0);
        // The sheet checks completely, so schedules can be placed on it without a position.
        let checked = s.check_drawing(&CheckScope {
            sheet: Some(block.clone()),
            ..Default::default()
        });
        assert!(
            checked.complete,
            "units {units}: not checked {:?}",
            checked.not_checked
        );
        assert!(
            s.free_space(&qiyas_core::placement::SpaceQuery {
                sheet: Some(block.clone()),
                size: Some([20.0, 10.0]),
                ..Default::default()
            })
            .is_ok(),
            "units {units}"
        );
        // A name already taken is refused, not drawn on twice.
        let again = s.run_input(
            &json!({"type":"make_sheet","name":"a-101","title":"Again"}),
            Source::User,
        );
        assert!(again
            .unwrap_err()
            .to_string()
            .contains("already has a sheet"));
        // Undo takes the whole sheet away.
        assert!(s.undo());
        assert!(!s.drawing.sheets().iter().any(|x| x.name == "A-101"));
    }
}

#[test]
fn make_sheet_speaks_the_requested_language_and_takes_the_user_language_by_default() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("ar-sheet.dwg"), 6);
    added(
        &mut s,
        json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[12,15]}),
    );
    s.set_sheet_language(Some(qiyas_core::rooms::LabelLanguage::Ar));
    let Outcome::Sheet { report } = run(
        &mut s,
        json!({"type":"make_sheet","name":"A-101","title":"المسقط الأفقي","notes":["الأبعاد بالمتر."],"date":"2026-10-27"}),
    ) else {
        panic!("sheet")
    };
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|x| x.name == "A-101")
        .unwrap()
        .block;
    let all: String = layout_texts(&s, &block)
        .into_iter()
        .map(|t| t.0)
        .collect::<Vec<_>>()
        .join("|");
    for must in [
        "المشروع",
        "رقم اللوحة",
        "للمراجعة",
        "قياس",
        "مقياس الرسم",
        "ملاحظات عامة",
        "١:٥٠",
        "٢٠٢٦-١٠-٢٧",
    ] {
        assert!(all.contains(must), "{must} missing in {all}");
    }
    assert!(!all.contains("For review") && !all.contains("Scale"));
    assert_eq!(report.language, qiyas_core::rooms::LabelLanguage::Ar);
    // Fields nobody gave are listed, never invented.
    assert_eq!(report.empty_fields, ["project", "drawing_no"]);
}

#[test]
fn tags_and_room_labels_live_together_on_a_walled_plan_in_every_unit() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("plan.dwg"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,"height":3.0,
                "runs":[{"points":[[0,0],[12.0*k,0],[12.0*k,15.0*k],[0,15.0*k]],"closed":true},
                        {"points":[[0,7.5*k],[12.0*k,7.5*k]]}],
                "openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"},
                            {"run":0,"at":6.0*k,"width":1.2*k,"kind":"window"},
                            {"run":0,"at":9.0*k,"width":1.0*k,"kind":"door"},
                            {"run":0,"at":18.0*k,"width":1.2*k,"kind":"window"},
                            {"run":1,"at":3.0*k,"width":0.9*k,"kind":"door"}]}),
        );
        s.set_sheet_language(Some(qiyas_core::rooms::LabelLanguage::Ar));
        run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[
                {"name":"Living","point":[3.0*k,3.0*k]},{"name":"Bedroom","point":[3.0*k,10.0*k]}]}}),
        );
        let tags = added(
            &mut s,
            json!({"type":"object_tags","category":"opening","property":"mark"}),
        );
        assert_eq!(tags.len(), 5, "units {units}");
        assert_eq!(findings(&s, "label_outside_room"), 0);
    }
}

#[test]
fn an_arabic_room_schedule_has_arabic_digits_and_its_unit_only_in_the_heading() {
    for units in UNITS {
        let (_dir, mut s, _boundary, _u) = room(units);
        s.set_sheet_language(Some(qiyas_core::rooms::LabelLanguage::Ar));
        run(&mut s, json!({"type":"set_sheet","block":"*Paper_Space"}));
        let table = added(&mut s, json!({"type":"room_schedule"}))[0];
        let EntityType::Table(t) = s.drawing.doc.get_entity(Handle::new(table)).unwrap() else {
            panic!("table")
        };
        assert_eq!(t.cell_text(0, 1), Some("المساحة (م²)"), "units {units}");
        assert_eq!(t.cell_text(1, 1), Some("١٢٫٠٠"), "units {units}");
        assert_eq!(t.cell_text(2, 1), Some("١٢٫٠٠"));
    }
}

#[test]
fn schedules_placed_by_themselves_on_a_sheet_stay_on_the_paper_and_clear_of_the_view() {
    for units in UNITS {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("sched.dwg"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        run(
            &mut s,
            json!({"type":"walls","thickness":0.2*k,"height":3.0,
                "runs":[{"points":[[0,0],[12.0*k,0],[12.0*k,15.0*k],[0,15.0*k]],"closed":true}],
                "openings":[{"run":0,"at":2.0*k,"width":0.9*k,"kind":"door"},
                            {"run":0,"at":6.0*k,"width":1.2*k,"kind":"window"}]}),
        );
        run(
            &mut s,
            json!({"type":"label_spaces","query":{"seeds":[{"name":"Hall","point":[3.0*k,3.0*k]}]}}),
        );
        run(&mut s, json!({"type":"make_sheet","name":"A-101","title":"Plan"}));
        let block = s.drawing.sheets().into_iter().find(|x| x.name == "A-101").unwrap().block;
        run(&mut s, json!({"type":"set_sheet","block":block.clone()}));
        let a = added(&mut s, json!({"type":"opening_schedule"}))[0];
        let b = added(&mut s, json!({"type":"room_schedule"}))[0];
        let view = s.paper_views_of(&block)[0].rect;
        let rows = s
            .find_entities(&qiyas_core::EntityFilter {
                kind: Some("Table".into()),
                sheet: Some(block.clone()),
                ..Default::default()
            })
            .rows;
        assert_eq!(rows.len(), 2);
        let paper = s.paper().unwrap();
        let mut seen = Vec::new();
        for row in rows {
            assert!([a, b].contains(&row.handle));
            let t = row.bounds;
            assert!(t[0][0] >= 20.0 && t[1][0] <= paper.w_mm - 10.0, "units {units}: {t:?}");
            assert!(t[0][1] >= 10.0 && t[1][1] <= paper.h_mm - 10.0, "units {units}: {t:?}");
            let apart = |o: [[f64; 2]; 2]| {
                t[1][0] <= o[0][0] || o[1][0] <= t[0][0] || t[1][1] <= o[0][1] || o[1][1] <= t[0][1]
            };
            assert!(apart(view), "units {units}: table {t:?} over the view {view:?}");
            for other in &seen {
                assert!(apart(*other), "tables overlap: {t:?} {other:?}");
            }
            seen.push(t);
        }
    }
}
