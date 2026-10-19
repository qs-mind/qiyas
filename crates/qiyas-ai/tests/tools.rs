use opencadcodec::{CadDocument, DxfWriter, EntityType, Line, Text, Vector3};
use qiyas_ai::{catalogue, execute, ToolError, ToolOutput};
use qiyas_core::{Boq, BoqItem, Command, EntityFilter, Kind, Session, Source};
use serde_json::{json, Value};

/// Opens the sample drawing (DXF) in a session. The temp dir must outlive the test body.
fn sample() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

/// The handles of the sample's entities of `kind` (e.g. "Insert"), in drawing order.
fn handles_of(session: &Session, kind: &str) -> Vec<u64> {
    session
        .find_entities(&EntityFilter {
            kind: Some(kind.to_string()),
            ..Default::default()
        })
        .rows
        .iter()
        .map(|r| r.handle)
        .collect()
}

fn json_of(output: ToolOutput) -> Value {
    match output {
        ToolOutput::Json(v) => v,
        ToolOutput::Image { .. } => panic!("expected JSON, got an image"),
    }
}

fn run(session: &mut Session, name: &str, input: Value) -> Result<Value, ToolError> {
    execute(session, name, &input, Source::Ai).map(json_of)
}

/// Schema keywords the API's strict tool mode does not accept.
const UNSUPPORTED: [&str; 7] = [
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "oneOf",
];

fn assert_strict_keywords(name: &str, v: &Value) {
    match v {
        Value::Object(map) => {
            for key in UNSUPPORTED {
                assert!(!map.contains_key(key), "{name} uses {key}");
            }
            if map.get("type") == Some(&json!("object")) {
                assert_eq!(
                    map.get("additionalProperties"),
                    Some(&json!(false)),
                    "{name}"
                );
            }
            map.values().for_each(|v| assert_strict_keywords(name, v));
        }
        Value::Array(items) => items.iter().for_each(|v| assert_strict_keywords(name, v)),
        _ => {}
    }
}

#[test]
fn catalogue_schemas_are_strict_objects() {
    let tools = catalogue();
    let names: Vec<&str> = tools.iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        [
            "list_drawings",
            "new_drawing",
            "open_drawing",
            "switch_drawing",
            "save_drawing",
            "close_drawing",
            "list_sheets",
            "set_sheet",
            "list_layers",
            "show_layers",
            "find_entities",
            "get_entity",
            "read_text",
            "read_tables",
            "read_dimensions",
            "block_summary",
            "measure_length",
            "measure_area",
            "count_blocks",
            "add_measurement",
            "set_height",
            "list_takeoff",
            "remove_measurement",
            "set_description",
            "import_boq",
            "list_boq",
            "link_boq",
            "compare_boq",
            "list_conflicts",
            "resolve_conflict",
            "add_markup",
            "add_dimension",
            "set_annotation_scale",
            "add_comment",
            "list_comments",
            "measure_geometry",
            "divide_along",
            "measure_along",
            "id_point",
            "extract_data",
            "zoom_to",
            "add_text",
            "room_areas",
            "label_spaces",
            "add_table",
            "edit_table_cell",
            "draw",
            "insert_block",
            "modify",
            "edit",
            "erase",
            "set_text",
            "undo_last",
            "view_image",
        ]
    );
    for tool in &tools {
        let schema = &tool.input_schema;
        assert_eq!(schema["type"], "object", "{}", tool.name);
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{}",
            tool.name
        );
        let properties = schema["properties"].as_object().unwrap();
        let required = schema["required"].as_array().unwrap();
        for r in required {
            assert!(
                properties.contains_key(r.as_str().unwrap()),
                "{}: {r} is required but not a property",
                tool.name
            );
        }
        assert!(!tool.description.is_empty(), "{}", tool.name);
        assert_strict_keywords(tool.name, schema);
    }
}

