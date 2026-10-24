//! The tools that write into the drawing (text, room areas and labels, tables, shapes, blocks),
//! data-defined element types, the measure-by-handle rule and advice about unmeasured rows. Run through the
//! generic surface as the model calls it. Curve-hint tests also cover the internal executor.

use std::f64::consts::PI;

use opencadcodec::entities::{Polyline2D, Vertex2D};
use opencadcodec::{Arc, CadDocument, EntityType, LwPolyline, Vector2, Vector3};
use qiyas_ai::{execute, surface, ToolError, ToolOutput};
use qiyas_core::fixtures::{sample_doc, write_drawing};
use qiyas_core::{Command, EntityFilter, Session, Source};
use serde_json::{json, Value};

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn internal(session: &mut Session, name: &str, input: Value) -> Result<Value, ToolError> {
    match execute(session, name, &input, Source::Ai)? {
        ToolOutput::Json(v) => Ok(v),
        ToolOutput::Image { .. } => panic!("expected JSON"),
    }
}

fn call(session: &mut Session, tool: &str, op: &str, args: Value) -> Result<Value, ToolError> {
    let input = json!({"op": op, "args": args.to_string()});
    let surface::Route::Session { tool, op, args } =
        surface::route(tool, &input).map_err(ToolError::BadInput)?
    else {
        panic!("a session operation")
    };
    match surface::run_session(session, &tool, &op, &args, Source::Ai)? {
        ToolOutput::Json(value) => Ok(value),
        ToolOutput::Image { .. } => panic!("expected JSON"),
    }
}

fn command(session: &mut Session, command: Value) -> Result<Value, ToolError> {
    call(
        session,
        surface::EXECUTE,
        "run",
        json!({"command": command}),
    )
}

fn read(session: &mut Session, op: &str, args: Value) -> Result<Value, ToolError> {
    call(session, surface::READ, op, args)
}

fn batch(session: &mut Session, args: Value) -> Result<Value, ToolError> {
    call(session, surface::EXECUTE, "batch", args)
}

fn assert_bad_input(error: &ToolError) {
    let detail = error.json();
    assert!(
        matches!(
            detail["code"].as_str(),
            Some("missing_field" | "invalid_value" | "unknown_field" | "refused")
        ),
        "{detail}"
    );
    assert!(
        detail["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "{detail}"
    );
}

fn bounds(session: &Session, handle: u64) -> [[f64; 2]; 2] {
    session
        .find_entities(&EntityFilter {
            limit: 500,
            ..Default::default()
        })
        .rows
        .into_iter()
        .find(|row| row.handle == handle)
        .unwrap()
        .bounds
}

fn handles_of(session: &Session, kind: &str) -> Vec<u64> {
    session
        .find_entities(&EntityFilter {
            kind: Some(kind.to_string()),
            limit: 500,
            ..Default::default()
        })
        .rows
        .iter()
        .map(|r| r.handle)
        .collect()
}

fn count_entities(session: &Session) -> usize {
    session
        .find_entities(&EntityFilter {
            limit: 0,
            ..Default::default()
        })
        .total
}

/// How many undo steps the session has (undone and redone again).
fn steps(s: &mut Session) -> usize {
    let mut n = 0;
    while s.undo() {
        n += 1;
    }
    for _ in 0..n {
        assert!(s.redo());
    }
    n
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// These five trade definitions belong to the test drawing. The engine receives ordinary data
// through the same command discovery, execution and records interface that Mizan uses.
fn element_type_examples() -> Vec<Value> {
    vec![
        json!({
            "name":"Blockwork", "category":"Masonry", "geometry":"band",
            "properties":{
                "thickness":{"kind":"number","unit":"length","default":0.2,"min":0.01},
                "storey_height":{"kind":"number","unit":"length","default":3.0,"min":0.01},
                "grade":{"kind":"choice","default":"A","choices":["A","B"]}
            },
            "vertical":{"height":"storey_height"},
            "plan":{"band_faces":"thickness","hatch":true,"label_template":"{type}: {volume}"},
            "quantities":{"volume":"length * thickness * height","pieces":"count"}
        }),
        json!({
            "name":"Slab", "category":"Concrete floor", "geometry":"region",
            "properties":{"thickness":{"kind":"number","unit":"length","default":0.2,"min":0.01}},
            "plan":{"outline":true,"hatch":true,"label_template":"{type}: {volume}"},
            "quantities":{"volume":"area * thickness","pieces":"count"}
        }),
        json!({
            "name":"Column", "category":"Structure", "geometry":"point",
            "properties":{
                "width":{"kind":"number","unit":"length","default":0.4,"min":0.01},
                "depth":{"kind":"number","unit":"length","default":0.5,"min":0.01},
                "rise":{"kind":"number","unit":"length","default":3.0,"min":0.01}
            },
            "vertical":{"height":"rise"},
            "plan":{
                "symbols":[{"kind":"rectangle","a":["=-width/(2*unit)","=-depth/(2*unit)"],"b":["=width/(2*unit)","=depth/(2*unit)"]}],
                "label_template":"{type}: {volume}"
            },
            "quantities":{"volume":"width * depth * height","pieces":"count"}
        }),
        json!({
            "name":"Duct", "category":"Ventilation", "geometry":"path",
            "properties":{
                "breadth":{"kind":"number","unit":"length","default":0.6,"min":0.01},
                "depth":{"kind":"number","unit":"length","default":0.4,"min":0.01},
                "insulated":{"kind":"bool","default":true},
                "reference":{"kind":"text","default":"AHU-١"}
            },
            "plan":{"band_faces":"breadth","centreline":true,"label_template":"{type}: {volume}"},
            "quantities":{
                "volume":"geometry.length * breadth * depth", "pieces":"count",
                "lining":"if(insulated, length * 2 * (breadth + depth), 0)"
            }
        }),
        json!({
            "name":"Kerb", "category":"External works", "geometry":"band",
            "properties":{
                "width":{"kind":"number","unit":"length","default":0.15,"min":0.01},
                "depth":{"kind":"number","unit":"length","default":0.3,"min":0.01}
            },
            "plan":{"band_faces":"width","label_template":"{type}: {volume}"},
            "quantities":{"volume":"length * width * depth","pieces":"count"}
        }),
    ]
}

fn object_records(s: &mut Session) -> Vec<Value> {
    read(
        s,
        "records",
        json!({"collection":"objects","detail":"full"}),
    )
    .unwrap()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["data"].clone())
        .collect()
}

fn object_record(s: &mut Session, name: &str) -> Value {
    object_records(s)
        .into_iter()
        .find(|record| record["type"] == name)
        .unwrap()
}

fn element_examples(units: i16) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("elements.dxf"), units);
    for definition in element_type_examples() {
        let before = s.history_steps().0.len();
        command(
            &mut s,
            json!({"type":"define_object_type","definition":definition}),
        )
        .unwrap();
        assert_eq!(
            s.history_steps().0.len(),
            before + 1,
            "one type definition is one undo step"
        );
    }
    let k = 1.0 / s.drawing.units.to_metres();
    let before = s.history_steps().0.len();
    command(
        &mut s,
        json!({"type":"place_objects","objects":[
            {"type":"Blockwork","points":[[0,0],[10.0*k,0]]},
            {"type":"Slab","points":[[0,5.0*k],[4.0*k,5.0*k],[4.0*k,8.0*k],[0,8.0*k]]},
            {"type":"Column","points":[[15.0*k,0]]},
            {"type":"Duct","points":[[0,12.0*k],[5.0*k,12.0*k]]},
            {"type":"Kerb","points":[[0,16.0*k],[8.0*k,16.0*k]]}
        ]}),
    )
    .unwrap();
    assert_eq!(
        s.history_steps().0.len(),
        before + 1,
        "all five placements share one undo step"
    );
    (dir, s)
}

