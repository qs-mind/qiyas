//! Layout commands keep paper sizes, dictionary entries and annotations through undo and save.

use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::objects::ObjectType;
use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, DimSpec, LayoutOrientation, Outcome, Session, ViewportScale};
use serde_json::json;

const MODEL: &str = "*Model_Space";

fn blank(units: i16) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_blank(&dir.path().join("Untitled.dwg"), units);
    (dir, s)
}

fn add_layout(s: &mut Session, name: &str, orientation: LayoutOrientation) -> String {
    s.run(Command::NewLayout {
        name: name.to_string(),
        paper: "ISO A1".to_string(),
        orientation,
    })
    .unwrap();
    s.drawing
        .sheets()
        .into_iter()
        .find(|sh| sh.name == name)
        .unwrap()
        .block
}

fn added(s: &mut Session, command: Command) -> Vec<u64> {
    match s.run(command).unwrap() {
        Outcome::Added { handles } | Outcome::TitleBlock { handles, .. } => handles,
        other => panic!("new entities: {other:?}"),
    }
}

fn values(s: &Session) -> BTreeSet<String> {
    s.drawing
        .doc
        .entities_in_block(&s.sheet)
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            EntityType::MText(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn layout_commands_and_make_sheet_share_iso_paper_aliases() {
    for paper in qiyas_core::paper_catalog()
        .into_iter()
        .filter(|paper| paper.name.starts_with("ISO ") || paper.name == "ANSI B")
    {
        let bare = paper.name.strip_prefix("ISO ").unwrap_or(&paper.name);
        for input in [bare.to_string(), format!(" {} ", bare.to_lowercase())] {
            let (_dir, mut s) = blank(6);
            s.run(Command::NewLayout {
                name: "Alias".into(),
                paper: input.clone(),
                orientation: LayoutOrientation::Portrait,
            })
            .unwrap();
            let block = s
                .drawing
                .sheets()
                .into_iter()
                .find(|sheet| sheet.name == "Alias")
                .unwrap()
                .block;
            s.set_sheet(&block).unwrap();
            assert_eq!(s.paper(), Some(paper.clone()));
            s.run(Command::SetLayoutPaper {
                name: "Alias".into(),
                paper: input.clone(),
                orientation: LayoutOrientation::Landscape,
            })
            .unwrap();
            let landscape = s.paper().unwrap();
            assert_eq!(landscape.name, paper.name);
            assert_eq!((landscape.w_mm, landscape.h_mm), (paper.h_mm, paper.w_mm));
            s.set_sheet(MODEL).unwrap();
            s.run_input(
                &json!({"type":"make_sheet","name":"Made","paper":input,
                    "orientation":"portrait","model_bounds":[[0,0],[1,1]]}),
                qiyas_core::Source::User,
            )
            .unwrap();
            let block = s
                .drawing
                .sheets()
                .into_iter()
                .find(|sheet| sheet.name == "Made")
                .unwrap()
                .block;
            s.set_sheet(&block).unwrap();
            assert_eq!(s.paper(), Some(paper.clone()));
        }
    }
    let (_dir, mut s) = blank(6);
    for paper in ["A5", "B", "ISO A5"] {
        assert!(s
            .run(Command::NewLayout {
                name: "Unknown".into(),
                paper: paper.into(),
                orientation: LayoutOrientation::Portrait,
            })
            .is_err());
    }
}

#[test]
fn new_drawings_have_template_paper_without_a_history_step() {
    for (units, name, size) in [
        (4, "ISO A3", (420.0, 297.0)),
        (5, "ISO A3", (420.0, 297.0)),
        (6, "ISO A3", (420.0, 297.0)),
        (1, "ANSI B", (431.8, 279.4)),
        (2, "ANSI B", (431.8, 279.4)),
    ] {
        let (_dir, mut s) = blank(units);
        assert_eq!(s.sheet, MODEL);
        assert!(!s.can_undo());
        let sheet = s
            .drawing
            .sheets()
            .into_iter()
            .find(|sheet| sheet.name == "Layout1")
            .unwrap();
        s.set_sheet(&sheet.block).unwrap();
        let paper = s.paper().unwrap();
        assert_eq!(paper.name, name);
        assert_eq!((paper.w_mm, paper.h_mm), size);
        assert!(!s.can_undo());
    }
}

#[test]
fn imported_unsized_layouts_stay_unsized_and_can_be_sized_in_one_step() {
    for ext in ["dwg", "dxf"] {
        let (dir, mut s) = blank(6);
        let sheet = s
            .drawing
            .sheets()
            .into_iter()
            .find(|sheet| sheet.name == "Layout1")
            .unwrap();
        for object in s.drawing.doc.objects.values_mut() {
            if let ObjectType::Layout(layout) = object {
                if layout.name == "Layout1" {
                    layout.paper_width = 0.0;
                    layout.paper_height = 0.0;
                    layout.paper_size.clear();
                }
            }
        }
        let path = dir.path().join(format!("unsized.{ext}"));
        s.save_as(&path).unwrap();
        let mut s = Session::open(&path).unwrap();
        s.set_sheet(&sheet.block).unwrap();
        assert!(
            s.paper().is_none(),
            "opening must not assign an imported layout's paper"
        );
        let command = json!({"type":"title_block", "fields":{}});
        let validation = s.validate_input(&command);
        assert!(validation.valid);
        assert!(validation
            .not_checked
            .iter()
            .any(|note| note.contains("paper size")));
        let error = s
            .run_input(&command, qiyas_core::Source::User)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("new_layout") && error.contains("set_layout_paper"),
            "{error}"
        );
        assert!(!s.can_undo());
        let objects = s.drawing.doc.objects.clone();
        s.run(Command::SetLayoutPaper {
            name: "Layout1".into(),
            paper: "ISO A3".into(),
            orientation: LayoutOrientation::Landscape,
        })
        .unwrap();
        assert_eq!(s.sheet, sheet.block);
        assert_eq!(s.history_steps().0.len(), 1);
        assert_eq!(s.paper().map(|p| (p.w_mm, p.h_mm)), Some((420.0, 297.0)));
        assert!(s.undo());
        assert_eq!(s.drawing.doc.objects, objects);
        assert!(s.paper().is_none());
        assert!(s.redo());
        s.save().unwrap();
        let mut again = Session::open(&path).unwrap();
        again.set_sheet(&sheet.block).unwrap();
        assert_eq!(
            again.paper().map(|p| (p.w_mm, p.h_mm)),
            Some((420.0, 297.0))
        );
    }
}

#[test]
fn title_blocks_report_empty_fields_and_own_their_layer_in_the_same_step() {
    for layer in ["A-ANNO-TTLB", "PROJECT-TITLE"] {
        let (_dir, mut s) = blank(6);
        s.set_sheet("*Paper_Space").unwrap();
        let current = s.current.clone();
        let mut input =
            json!({"type":"title_block", "fields":{"title":"Plan", "date":"", "revision":""}});
        if layer != "A-ANNO-TTLB" {
            input["layer"] = json!(layer);
        }
        let result = s.run_input(&input, qiyas_core::Source::User).unwrap();
        let Outcome::TitleBlock {
            handles,
            empty_fields,
        } = result.outcome
        else {
            panic!("title block result")
        };
        assert_eq!(
            empty_fields,
            [
                "project",
                "client",
                "consultant",
                "project_no",
                "drawing_no",
                "scale",
                "date",
                "drawn_by",
                "checked_by",
                "approved_by",
                "revision",
                "sheet",
                "revision_table"
            ]
        );
        assert!(!handles.is_empty());
        assert!(handles.iter().all(|handle| s
            .drawing
            .doc
            .get_entity(Handle::new(*handle))
            .unwrap()
            .common()
            .layer
            == layer));
        assert_eq!(s.current, current);
        assert_eq!(s.history_steps().0.len(), 1);
        assert!(s.drawing.doc.layers.contains(layer));
        assert!(s.undo());
        assert!(!s.drawing.doc.layers.contains(layer));
        assert!(handles.iter().all(|handle| s
            .drawing
            .doc
            .get_entity(Handle::new(*handle))
            .is_none()));
        assert!(s.redo());
        assert!(s.drawing.doc.layers.contains(layer));
    }
}

#[test]
fn a_new_layout_and_its_tables_are_one_undo_step() {
    let (_dir, mut s) = blank(6);
    let blocks = s.drawing.doc.block_records.clone();
    let objects = s.drawing.doc.objects.clone();
    let count = s.drawing.doc.entity_count();
    let sheet = add_layout(&mut s, "Ground floor", LayoutOrientation::Landscape);
    assert_eq!(s.sheet, MODEL);
    assert_eq!(s.history_steps().0.len(), 1);
    assert!(s.drawing.doc.block_records.contains(&sheet));
    assert_eq!(
        s.drawing.doc.entity_count(),
        count + 1,
        "the overall viewport"
    );
    s.set_sheet(&sheet).unwrap();
    assert!(s.undo());
    assert_eq!(s.sheet, MODEL);
    assert_eq!(s.drawing.doc.block_records, blocks);
    assert_eq!(s.drawing.doc.objects, objects);
    assert_eq!(s.drawing.doc.entity_count(), count);
    assert!(!s.can_undo());
    assert!(s.redo());
    s.set_sheet(&sheet).unwrap();
    let paper = s.info().paper.unwrap();
    assert_eq!(paper.name, "ISO A1");
    assert_eq!((paper.w_mm, paper.h_mm), (841.0, 594.0));
}

#[test]
fn rename_is_one_step_and_taken_names_are_refused_without_changes() {
    let (_dir, mut s) = blank(6);
    let block = add_layout(&mut s, "Ground", LayoutOrientation::Portrait);
    add_layout(&mut s, "Roof", LayoutOrientation::Portrait);
    let objects = s.drawing.doc.objects.clone();
    let blocks = s.drawing.doc.block_records.clone();
    let count = s.drawing.doc.entity_count();
    let steps = s.history_steps().0.len();
    s.run(Command::NewLayout {
        name: "GROUND".into(),
        paper: "ISO A1".into(),
        orientation: LayoutOrientation::Portrait,
    })
    .unwrap();
    assert_eq!(s.drawing.doc.objects, objects);
    assert_eq!(s.drawing.doc.block_records, blocks);
    assert_eq!(s.history_steps().0.len(), steps);
    for command in [
        Command::NewLayout {
            name: "GROUND".into(),
            paper: "ISO A4".into(),
            orientation: LayoutOrientation::Portrait,
        },
        Command::RenameLayout {
            from: "Ground".into(),
            to: "roof".into(),
        },
        Command::RenameLayout {
            from: "Model".into(),
            to: "Other".into(),
        },
    ] {
        assert!(s.run(command).is_err());
        assert_eq!(s.drawing.doc.objects, objects);
        assert_eq!(s.drawing.doc.block_records, blocks);
        assert_eq!(s.drawing.doc.entity_count(), count);
        assert_eq!(s.history_steps().0.len(), steps);
    }
    s.set_sheet(&block).unwrap();
    s.run(Command::RenameLayout {
        from: "ground".into(),
        to: "First floor".into(),
    })
    .unwrap();
    assert_eq!(s.sheet, block);
    assert_eq!(s.history_steps().0.len(), steps + 1);
    let dict = &s.drawing.doc.objects[&s.drawing.doc.header.acad_layout_dict_handle];
    let ObjectType::Dictionary(dict) = dict else {
        panic!("the layout dictionary")
    };
    assert!(dict.entries.iter().any(|(n, _)| n == "First floor"));
    assert!(!dict.entries.iter().any(|(n, _)| n == "Ground"));
    assert!(s.undo());
    assert_eq!(s.drawing.doc.objects, objects);
    assert!(s.redo());
    assert!(s
        .info()
        .sheets
        .iter()
        .any(|sh| sh.name == "First floor" && sh.block == block));
}

#[test]
fn a_backing_block_collision_is_refused_before_allocating_a_layout() {
    let (_dir, mut s) = blank(4);
    let count = s
        .drawing
        .doc
        .block_records
        .iter()
        .filter(|b| b.is_paper_space())
        .count();
    let next = if count == 0 {
        "*Paper_Space".into()
    } else {
        format!("*Paper_Space{}", count - 1)
    };
    // Make a gap: the codec chooses its name from the count, not from the largest suffix.
    let existing = s
        .drawing
        .doc
        .block_records
        .iter()
        .find(|b| b.is_paper_space())
        .map(|b| b.name.clone())
        .unwrap();
    s.drawing
        .doc
        .block_records
        .rename(&existing, &next)
        .unwrap();
    let before = s.drawing.doc.objects.clone();
    assert!(s
        .run(Command::NewLayout {
            name: "Sheet".into(),
            paper: "ISO A1".into(),
            orientation: LayoutOrientation::Portrait,
        })
        .is_err());
    assert_eq!(s.drawing.doc.objects, before);
    assert!(!s.can_undo());
}

#[test]
fn paper_size_and_renamed_layout_round_trip_in_dwg_and_dxf() {
    for ext in ["dwg", "dxf"] {
        let (dir, mut s) = blank(6);
        for (name, orientation) in [
            ("Portrait", LayoutOrientation::Portrait),
            ("Landscape", LayoutOrientation::Landscape),
        ] {
            add_layout(&mut s, name, orientation);
        }
        s.run(Command::RenameLayout {
            from: "Landscape".into(),
            to: "Landscape renamed".into(),
        })
        .unwrap();
        let path = dir.path().join(format!("layouts.{ext}"));
        s.save_as(&path).unwrap();
        let mut again = Session::open(&path).unwrap();
        assert!(again.info().paper.is_none());
        for (name, size) in [
            ("Portrait", (594.0, 841.0)),
            ("Landscape renamed", (841.0, 594.0)),
        ] {
            let sheet = again
                .drawing
                .sheets()
                .into_iter()
                .find(|sh| sh.name == name)
                .unwrap();
            again.set_sheet(&sheet.block).unwrap();
            let paper = again.info().paper.expect("a sized layout");
            assert_eq!(paper.name, "ISO A1", "{ext}");
            assert_eq!((paper.w_mm, paper.h_mm), size, "{ext}");
            let layout = again
                .drawing
                .doc
                .objects
                .values()
                .find_map(|o| match o {
                    ObjectType::Layout(l) if l.name == name => Some(l),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                layout.plot_paper_units, 1,
                "{ext}: paper coordinates remain millimetres"
            );
        }
    }
}

#[test]
fn a_viewport_ratio_uses_model_units_and_custom_scale_is_kept() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut s) = blank(units);
        let sheet = add_layout(&mut s, "Plan", LayoutOrientation::Landscape);
        s.set_sheet(&sheet).unwrap();
        let steps = s.history_steps().0.len();
        let h = added(
            &mut s,
            Command::MView {
                model_bounds: None,
                paper_centre: None,
                target: None,
                corner1: [20.0, 20.0],
                corner2: [820.0, 570.0],
                scale: Some(ViewportScale::Ratio("1:100".into())),
                centre: Some([500_000.0, 2_700_000.0]),
            },
        )[0];
        let view = &s.paper_views()[0];
        let one_metre = 1.0 / s.drawing.units.to_metres();
        assert!(
            (view.scale * one_metre - 10.0).abs() < 1e-10,
            "units {units}"
        );
        assert_eq!(view.centre, [500_000.0, 2_700_000.0]);
        assert_eq!(s.history_steps().0.len(), steps + 1);
        assert!(s.undo());
        assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
        assert!(s.redo());
        let custom = added(
            &mut s,
            Command::MView {
                model_bounds: None,
                paper_centre: None,
                target: None,
                corner1: [20.0, 20.0],
                corner2: [120.0, 120.0],
                scale: Some(ViewportScale::Number(0.125)),
                centre: Some([3.0, 4.0]),
            },
        )[0];
        let view = s
            .paper_views()
            .into_iter()
            .find(|v| v.handle == custom)
            .unwrap();
        assert_eq!(view.scale, 0.125);
        assert_eq!(view.centre, [3.0, 4.0]);
        for scale in [
            ViewportScale::Ratio("1:0".into()),
            ViewportScale::Number(-1.0),
        ] {
            assert!(s
                .run(Command::MView {
                    model_bounds: None,
                    paper_centre: None,
                    target: None,
                    corner1: [0.0, 0.0],
                    corner2: [1.0, 1.0],
                    scale: Some(scale),
                    centre: None,
                })
                .is_err());
        }
    }
}