#[test]
fn execute_measure_area_by_handle_marks_ai_source() {
    let (_dir, mut session) = sample();
    let rectangle = handles_of(&session, "LwPolyline")[0];
    let out = run(&mut session, "measure_area", json!({ "handle": rectangle })).unwrap();
    let m = &out["measurement"];
    assert_eq!(m["source"], "ai");
    assert_eq!(m["kind"], "area");
    assert_eq!(m["unit"], "m2");
    assert!((m["shown"].as_f64().unwrap() - 12.0).abs() < 1e-9, "{m}");
    assert_eq!(session.takeoff.items.len(), 1);
    assert_eq!(session.takeoff.items[0].source, "ai");
    assert_eq!(session.takeoff.items[0].handles, [rectangle]);

    // By points, from MCP.
    execute(
        &mut session,
        "measure_length",
        &json!({ "points": [[0, 0], [3, 0], [3, 4]] }),
        Source::Mcp,
    )
    .unwrap();
    let length = &session.takeoff.items[1];
    assert_eq!((length.kind, length.value), (Kind::Length, 7.0));
    assert_eq!(length.source, "mcp");

    // Both carry a default description; one the model gives is its own.
    assert!(session.takeoff.items.iter().all(|m| m.auto_description));
    assert_eq!(m["auto_description"], true);
    let out = run(
        &mut session,
        "add_measurement",
        json!({ "kind": "area", "handles": [rectangle], "description": "Slab" }),
    )
    .unwrap();
    assert_eq!(out["measurement"]["auto_description"], false);
}

#[test]
fn execute_find_entities_truncates_at_40kb() {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for i in 0..2000 {
        let y = i as f64;
        let line = Line::from_coords(0.0, y, 0.0, 10.0, y, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lines.dxf");
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    let mut session = Session::open(&path).unwrap();

    let out = run(&mut session, "find_entities", json!({ "limit": 500 })).unwrap();
    assert_eq!(out["truncated"], true);
    assert_eq!(out["total"], 2000);
    let rows = out["rows"].as_array().unwrap().len();
    assert!(rows > 0 && rows < 500, "{rows}");
    assert_eq!(out["next_offset"], rows);
    assert!(serde_json::to_vec(&out).unwrap().len() <= 40_960);

    // The next page starts where this one stopped.
    let next = run(
        &mut session,
        "find_entities",
        json!({ "limit": 500, "offset": rows }),
    )
    .unwrap();
    assert_eq!(
        next["rows"][0]["handle"],
        handles_at(&session, rows),
        "{}",
        next["rows"][0]
    );
}

#[test]
fn extract_data_is_paged_under_40kb() {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for i in 0..3000 {
        let y = i as f64;
        let line = Line::from_coords(0.0, y, 0.0, 10.0, y, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lines.dxf");
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    let mut session = Session::open(&path).unwrap();
    let fields = json!(["handle", "kind", "layer", "length", "x", "y", "sheet"]);

    let out = run(
        &mut session,
        "extract_data",
        json!({ "fields": fields, "limit": 5000 }),
    )
    .unwrap();
    assert_eq!(out["truncated"], true);
    assert_eq!(out["total"], 3000);
    assert_eq!(
        out["columns"],
        json!([
            "Handle",
            "Kind",
            "Layer",
            "Length (m)",
            "X (m)",
            "Y (m)",
            "Sheet"
        ])
    );
    let rows = out["rows"].as_array().unwrap().len();
    assert!(rows > 0 && rows < 3000, "{rows}");
    assert_eq!(out["next_offset"], rows);
    assert!(serde_json::to_vec(&out).unwrap().len() <= 40_960);
    assert_eq!(out["rows"][0][3], "10.000");

    let next = run(
        &mut session,
        "extract_data",
        json!({ "fields": ["handle"], "limit": 2, "offset": rows }),
    )
    .unwrap();
    assert_eq!(next["rows"][0][0], handles_at(&session, rows).to_string());
    assert_eq!(next["next_offset"], rows + 2);
    assert!(next.get("truncated").is_none());
    assert!(session.takeoff.items.is_empty());
}

#[test]
fn inquiry_tools_measure_divide_and_extract() {
    let (_dir, mut session) = sample();
    let line = handles_of(&session, "Line")[0];
    let out = run(&mut session, "measure_geometry", json!({ "handle": line })).unwrap();
    assert_eq!(out["title"], "Line");
    assert!(out["rows"]
        .as_array()
        .unwrap()
        .contains(&json!(["length", "10.000 m"])));

    let out = run(&mut session, "id_point", json!({ "point": [2.5, 1] })).unwrap();
    assert_eq!(out["rows"], json!([["x", "2.500 m"], ["y", "1.000 m"]]));

    let out = run(
        &mut session,
        "divide_along",
        json!({ "handle": line, "count": 4 }),
    )
    .unwrap();
    assert_eq!(out["measurement"]["value"], 3.0);
    assert_eq!(out["measurement"]["source"], "ai");
    assert_eq!(out["ui"]["action"], "redraw");
    assert!(session.undo_if(Source::Ai));
    assert!(session.takeoff.items.is_empty());

    // The step is in metres: every 2.5 m along the 10 m line.
    let out = run(
        &mut session,
        "measure_along",
        json!({ "handle": line, "step": 2.5 }),
    )
    .unwrap();
    assert_eq!(
        out["measurement"]["points"],
        json!([[2.5, 0.0], [5.0, 0.0], [7.5, 0.0]])
    );

    let out = run(
        &mut session,
        "extract_data",
        json!({ "block_name": "DOOR", "fields": ["block_name", "x"], "attributes": ["MARK"] }),
    )
    .unwrap();
    assert_eq!(out["columns"], json!(["Block", "X (m)", "MARK"]));
    assert_eq!(out["total"], 3);
    assert_eq!(out["rows"][0], json!(["DOOR", "1.000", ""]));
    assert!(out["next_offset"].is_null());

    let err = run(
        &mut session,
        "extract_data",
        json!({ "fields": [{ "attribute": "MARK" }] }),
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)));
    let err = run(
        &mut session,
        "divide_along",
        json!({ "handle": line, "count": 1 }),
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)));
}