fn object_table_cells(s: &Session, handle: u64) -> Vec<Vec<String>> {
    let EntityType::Table(table) = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(handle))
        .unwrap()
    else {
        panic!("object schedule must be an ordinary CAD table")
    };
    (0..table.rows.len())
        .map(|row| {
            (0..table.columns.len())
                .map(|column| table.cell_text(row, column).unwrap_or("").to_owned())
                .collect()
        })
        .collect()
}

fn assert_element_quantities(s: &mut Session, table: u64) {
    let records = object_records(s);
    assert_eq!(records.len(), 5);
    let cells = object_table_cells(s, table);
    for (name, expected) in [
        ("Blockwork", 6.0),
        ("Slab", 2.4),
        ("Column", 0.6),
        ("Duct", 1.2),
        ("Kerb", 0.36),
    ] {
        let record = records.iter().find(|row| row["type"] == name).unwrap();
        assert!(
            near(
                record["quantities"]["volume"].as_f64().unwrap(),
                expected,
                1e-8
            ),
            "{record}"
        );
        assert_eq!(record["quantities"]["pieces"], 1.0);
        let row = cells
            .iter()
            .find(|row| row.first().is_some_and(|text| text == name))
            .unwrap();
        assert!(
            near(row[1].parse::<f64>().unwrap(), expected, 1e-8),
            "{row:?}"
        );
        assert!(s.drawing.doc.entities().any(|entity| matches!(entity, EntityType::Text(text) if text.value.starts_with(&format!("{name}:")) && text.common.extended_data.get_record("QIYAS_OBJECT_PART").is_some())), "{name} must have generated graphics");
        let driver = record["id"].as_u64().unwrap();
        assert!(s
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(driver))
            .unwrap()
            .common()
            .extended_data
            .get_record("QIYAS_OBJECT")
            .is_some());
    }
    assert_eq!(object_record(s, "Duct")["props"]["reference"], "AHU-١");
}

#[test]
fn five_trades_are_defined_drawn_measured_scheduled_and_round_trip_as_data() {
    for units in [6, 4, 1] {
        let (dir, mut s) = element_examples(units);
        for name in [
            "define_object_type",
            "redefine_object_type",
            "delete_object_type",
            "place_objects",
            "set_object_props",
            "object_schedule",
        ] {
            let docs = read(&mut s, "commands", json!({"name":name})).unwrap();
            assert_eq!(docs["name"], name);
            assert!(!docs["doc"].as_str().unwrap().is_empty(), "{docs}");
        }
        let definitions = read(
            &mut s,
            "records",
            json!({"collection":"object_types","detail":"full"}),
        )
        .unwrap();
        assert_eq!(definitions["records"].as_array().unwrap().len(), 5);
        assert_eq!(
            call(&mut s, surface::EXECUTE, "undo_own", json!({})).unwrap()["undone"],
            true
        );
        assert!(object_records(&mut s).is_empty());
        assert_eq!(
            count_entities(&s),
            0,
            "undo removes drivers and generated graphics together"
        );
        assert!(s.redo());
        let k = 1.0 / s.drawing.units.to_metres();
        let before = s.history_steps().0.len();
        let out = command(&mut s, json!({"type":"object_schedule","at":[20.0*k,20.0*k],"columns":["type","volume","pieces"],"group_by":["type"]})).unwrap();
        let table = out["added"][0].as_u64().unwrap();
        assert_eq!(s.history_steps().0.len(), before + 1);
        assert_element_quantities(&mut s, table);
        assert!(s.undo_if(Source::Ai));
        assert!(s
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(table))
            .is_none());
        assert!(s.redo());
        for ext in ["dwg", "dxf"] {
            let path = dir.path().join(format!("saved-{units}.{ext}"));
            s.save_as_headless(&path).unwrap();
            let project = path.with_extension("qiyas");
            if project.exists() {
                std::fs::remove_file(project).unwrap();
            }
            let mut reopened = Session::open(&path).unwrap();
            assert_element_quantities(&mut reopened, table);
            assert_eq!(
                read(
                    &mut reopened,
                    "records",
                    json!({"collection":"object_types"})
                )
                .unwrap()["records"]
                    .as_array()
                    .unwrap()
                    .len(),
                5
            );
            let wall = object_record(&mut reopened, "Blockwork")["id"]
                .as_u64()
                .unwrap();
            command(
                &mut reopened,
                json!({"type":"set_object_props","targets":[wall],"props":{"thickness":0.3}}),
            )
            .unwrap();
            assert!(near(
                object_record(&mut reopened, "Blockwork")["quantities"]["volume"]
                    .as_f64()
                    .unwrap(),
                9.0,
                1e-8
            ));
            assert!(object_table_cells(&reopened, table)
                .iter()
                .any(|row| row[0] == "Blockwork" && row[1].parse::<f64>() == Ok(9.0)));
            assert!(reopened.undo_if(Source::Ai));
            assert_element_quantities(&mut reopened, table);
        }
    }
}