#[test]
fn layout_text_and_dimension_sizes_are_paper_mm_in_every_model_unit() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut s) = blank(units);
        let sheet = add_layout(&mut s, "Plan", LayoutOrientation::Landscape);
        s.set_sheet(&sheet).unwrap();
        s.run(Command::AnnotationScale {
            name: "1:250".into(),
        })
        .unwrap();
        assert_eq!(s.paper_unit(), 1.0);
        assert_eq!(s.default_text_height(), 2.5);
        let text = added(
            &mut s,
            Command::Text {
                at: [20.0, 20.0],
                height: 0.0,
                rotation: 0.0,
                value: "Given".into(),
                style: None,
            },
        )[0];
        let Some(EntityType::Text(t)) = s.drawing.doc.get_entity(Handle::new(text)) else {
            panic!("text")
        };
        assert_eq!(t.height, 2.5);
        let dim = added(
            &mut s,
            Command::Dimension(DimSpec::Linear {
                p1: [20.0, 20.0],
                p2: [120.0, 20.0],
                line_point: [70.0, 10.0],
                rotation: None,
            }),
        )[0];
        let Some(EntityType::Dimension(d)) = s.drawing.doc.get_entity(Handle::new(dim)) else {
            panic!("dimension")
        };
        let height = s
            .drawing
            .doc
            .entities_in_block(&d.base().block_name)
            .find_map(|e| match e {
                EntityType::Text(t) => Some(t.height),
                _ => None,
            })
            .unwrap();
        assert!((height - 2.5).abs() < 1e-10, "units {units}: {height}");
        let pasted = added(
            &mut s,
            Command::PasteFromClipboard {
                at: [30.0, 30.0],
                text: Some("Pasted".into()),
                as_block: false,
                height: None,
            },
        )[0];
        let Some(EntityType::Text(t)) = s.drawing.doc.get_entity(Handle::new(pasted)) else {
            panic!("pasted text")
        };
        assert_eq!(t.height, 2.5);
        s.set_sheet(MODEL).unwrap();
        assert!((s.default_text_height() * s.drawing.units.to_metres() - 0.625).abs() < 1e-12);
    }
}

