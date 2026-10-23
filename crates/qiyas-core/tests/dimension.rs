//! Dimensions, leaders, tolerances and the annotation scale. A new dimension is a real Dimension
//! entity with its `*D` block (lines, Solid arrowheads, a Text) generated from the current dimension
//! style, so another CAD program shows it as Qiyas does; the dimensions of an existing drawing are
//! drawn (their block, or geometry generated from their points), picked and measured as themselves.

use opencadcodec::entities::{Dimension, DimensionLinear, Leader, LeaderCreationType};
use opencadcodec::{BlockRecord, CadDocument, Color, EntityType, Handle, Line, Vector3};
use qiyas_core::fixtures::{sample_doc, schedule_doc, write_drawing};
use qiyas_core::{
    Command, CoreError, DimEditOp, DimSpec, DimStyleFields, Kind, ModifySpec, Outcome, Session,
    Target, TextScope,
};
use qiyas_geom::DimLook;

const MODEL: &str = "*Model_Space";

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    (dir, Session::open(&path).unwrap())
}

/// The sample (metres; a 10 m line from (0, 0) to (10, 0) on A-WALL) with its Standard dimension
/// style keeping trailing zeros.
fn sample() -> CadDocument {
    let mut doc = sample_doc();
    let style = doc.dim_styles.get_mut("Standard").unwrap();
    style.dimzin = 0;
    doc
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn fields(precision: i16) -> DimStyleFields {
    DimStyleFields {
        arrow_size: 0.25,
        text_height: 0.3,
        precision,
        ext_offset: 0.1,
        ext_extension: 0.2,
        gap: 0.05,
        scale: 1.0,
        show_units: true,
    }
}

fn added(outcome: Outcome) -> Vec<u64> {
    match outcome {
        Outcome::Added { handles } => handles,
        other => panic!("expected new entities, got {other:?}"),
    }
}

fn one(outcome: Outcome) -> u64 {
    let handles = added(outcome);
    assert_eq!(handles.len(), 1, "{handles:?}");
    handles[0]
}

fn dim(s: &Session, h: u64) -> Dimension {
    match s.drawing.doc.get_entity(Handle::new(h)) {
        Some(EntityType::Dimension(d)) => d.clone(),
        other => panic!("{h:X} is no dimension: {other:?}"),
    }
}

/// The entities of a dimension's `*D` block.
fn block_of(s: &Session, h: u64) -> Vec<EntityType> {
    let d = dim(s, h);
    let name = &d.base().block_name;
    assert!(name.starts_with("*D"), "{name}");
    let record = s.drawing.doc.block_records.get(name).expect("its block");
    assert!(record.flags.anonymous);
    s.drawing.doc.entities_in_block(name).cloned().collect()
}

/// (lines, arrows, texts) of a dimension's block.
fn counts(parts: &[EntityType]) -> (usize, usize, Vec<String>) {
    let lines = parts
        .iter()
        .filter(|e| matches!(e, EntityType::Line(_)))
        .count();
    let arrows = parts
        .iter()
        .filter(|e| matches!(e, EntityType::Solid(_)))
        .count();
    let texts = parts
        .iter()
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    (lines, arrows, texts)
}

fn text_height(parts: &[EntityType]) -> f64 {
    parts
        .iter()
        .find_map(|e| match e {
            EntityType::Text(t) => Some(t.height),
            _ => None,
        })
        .expect("a text")
}

fn linear_cmd() -> Command {
    Command::Dimension(DimSpec::Linear {
        p1: [0.0, 0.0],
        p2: [10.0, 0.0],
        line_point: [5.0, 2.0],
        rotation: None,
    })
}

/// The sample with a 3-decimal Standard style and the 10 m line dimensioned.
fn dimensioned(name: &str) -> (tempfile::TempDir, Session, u64) {
    let (dir, mut s) = open(&sample(), name);
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: true,
    })
    .unwrap();
    let h = one(s.run(linear_cmd()).unwrap());
    (dir, s, h)
}

#[test]
fn a_linear_dimension_of_the_10_m_line_reads_10_000_and_has_a_complete_block() {
    let (_dir, mut s, h) = dimensioned("dim.dxf");
    let d = dim(&s, h);
    let Dimension::Linear(linear) = &d else {
        panic!("{d:?}")
    };
    assert!((d.measurement() - 10.0).abs() < 1e-9);
    assert_eq!(
        linear.rotation, 0.0,
        "a line point above the points: horizontal"
    );
    assert_eq!(d.base().style_name, "Standard");
    assert_eq!(d.base().common.layer, "0", "the current layer");
    let parts = block_of(&s, h);
    let (lines, arrows, texts) = counts(&parts);
    assert_eq!(
        texts,
        vec!["10.000 m".to_string()],
        "the style's precision, then the drawing's unit"
    );
    assert_eq!(lines, 3, "two extension lines and the dimension line");
    assert_eq!(arrows, 2);
    assert!((text_height(&parts) - 0.3).abs() < 1e-12);
    // Text above the line, its middle recorded on the dimension.
    let at = d.base().text_middle_point;
    assert!((at.x - 5.0).abs() < 1e-9 && at.y > 2.0, "{at:?}");

    // One undo step: the dimension and its block go, and come back.
    let blocks = s.drawing.doc.block_records.iter().count();
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
    assert_eq!(s.drawing.doc.block_records.iter().count(), blocks - 1);
    assert!(s.redo());
    assert_eq!(counts(&block_of(&s, h)).2, vec!["10.000 m".to_string()]);
}

#[test]
fn a_new_dimension_saves_with_its_block_and_reads_back_in_dxf_and_dwg() {
    for ext in ["dxf", "dwg"] {
        let (dir, mut s, h) = dimensioned(&format!("dim.{ext}"));
        s.save().unwrap();
        let again = Session::open(&dir.path().join(format!("dim.{ext}"))).unwrap();
        let parts = block_of(&again, h);
        let (lines, arrows, texts) = counts(&parts);
        assert_eq!((lines, arrows), (3, 2), "{ext}: the block is complete");
        assert_eq!(texts, vec!["10.000 m".to_string()], "{ext}");
        let rows = again.read_dimensions(None);
        let row = rows
            .iter()
            .find(|r| r.handle == h)
            .expect("read as a dimension");
        assert!((row.measurement.unwrap() - 10.0).abs() < 1e-9, "{ext}");
    }
}