#[test]
fn object_driver_edits_properties_and_type_redefinition_refresh_in_the_same_undo_step() {
    let (_dir, mut s) = element_examples(6);
    let table = command(&mut s, json!({"type":"object_schedule","at":[20,20],"columns":["type","volume"],"group_by":["type"]})).unwrap()["added"][0].as_u64().unwrap();
    let wall = object_record(&mut s, "Blockwork")["id"].as_u64().unwrap();
    let original = object_table_cells(&s, table);
    let before = s.history_steps().0.len();
    command(
        &mut s,
        json!({"type":"modify","kind":"scale","targets":[{"handle":wall}],"base":[0,0],"factor":2}),
    )
    .unwrap();
    assert_eq!(s.history_steps().0.len(), before + 1);
    assert_eq!(
        object_record(&mut s, "Blockwork")["quantities"]["volume"],
        12.0
    );
    assert!(object_table_cells(&s, table)
        .iter()
        .any(|row| row[0] == "Blockwork" && row[1].parse::<f64>() == Ok(12.0)));
    assert!(s.undo_if(Source::Ai));
    assert_eq!(object_table_cells(&s, table), original);
    let mut definition = element_type_examples().remove(0);
    definition["quantities"]["volume"] = json!("length * thickness * height * 2");
    command(
        &mut s,
        json!({"type":"redefine_object_type","definition":definition}),
    )
    .unwrap();
    assert_eq!(s.history_steps().0.len(), before + 1);
    assert_eq!(
        object_record(&mut s, "Blockwork")["quantities"]["volume"],
        12.0
    );
    assert!(object_table_cells(&s, table)
        .iter()
        .any(|row| row[0] == "Blockwork" && row[1].parse::<f64>() == Ok(12.0)));
    assert!(s.drawing.doc.entities().any(
        |entity| matches!(entity,EntityType::Text(text) if text.value == "Blockwork: 12.000")
    ));
    assert!(s.undo_if(Source::Ai));
    assert_eq!(object_table_cells(&s, table), original);
    assert_eq!(
        object_record(&mut s, "Blockwork")["quantities"]["volume"],
        6.0
    );
    let part = s
        .drawing
        .doc
        .entities()
        .find(|e| {
            qiyas_core::xdata::read_json::<Value>(e.common(), "QIYAS_OBJECT_PART")
                .is_some_and(|part| part["owner"] == wall)
        })
        .unwrap()
        .common()
        .handle
        .value();
    batch(
        &mut s,
        json!({"commands":[
            {"type":"set_property","target":{"handle":part},"prop":"color","value":"#ff0000"},
            {"type":"set_property","target":{"handle":part},"prop":"lineweight","value":50}
        ]}),
    )
    .unwrap();
    command(
        &mut s,
        json!({"type":"set_object_props","targets":[wall],"props":{"thickness":0.3}}),
    )
    .unwrap();
    let driver = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(wall))
        .unwrap();
    assert_eq!(
        driver.common().line_weight,
        opencadcodec::LineWeight::from_value(50)
    );
    assert_eq!(s.entity_info(wall, &[]).unwrap().color, "#ff0000");
    for entity in s.drawing.doc.entities().filter(|e| {
        qiyas_core::xdata::read_json::<Value>(e.common(), "QIYAS_OBJECT_PART")
            .is_some_and(|part| part["owner"] == wall)
    }) {
        assert_eq!(entity.common().color, driver.common().color);
        assert_eq!(entity.common().line_weight, driver.common().line_weight);
    }
}

#[test]
fn invalid_object_data_is_refused_without_partial_drivers_graphics_or_undo_steps() {
    let (_dir, mut s) = element_examples(6);
    let wall = object_record(&mut s, "Blockwork")["id"].as_u64().unwrap();
    let before = serde_json::to_value(&s.drawing.doc).unwrap();
    let before_steps = s.history_steps().0.len();
    for input in [
        json!({"type":"delete_object_type","name":"Blockwork"}),
        json!({"type":"define_object_type","definition":element_type_examples()[0]}),
        json!({"type":"set_object_props","targets":[wall],"props":{"thickness":-1}}),
        json!({"type":"set_object_props","targets":[wall],"props":{"grade":"missing"}}),
        json!({"type":"place_objects","objects":[{"type":"Column","points":[[20,0]]},{"type":"Blockwork","points":[[0,30],[10,30]],"props":{"thickness":-1}}]}),
        json!({"type":"place_objects","objects":[{"type":"Slab","points":[[0,0],[1,0],[2,0]]}]}),
    ] {
        assert!(command(&mut s, input.clone()).is_err(), "{input}");
        let after = serde_json::to_value(&s.drawing.doc).unwrap();
        let changed: Vec<_> = after
            .as_object()
            .unwrap()
            .iter()
            .filter(|(key, value)| before.get(*key) != Some(*value))
            .map(|(key, _)| key.as_str())
            .collect();
        assert!(
            after == before,
            "{input}: changed document keys {changed:?}"
        );
        assert_eq!(s.history_steps().0.len(), before_steps, "{input}");
    }
}

// ---- Measuring a curve by handle gives its true length ----

/// Metres. An arc, a polyline with a bulge, a closed one with bulges either way, and a heavy
/// POLYLINE with a bulge.
fn curves() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    // A half circle of radius 2: length 2 pi.
    doc.add_entity(EntityType::Arc(Arc::from_center_radius_angles(
        Vector3::new(0.0, 0.0, 0.0),
        2.0,
        0.0,
        PI,
    )))
    .unwrap();
    // (10,0) to (14,0) a half circle (bulge 1), then straight up 3: 2 pi + 3.
    let mut open = LwPolyline::new();
    open.add_point_with_bulge(Vector2::new(10.0, 0.0), 1.0);
    open.add_point(Vector2::new(14.0, 0.0));
    open.add_point(Vector2::new(14.0, 3.0));
    doc.add_entity(EntityType::LwPolyline(open)).unwrap();
    // A 4 x 3 rectangle whose bottom edge bulges out as a half circle: perimeter 2 pi + 10,
    // area 12 + 2 pi.
    let mut out = LwPolyline::new();
    out.add_point_with_bulge(Vector2::new(20.0, 0.0), 1.0);
    for (x, y) in [(24.0, 0.0), (24.0, 3.0), (20.0, 3.0)] {
        out.add_point(Vector2::new(x, y));
    }
    out.is_closed = true;
    doc.add_entity(EntityType::LwPolyline(out)).unwrap();
    // The same with the bottom edge bent in (bulge -0.5): arc 2.5 * 4 atan(0.5), segment off.
    let mut inward = LwPolyline::new();
    inward.add_point_with_bulge(Vector2::new(30.0, 0.0), -0.5);
    for (x, y) in [(34.0, 0.0), (34.0, 3.0), (30.0, 3.0)] {
        inward.add_point(Vector2::new(x, y));
    }
    inward.is_closed = true;
    doc.add_entity(EntityType::LwPolyline(inward)).unwrap();
    // A heavy POLYLINE like the first open one.
    let mut heavy = Polyline2D::new();
    let mut first = Vertex2D::from_point(Vector2::new(40.0, 0.0));
    first.bulge = 1.0;
    heavy.add_vertex(first);
    heavy.add_vertex(Vertex2D::from_point(Vector2::new(44.0, 0.0)));
    heavy.add_vertex(Vertex2D::from_point(Vector2::new(44.0, 3.0)));
    doc.add_entity(EntityType::Polyline2D(heavy)).unwrap();
    doc
}