/// The handle of the `offset`-th entity of the current sheet.
fn handles_at(session: &Session, offset: usize) -> Value {
    json!(
        session
            .find_entities(&EntityFilter {
                offset,
                limit: 1,
                ..Default::default()
            })
            .rows[0]
            .handle
    )
}

/// Dark pixels in a PNG (any channel under 128).
fn dark_pixels(png: &[u8]) -> usize {
    let pixmap = tiny_skia::Pixmap::decode_png(png).expect("a PNG");
    pixmap
        .pixels()
        .iter()
        .filter(|p| p.red() < 128 || p.green() < 128 || p.blue() < 128)
        .count()
}

/// Mizan looks at what the engineer sees: a text is drawn as its letters (it once saw nothing
/// of text, and drew labels again over labels it couldn't see), and a solid hatch as a fill.
#[test]
fn view_image_shows_text_and_solid_fills() {
    let (_dir, mut session) = sample();
    let picture = |session: &mut Session| {
        let out = execute(
            session,
            "view_image",
            &json!({ "width": 800, "bounds": [[100, 100], [110, 105]] }),
            Source::Ai,
        )
        .unwrap();
        let ToolOutput::Image { png, caption } = out else {
            panic!("expected an image");
        };
        assert!(caption.contains("text"), "{caption}");
        dark_pixels(&png)
    };
    assert_eq!(picture(&mut session), 0, "an empty corner of the sheet");
    execute(
        &mut session,
        "add_text",
        &json!({ "texts": [{ "text": "MAJLIS", "point": [101, 102], "height": 1.0 }] }),
        Source::Ai,
    )
    .unwrap();
    let text = picture(&mut session);
    assert!(text > 500, "the letters: {text} dark pixels");
    execute(
        &mut session,
        "draw",
        &json!({ "shapes": [{ "kind": "hatch", "points": [[107, 101], [109, 101], [109, 104], [107, 104]] }] }),
        Source::Ai,
    )
    .unwrap();
    let filled = picture(&mut session) - text;
    // A 2 x 3 unit square at 80 px a unit: about 38,400 pixels.
    assert!(filled > 30_000, "the solid fill: {filled} dark pixels");
}

#[test]
fn view_image_is_a_png_of_the_right_size() {
    let (_dir, mut session) = sample();
    let out = execute(
        &mut session,
        "view_image",
        &json!({ "width": 640 }),
        Source::Ai,
    )
    .unwrap();
    let ToolOutput::Image { png, caption } = out else {
        panic!("expected an image");
    };
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!(width, 640);
    assert!((1..=1600).contains(&height), "{height}");
    assert!(caption.contains("bounds"), "{caption}");

    // Bounds given: a 2:1 box gives a 2:1 picture.
    let out = execute(
        &mut session,
        "view_image",
        &json!({ "width": 400, "bounds": [[0, 0], [10, 5]] }),
        Source::Ai,
    )
    .unwrap();
    let ToolOutput::Image { png, .. } = out else {
        panic!("expected an image");
    };
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!(height, 200);
}

