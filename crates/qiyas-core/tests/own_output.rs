//! Every registry example that edits drawing geometry is also a drawing-check fixture. Run
//! each against a fresh drawing so an earlier command cannot conceal the output's problems.

use std::collections::{BTreeMap, BTreeSet};

use qiyas_core::check::{CheckReport, CheckScope, Severity};
use qiyas_core::registry::{registry, Effect};
use qiyas_core::{Command, Session, Units};
use serde_json::{json, Value};

/// A blank drawing that draws on a layer of its own, as a careful user does: the guard checks
/// commands, and layer 0 on a sheet is a drafting fault of the drawing, not of the command.
fn guard_session(path: &std::path::Path, units: i16) -> Session {
    let mut session = Session::new_blank(path, units);
    for command in [
        json!({"type": "new_layer", "name": "GUARD"}),
        json!({"type": "set_current", "current": {"layer": "GUARD"}}),
    ] {
        session
            .run(serde_json::from_value(command).unwrap())
            .unwrap();
    }
    session
}

#[path = "support/own_output_records.rs"]
mod record_fixtures;

const UNITS: [i16; 4] = [4, 5, 6, 1];

/// Examples use metres, except the millimetre examples of the drafting conventions. Scale
/// geometric coordinates and lengths, keeping angles, counts, physical metres and paper mm.
fn scale_example(value: &mut Value, factor: f64, command: &str) {
    fn numbers(value: &mut Value, factor: f64) {
        match value {
            Value::Number(n) => *value = json!(n.as_f64().unwrap() * factor),
            Value::Array(values) => values.iter_mut().for_each(|v| numbers(v, factor)),
            _ => {}
        }
    }
    match value {
        Value::Object(map) => {
            for (field, child) in map {
                let geometric = matches!(
                    field.as_str(),
                    "points"
                        | "fit_points"
                        | "holes"
                        | "a"
                        | "b"
                        | "c"
                        | "p"
                        | "p1"
                        | "p2"
                        | "centre"
                        | "major"
                        | "origin"
                        | "at"
                        | "line_point"
                        | "base"
                        | "delta"
                        | "window"
                        | "center"
                        | "insertion_point"
                        | "location"
                        | "vertex"
                        | "arc_point"
                        | "point"
                        | "feature"
                        | "leader_end"
                        | "offset"
                        | "at_end"
                        | "at_point"
                        | "at_a"
                        | "at_b"
                        | "side_point"
                        | "bounds"
                        | "insert_at"
                        | "radius"
                        | "inner"
                        | "outer"
                        | "arc_len"
                        | "thickness"
                        | "width"
                        | "width_units"
                        | "col_width"
                        | "row_height"
                        | "distance"
                        | "real_distance"
                        | "spacing"
                        | "d1"
                        | "d2"
                        | "dx"
                        | "dy"
                        | "step"
                        | "extension"
                        | "tol"
                        | "pairs"
                        | "min"
                        | "max"
                        | "bubble"
                ) || field == "height"
                    && !matches!(command, "walls" | "add_wall_openings" | "edit_wall_opening")
                    || field == "scale" && matches!(command, "import_pdf" | "attach_pdf_underlay")
                    || matches!(field.as_str(), "x" | "y" | "z") && child.is_number()
                    || matches!(field.as_str(), "start" | "end") && child.is_array();
                let geometric = geometric
                    && !(command == "center_line" && matches!(field.as_str(), "a" | "b"))
                    && !(field == "at"
                        && child.is_number()
                        && !matches!(
                            command,
                            "walls" | "grid" | "add_wall_openings" | "edit_wall_opening"
                        ));
                if geometric && !child.is_object() {
                    numbers(child, factor);
                } else {
                    scale_example(child, factor, command);
                }
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|v| scale_example(v, factor, command)),
        _ => {}
    }
}

/// Check both visible parents and their new block contents. A dimension, table, or reusable
/// symbol must not hide malformed geometry inside its definition.
fn check_added(session: &Session, handles: &[u64]) -> Vec<CheckReport> {
    let doc = &session.drawing.doc;
    let mut blocks: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for &handle in handles {
        let entity = doc.get_entity(opencadcodec::Handle::new(handle)).unwrap();
        let mut owner = Some(entity.common().owner_handle);
        let mut name = None;
        // Attributes may belong to an insert, which in turn belongs to a block.
        for _ in 0..64 {
            if let Some(block) = doc
                .block_records
                .iter()
                .find(|block| Some(block.handle) == owner)
            {
                name = Some(block.name.clone());
                break;
            }
            owner = owner
                .and_then(|owner| doc.get_entity(owner))
                .map(|e| e.common().owner_handle);
            if owner.is_none() {
                break;
            }
        }
        let name = name.unwrap_or_else(|| panic!("Added entity {handle} has no owning block"));
        blocks.entry(name).or_default().push(handle);
    }
    blocks
        .into_iter()
        .map(|(sheet, handles)| {
            session.check_drawing(&CheckScope {
                sheet: Some(sheet),
                handles,
                packs: vec!["general".into()],
                ..Default::default()
            })
        })
        .collect()
}

fn assert_clean(session: &Session, handles: Vec<u64>, label: &str) {
    for report in check_added(session, &handles) {
        let bad: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity != Severity::Info)
            .collect();
        assert!(bad.is_empty(), "{label}, block {}: {bad:#?}", report.sheet);
        assert_eq!(
            report.errors + report.warnings,
            0,
            "{label}: {}",
            report.line
        );
    }
}

