//! The tools that write into the drawing (text, room areas and labels, tables, shapes, blocks),
//! the measure-by-handle rule and the refusal of take-off rows that are notes. Run through the
//! tool executor, as the model calls them.

use std::f64::consts::PI;

use opencadcodec::entities::{Polyline2D, Vertex2D};
use opencadcodec::{Arc, CadDocument, EntityType, LwPolyline, Vector2, Vector3};
use qiyas_ai::{execute, ToolError, ToolOutput};
use qiyas_core::fixtures::{sample_doc, write_drawing};
use qiyas_core::{Command, EntityFilter, Session, Source};
use serde_json::{json, Value};

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    let session = Session::open(&path).unwrap();
    (dir, session)
}

fn run(session: &mut Session, name: &str, input: Value) -> Result<Value, ToolError> {
    match execute(session, name, &input, Source::Ai)? {
        ToolOutput::Json(v) => Ok(v),
        ToolOutput::Image { .. } => panic!("expected JSON"),
    }
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
            run(s, "measure_length", json!({ "handle": handle })).unwrap()["measurement"]["shown"]
                .as_f64()
                .unwrap()
        };
        let area = |s: &mut Session, handle: u64| -> f64 {
            run(s, "measure_area", json!({ "handle": handle })).unwrap()["measurement"]["shown"]
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
    let out = run(&mut s, "measure_length", json!({ "points": clicks })).unwrap();
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
    let exact = run(&mut s, "measure_length", json!({ "handle": arc })).unwrap();
    assert!(exact.get("hint").is_none());
    let free = run(
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

// ---- The take-off takes quantities, not notes ----

#[test]
fn a_take_off_row_with_no_geometry_and_no_quantity_is_refused() {
    let (_dir, mut s) = open(&sample_doc(), "rows.dxf");
    for note in [
        json!({ "kind": "item", "description": "Overall Width: 12.00 m" }),
        json!({ "kind": "item", "description": "Living Room: 6.70 m x 8.70 m", "value": 0, "unit": "m" }),
        json!({ "kind": "item", "description": "Analysis", "unit": "note", "note": "text" }),
    ] {
        let err = run(&mut s, "add_measurement", note).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{err}");
        let why = err.to_string();
        assert!(why.contains("real measured quantity"), "{why}");
        assert!(why.contains("add_text"), "{why}");
        assert!(why.contains("empty_item"), "{why}");
    }
    assert!(s.takeoff.items.is_empty());
    assert!(!s.can_undo(), "nothing was recorded");

    // A real quantity, geometry, or an explicit empty item goes in.
    run(
        &mut s,
        "add_measurement",
        json!({ "kind": "item", "description": "Cement", "value": 5, "unit": "bags" }),
    )
    .unwrap();
    let wall = handles_of(&s, "Line")[0];
    run(
        &mut s,
        "add_measurement",
        json!({ "kind": "length", "description": "Wall", "handles": [wall] }),
    )
    .unwrap();
    run(&mut s, "add_measurement", json!({ "kind": "item", "description": "Provisional sum", "unit": "ls", "empty_item": true })).unwrap();
    assert_eq!(s.takeoff.items.len(), 3);
    // add_markup still makes its own quantity-less row: it is not a take-off quantity.
    run(
        &mut s,
        "add_markup",
        json!({ "points": [[0, 0]], "text": "look here", "kind": "note" }),
    )
    .unwrap();
    assert_eq!(s.takeoff.items.len(), 4);
}

// ---- add_text ----

#[test]
fn add_text_writes_single_and_multi_line_text_in_one_undo_step() {
    let (_dir, mut s) = open(&sample_doc(), "text.dxf");
    let before = count_entities(&s);
    let before_steps = steps(&mut s);
    let out = run(
        &mut s,
        "add_text",
        json!({
            "texts": [
                { "text": "Net area 51.22 m²", "point": [1.0, 1.0] },
                { "text": "الصالة", "point": [2.0, 2.0], "height": 0.4, "rotation": 90 },
                { "text": "Line one\nLine two", "point": [3.0, 3.0], "height": 0.3 },
                { "text": "Wrapped", "point": [4.0, 4.0], "width": 2.0 }
            ],
            "layer": "NOTES"
        }),
    )
    .unwrap();
    assert_eq!(out["layer"], "NOTES");
    assert_eq!(out["layer_created"], true);
    let texts = out["texts"].as_array().unwrap();
    assert_eq!(texts.len(), 4);
    assert_eq!(texts[0]["kind"], "Text");
    assert_eq!(texts[2]["kind"], "MText");
    assert_eq!(texts[3]["kind"], "MText");
    // 2.5 mm at 1:1 in a drawing in metres.
    assert!(
        near(texts[0]["height_m"].as_f64().unwrap(), 0.0025, 1e-9),
        "{out}"
    );
    assert!(near(texts[1]["height_m"].as_f64().unwrap(), 0.4, 1e-9));
    assert_eq!(count_entities(&s), before + 4);
    assert_eq!(
        steps(&mut s),
        before_steps + 1,
        "one undo step for the call"
    );
    assert_eq!(s.current.layer, "0", "the current layer is put back");
    for t in texts {
        let h = t["handle"].as_u64().unwrap();
        assert_eq!(s.entity_info(h, &[]).unwrap().layer_short, "NOTES");
    }
    let read = run(&mut s, "read_text", json!({})).unwrap();
    assert!(read.to_string().contains("الصالة"), "{read}");
    assert!(read.to_string().contains("51.22 m²"), "{read}");

    // The annotation scale sizes the default.
    run(&mut s, "set_annotation_scale", json!({ "scale": "1:100" })).unwrap();
    let out = run(
        &mut s,
        "add_text",
        json!({ "texts": [{ "text": "Scaled", "point": [0, 0] }] }),
    )
    .unwrap();
    assert!(
        near(out["texts"][0]["height_m"].as_f64().unwrap(), 0.25, 1e-9),
        "{out}"
    );

    // Undo takes back the scaled text, the annotation scale, then the whole first call, its
    // layer included.
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
        json!({ "texts": [] }),
        json!({ "texts": [{ "text": "  ", "point": [0, 0] }] }),
        json!({ "texts": [{ "text": "x", "point": [0, 0], "height": 0 }] }),
        json!({ "texts": [{ "text": "a\nb", "point": [0, 0], "rotation": 30 }] }),
        json!({ "texts": [{ "text": "x", "point": [0, 0] }], "layer": "XREF|WALL" }),
    ] {
        let err = run(&mut s, "add_text", input.clone()).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{input}: {err}");
    }
    assert!(!s.can_undo());
    // The second text is bad: the first is not left behind.
    let err = run(
        &mut s,
        "add_text",
        json!({ "texts": [{ "text": "ok", "point": [0, 0] }, { "text": "bad", "point": [0, 0], "height": -1 }] }),
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
    let out = run(
        &mut s,
        "draw",
        json!({
            "shapes": [
                { "kind": "rectangle", "a": [30, 0], "b": [36, 6] },
                { "kind": "line", "a": [31, 5], "b": [33, 5] },
                { "kind": "polyline", "points": [[31, 4], [33, 4], [33, 3]], "bulges": [0.5, 0, 0], "closed": false },
                { "kind": "hatch", "points": [[31, 1], [33, 1], [33, 2], [31, 2]], "pattern": "ansi31" },
                { "kind": "circle", "centre": [32, 2.5], "radius": 0.2 },
                { "kind": "arc", "centre": [34, 2.5], "radius": 0.5, "start": 0, "end": 180 },
                { "kind": "revcloud", "points": [[29, -1], [37, -1], [37, 7], [29, 7]], "arc_length": 0.5 }
            ],
            "layer": "LEGEND",
            "color": "#ff0000"
        }),
    )
    .unwrap();
    assert_eq!(out["layer_created"], true);
    let handles = out["shapes"].as_array().unwrap();
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
            "at least 2",
        ),
        (
            json!({ "kind": "polyline", "points": [[0, 0], [1, 1]], "bulges": [1] }),
            "bulges",
        ),
        (
            json!({ "kind": "hatch", "points": [[0, 0], [1, 1]] }),
            "at least 3",
        ),
        (
            json!({ "kind": "revcloud", "points": [[0, 0], [1, 0], [1, 1]] }),
            "arc_length",
        ),
    ] {
        let err = run(&mut s, "draw", json!({ "shapes": [shape] })).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{shape}: {err}");
        assert!(err.to_string().contains(word), "{shape}: {err}");
    }
    assert!(!s.can_undo());
}

