//! Drafting symbols keep the same paper sizes in every supported drawing unit.

use opencadcodec::tables::{LineType, LineTypeElement};
use opencadcodec::{EntityType, Handle};
use qiyas_core::check::CheckScope;
use qiyas_core::{Command, DrawSpec, GridAxis, GridExtent, LayerProp, Outcome, Session};
use serde_json::{json, Value};

const UNITS: [(i16, f64); 4] = [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)];

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn added(session: &mut Session, command: Command) -> Vec<u64> {
    match session.run(command).unwrap() {
        Outcome::Added { handles } => handles,
        other => panic!("Expected added entities, got {other:?}"),
    }
}

fn line(session: &Session, handle: u64) -> &opencadcodec::Line {
    match session.drawing.doc.get_entity(Handle::new(handle)).unwrap() {
        EntityType::Line(line) => line,
        other => panic!("Expected line, got {other:?}"),
    }
}

fn snapshot(session: &Session) -> Value {
    let doc = &session.drawing.doc;
    let mut entities: Vec<_> = doc.entities().collect();
    entities.sort_by_key(|entity| entity.common().handle.value());
    json!({
        "entities": entities,
        "layers": doc.layers.iter().collect::<Vec<_>>(),
        "linetypes": doc.line_types.iter().collect::<Vec<_>>(),
        "apps": doc.app_ids.iter().collect::<Vec<_>>(),
        "current": session.current,
    })
}

fn drawing(session: &mut Session, kind: DrawSpec) -> u64 {
    added(session, Command::Draw(kind))[0]
}

fn set_scale(session: &mut Session) {
    session
        .run(Command::AnnotationScale {
            name: "1:50".into(),
        })
        .unwrap();
}

#[test]
fn centre_marks_on_circles_and_arcs_have_unit_scaled_extensions_and_one_undo() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("marks.dxf"), units);
        set_scale(&mut session);
        let circle = drawing(
            &mut session,
            DrawSpec::Circle {
                centre: [0.0, 0.0],
                radius: 1.0 / factor,
            },
        );
        let arc = drawing(
            &mut session,
            DrawSpec::Arc {
                centre: [3.0 / factor, 0.0],
                radius: 0.5 / factor,
                start: 10.0,
                end: 150.0,
            },
        );
        let before = snapshot(&session);
        let steps = session.history_steps().0.len();
        let marks = added(
            &mut session,
            Command::CenterMark {
                targets: vec![circle, arc, circle],
                extension: None,
            },
        );
        assert_eq!(marks.len(), 4);
        assert_eq!(session.history_steps().0.len(), steps + 1);
        assert_eq!(session.current.layer, "0");
        let first = line(&session, marks[0]);
        near(first.start.x * factor, -1.125);
        near(first.end.x * factor, 1.125);
        near(first.start.y, 0.0);
        let arc_mark = line(&session, marks[2]);
        near(arc_mark.start.x * factor, 2.375);
        near(arc_mark.end.x * factor, 3.625);
        for handle in &marks {
            let mark = line(&session, *handle);
            assert_eq!(mark.common.layer, "CENTER");
            assert_eq!(mark.common.linetype, "CENTER");
        }
        let pattern = session.drawing.doc.line_types.get("CENTER").unwrap();
        let global = session.drawing.doc.header.linetype_scale;
        near(pattern.elements[0].length * global * factor, 0.625);
        assert!(session
            .check_drawing(&CheckScope {
                rules: vec!["missing_center_mark".into()],
                handles: vec![circle],
                ..CheckScope::default()
            })
            .findings
            .is_empty());
        let after = snapshot(&session);
        assert!(session.undo());
        assert_eq!(snapshot(&session), before);
        assert!(session.redo());
        assert_eq!(snapshot(&session), after);

        let custom = added(
            &mut session,
            Command::CenterMark {
                targets: vec![circle],
                extension: Some(0.2 / factor),
            },
        );
        near(line(&session, custom[0]).end.x * factor, 1.2);
    }
}

#[test]
fn centre_lines_use_shared_parallel_spans_and_crossing_bisectors_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("lines.dxf"), units);
        set_scale(&mut session);
        let a = drawing(
            &mut session,
            DrawSpec::Line {
                a: [0.0, 0.0],
                b: [10.0 / factor, 0.0],
            },
        );
        let b = drawing(
            &mut session,
            DrawSpec::Line {
                a: [8.0 / factor, 2.0 / factor],
                b: [2.0 / factor, 2.0 / factor],
            },
        );
        let before = snapshot(&session);
        let steps = session.history_steps().0.len();
        let center = added(&mut session, Command::CenterLine { a, b });
        assert_eq!(center.len(), 1);
        let center = line(&session, center[0]);
        near(center.start.x * factor, 1.875);
        near(center.end.x * factor, 8.125);
        near(center.start.y * factor, 1.0);
        near(center.end.y * factor, 1.0);
        assert_eq!(session.history_steps().0.len(), steps + 1);
        assert!(session.undo());
        assert_eq!(snapshot(&session), before);

        let vertical = drawing(
            &mut session,
            DrawSpec::Line {
                a: [0.0, 4.0 / factor],
                b: [0.0, -4.0 / factor],
            },
        );
        let center = added(&mut session, Command::CenterLine { a, b: vertical });
        let center = line(&session, center[0]);
        near(center.start.x, center.start.y);
        near(center.end.x, center.end.y);
        assert!(center.start.x < 0.0 && center.end.x > 0.0);
        assert_eq!(center.common.linetype, "CENTER");
    }
}