#[test]
fn unknown_tool_is_an_error() {
    let (_dir, mut session) = sample();
    let err = execute(&mut session, "delete_everything", &json!({}), Source::Ai).unwrap_err();
    assert!(matches!(err, ToolError::UnknownTool(ref n) if n == "delete_everything"));
    // Every tool in the catalogue is known to `execute`, but the file tools: the app's host runs
    // those, outside the session.
    for tool in catalogue()
        .into_iter()
        .filter(|t| !qiyas_ai::is_host_tool(t.name))
    {
        let result = execute(&mut session, tool.name, &json!({}), Source::Ai);
        assert!(
            !matches!(result, Err(ToolError::UnknownTool(_))),
            "{} is not dispatched",
            tool.name
        );
    }
}

#[test]
fn add_markup_lands_on_the_markup_layer() {
    let (dir, mut session) = sample();
    let out = run(
        &mut session,
        "add_markup",
        json!({
            "points": [[0, 0], [4, 0], [4, 3]],
            "text": "Check this wall",
            "kind": "cloud"
        }),
    )
    .unwrap();
    assert_eq!(out["ui"]["action"], "show_markup");
    let m = &session.takeoff.items[0];
    assert_eq!(
        (m.kind, m.value, m.unit.as_str()),
        (Kind::Item, 0.0, "markup")
    );
    assert_eq!(
        (m.description.as_str(), m.note.as_str()),
        ("Check this wall", "cloud")
    );

    let out = dir.path().join("out.dxf");
    session.save_as(&out).unwrap();
    std::fs::remove_file(qiyas_core::project_path(&out)).unwrap();
    let reopened = Session::open(&out).unwrap();
    let markup_layer: Vec<&str> = reopened
        .drawing
        .doc
        .entities()
        .filter(|e| e.common().extended_data.get_record("QIYAS").is_some())
        .map(|e| e.common().layer.as_str())
        .collect();
    assert_eq!(markup_layer, ["QIYAS-MARKUP"]);
    let back = &reopened.takeoff.items[0];
    assert_eq!(
        (back.unit.as_str(), back.description.as_str()),
        ("markup", "Check this wall")
    );
}