#[test]
fn a_dimension_with_a_block_in_an_existing_drawing_is_drawn_picked_and_measured() {
    let mut doc = sample();
    let mut record = BlockRecord::new("*D7");
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    record.flags.anonymous = true;
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    let mut inside = EntityType::Line(Line::from_coords(0.0, 30.0, 0.0, 4.0, 30.0, 0.0));
    inside.common_mut().owner_handle = owner;
    inside.common_mut().color = Color::ByBlock;
    doc.add_entity(inside).unwrap();
    let mut d = DimensionLinear::new(Vector3::new(0.0, 28.0, 0.0), Vector3::new(4.0, 28.0, 0.0));
    d.definition_point = Vector3::new(4.0, 30.0, 0.0);
    d.base.block_name = "*D7".to_string();
    d.base.common.layer = "A-WALL".to_string();
    let h = doc
        .add_entity(EntityType::Dimension(Dimension::Linear(d)))
        .unwrap()
        .value();
    let (_dir, mut s) = open(&doc, "old.dxf");

    let picked = s
        .pick([2.0, 30.0], 0.05)
        .expect("the block's line is drawn");
    assert_eq!(
        (picked.top, picked.entity),
        (h, h),
        "picked as the dimension"
    );
    assert!(picked.chain.is_empty());
    assert_eq!(picked.kind.as_deref(), Some("Dimension"));
    let window = s.pick_window([[-1.0, 27.0], [5.0, 31.0]], false);
    assert!(window.iter().any(|p| p.top == h));

    let info = s.entity_info(h, &[]).unwrap();
    assert_eq!(info.kind, "Dimension");
    assert!((info.length.unwrap() - 4.0).abs() < 1e-9);
    assert_eq!(info.area, None);
    assert_eq!(info.text.as_deref(), Some("4.00"));

    let outcome = s
        .run(Command::MeasureEntity {
            handle: h,
            chain: Vec::new(),
        })
        .unwrap();
    let Outcome::Measured { measurement } = outcome else {
        panic!()
    };
    assert_eq!(measurement.kind, Kind::Length);
    assert!((measurement.value - 4.0).abs() < 1e-9);

    // The take-off follows a move; the dimension keeps measuring 4 m.
    s.run(Command::Modify(ModifySpec::Move {
        targets: vec![t(h)],
        delta: [0.0, 5.0],
    }))
    .unwrap();
    let row = s
        .takeoff
        .items
        .iter()
        .find(|m| m.handles == vec![h])
        .unwrap();
    assert!((row.value - 4.0).abs() < 1e-9);
    assert!(s.pick([2.0, 35.0], 0.05).is_some_and(|p| p.top == h));
}

#[test]
fn a_dimension_without_a_block_is_generated_on_screen() {
    // The schedule drawing's dimension (0, -5) to (4, -5) has no block; its line is at y 0.
    let (_dir, s) = open(&schedule_doc(), "plain.dxf");
    let h = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Dimension(_)))
        .unwrap()
        .common()
        .handle
        .value();
    assert!(dim(&s, h).base().block_name.is_empty());
    let picked = s
        .pick([2.0, 0.0], 0.01)
        .expect("its dimension line is drawn");
    assert_eq!(picked.top, h);
    let batch = s.batch();
    assert!(
        batch.texts.iter().any(|t| t.entity == h),
        "its text is drawn"
    );
}

#[test]
fn quick_dimension_of_three_lines_makes_three_dimensions() {
    let (_dir, mut s) = open(&sample(), "quick.dxf");
    let lines: Vec<u64> = [
        ([20.0, 5.0], [24.0, 5.0]),
        ([30.0, 5.0], [33.0, 5.0]),
        ([40.0, 5.0], [40.0, 7.0]),
    ]
    .into_iter()
    .map(|(a, b)| {
        one(s
            .run(Command::Draw(qiyas_core::DrawSpec::Line { a, b }))
            .unwrap())
    })
    .collect();
    let dims = added(
        s.run(Command::Dimension(DimSpec::Quick {
            targets: lines.iter().map(|&h| t(h)).collect(),
            line_point: [0.0, 4.0],
        }))
        .unwrap(),
    );
    assert_eq!(dims.len(), 3);
    let values: Vec<f64> = dims.iter().map(|&h| dim(&s, h).measurement()).collect();
    assert_eq!(values, vec![4.0, 3.0, 2.0]);
    for h in &dims {
        assert!(!block_of(&s, *h).is_empty());
    }
    assert!(s.undo(), "one step");
    assert!(dims
        .iter()
        .all(|&h| s.drawing.doc.get_entity(Handle::new(h)).is_none()));
}