#[test]
fn measure_by_handle_gives_the_true_length_of_arcs_and_bulges() {
    let inward_arc = 2.5 * 4.0 * 0.5_f64.atan();
    let inward_segment = 2.5 * 2.5 / 2.0 * (4.0 * 0.5_f64.atan() - 0.96);
    for ext in ["dxf", "dwg"] {
        let (_dir, mut s) = open(&curves(), &format!("curves.{ext}"));
        let length = |s: &mut Session, handle: u64| -> f64 {
            internal(s, "measure_length", json!({ "handle": handle })).unwrap()["measurement"]
                ["shown"]
                .as_f64()
                .unwrap()
        };
        let area = |s: &mut Session, handle: u64| -> f64 {
            internal(s, "measure_area", json!({ "handle": handle })).unwrap()["measurement"]
                ["shown"]
                .as_f64()
                .unwrap()
        };
        let arc = handles_of(&s, "Arc")[0];
        assert!(near(length(&mut s, arc), 2.0 * PI, 1e-6), "{ext}: the arc");
        // The polylines, left to right: the open one, the two closed ones, the heavy one.
        let mut rows = s
            .find_entities(&EntityFilter {
                limit: 500,
                ..Default::default()
            })
            .rows;
        rows.retain(|r| r.kind != "Arc");
        rows.sort_by(|a, b| a.bounds[0][0].total_cmp(&b.bounds[0][0]));
        assert_eq!(rows.len(), 4, "{ext}: {rows:?}");
        let lw: Vec<u64> = rows.iter().map(|r| r.handle).collect();
        assert!(
            near(length(&mut s, lw[0]), 2.0 * PI + 3.0, 1e-6),
            "{ext}: a polyline with a bulge"
        );
        assert!(
            near(length(&mut s, lw[1]), 2.0 * PI + 10.0, 1e-6),
            "{ext}: perimeter, out"
        );
        assert!(
            near(area(&mut s, lw[1]), 12.0 + 2.0 * PI, 1e-6),
            "{ext}: area, out"
        );
        assert!(
            near(length(&mut s, lw[2]), inward_arc + 10.0, 1e-6),
            "{ext}: perimeter, bent in"
        );
        assert!(
            near(area(&mut s, lw[2]), 12.0 - inward_segment, 1e-6),
            "{ext}: area, bent in"
        );
        eprintln!("{ext}: the heavy one is a {}", rows[3].kind);
        assert!(
            near(length(&mut s, lw[3]), 2.0 * PI + 3.0, 1e-6),
            "{ext}: a heavy POLYLINE with a bulge"
        );
    }
}

#[test]
fn points_that_trace_one_drawn_entity_get_a_hint_to_measure_it_by_handle() {
    let (_dir, mut s) = open(&curves(), "hint.dxf");
    let arc = handles_of(&s, "Arc")[0];
    // Clicks along the half circle: the length they give is only roughly the arc's.
    let clicks: Vec<[f64; 2]> = (0..=4)
        .map(|i| {
            let a = PI * f64::from(i) / 4.0;
            [2.0 * a.cos(), 2.0 * a.sin()]
        })
        .collect();
    let out = internal(&mut s, "measure_length", json!({ "points": clicks })).unwrap();
    assert!(out["hint"].as_str().unwrap().contains("handle"), "{out}");
    let candidates = out["candidates"].as_array().unwrap();
    assert!(
        candidates.iter().any(|c| c["handle"] == json!(arc)),
        "{out}"
    );
    assert!(
        out["measurement"]["shown"].as_f64().unwrap() < 2.0 * PI,
        "{out}"
    );

    // By handle there is nothing to say; points between places the user gave have no hint.
    let exact = internal(&mut s, "measure_length", json!({ "handle": arc })).unwrap();
    assert!(exact.get("hint").is_none());
    let free = internal(
        &mut s,
        "measure_length",
        json!({ "points": [[100, 100], [103, 104]] }),
    )
    .unwrap();
    assert!(free.get("hint").is_none(), "{free}");
    assert!(near(
        free["measurement"]["shown"].as_f64().unwrap(),
        5.0,
        1e-9
    ));
}

// ---- Unmeasured items carry advice; missing units remain invalid ----

