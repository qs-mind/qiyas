//! `Command::CheckDrawing`: every rule, sheet and handle scopes, and a clean drawing.
//! Line faults, overlaps, dimensions and text scale are also checked in mm, cm, m and inches.

use opencadcodec::{Arc, Color, EntityType, Handle, Line, MText, Vector3};
use qiyas_core::check::{CheckReport, CheckScope, Severity};
use qiyas_core::{
    Command, DimEditOp, DimSpec, DrawSpec, Kind, Measurement, Outcome, Session, Target,
};

/// A blank drawing in `units` (6 metres, 5 centimetres, 4 millimetres, 1 inches) and the drawing units per
/// metre.
fn blank(units: i16) -> (Session, f64) {
    let dir = std::env::temp_dir().join("qiyas-check-test");
    let s = Session::new_blank(&dir.join("Untitled-1.dwg"), units);
    let per_m = 1.0 / s.drawing.units.to_metres();
    (s, per_m)
}

fn run(s: &mut Session, c: Command) -> Vec<u64> {
    match s.run(c).unwrap() {
        Outcome::Added { handles } => handles,
        other => panic!("{other:?}"),
    }
}

fn line(s: &mut Session, k: f64, a: [f64; 2], b: [f64; 2]) -> u64 {
    run(
        s,
        Command::Draw(DrawSpec::Line {
            a: [a[0] * k, a[1] * k],
            b: [b[0] * k, b[1] * k],
        }),
    )[0]
}

fn check(s: &Session) -> CheckReport {
    s.check_drawing(&CheckScope::default())
}

fn rules(r: &CheckReport) -> Vec<&str> {
    r.findings.iter().map(|f| f.rule).collect()
}

#[test]
fn a_closed_room_on_its_own_layer_is_clean() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        s.run(Command::NewLayer {
            name: "A-WALL".into(),
        })
        .unwrap();
        let mut current = s.current.clone();
        current.layer = "A-WALL".into();
        s.run(Command::SetCurrent { current }).unwrap();
        run(
            &mut s,
            Command::Draw(DrawSpec::Rectangle {
                a: [0.0, 0.0],
                b: [5.0 * k, 4.0 * k],
            }),
        );
        let r = check(&s);
        assert!(r.findings.is_empty(), "{units}: {:?}", r.findings);
        assert_eq!(r.line, "No findings");
    }
}

#[test]
fn line_work_faults_are_found_in_every_unit() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let wall = line(&mut s, k, [0.0, 0.0], [5.0, 0.0]);
        // Stops 0.1 m short of the wall.
        let short = line(&mut s, k, [2.0, 3.0], [2.0, 0.1]);
        // Runs 0.05 m past it.
        let over = line(&mut s, k, [4.0, 3.0], [4.0, -0.05]);
        // Drawn twice.
        let twice = line(&mut s, k, [0.0, 5.0], [5.0, 5.0]);
        let again = line(&mut s, k, [5.0, 5.0], [0.0, 5.0]);
        // A scrap.
        let scrap = line(&mut s, k, [8.0, 8.0], [8.0005, 8.0]);
        let r = check(&s);
        let found = |rule: &str, h: u64| {
            r.findings
                .iter()
                .any(|f| f.rule == rule && f.handles.contains(&h))
        };
        assert!(found("dangling_end", short), "{units}: {:?}", rules(&r));
        assert!(found("overshoot", over), "{units}: {:?}", rules(&r));
        assert!(
            found("duplicate", twice) && found("duplicate", again),
            "{units}"
        );
        assert!(found("tiny_segment", scrap), "{units}");
        assert!(found("layer_zero", wall), "{units}");
        // Only what a change touched.
        let touched = s.check_drawing(&CheckScope {
            handles: vec![short],
            ..CheckScope::default()
        });
        assert!(touched.findings.iter().all(|f| f.handles.contains(&short)));
        assert!(!touched.findings.is_empty());
    }
}