#[test]
fn insert_block_places_references_with_attributes_in_one_step() {
    let (_dir, mut s) = open(&sample_doc(), "insert.dxf");
    let doors = handles_of(&s, "Insert").len();
    let out = run(
        &mut s,
        "insert_block",
        json!({
            "blocks": [
                { "name": "DOOR", "point": [30, 0] },
                { "name": "door", "point": [32, 0], "scale": 2, "rotation": 90 }
            ],
            "layer": "LEGEND"
        }),
    )
    .unwrap();
    assert_eq!(out["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(handles_of(&s, "Insert").len(), doors + 2);
    for h in out["blocks"].as_array().unwrap() {
        assert_eq!(
            s.entity_info(h.as_u64().unwrap(), &[]).unwrap().layer_short,
            "LEGEND"
        );
    }
    let err = run(
        &mut s,
        "insert_block",
        json!({ "blocks": [{ "name": "NOPE", "point": [0, 0] }] }),
    )
    .unwrap_err();
    assert!(err.to_string().contains("block_summary"), "{err}");
    assert!(s.undo_if(Source::Ai));
    assert_eq!(handles_of(&s, "Insert").len(), doors);
}

#[test]
fn add_table_draws_a_schedule_and_edit_table_cell_changes_it() {
    let (_dir, mut s) = open(&sample_doc(), "table.dxf");
    let out = run(
        &mut s,
        "add_table",
        json!({
            "point": [30, 20],
            "columns": ["Room", "Area (m²)"],
            "rows": [["Living", "51.22"], ["غرفة النوم", "22.09"]],
            "layer": "SCHEDULE"
        }),
    )
    .unwrap();
    assert_eq!(out["rows"], 3);
    assert_eq!(out["columns"], 2);
    assert_eq!(out["layer_created"], true);
    let table = out["table"].as_u64().unwrap();
    assert_eq!(s.entity_info(table, &[]).unwrap().layer_short, "SCHEDULE");
    let read = run(&mut s, "read_tables", json!({})).unwrap();
    let text = read.to_string();
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
    let bounds = out["bounds"].as_array().unwrap();
    assert!(near(bounds[1][1].as_f64().unwrap(), 20.0, 1e-9));
    assert!(bounds[1][0].as_f64().unwrap() > bounds[0][0].as_f64().unwrap());

    let before_steps = steps(&mut s);
    run(
        &mut s,
        "edit_table_cell",
        json!({ "handle": table, "cells": [{ "row": 1, "col": 1, "text": "50.00" }, { "row": 2, "col": 0, "text": "Bedroom" }] }),
    )
    .unwrap();
    assert_eq!(steps(&mut s), before_steps + 1);
    let text = run(&mut s, "read_tables", json!({})).unwrap().to_string();
    assert!(
        text.contains("50.00") && text.contains("Bedroom") && !text.contains("51.22"),
        "{text}"
    );

    // A row longer than the headings, or an empty table, is refused.
    for input in [
        json!({ "point": [0, 0], "columns": ["A"], "rows": [["1", "2"]] }),
        json!({ "point": [0, 0], "columns": [], "rows": [] }),
    ] {
        let err = run(&mut s, "add_table", input).unwrap_err();
        assert!(matches!(err, ToolError::BadInput(_)), "{err}");
    }
    let err = run(
        &mut s,
        "edit_table_cell",
        json!({ "handle": table, "cells": [] }),
    )
    .unwrap_err();
    assert!(matches!(err, ToolError::BadInput(_)), "{err}");
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
    let out = run(&mut s, "room_areas", json!({ "text_layer": "A-TEXT" })).unwrap();
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
    let out = run(
        &mut s,
        "room_areas",
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
    let out = run(
        &mut s,
        "room_areas",
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
fn label_spaces_writes_unapproved_candidate_areas_under_names_and_a_schedule_follows() {
    let (_dir, mut s) = test_plan();
    let before_steps = steps(&mut s);
    let names = run(&mut s, "read_text", json!({})).unwrap();
    let out = run(
        &mut s,
        "label_spaces",
        json!({ "text_layer": "A-TEXT", "layer": "A-AREA" }),
    )
    .unwrap();
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

    let living = room(&out, "LIVING");
    let label = living["label_handle"].as_u64().unwrap();
    let info = s.entity_info(label, &[]).unwrap();
    assert_eq!(info.layer_short, "A-AREA");
    assert_eq!(info.text.as_deref(), Some("Unapproved candidate 51.21 m²"));
    // Under the name: same start, lower down.
    let name_row = names["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["text"] == "LIVING")
        .unwrap();
    let at = living["label_point"].as_array().unwrap();
    assert!(near(
        at[0].as_f64().unwrap(),
        name_row["at"][0].as_f64().unwrap(),
        1e-9
    ));
    assert!(at[1].as_f64().unwrap() < name_row["at"][1].as_f64().unwrap());
    let all = run(&mut s, "read_text", json!({})).unwrap().to_string();
    for name in ["BEDROOM", "KITCHEN"] {
        let net = room(&out, name)["net_area_m2"].as_f64().unwrap();
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
    let table = run(
        &mut s,
        "add_table",
        json!({ "point": [15, 9], "columns": ["Room", "Net area m²"], "rows": rows, "layer": "A-AREA" }),
    )
    .unwrap();
    let schedule = run(&mut s, "read_tables", json!({})).unwrap().to_string();
    for cell in ["LIVING", "51.21", "BEDROOM", "KITCHEN"] {
        assert!(schedule.contains(cell), "{cell}: {schedule}");
    }
    assert!(table["table"].as_u64().is_some());

    // Both are undone one step at a time; the labels go together.
    assert!(s.undo_if(Source::Ai));
    assert!(s.undo_if(Source::Ai));
    let after = run(&mut s, "read_text", json!({})).unwrap().to_string();
    assert!(!after.contains("m²"), "{after}");
    assert_eq!(steps(&mut s), before_steps);
}

// ---- The batch command itself ----

#[test]
fn a_batch_is_one_step_takes_drawing_commands_only_and_fails_whole() {
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
    })
    .unwrap();
    assert_eq!(count_entities(&s), before + 3);
    assert!(s.undo());
    assert_eq!(count_entities(&s), before);

    // Something that is not a drawing command: refused, nothing drawn.
    let wall = handles_of(&s, "Line")[0];
    let err = s
        .run(Command::Batch {
            commands: vec![
                line(50.0),
                Command::DeleteEntities {
                    targets: vec![qiyas_core::Target {
                        handle: wall,
                        chain: vec![],
                    }],
                },
            ],
        })
        .unwrap_err();
    assert!(err.to_string().contains("drawing commands only"), "{err}");
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
        ("LIVING", Some("ar"), "مساحة مرشحة غير معتمدة 51.21 م²"),
        ("غرفة", Some("en"), "Unapproved candidate 51.21 m²"),
        ("غرفة", None, "مساحة مرشحة غير معتمدة 51.21 م²"),
        ("LIVING", None, "Unapproved candidate 51.21 m²"),
    ] {
        let (_dir, mut s) = test_plan();
        let texts = run(&mut s, "read_text", json!({})).unwrap();
        let at = texts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["text"] == "LIVING")
            .unwrap()["at"]
            .clone();
        let mut input = json!({"seeds": [{"name": name, "point": at}], "layer": "A-AREA"});
        if let Some(language) = language {
            input["language"] = json!(language);
        }
        let out = run(&mut s, "label_spaces", input).unwrap();
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
    assert!(run(&mut s, "label_spaces", json!({"language": "fr"})).is_err());
}
