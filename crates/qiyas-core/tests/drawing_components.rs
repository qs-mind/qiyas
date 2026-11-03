//! Drawing-local component facts use no installed library or remembered state.
use opencadcodec::entities::{AttributeDefinition, AttributeEntity, Insert, MText, Text};
use opencadcodec::{BlockRecord, EntityType, Handle, Line, Vector3};
use qiyas_core::records::{Collection, RecordQuery, Where, WhereOp, MAX_BYTES};
use qiyas_core::Session;
use serde_json::{json, Value};

fn session(units: i16) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_blank(&dir.path().join("airport-components.dxf"), units);
    (dir, s)
}

fn query() -> RecordQuery {
    RecordQuery {
        collection: Collection::Blocks,
        conditions: vec![Where {
            path: "layout".into(),
            op: WhereOp::Eq,
            value: json!(false),
        }],
        ..Default::default()
    }
}

fn block(s: &mut Session, name: &str, units: i16, scale: f64) -> Handle {
    let doc = &mut s.drawing.doc;
    let mut b = BlockRecord::new(name);
    b.handle = doc.allocate_handle();
    b.units = units;
    b.base_point = Vector3::new(2.0 * scale, 3.0 * scale, 0.0);
    let owner = b.handle;
    doc.block_records.add(b).unwrap();
    let mut line = Line::from_points(
        Vector3::new(2.0 * scale, 3.0 * scale, 0.0),
        Vector3::new(3.2 * scale, 5.4 * scale, 0.0),
    );
    line.common.owner_handle = owner;
    doc.add_entity(EntityType::Line(line)).unwrap();
    owner
}

fn insert(s: &mut Session, name: &str, owner: Handle, columns: u16, rows: u16) {
    let mut i = Insert::new(name, Vector3::ZERO);
    i.common.owner_handle = owner;
    i.column_count = columns;
    i.row_count = rows;
    i.column_spacing = 10.0;
    i.row_spacing = 20.0;
    s.drawing.doc.add_entity(EntityType::Insert(i)).unwrap();
}

fn record(s: &Session, name: &str) -> Value {
    s.records(&RecordQuery {
        names: vec![name.into()],
        ..query()
    })
    .unwrap()
    .records
    .remove(0)
}

#[test]
fn millimetres_and_metres_describe_the_same_physical_component() {
    for (drawing_units, block_units, scale) in
        [(4, 4, 1000.0), (6, 6, 1.0), (6, 4, 1000.0), (4, 0, 1000.0)]
    {
        let (_dir, mut s) = session(drawing_units);
        block(&mut s, "TERMINAL-PANEL", block_units, scale);
        let r = record(&s, "TERMINAL-PANEL");
        assert_eq!(r["base_point"], json!([2.0 * scale, 3.0 * scale, 0.0]));
        assert_eq!(r["bounds_m"], json!([[2.0, 3.0], [3.2, 5.4]]));
        let size = r["size_m"].as_array().unwrap();
        assert!((size[0].as_f64().unwrap() - 1.2).abs() < 1e-9);
        assert!((size[1].as_f64().unwrap() - 2.4).abs() < 1e-9);
        assert_eq!(r["bounds_complete"], true);
        assert_eq!(r["bounds_kind"], "conservative");
    }
}

#[test]
fn tags_and_stored_nested_insert_counts_do_not_expand_parent_instances() {
    let (_dir, mut s) = session(6);
    let panel = block(&mut s, "TERMINAL-PANEL", 6, 1.0);
    let assembly = block(&mut s, "CONCOURSE-ASSEMBLY", 6, 1.0);
    let model = s
        .drawing
        .doc
        .block_records
        .get("*Model_Space")
        .unwrap()
        .handle;
    let mut att = AttributeDefinition::new("MARK".into(), "Panel mark".into(), "P01".into());
    att.common.owner_handle = panel;
    s.drawing
        .doc
        .add_entity(EntityType::AttributeDefinition(att))
        .unwrap();
    insert(&mut s, "TERMINAL-PANEL", assembly, 2, 3);
    insert(&mut s, "TERMINAL-PANEL", model, 1, 1);
    insert(&mut s, "CONCOURSE-ASSEMBLY", model, 4, 1);
    let r = record(&s, "TERMINAL-PANEL");
    assert_eq!(r["attribute_tags"], json!(["MARK"]));
    assert_eq!(
        r["insert_count"], 7,
        "six stored nested array cells and one direct insertion"
    );
    assert_eq!(record(&s, "CONCOURSE-ASSEMBLY")["insert_count"], 4);
}