#[test]
fn parallel_lines_and_curve_tangencies_are_clean_but_real_gaps_and_overlaps_remain() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        line(&mut s, k, [0.0, 0.0], [5.0, 0.0]);
        line(&mut s, k, [0.0, 0.1], [5.0, 0.1]);
        line(&mut s, k, [0.0, 5.0], [5.0, 5.0]);
        run(
            &mut s,
            Command::Draw(DrawSpec::Circle {
                centre: [2.5 * k, 6.0 * k],
                radius: k,
            }),
        );
        let report = s.check_drawing(&CheckScope {
            rules: vec!["overlap".into(), "dangling_end".into()],
            ..Default::default()
        });
        assert!(report.findings.is_empty(), "{units}: {:?}", report.findings);
        // Two collinear lines facing each other across a gap still stop short.
        let gap = line(&mut s, k, [5.1, 0.0], [8.0, 0.0]);
        let overlap = line(&mut s, k, [1.0, 0.0], [3.0, 0.0]);
        let report = check(&s);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == "dangling_end" && f.handles.contains(&gap)),
            "{units}: {:?}",
            report.findings
        );
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == "overlap" && f.handles.contains(&overlap)),
            "{units}: {:?}",
            report.findings
        );
        assert!(report
            .findings
            .iter()
            .filter(|f| f.rule == "layer_zero")
            .all(|f| f.severity == Severity::Warning));
    }
}

#[test]
fn dimension_extensions_are_valid_but_a_changed_overrun_is_still_reported() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let added = run(
            &mut s,
            Command::Dimension(DimSpec::Linear {
                p1: [0.0, 0.0],
                p2: [5.0 * k, 0.0],
                line_point: [2.5 * k, -k],
                rotation: None,
            }),
        );
        let block = added
            .iter()
            .find_map(
                |&handle| match s.drawing.doc.get_entity(Handle::new(handle)) {
                    Some(EntityType::Dimension(d)) => Some(d.base().block_name.clone()),
                    _ => None,
                },
            )
            .unwrap();
        let scope = CheckScope {
            sheet: Some(block.clone()),
            rules: vec!["overshoot".into()],
            ..Default::default()
        };
        assert!(s.check_drawing(&scope).findings.is_empty(), "{units}");
        let handle = s
            .drawing
            .entities_in(&block)
            .find_map(|entity| match entity {
                EntityType::Line(line) if line.end.x == 0.0 && line.end.y < -k => {
                    Some(line.common.handle)
                }
                _ => None,
            })
            .unwrap();
        let Some(EntityType::Line(line)) = s.drawing.doc.get_entity_mut(handle) else {
            panic!("extension line")
        };
        line.end.y -= 0.03 * k;
        let report = s.check_drawing(&scope);
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.rule == "overshoot" && f.handles.contains(&handle.value())),
            "{units}: {:?}",
            report.findings
        );
    }
}

#[test]
fn rotated_text_checks_its_actual_box_and_paper_checks_use_paper_units() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        line(&mut s, k, [0.0, 0.0], [3.0, 3.0]);
        run(
            &mut s,
            Command::Text {
                at: [k, 1.25 * k],
                height: 0.25 * k,
                rotation: 45.0,
                value: "ROTATED".into(),
                style: None,
            },
        );
        let scope = CheckScope {
            rules: vec!["text_over_lines".into()],
            ..Default::default()
        };
        assert!(s.check_drawing(&scope).findings.is_empty(), "{units}");
        let crossing = line(&mut s, k, [1.0, 1.5], [3.0, 3.5]);
        assert!(
            s.check_drawing(&scope)
                .findings
                .iter()
                .any(|f| f.handles.contains(&crossing)),
            "{units}"
        );

        s.run(
            serde_json::from_value(
                serde_json::json!({"type":"new_layout","name":"Paper","paper":"ISO A1"}),
            )
            .unwrap(),
        )
        .unwrap();
        let paper = s
            .drawing
            .sheets()
            .into_iter()
            .find(|sheet| sheet.name == "Paper")
            .unwrap();
        s.run(Command::SetSheet { block: paper.block }).unwrap();
        let short = line(&mut s, 1.0, [0.0, 0.0], [0.5, 0.0]);
        let report = s.check_drawing(&CheckScope {
            rules: vec!["tiny_segment".into()],
            ..Default::default()
        });
        assert!(
            report.findings.iter().any(|f| f.handles == [short]),
            "{units}: {:?}",
            report.findings
        );
    }
}