#[test]
fn every_kind_of_dimension_measures_what_it_dimensions() {
    let (_dir, mut s) = open(&sample(), "kinds.dxf");
    // The sample's arc: centre (20, 0), radius 2, from 0 to 180 degrees.
    let arc = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Arc(_)))
        .unwrap()
        .common()
        .handle
        .value();
    let measure = |s: &mut Session, spec: DimSpec| {
        let h = one(s.run(Command::Dimension(spec)).unwrap());
        assert!(!block_of(s, h).is_empty());
        dim(s, h)
    };
    let aligned = measure(
        &mut s,
        DimSpec::Aligned {
            p1: [0.0, 0.0],
            p2: [3.0, 4.0],
            offset: 1.0,
        },
    );
    assert!(matches!(aligned, Dimension::Aligned(_)));
    assert!((aligned.measurement() - 5.0).abs() < 1e-9);
    let vertical = measure(
        &mut s,
        DimSpec::Linear {
            p1: [0.0, 0.0],
            p2: [3.0, 4.0],
            line_point: [-2.0, 2.0],
            rotation: None,
        },
    );
    assert!(
        (vertical.measurement() - 4.0).abs() < 1e-9,
        "beside: vertical"
    );
    let radius = measure(
        &mut s,
        DimSpec::Radius {
            target: t(arc),
            point: [20.0, 3.0],
        },
    );
    assert!(matches!(radius, Dimension::Radius(_)));
    assert!((radius.measurement() - 2.0).abs() < 1e-9);
    let diameter = measure(
        &mut s,
        DimSpec::Diameter {
            target: t(arc),
            point: [21.0, 1.0],
        },
    );
    assert!((diameter.measurement() - 4.0).abs() < 1e-9);
    let length = measure(
        &mut s,
        DimSpec::ArcLength {
            target: t(arc),
            point: [20.0, 3.0],
        },
    );
    assert!((length.measurement() - 2.0 * std::f64::consts::PI).abs() < 1e-9);
    let angle = measure(
        &mut s,
        DimSpec::Angular {
            vertex: [0.0, 0.0],
            p1: [1.0, 0.0],
            p2: [0.0, 1.0],
            arc_point: [1.0, 1.0],
        },
    );
    assert!((angle.measurement() - 90.0).abs() < 1e-9);
    let ordinate = measure(
        &mut s,
        DimSpec::Ordinate {
            feature: [4.0, 3.0],
            leader_end: [4.0, 6.0],
        },
    );
    assert!((ordinate.measurement() - 4.0).abs() < 1e-9, "an X datum");

    let base = one(s.run(linear_cmd()).unwrap());
    let baseline = added(
        s.run(Command::Dimension(DimSpec::Baseline {
            from: t(base),
            points: vec![[12.0, 0.0], [15.0, 0.0]].into(),
        }))
        .unwrap(),
    );
    let values: Vec<f64> = baseline.iter().map(|&h| dim(&s, h).measurement()).collect();
    assert_eq!(values, vec![12.0, 15.0], "from the base's first point");
    let continued = added(
        s.run(Command::Dimension(DimSpec::Continue {
            from: t(base),
            points: vec![[12.0, 0.0], [15.0, 0.0]].into(),
        }))
        .unwrap(),
    );
    let values: Vec<f64> = continued
        .iter()
        .map(|&h| dim(&s, h).measurement())
        .collect();
    assert_eq!(values, vec![2.0, 3.0], "each from the last");

    let err = s
        .run(Command::Dimension(DimSpec::Radius {
            target: t(base),
            point: [0.0, 0.0],
        }))
        .unwrap_err();
    assert!(matches!(err, CoreError::InvalidCommand(_)), "{err:?}");
}

#[test]
fn dim_edit_overrides_flips_and_moves_text_and_rebuilds_the_block() {
    let (dir, mut s, h) = dimensioned("edit.dxf");
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride("<> TYP".to_string()),
    })
    .unwrap();
    assert_eq!(counts(&block_of(&s, h)).2, vec!["10.000 m TYP".to_string()]);
    assert_eq!(dim(&s, h).base().text_override(), Some("<> TYP"));

    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::Flip,
    })
    .unwrap();
    let d = dim(&s, h);
    assert!(d.base().flip_arrow1 && d.base().flip_arrow2);
    // Flipped arrows point in from outside the extension lines.
    let tips_outside = block_of(&s, h).iter().any(|e| match e {
        EntityType::Solid(solid) => solid.second_corner.x < 0.0,
        _ => false,
    });
    assert!(tips_outside);

    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::MoveText([14.0, 2.0]),
    })
    .unwrap();
    let d = dim(&s, h);
    assert!(d.base().text_user_positioned);
    let text_at = block_of(&s, h)
        .iter()
        .find_map(|e| match e {
            EntityType::Text(t) => t.alignment_point,
            _ => None,
        })
        .unwrap();
    assert!((text_at.x - 14.0).abs() < 1e-9);
    // The dimension line runs on under text moved past the extension line.
    let reaches = block_of(&s, h).iter().any(|e| match e {
        EntityType::Line(l) => l.start.x.max(l.end.x) > 12.0 && (l.start.y - 2.0).abs() < 1e-9,
        _ => false,
    });
    assert!(reaches);

    // The override survives a save.
    s.save().unwrap();
    let again = Session::open(&dir.path().join("edit.dxf")).unwrap();
    assert_eq!(dim(&again, h).base().text_override(), Some("<> TYP"));
    assert_eq!(
        counts(&block_of(&again, h)).2,
        vec!["10.000 m TYP".to_string()]
    );

    // Each edit was one step.
    for _ in 0..3 {
        assert!(s.undo());
    }
    assert_eq!(dim(&s, h).base().text_override(), None);
    assert_eq!(counts(&block_of(&s, h)).2, vec!["10.000 m".to_string()]);
}

#[test]
fn find_and_replace_rebuilds_a_dimension_block() {
    let (_dir, mut s, h) = dimensioned("find.dxf");
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride("WALL <>".to_string()),
    })
    .unwrap();
    s.run(Command::FindReplace {
        find: "WALL".to_string(),
        replace: "SLAB".to_string(),
        scope: TextScope::Sheet,
        match_case: true,
        whole_word: false,
    })
    .unwrap();
    assert_eq!(
        counts(&block_of(&s, h)).2,
        vec!["SLAB 10.000 m".to_string()]
    );
}