#[test]
fn count_with_non_insert_handle_is_bad_input() {
    let (_dir, mut session) = sample();
    let line = handles_of(&session, "Line")[0];
    let doors = handles_of(&session, "Insert");
    let count =
        |handles: Vec<u64>| json!({ "kind": "count", "handles": handles, "description": "Doors" });

    let err = run(&mut session, "add_measurement", count(vec![doors[0], line])).unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");
    let err = run(
        &mut session,
        "add_measurement",
        count(vec![doors[0], doors[0]]),
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");
    assert!(session.takeoff.items.is_empty());

    let out = run(&mut session, "add_measurement", count(doors)).unwrap();
    assert_eq!(out["measurement"]["value"], 3.0);
    assert_eq!(out["measurement"]["source"], "ai");
}

#[test]
fn missing_handles_blocks_and_ids_are_bad_input() {
    let (_dir, mut session) = sample();
    for (name, input) in [
        ("measure_length", json!({ "handle": 999_999 })),
        ("measure_area", json!({ "handle": 999_999 })),
        ("measure_area", json!({})),
        ("count_blocks", json!({ "block_name": "WINDOW" })),
        ("get_entity", json!({ "handle": 999_999 })),
        ("set_description", json!({ "id": "nope", "text": "x" })),
        ("remove_measurement", json!({ "id": "nope" })),
        ("set_sheet", json!({ "sheet": "Layout 9" })),
        (
            "show_layers",
            json!({ "names": ["NO-SUCH"], "visible": false }),
        ),
        ("find_entities", json!({ "colour": "red" })),
    ] {
        let err = run(&mut session, name, input).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{name}: {err:?}");
    }
    assert!(session.takeoff.items.is_empty());

    // Block names match case-insensitively.
    let out = run(
        &mut session,
        "count_blocks",
        json!({ "block_name": "door" }),
    )
    .unwrap();
    assert_eq!(out["measurement"]["value"], 3.0);
}

#[test]
fn compare_and_conflict_tools_work() {
    let (_dir, mut session) = sample();
    let item = |code: &str, description: &str, unit: &str, quantity: f64| BoqItem {
        code: code.to_string(),
        description: description.to_string(),
        unit: unit.to_string(),
        quantity,
        rate: None,
        section: String::new(),
        tolerance: None,
        row: 2,
    };
    let bill = Boq::new(
        "boq.xlsx".to_string(),
        vec![
            item("A.1", "Walls", "m", 100.0),
            item("A.2", "Roof waterproofing", "m2", 50.0),
        ],
        0,
    );
    session.run(Command::SetBoq { boq: bill }).unwrap();
    let out = run(
        &mut session,
        "measure_length",
        json!({ "points": [[0, 0], [10, 0]] }),
    )
    .unwrap();
    let id = out["measurement"]["id"].as_str().unwrap().to_string();
    run(&mut session, "link_boq", json!({ "id": id, "code": "A.1" })).unwrap();
    assert_eq!(session.takeoff.items[0].boq_code.as_deref(), Some("A.1"));

    let out = run(&mut session, "compare_boq", json!({})).unwrap();
    assert_eq!(out["total"], 2, "{out}");
    assert_eq!(out["by_kind"]["quantity_mismatch"], 1, "{out}");
    assert_eq!(out["by_kind"]["not_in_drawings"], 1, "{out}");
    assert_eq!(out["by_status"]["open"], 2, "{out}");
    assert_eq!(session.conflicts.len(), 2);
    assert_eq!(session.top_step_source(), Some(Source::Ai));
    let mismatch = out["rows"][0].clone();
    assert_eq!(mismatch["kind"], "quantity_mismatch");
    assert_eq!(mismatch["boq_code"], "A.1");
    assert_eq!(mismatch["expected"], 100.0);
    assert_eq!(mismatch["found"], 10.0);
    let conflict_id = mismatch["id"].as_str().unwrap().to_string();

    let out = run(
        &mut session,
        "resolve_conflict",
        json!({ "id": conflict_id, "status": "ignored", "note": "Walls are on sheet 2" }),
    )
    .unwrap();
    assert_eq!(out["conflict"]["status"], "ignored");
    for input in [
        json!({ "id": "nope", "status": "resolved", "note": "" }),
        json!({ "id": conflict_id, "status": "done", "note": "" }),
    ] {
        let err = run(&mut session, "resolve_conflict", input).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");
    }

    // A new comparison keeps the status and note; a wider tolerance does not change the ids.
    let out = run(&mut session, "compare_boq", json!({ "tolerance": 10 })).unwrap();
    assert_eq!(out["rows"][0]["id"], conflict_id.as_str());
    assert_eq!(out["rows"][0]["status"], "ignored");
    assert_eq!(out["rows"][0]["note"], "Walls are on sheet 2");
    let list = run(&mut session, "list_conflicts", json!({ "offset": 1 })).unwrap();
    assert_eq!(list["total"], 2);
    assert_eq!(list["rows"].as_array().unwrap().len(), 1);
    assert_eq!(list["by_status"]["ignored"], 1);

    let err = run(&mut session, "compare_boq", json!({ "tolerance": -1 })).unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");
}

#[test]
fn compare_counts_blocks_on_layers_the_user_hides() {
    // The sample (three DOOR references on A-DOOR) with a schedule saying 2 doors.
    let mut doc = qiyas_core::fixtures::sample_doc();
    let grid = [["MARK", "QTY"], ["DOOR", "2"], ["WINDOW", "1"]];
    for (r, row) in grid.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            let at = Vector3::new(100.0 + 2.0 * c as f64, 100.0 - 0.5 * r as f64, 0.0);
            let text = Text::with_value(*value, at).with_height(0.25);
            doc.add_entity(EntityType::Text(text)).unwrap();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doors.dxf");
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    let mut session = Session::open(&path).unwrap();
    session.set_layer_hidden("A-DOOR", true);

    let out = run(&mut session, "compare_boq", json!({})).unwrap();
    assert_eq!(out["total"], 1, "{out}");
    assert_eq!(out["rows"][0]["kind"], "schedule_vs_plan");
    assert_eq!(out["rows"][0]["expected"], 2.0);
    assert_eq!(out["rows"][0]["found"], 3.0, "hidden doors still count");
}

#[test]
fn heights_are_metres_and_zoom_returns_bounds() {
    let (_dir, mut session) = sample();
    let out = run(
        &mut session,
        "add_measurement",
        json!({
            "kind": "volume",
            "points": [[0, 0], [4, 0], [4, 3], [0, 3]],
            "height": 0.2,
            "description": "Slab"
        }),
    )
    .unwrap();
    assert!((out["measurement"]["shown"].as_f64().unwrap() - 2.4).abs() < 1e-9);

    let doors = handles_of(&session, "Insert");
    let out = run(&mut session, "zoom_to", json!({ "handles": [doors[0]] })).unwrap();
    assert_eq!(out["ui"]["action"], "zoom");
    // The plain door's leaf runs from (1, 1) to (1.9, 1).
    let b = &out["ui"]["bounds"];
    assert!((b[0][0].as_f64().unwrap() - 1.0).abs() < 1e-6, "{b}");
    assert!((b[1][0].as_f64().unwrap() - 1.9).abs() < 1e-6, "{b}");

    let takeoff = run(&mut session, "list_takeoff", json!({})).unwrap();
    assert_eq!(takeoff["rows"].as_array().unwrap().len(), 1);
    assert_eq!(takeoff["totals"][0]["unit"], "m3");
}

#[test]
fn import_boq_and_list_boq() {
    let (dir, mut session) = sample();
    let err = run(&mut session, "list_boq", json!({})).unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");

    let path = dir.path().join("boq.xlsx");
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet
        .write_row(0, 0, ["Item", "Description", "Unit", "Quantity"])
        .unwrap();
    sheet.write_string(1, 1, "Finishes").unwrap();
    sheet
        .write_row(2, 0, ["F.1", "Floor tiles", "sqm"])
        .unwrap();
    sheet.write_number(2, 3, 48.0).unwrap();
    book.save(&path).unwrap();

    let missing = json!({ "path": dir.path().join("nope.xlsx") });
    let err = run(&mut session, "import_boq", missing).unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err:?}");

    let out = run(&mut session, "import_boq", json!({ "path": path })).unwrap();
    assert_eq!(out["items"], 1);
    assert_eq!(out["sections"], json!(["Finishes"]));
    assert_eq!(session.top_step_source(), Some(Source::Ai));

    let out = run(&mut session, "list_boq", json!({})).unwrap();
    assert_eq!(out["total"], 1);
    assert_eq!(out["rows"][0]["code"], "F.1");
    assert_eq!(out["rows"][0]["unit"], "m2");
    assert_eq!(out["rows"][0]["section"], "Finishes");

    assert!(session.undo_if(Source::Ai));
    assert_eq!(session.boq, None);
}