#[test]
fn title_block_writes_only_supplied_values_and_undo_removes_it_all() {
    let (_dir, mut s) = blank(6);
    let fields = BTreeMap::from([
        ("title".into(), "Given title".into()),
        ("drawing_no".into(), "A-101".into()),
        ("revision".into(), "P02".into()),
    ]);
    let command = Command::TitleBlock {
        revision_table: vec![],
        language: None,
        layer: "A-ANNO-TTLB".into(),
        fields,
        margin_mm: 10.0,
        binding_mm: 20.0,
    };
    assert!(s.run(command.clone()).is_err(), "model space has no paper");
    let sheet = add_layout(&mut s, "Plan", LayoutOrientation::Landscape);
    s.set_sheet(&sheet).unwrap();
    let before = s.drawing.doc.entity_count();
    let apps = s.drawing.doc.app_ids.clone();
    let steps = s.history_steps().0.len();
    let handles = added(&mut s, command);
    assert_eq!(s.history_steps().0.len(), steps + 1);
    let text = values(&s);
    for value in [
        "Given title",
        "A-101",
        "P02",
        "For review",
        "Client",
        "Consultant",
        "Checked by",
        "Approved by",
        "Description",
    ] {
        assert!(text.contains(value), "{value}: {text:?}");
    }
    assert!(!text.contains("Qiyas"));
    let frame = handles
        .iter()
        .find_map(|h| match s.drawing.doc.get_entity(Handle::new(*h)) {
            Some(EntityType::LwPolyline(p)) => Some(p),
            _ => None,
        })
        .unwrap();
    let xs: Vec<_> = frame.vertices.iter().map(|v| v.location.x).collect();
    let ys: Vec<_> = frame.vertices.iter().map(|v| v.location.y).collect();
    assert_eq!(xs.iter().copied().fold(f64::INFINITY, f64::min), 20.0);
    assert_eq!(xs.iter().copied().fold(f64::NEG_INFINITY, f64::max), 831.0);
    assert_eq!(ys.iter().copied().fold(f64::INFINITY, f64::min), 10.0);
    assert_eq!(ys.iter().copied().fold(f64::NEG_INFINITY, f64::max), 584.0);
    assert!(s.undo());
    assert_eq!(s.drawing.doc.entity_count(), before);
    assert_eq!(s.drawing.doc.app_ids, apps);
    assert!(s.redo());
    assert_eq!(values(&s), text);
}

