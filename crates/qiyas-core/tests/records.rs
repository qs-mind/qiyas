//! Records: every object readable as a record, any allowed property set by JSON pointer
//! (`SetRecords`, one undo step, refused paths naming their command), entities of any type made
//! from defaults (`CreateEntities`), and `record_schema` describing every type.

use opencadcodec::{EntityType, Handle};
use qiyas_core::records::{Collection, Detail, RecordQuery, Where, WhereOp};
use qiyas_core::{Command, NewEntity, Outcome, RecordUpdate, Session};
use serde_json::{json, Value};

fn session(ext: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), ext);
    (dir, Session::open(&path).unwrap())
}

fn query(collection: Collection) -> RecordQuery {
    RecordQuery {
        collection,
        ..RecordQuery::default()
    }
}

fn update(key: Value, path: &str, value: Value) -> RecordUpdate {
    RecordUpdate {
        key,
        path: path.into(),
        value,
        expected: None,
    }
}

fn set(
    s: &mut Session,
    collection: Collection,
    updates: Vec<RecordUpdate>,
) -> Result<Outcome, String> {
    s.run(Command::SetRecords {
        collection,
        updates,
    })
    .map_err(|e| e.to_string())
}

/// The first entity of `kind` on the sheet on screen.
fn first(s: &Session, kind: &str) -> u64 {
    let page = s
        .records(&RecordQuery {
            kind: Some(kind.into()),
            limit: Some(1),
            ..query(Collection::Entities)
        })
        .unwrap();
    page.records[0]["key"].as_u64().unwrap()
}

#[test]
fn entities_read_as_records_filtered_paged_and_projected() {
    let (_dir, s) = session("dxf");
    let all = s.records(&query(Collection::Entities)).unwrap();
    assert!(all.total >= 5, "{}", all.total);
    assert!(
        all.records[0].get("data").is_none(),
        "the summary has no data"
    );
    let page = s
        .records(&RecordQuery {
            limit: Some(2),
            offset: 1,
            ..query(Collection::Entities)
        })
        .unwrap();
    assert_eq!(page.records.len(), 2);
    assert_eq!(page.next_offset, Some(3));
    assert_eq!(page.records[0]["key"], all.records[1]["key"]);

    let lines = s
        .records(&RecordQuery {
            kind: Some("line".into()),
            detail: Detail::Full,
            ..query(Collection::Entities)
        })
        .unwrap();
    assert!(lines.total >= 1);
    for r in &lines.records {
        assert_eq!(r["type"], "Line");
        assert!(r["data"]["start"]["x"].is_number(), "{r}");
        assert!(r["data"]["common"]["layer"].is_string());
    }

    let walls = s
        .records(&RecordQuery {
            conditions: vec![Where {
                path: "/common/layer".into(),
                op: WhereOp::Eq,
                value: json!("a-wall"),
            }],
            fields: vec!["/common/layer".into()],
            ..query(Collection::Entities)
        })
        .unwrap();
    assert!(walls.total >= 1);
    for r in &walls.records {
        assert_eq!(r["fields"]["/common/layer"], "A-WALL");
        assert!(r.get("data").is_none());
    }

    let page = s
        .records(&RecordQuery {
            detail: Detail::Full,
            limit: Some(200),
            ..query(Collection::Entities)
        })
        .unwrap();
    let bytes = serde_json::to_string(&page.records).unwrap().len();
    assert!(bytes <= qiyas_core::records::MAX_BYTES, "{bytes}");
}

#[test]
fn every_collection_reads() {
    let (_dir, mut s) = session("dxf");
    s.run(Command::MeasureDistance {
        points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
    })
    .unwrap();
    for collection in [
        Collection::Layers,
        Collection::Linetypes,
        Collection::TextStyles,
        Collection::DimStyles,
        Collection::Blocks,
        Collection::Layouts,
        Collection::Viewports,
        Collection::Groups,
        Collection::Views,
        Collection::Header,
        Collection::LayerStates,
        Collection::Xrefs,
        Collection::Takeoff,
        Collection::Boq,
        Collection::Conflicts,
        Collection::Schedules,
        Collection::Comments,
        Collection::Constraints,
        Collection::Parameters,
    ] {
        let page = s
            .records(&RecordQuery {
                detail: Detail::Full,
                ..query(collection)
            })
            .unwrap_or_else(|e| panic!("{collection:?}: {e}"));
        assert!(page.total >= page.records.len());
    }
    let layers = s
        .records(&RecordQuery {
            names: vec!["a-wall".into()],
            ..query(Collection::Layers)
        })
        .unwrap();
    assert_eq!(layers.total, 1);
    assert_eq!(layers.records[0]["key"], "A-WALL");
    let header = s.records(&query(Collection::Header)).unwrap();
    assert_eq!(header.records[0]["key"], "header");
    let takeoff = s.records(&query(Collection::Takeoff)).unwrap();
    assert_eq!(takeoff.total, 1);
}