#[test]
fn arabic_dimension_text_is_kept_in_reading_order() {
    let (_dir, mut s, h) = dimensioned("arabic.dxf");
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride("<> جدار".to_string()),
    })
    .unwrap();
    let text = block_of(&s, h)
        .into_iter()
        .find_map(|e| match e {
            EntityType::Text(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(text.value, "10.000 m جدار");
    assert!(qiyas_geom::is_logical_text(&text.common));
    let glyphs = |s: &Session| {
        let batch = s.batch();
        let laid = batch.texts.iter().find(|t| t.entity == h).unwrap();
        laid.glyphs
            .iter()
            .map(|g| (g.font, g.glyph, (g.x * 1000.0).round() as i64))
            .collect::<Vec<_>>()
    };
    let as_read = glyphs(&s);
    s.run(Command::SetArabicReversed { on: true }).unwrap();
    assert_eq!(
        glyphs(&s),
        as_read,
        "shaped in reading order whatever the setting"
    );
}

#[test]
fn dim_break_space_and_jog_line_change_the_dimensions() {
    let (_dir, mut s, h) = dimensioned("tools.dxf");
    // A wall line crossing the dimension line (y 2) at x 5.
    let cutter = one(s
        .run(Command::Draw(qiyas_core::DrawSpec::Line {
            a: [5.0, 1.0],
            b: [5.0, 3.0],
        }))
        .unwrap());
    let before = counts(&block_of(&s, h)).0;
    s.run(Command::DimBreak {
        target: t(h),
        cutter: t(cutter),
    })
    .unwrap();
    let parts = block_of(&s, h);
    assert_eq!(counts(&parts).0, before + 1, "the dimension line is in two");
    let crosses = parts.iter().any(|e| match e {
        EntityType::Line(l) => {
            (l.start.y - 2.0).abs() < 1e-9
                && l.start.x.min(l.end.x) < 5.0
                && l.start.x.max(l.end.x) > 5.0
        }
        _ => false,
    });
    assert!(!crosses, "a gap where the line crosses");

    let jog = counts(&block_of(&s, h)).0;
    s.run(Command::DimJogLine {
        target: t(h),
        at: [2.0, 2.0],
    })
    .unwrap();
    assert!(counts(&block_of(&s, h)).0 > jog, "a jog symbol in the line");

    // Two more dimensions above the first, unevenly spaced: equal spacing of 1.5.
    let mut next = Vec::new();
    for y in [2.7, 6.0] {
        next.push(one(s
            .run(Command::Dimension(DimSpec::Linear {
                p1: [0.0, 0.0],
                p2: [10.0, 0.0],
                line_point: [5.0, y],
                rotation: None,
            }))
            .unwrap()));
    }
    s.run(Command::DimSpace {
        base: t(h),
        targets: next.iter().map(|&h| t(h)).collect(),
        spacing: Some(1.5),
    })
    .unwrap();
    let ys: Vec<f64> = next
        .iter()
        .map(|&n| match dim(&s, n) {
            Dimension::Linear(d) => d.definition_point.y,
            _ => unreachable!(),
        })
        .collect();
    assert!(
        (ys[0] - 3.5).abs() < 1e-9 && (ys[1] - 5.0).abs() < 1e-9,
        "{ys:?}"
    );
    let line_at = |s: &Session, n: u64, y: f64| {
        block_of(s, n).iter().any(|e| match e {
            EntityType::Line(l) => (l.start.y - y).abs() < 1e-9 && (l.end.y - y).abs() < 1e-9,
            _ => false,
        })
    };
    assert!(line_at(&s, next[1], 5.0), "its block follows");
}

#[test]
fn leaders_multileaders_and_tolerances_are_real_entities() {
    for ext in ["dxf", "dwg"] {
        let (dir, mut s, _) = dimensioned(&format!("notes.{ext}"));
        let leader = added(
            s.run(Command::Leader {
                points: vec![[0.0, 0.0], [2.0, 2.0], [4.0, 2.0]].into(),
                text: "SEE DETAIL".to_string(),
            })
            .unwrap(),
        );
        assert_eq!(leader.len(), 2, "the leader and its text");
        let EntityType::Leader(l) = s.drawing.doc.get_entity(Handle::new(leader[0])).unwrap()
        else {
            panic!()
        };
        assert_eq!(l.vertices.len(), 3);
        assert_eq!(l.creation_type, LeaderCreationType::WithText);
        assert_eq!(l.annotation_handle, Handle::new(leader[1]));
        let EntityType::MText(m) = s.drawing.doc.get_entity(Handle::new(leader[1])).unwrap() else {
            panic!()
        };
        assert_eq!(m.value, "SEE DETAIL");
        assert!((m.height - 0.3).abs() < 1e-12, "the style's text height");
        // (The sample's door sits at (1, 1).)
        assert!(s.pick([1.5, 1.5], 0.05).is_some_and(|p| p.top == leader[0]));

        let mleader = added(
            s.run(Command::MLeader {
                points: vec![[0.0, 10.0], [2.0, 12.0]].into(),
                text: "NOTE".to_string(),
            })
            .unwrap(),
        );
        assert_eq!(mleader.len(), 2);

        let tol = one(s
            .run(Command::Tolerance {
                at: [0.0, 20.0],
                frame: "{\\Fgdt;p}%%v0.5%%vA".to_string(),
            })
            .unwrap());
        assert!(s.pick([0.0, 20.3], 0.05).is_some_and(|p| p.top == tol));

        s.save().unwrap();
        let again = Session::open(&dir.path().join(format!("notes.{ext}"))).unwrap();
        let doc = &again.drawing.doc;
        assert!(
            matches!(doc.get_entity(Handle::new(leader[0])), Some(EntityType::Leader(Leader { vertices, .. })) if vertices.len() == 3),
            "{ext}"
        );
        match doc.get_entity(Handle::new(tol)) {
            Some(EntityType::Tolerance(t)) => assert_eq!(t.text, "{\\Fgdt;p}%%v0.5%%vA"),
            other => panic!("{ext}: {other:?}"),
        }
    }
}

#[test]
fn the_annotation_scale_sizes_new_annotations_and_is_kept() {
    let (dir, mut s) = open(&sample(), "scale.dxf");
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: true,
    })
    .unwrap();
    s.run(Command::AnnotationScale {
        name: "1:50".to_string(),
    })
    .unwrap();
    let info = s.info();
    assert_eq!(info.annotation_scale, "1:50");
    assert!((info.annotation_factor - 50.0).abs() < 1e-12);
    let h = one(s.run(linear_cmd()).unwrap());
    assert!((text_height(&block_of(&s, h)) - 0.3 * 50.0).abs() < 1e-9);
    assert_eq!(
        qiyas_geom::dimscale(&dim(&s, h).base().common),
        Some(50.0),
        "kept on the dimension, so AutoCAD draws it the same"
    );
    let note = added(
        s.run(Command::Leader {
            points: vec![[0.0, 0.0], [2.0, 2.0]].into(),
            text: "A".to_string(),
        })
        .unwrap(),
    );
    let EntityType::MText(m) = s.drawing.doc.get_entity(Handle::new(note[1])).unwrap() else {
        panic!()
    };
    assert!((m.height - 15.0).abs() < 1e-9);

    // An edit keeps the dimension's own scale.
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride("X".to_string()),
    })
    .unwrap();
    assert!((text_height(&block_of(&s, h)) - 15.0).abs() < 1e-9);

    s.save().unwrap();
    let again = Session::open(&dir.path().join("scale.dxf")).unwrap();
    assert_eq!(again.info().annotation_scale, "1:50");
    assert!((text_height(&block_of(&again, h)) - 15.0).abs() < 1e-9);

    // Undone with the drawing; a name that is no scale is refused.
    for _ in 0..4 {
        assert!(s.undo());
    }
    assert_eq!(s.info().annotation_scale, "1:1");
    assert!(s
        .run(Command::AnnotationScale {
            name: "big".to_string()
        })
        .is_err());
}