#[test]
fn empty_title_block_never_fills_in_date_project_or_author() {
    let (_dir, mut s) = blank(4);
    let sheet = add_layout(&mut s, "Plan", LayoutOrientation::Portrait);
    s.set_sheet(&sheet).unwrap();
    added(
        &mut s,
        serde_json::from_value(serde_json::json!({
            "type": "title_block", "fields": {}
        }))
        .unwrap(),
    );
    let text = values(&s);
    assert!(text.contains("For review"));
    assert!(!text.contains("Qiyas") && !text.contains("P01"));
    assert!(!text
        .iter()
        .any(|v| v.len() == 10 && v.chars().next().is_some_and(|c| c.is_ascii_digit())));
    assert!(s
        .drawing
        .doc
        .entities_in_block(&sheet)
        .any(|e| matches!(e, EntityType::LwPolyline(_))));
}

#[test]
fn title_block_shares_its_frame_edges_on_small_and_large_paper() {
    for units in [4, 5, 6, 1] {
        for paper in ["ISO A4", "ISO A1"] {
            let (_dir, mut s) = blank(units);
            s.run(Command::NewLayout {
                name: "Plan".into(),
                paper: paper.into(),
                orientation: LayoutOrientation::Portrait,
            })
            .unwrap();
            let sheet = s
                .drawing
                .sheets()
                .into_iter()
                .find(|sheet| sheet.name == "Plan")
                .unwrap();
            s.set_sheet(&sheet.block).unwrap();
            // On a layer of its own: layer 0 on a sheet is a drafting fault of its own.
            for command in [
                serde_json::json!({"type": "new_layer", "name": "A-ANNO-TTLB"}),
                serde_json::json!({"type": "set_current", "current": {"layer": "A-ANNO-TTLB"}}),
            ] {
                s.run(serde_json::from_value(command).unwrap()).unwrap();
            }
            let handles = added(
                &mut s,
                Command::TitleBlock {
                    revision_table: vec![],
                    language: None,
                    layer: "A-ANNO-TTLB".into(),
                    fields: BTreeMap::from([
                        ("title".into(), "Ground floor".into()),
                        ("drawing_no".into(), "A-101".into()),
                    ]),
                    margin_mm: 10.0,
                    binding_mm: 20.0,
                },
            );
            let check = s.check_drawing(&qiyas_core::check::CheckScope {
                handles,
                packs: vec!["general".into()],
                ..Default::default()
            });
            assert_eq!(
                (check.errors, check.warnings),
                (0, 0),
                "{units}, {paper}: {:?}",
                check.findings
            );
        }
    }
}