/// Every entity of the sample drawings reads as data and back to itself (in DXF and DWG).
#[test]
fn every_entity_round_trips_through_its_record() {
    let dir = tempfile::tempdir().unwrap();
    let plan =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../samples/test-plan.dwg");
    let mut paths = vec![plan];
    for ext in ["dxf", "dwg"] {
        let sub = dir.path().join(ext);
        std::fs::create_dir_all(&sub).unwrap();
        paths.push(qiyas_core::fixtures::write_sample(&sub, ext));
        paths.push(qiyas_core::fixtures::write_schedule(&sub, ext));
    }
    let mut kinds = std::collections::BTreeSet::new();
    for path in paths {
        let s = Session::open(&path).unwrap();
        for block in s.drawing.doc.block_records.iter().map(|b| b.name.clone()) {
            for e in s.drawing.entities_in(&block) {
                let kind = format!("{e:?}");
                let kind = kind.split('(').next().unwrap().to_string();
                let data = qiyas_core::records::entity_data(e);
                let back =
                    qiyas_core::record_edit::entity_from_record(&kind, data, e, "test").unwrap();
                // All but the bytes the DWG was read from, which an edit must drop anyway.
                let mut original = e.clone();
                original.common_mut().raw_record = None;
                if let EntityType::Insert(i) = &mut original {
                    for a in &mut i.attributes {
                        a.common.raw_record = None;
                    }
                }
                assert_eq!(back, original, "{} {kind}", path.display());
                kinds.insert(kind);
            }
        }
    }
    for kind in [
        "Line",
        "LwPolyline",
        "Arc",
        "Text",
        "MText",
        "Insert",
        "Dimension",
        "Table",
    ] {
        assert!(kinds.contains(kind), "{kind} not in {kinds:?}");
    }
}

#[test]
fn set_records_changes_an_entity_in_one_step_and_the_take_off_follows() {
    let (_dir, mut s) = session("dxf");
    let arc = first(&s, "Arc");
    s.run(Command::MeasureEntity {
        handle: arc,
        chain: vec![],
    })
    .unwrap();
    let before = s.takeoff.items[0].value;
    let radius = |s: &Session| match s.drawing.doc.get_entity(Handle::new(arc)).unwrap() {
        EntityType::Arc(a) => a.radius,
        _ => unreachable!(),
    };
    let r0 = radius(&s);
    let steps = s.history_steps().0.len();
    set(
        &mut s,
        Collection::Entities,
        vec![update(json!(arc), "/radius", json!(r0 * 2.0))],
    )
    .unwrap();
    assert_eq!(radius(&s), r0 * 2.0);
    assert_eq!(s.history_steps().0.len(), steps + 1);
    assert_eq!(s.history_steps().0.last().unwrap().label, "set_records");
    assert_eq!(s.last_change().changed, vec![arc]);
    let after = s.takeoff.items[0].value;
    assert!((after - before * 2.0).abs() < 1e-6, "{before} -> {after}");
    assert!(s.undo());
    assert_eq!(radius(&s), r0);
    assert!((s.takeoff.items[0].value - before).abs() < 1e-9);
}