/// A one-item BOQ workbook at `path`.
fn write_boq(path: &std::path::Path) {
    let mut book = rust_xlsxwriter::Workbook::new();
    let sheet = book.add_worksheet();
    sheet
        .write_row(0, 0, ["Item", "Description", "Unit", "Quantity"])
        .unwrap();
    sheet
        .write_row(1, 0, ["F.1", "Floor tiles", "sqm"])
        .unwrap();
    sheet.write_number(1, 3, 48.0).unwrap();
    book.save(path).unwrap();
}

const OUTSIDE: &str = "Only files beside the open drawing can be opened by the assistant; use the BOQ tab's Import for other files";

#[test]
fn import_boq_opens_files_beside_the_drawing_only() {
    // outer/x.xlsx, outer/plan/sample.dxf, outer/plan/test-boq.xlsx, outer/plan/boq/sub.xlsx.
    let outer = tempfile::tempdir().unwrap();
    let dir = outer.path().join("plan");
    std::fs::create_dir_all(dir.join("boq")).unwrap();
    let mut session = Session::open(&qiyas_core::fixtures::write_sample(&dir, "dxf")).unwrap();
    write_boq(&dir.join("test-boq.xlsx"));
    write_boq(&dir.join("boq").join("sub.xlsx"));

    // Relative paths are read from the drawing's folder, whatever the process's own folder is.
    let out = run(
        &mut session,
        "import_boq",
        json!({ "path": "test-boq.xlsx" }),
    )
    .unwrap();
    assert_eq!(out["items"], 1, "{out}");
    let out = run(
        &mut session,
        "import_boq",
        json!({ "path": "boq/sub.xlsx" }),
    )
    .unwrap();
    assert_eq!(out["items"], 1, "{out}");
    let inside = dir.join("test-boq.xlsx");
    run(&mut session, "import_boq", json!({ "path": inside })).unwrap();

    // Anything outside the folder is refused, before it is read.
    let outside = outer.path().join("x.xlsx");
    write_boq(&outside);
    let mut refused = vec![
        json!("../x.xlsx"),
        json!("boq/../../x.xlsx"),
        json!(outside),
        json!("../missing.xlsx"),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, dir.join("link.xlsx")).unwrap();
        refused.push(json!("link.xlsx"));
    }
    for path in refused {
        let err = run(&mut session, "import_boq", json!({ "path": path })).unwrap_err();
        match err {
            ToolError::BadInput(why) => assert_eq!(why, OUTSIDE, "{path}"),
            other => panic!("{path}: {other:?}"),
        }
    }
}