fn sheet(s: &mut Session, input: serde_json::Value) -> qiyas_core::sheet_setup::SheetReport {
    match s.run_input(&input, qiyas_core::Source::Ai).unwrap().outcome {
        Outcome::Sheet { report } => *report,
        other => panic!("sheet: {other:?}"),
    }
}

#[test]
fn sheet_orientation_uses_aspect_fit_and_an_explicit_override() {
    for (bounds, orientation) in [
        ([[0.0, 0.0], [180.0, 96.0]], LayoutOrientation::Landscape),
        ([[0.0, 0.0], [96.0, 180.0]], LayoutOrientation::Portrait),
    ] {
        let (_dir, mut s) = blank(6);
        let made = sheet(
            &mut s,
            json!({"type":"make_sheet","name":"Fit","paper":"A1",
            "scale":"1:500","model_bounds":bounds}),
        );
        assert_eq!(made.orientation, orientation);
        assert_eq!(made.scale, "1:500");
        assert!(made.continuations.is_empty());
    }
    let (_dir, mut s) = blank(6);
    let made = sheet(
        &mut s,
        json!({"type":"make_sheet","name":"Override","paper":"A1",
        "scale":"1:500","orientation":"portrait","model_bounds":[[0,0],[180,96]]}),
    );
    assert_eq!(made.orientation, LayoutOrientation::Portrait);
}