fn added_by(session: &mut Session, mut command: Value) -> Vec<u64> {
    let base = command["type"].as_str().unwrap().to_string();
    scale_example(&mut command, 1.0 / session.drawing.units.to_metres(), &base);
    let before: BTreeSet<_> = session
        .drawing
        .doc
        .entities()
        .map(|e| e.common().handle.value())
        .collect();
    session
        .run(serde_json::from_value(command).unwrap())
        .unwrap();
    session
        .drawing
        .doc
        .entities()
        .map(|e| e.common().handle.value())
        .filter(|h| !before.contains(h))
        .collect()
}

fn remap_handles(value: &mut Value, handles: &[u64]) {
    match value {
        Value::Number(n) => {
            if let Some(index) = n
                .as_u64()
                .and_then(|h| h.checked_sub(1234))
                .filter(|i| *i < handles.len() as u64)
            {
                *value = json!(handles[index as usize]);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|v| remap_handles(v, handles)),
        Value::Object(values) => values.values_mut().for_each(|v| remap_handles(v, handles)),
        _ => {}
    }
}

/// Minimal prerequisites on a fresh drawing. Only these inputs change: documented example
/// handles are bound to the objects they name; clipboard text comes from its file host.
fn prepare_example(session: &mut Session, name: &str, example: &mut Value) {
    let mut handles = Vec::new();
    if let Some(prepared) = record_fixtures::prepare(session, name, example) {
        handles = prepared;
    } else if matches!(
        name,
        "add_wall_openings" | "remove_wall_openings" | "edit_wall_opening"
    ) {
        let command = json!({"type":"walls", "thickness":0.2, "runs":[{"points":[[0,0],[6,0]]}], "openings":if name == "add_wall_openings" { json!([]) } else { json!([{"run":0,"at":2,"width":0.9,"kind":"door"}]) }});
        let mut command = command;
        scale_example(
            &mut command,
            1.0 / session.drawing.units.to_metres(),
            "walls",
        );
        let qiyas_core::Outcome::Walls { result } = session
            .run(serde_json::from_value(command).unwrap())
            .unwrap()
        else {
            panic!("wall fixture");
        };
        handles = if name == "add_wall_openings" {
            result.centrelines
        } else {
            result.inserts
        };
    } else if name == "edit_text" {
        handles = added_by(
            session,
            json!({"type":"text","at":[0,5],"height":0.25,"value":"ROOM"}),
        );
    } else if name == "find_replace" {
        // Exercise both kinds of text-bearing parent that rebuild their child geometry.
        added_by(
            session,
            json!({"type":"table","at":[0,10],"rows":2,"cols":2,"col_width":4,"row_height":0.8,"data":[["Room","Area"],["BED ROOM","17.39"]]}),
        );
        let dimension = added_by(
            session,
            json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[5,0],"line_point":[2.5,-1]}),
        )
        .into_iter()
        .find(|&h| {
            matches!(
                session.drawing.doc.get_entity(opencadcodec::Handle::new(h)),
                Some(opencadcodec::EntityType::Dimension(_))
            )
        })
        .unwrap();
        added_by(
            session,
            json!({"type":"dim_edit","target":{"handle":dimension},"op":{"text_override":"<> BED ROOM"}}),
        );
    } else if matches!(
        name,
        "dim_edit" | "dim_break" | "dim_space" | "dim_jog_line"
    ) {
        let length = if name == "dim_jog_line" { 10.0 } else { 5.0 };
        for y in if name == "dim_space" {
            vec![-1, -2, -3]
        } else {
            vec![-1]
        } {
            handles.extend(
                added_by(
                    session,
                    json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[length,0],"line_point":[length / 2.0,y]}),
                )
                .into_iter()
                .filter(|&h| {
                    matches!(
                        session.drawing.doc.get_entity(opencadcodec::Handle::new(h)),
                        Some(opencadcodec::EntityType::Dimension(_))
                    )
                }),
            );
        }
        if name == "dim_break" {
            handles.extend(added_by(
                session,
                json!({"type":"draw","kind":"line","a":[1,-2],"b":[1,0]}),
            ));
        }
    } else if matches!(name, "set_image_display" | "calibrate") {
        let path =
            qiyas_core::fixtures::write_png(session.drawing.path.parent().unwrap(), "guard.png");
        handles = added_by(
            session,
            json!({"type":"attach_image","path":path,"insert_at":[0,0],"width_units":20}),
        );
    } else if matches!(name, "delete_constraint" | "set_parameter") {
        for (origin, length) in if name == "set_parameter" {
            vec![(0.0, 5.0), (10.0, 3.0)]
        } else {
            vec![(0.0, 5.0)]
        } {
            let target = added_by(
                session,
                json!({"type":"draw","kind":"line","a":[origin,0],"b":[origin+length,0]}),
            )[0];
            let expression = (length / session.drawing.units.to_metres()).to_string();
            added_by(
                session,
                json!({"type":"add_dim_constraint","kind":"linear","targets":[{"handle":target}],"points":[[origin+1.0,0]],"line_point":[origin+1.0,-1],"expression":expression}),
            );
        }
        if name == "delete_constraint" {
            example["id"] = json!(session.list_constraints()[0].id);
        } else {
            // Parameter expressions contain both dimensionless multipliers and lengths.
            // Scale just the literal length in the documented expression.
            example["expression"] = json!(format!(
                "2*d2 + {}",
                1.5 / session.drawing.units.to_metres()
            ));
        }
    } else if name == "attach_image" {
        example["path"] = json!(qiyas_core::fixtures::write_png(
            session.drawing.path.parent().unwrap(),
            "guard.png",
        ));
    } else if matches!(name, "import_pdf" | "attach_pdf_underlay") {
        assert!(
            qiyas_pdf::available(),
            "The own-output PDF fixtures need the bundled pdfium library"
        );
        // A rotated page exercises vector placement without assuming a text annotation scale.
        example["path"] = json!(qiyas_pdf::fixtures::write_rotated(
            session.drawing.path.parent().unwrap(),
        ));
    } else if name == "redefine_block" {
        let name = example["name"].as_str().unwrap().to_string();
        let definition = json!({"type":"define_block","name":name,"base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":1,"y":0,"z":0}}}]});
        added_by(session, definition);
        added_by(
            session,
            json!({"type":"insert_block","name":name,"at":[0,0]}),
        );
        let source_path = session
            .drawing
            .path
            .parent()
            .unwrap()
            .join("guard-library.dxf");
        let mut source =
            Session::new_blank(&source_path, session.drawing.doc.header.insertion_units);
        added_by(
            &mut source,
            json!({"type":"define_block","name":name,"base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":2,"y":0,"z":0}}}]}),
        );
        qiyas_core::fixtures::write_drawing(&source.drawing.doc, &source_path);
        example["from_path"] = json!(source_path);
    } else if name == "add_dim_constraint" {
        handles = added_by(
            session,
            json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
        );
        if let Some(length) = example["expression"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
        {
            example["expression"] = json!((length / session.drawing.units.to_metres()).to_string());
        }
    } else if matches!(name, "title_block" | "mview") {
        session
            .run(
                serde_json::from_value(
                    json!({"type":"new_layout","name":"Guard sheet","paper":"ISO A1"}),
                )
                .unwrap(),
            )
            .unwrap();
        let paper = session
            .drawing
            .sheets()
            .into_iter()
            .find(|sheet| sheet.name == "Guard sheet")
            .unwrap();
        session
            .run(Command::SetSheet { block: paper.block })
            .unwrap();
    } else if matches!(
        name,
        "center_mark" | "dimension.radius" | "dimension.diameter" | "dimension.arc_length"
    ) {
        handles = added_by(
            session,
            json!({"type":"draw","kind":"arc","centre":[0,0],"radius":1,"start":0,"end":180}),
        );
    } else if matches!(name, "dimension.baseline" | "dimension.continue") {
        handles = added_by(
            session,
            json!({"type":"dimension","kind":"linear","p1":[0,0],"p2":[5,0],"line_point":[2.5,-1]}),
        );
        handles.retain(|&h| {
            matches!(
                session.drawing.doc.get_entity(opencadcodec::Handle::new(h)),
                Some(opencadcodec::EntityType::Dimension(_))
            )
        });
    } else if name == "draw.hatch" {
        handles = added_by(
            session,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[5,4]}),
        );
    } else if name == "field" {
        handles = added_by(
            session,
            json!({"type":"draw","kind":"rectangle","a":[-10,-10],"b":[-5,-6]}),
        );
    } else if name == "insert_block" || name == "modify.explode" {
        let block = example["name"]
            .as_str()
            .unwrap_or("GUARD-BLOCK")
            .to_string();
        added_by(
            session,
            json!({"type":"define_block","name":block,"base":[0,0],"entities":[{"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":1,"y":0,"z":0}}}]}),
        );
        if name == "modify.explode" {
            handles = added_by(
                session,
                json!({"type":"insert_block","name":block,"at":[0,0]}),
            );
        }
    } else if name == "paste_from_clipboard" {
        example["text"] = json!("Guard note");
    } else if matches!(
        name,
        "edit_cell" | "insert_row" | "delete_row" | "insert_col" | "delete_col" | "merge_cells"
    ) {
        handles = added_by(
            session,
            json!({"type":"table","at":[0,10],"rows":3,"cols":2,"col_width":4,"row_height":0.8,"data":[["Room","Area"],["Living","58.28"],["Kitchen","17.39"]]}),
        );
        handles.retain(|&h| {
            matches!(
                session.drawing.doc.get_entity(opencadcodec::Handle::new(h)),
                Some(opencadcodec::EntityType::Table(_))
            )
        });
    } else if name == "edit.pedit" {
        handles = added_by(
            session,
            json!({"type":"draw","kind":"polyline","points":[[0,0],[5,0],[5,5]]}),
        );
    } else if name == "label_spaces" {
        added_by(
            session,
            json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[10,8]}),
        );
        session
            .run(serde_json::from_value(json!({"type":"new_layer","name":"A-ROOM-NAME"})).unwrap())
            .unwrap();
        session
            .run(
                serde_json::from_value(
                    json!({"type":"set_current","current":{"layer":"A-ROOM-NAME"}}),
                )
                .unwrap(),
            )
            .unwrap();
        added_by(
            session,
            json!({"type":"text","at":[2,4],"height":0.25,"value":"Living"}),
        );
    } else if name.starts_with("edit.")
        || name.starts_with("modify.")
        || matches!(
            name,
            "edit"
                | "modify"
                | "center_line"
                | "dimension.quick"
                | "divide_along"
                | "measure_along"
                | "create_block"
                | "create_group"
                | "ungroup"
                | "match_properties"
                | "delete_entities"
                | "add_constraint"
                | "auto_constrain"
                | "delete_constraints_on"
        )
    {
        let end = if name == "edit.trim" {
            10.0
        } else if matches!(name, "edit.extend" | "edit.blend") {
            4.0
        } else {
            5.0
        };
        handles.extend(added_by(
            session,
            json!({"type":"draw","kind":"line","a":[0,0],"b":[end,0]}),
        ));
        let (a, b) = if name == "edit.blend" {
            ([6, 1], [6, 5])
        } else {
            ([5, 0], [5, 5])
        };
        handles.extend(added_by(
            session,
            json!({"type":"draw","kind":"line","a":a,"b":b}),
        ));
        if name == "ungroup" {
            session.run(serde_json::from_value(json!({"type":"create_group","name":"Guard group","targets":handles.iter().map(|h| json!({"handle":h})).collect::<Vec<_>>()})).unwrap()).unwrap();
        }
    }
    remap_handles(example, &handles);
}