#[test]
fn dimension_styles_are_created_from_the_current_one_and_edited() {
    let (_dir, mut s) = open(&sample(), "styles.dxf");
    s.run(Command::SetDimStyle {
        name: "QS-50".to_string(),
        fields: DimStyleFields {
            scale: 50.0,
            ..fields(2)
        },
        current: true,
    })
    .unwrap();
    let info = s.info();
    assert_eq!(info.dim_style, "QS-50");
    let style = info.dim_styles.iter().find(|d| d.name == "QS-50").unwrap();
    assert_eq!(style.fields.precision, 2);
    assert!((style.fields.scale - 50.0).abs() < 1e-12);
    assert!(info.dim_styles.iter().any(|d| d.name == "Standard"));
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(dim(&s, h).base().style_name, "QS-50");
    assert_eq!(counts(&block_of(&s, h)).2, vec!["10.00 m".to_string()]);
    assert!((text_height(&block_of(&s, h)) - 15.0).abs() < 1e-9);
    assert!(s.undo() && s.undo());
    assert_eq!(s.info().dim_style, "Standard");
    assert!(s.drawing.doc.dim_styles.get("QS-50").is_none());
    let bad = DimStyleFields {
        arrow_size: -1.0,
        ..fields(2)
    };
    assert!(s
        .run(Command::SetDimStyle {
            name: "X".to_string(),
            fields: bad,
            current: false,
        })
        .is_err());
}

#[test]
fn dimension_commands_read_from_the_wire() {
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "type": "dimension", "kind": "linear",
        "p1": [0.0, 0.0], "p2": [1.0, 0.0], "line_point": [0.5, 1.0]
    }))
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Dimension(DimSpec::Linear { rotation: None, .. })
    ));
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "type": "dim_edit", "target": {"handle": 1}, "op": {"text_override": "A"}
    }))
    .unwrap();
    assert!(matches!(
        cmd,
        Command::DimEdit {
            op: DimEditOp::TextOverride(_),
            ..
        }
    ));
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "type": "dim_edit", "target": {"handle": 1}, "op": "flip"
    }))
    .unwrap();
    assert!(matches!(
        cmd,
        Command::DimEdit {
            op: DimEditOp::Flip,
            ..
        }
    ));
    let cmd: Command = serde_json::from_value(serde_json::json!({
        "type": "mleader", "points": [[0.0, 0.0], [1.0, 1.0]], "text": "A"
    }))
    .unwrap();
    assert!(matches!(cmd, Command::MLeader { .. }));
}

#[test]
fn continued_dimensions_stay_on_the_base_line_and_baselines_step_out_from_it() {
    let (_dir, mut s) = open(&sample(), "chain.dxf");
    let base = one(s
        .run(Command::Dimension(DimSpec::Linear {
            p1: [0.0, 0.0],
            p2: [10.0, 3.0],
            line_point: [5.0, 5.0],
            rotation: Some(0.0),
        }))
        .unwrap());
    let line_y = |s: &Session, h: u64| match dim(s, h) {
        Dimension::Linear(d) => d.definition_point.y,
        other => panic!("{other:?}"),
    };
    let next = added(
        s.run(Command::Dimension(DimSpec::Continue {
            from: t(base),
            points: vec![[14.0, 1.0], [20.0, -2.0]].into(),
        }))
        .unwrap(),
    );
    for &h in &next {
        assert!((line_y(&s, h) - 5.0).abs() < 1e-9, "on the base's line");
    }
    assert!((dim(&s, next[0]).measurement() - 4.0).abs() < 1e-9);
    let step = DimLook::of(&s.drawing.doc, &dim(&s, base)).increment;
    let out = added(
        s.run(Command::Dimension(DimSpec::Baseline {
            from: t(base),
            points: vec![[14.0, 1.0], [20.0, -2.0]].into(),
        }))
        .unwrap(),
    );
    assert!((line_y(&s, out[0]) - (5.0 + step)).abs() < 1e-9);
    assert!((line_y(&s, out[1]) - (5.0 + 2.0 * step)).abs() < 1e-9);
}