#[test]
fn json_floats_are_rounded_to_6_decimals() {
    let (_dir, mut session) = sample();
    // 0.1 x 0.2 is 0.020000000000000004 in floating point.
    let out = run(
        &mut session,
        "add_measurement",
        json!({
            "kind": "area",
            "points": [[0, 0], [0.1, 0], [0.1, 0.2], [0, 0.2]],
            "description": "Pad"
        }),
    )
    .unwrap();
    assert_eq!(out["measurement"]["value"].to_string(), "0.02", "{out}");
    assert_eq!(out["measurement"]["shown"].to_string(), "0.02", "{out}");
    // Handles stay integers.
    let doors = handles_of(&session, "Insert");
    let out = run(&mut session, "get_entity", json!({ "handle": doors[0] })).unwrap();
    assert_eq!(out["entity"]["handle"], json!(doors[0]));
    assert!(out["entity"]["handle"].is_u64());
}

#[test]
fn view_image_is_800_wide_by_default() {
    let (_dir, mut session) = sample();
    let ToolOutput::Image { png, .. } =
        execute(&mut session, "view_image", &json!({}), Source::Ai).unwrap()
    else {
        panic!("expected an image");
    };
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 800);
}

#[test]
fn entity_tools_reach_inside_blocks_with_a_chain() {
    let (_dir, mut session) = sample();
    let scaled = handles_of(&session, "Insert")[2];
    let leaf = session
        .drawing
        .entities_in("DOOR")
        .next()
        .unwrap()
        .common()
        .handle
        .value();
    let out = run(
        &mut session,
        "get_entity",
        json!({ "handle": leaf, "chain": [scaled] }),
    )
    .unwrap();
    assert_eq!(out["entity"]["inside_block"], "DOOR");
    assert!((out["entity"]["length"].as_f64().unwrap() - 1.8).abs() < 1e-9);

    let out = run(
        &mut session,
        "measure_length",
        json!({ "handle": leaf, "chain": [scaled] }),
    )
    .unwrap();
    assert!((out["measurement"]["value"].as_f64().unwrap() - 1.8).abs() < 1e-9);
    assert_eq!(out["measurement"]["chain"], json!([scaled]));
    assert!(matches!(
        run(
            &mut session,
            "measure_length",
            json!({ "chain": [scaled], "points": [[0, 0], [1, 0]] })
        ),
        Err(ToolError::BadInput(_))
    ));
}

#[test]
fn tools_see_xref_layers_blocks_and_entities() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_xref_pair(dir.path());
    let mut session = Session::open(&path).unwrap();

    let layers = run(&mut session, "list_layers", json!({})).unwrap();
    assert!(layers.to_string().contains("plan-arch|A-WALL"), "{layers}");

    // DOOR is only in the xref: still a block to count, on the xref's layer.
    let out = run(
        &mut session,
        "count_blocks",
        json!({ "block_name": "DOOR", "layer": "plan-arch|A-DOOR" }),
    )
    .unwrap();
    assert_eq!(out["measurement"]["value"], 3.0);

    let found = run(
        &mut session,
        "find_entities",
        json!({ "layer": "plan-arch|A-WALL", "kind": "Line" }),
    )
    .unwrap();
    let row = &found["rows"][0];
    assert_eq!(row["xref"], "plan-arch");
    let out = run(
        &mut session,
        "measure_length",
        json!({ "handle": row["handle"], "chain": row["chain"] }),
    )
    .unwrap();
    assert!((out["measurement"]["value"].as_f64().unwrap() - 10.0).abs() < 1e-9);
    assert_eq!(out["measurement"]["xref"], "plan-arch");

    let out = run(
        &mut session,
        "show_layers",
        json!({ "names": ["plan-arch|A-WALL"], "visible": false }),
    )
    .unwrap();
    assert_eq!(out["hidden"], json!(["plan-arch|A-WALL"]));
}