#[test]
fn split_sheets_avoid_the_core_have_reciprocal_references_and_key_plans_in_every_unit() {
    for units in [1, 2, 4, 5, 6] {
        for language in ["en", "ar"] {
            let (_dir, mut s) = blank(units);
            let u = 1.0 / s.drawing.units.to_metres();
            s.run_input(
                &json!({"type":"grid","x":[{"label":"A","at":80.0*u},{"label":"B","at":90.0*u}],
                "y":[{"label":"1","at":40.0*u},{"label":"2","at":48.0*u}],
                "extent":{"min":[0,0],"max":[180.0*u,96.0*u]}}),
                qiyas_core::Source::Ai,
            )
            .unwrap();
            added(
                &mut s,
                serde_json::from_value(
                    json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[180.0*u,96.0*u]}),
                )
                .unwrap(),
            );
            let before = s.history_steps().0.len();
            let made = sheet(
                &mut s,
                json!({"type":"make_sheet","name":"S-01","drawing_no":"D-01","paper":"A1",
                "scale":"1:250","language":language,"model_bounds":[[0,0],[180.0*u,96.0*u]],
                "core_regions":[[[85.0*u,42.0*u],[95.0*u,42.0*u],[95.0*u,54.0*u],[85.0*u,54.0*u]]]}),
            );
            assert_eq!(s.history_steps().0.len(), before + 1);
            assert_eq!(made.orientation, LayoutOrientation::Landscape);
            assert_eq!(made.continuations.len(), 1);
            assert_eq!(made.model_bounds[1][1], 40.0 * u);
            assert_eq!(made.continuations[0].model_bounds[0][1], 40.0 * u);
            assert_eq!(made.splits.len(), 1);
            assert!(made.splits[0].basis.contains("grid"));
            for (page, other_number) in [(&made, "D-01-2"), (&made.continuations[0], "D-01-1")] {
                assert_eq!(page.scale, "1:250");
                let block = s
                    .drawing
                    .sheets()
                    .into_iter()
                    .find(|sh| sh.name == page.layout)
                    .unwrap()
                    .block;
                s.set_sheet(&block).unwrap();
                let text = values(&s).into_iter().collect::<Vec<_>>().join("|");
                assert!(text.contains(other_number), "{text}");
                assert!(text.contains(if language == "ar" {
                    "خط المطابقة"
                } else {
                    "Match line"
                }));
                assert!(text.contains(if language == "ar" {
                    "المخطط المفتاحي"
                } else {
                    "Key plan"
                }));
                if language == "ar" {
                    assert!(!text.contains("For review") && !text.contains("Match line"));
                }
                let views = s.paper_views_of(&block);
                assert_eq!(views.len(), 2, "main view and key plan");
                assert!(views
                    .iter()
                    .any(|v| v.handle == page.viewport
                        && (v.scale - 1000.0 / u / 250.0).abs() < 1e-8));
                assert!(s
                    .drawing
                    .doc
                    .entities_in_block(&block)
                    .any(|e| e.common().layer == "A-ANNO-KEY"
                        && matches!(e, EntityType::LwPolyline(_))));
            }
            assert!(!s.drawing.sheets().iter().any(|sh| sh.name == "Layout1"));
            assert!(s.undo());
            assert!(s.drawing.sheets().iter().any(|sh| sh.name == "Layout1"));
            assert!(!s
                .drawing
                .sheets()
                .iter()
                .any(|sh| sh.name.starts_with("S-01")));
            assert!(s.redo());
            assert_eq!(
                s.drawing
                    .sheets()
                    .iter()
                    .filter(|sh| sh.name.starts_with("S-01"))
                    .count(),
                2
            );
        }
    }
}