#[test]
fn the_annotation_scale_replaces_the_style_scale() {
    let (_dir, mut s) = open(&sample(), "replace.dxf");
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: DimStyleFields {
            scale: 100.0,
            ..fields(3)
        },
        current: true,
    })
    .unwrap();
    // At 1:1 the style's DIMSCALE applies.
    let plain = one(s.run(linear_cmd()).unwrap());
    assert!((text_height(&block_of(&s, plain)) - 0.3 * 100.0).abs() < 1e-9);
    assert_eq!(qiyas_geom::dimscale(&dim(&s, plain).base().common), None);
    s.run(Command::AnnotationScale {
        name: "1:100".to_string(),
    })
    .unwrap();
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(qiyas_geom::dimscale(&dim(&s, h).base().common), Some(100.0));
    assert!(
        (text_height(&block_of(&s, h)) - 0.3 * 100.0).abs() < 1e-9,
        "100, not 10,000"
    );
    let note = added(
        s.run(Command::Leader {
            points: vec![[0.0, 0.0], [2.0, 2.0]].into(),
            text: "A".to_string(),
        })
        .unwrap(),
    );
    let EntityType::MText(m) = s.drawing.doc.get_entity(Handle::new(note[1])).unwrap() else {
        panic!()
    };
    assert!((m.height - 30.0).abs() < 1e-9);
    let tol = one(s
        .run(Command::Tolerance {
            at: [0.0, 20.0],
            frame: "%%v0.1".to_string(),
        })
        .unwrap());
    let EntityType::Tolerance(tol) = s.drawing.doc.get_entity(Handle::new(tol)).unwrap() else {
        panic!()
    };
    assert!((tol.text_height - 30.0).abs() < 1e-9);
}

#[test]
fn editing_a_style_rebuilds_qiyas_dimensions_of_that_style_only() {
    // The existing drawing's dimension has AutoCAD's own block (one line); a Qiyas one is added.
    let mut doc = sample();
    let mut record = BlockRecord::new("*D7");
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    record.flags.anonymous = true;
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    let mut inside = EntityType::Line(Line::from_coords(0.0, 30.0, 0.0, 4.0, 30.0, 0.0));
    inside.common_mut().owner_handle = owner;
    doc.add_entity(inside).unwrap();
    let mut d = DimensionLinear::new(Vector3::new(0.0, 28.0, 0.0), Vector3::new(4.0, 28.0, 0.0));
    d.definition_point = Vector3::new(4.0, 30.0, 0.0);
    d.base.block_name = "*D7".to_string();
    let imported = doc
        .add_entity(EntityType::Dimension(Dimension::Linear(d)))
        .unwrap()
        .value();
    let (_dir, mut s) = open(&doc, "restyle.dxf");
    let ours = one(s.run(linear_cmd()).unwrap());
    let arrow_len = |s: &Session, h: u64| {
        block_of(s, h)
            .iter()
            .find_map(|e| match e {
                EntityType::Solid(solid) => {
                    let (a, b) = (solid.first_corner, solid.second_corner);
                    Some((a.x - b.x).hypot(a.y - b.y))
                }
                _ => None,
            })
            .unwrap()
    };
    let before = arrow_len(&s, ours);
    let theirs = block_of(&s, imported);
    let mut bigger = s
        .info()
        .dim_styles
        .into_iter()
        .find(|d| d.name == "Standard")
        .unwrap()
        .fields;
    bigger.arrow_size *= 3.0;
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: bigger,
        current: false,
    })
    .unwrap();
    assert!(
        arrow_len(&s, ours) > 2.5 * before,
        "rebuilt with the new arrow size"
    );
    assert_eq!(
        block_of(&s, imported),
        theirs,
        "AutoCAD's block is left alone"
    );
    assert!(s.undo());
    assert!((arrow_len(&s, ours) - before).abs() < 1e-12, "one step");
}

#[test]
fn an_ordinate_at_the_datum_reads_0_and_entity_info_shows_symbols() {
    let (_dir, mut s) = open(&sample(), "zero.dxf");
    let h = one(s
        .run(Command::Dimension(DimSpec::Ordinate {
            feature: [0.0, 3.0],
            leader_end: [0.0, 6.0],
        }))
        .unwrap());
    assert_eq!(dim(&s, h).measurement(), 0.0);
    let arc = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Arc(_)))
        .unwrap()
        .common()
        .handle
        .value();
    let d = one(s
        .run(Command::Dimension(DimSpec::Diameter {
            target: t(arc),
            point: [21.0, 1.0],
        }))
        .unwrap());
    assert_eq!(
        s.entity_info(d, &[]).unwrap().text.as_deref(),
        Some("Ø4.00 m")
    );
}

// ---- The unit after the number (DIMPOST, "Show units") ----

/// The sample in `units` (INSUNITS) with a 3-decimal Standard style, the 10 line dimensioned.
fn dimensioned_in(units: i16, name: &str) -> (tempfile::TempDir, Session, u64) {
    let mut doc = sample();
    doc.header.insertion_units = units;
    let (dir, mut s) = open(&doc, name);
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: true,
    })
    .unwrap();
    let h = one(s.run(linear_cmd()).unwrap());
    (dir, s, h)
}

fn text_of(s: &Session, h: u64) -> String {
    let texts = counts(&block_of(s, h)).2;
    assert_eq!(texts.len(), 1, "{texts:?}");
    texts[0].clone()
}

fn own_post(s: &Session, h: u64) -> Option<String> {
    qiyas_geom::dimpost(&dim(s, h).base().common)
}

fn show_units(s: &Session, style: &str) -> bool {
    s.info()
        .dim_styles
        .into_iter()
        .find(|d| d.name == style)
        .unwrap()
        .fields
        .show_units
}

#[test]
fn a_new_dimension_shows_the_drawings_unit_after_the_number() {
    // INSUNITS: 1 inches, 2 feet, 4 mm, 5 cm, 6 m; anything else (0, 9 ...) is read as mm.
    for (units, text, post) in [
        (6, "10.000 m", "<> m"),
        (5, "10.000 cm", "<> cm"),
        (4, "10.000 mm", "<> mm"),
        (1, "10.000\"", "<>\""),
        (2, "10.000'", "<>'"),
        (0, "10.000 mm", "<> mm"),
    ] {
        let (_dir, s, h) = dimensioned_in(units, "units.dxf");
        assert_eq!(text_of(&s, h), text, "INSUNITS {units}");
        // AutoCAD's way: the dimension's own DIMPOST override, the style untouched.
        assert_eq!(own_post(&s, h).as_deref(), Some(post), "INSUNITS {units}");
        assert!(
            !s.drawing
                .doc
                .dim_styles
                .get("Standard")
                .unwrap()
                .dimpost
                .contains(' '),
            "the style itself is untouched"
        );
    }
}