#[test]
fn nested_array_bounds_include_base_points_and_extreme_cells() {
    let (_dir, mut s) = session(6);
    block(&mut s, "TERMINAL-PANEL", 6, 1.0);
    let assembly = block(&mut s, "CONCOURSE-ASSEMBLY", 6, 1.0);
    insert(&mut s, "TERMINAL-PANEL", assembly, 30000, 30000);
    let r = record(&s, "CONCOURSE-ASSEMBLY");
    assert_eq!(r["bounds_m"][0], json!([0.0, 0.0]));
    assert!((r["bounds_m"][1][0].as_f64().unwrap() - 299991.2).abs() < 1e-8);
    assert!((r["bounds_m"][1][1].as_f64().unwrap() - 599982.4).abs() < 1e-8);
    assert_eq!(record(&s, "TERMINAL-PANEL")["insert_count"], 900000000u64);
}

#[test]
fn blank_install_has_no_components_and_reads_do_not_change_drawing_or_history() {
    let (_dir, mut s) = session(6);
    assert_eq!(s.records(&query()).unwrap().total, 0);
    let before = s.revision();
    assert!(!s.is_dirty());
    assert!(!s.can_undo());
    for _ in 0..3 {
        assert!(s.records(&query()).unwrap().records.is_empty());
    }
    assert_eq!(s.revision(), before);
    assert!(!s.is_dirty());
    assert!(!s.undo());
    block(&mut s, "TERMINAL-PANEL", 6, 1.0);
    let before = s.revision();
    let entities = s.drawing.doc.entity_count();
    record(&s, "TERMINAL-PANEL");
    assert_eq!(s.revision(), before);
    assert_eq!(s.drawing.doc.entity_count(), entities);
    assert!(!s.undo());
}