#[test]
fn unmeasured_takeoff_rows_advise_and_still_require_units() {
    let (_dir, mut s) = open(&sample_doc(), "rows.dxf");
    let err = internal(
        &mut s,
        "add_measurement",
        json!({"kind":"item","description":"Overall Width: 12.00 m"}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("needs a unit"));
    assert!(s.takeoff.items.is_empty());
    assert!(!s.can_undo(), "the invalid item was not recorded");
    for command in [
        json!({"type":"add_measurement","kind":"item","description":"Living Room: 6.70 m x 8.70 m","value":0,"unit":"m"}),
        json!({"type":"add_measurement","kind":"item","description":"Analysis","unit":"note","note":"text"}),
    ] {
        let out = call(&mut s, surface::EXECUTE, "run", json!({"command":command})).unwrap();
        assert!(out["advice"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["code"] == "unmeasured_item"));
        assert_eq!(out["outcome"]["measurement"]["value"], 0.0);
        assert_eq!(s.takeoff.items.last().unwrap().value, 0.0);
    }

    // A real quantity, geometry, or an explicit empty item goes in.
    internal(
        &mut s,
        "add_measurement",
        json!({ "kind": "item", "description": "Cement", "value": 5, "unit": "bags" }),
    )
    .unwrap();
    let wall = handles_of(&s, "Line")[0];
    internal(
        &mut s,
        "add_measurement",
        json!({ "kind": "length", "description": "Wall", "handles": [wall] }),
    )
    .unwrap();
    internal(&mut s, "add_measurement", json!({ "kind": "item", "description": "Provisional sum", "unit": "ls", "empty_item": true })).unwrap();
    assert_eq!(s.takeoff.items.len(), 5);
    // add_markup still makes its own quantity-less row: it is not a take-off quantity.
    internal(
        &mut s,
        "add_markup",
        json!({ "points": [[0, 0]], "text": "look here", "kind": "note" }),
    )
    .unwrap();
    assert_eq!(s.takeoff.items.len(), 6);
}

// ---- add_text ----

#[test]
fn add_text_writes_single_and_multi_line_text_in_one_undo_step() {
    let (_dir, mut s) = open(&sample_doc(), "text.dxf");
    let before = count_entities(&s);
    let before_steps = steps(&mut s);
    let out = batch(&mut s, json!({
        "commands": [
            {"type": "text", "value": "Net area 51.22 m²", "at": [1, 1], "rotation": 0},
            {"type": "text", "value": "الصالة", "at": [2, 2], "height": 0.4, "rotation": 90},
            {"type": "mtext", "value": "Line one\nLine two", "at": [3, 3], "height": 0.3, "width": 0},
            {"type": "mtext", "value": "Wrapped", "at": [4, 4], "width": 2}
        ], "layer": "NOTES"
    })).unwrap();
    assert!(s.drawing.doc.layers.get("NOTES").is_some());
    let texts = out["added"].as_array().unwrap();
    assert_eq!(texts.len(), 4);
    let info = |i: usize| s.entity_info(texts[i].as_u64().unwrap(), &[]).unwrap();
    assert_eq!(info(0).kind, "Text");
    assert_eq!(info(2).kind, "MText");
    assert_eq!(info(3).kind, "MText");
    // 2.5 mm at 1:1 in a drawing in metres.
    assert!(near(info(0).height.unwrap(), 0.0025, 1e-9), "{out}");
    assert!(near(info(1).height.unwrap(), 0.4, 1e-9));
    assert_eq!(count_entities(&s), before + 4);
    assert_eq!(
        steps(&mut s),
        before_steps + 1,
        "one undo step for the call"
    );
    assert_eq!(s.current.layer, "0", "the current layer is put back");
    for text in texts {
        assert_eq!(
            s.entity_info(text.as_u64().unwrap(), &[])
                .unwrap()
                .layer_short,
            "NOTES"
        );
    }
    let text = read(&mut s, "query", json!({"kind": "texts"})).unwrap();
    assert!(text.to_string().contains("الصالة"), "{text}");
    assert!(text.to_string().contains("51.22 m²"), "{text}");
    command(&mut s, json!({"type": "annotation_scale", "name": "1:100"})).unwrap();
    let out = command(
        &mut s,
        json!({"type": "text", "value": "Scaled", "at": [0, 0], "rotation": 0}),
    )
    .unwrap();
    let handle = out["added"][0].as_u64().unwrap();
    assert!(
        near(
            s.entity_info(handle, &[]).unwrap().height.unwrap(),
            0.25,
            1e-9
        ),
        "{out}"
    );
    for _ in 0..3 {
        assert!(s.undo_if(Source::Ai));
    }
    assert_eq!(count_entities(&s), before);
    assert!(s.drawing.doc.layers.get("NOTES").is_none());
}

#[test]
fn add_text_refuses_what_it_cannot_write_and_changes_nothing() {
    let (_dir, mut s) = open(&sample_doc(), "textbad.dxf");
    for input in [
        json!({"commands": []}),
        json!({"commands": [{"type": "text", "value": "  ", "at": [0, 0], "rotation": 0}]}),
        json!({"commands": [{"type": "text", "value": "x", "at": [0, 0], "height": -1, "rotation": 0}]}),
        json!({"commands": [{"type": "mtext", "value": "a\nb", "at": [0, 0], "width": -1}]}),
        json!({"commands": [{"type": "text", "value": "x", "at": [0, 0], "rotation": 0}], "layer": "XREF|WALL"}),
    ] {
        let err = batch(&mut s, input.clone()).unwrap_err();
        assert_bad_input(&err);
    }
    assert!(!s.can_undo());
    let err = batch(
        &mut s,
        json!({"commands": [
            {"type": "text", "value": "ok", "at": [0, 0], "rotation": 0},
            {"type": "text", "value": "bad", "at": [0, 0], "height": -1, "rotation": 0}
        ]}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("height"), "{err}");
    assert!(!s.can_undo());
}

// ---- draw, insert_block, add_table, edit_table_cell ----

#[test]
fn draw_builds_a_legend_in_one_undo_step_on_a_layer_and_colour() {
    let (_dir, mut s) = open(&sample_doc(), "legend.dxf");
    let before = count_entities(&s);
    let out = batch(&mut s, json!({
        "commands": [
            {"type": "draw", "kind": "rectangle", "a": [30, 0], "b": [36, 6]},
            {"type": "draw", "kind": "line", "a": [31, 5], "b": [33, 5]},
            {"type": "draw", "kind": "polyline", "points": [[31, 4], [33, 4], [33, 3]], "bulges": [0.5, 0, 0], "closed": false},
            {"type": "draw", "kind": "hatch", "points": [[31, 1], [33, 1], [33, 2], [31, 2]], "pattern": {"custom": {"name": "ANSI31", "angle": 0, "scale": 0.025}}},
            {"type": "draw", "kind": "circle", "centre": [32, 2.5], "radius": 0.2},
            {"type": "draw", "kind": "arc", "centre": [34, 2.5], "radius": 0.5, "start": 0, "end": 180},
            {"type": "draw", "kind": "revcloud", "points": [[29, -1], [37, -1], [37, 7], [29, 7]], "arc_len": 0.5}
        ], "layer": "LEGEND", "color": "#ff0000"
    })).unwrap();
    assert!(s.drawing.doc.layers.get("LEGEND").is_some());
    let handles = out["added"].as_array().unwrap();
    assert_eq!(handles.len(), 7);
    assert_eq!(count_entities(&s), before + 7);
    for h in handles {
        let info = s.entity_info(h.as_u64().unwrap(), &[]).unwrap();
        assert_eq!(info.layer_short, "LEGEND");
        assert_eq!(info.color, "#ff0000");
    }
    assert_eq!(s.current.layer, "0");
    assert_eq!(s.current.color, "ByLayer");
    assert!(s.undo_if(Source::Ai), "one step");
    assert_eq!(count_entities(&s), before);
    assert!(
        s.drawing.doc.layers.get("LEGEND").is_none(),
        "the made layer goes too"
    );
    assert!(s.redo());
    assert_eq!(count_entities(&s), before + 7);
}

/// Mizan's edit tools: an offset wall, a moved copy, an erase, each one undo step it can take
/// back itself, never the user's.
#[test]
fn modify_edit_erase_and_undo_last_are_steps_of_the_assistants_own() {
    let (_dir, mut s) = open(&sample_doc(), "edit.dxf");
    let before = count_entities(&s);
    let drawn = command(
        &mut s,
        json!({"type": "draw", "kind": "line", "a": [40, 0], "b": [45, 0]}),
    )
    .unwrap();
    let wall = drawn["added"][0].as_u64().unwrap();
    let offset = command(&mut s, json!({"type": "edit", "kind": "offset", "target": {"handle": wall}, "distance": 0.2, "side_point": [42, 1]})).unwrap();
    assert_eq!(offset["added"].as_array().unwrap().len(), 1);
    assert_eq!(count_entities(&s), before + 2);
    let copied = command(&mut s, json!({"type": "modify", "kind": "copy", "targets": [{"handle": wall}], "delta": [0, 5], "count": 2})).unwrap();
    assert_eq!(copied["added"].as_array().unwrap().len(), 2);
    assert_eq!(count_entities(&s), before + 4);
    command(
        &mut s,
        json!({"type": "delete_entities", "targets": [{"handle": wall}]}),
    )
    .unwrap();
    assert_eq!(count_entities(&s), before + 3);
    assert_eq!(
        call(&mut s, surface::EXECUTE, "undo_own", json!({})).unwrap()["undone"],
        true
    );
    assert_eq!(count_entities(&s), before + 4);
    assert_eq!(
        call(&mut s, surface::EXECUTE, "undo_own", json!({})).unwrap()["undone"],
        true
    );
    assert_eq!(count_entities(&s), before + 2);
    s.run(Command::Draw(qiyas_core::DrawSpec::Line {
        a: [0.0, 50.0],
        b: [1.0, 50.0],
    }))
    .unwrap();
    let refused = call(&mut s, surface::EXECUTE, "undo_own", json!({})).unwrap();
    assert_eq!(refused["undone"], false);
    assert!(
        refused["why"].as_str().unwrap().contains("not yours"),
        "{refused}"
    );
    assert_eq!(count_entities(&s), before + 3);
    let err = command(
        &mut s,
        json!({"type": "modify", "kind": "rotate", "targets": [{"handle": wall}], "angle": 90}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("base"), "{err}");
}

/// Fixing a label is changing it: its words and height (metres) in place, no second text.
#[test]
fn set_text_changes_a_label_in_place() {
    let (_dir, mut s) = open(&sample_doc(), "label.dxf");
    let out = command(
        &mut s,
        json!({"type": "text", "value": "MAJLIS", "at": [40, 40], "height": 0.25, "rotation": 0}),
    )
    .unwrap();
    let h = out["added"][0].as_u64().unwrap();
    let before = count_entities(&s);
    let changed = batch(
        &mut s,
        json!({"commands": [
            {"type": "edit_text", "target": {"handle": h}, "value": "MAJLIS / LIVING"},
            {"type": "set_property", "target": {"handle": h}, "prop": "text_height", "value": 0.3}
        ]}),
    )
    .unwrap();
    assert_eq!(changed["changed"], json!([h]));
    assert_eq!(count_entities(&s), before, "no second text");
    let info = s.entity_info(h, &[]).unwrap();
    assert_eq!(info.text.as_deref(), Some("MAJLIS / LIVING"));
    assert!(
        (info.height.unwrap() - 0.3).abs() < 1e-9,
        "metres in a metre drawing"
    );
    assert!(command(
        &mut s,
        json!({"type": "edit_text", "target": {"handle": h}})
    )
    .is_err());
}

/// A pattern hatch Mizan draws is scaled as the Hatch tool's default (0.025 / metres per unit):
/// at scale 1 its millimetre pattern would put lines 3 m apart in a metre plan.
#[test]
fn a_pattern_hatch_is_scaled_for_the_drawings_units() {
    for (insunits, scale) in [(6, 0.025), (4, 25.0)] {
        let mut doc = sample_doc();
        doc.header.insertion_units = insunits;
        let (_dir, mut s) = open(&doc, "hatch.dxf");
        let state = read(&mut s, "state", json!({})).unwrap();
        let unit_scale = 0.025 / state["metres_per_unit"].as_f64().unwrap();
        let out = command(&mut s, json!({"type": "draw", "kind": "hatch", "points": [[31, 1], [33, 1], [33, 2], [31, 2]], "pattern": {"custom": {"name": "ANSI31", "angle": 0, "scale": unit_scale}}})).unwrap();
        let h = out["added"][0].as_u64().unwrap();
        let Some(EntityType::Hatch(hatch)) = s.drawing.doc.get_entity(opencadcodec::Handle::new(h))
        else {
            panic!("a hatch")
        };
        assert!(
            (hatch.pattern_scale - scale).abs() < 1e-12,
            "INSUNITS {insunits}: scale {}",
            hatch.pattern_scale
        );
    }
}

#[test]
fn draw_names_what_a_shape_is_missing() {
    let (_dir, mut s) = open(&sample_doc(), "drawbad.dxf");
    for (shape, word) in [
        (json!({ "kind": "line", "a": [0, 0] }), "b"),
        (json!({ "kind": "circle", "centre": [0, 0] }), "radius"),
        (
            json!({ "kind": "circle", "centre": [0, 0], "radius": 0 }),
            "radius",
        ),
        (
            json!({ "kind": "arc", "centre": [0, 0], "radius": 1, "start": 0 }),
            "end",
        ),
        (
            json!({ "kind": "polyline", "points": [[0, 0]] }),
            "2 points or more",
        ),
        (
            json!({ "kind": "polyline", "points": [[0, 0], [1, 1]], "bulges": [1] }),
            "bulge",
        ),
        (
            json!({ "kind": "hatch", "points": [[0, 0], [1, 1]], "pattern": "solid" }),
            "3 points or more",
        ),
        (
            json!({ "kind": "revcloud", "points": [[0, 0], [1, 0], [1, 1]], "arc_len": "bad" }),
            "arc_len",
        ),
    ] {
        let mut draw = shape.clone();
        draw["type"] = json!("draw");
        let err = command(&mut s, draw).unwrap_err();
        assert_bad_input(&err);
        assert!(err.to_string().contains(word), "{shape}: {err}");
    }
    assert!(!s.can_undo());

    let cloud = command(
        &mut s,
        json!({"type":"draw","kind":"revcloud","points":[[30,0],[31,0],[31,1]]}),
    )
    .unwrap();
    assert!(
        cloud["defaults_used"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["field"] == "/arc_len"
                && entry["value"].as_f64().is_some_and(|length| length > 0.0)),
        "{cloud}"
    );
}

#[test]
fn insert_block_places_references_with_attributes_in_one_step() {
    let (_dir, mut s) = open(&sample_doc(), "insert.dxf");
    let doors = handles_of(&s, "Insert").len();
    let out = batch(
        &mut s,
        json!({"commands": [
        {"type": "insert_block", "name": "DOOR", "at": [30, 0]},
        {"type": "insert_block", "name": "door", "at": [32, 0], "scale": 2, "rotation": 90}
    ], "layer": "LEGEND"}),
    )
    .unwrap();
    assert_eq!(out["added"].as_array().unwrap().len(), 2);
    assert_eq!(handles_of(&s, "Insert").len(), doors + 2);
    for h in out["added"].as_array().unwrap() {
        assert_eq!(
            s.entity_info(h.as_u64().unwrap(), &[]).unwrap().layer_short,
            "LEGEND"
        );
    }
    let err = command(
        &mut s,
        json!({"type": "insert_block", "name": "NOPE", "at": [0, 0]}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("NOPE"), "{err}");
    assert!(s.undo_if(Source::Ai));
    assert_eq!(handles_of(&s, "Insert").len(), doors);
}

#[test]
fn add_table_draws_a_schedule_and_edit_table_cell_changes_it() {
    let (_dir, mut s) = open(&sample_doc(), "table.dxf");
    let out = batch(&mut s, json!({"commands": [{"type": "table", "at": [30, 20], "rows": 3, "cols": 2,
        "data": [["Room", "Area (m²)"], ["Living", "51.22"], ["غرفة النوم", "22.09"]]}], "layer": "SCHEDULE"})).unwrap();
    assert!(s.drawing.doc.layers.get("SCHEDULE").is_some());
    let table = out["added"][0].as_u64().unwrap();
    assert_eq!(s.entity_info(table, &[]).unwrap().layer_short, "SCHEDULE");
    let readback = read(&mut s, "query", json!({"kind": "tables"})).unwrap();
    let schedule = readback["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["source"] == table)
        .unwrap();
    assert_eq!(schedule["rows"].as_array().unwrap().len() + 1, 3);
    assert_eq!(schedule["columns"].as_array().unwrap().len(), 2);
    let text = readback.to_string();
    for cell in [
        "Room",
        "Area (m²)",
        "Living",
        "51.22",
        "غرفة النوم",
        "22.09",
    ] {
        assert!(text.contains(cell), "{cell}: {text}");
    }
    let bounds = bounds(&s, table);
    assert!(near(bounds[1][1], 20.0, 1e-9));
    assert!(bounds[1][0] > bounds[0][0]);
    let before_steps = steps(&mut s);
    batch(&mut s, json!({"commands": [
        {"type": "edit_cell", "target": {"handle": table}, "row": 1, "col": 1, "text": "50.00"},
        {"type": "edit_cell", "target": {"handle": table}, "row": 2, "col": 0, "text": "Bedroom"}
    ]})).unwrap();
    assert_eq!(steps(&mut s), before_steps + 1);
    let text = read(&mut s, "query", json!({"kind": "tables"}))
        .unwrap()
        .to_string();
    assert!(
        text.contains("50.00") && text.contains("Bedroom") && !text.contains("51.22"),
        "{text}"
    );
    for input in [
        json!({"type": "table", "at": [0, 0], "rows": 2, "cols": 1, "data": [["A"], ["1", "2"]]}),
        json!({"type": "table", "at": [0, 0], "rows": 0, "cols": 0, "data": []}),
    ] {
        let err = command(&mut s, input).unwrap_err();
        assert_bad_input(&err);
    }
    let err = batch(&mut s, json!({"commands": []})).unwrap_err();
    assert_bad_input(&err);
}

// ---- Rooms and their net areas ----

fn test_plan() -> (tempfile::TempDir, Session) {
    // A copy: opening a drawing writes a lock file beside it.
    let dir = tempfile::tempdir().unwrap();
    let from = concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/test-plan.dwg");
    let to = dir.path().join("test-plan.dwg");
    std::fs::copy(from, &to).unwrap();
    let session = Session::open(&to).unwrap();
    (dir, session)
}

fn room<'a>(out: &'a Value, name: &str) -> &'a Value {
    out["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == name)
        .unwrap_or_else(|| panic!("no room {name}: {out}"))
}

#[test]
fn room_areas_of_the_test_plan_match_its_documented_numbers() {
    let (_dir, mut s) = test_plan();
    let out = read(&mut s, "rooms", json!({ "text_layer": "A-TEXT" })).unwrap();
    // samples/test-plan.md: living room 6.7 x 8.7 = 58.290, bedroom 22.090, kitchen 17.390 m²
    // (the room polylines' own areas); the fountain (radius 1.5, 7.069 m²) stands in the living
    // room. The 0.4 m column squares at the wall corners overlap the rooms' inset corners by
    // 0.05 x 0.05 = 0.0025 m² each (living 4 corners, bedroom and kitchen 2), and the region
    // the line work encloses is net of them.
    let fountain = PI * 1.5 * 1.5;
    let living = room(&out, "LIVING");
    assert!(
        near(
            living["enclosed_area_m2"].as_f64().unwrap(),
            58.29 - 4.0 * 0.0025,
            1e-6
        ),
        "{out}"
    );
    assert!(
        near(
            living["net_area_m2"].as_f64().unwrap(),
            58.28 - fountain,
            1e-3
        ),
        "{out}"
    );
    let islands = living["islands"].as_array().unwrap();
    assert_eq!(islands.len(), 1, "{out}");
    assert!(
        near(islands[0]["area_m2"].as_f64().unwrap(), fountain, 1e-3),
        "{out}"
    );
    let bedroom = room(&out, "BEDROOM");
    assert!(
        near(
            bedroom["enclosed_area_m2"].as_f64().unwrap(),
            22.09 - 2.0 * 0.0025,
            1e-6
        ),
        "{out}"
    );
    assert!(
        near(bedroom["net_area_m2"].as_f64().unwrap(), 22.085, 1e-6),
        "{out}"
    );
    let kitchen = room(&out, "KITCHEN");
    assert!(
        near(
            kitchen["net_area_m2"].as_f64().unwrap(),
            17.39 - 2.0 * 0.0025,
            1e-6
        ),
        "{out}"
    );
    assert_eq!(
        out["rooms"].as_array().unwrap().len(),
        3,
        "the title is no room: {out}"
    );
    assert_eq!(
        out["skipped"].as_array().unwrap().len(),
        1,
        "the title sits in no space: {out}"
    );
    // Read-only.
    assert!(!s.can_undo());

    // Not subtracting the fountain gives the gross area; names narrow the rooms.
    let out = read(
        &mut s,
        "rooms",
        json!({ "text_layer": "A-TEXT", "names": ["living"], "subtract_islands": false }),
    )
    .unwrap();
    assert_eq!(out["rooms"].as_array().unwrap().len(), 1);
    assert!(near(
        room(&out, "LIVING")["net_area_m2"].as_f64().unwrap(),
        58.28,
        1e-6
    ));

    // Seeds by hand: a point in the kitchen.
    let out = read(
        &mut s,
        "rooms",
        json!({ "seeds": [{ "name": "K", "point": [9.5, 7.5] }, { "point": [-5, -5] }] }),
    )
    .unwrap();
    assert!(
        near(
            room(&out, "K")["net_area_m2"].as_f64().unwrap(),
            17.385,
            1e-6
        ),
        "{out}"
    );
    assert_eq!(out["skipped"].as_array().unwrap().len(), 1, "{out}");
}

#[test]
fn label_spaces_fits_unapproved_candidate_areas_and_a_schedule_follows() {
    let (_dir, mut s) = test_plan();
    let before_steps = steps(&mut s);
    let result = command(
        &mut s,
        json!({"type": "label_spaces", "query": {"text_layer": "A-TEXT"}, "layer": "A-AREA"}),
    )
    .unwrap();
    let out = &result["outcome"]["report"];
    assert_eq!(out["labels_written"], 3, "{out}");
    for row in out["rooms"].as_array().unwrap() {
        assert_eq!(row["measurement_basis"], "unapproved_net");
        assert_eq!(row["area_origin"], "manual_or_entity");
        assert_eq!(row["region_method"], "enclosed_drawn_linework");
        assert_eq!(row["net_accuracy_eligible"], false);
        assert_eq!(row["room_type_status"], "unapproved");
    }
    assert_eq!(out["layer"], "A-AREA");
    assert_eq!(out["layer_created"], true);
    assert_eq!(
        steps(&mut s),
        before_steps + 1,
        "one undo step for every label"
    );
    assert_eq!(s.current.layer, "0");

    let living = room(out, "LIVING");
    let label = living["label_handle"].as_u64().unwrap();
    let info = s.entity_info(label, &[]).unwrap();
    assert_eq!(info.layer_short, "A-AREA");
    assert_eq!(info.text.as_deref(), Some("Area 51.21 m²"));
    // Placement may move away from the name to keep the rendered label inside the room.
    assert_eq!(
        serde_json::to_value(info.position).unwrap(),
        living["label_point"]
    );
    let layout = s.check_drawing(&qiyas_core::check::CheckScope {
        handles: out["rooms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["label_handle"].as_u64().unwrap())
            .collect(),
        rules: vec![
            "label_outside_room".into(),
            "text_over_lines".into(),
            "text_overlaps_text".into(),
        ],
        ..Default::default()
    });
    assert!(layout.complete && layout.findings.is_empty(), "{layout:?}");
    let all = read(&mut s, "query", json!({"kind": "texts"}))
        .unwrap()
        .to_string()
        .replace('\u{00a0}', " ");
    for name in ["BEDROOM", "KITCHEN"] {
        let net = room(out, name)["net_area_m2"].as_f64().unwrap();
        assert!(all.contains(&format!("{net:.2} m²")), "{name}: {all}");
    }

    // A schedule of the same numbers goes in as a table.
    let rows: Vec<Value> = out["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            json!([
                r["name"].as_str().unwrap(),
                format!("{:.2}", r["net_area_m2"].as_f64().unwrap())
            ])
        })
        .collect();
    let mut data = vec![json!(["Room", "Net area m²"])];
    data.extend(rows);
    let table = batch(&mut s, json!({"layer": "A-AREA", "commands": [{"type": "table", "at": [15, 9], "rows": data.len(), "cols": 2, "data": data}]})).unwrap();
    let schedule = read(&mut s, "query", json!({"kind": "tables"}))
        .unwrap()
        .to_string();
    for cell in ["LIVING", "51.21", "BEDROOM", "KITCHEN"] {
        assert!(schedule.contains(cell), "{cell}: {schedule}");
    }
    assert!(table["added"][0].as_u64().is_some());

    // Both are undone one step at a time; the labels go together.
    assert!(s.undo_if(Source::Ai));
    assert!(s.undo_if(Source::Ai));
    let after = read(&mut s, "query", json!({"kind": "texts"}))
        .unwrap()
        .to_string();
    assert!(!after.contains("m²"), "{after}");
    assert_eq!(steps(&mut s), before_steps);
}

// ---- The batch command itself ----

#[test]
fn a_batch_is_one_step_and_fails_whole() {
    let (_dir, mut s) = open(&sample_doc(), "batch.dxf");
    let before = count_entities(&s);
    let line = |x: f64| {
        Command::Draw(qiyas_core::DrawSpec::Line {
            a: [x, 0.0],
            b: [x, 1.0],
        })
    };
    s.run(Command::Batch {
        commands: vec![line(40.0), line(41.0), line(42.0)],
        layer: None,
        color: None,
    })
    .unwrap();
    assert_eq!(count_entities(&s), before + 3);
    assert!(s.undo());
    assert_eq!(count_entities(&s), before);

    // A command that can't run in a batch: refused, nothing drawn.
    let err = s
        .run(Command::Batch {
            commands: vec![
                line(50.0),
                Command::WriteBlock {
                    name: "DOOR".into(),
                    path: "door.dwg".into(),
                },
            ],
            layer: None,
            color: None,
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("can't run inside a batch"),
        "{err}"
    );
    assert_eq!(count_entities(&s), before);

    // A command that fails half way puts back what the earlier ones drew.
    let err = s
        .run(Command::Batch {
            commands: vec![
                line(60.0),
                Command::InsertBlock {
                    name: "NOPE".into(),
                    at: [0.0, 0.0],
                    scale: 1.0,
                    rotation: 0.0,
                    attribute_values: Default::default(),
                    from_path: None,
                },
            ],
            layer: None,
            color: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("NOPE"), "{err}");
    assert_eq!(count_entities(&s), before);

    // On the wire.
    let cmd: Command = serde_json::from_value(json!({
        "type": "batch",
        "commands": [{ "type": "draw", "kind": "line", "a": [0, 0], "b": [1, 0] }]
    }))
    .unwrap();
    assert!(matches!(cmd, Command::Batch { .. }));
}

#[test]
fn candidate_labels_follow_request_language_without_net_approval() {
    for (name, language, expected) in [
        ("LIVING", Some("ar"), "المساحة ٥١٫٢١ م²"),
        ("غرفة", Some("en"), "Area 51.21 m²"),
        ("غرفة", None, "المساحة ٥١٫٢١ م²"),
        ("LIVING", None, "Area 51.21 m²"),
    ] {
        let (_dir, mut s) = test_plan();
        let texts = read(&mut s, "query", json!({"kind": "texts"})).unwrap();
        let at = texts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["text"] == "LIVING")
            .unwrap()["at"]
            .clone();
        let mut input = json!({"type": "label_spaces", "query": {"seeds": [{"name": name, "point": at}]}, "layer": "A-AREA"});
        if let Some(language) = language {
            input["language"] = json!(language);
        }
        let result = command(&mut s, input).unwrap();
        let out = &result["outcome"]["report"];
        assert_eq!(out["labels_written"], 1, "{out}");
        let row = &out["rooms"][0];
        assert_eq!(row["measurement_basis"], "unapproved_net");
        assert_eq!(row["net_accuracy_eligible"], false);
        let handle = row["label_handle"].as_u64().unwrap();
        assert_eq!(
            s.entity_info(handle, &[]).unwrap().text.as_deref(),
            Some(expected)
        );
    }
    let (_dir, mut s) = test_plan();
    assert!(command(
        &mut s,
        json!({"type": "label_spaces", "query": {}, "language": "fr"})
    )
    .is_err());
}