#[test]
fn the_unit_goes_with_the_other_kinds_but_not_with_angles() {
    let (_dir, mut s, _) = dimensioned_in(6, "kinds.dxf");
    let aligned = one(s
        .run(Command::Dimension(DimSpec::Aligned {
            p1: [0.0, 0.0],
            p2: [3.0, 4.0],
            offset: 1.0,
        }))
        .unwrap());
    assert_eq!(text_of(&s, aligned), "5.000 m");
    let angular = one(s
        .run(Command::Dimension(DimSpec::Angular {
            vertex: [0.0, 0.0],
            p1: [4.0, 0.0],
            p2: [0.0, 4.0],
            arc_point: [2.0, 2.0],
        }))
        .unwrap());
    let angle = text_of(&s, angular);
    assert!(angle.starts_with("90") && angle.ends_with("%%d"), "{angle}");
    assert_eq!(own_post(&s, angular), None, "an angle has no length unit");
}

#[test]
fn a_style_with_its_own_suffix_is_left_alone() {
    let mut doc = sample();
    doc.dim_styles.get_mut("Standard").unwrap().dimpost = "<> (net)".to_string();
    let (_dir, mut s) = open(&doc, "own.dxf");
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: true,
    })
    .unwrap();
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(text_of(&s, h), "10.000 (net)");
    assert_eq!(own_post(&s, h), None, "no override on top of the style's");
    // Editing the style keeps it that way.
    let mut bigger = fields(3);
    bigger.arrow_size = 0.5;
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: bigger,
        current: false,
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "10.000 (net)");
    assert_eq!(own_post(&s, h), None);
}

#[test]
fn the_show_units_switch_turns_the_suffix_off_and_on_and_is_one_undo_step() {
    let (_dir, mut s, h) = dimensioned_in(6, "switch.dxf");
    assert!(show_units(&s, "Standard"), "on by default");
    let mut off = fields(3);
    off.show_units = false;
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: off,
        current: false,
    })
    .unwrap();
    assert!(!show_units(&s, "Standard"));
    // Only the switch changed, and the dimension of that style is rebuilt without the unit.
    assert_eq!(text_of(&s, h), "10.000");
    assert_eq!(own_post(&s, h), None);
    let plain = one(s.run(linear_cmd()).unwrap());
    assert_eq!(text_of(&s, plain), "10.000", "new ones follow the switch");
    assert_eq!(own_post(&s, plain), None);

    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: false,
    })
    .unwrap();
    assert!(show_units(&s, "Standard"));
    assert_eq!(text_of(&s, h), "10.000 m");
    assert_eq!(text_of(&s, plain), "10.000 m");

    // Undo puts the switch and the text back, in one step each.
    assert!(s.undo());
    assert!(!show_units(&s, "Standard"));
    assert_eq!(text_of(&s, h), "10.000");
    assert!(s.undo() && s.undo());
    assert!(show_units(&s, "Standard"));
    assert_eq!(text_of(&s, h), "10.000 m");
}

#[test]
fn a_suffix_that_is_not_ours_is_never_taken_off_or_replaced() {
    let (_dir, mut s, h) = dimensioned_in(6, "foreign.dxf");
    // Somebody (AutoCAD) gave this dimension its own suffix.
    if let Some(EntityType::Dimension(d)) = s.drawing.doc.get_entity_mut(Handle::new(h)) {
        qiyas_geom::set_dimpost(&mut d.base_mut().common, "<> TYP");
    }
    let mut off = fields(3);
    off.show_units = false;
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: off,
        current: false,
    })
    .unwrap();
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(3),
        current: false,
    })
    .unwrap();
    assert_eq!(own_post(&s, h).as_deref(), Some("<> TYP"));
    assert_eq!(text_of(&s, h), "10.000 TYP");
}

#[test]
fn a_dimension_style_in_other_numbers_gets_no_unit() {
    // DIMLFAC 1000 turns metres into millimetres: the drawing's unit would be wrong.
    let mut doc = sample();
    doc.dim_styles.get_mut("Standard").unwrap().dimlfac = 1000.0;
    let (_dir, mut s) = open(&doc, "lfac.dxf");
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(own_post(&s, h), None);
    assert!(!text_of(&s, h).contains(' '), "{}", text_of(&s, h));
}

#[test]
fn a_dimension_on_a_layout_gets_no_drawing_unit() {
    // Paper space is measured in paper units, not the drawing's metres.
    let (_dir, mut s, model) = dimensioned_in(6, "paper.dxf");
    assert_eq!(text_of(&s, model), "10.000 m");
    let paper = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&paper.block).unwrap();
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(own_post(&s, h), None);
    assert_eq!(text_of(&s, h), "10.000");
}

#[test]
fn a_dimension_given_another_style_takes_that_styles_unit_rule() {
    let mut doc = sample();
    let mut lfac = doc.dim_styles.get("Standard").unwrap().clone();
    lfac.name = "MM".to_string();
    lfac.handle = doc.allocate_handle();
    lfac.dimlfac = 1000.0;
    doc.dim_styles.add(lfac).unwrap();
    let (_dir, mut s) = open(&doc, "restyle.dxf");
    let h = one(s.run(linear_cmd()).unwrap());
    assert_eq!(own_post(&s, h).as_deref(), Some("<> m"));
    // Metres shown as millimetres: the drawing's unit would be wrong, so it goes.
    s.run(Command::SetProperty {
        target: t(h),
        prop: qiyas_core::EntityProp::DimStyle,
        value: serde_json::json!("MM"),
    })
    .unwrap();
    assert_eq!(own_post(&s, h), None);
    assert!(!text_of(&s, h).contains(' '), "{}", text_of(&s, h));
    // And back: Standard shows the unit again.
    s.run(Command::SetProperty {
        target: t(h),
        prop: qiyas_core::EntityProp::DimStyle,
        value: serde_json::json!("Standard"),
    })
    .unwrap();
    assert_eq!(own_post(&s, h).as_deref(), Some("<> m"));
}