#[test]
fn add_dimension_draws_real_dimensions_of_each_kind() {
    let (_dir, mut session) = sample();
    let linear = run(
        &mut session,
        "add_dimension",
        json!({ "kind": "linear", "p1": [0, 0], "p2": [4, 0], "line_point": [2, -1] }),
    )
    .unwrap();
    let dims = linear["dimensions"].as_array().unwrap();
    assert_eq!(dims.len(), 1);
    let h = dims[0].as_u64().unwrap();
    assert!(matches!(
        session.drawing.doc.get_entity(opencadcodec::Handle::new(h)),
        Some(EntityType::Dimension(_))
    ));
    let read = run(&mut session, "read_dimensions", json!({})).unwrap();
    assert!(read.to_string().contains("4"), "{read}");
    assert!(
        session.is_dirty(),
        "a drawing change, saved with the drawing"
    );

    run(
        &mut session,
        "add_dimension",
        json!({ "kind": "aligned", "p1": [0, 0], "p2": [3, 4], "offset": 0.5 }),
    )
    .unwrap();
    run(
        &mut session,
        "add_dimension",
        json!({ "kind": "angular", "vertex": [0, 0], "p1": [4, 0], "p2": [0, 4], "arc_point": [2, 2] }),
    )
    .unwrap();
    run(
        &mut session,
        "add_dimension",
        json!({ "kind": "ordinate", "point": [4, 0], "leader_end": [4, -2] }),
    )
    .unwrap();
    let arc = handles_of(&session, "Arc")[0];
    for kind in ["radius", "diameter", "arc_length"] {
        run(
            &mut session,
            "add_dimension",
            json!({ "kind": kind, "handle": arc, "point": [3.5, 12] }),
        )
        .unwrap_or_else(|e| panic!("{kind}: {e}"));
    }
    let base = h;
    let out = run(
        &mut session,
        "add_dimension",
        json!({ "kind": "continue", "handle": base, "points": [[6, 0], [9, 0]] }),
    )
    .unwrap();
    assert_eq!(out["dimensions"].as_array().unwrap().len(), 2);
    let out = run(
        &mut session,
        "add_dimension",
        json!({ "kind": "baseline", "handle": base, "points": [[6, 0]] }),
    )
    .unwrap();
    assert_eq!(out["dimensions"].as_array().unwrap().len(), 1);
}

#[test]
fn add_dimension_names_what_is_missing() {
    let (_dir, mut session) = sample();
    let err = run(
        &mut session,
        "add_dimension",
        json!({ "kind": "linear", "p1": [0, 0], "p2": [4, 0] }),
    )
    .unwrap_err();
    assert!(err.to_string().contains("line_point"), "{err}");
    assert!(!session.can_undo());
}

#[test]
fn add_comment_draws_a_cloud_and_list_comments_reads_it() {
    let (_dir, mut session) = sample();
    let out = run(
        &mut session,
        "add_comment",
        json!({ "points": [[0, 0], [4, 0], [4, 3], [0, 3]], "text": "Check the door width" }),
    )
    .unwrap();
    assert_eq!(out["layer"], "COMMENTS");
    let cloud = out["cloud"].as_u64().unwrap();
    let list = run(&mut session, "list_comments", json!({})).unwrap();
    let comments = list["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0]["handle"], cloud);
    assert_eq!(comments[0]["text"], "Check the door width");
    assert!(session.undo_if(Source::Ai), "the AI's own step");
    let list = run(&mut session, "list_comments", json!({})).unwrap();
    assert!(list["comments"].as_array().unwrap().is_empty());
}

#[test]
fn zero_count_and_empty_summary_keep_scope_and_limits() {
    let (_dir, mut s) = sample();
    s.set_layer_hidden("A-DOOR", true);
    let summary = run(&mut s, "block_summary", json!({})).unwrap();
    assert_eq!(summary["count_scope"], qiyas_core::INSERT_COUNT_SCOPE);
    assert_eq!(summary["visibility_policy"], "as_shown");
    assert!(summary["limitations"]
        .as_str()
        .unwrap()
        .contains("Not a complete visible count"));
    let count = run(&mut s, "count_blocks", json!({"block_name":"DOOR"})).unwrap();
    assert!(count.to_string().contains(qiyas_core::INSERT_COUNT_SCOPE));
    assert!(count.to_string().contains("Not a complete visible count"));
}