#[test]
fn supplied_split_lines_work_without_grid_and_invalid_splits_leave_no_partial_sheets() {
    let (_dir, mut s) = blank(6);
    let before = s.drawing.doc.objects.clone();
    let bad = json!({"type":"make_sheet","name":"Split","paper":"A1","scale":"1:250",
        "model_bounds":[[0,0],[180,96]],"split_lines":[[[0,48],[180,48]]],
        "core_regions":[[[85,42],[95,42],[95,54],[85,54]]]});
    assert!(s.run_input(&bad, qiyas_core::Source::Ai).is_err());
    assert_eq!(s.drawing.doc.objects, before);
    assert!(!s.can_undo());
    let mut good = bad;
    good["split_lines"] = json!([[[0, 40], [180, 40]]]);
    let made = sheet(&mut s, good);
    assert_eq!(made.continuations.len(), 1);
    assert!(made.splits[0].basis.contains("Supplied"));
}

#[test]
fn default_layout_reuse_survives_save_but_never_changes_an_opened_or_nonempty_layout() {
    let (dir, mut s) = blank(6);
    let path = dir.path().join("saved.dxf");
    s.save_as_headless(&path).unwrap();
    let mut opened = Session::open(&path).unwrap();
    let before = opened.drawing.doc.objects.clone();
    let old = before
        .iter()
        .find_map(|(h, o)| match o {
            ObjectType::Layout(l) if l.name == "Layout1" => Some((*h, o.clone())),
            _ => None,
        })
        .unwrap();
    let input = json!({"type":"make_sheet","name":"First","model_bounds":[[0,0],[10,8]]});
    sheet(&mut s, input.clone());
    assert!(!s.drawing.sheets().iter().any(|sh| sh.name == "Layout1"));
    sheet(&mut opened, input);
    assert_eq!(opened.drawing.doc.objects.get(&old.0), Some(&old.1));
    let (_dir, mut occupied) = blank(6);
    occupied.set_sheet("*Paper_Space").unwrap();
    let note = added(
        &mut occupied,
        serde_json::from_value(
            json!({"type":"text","at":[30,30],"height":3,"value":"Engineer note"}),
        )
        .unwrap(),
    )[0];
    sheet(
        &mut occupied,
        json!({"type":"make_sheet","name":"Second","model_bounds":[[0,0],[10,8]]}),
    );
    assert!(occupied
        .drawing
        .sheets()
        .iter()
        .any(|sh| sh.name == "Layout1"));
    assert!(occupied.drawing.doc.get_entity(Handle::new(note)).is_some());
}