#[test]
fn text_faults_are_found() {
    let (mut s, k) = blank(6);
    s.run(Command::AnnotationScale {
        name: "1:100".into(),
    })
    .unwrap();
    // 2.5 mm at 1:100 is 0.25 m: fine. 0.02 m is 0.2 mm on paper: too small.
    let tiny = run(
        &mut s,
        Command::Text {
            at: [0.0, 10.0 * k],
            height: 0.02,
            rotation: 0.0,
            value: "NOTE".into(),
            style: None,
        },
    )[0];
    let a = run(
        &mut s,
        Command::Text {
            at: [20.0, 20.0],
            height: 0.25,
            rotation: 0.0,
            value: "KITCHEN".into(),
            style: None,
        },
    )[0];
    let b = run(
        &mut s,
        Command::Text {
            at: [20.1, 20.05],
            height: 0.25,
            rotation: 0.0,
            value: "KITCHEN".into(),
            style: None,
        },
    )[0];
    let r = check(&s);
    let found = |rule: &str, h: u64| {
        r.findings
            .iter()
            .any(|f| f.rule == rule && f.handles.contains(&h))
    };
    assert!(found("text_height_vs_scale", tiny), "{:?}", rules(&r));
    assert!(
        found("text_overlaps_text", a) && found("text_overlaps_text", b),
        "{:?}",
        rules(&r)
    );
}

#[test]
fn a_room_label_that_disagrees_with_its_room_and_an_open_room_are_found() {
    let (mut s, k) = blank(6);
    s.run(Command::NewLayer {
        name: "A-ROOM-NAME".into(),
    })
    .unwrap();
    // A closed 4 x 3 room labelled 5.25 x 3.25.
    run(
        &mut s,
        Command::Draw(DrawSpec::Rectangle {
            a: [0.0, 0.0],
            b: [4.0 * k, 3.0 * k],
        }),
    );
    let mut on_names = s.current.clone();
    on_names.layer = "A-ROOM-NAME".into();
    s.run(Command::SetCurrent { current: on_names }).unwrap();
    let wrong = run(
        &mut s,
        Command::Text {
            at: [1.0, 1.5],
            height: 0.2,
            rotation: 0.0,
            value: "BED 5.25m x 3.25m".into(),
            style: None,
        },
    )[0];
    // A room with a gap: three sides only.
    let mut c = s.current.clone();
    c.layer = "0".into();
    s.run(Command::SetCurrent { current: c }).unwrap();
    for (a, b) in [
        ([10.0, 0.0], [14.0, 0.0]),
        ([14.0, 0.0], [14.0, 3.0]),
        ([14.0, 3.0], [10.0, 3.0]),
    ] {
        line(&mut s, k, a, b);
    }
    let mut on_names = s.current.clone();
    on_names.layer = "A-ROOM-NAME".into();
    s.run(Command::SetCurrent { current: on_names }).unwrap();
    let open = run(
        &mut s,
        Command::Text {
            at: [11.0, 1.5],
            height: 0.2,
            rotation: 0.0,
            value: "STORE".into(),
            style: None,
        },
    )[0];
    let r = check(&s);
    let found = |rule: &str, h: u64| {
        r.findings
            .iter()
            .any(|f| f.rule == rule && f.handles.contains(&h))
    };
    assert!(found("room_size_text_differs", wrong), "{:?}", r.findings);
    assert!(found("room_open", open), "{:?}", r.findings);
}

#[test]
fn literal_newlines_and_far_off_entities_are_found() {
    let (mut s, k) = blank(6);
    for i in 0..12 {
        line(&mut s, k, [i as f64, 0.0], [i as f64, 1.0]);
    }
    let far = line(&mut s, k, [1e6, 1e6], [1e6 + 1.0, 1e6]);
    let r = check(&s);
    assert!(
        r.findings
            .iter()
            .any(|f| f.rule == "far_off_extents" && f.handles.contains(&far)),
        "{:?}",
        rules(&r)
    );
    // A literal \n read from another program's file.
    let owner = s
        .drawing
        .doc
        .block_records
        .get("*Model_Space")
        .unwrap()
        .handle;
    let mut text = MText::with_value("BED\\nROOM".to_string(), Vector3::new(0.0, 5.0, 0.0));
    text.common.owner_handle = owner;
    let h = s
        .drawing
        .doc
        .add_entity(EntityType::MText(text))
        .unwrap()
        .value();
    let r = check(&s);
    assert!(
        r.findings
            .iter()
            .any(|f| f.rule == "literal_newline" && f.handles.contains(&h)),
        "{:?}",
        rules(&r)
    );
}