#[test]
fn set_records_refuses_what_it_must_and_says_which_command_does_it() {
    let (_dir, mut s) = session("dxf");
    let line = first(&s, "Line");
    let text = first(&s, "Text");
    let refused = |s: &mut Session, key: Value, path: &str, value: Value| {
        set(s, Collection::Entities, vec![update(key, path, value)]).unwrap_err()
    };
    let e = refused(&mut s, json!(line), "/common/handle", json!(1));
    assert!(e.contains("identity"), "{e}");
    let e = refused(&mut s, json!(text), "/value", json!("X"));
    assert!(e.contains("edit_text"), "{e}");
    let e = refused(&mut s, json!(line), "/common/layer", json!("NO-SUCH"));
    assert!(e.contains("no layer NO-SUCH"), "{e}");
    let e = refused(&mut s, json!(line), "/no/such/path", json!(1));
    assert!(e.contains("record_schema"), "{e}");
    let e = refused(&mut s, json!(line), "/start/x", json!("not a number"));
    assert!(e.contains("start.x"), "the failing field is named: {e}");
    let stale = RecordUpdate {
        expected: Some(json!(12345.0)),
        ..update(json!(line), "/start/x", json!(1.0))
    };
    let e = set(&mut s, Collection::Entities, vec![stale]).unwrap_err();
    assert!(e.contains("changed since it was read"), "{e}");
    let e = set(
        &mut s,
        Collection::Takeoff,
        vec![update(json!("m1"), "/note", json!("x"))],
    )
    .unwrap_err();
    assert!(e.contains("set_description"), "{e}");
    // All or nothing: a good update before a bad one is not kept.
    let x0 = s.records(&RecordQuery {
        handles: vec![line],
        fields: vec!["/start/x".into()],
        ..query(Collection::Entities)
    });
    let x0 = x0.unwrap().records[0]["fields"]["/start/x"].clone();
    let e = set(
        &mut s,
        Collection::Entities,
        vec![
            update(json!(line), "/start/x", json!(99.0)),
            update(json!(text), "/value", json!("X")),
        ],
    )
    .unwrap_err();
    assert!(e.contains("edit_text"));
    let x1 = s
        .records(&RecordQuery {
            handles: vec![line],
            fields: vec!["/start/x".into()],
            ..query(Collection::Entities)
        })
        .unwrap()
        .records[0]["fields"]["/start/x"]
        .clone();
    assert_eq!(x0, x1);
}

#[test]
fn set_records_on_layers_and_the_header() {
    let (_dir, mut s) = session("dxf");
    let colour = s.drawing.doc.layers.get("A-WALL").unwrap().color;
    let other = if colour == opencadcodec::Color::Index(5) {
        6
    } else {
        5
    };
    set(
        &mut s,
        Collection::Layers,
        vec![update(json!("A-WALL"), "/color", json!({ "Index": other }))],
    )
    .unwrap();
    assert_eq!(
        s.drawing.doc.layers.get("A-WALL").unwrap().color,
        opencadcodec::Color::Index(other)
    );
    let e = set(
        &mut s,
        Collection::Layers,
        vec![update(json!("A-WALL"), "/name", json!("WALLS"))],
    )
    .unwrap_err();
    assert!(e.contains("rename_layer"), "{e}");
    let current = s.current.layer.clone();
    let e = set(
        &mut s,
        Collection::Layers,
        vec![update(json!(current), "/flags/frozen", json!(true))],
    )
    .unwrap_err();
    assert!(e.contains("current layer"), "{e}");
    assert!(s.undo());
    assert_eq!(s.drawing.doc.layers.get("A-WALL").unwrap().color, colour);

    let lts = s.drawing.doc.header.linetype_scale;
    set(
        &mut s,
        Collection::Header,
        vec![update(
            json!("header"),
            "/linetype_scale",
            json!(lts * 10.0),
        )],
    )
    .unwrap();
    assert_eq!(s.drawing.doc.header.linetype_scale, lts * 10.0);
    assert!(s.undo());
    assert_eq!(s.drawing.doc.header.linetype_scale, lts);
    let e = set(
        &mut s,
        Collection::Header,
        vec![update(json!("header"), "/insertion_units", json!(1))],
    )
    .unwrap_err();
    assert!(e.contains("new_drawing"), "{e}");
}