#[test]
fn title_metadata_is_literal_bilingual_and_missing_fields_are_reported() {
    for language in ["en", "ar"] {
        let (_dir, mut s) = blank(6);
        let made = sheet(
            &mut s,
            json!({"type":"make_sheet","name":"Metadata","paper":"A1","model_bounds":[[0,0],[10,8]],
            "language":language,"client":"Client name","consultant":"Consultant name","project_no":"P-17",
            "drawing_no":"D-20","checked_by":"Checker","approved_by":"","revision":"R3","status":"Approved",
            "revision_table":[{"rev":"R3","date":"2026-10-29","description":"Coordination update"}]}),
        );
        assert!(made.empty_fields.contains(&"drawn_by".into()));
        assert!(made.empty_fields.contains(&"approved_by".into()));
        assert!(made.empty_fields.contains(&"date".into()));
        assert!(!made.empty_fields.contains(&"revision_table".into()));
        let block = s
            .drawing
            .sheets()
            .into_iter()
            .find(|sh| sh.name == "Metadata")
            .unwrap()
            .block;
        s.set_sheet(&block).unwrap();
        let text = values(&s);
        for value in [
            "Client name",
            "Consultant name",
            "P-17",
            "D-20",
            "Checker",
            "R3",
            "2026-10-29",
            "Coordination update",
        ] {
            assert!(text.contains(value));
        }
        assert!(!text.contains("Approved") && !text.contains("Qiyas") && !text.contains("P01"));
        for value in if language == "ar" {
            [
                "العميل",
                "الاستشاري",
                "دقق بواسطة",
                "اعتمد بواسطة",
                "الوصف",
                "للمراجعة",
            ]
        } else {
            [
                "Client",
                "Consultant",
                "Checked by",
                "Approved by",
                "Description",
                "For review",
            ]
        } {
            assert!(text.contains(value), "{text:?}");
        }
    }
}

#[test]
fn notes_overflow_returns_the_complete_notes_without_drawing_over_other_zones() {
    let (_dir, mut s) = blank(6);
    let notes = vec!["A long note with information to retain. ".repeat(300)];
    let made = sheet(
        &mut s,
        json!({"type":"make_sheet","name":"Notes","paper":"A3",
        "model_bounds":[[0,0],[10,8]],"notes":notes}),
    );
    assert!(!made.notes_fit);
    assert_eq!(made.unplaced_notes, notes);
}

#[test]
fn sheet_craft_inputs_are_discoverable_and_reject_unknown_revision_fields() {
    let registry = qiyas_core::registry::registry();
    let schema = &registry.get("make_sheet").unwrap().schema;
    for key in [
        "client",
        "consultant",
        "project_no",
        "checked_by",
        "approved_by",
        "revision_table",
        "split_lines",
        "core_regions",
        "zones",
    ] {
        assert!(schema["properties"].get(key).is_some(), "{key}: {schema}");
    }
    let schema = &registry.get("title_block").unwrap().schema;
    assert!(schema["properties"].get("revision_table").is_some());
    assert!(schema["properties"].get("language").is_some());
    let (_dir, mut s) = blank(6);
    let result = s.run_input(
        &json!({"type":"make_sheet","name":"Invalid","model_bounds":[[0,0],[10,8]],
        "revision_table":[{"rev":"R1","date":"","description":"","invented":"value"}]}),
        qiyas_core::Source::Ai,
    );
    assert!(result.is_err());
    assert!(!s.can_undo());
}

#[test]
fn direct_title_block_lists_empty_revision_cells_and_uses_arabic_captions() {
    let (_dir, mut s) = blank(6);
    s.set_sheet("*Paper_Space").unwrap();
    let outcome = s
        .run_input(
            &json!({"type":"title_block","language":"ar",
        "fields":{"client":"العميل المحدد","status":"Approved"},
        "revision_table":[{"rev":"R2","date":"","description":"تنسيق"}]}),
            qiyas_core::Source::User,
        )
        .unwrap()
        .outcome;
    let Outcome::TitleBlock { empty_fields, .. } = outcome else {
        panic!("title block")
    };
    assert!(empty_fields.contains(&"revision_table[0].date".into()));
    assert!(empty_fields.contains(&"drawn_by".into()));
    let text = values(&s);
    assert!(text.contains("للمراجعة") && text.contains("العميل المحدد") && text.contains("تنسيق"));
    assert!(!text.contains("Approved") && !text.contains("Qiyas") && !text.contains("For review"));
}

#[test]
fn requested_sheet_scale_keeps_its_full_denominator_precision() {
    let (_dir, mut s) = blank(6);
    let den = 333.3333333333333;
    let made = sheet(
        &mut s,
        json!({"type":"make_sheet","name":"Precise","paper":"A1",
        "scale":format!("1:{den}"),"model_bounds":[[0,0],[30,20]]}),
    );
    assert_eq!(made.scale, format!("1:{den}"));
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| sh.name == "Precise")
        .unwrap()
        .block;
    let view = s
        .paper_views_of(&block)
        .into_iter()
        .find(|v| v.handle == made.viewport)
        .unwrap();
    assert!((view.scale - 1000.0 / den).abs() < 1e-12);
}