#[test]
fn the_check_line_stays_short_and_counts_every_rule() {
    let (mut s, k) = blank(6);
    for i in 0..300 {
        let x = i as f64;
        line(&mut s, k, [x, 0.0], [x, 1.0]);
        line(&mut s, k, [x, 1.0], [x, 0.0]);
    }
    let r = check(&s);
    assert!(r.line.chars().count() < 200, "{}", r.line);
    assert!(r.findings.len() <= 200);
    assert_eq!(r.counts["duplicate"], 300);
}

#[test]
fn overlaps_near_the_end_of_a_long_segment_are_found() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let a = line(&mut s, k, [0.0, 0.0], [20.0, 0.0]);
        let b = line(&mut s, k, [18.0, 0.0], [22.0, 0.0]);
        let r = s.check_drawing(&CheckScope {
            rules: vec!["overlap".into()],
            ..Default::default()
        });
        assert_eq!(rules(&r), ["overlap"], "{units}: {:?}", r.findings);
        assert_eq!(r.findings[0].handles, [a, b]);
    }
}

#[test]
fn each_rule_filter_keeps_only_its_own_findings() {
    let (mut s, k) = blank(6);
    line(&mut s, k, [0.0, 0.0], [5.0, 0.0]);
    line(&mut s, k, [0.0, 0.0], [5.0, 0.0]);
    line(&mut s, k, [2.0, 3.0], [2.0, 0.1]);
    line(&mut s, k, [4.0, 3.0], [4.0, -0.05]);
    for rule in ["duplicate", "overlap", "dangling_end", "overshoot"] {
        let r = s.check_drawing(&CheckScope {
            rules: vec![rule.into()],
            ..Default::default()
        });
        assert!(
            r.findings.iter().all(|f| f.rule == rule),
            "{rule}: {:?}",
            r.findings
        );
    }
}

#[test]
fn text_checks_find_text_after_many_lines_and_across_its_whole_width() {
    let (mut s, k) = blank(6);
    for i in 0..550 {
        line(&mut s, k, [i as f64, -5.0], [i as f64, -4.0]);
    }
    let t = run(
        &mut s,
        Command::Text {
            at: [0.0, 0.0],
            height: 0.25,
            rotation: 0.0,
            value: "A long annotation across the drawing".into(),
            style: None,
        },
    )[0];
    let bounds = s
        .find_entities(&qiyas_core::EntityFilter {
            kind: Some("Text".into()),
            ..Default::default()
        })
        .rows[0]
        .bounds;
    let x = bounds[0][0] + (bounds[1][0] - bounds[0][0]) * 0.3;
    let crossing = line(&mut s, k, [x, -1.0], [x, 1.0]);
    let r = s.check_drawing(&CheckScope {
        rules: vec!["text_over_lines".into()],
        ..Default::default()
    });
    assert!(
        r.findings
            .iter()
            .any(|f| f.handles.contains(&t) && f.handles.contains(&crossing)),
        "{:?}",
        r.findings
    );
}

#[test]
fn colours_and_layer_zero_can_be_checked_after_the_first_fifty_entities() {
    let (mut s, k) = blank(6);
    let mut last = 0;
    for i in 0..60 {
        last = line(&mut s, k, [i as f64, 0.0], [i as f64, 1.0]);
        s.drawing
            .doc
            .get_entity_mut(Handle::new(last))
            .unwrap()
            .common_mut()
            .color = Color::Index(1);
    }
    let r = s.check_drawing(&CheckScope {
        handles: vec![last],
        rules: vec!["layer_zero".into(), "colour_not_bylayer".into()],
        ..Default::default()
    });
    assert_eq!(rules(&r), ["layer_zero", "colour_not_bylayer"]);
    assert!(r.findings.iter().all(|f| f.handles.contains(&last)));
}

#[test]
fn a_dimension_override_must_agree_with_its_measurement() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let h = run(
            &mut s,
            Command::Dimension(DimSpec::Linear {
                p1: [0.0, 0.0],
                p2: [5.0 * k, 0.0],
                line_point: [2.5 * k, k],
                rotation: None,
            }),
        )[0];
        for (value, mismatch) in [
            ("99.0", true),
            ("5.00 m", false),
            ("500 m", true),
            ("500 cm", false),
            ("196.8504 in", false),
            ("<> TYP 2 PLACES", false),
        ] {
            s.run(Command::DimEdit {
                target: Target {
                    handle: h,
                    chain: vec![],
                },
                op: DimEditOp::TextOverride(value.into()),
            })
            .unwrap();
            let r = s.check_drawing(&CheckScope {
                rules: vec!["dim_override_mismatch".into()],
                ..Default::default()
            });
            assert_eq!(
                !r.findings.is_empty(),
                mismatch,
                "{units}, {value}: {:?}",
                r.findings
            );
        }
    }
}

