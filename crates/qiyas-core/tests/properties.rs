//! Editing an entity's properties (the properties palette) and matching one entity's properties
//! onto others. One undo step each; the take-off follows a change of shape.

use opencadcodec::entities::{Circle, Dimension};
use opencadcodec::{CadDocument, Color, EntityType, Handle, LineType};
use qiyas_core::fixtures::{sample_doc, schedule_doc, write_drawing, write_xref_pair};
use qiyas_core::{
    Command, CoreError, DimSpec, DrawSpec, EntityProp, MatchProp, Outcome, Session, Target,
};
use serde_json::{json, Value};

const MODEL: &str = "*Model_Space";

fn open(doc: &CadDocument) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join("plan.dxf"));
    let s = Session::open(&path).unwrap();
    (dir, s)
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn entity(s: &Session, h: u64) -> EntityType {
    s.drawing.doc.get_entity(Handle::new(h)).cloned().unwrap()
}

fn set(s: &mut Session, h: u64, prop: EntityProp, value: Value) {
    s.run(Command::SetProperty {
        target: t(h),
        prop,
        value,
    })
    .unwrap();
}

fn refused(s: &mut Session, cmd: Command) -> String {
    let undo = s.can_undo();
    let err = s.run(cmd).unwrap_err();
    assert_eq!(s.can_undo(), undo, "a refused command is no step");
    match err {
        CoreError::InvalidCommand(why) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// The first model space entity of `kind` ("Line", "Arc"…).
fn first(s: &Session, kind: &str) -> u64 {
    s.drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| format!("{e:?}").starts_with(kind))
        .unwrap()
        .common()
        .handle
        .value()
}

fn added(outcome: Outcome) -> u64 {
    match outcome {
        Outcome::Added { handles } => handles[0],
        other => panic!("{other:?}"),
    }
}

fn circle(s: &mut Session) -> u64 {
    added(
        s.run(Command::Draw(DrawSpec::Circle {
            centre: [50.0, 50.0],
            radius: 2.0,
        }))
        .unwrap(),
    )
}

/// The batch's extent of what entity `h` draws.
fn drawn_width(s: &Session, h: u64) -> f64 {
    let batch = s.batch();
    let mut xs = Vec::new();
    for r in batch.ranges.iter().filter(|r| r.top == h) {
        for i in r.first..r.first + r.count {
            let seg = &batch.segments[4 * i as usize..4 * i as usize + 4];
            xs.push(f64::from(seg[0]));
            xs.push(f64::from(seg[2]));
        }
    }
    let lo = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    hi - lo
}

#[test]
fn set_radius_changes_the_circle_the_batch_and_the_takeoff() {
    let (_dir, mut s) = open(&sample_doc());
    let c = circle(&mut s);
    s.run(Command::MeasureEntity {
        handle: c,
        chain: vec![],
    })
    .unwrap();
    let before = s.takeoff.items[0].value;
    assert!((drawn_width(&s, c) - 4.0).abs() < 0.1);

    set(&mut s, c, EntityProp::Radius, json!(3.0));
    match entity(&s, c) {
        EntityType::Circle(Circle { radius, .. }) => assert_eq!(radius, 3.0),
        other => panic!("{other:?}"),
    }
    assert!((drawn_width(&s, c) - 6.0).abs() < 0.1);
    let after = s.takeoff.items[0].value;
    assert!(
        (after / before - 9.0 / 4.0).abs() < 1e-6,
        "{before} {after}"
    );

    refused(
        &mut s,
        Command::SetProperty {
            target: t(c),
            prop: EntityProp::Radius,
            value: json!(0),
        },
    );
    // A line has no radius.
    let line = first(&s, "Line");
    refused(
        &mut s,
        Command::SetProperty {
            target: t(line),
            prop: EntityProp::Radius,
            value: json!(1),
        },
    );
    assert!(s.undo());
    assert_eq!(s.takeoff.items[0].value, before);
}

#[test]
fn the_general_properties_of_a_line() {
    let mut doc = sample_doc();
    let mut dashed = LineType::new("DASHED");
    dashed.handle = doc.allocate_handle();
    doc.line_types.add(dashed).unwrap();
    let (_dir, mut s) = open(&doc);
    let line = first(&s, "Line");
    s.run(Command::MeasureEntity {
        handle: line,
        chain: vec![],
    })
    .unwrap();

    set(&mut s, line, EntityProp::Layer, json!("a-door"));
    set(&mut s, line, EntityProp::Color, json!("ACI 3"));
    set(&mut s, line, EntityProp::Linetype, json!("dashed"));
    set(&mut s, line, EntityProp::Lineweight, json!(25));
    let c = entity(&s, line);
    let common = c.common();
    assert_eq!(common.layer, "A-DOOR");
    assert_eq!(common.color, Color::Index(3));
    assert_eq!(common.linetype, "DASHED");
    assert_eq!(common.line_weight.value(), 25);
    // The row measuring the line says where it is now.
    assert_eq!(s.takeoff.items[0].layer, "A-DOOR");
    let info = s.entity_info(line, &[]).unwrap();
    assert_eq!(info.color, "ACI 3");

    set(&mut s, line, EntityProp::Linetype, json!("ByLayer"));
    assert_eq!(entity(&s, line).common().linetype, "");
    for (prop, value) in [
        (EntityProp::Layer, json!("NOPE")),
        (EntityProp::Color, json!("not-a-color")),
        (EntityProp::Lineweight, json!(3)),
        (EntityProp::Text, json!("x")),
    ] {
        refused(
            &mut s,
            Command::SetProperty {
                target: t(line),
                prop,
                value,
            },
        );
    }
    for _ in 0..5 {
        assert!(s.undo());
    }
    assert_eq!(entity(&s, line).common().layer, "A-WALL");
    assert_eq!(s.takeoff.items[0].layer, "A-WALL");
}

#[test]
fn texts_polylines_and_block_references() {
    let (_dir, mut s) = open(&sample_doc());
    let text = first(&s, "Text");
    set(&mut s, text, EntityProp::Text, json!("FIRST FLOOR"));
    set(&mut s, text, EntityProp::TextHeight, json!(0.75));
    set(&mut s, text, EntityProp::TextRotation, json!(90));
    match entity(&s, text) {
        EntityType::Text(t) => {
            assert_eq!(t.value, "FIRST FLOOR");
            assert_eq!(t.height, 0.75);
            assert!((t.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        }
        other => panic!("{other:?}"),
    }
    // The palette shows what it edits, in degrees.
    let info = s.entity_info(text, &[]).unwrap();
    assert!((info.rotation.unwrap() - 90.0).abs() < 1e-9);

    // Opening the rectangle: its area row can't be measured any more and keeps its value.
    let rect = first(&s, "LwPolyline");
    s.run(Command::MeasureEntity {
        handle: rect,
        chain: vec![],
    })
    .unwrap();
    set(&mut s, rect, EntityProp::Closed, json!(false));
    set(&mut s, rect, EntityProp::GlobalWidth, json!(0.2));
    match entity(&s, rect) {
        EntityType::LwPolyline(p) => {
            assert!(!p.is_closed);
            assert_eq!(p.constant_width, 0.2);
        }
        other => panic!("{other:?}"),
    }
    let info = s.entity_info(rect, &[]).unwrap();
    assert_eq!((info.closed, info.global_width), (Some(false), Some(0.2)));
    assert!(s.takeoff.items[0].handles.is_empty(), "unlinked");
    assert_eq!(s.takeoff.items[0].value, 12.0);

    let door = first(&s, "Insert");
    set(&mut s, door, EntityProp::InsertScale, json!(3));
    set(&mut s, door, EntityProp::InsertRotation, json!(45));
    match entity(&s, door) {
        EntityType::Insert(i) => {
            assert_eq!(i.x_scale(), 3.0);
            assert_eq!(i.y_scale(), 3.0);
            assert!((i.rotation - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        }
        other => panic!("{other:?}"),
    }
    let info = s.entity_info(door, &[]).unwrap();
    assert_eq!(info.scale, Some(3.0));
    assert!((info.rotation.unwrap() - 45.0).abs() < 1e-9);
    refused(
        &mut s,
        Command::SetProperty {
            target: t(door),
            prop: EntityProp::InsertScale,
            value: json!("big"),
        },
    );
}

#[test]
fn a_block_references_attributes_follow_its_scale_and_turn() {
    let (_dir, mut s) = open(&schedule_doc());
    let tag = first(&s, "Insert");
    let attribute = |s: &Session| match entity(s, tag) {
        EntityType::Insert(i) => {
            let a = &i.attributes[0];
            (
                [a.insertion_point.x, a.insertion_point.y],
                a.height,
                i.x_scale(),
            )
        }
        other => panic!("{other:?}"),
    };
    // MARK sits 0.5 right of the insertion point (30, 10), 0.2 tall.
    set(&mut s, tag, EntityProp::InsertRotation, json!(90));
    let (at, height, _) = attribute(&s);
    assert!(
        (at[0] - 30.0).abs() < 1e-9 && (at[1] - 10.5).abs() < 1e-9,
        "{at:?}"
    );
    set(&mut s, tag, EntityProp::InsertScale, json!(2));
    let (at, height2, scale) = attribute(&s);
    assert!(
        (at[0] - 30.0).abs() < 1e-9 && (at[1] - 11.0).abs() < 1e-9,
        "{at:?}"
    );
    assert!((height2 - 2.0 * height).abs() < 1e-9);
    assert_eq!(scale, 2.0);
    // A mirrored reference stays mirrored: the palette shows the signed scale.
    s.run(Command::Modify(qiyas_core::ModifySpec::Mirror {
        targets: vec![t(tag)],
        p1: [30.0, 0.0],
        p2: [30.0, 1.0],
        keep: false,
    }))
    .unwrap();
    let mirrored = s.entity_info(tag, &[]).unwrap().scale.unwrap();
    assert!(mirrored < 0.0, "{mirrored}");
    set(&mut s, tag, EntityProp::InsertScale, json!(3));
    assert!((s.entity_info(tag, &[]).unwrap().scale.unwrap() + 3.0).abs() < 1e-9);
    refused(
        &mut s,
        Command::SetProperty {
            target: t(tag),
            prop: EntityProp::InsertScale,
            value: json!(0),
        },
    );
}

#[test]
fn a_reference_seen_from_below_turns_to_the_angle_typed() {
    // A TAG reference with a -Z normal (as mirrored drawings store them): its OCS x runs along
    // world -x, so a turn in its own angle is the opposite turn on the plan.
    let mut doc = schedule_doc();
    let tag = doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle;
    if let Some(EntityType::Insert(i)) = doc.get_entity_mut(tag) {
        let down = opencadcodec::Vector3::new(0.0, 0.0, -1.0);
        i.normal = down;
        i.insert_point = opencadcodec::Vector3::new(-30.0, 10.0, 0.0);
        // MARK 0.5 along the reference's x, on the plan (its own normal up).
        i.attributes[0].insertion_point = opencadcodec::Vector3::new(29.5, 10.0, 0.0);
    }
    let (_dir, mut s) = open(&doc);
    let tag = tag.value();
    set(&mut s, tag, EntityProp::InsertRotation, json!(90));
    match entity(&s, tag) {
        EntityType::Insert(i) => {
            assert!(
                (i.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
                "{}",
                i.rotation
            );
            // The reference stays where it was; MARK turns with it: 0.5 along its y now.
            assert_eq!([i.insert_point.x, i.insert_point.y], [-30.0, 10.0]);
            let p = i.attributes[0].insertion_point;
            assert!(
                (p.x - 30.0).abs() < 1e-9 && (p.y - 10.5).abs() < 1e-9,
                "{p:?}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert!((s.entity_info(tag, &[]).unwrap().rotation.unwrap() - 90.0).abs() < 1e-9);
}

#[test]
fn an_xrefs_layer_is_never_given_to_an_entity() {
    let mut doc = sample_doc();
    let mut theirs = opencadcodec::Layer::new("PLAN|A-WALL");
    theirs.handle = doc.allocate_handle();
    theirs.flags.xref_dependent = true;
    doc.layers.add(theirs).unwrap();
    let (_dir, mut s) = open(&doc);
    let line = first(&s, "Line");
    let why = refused(
        &mut s,
        Command::SetProperty {
            target: t(line),
            prop: EntityProp::Layer,
            value: json!("PLAN|A-WALL"),
        },
    );
    assert!(why.contains("xref"), "{why}");
}

#[test]
fn a_dimensions_text_height_draws_its_block_again() {
    let (_dir, mut s) = open(&sample_doc());
    let dim = added(
        s.run(Command::Dimension(DimSpec::Linear {
            p1: [0.0, 0.0],
            p2: [10.0, 0.0],
            line_point: [5.0, 2.0],
            rotation: None,
        }))
        .unwrap(),
    );
    let block_text_height = |s: &Session| {
        let EntityType::Dimension(d) = entity(s, dim) else {
            panic!("a dimension")
        };
        let name = d.base().block_name.clone();
        s.drawing
            .doc
            .entities_in_block(&name)
            .find_map(|e| match e {
                EntityType::Text(t) => Some(t.height),
                _ => None,
            })
            .unwrap()
    };
    let before = block_text_height(&s);
    set(&mut s, dim, EntityProp::TextHeight, json!(before * 4.0));
    assert!((block_text_height(&s) - before * 4.0).abs() < 1e-9);
    assert_eq!(
        s.entity_info(dim, &[]).unwrap().height,
        Some(before * 4.0),
        "the palette shows it"
    );
    assert!(matches!(
        entity(&s, dim),
        EntityType::Dimension(Dimension::Linear(_))
    ));
    assert!(s.undo());
    assert!((block_text_height(&s) - before).abs() < 1e-9);
}

#[test]
fn match_properties_copies_layer_and_colour() {
    let (_dir, mut s) = open(&sample_doc());
    let door = first(&s, "Insert");
    let line = first(&s, "Line");
    let arc = first(&s, "Arc");
    set(&mut s, door, EntityProp::Color, json!("#ff8000"));
    s.run(Command::MatchProperties {
        source: t(door),
        targets: vec![t(line), t(arc)],
        props: vec![MatchProp::Layer, MatchProp::Color],
    })
    .unwrap();
    for h in [line, arc] {
        let e = entity(&s, h);
        assert_eq!(e.common().layer, "A-DOOR");
        assert_eq!(
            e.common().color,
            Color::Rgb {
                r: 0xff,
                g: 0x80,
                b: 0
            }
        );
    }
    // One step.
    assert!(s.undo());
    assert_eq!(entity(&s, line).common().layer, "A-WALL");
    assert_eq!(entity(&s, arc).common().layer, "A-WALL");

    // Text height goes text to text only; a line takes the layer and leaves the rest.
    let text = first(&s, "Text");
    let other = added(
        s.run(Command::Text {
            at: [0.0, 20.0],
            height: 2.0,
            rotation: 0.0,
            value: "B".into(),
            style: None,
        })
        .unwrap(),
    );
    s.run(Command::MatchProperties {
        source: t(text),
        targets: vec![t(other), t(line)],
        props: vec![MatchProp::TextHeight, MatchProp::Layer],
    })
    .unwrap();
    match entity(&s, other) {
        EntityType::Text(t) => assert_eq!(t.height, 0.5),
        e => panic!("{e:?}"),
    }
    assert_eq!(entity(&s, line).common().layer, "0");
    refused(
        &mut s,
        Command::MatchProperties {
            source: t(text),
            targets: vec![],
            props: vec![MatchProp::Layer],
        },
    );
}

#[test]
fn an_xrefs_entity_is_never_edited_from_the_palette() {
    let dir = tempfile::tempdir().unwrap();
    let host = write_xref_pair(dir.path());
    let mut s = Session::open(&host).unwrap();
    let xref = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value();
    let inside = s.xrefs.entries[0]
        .drawing
        .as_ref()
        .unwrap()
        .doc
        .entities_in_block(MODEL)
        .next()
        .unwrap()
        .common()
        .handle
        .value();
    let why = refused(
        &mut s,
        Command::SetProperty {
            target: Target {
                handle: inside,
                chain: vec![xref],
            },
            prop: EntityProp::Color,
            value: json!("ACI 1"),
        },
    );
    assert!(why.contains("read-only"), "{why}");
    // Nor can one be matched onto.
    let own = first(&s, "Line");
    let why = refused(
        &mut s,
        Command::MatchProperties {
            source: t(own),
            targets: vec![Target {
                handle: inside,
                chain: vec![xref],
            }],
            props: vec![MatchProp::Color],
        },
    );
    assert!(why.contains("read-only"), "{why}");
    // Matching from it: its colour, never its layer (an xref's).
    s.run(Command::MatchProperties {
        source: Target {
            handle: inside,
            chain: vec![xref],
        },
        targets: vec![t(own)],
        props: vec![MatchProp::Color],
    })
    .unwrap();
    let why = refused(
        &mut s,
        Command::MatchProperties {
            source: Target {
                handle: inside,
                chain: vec![xref],
            },
            targets: vec![t(own)],
            props: vec![MatchProp::Layer],
        },
    );
    assert!(why.contains("xref"), "{why}");
}
