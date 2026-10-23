//! Defaults and conveniences follow the same rules through every command caller.

use opencadcodec::entities::AttachmentPoint;
use opencadcodec::{EntityType, Handle};
use qiyas_core::{Command, Outcome, Session, Source};
use serde_json::{json, Value};

fn blank(units: i16) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::new_blank(&dir.path().join("conveniences.dwg"), units);
    (dir, session)
}

fn command(value: Value) -> Command {
    serde_json::from_value(value).unwrap()
}

fn measurement(value: Value) -> Command {
    let mut fields = json!({
        "type": "add_measurement", "points": [], "handles": [], "description": "", "note": ""
    });
    fields
        .as_object_mut()
        .unwrap()
        .extend(value.as_object().unwrap().clone());
    command(fields)
}

fn add(session: &mut Session, value: Value) -> u64 {
    match session.run(command(value)).unwrap() {
        Outcome::Added { handles } if handles.len() == 1 => handles[0],
        other => panic!("expected one entity, got {other:?}"),
    }
}

fn entity(session: &Session, handle: u64) -> &EntityType {
    session.drawing.doc.get_entity(Handle::new(handle)).unwrap()
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < expected.abs().max(1.0) * 1e-10,
        "{actual} != {expected}"
    );
}

#[test]
fn omitted_and_zero_text_heights_use_paper_size_in_each_unit() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut session) = blank(units);
        for scale in [50.0, 100.0] {
            session
                .run(Command::AnnotationScale {
                    name: format!("1:{scale}"),
                })
                .unwrap();
            let expected = 0.0025 * scale / session.drawing.units.to_metres();
            for mut value in [
                json!({"type": "text", "at": [1, 2], "rotation": 0, "value": "Room"}),
                json!({"type": "mtext", "at": [1, 2], "width": 0, "value": "Room"}),
                json!({"type": "att_def", "at": [1, 2], "tag": "ROOM", "prompt": "Room", "default": "1"}),
                json!({"type": "field", "at": [1, 2], "field": {"kind": "file_name"}}),
            ] {
                for height in [None, Some(0.0)] {
                    if let Some(height) = height {
                        value["height"] = json!(height);
                    }
                    let handle = add(&mut session, value.clone());
                    let height = match entity(&session, handle) {
                        EntityType::Text(text) => text.height,
                        EntityType::MText(text) => text.height,
                        EntityType::AttributeDefinition(text) => text.height,
                        other => panic!("expected text, got {other:?}"),
                    };
                    near(height, expected);
                    assert!(session.undo());
                    assert!(session
                        .drawing
                        .doc
                        .get_entity(Handle::new(handle))
                        .is_none());
                }
                value["height"] = json!(-1.0);
                let before = session.history_steps();
                assert!(session.run(command(value)).is_err());
                assert_eq!(session.history_steps(), before);
            }
        }
    }
}

#[test]
fn text_line_breaks_make_mtext_at_the_original_baseline() {
    let (_dir, mut session) = blank(6);
    for value in ["Room\n12 m²", r"Room\n12 m²"] {
        let handle = add(
            &mut session,
            json!({"type": "text", "at": [2, 3], "height": 0.25, "rotation": 0, "value": value}),
        );
        let EntityType::MText(text) = entity(&session, handle) else {
            panic!("multiline Text becomes MText");
        };
        assert_eq!(text.value, r"Room\P12 m²");
        assert_eq!(
            [text.insertion_point.x, text.insertion_point.y],
            [2.0, 3.25]
        );
        assert_eq!(text.attachment_point, AttachmentPoint::TopLeft);
        assert_eq!(text.rectangle_width, 0.0);
        assert!(session.undo());
        assert!(session
            .drawing
            .doc
            .get_entity(Handle::new(handle))
            .is_none());
    }
}

#[test]
fn literal_newlines_work_in_mtext_and_edits_and_undo_keeps_the_original() {
    let (_dir, mut session) = blank(6);
    let text = add(
        &mut session,
        json!({"type": "text", "at": [2, 3], "height": 0.25, "rotation": 0, "value": "Room"}),
    );
    let mtext = add(
        &mut session,
        json!({"type": "mtext", "at": [4, 5], "height": 0.25, "width": 2, "value": r"Room\nArea"}),
    );
    let EntityType::MText(created) = entity(&session, mtext) else {
        panic!("an MText");
    };
    assert_eq!(created.value, r"Room\PArea");
    for handle in [text, mtext] {
        session
            .run(command(json!({
                "type": "edit_text", "target": {"handle": handle}, "value": r"غرفة\nمساحة"
            })))
            .unwrap();
        let EntityType::MText(edited) = entity(&session, handle) else {
            panic!("the edited text has two lines");
        };
        assert_eq!(edited.value, r"غرفة\Pمساحة");
        assert!(session.undo());
    }
    assert!(matches!(entity(&session, text), EntityType::Text(t) if t.value == "Room"));
    assert!(matches!(entity(&session, mtext), EntityType::MText(t) if t.value == r"Room\PArea"));
}