#[test]
fn a_viewport_scale_is_set_through_its_record() {
    let (_dir, mut s) = session("dxf");
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .expect("the sample has a layout");
    s.run(Command::SetSheet {
        block: layout.block.clone(),
    })
    .unwrap();
    let added = s
        .run(Command::MView {
            scale: None,
            centre: None,
            corner1: [10.0, 10.0],
            corner2: [110.0, 80.0],
        })
        .unwrap();
    let Outcome::Added { handles } = added else {
        panic!()
    };
    let vp = handles[0];
    let page = s
        .records(&RecordQuery {
            handles: vec![vp],
            detail: Detail::Full,
            ..query(Collection::Viewports)
        })
        .unwrap();
    assert_eq!(page.records.len(), 1);
    let height = page.records[0]["data"]["view_height"].as_f64().unwrap();
    set(
        &mut s,
        Collection::Viewports,
        vec![update(json!(vp), "/view_height", json!(height * 2.0))],
    )
    .unwrap();
    match s.drawing.doc.get_entity(Handle::new(vp)).unwrap() {
        EntityType::Viewport(v) => assert_eq!(v.view_height, height * 2.0),
        _ => unreachable!(),
    }
}

#[test]
fn create_entities_makes_any_type_from_defaults_on_a_new_layer() {
    let (_dir, mut s) = session("dxf");
    let made = s
        .run(Command::CreateEntities {
            entities: vec![
                NewEntity {
                    kind: "Circle".into(),
                    data: json!({"center": {"x": 2.0, "y": 3.0, "z": 0.0}, "radius": 0.3, "common": {"layer": "A-COLS"}}),
                },
                NewEntity {
                    kind: "text".into(),
                    data: json!({"value": "C1", "height": 0.2, "insertion_point": {"x": 2.5, "y": 3.0, "z": 0.0}}),
                },
            ],
            sheet: None,
        })
        .unwrap();
    let Outcome::Added { handles } = made else {
        panic!("{made:?}")
    };
    assert_eq!(handles.len(), 2);
    match s.drawing.doc.get_entity(Handle::new(handles[0])).unwrap() {
        EntityType::Circle(c) => {
            assert_eq!(c.radius, 0.3);
            assert_eq!(c.common.layer, "A-COLS");
            assert_eq!(c.normal.z, 1.0, "defaults kept");
        }
        other => panic!("{other:?}"),
    }
    match s.drawing.doc.get_entity(Handle::new(handles[1])).unwrap() {
        EntityType::Text(t) => {
            assert_eq!(t.value, "C1");
            assert_eq!(t.common.layer, s.current.layer, "the current layer");
            assert!(t
                .common
                .extended_data
                .get_record(qiyas_geom::LOGICAL_TEXT_APP)
                .is_some());
        }
        other => panic!("{other:?}"),
    }
    assert!(s.drawing.doc.layers.get("A-COLS").is_some());
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(handles[0])).is_none());
    assert!(s.drawing.doc.layers.get("A-COLS").is_none());

    let e = s
        .run(Command::CreateEntities {
            entities: vec![NewEntity {
                kind: "Dimension".into(),
                data: json!({}),
            }],
            sheet: None,
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("dimension"), "{e}");
}

#[test]
fn record_schema_lists_paths_types_and_rules() {
    let line = qiyas_core::record_edit::record_schema("line").unwrap();
    let path = |schema: &Value, p: &str| -> Value {
        schema["paths"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["path"] == p)
            .cloned()
            .unwrap_or_else(|| panic!("no {p} in {schema}"))
    };
    assert_eq!(path(&line, "/start/x")["type"], "number");
    assert!(path(&line, "/common/layer").get("write").is_none());
    assert_eq!(path(&line, "/common/handle")["write"], false);
    assert!(line["types"]["Color"].as_str().unwrap().contains("ByLayer"));
    assert_eq!(line["creatable"], true);
    let text = qiyas_core::record_edit::record_schema("Text").unwrap();
    assert_eq!(path(&text, "/value")["via"], "edit_text");
    let layer = qiyas_core::record_edit::record_schema("layers").unwrap();
    assert_eq!(path(&layer, "/name")["via"], "rename_layer");
    let header = qiyas_core::record_edit::record_schema("header").unwrap();
    assert_eq!(path(&header, "/insertion_units")["via"], "new_drawing");
    assert!(path(&header, "/linetype_scale").get("write").is_none());
    let e = qiyas_core::record_edit::record_schema("Nope")
        .unwrap_err()
        .to_string();
    assert!(e.contains("LwPolyline"), "{e}");
}