#[test]
fn registry_drawing_examples_pass_the_general_check_in_every_unit() {
    let dir = tempfile::tempdir().unwrap();
    let mut problems = Vec::new();
    for units in UNITS {
        let mut produced = BTreeSet::new();
        let mut executed = BTreeSet::new();
        let mut example_count = 0;
        let unit = Units::from_header(units).to_metres();
        for info in registry().commands().iter().filter(|c| {
            c.effect == Effect::Edit
                && c.undo
                    .as_deref()
                    .is_some_and(|part| part.contains("drawing"))
        }) {
            assert!(
                !info.examples.is_empty(),
                "{} needs an executable example",
                info.name
            );
            for (i, example) in info.examples.iter().enumerate() {
                example_count += 1;
                let mut session = guard_session(&dir.path().join("example.dxf"), units);
                let mut example = example.clone();
                let base = info.name.split('.').next().unwrap();
                let origin_unit = if matches!(
                    base,
                    "walls"
                        | "add_wall_openings"
                        | "edit_wall_opening"
                        | "grid"
                        | "symmetry_axis"
                        | "mview"
                        | "define_block"
                        | "import_pdf"
                        | "attach_pdf_underlay"
                        | "calibrate"
                ) {
                    0.001
                } else {
                    1.0
                };
                scale_example(&mut example, origin_unit / unit, base);
                prepare_example(&mut session, &info.name, &mut example);
                let before: BTreeSet<_> = session
                    .drawing
                    .doc
                    .entities()
                    .map(|e| e.common().handle.value())
                    .collect();
                let command: Command = match serde_json::from_value(example.clone()) {
                    Ok(command) => command,
                    Err(error) => {
                        problems.push(format!("{} units {units}: {example}: {error}", info.name));
                        continue;
                    }
                };
                let result = session.run(command);
                let added: Vec<_> = session
                    .drawing
                    .doc
                    .entities()
                    .map(|e| e.common().handle.value())
                    .filter(|h| !before.contains(h))
                    .collect();
                if result.is_err() {
                    assert!(
                        added.is_empty(),
                        "{} failed after adding entities: {result:?}",
                        info.name
                    );
                    problems.push(format!(
                        "{} example {i} units={units} has no executable fixture: {result:?}",
                        info.name
                    ));
                    continue;
                }
                executed.insert(info.name.clone());
                if !added.is_empty() {
                    produced.insert(info.name.clone());
                }
                for report in check_added(&session, &added) {
                    if report.errors + report.warnings > 0 {
                        problems.push(format!(
                            "{} example {i} units={units}, block {}: {:?}",
                            info.name,
                            report.sheet,
                            report
                                .findings
                                .iter()
                                .filter(|f| f.severity != Severity::Info)
                                .collect::<Vec<_>>()
                        ));
                    }
                }
            }
        }
        println!("units {units}: {example_count} examples, {} commands/kinds executed, {} produced geometry", executed.len(), produced.len());
        for info in registry().commands().iter().filter(|c| {
            c.effect == Effect::Edit
                && (c.category == "draw"
                    || c.name.starts_with("dimension.")
                    || matches!(
                        c.name.as_str(),
                        "comment"
                            | "text"
                            | "mtext"
                            | "leader"
                            | "mleader"
                            | "table"
                            | "label_spaces"
                            | "title_block"
                            | "mview"
                            | "attach_image"
                            | "import_pdf"
                            | "attach_pdf_underlay"
                            | "redefine_block"
                            | "add_dim_constraint"
                    ))
        }) {
            if !produced.contains(&info.name) {
                problems.push(format!(
                    "{} has no successful drawing example in units {units}",
                    info.name
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn wall_rooms_junctions_and_openings_pass_the_general_check() {
    let dir = tempfile::tempdir().unwrap();
    let cases = [
        json!({"runs":[{"points":[[0,0],[10,0],[10,8],[0,8]],"closed":true}]}),
        json!({"runs":[{"points":[[0,0],[10,0],[10,8],[0,8],[0,0]]}]}),
        json!({"runs":[{"points":[[0,0],[0,8],[10,8],[10,0],[0,0]]}]}),
        json!({"runs":[{"points":[[0,0],[6,0],[6,5]]}]}),
        json!({"runs":[{"points":[[0,0],[10,0]]},{"points":[[5,0],[5,5]]}]}),
        json!({"runs":[{"points":[[0,0],[10,0]]},{"points":[[5,-5],[5,5]]}]}),
        json!({"runs":[{"points":[[0,0],[10,0],[10,8],[0,8]],"closed":true}],"openings":[{"run":0,"at":0.8,"width":0.9,"kind":"door"},{"run":0,"at":9.2,"width":0.9,"kind":"window"}]}),
        json!({"runs":[{"points":[[0,0],[10,0]]}],"openings":[{"run":0,"at":0.7,"width":0.9,"kind":"door"},{"run":0,"at":9.3,"width":0.9,"kind":"window"}]}),
    ];
    for units in UNITS {
        for (i, case) in cases.iter().enumerate() {
            let mut session = guard_session(&dir.path().join("walls.dxf"), units);
            let mut command = case.clone();
            command["type"] = json!("walls");
            command["thickness"] = json!(0.2);
            scale_example(
                &mut command,
                1.0 / session.drawing.units.to_metres(),
                "walls",
            );
            session
                .run(serde_json::from_value(command).unwrap())
                .unwrap();
            let handles = session
                .drawing
                .doc
                .entities()
                .map(|e| e.common().handle.value())
                .collect();
            assert_clean(&session, handles, &format!("wall case {i}, units {units}"));
        }
    }
}

#[test]
fn small_curves_pass_without_treating_display_chords_as_cad_edges() {
    let dir = tempfile::tempdir().unwrap();
    for units in UNITS {
        for command in [
            json!({"type":"draw","kind":"circle","centre":[0,0],"radius":0.001}),
            json!({"type":"draw","kind":"arc","centre":[0,0],"radius":0.001,"start":0,"end":180}),
            json!({"type":"draw","kind":"donut","centre":[0,0],"inner":0.001,"outer":0.002}),
            json!({"type":"draw","kind":"ellipse","centre":[0,0],"major":[0.002,0],"ratio":0.5}),
        ] {
            let mut session = guard_session(&dir.path().join("small-curve.dxf"), units);
            let label = format!("{command}, units {units}");
            let handles = added_by(&mut session, command);
            assert_clean(&session, handles, &label);
        }
    }
}