#[test]
fn automatic_table_sizes_fit_text_at_the_annotation_scale_in_each_unit() {
    for units in [4, 5, 6, 1] {
        let (_dir, mut session) = blank(units);
        let height = 0.0025 * 100.0 / session.drawing.units.to_metres();
        for sizes in [json!({}), json!({"col_width": 0, "row_height": 0})] {
            let mut value = json!({
                "type": "table", "at": [0, 0], "rows": 2, "cols": 2,
                "data": [["Room", "Area"], ["BEDROOM", "12"]]
            });
            value
                .as_object_mut()
                .unwrap()
                .extend(sizes.as_object().unwrap().clone());
            let handle = add(&mut session, value);
            let EntityType::Table(table) = entity(&session, handle) else {
                panic!("a table");
            };
            for row in &table.rows {
                near(row.height, height * 2.0);
            }
            for column in &table.columns {
                near(column.width, height * (7.0 * 0.7 + 2.0));
            }
            assert!(session.undo());
            assert!(session
                .drawing
                .doc
                .get_entity(Handle::new(handle))
                .is_none());
        }
    }
}

#[test]
fn empty_items_need_explicit_intent_from_ai_and_mcp_only() {
    for source in [Source::Ai, Source::Mcp] {
        let (_dir, mut session) = blank(6);
        for value in [Value::Null, json!(0)] {
            let item =
                json!({"type": "add_measurement", "kind": "item", "value": value, "unit": "item"});
            let error = session
                .run_as(measurement(item.clone()), source)
                .unwrap_err();
            assert!(error.to_string().contains("empty_item true"), "{error}");
            assert!(session.takeoff.items.is_empty());
            assert!(!session.can_undo());
            let mut allowed = item;
            allowed["empty_item"] = json!(true);
            let Outcome::Measured { measurement } =
                session.run_as(measurement(allowed), source).unwrap()
            else {
                panic!("an explicit empty item");
            };
            assert_eq!(measurement.value, 0.0);
            assert_eq!(measurement.source, source.name());
            assert!(session.undo());
            assert!(session.takeoff.items.is_empty());
        }
        for item in [
            json!({"type": "add_measurement", "kind": "item", "value": 3, "unit": "bags"}),
            json!({"type": "add_measurement", "kind": "item", "value": 0, "unit": "markup", "points": [[1, 2]]}),
        ] {
            session.run_as(measurement(item), source).unwrap();
            assert!(session.undo());
        }
    }
    let (_dir, mut session) = blank(6);
    session
        .run(measurement(
            json!({"kind": "item", "value": 0, "unit": "item"}),
        ))
        .unwrap();
}

#[test]
fn ai_and_mcp_count_block_references_but_users_can_still_count_other_entities() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let mut session = Session::open(&path).unwrap();
    let handle_of = |is_insert: bool| {
        session
            .drawing
            .entities_in("*Model_Space")
            .find(|entity| {
                if is_insert {
                    matches!(entity, EntityType::Insert(_))
                } else {
                    matches!(entity, EntityType::Line(_))
                }
            })
            .unwrap()
            .common()
            .handle
            .value()
    };
    let (line, insert) = (handle_of(false), handle_of(true));
    let count = |handle| measurement(json!({"kind": "count", "handles": [handle]}));
    for source in [Source::Ai, Source::Mcp] {
        let error = session.run_as(count(line), source).unwrap_err();
        assert!(error.to_string().contains("block reference"), "{error}");
        assert!(session.takeoff.items.is_empty());
        assert!(!session.can_undo());
        let Outcome::Measured { measurement } = session.run_as(count(insert), source).unwrap()
        else {
            panic!("a count");
        };
        assert_eq!(measurement.value, 1.0);
        assert!(session.undo());
    }
    session.run(count(line)).unwrap();
}