#[test]
fn symmetry_axes_keep_given_points_and_restore_layers_and_linetypes_on_undo() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("axis.dxf"), units);
        set_scale(&mut session);
        let before = snapshot(&session);
        let steps = session.history_steps().0.len();
        let p1 = [-2.0 / factor, -1.0 / factor];
        let p2 = [2.0 / factor, 1.0 / factor];
        let handles = added(&mut session, Command::SymmetryAxis { p1, p2 });
        let axis = line(&session, handles[0]);
        assert_eq!([axis.start.x, axis.start.y], p1);
        assert_eq!([axis.end.x, axis.end.y], p2);
        assert_eq!(axis.common.layer, "CENTER");
        assert_eq!(axis.common.linetype, "CENTER");
        assert_eq!(session.history_steps().0.len(), steps + 1);
        assert!(session.undo());
        assert_eq!(snapshot(&session), before);
        assert!(session.redo());
        assert!(session
            .drawing
            .doc
            .get_entity(Handle::new(handles[0]))
            .is_some());
    }
}

#[test]
fn labelled_grids_have_paper_sized_bubbles_and_text_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("grid.dxf"), units);
        set_scale(&mut session);
        let before = snapshot(&session);
        let steps = session.history_steps().0.len();
        let handles = added(
            &mut session,
            Command::Grid {
                x: vec![
                    GridAxis {
                        label: "A".into(),
                        at: 0.0,
                    },
                    GridAxis {
                        label: "B".into(),
                        at: 4.0 / factor,
                    },
                ],
                y: vec![
                    GridAxis {
                        label: "1".into(),
                        at: 0.0,
                    },
                    GridAxis {
                        label: "Datum".into(),
                        at: 3.0 / factor,
                    },
                ],
                extent: Some(GridExtent {
                    min: [0.0, 0.0],
                    max: [4.0 / factor, 3.0 / factor],
                }),
                bubble: None,
            },
        );
        assert_eq!(handles.len(), 20);
        assert_eq!(session.history_steps().0.len(), steps + 1);
        assert_eq!(session.current.layer, "0");
        let mut circles = 0;
        let mut labels = Vec::new();
        for handle in handles {
            let entity = session.drawing.doc.get_entity(Handle::new(handle)).unwrap();
            assert_eq!(entity.common().layer, "GRID");
            match entity {
                EntityType::Line(line) => assert_eq!(line.common.linetype, "CENTER"),
                EntityType::Circle(circle) => {
                    near(circle.radius * factor, 0.175);
                    assert_eq!(circle.common.linetype, "Continuous");
                    circles += 1;
                }
                EntityType::Text(text) => {
                    assert_eq!(text.common.linetype, "Continuous");
                    assert_eq!(
                        text.horizontal_alignment,
                        opencadcodec::entities::TextHorizontalAlignment::Center
                    );
                    assert_eq!(
                        text.vertical_alignment,
                        opencadcodec::entities::TextVerticalAlignment::Middle
                    );
                    assert_eq!(text.alignment_point, Some(text.insertion_point));
                    if text.value == "A" {
                        near(text.height * factor, 0.125);
                        assert!(
                            text.insertion_point.y < 0.0 || text.insertion_point.y > 3.0 / factor
                        );
                    }
                    labels.push(text.value.clone());
                }
                _ => panic!("Unexpected grid entity"),
            }
        }
        assert_eq!(circles, 8);
        for label in ["A", "B", "1", "Datum"] {
            assert_eq!(
                labels
                    .iter()
                    .filter(|value| value.as_str() == label)
                    .count(),
                2
            );
        }
        let after = snapshot(&session);
        assert!(session.undo());
        assert_eq!(snapshot(&session), before);
        assert!(session.redo());
        assert_eq!(snapshot(&session), after);
    }
}

#[test]
fn hidden_layers_load_a_scaled_pattern_with_the_same_undo_step_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("hidden.dxf"), units);
        set_scale(&mut session);
        session
            .run(Command::NewLayer {
                name: "HIDDEN".into(),
            })
            .unwrap();
        let before = snapshot(&session);
        let steps = session.history_steps().0.len();
        session
            .run(Command::SetLayer {
                name: "HIDDEN".into(),
                prop: LayerProp::Linetype,
                value: json!("hidden"),
            })
            .unwrap();
        let pattern = session.drawing.doc.line_types.get("HIDDEN").unwrap();
        near(
            pattern.elements[0].length * session.drawing.doc.header.linetype_scale * factor,
            0.3125,
        );
        assert_eq!(session.history_steps().0.len(), steps + 1);
        assert!(session.undo());
        assert_eq!(snapshot(&session), before);
    }
}