#[test]
fn takeoff_with_a_missing_entity_is_reported_on_its_own_sheet() {
    let (mut s, _) = blank(6);
    let mut measurement = Measurement::new(
        Kind::Length,
        1.0,
        s.drawing.units,
        "Wall length".into(),
        "A-WALL".into(),
        s.sheet.clone(),
    );
    measurement.handles = vec![u64::MAX];
    s.drawing.doc.add_layout("Plan").unwrap();
    let sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sheet| sheet.name == "Plan")
        .unwrap()
        .block;
    let mut other = measurement.clone();
    other.sheet = sheet;
    other.handles = vec![u64::MAX - 1];
    s.takeoff.items.extend([measurement, other]);
    let r = s.check_drawing(&CheckScope {
        rules: vec!["unlinked_takeoff".into()],
        ..Default::default()
    });
    assert_eq!(rules(&r), ["unlinked_takeoff"]);
    assert_eq!(r.findings[0].handles, [u64::MAX]);
    let r = s.check_drawing(&CheckScope {
        sheet: Some("Plan".into()),
        handles: vec![u64::MAX - 1],
        rules: vec!["unlinked_takeoff".into()],
        ..Default::default()
    });
    assert_eq!(rules(&r), ["unlinked_takeoff"]);
    assert_eq!(r.findings[0].handles, [u64::MAX - 1]);
}

#[test]
fn construction_checks_are_opt_in_until_the_drawing_has_a_construction_cue() {
    for layer in ["A-ROOM-NAME", "Spaces", "floor_areas"] {
        let (mut s, _) = blank(6);
        let h = run(
            &mut s,
            Command::Text {
                at: [0.0, 0.0],
                height: 0.2,
                rotation: 0.0,
                value: "5.25m x 3.25m".into(),
                style: None,
            },
        )[0];
        let scope = CheckScope {
            rules: vec!["room_open".into()],
            ..Default::default()
        };
        assert!(s.check_drawing(&scope).findings.is_empty());
        let enabled = CheckScope {
            packs: vec!["construction".into()],
            ..scope.clone()
        };
        let r = s.check_drawing(&enabled);
        assert_eq!(rules(&r), ["room_open"]);
        assert_eq!(r.findings[0].pack, "construction");
        assert_eq!(r.findings[0].handles, [h]);

        s.run(Command::NewLayer { name: layer.into() }).unwrap();
        assert_eq!(rules(&s.check_drawing(&scope)), ["room_open"], "{layer}");
        let general = CheckScope {
            packs: vec!["general".into()],
            ..scope
        };
        assert!(s.check_drawing(&general).findings.is_empty(), "{layer}");
    }
}

#[test]
fn takeoff_rows_enable_construction_but_an_explicit_pack_still_controls_it() {
    let (mut s, _) = blank(6);
    let h = line(&mut s, 1.0, [0.0, 0.0], [1.0, 0.0]);
    let mut measurement = Measurement::new(
        Kind::Length,
        1.0,
        s.drawing.units,
        "Length".into(),
        "0".into(),
        s.sheet.clone(),
    );
    measurement.handles = vec![u64::MAX];
    s.takeoff.items.push(measurement);
    let r = check(&s);
    assert!(r
        .findings
        .iter()
        .any(|f| f.rule == "unlinked_takeoff" && f.pack == "construction"));
    assert!(r
        .findings
        .iter()
        .any(|f| f.rule == "layer_zero" && f.pack == "general" && f.handles == [h]));

    let general = s.check_drawing(&CheckScope {
        packs: vec!["general".into()],
        ..Default::default()
    });
    assert_eq!(rules(&general), ["layer_zero"]);
    let construction = s.check_drawing(&CheckScope {
        packs: vec!["construction".into()],
        ..Default::default()
    });
    assert_eq!(rules(&construction), ["unlinked_takeoff"]);
}