#[test]
fn markups_validate_points_and_add_one_undoable_takeoff_row() {
    for source in [Source::User, Source::Ai, Source::Mcp] {
        let (_dir, mut session) = blank(6);
        for (kind, points) in [
            ("note", json!([[1, 2]])),
            ("cloud", json!([[1, 2], [3, 2], [3, 4]])),
        ] {
            assert!(session
                .run_as(
                    command(
                        json!({"type": "add_markup", "kind": kind, "points": [], "text": "Check"})
                    ),
                    source,
                )
                .is_err());
            assert!(!session.can_undo());
            let Outcome::Measured { measurement } = session
                .run_as(
                    command(json!({"type": "add_markup", "kind": kind, "points": points, "text": "Check"})),
                    source,
                )
                .unwrap()
            else {
                panic!("a markup");
            };
            assert_eq!(measurement.value, 0.0);
            assert_eq!(measurement.unit, "markup");
            assert_eq!(measurement.note, kind);
            assert_eq!(measurement.description, "Check");
            assert_eq!(session.takeoff.items.len(), 1);
            assert!(session.undo());
            assert!(session.takeoff.items.is_empty());
        }
    }
}

#[test]
fn label_spaces_has_one_named_step_and_can_run_inside_a_batch() {
    let (_dir, mut session) = blank(6);
    add(
        &mut session,
        json!({"type": "draw", "kind": "rectangle", "a": [0, 0], "b": [4, 3]}),
    );
    let labels = || {
        command(json!({
            "type": "label_spaces", "query": {"seeds": [{"name": "Room", "point": [2, 1]}]},
            "layer": "A-AREA"
        }))
    };
    let before = session.history_steps().0.len();
    let current = session.current.clone();
    let Outcome::Rooms { report } = session.run_as(labels(), Source::Ai).unwrap() else {
        panic!("a rooms report");
    };
    assert_eq!(report.labels_written, Some(1));
    assert_eq!(report.rooms[0].candidate_area_m2, 12.0);
    let handle = report.rooms[0].label_handle.unwrap();
    assert!(matches!(
        entity(&session, handle),
        EntityType::Text(t) if t.value == "Unapproved candidate 12.00 m²"
    ));
    let history = session.history_steps().0;
    assert_eq!(history.len(), before + 1);
    assert_eq!(history.last().unwrap().label, "label_spaces");
    assert_eq!(history.last().unwrap().source, Source::Ai);
    assert_eq!(session.current, current);
    assert!(session.undo());
    assert!(session.drawing.doc.layers.get("A-AREA").is_none());
    assert!(session
        .drawing
        .doc
        .get_entity(Handle::new(handle))
        .is_none());
    assert!(session.redo());
    assert!(session
        .drawing
        .doc
        .get_entity(Handle::new(handle))
        .is_some());
    assert!(session.undo());

    let Outcome::Batch { outcomes, .. } = session
        .run_as(
            Command::Batch {
                commands: vec![
                    command(json!({"type": "text", "at": [8, 8], "rotation": 0, "value": "Note"})),
                    labels(),
                ],
                layer: None,
                color: None,
            },
            Source::Mcp,
        )
        .unwrap()
    else {
        panic!("a batch");
    };
    assert!(matches!(outcomes[1], Outcome::Rooms { .. }));
    assert_eq!(session.history_steps().0.len(), before + 1);
    assert_eq!(session.history_steps().0.last().unwrap().label, "batch");
    assert!(session.undo());
    assert_eq!(session.drawing.entities_in("*Model_Space").count(), 1);
    assert!(session.drawing.doc.layers.get("A-AREA").is_none());
}

#[test]
fn a_refused_label_layer_rolls_back_the_whole_batch() {
    let (_dir, mut session) = blank(6);
    add(
        &mut session,
        json!({"type": "draw", "kind": "rectangle", "a": [0, 0], "b": [4, 3]}),
    );
    let current = session.current.clone();
    let history = session.history_steps();
    let error = session
        .run_as(
            Command::Batch {
                commands: vec![
                    command(json!({"type": "text", "at": [8, 8], "rotation": 0, "value": "Note"})),
                    command(json!({
                        "type": "label_spaces", "query": {"seeds": [{"point": [2, 1]}]},
                        "layer": "XREF|A-AREA"
                    })),
                ],
                layer: Some("TEMP-NOTES".to_string()),
                color: None,
            },
            Source::Ai,
        )
        .unwrap_err();
    assert!(error.to_string().contains("xref"), "{error}");
    assert_eq!(session.current, current);
    assert_eq!(session.history_steps(), history);
    assert!(session.drawing.doc.layers.get("TEMP-NOTES").is_none());
    assert_eq!(session.drawing.entities_in("*Model_Space").count(), 1);
}