#[test]
fn conventions_reuse_existing_linetypes_and_refuse_locked_layers_without_partial_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("existing.dxf"), 6);
    let mut center = LineType::new("Center");
    center.handle = session.drawing.doc.allocate_handle();
    center.elements = vec![LineTypeElement::dash(0.125), LineTypeElement::space(0.025)];
    center.pattern_length = 0.15;
    session.drawing.doc.line_types.add(center.clone()).unwrap();
    added(
        &mut session,
        Command::SymmetryAxis {
            p1: [0.0, 0.0],
            p2: [1.0, 1.0],
        },
    );
    assert_eq!(session.drawing.doc.line_types.get("CENTER"), Some(&center));
    session
        .run(Command::SetLayer {
            name: "CENTER".into(),
            prop: LayerProp::Locked,
            value: json!(true),
        })
        .unwrap();
    let before = snapshot(&session);
    let steps = session.history_steps().0.len();
    assert!(session
        .run(Command::SymmetryAxis {
            p1: [0.0, 0.0],
            p2: [2.0, 1.0]
        })
        .is_err());
    assert_eq!(snapshot(&session), before);
    assert_eq!(session.history_steps().0.len(), steps);

    session
        .run(Command::NewLayer {
            name: "GRID".into(),
        })
        .unwrap();
    session
        .run(Command::SetLayer {
            name: "GRID".into(),
            prop: LayerProp::Locked,
            value: json!(true),
        })
        .unwrap();
    let before = snapshot(&session);
    let steps = session.history_steps().0.len();
    assert!(session
        .run(Command::Grid {
            x: vec![GridAxis {
                label: "A".into(),
                at: 0.0
            }],
            y: vec![],
            extent: None,
            bubble: None,
        })
        .is_err());
    assert_eq!(snapshot(&session), before);
    assert_eq!(session.history_steps().0.len(), steps);
}

#[test]
fn invalid_targets_distances_and_grids_make_no_partial_symbols_or_tables() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("invalid.dxf"), 6);
    let circle = drawing(
        &mut session,
        DrawSpec::Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        },
    );
    let a = drawing(
        &mut session,
        DrawSpec::Line {
            a: [0.0, 0.0],
            b: [1.0, 0.0],
        },
    );
    let b = drawing(
        &mut session,
        DrawSpec::Line {
            a: [2.0, 1.0],
            b: [3.0, 1.0],
        },
    );
    let before = snapshot(&session);
    let steps = session.history_steps().0.len();
    let invalid = [
        Command::CenterMark {
            targets: vec![circle, a],
            extension: None,
        },
        Command::CenterMark {
            targets: vec![circle],
            extension: Some(-1.0),
        },
        Command::CenterMark {
            targets: vec![],
            extension: None,
        },
        Command::CenterLine { a, b },
        Command::CenterLine { a, b: circle },
        Command::SymmetryAxis {
            p1: [0.0, 0.0],
            p2: [0.0, 0.0],
        },
        Command::Grid {
            x: vec![],
            y: vec![],
            extent: None,
            bubble: None,
        },
        Command::Grid {
            x: vec![GridAxis {
                label: "Outside".into(),
                at: 3.0,
            }],
            y: vec![],
            extent: Some(GridExtent {
                min: [0.0, 0.0],
                max: [1.0, 1.0],
            }),
            bubble: None,
        },
        Command::Grid {
            x: vec![GridAxis {
                label: "A".into(),
                at: 0.0,
            }],
            y: vec![],
            extent: None,
            bubble: Some(0.0),
        },
    ];
    for command in invalid {
        assert!(session.run(command).is_err());
        assert_eq!(snapshot(&session), before);
        assert_eq!(session.history_steps().0.len(), steps);
    }
}

#[test]
fn grid_defaults_use_the_sheet_and_scale_and_allow_a_custom_bubble_radius() {
    let dir = tempfile::tempdir().unwrap();
    for (units, factor) in UNITS {
        let mut session = Session::new_blank(&dir.path().join("default_grid.dxf"), units);
        set_scale(&mut session);
        drawing(
            &mut session,
            DrawSpec::Line {
                a: [0.0, 0.0],
                b: [5.0 / factor, 3.0 / factor],
            },
        );
        let handles = added(
            &mut session,
            Command::Grid {
                x: vec![GridAxis {
                    label: "Reference axis".into(),
                    at: 2.0 / factor,
                }],
                y: vec![],
                extent: None,
                bubble: Some(0.3 / factor),
            },
        );
        let axis = line(&session, handles[0]);
        near(axis.start.y * factor, 0.0);
        near(axis.end.y * factor, 3.0);
        for handle in handles {
            if let Some(EntityType::Circle(circle)) =
                session.drawing.doc.get_entity(Handle::new(handle))
            {
                near(circle.radius * factor, 0.3);
            }
        }
    }
}