#[test]
fn components_remain_paged_under_the_existing_byte_budget() {
    let (_dir, mut s) = session(6);
    for n in 0..250 {
        block(&mut s, &format!("CONCOURSE-{n:03}-PANEL"), 6, 1.0);
    }
    let mut offset = 0;
    let mut keys = std::collections::BTreeSet::new();
    loop {
        let page = s
            .records(&RecordQuery {
                offset,
                limit: Some(200),
                ..query()
            })
            .unwrap();
        assert_eq!(page.total, 250);
        assert!(serde_json::to_vec(&page.records).unwrap().len() <= MAX_BYTES);
        for r in &page.records {
            assert!(keys.insert(r["key"].as_str().unwrap().to_owned()));
        }
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(keys.len(), 250);
}

#[test]
fn cyclic_definitions_report_unavailable_size_instead_of_partial_bounds() {
    let (_dir, mut s) = session(6);
    let owner = block(&mut s, "CYCLIC-ASSEMBLY", 6, 1.0);
    insert(&mut s, "CYCLIC-ASSEMBLY", owner, 1, 1);
    let r = record(&s, "CYCLIC-ASSEMBLY");
    assert_eq!(r["bounds_complete"], false);
    assert!(r["bounds_m"].is_null());
    assert!(r["size_m"].is_null());
}

fn text_block(s: &mut Session, name: &str, mut entity: EntityType) {
    let mut b = BlockRecord::new(name);
    b.handle = s.drawing.doc.allocate_handle();
    b.units = 6;
    entity.common_mut().owner_handle = b.handle;
    s.drawing.doc.block_records.add(b).unwrap();
    s.drawing.doc.add_entity(entity).unwrap();
}

#[test]
fn rotated_text_mtext_and_attributes_swap_their_shaped_width_and_height() {
    let (_dir, mut s) = session(6);
    // Attached attributes refer to a real, empty definition; only their text occupies space.
    let mut empty = BlockRecord::new("EMPTY");
    empty.handle = s.drawing.doc.allocate_handle();
    s.drawing.doc.block_records.add(empty).unwrap();
    for family in ["TEXT", "MTEXT", "CONSTANT", "ATTRIBUTE"] {
        for (suffix, angle) in [("PLAIN", 0.0), ("ROTATED", std::f64::consts::FRAC_PI_2)] {
            let value = "CONCOURSE TERMINAL PANEL";
            let entity = match family {
                "TEXT" => {
                    let mut t = Text::with_value(value, Vector3::ZERO);
                    t.rotation = angle;
                    EntityType::Text(t)
                }
                "MTEXT" => {
                    let mut t = MText::new();
                    t.value = value.into();
                    t.rectangle_width = 100.0;
                    t.rotation = angle;
                    t.attachment_point = opencadcodec::entities::AttachmentPoint::MiddleCenter;
                    EntityType::MText(t)
                }
                "CONSTANT" => {
                    let mut a = AttributeDefinition::new("MARK".into(), "".into(), value.into());
                    a.flags.constant = true;
                    a.height = 1.0;
                    a.rotation = angle;
                    EntityType::AttributeDefinition(a)
                }
                _ => {
                    let mut a = AttributeEntity::new("MARK".into(), value.into());
                    a.height = 1.0;
                    a.rotation = angle;
                    let mut i = Insert::new("EMPTY", Vector3::ZERO);
                    i.attributes.push(a);
                    EntityType::Insert(i)
                }
            };
            text_block(&mut s, &format!("{family}-{suffix}"), entity);
        }
        let plain = record(&s, &format!("{family}-PLAIN"));
        let rotated = record(&s, &format!("{family}-ROTATED"));
        assert_eq!(plain["bounds_complete"], true, "{family}");
        assert_eq!(rotated["bounds_complete"], true, "{family}");
        let width = plain["size_m"][0].as_f64().unwrap();
        let height = plain["size_m"][1].as_f64().unwrap();
        assert!(width > height * 5.0, "long label {family}");
        assert!(
            (rotated["size_m"][0].as_f64().unwrap() - height).abs() < 1e-8,
            "{family}"
        );
        assert!(
            (rotated["size_m"][1].as_f64().unwrap() - width).abs() < 1e-8,
            "{family}"
        );
        if family == "MTEXT" {
            assert!(plain["bounds_m"][0][0].as_f64().unwrap() < 0.0);
            assert!(plain["bounds_m"][0][1].as_f64().unwrap() < 0.0);
        }
    }
}

#[test]
fn text_alignment_uses_alignment_point_and_unshapable_text_is_incomplete() {
    let (_dir, mut s) = session(6);
    let mut text = Text::with_value("CONCOURSE TERMINAL PANEL", Vector3::ZERO);
    text.alignment_point = Some(Vector3::new(100.0, 50.0, 0.0));
    text.horizontal_alignment = opencadcodec::entities::TextHorizontalAlignment::Right;
    text_block(&mut s, "ALIGNED", EntityType::Text(text.clone()));
    let r = record(&s, "ALIGNED");
    assert!((r["bounds_m"][1][0].as_f64().unwrap() - 100.0).abs() < 1e-8);
    assert!(r["bounds_m"][0][1].as_f64().unwrap() > 40.0);
    text.height = 0.0;
    text_block(&mut s, "UNSHAPABLE", EntityType::Text(text));
    let r = record(&s, "UNSHAPABLE");
    assert_eq!(r["bounds_complete"], false);
    assert!(r["size_m"].is_null());
}

#[test]
fn invisible_and_nonconstant_attribute_definitions_do_not_enlarge_component_bounds() {
    let (_dir, mut s) = session(6);
    let owner = block(&mut s, "TERMINAL-PANEL", 6, 1.0);
    let before = record(&s, "TERMINAL-PANEL")["bounds_m"].clone();
    for (tag, constant, invisible) in [("VARIABLE", false, false), ("HIDDEN", true, true)] {
        let mut a = AttributeDefinition::new(tag.into(), "".into(), "REMOTE LABEL".into());
        a.common.owner_handle = owner;
        a.flags.constant = constant;
        a.flags.invisible = invisible;
        a.insertion_point = Vector3::new(1000.0, 1000.0, 0.0);
        s.drawing
            .doc
            .add_entity(EntityType::AttributeDefinition(a))
            .unwrap();
    }
    let r = record(&s, "TERMINAL-PANEL");
    assert_eq!(r["bounds_m"], before);
    assert_eq!(r["attribute_tags"], json!(["HIDDEN", "VARIABLE"]));
}