#[test]
fn centre_marks_are_checked_in_every_unit_and_resolve_layer_linetypes() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let circle = run(
            &mut s,
            Command::Draw(DrawSpec::Circle {
                centre: [2.0 * k, 3.0 * k],
                radius: k,
            }),
        )[0];
        let scope = CheckScope {
            rules: vec!["missing_center_mark".into()],
            ..Default::default()
        };
        let r = s.check_drawing(&scope);
        assert_eq!(rules(&r), ["missing_center_mark"], "{units}");
        assert_eq!(r.findings[0].handles, [circle]);
        assert_eq!(r.findings[0].pack, "general");
        assert_eq!(r.findings[0].severity, Severity::Info);
        assert_eq!(r.findings[0].at, Some([2.0 * k, 3.0 * k]));

        // An ordinary line through the centre is not a centre mark.
        let mark = line(&mut s, k, [0.0, 3.0], [4.0, 3.0]);
        assert_eq!(rules(&s.check_drawing(&scope)), ["missing_center_mark"]);
        s.drawing
            .doc
            .get_entity_mut(Handle::new(mark))
            .unwrap()
            .common_mut()
            .linetype = "CENTER".into();
        assert!(s.check_drawing(&scope).findings.is_empty(), "{units}");

        // The same mark may inherit CENTER from its layer.
        s.drawing
            .doc
            .get_entity_mut(Handle::new(mark))
            .unwrap()
            .common_mut()
            .linetype = "ByLayer".into();
        s.drawing.doc.layers.get_mut("0").unwrap().line_type = "CENTER".into();
        assert!(s.check_drawing(&scope).findings.is_empty(), "{units}");

        // The tolerance stays 0.5 mm in each drawing unit.
        let EntityType::Line(mark) = s.drawing.doc.get_entity_mut(Handle::new(mark)).unwrap()
        else {
            panic!("expected a line");
        };
        mark.start.y += 0.001 * k;
        mark.end.y += 0.001 * k;
        assert_eq!(
            rules(&s.check_drawing(&scope)),
            ["missing_center_mark"],
            "{units}"
        );
    }
}

#[test]
fn only_full_arcs_need_a_centre_mark() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let owner = s
            .drawing
            .doc
            .block_records
            .get("*Model_Space")
            .unwrap()
            .handle;
        let mut full = Vec::new();
        for (i, end) in [0.0, std::f64::consts::TAU, std::f64::consts::FRAC_PI_2]
            .into_iter()
            .enumerate()
        {
            let mut arc = Arc::from_center_radius_angles(
                Vector3::new(i as f64 * 4.0 * k, 0.0, 0.0),
                k,
                0.0,
                end,
            );
            arc.common.owner_handle = owner;
            let h = s
                .drawing
                .doc
                .add_entity(EntityType::Arc(arc))
                .unwrap()
                .value();
            if i < 2 {
                full.push(h);
            }
        }
        let r = s.check_drawing(&CheckScope {
            rules: vec!["missing_center_mark".into()],
            ..Default::default()
        });
        assert_eq!(r.findings.len(), 2, "{units}: {:?}", r.findings);
        assert_eq!(
            r.findings
                .iter()
                .flat_map(|f| f.handles.iter().copied())
                .collect::<Vec<_>>(),
            full
        );
    }
}

#[test]
fn text_height_on_paper_is_the_same_in_every_model_unit() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        s.run(Command::AnnotationScale {
            name: "1:100".into(),
        })
        .unwrap();
        let good = run(
            &mut s,
            Command::Text {
                at: [0.0, 0.0],
                height: 0.25 * k,
                rotation: 0.0,
                value: "NOTE".into(),
                style: None,
            },
        )[0];
        let bad = run(
            &mut s,
            Command::Text {
                at: [0.0, k],
                height: 0.02 * k,
                rotation: 0.0,
                value: "NOTE".into(),
                style: None,
            },
        )[0];
        let r = s.check_drawing(&CheckScope {
            rules: vec!["text_height_vs_scale".into()],
            ..Default::default()
        });
        assert_eq!(r.findings.len(), 1, "{units}: {:?}", r.findings);
        assert!(r.findings[0].handles.contains(&bad));
        assert!(!r.findings[0].handles.contains(&good));
    }
}

