//! Layout commands keep paper sizes, dictionary entries and annotations through undo and save.

use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::objects::ObjectType;
use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, DimSpec, LayoutOrientation, Outcome, Session, ViewportScale};

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
    let Outcome::Added { handles } = s.run(command).unwrap() else {
        panic!("new entities");
    };
    handles
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
    for command in [
        Command::NewLayout {
            name: "GROUND".into(),
            paper: "ISO A1".into(),
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
    assert_eq!(
        values(&s),
        BTreeSet::from(["Given title".into(), "A-101".into(), "P02".into()])
    );
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
    assert_eq!(values(&s).len(), 3);
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
    assert!(values(&s).is_empty());
    assert!(s
        .drawing
        .doc
        .entities_in_block(&sheet)
        .any(|e| matches!(e, EntityType::LwPolyline(_))));
}