#[test]
fn rebuilt_dimensions_keep_the_suffix() {
    let (_dir, mut s, h) = dimensioned_in(6, "rebuilt.dxf");
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride("WALL <>".to_string()),
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "WALL 10.000 m");
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::Flip,
    })
    .unwrap();
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::MoveText([14.0, 2.0]),
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "WALL 10.000 m");
    s.run(Command::FindReplace {
        find: "WALL".to_string(),
        replace: "SLAB".to_string(),
        scope: TextScope::Sheet,
        match_case: true,
        whole_word: false,
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "SLAB 10.000 m");
    // Back to the measured text.
    s.run(Command::DimEdit {
        target: t(h),
        op: DimEditOp::TextOverride(String::new()),
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "10.000 m");
    // A style edit that changes the precision rebuilds with the suffix too.
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(1),
        current: false,
    })
    .unwrap();
    assert_eq!(text_of(&s, h), "10.0 m");
}

#[test]
fn an_autocad_dimension_gets_no_suffix_when_it_is_edited() {
    // A dimension with no block (AutoCAD's own, as the file has it) and no suffix of its own.
    let mut doc = sample();
    let mut d = DimensionLinear::new(Vector3::new(0.0, 28.0, 0.0), Vector3::new(4.0, 28.0, 0.0));
    d.definition_point = Vector3::new(4.0, 30.0, 0.0);
    let imported = doc
        .add_entity(EntityType::Dimension(Dimension::Linear(d)))
        .unwrap()
        .value();
    let (_dir, mut s) = open(&doc, "theirs.dxf");
    s.run(Command::DimEdit {
        target: t(imported),
        op: DimEditOp::Flip,
    })
    .unwrap();
    assert_eq!(text_of(&s, imported), "4.00");
    assert_eq!(own_post(&s, imported), None);
    // Nor does editing the style (its block is not Qiyas's).
    s.run(Command::SetDimStyle {
        name: "Standard".to_string(),
        fields: fields(2),
        current: false,
    })
    .unwrap();
    assert_eq!(own_post(&s, imported), None);
}

#[test]
fn the_suffix_and_the_switch_survive_a_save_in_dxf_and_dwg() {
    for ext in ["dxf", "dwg"] {
        let (dir, mut s, h) = dimensioned_in(6, &format!("keep.{ext}"));
        s.save().unwrap();
        let mut again = Session::open(&dir.path().join(format!("keep.{ext}"))).unwrap();
        assert_eq!(text_of(&again, h), "10.000 m", "{ext}: the block's text");
        assert_eq!(own_post(&again, h).as_deref(), Some("<> m"), "{ext}");
        // Edited after reopening, still the same.
        again
            .run(Command::DimEdit {
                target: t(h),
                op: DimEditOp::Flip,
            })
            .unwrap();
        assert_eq!(text_of(&again, h), "10.000 m", "{ext}");
        assert!(show_units(&again, "Standard"), "{ext}");

        // Switched off and saved: the switch and the plain text come back.
        let mut off = fields(3);
        off.show_units = false;
        again
            .run(Command::SetDimStyle {
                name: "Standard".to_string(),
                fields: off,
                current: false,
            })
            .unwrap();
        assert_eq!(own_post(&again, h), None, "{ext}: off in memory");
        again.save().unwrap();
        let off_again = Session::open(&dir.path().join(format!("keep.{ext}"))).unwrap();
        assert!(!show_units(&off_again, "Standard"), "{ext}");
        assert_eq!(text_of(&off_again, h), "10.000", "{ext}");
        assert_eq!(own_post(&off_again, h), None, "{ext}");
    }
}

#[test]
fn the_style_wire_format_takes_show_units_and_defaults_it_on() {
    let old = serde_json::json!({
        "type": "set_dim_style",
        "name": "Standard",
        "fields": {
            "arrow_size": 0.25, "text_height": 0.3, "precision": 2, "ext_offset": 0.1,
            "ext_extension": 0.2, "gap": 0.05, "scale": 1.0
        }
    });
    let Command::SetDimStyle { fields, .. } = serde_json::from_value::<Command>(old).unwrap()
    else {
        panic!("a style command")
    };
    assert!(fields.show_units, "an older caller gets the units");
    let json = serde_json::to_value(fields).unwrap();
    assert_eq!(json["show_units"], true);
}

#[test]
fn an_autocad_suffix_after_other_overrides_is_still_theirs() {
    use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
    let (_dir, mut s, h) = dimensioned_in(6, "dimtad.dxf");
    // DIMTAD 3 first (its value 3 is also DIMPOST's code), then AutoCAD's own suffix.
    let mut record = ExtendedDataRecord::new("ACAD");
    for v in [
        XDataValue::String("DSTYLE".to_string()),
        XDataValue::ControlString("{".to_string()),
        XDataValue::Integer16(271),
        XDataValue::Integer16(3),
        XDataValue::Integer16(3),
        XDataValue::String("<> ft".to_string()),
        XDataValue::ControlString("}".to_string()),
    ] {
        record.add_value(v);
    }
    if let Some(EntityType::Dimension(d)) = s.drawing.doc.get_entity_mut(Handle::new(h)) {
        d.base_mut().common.extended_data.upsert_record(record);
    }
    for show in [false, true] {
        let mut f = fields(3);
        f.show_units = show;
        s.run(Command::SetDimStyle {
            name: "Standard".to_string(),
            fields: f,
            current: false,
        })
        .unwrap();
        assert_eq!(own_post(&s, h).as_deref(), Some("<> ft"), "show {show}");
        assert_eq!(text_of(&s, h), "10.000 ft", "show {show}");
    }
}