#[test]
fn a_requested_sheet_uses_its_own_room_boundaries() {
    let (mut s, _) = blank(6);
    s.run(Command::NewLayer {
        name: "A-ROOM-NAME".into(),
    })
    .unwrap();
    run(
        &mut s,
        Command::Draw(DrawSpec::Rectangle {
            a: [0.0, 0.0],
            b: [4.0, 3.0],
        }),
    );
    s.drawing.doc.add_layout("Plan").unwrap();
    let sheet = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sheet| sheet.name == "Plan")
        .unwrap()
        .block;
    s.run(Command::SetSheet {
        block: sheet.clone(),
    })
    .unwrap();
    let mut current = s.current.clone();
    current.layer = "A-ROOM-NAME".into();
    s.run(Command::SetCurrent { current }).unwrap();
    let text = run(
        &mut s,
        Command::Text {
            at: [1.0, 1.0],
            height: 0.2,
            rotation: 0.0,
            value: "STORE".into(),
            style: None,
        },
    )[0];
    s.run(Command::SetSheet {
        block: "*Model_Space".into(),
    })
    .unwrap();
    let r = s.check_drawing(&CheckScope {
        sheet: Some("Plan".into()),
        rules: vec!["room_open".into()],
        ..Default::default()
    });
    assert_eq!(r.sheet, sheet);
    assert_eq!(rules(&r), ["room_open"], "{:?}", r.findings);
    assert_eq!(r.findings[0].handles, [text]);
    assert_eq!(s.sheet, "*Model_Space");
}

#[test]
fn checking_thousands_of_lines_stays_quick() {
    let (mut s, _) = blank(6);
    let owner = s
        .drawing
        .doc
        .block_records
        .get("*Model_Space")
        .unwrap()
        .handle;
    for i in 0..5_000 {
        let mut line = Line::from_points(
            Vector3::new(i as f64, 0.0, 0.0),
            Vector3::new(i as f64, 1.0, 0.0),
        );
        line.common.owner_handle = owner;
        s.drawing.doc.add_entity(EntityType::Line(line)).unwrap();
    }
    let started = std::time::Instant::now();
    let r = check(&s);
    assert_eq!(r.entities, 5_000);
    assert!(
        started.elapsed().as_secs_f64() < 10.0,
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn room_sizes_accept_multiline_names_and_spaced_units() {
    for value in ["BOX 5.25 m x 3.25 m", "BEDROOM\n5.25m x 3.25m"] {
        let (mut s, _) = blank(6);
        run(
            &mut s,
            Command::Draw(DrawSpec::Rectangle {
                a: [0.0, 0.0],
                b: [4.0, 3.0],
            }),
        );
        s.run(Command::NewLayer {
            name: "A-ROOM-NAME".into(),
        })
        .unwrap();
        let mut current = s.current.clone();
        current.layer = "A-ROOM-NAME".into();
        s.run(Command::SetCurrent { current }).unwrap();
        let h = run(
            &mut s,
            Command::MText {
                at: [0.5, 2.0],
                width: 3.0,
                height: 0.15,
                value: value.into(),
                style: None,
            },
        )[0];
        let r = s.check_drawing(&CheckScope {
            rules: vec!["room_size_text_differs".into()],
            ..Default::default()
        });
        assert_eq!(
            rules(&r),
            ["room_size_text_differs"],
            "{value}: {:?}",
            r.findings
        );
        assert_eq!(r.findings[0].handles, [h]);
    }
}

#[test]
fn layer_zero_is_a_warning_on_a_sheet_but_right_inside_a_block() {
    for units in [6, 5, 4, 1] {
        let (mut s, k) = blank(units);
        let on_sheet = line(&mut s, k, [0.0, 0.0], [2.0, 0.0]);
        let r = s.check_drawing(&CheckScope {
            rules: vec!["layer_zero".into()],
            ..Default::default()
        });
        assert!(
            r.findings.iter().any(|f| f.rule == "layer_zero"
                && f.severity == Severity::Warning
                && f.handles == [on_sheet]),
            "{units}: {:?}",
            r.findings
        );
        let inside = line(&mut s, k, [0.0, 1.0], [2.0, 1.0]);
        let block: Command = serde_json::from_value(serde_json::json!({
            "type": "create_block", "name": "MARK", "base": [0, 0],
            "targets": [{"handle": inside}], "convert_to_insert": false
        }))
        .unwrap();
        s.run(block).unwrap();
        let r = s.check_drawing(&CheckScope {
            sheet: Some("MARK".into()),
            rules: vec!["layer_zero".into()],
            ..Default::default()
        });
        assert!(r.findings.is_empty(), "{units}: {:?}", r.findings);
    }
}
