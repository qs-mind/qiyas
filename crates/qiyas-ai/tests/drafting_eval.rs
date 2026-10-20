//! Offline drafting through Mizan's real agent loop. The script supplies decisions, never a
//! model; the reference quantities below are independent arithmetic, not engine output.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use opencadcodec::{EntityType, Handle};
use qiyas_ai::surface::{CAPTURE, EXECUTE, READ};
use qiyas_ai::{Agent, AgentEvent, AgentSettings, ContentBlock, Message, ResultPart, TurnRequest};
use qiyas_core::Session;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[path = "drafting_eval/replay.rs"]
mod replay;
#[path = "support/scripted.rs"]
pub mod scripted;
use scripted::{Script, ScriptCall};

const MODEL: &str = "*Model_Space";
const ROOMS: [&str; 4] = ["Living", "Kitchen", "Bedroom", "Bath"];
const LAYERS: [&str; 6] = [
    "A-WALL",
    "A-DOOR",
    "A-GLAZ",
    "A-ANNO-TEXT",
    "A-ANNO-DIMS",
    "A-ROOM-NAME",
];

fn input(op: &str, args: Value) -> Value {
    // Exercise the provider-facing, strict string form, including command payloads.
    json!({"op": op, "args": args.to_string()})
}

fn call(tool: &'static str, op: &str, args: Value) -> ScriptCall {
    let body = if tool == CAPTURE {
        json!({"source": op, "args": args.to_string()})
    } else {
        input(op, args)
    };
    ScriptCall::Fixed(tool, body)
}

fn run(command: Value) -> ScriptCall {
    call(EXECUTE, "run", json!({"command": command.to_string()}))
}

fn batch(layer: &str, commands: Vec<Value>) -> ScriptCall {
    call(
        EXECUTE,
        "batch",
        json!({"layer": layer, "commands": commands}),
    )
}

fn json_results(messages: &[Message]) -> Vec<Value> {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(!is_error, "{content:?}");
                content.iter().find_map(|part| match part {
                    ResultPart::Text(text) => serde_json::from_str(text).ok(),
                    ResultPart::Png(_) => None,
                })
            }
            _ => None,
        })
        .collect()
}

fn command_result(results: &[Value], command: &str) -> Value {
    results
        .iter()
        .find(|v| v["command"] == command)
        .unwrap_or_else(|| panic!("missing {command}: {results:#?}"))
        .clone()
}

fn targets(handles: &Value) -> Vec<Value> {
    handles
        .as_array()
        .unwrap()
        .iter()
        .map(|handle| json!({"handle": handle}))
        .collect()
}

fn villa_verify(request: &TurnRequest) -> (&'static str, Value) {
    let walls = command_result(&json_results(&request.messages), "walls")["outcome"].clone();
    let outer = walls["centrelines"][0].clone();
    let pair = json!([{"handle": outer}, {"handle": outer}]);
    let mut expectations = vec![
        json!({"kind": "distance", "targets": pair, "points": [[0,0],[12,0]],
               "value": 12, "in_metres": true}),
        json!({"kind": "distance", "targets": pair, "points": [[0,0],[0,10]],
               "value": 10, "in_metres": true}),
        json!({"kind": "parallel", "targets": pair, "points": [[3,0],[3,10]]}),
        json!({"kind": "parallel", "targets": pair, "points": [[0,3],[12,3]]}),
        json!({"kind": "closed", "targets": [{"handle": outer}]}),
        json!({"kind": "area", "targets": targets(&walls["hatches"]), "value": 11.46,
               "tolerance": 1e-6, "in_metres": true}),
        json!({"kind": "inside_bounds", "targets": targets(&walls["outlines"]),
               "bounds": [[-0.1,-0.1],[12.1,10.1]]}),
        json!({"kind": "count", "layer": "A-ROOM-NAME", "entity_type": "Text", "value": 4}),
    ];
    // Verify checks the wall contours; rooms separately proves each named space is enclosed.
    for target in targets(&walls["outlines"]) {
        expectations.push(json!({"kind": "closed", "targets": [target]}));
    }
    (
        READ,
        input(
            "run",
            json!({"command": json!({
            "type": "verify", "expectations": expectations
        }).to_string()}),
        ),
    )
}

struct Evaluation {
    session: Session,
    results: Vec<Value>,
    pictures: usize,
}

async fn evaluate(prompt: &str, steps: Vec<ScriptCall>) -> Evaluation {
    let dir = tempfile::tempdir().unwrap();
    let session = Session::new_blank(&dir.path().join("drafting-eval.dwg"), 6);
    let shared = Arc::new(Mutex::new(Some(session)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let count = steps.len();
    let provider = Script::steps(steps, requests.clone());
    let mut agent = Agent::new(Box::new(provider), shared.clone(), AgentSettings::default());
    let (tx, mut rx) = mpsc::channel(1_000);
    agent
        .run_turn(prompt.into(), tx, CancellationToken::new())
        .await
        .unwrap();
    let mut completed = false;
    let mut calls = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::ToolCall { name, .. } => {
                assert!([READ, EXECUTE, CAPTURE].contains(&name.as_str()));
                calls += 1;
            }
            AgentEvent::ToolResult {
                is_error, summary, ..
            } => assert!(!is_error, "{summary}"),
            AgentEvent::Done { .. } => completed = true,
            AgentEvent::Stopped { reason } => panic!("{reason}"),
            _ => {}
        }
    }
    assert!(completed);
    assert_eq!(calls, count, "the entire script must execute");
    assert_eq!(requests.lock().unwrap().len(), count + 1);
    let results = json_results(agent.history());
    let pictures = agent
        .history()
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::ToolResult { content, .. } => Some(content),
            _ => None,
        })
        .flatten()
        .filter(|part| {
            if let ResultPart::Png(data) = part {
                let png = qiyas_ai::from_base64(data).unwrap();
                assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
                assert!(png.len() > 1_000, "a real drawing picture");
                true
            } else {
                false
            }
        })
        .count();
    drop(agent);
    let session = shared.lock().unwrap().take().unwrap();
    Evaluation {
        session,
        results,
        pictures,
    }
}

fn near(value: &Value, expected: f64) {
    let actual = value.as_f64().unwrap();
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

fn checks_and_verification(results: &[Value], checks: usize) {
    let reports: Vec<_> = results
        .iter()
        .filter(|v| v.get("findings").is_some())
        .collect();
    assert_eq!(reports.len(), checks);
    for report in reports {
        assert_eq!(report["errors"], 0, "{report:#}");
        println!("{}: {}", report["sheet"], report["line"]);
    }
    let verified: Vec<_> = results
        .iter()
        .filter(|v| v["outcome"]["type"] == "verified")
        .collect();
    assert_eq!(verified.len(), 1);
    let expectations = verified[0]["outcome"]["results"].as_array().unwrap();
    assert!(expectations.len() >= 6);
    for expectation in expectations {
        assert_eq!(expectation["passed"], true, "{expectation:#}");
    }
}

fn on_layer(session: &Session, handles: &Value, layer: &str) {
    let handles = handles.as_array().unwrap();
    assert!(!handles.is_empty(), "expected entities on {layer}");
    for handle in handles {
        let entity = session
            .drawing
            .doc
            .get_entity(Handle::new(handle.as_u64().unwrap()))
            .unwrap();
        assert_eq!(entity.common().layer, layer);
    }
}

#[tokio::test]
async fn villa_ground_floor_through_mizans_three_tools() {
    let mut steps = vec![
        call(READ, "state", json!({})),
        call(READ, "commands", json!({"name": "walls"})),
        call(
            EXECUTE,
            "batch",
            json!({"commands": LAYERS.map(|name| json!({"type": "new_layer", "name": name}))}),
        ),
        run(json!({"type": "annotation_scale", "name": "1:100"})),
        run(json!({
            "type": "walls", "thickness": 0.2, "height": 3,
            "runs": [
                {"points": [[0,0],[12,0],[12,10],[0,10]], "closed": true},
                {"points": [[6,0],[6,10]]},
                {"points": [[0,5],[12,5]]}
            ],
            "openings": [
                {"run": 0, "at": 3, "width": 0.9, "kind": "door"},
                {"run": 0, "at": 9, "width": 1.2, "kind": "window", "sill": 0.9, "head": 2.1},
                {"run": 0, "at": 25, "width": 1.2, "kind": "window", "sill": 0.9, "head": 2.1},
                {"run": 0, "at": 31, "width": 1.2, "kind": "window", "sill": 0.9, "head": 2.1},
                {"run": 1, "at": 2.5, "width": 0.9, "kind": "door"},
                {"run": 1, "at": 7.5, "width": 0.9, "kind": "door"},
                {"run": 2, "at": 3, "width": 0.9, "kind": "door"},
                {"run": 2, "at": 9, "width": 0.9, "kind": "door"}
            ]
        })),
        run(
            json!({"type": "set_layer", "name": "A-WALL-CNTR", "prop": "linetype", "value": "CENTER"}),
        ),
        batch(
            "A-ROOM-NAME",
            ROOMS
                .into_iter()
                .zip([[0.6, 2.5], [6.6, 2.5], [0.6, 7.5], [6.6, 7.5]])
                .map(|(value, at)| json!({"type": "text", "value": value, "at": at, "rotation": 0}))
                .collect(),
        ),
        call(READ, "rooms", json!({"text_layer": "A-ROOM-NAME"})),
        run(
            json!({"type": "label_spaces", "query": {"text_layer": "A-ROOM-NAME"},
                   "layer": "A-ANNO-TEXT", "language": "en", "decimals": 2}),
        ),
        batch(
            "A-ANNO-DIMS",
            vec![
                json!({"type": "dimension", "kind": "linear", "p1": [0,0], "p2": [12,0], "line_point": [6,-2]}),
                json!({"type": "dimension", "kind": "linear", "p1": [0,0], "p2": [0,10], "line_point": [-2,5]}),
            ],
        ),
        run(json!({"type": "grid",
            "x": [{"label":"A","at":0},{"label":"B","at":6},{"label":"C","at":12}],
            "y": [{"label":"1","at":0},{"label":"2","at":5},{"label":"3","at":10}],
            "extent": {"min": [-1,-1], "max": [13,11]}
        })),
        call(READ, "rooms", json!({"text_layer": "A-ROOM-NAME"})),
        ScriptCall::FromHistory(villa_verify),
        call(READ, "check", json!({"packs": ["general","construction"]})),
        call(
            READ,
            "records",
            json!({"collection": "entities", "limit": 200}),
        ),
        call(CAPTURE, "picture", json!({"sheet": MODEL, "width": 900})),
        run(
            json!({"type": "new_layout", "name": "A1", "paper": "ISO A1", "orientation": "landscape"}),
        ),
        call(EXECUTE, "view", json!({"sheet": "A1"})),
        batch(
            "A-ANNO-TEXT",
            vec![
                json!({"type": "mview", "corner1": [40,100], "corner2": [790,560],
                   "scale": "1:100", "centre": [6,5]}),
                json!({"type": "title_block", "fields": {
                    "title": "Villa ground floor", "drawing_no": "A-001", "scale": "1:100"
                }}),
            ],
        ),
        call(READ, "check", json!({"packs": ["general","construction"]})),
        call(CAPTURE, "picture", json!({"sheet": "A1", "width": 900})),
    ];
    steps.push(call(READ, "state", json!({})));
    let eval = evaluate(
        "Draft the specified 12 x 10 m villa ground floor, four named rooms, 200 mm walls, \
         five 900 mm doors and three 1200 mm windows. Add dimensions, structural axes, \
         candidate space labels and an A1 layout at 1:100. Title: Villa ground floor; \
         drawing number: A-001; scale: 1:100. Check the drawing and verify the stated intent.",
        steps,
    )
    .await;
    checks_and_verification(&eval.results, 2);
    assert_eq!(eval.pictures, 2);
    assert_eq!(eval.results[0]["entities_on_sheet"], 0);
    near(&eval.results[0]["metres_per_unit"], 1.0);
    assert_eq!(eval.results.last().unwrap()["annotation_scale"], "1:100");
    let layers_batch = command_result(&eval.results, "batch");
    assert_eq!(
        layers_batch["outcome"]["steps"].as_array().unwrap().len(),
        6
    );
    assert_eq!(layers_batch["step"]["source"], "ai");
    for layer in LAYERS {
        assert!(eval.session.drawing.doc.layers.contains(layer));
    }
    let wall = command_result(&eval.results, "walls")["outcome"].clone();
    // Gross = 12.2*10.2 - 4*(5.8*4.8); net removes .2*(5*.9 + 3*1.2).
    // Volume keeps lintels/sills: 13.08*3 - .2*(5*.9*2.1 + 3*1.2*1.2).
    near(&wall["centreline_length"], 66.0);
    near(&wall["gross_area"], 13.08);
    near(&wall["net_area"], 11.46);
    near(&wall["volume"], 36.486);
    for (field, layer) in [
        ("outlines", "A-WALL"),
        ("hatches", "A-WALL-PATT"),
        ("centrelines", "A-WALL-CNTR"),
    ] {
        on_layer(&eval.session, &wall[field], layer);
    }
    let room_reports: Vec<_> = eval
        .results
        .iter()
        .filter(|v| v.get("rooms").is_some())
        .collect();
    assert_eq!(room_reports.len(), 2);
    for report in &room_reports {
        assert_eq!(report["skipped"], json!([]), "{report:#}");
        let rooms = report["rooms"].as_array().unwrap();
        assert_eq!(rooms.len(), 4, "{report:#}");
        assert_eq!(
            rooms
                .iter()
                .map(|r| r["name"].as_str().unwrap())
                .collect::<BTreeSet<_>>(),
            ROOMS.into_iter().collect()
        );
        for room in rooms {
            assert!(
                room["candidate_area_m2"].as_f64().unwrap() > 20.0,
                "{room:#}"
            );
            assert!(room.get("also_named").is_none(), "{room:#}");
            assert_eq!(room["measurement_basis"], "unapproved_net");
        }
    }
    assert_eq!(
        room_reports[0], room_reports[1],
        "grid must not close or subdivide rooms"
    );
    let labels = command_result(&eval.results, "label_spaces");
    assert_eq!(labels["outcome"]["report"]["labels_written"], 4);
    on_layer(&eval.session, &labels["added"], "A-ANNO-TEXT");
    on_layer(
        &eval.session,
        &command_result(&eval.results, "grid")["added"],
        "GRID",
    );
    let doc = &eval.session.drawing.doc;
    let mut doors = 0;
    let mut windows = 0;
    let mut dimensions = 0;
    let mut names = 0;
    for entity in doc.entities_in_block(MODEL) {
        assert_ne!(entity.common().layer, "0");
        match entity {
            EntityType::Insert(insert) => {
                if insert.block_name.starts_with("QIYAS_DOOR_") {
                    assert_eq!(insert.common.layer, "A-DOOR");
                    doors += 1;
                } else {
                    assert!(insert.block_name.starts_with("QIYAS_WIN_"));
                    assert_eq!(insert.common.layer, "A-GLAZ");
                    windows += 1;
                }
            }
            EntityType::Dimension(_) => {
                assert_eq!(entity.common().layer, "A-ANNO-DIMS");
                dimensions += 1;
            }
            EntityType::Text(text) if ROOMS.contains(&text.value.as_str()) => {
                assert_eq!(text.common.layer, "A-ROOM-NAME");
                near(&json!(text.height), 0.25);
                names += 1;
            }
            EntityType::Text(text) if text.value.starts_with("Unapproved candidate") => {
                assert_eq!(text.common.layer, "A-ANNO-TEXT");
            }
            _ => {}
        }
    }
    assert_eq!((doors, windows, dimensions, names), (5, 3, 2, 4));
    let mut title_values = BTreeSet::new();
    let mut viewport_count = 0;
    for entity in doc.entities_in_block(&eval.session.sheet) {
        // Codec layout bookkeeping, not authored/displayed drawing geometry.
        if matches!(entity, EntityType::Viewport(v) if v.id == 1) {
            continue;
        }
        assert_eq!(entity.common().layer, "A-ANNO-TEXT");
        match entity {
            EntityType::Viewport(v) => {
                near(&json!(1000.0 / v.scale()), 100.0);
                viewport_count += 1;
            }
            EntityType::MText(text) => {
                title_values.insert(text.value.as_str());
            }
            _ => {}
        }
    }
    assert_eq!(viewport_count, 1);
    assert_eq!(
        title_values,
        BTreeSet::from(["Villa ground floor", "A-001", "1:100"])
    );
    assert_eq!(eval.session.info().paper.unwrap().name, "ISO A1");
}

fn flange_verify(request: &TurnRequest) -> (&'static str, Value) {
    let results = json_results(&request.messages);
    let batch = command_result(&results, "batch");
    let outer = batch["outcome"]["steps"][0]["added"][0].clone();
    let inner = batch["outcome"]["steps"][1]["added"][0].clone();
    (
        READ,
        input(
            "run",
            json!({"command": json!({"type": "verify", "expectations": [
            {"kind": "radius", "targets": [{"handle": outer}], "value": 2, "in_metres": true},
            {"kind": "radius", "targets": [{"handle": inner}], "value": 0.5, "in_metres": true},
            {"kind": "area", "targets": [{"handle": outer}], "value": 4.0*std::f64::consts::PI,
             "tolerance": 0.00001, "in_metres": true},
            {"kind": "concentric", "targets": [{"handle": outer},{"handle": inner}]},
            {"kind": "closed", "targets": [{"handle": outer},{"handle": inner}]},
            {"kind": "no_self_intersection", "targets": [{"handle": outer},{"handle": inner}]},
            {"kind": "count", "layer": "CENTER", "entity_type": "Line", "value": 2}
        ]}).to_string()}),
        ),
    )
}

#[tokio::test]
async fn a_flange_uses_the_same_surface_without_building_rules() {
    let eval = evaluate(
        "Draw a circular flange outline in metres: outer radius 2, concentric \
        bore radius 0.5. Add a centre mark and outer diameter. Verify sizes and concentricity.",
        vec![
            call(READ, "state", json!({})),
            call(READ, "commands", json!({"name": "center_mark"})),
            batch(
                "PART",
                vec![
                    json!({"type": "draw", "kind": "circle", "centre": [0,0], "radius": 2}),
                    json!({"type": "draw", "kind": "circle", "centre": [0,0], "radius": 0.5}),
                    json!({"type": "center_mark", "targets": [{"$step":0,"pointer":"/added/0"}]}),
                    json!({"type": "new_layer", "name": "DIMS"}),
                    json!({"type": "set_current", "current": {"layer": "DIMS"}}),
                    json!({"type": "dimension", "kind": "diameter",
                       "target": {"handle": {"$step":0,"pointer":"/added/0"}}, "point": [2,2]}),
                ],
            ),
            ScriptCall::FromHistory(flange_verify),
            call(READ, "check", json!({"packs": ["general"]})),
            call(READ, "check", json!({"packs": ["general","construction"]})),
            call(CAPTURE, "picture", json!({"width": 700})),
        ],
    )
    .await;
    checks_and_verification(&eval.results, 2);
    assert_eq!(eval.pictures, 1);
    let batch = command_result(&eval.results, "batch");
    for (index, layer) in [(0, "PART"), (1, "PART"), (2, "CENTER")] {
        on_layer(
            &eval.session,
            &batch["outcome"]["steps"][index]["added"],
            layer,
        );
    }
    on_layer(
        &eval.session,
        &batch["outcome"]["steps"][5]["outcome"]["handles"],
        "DIMS",
    );
    for entity in eval.session.drawing.doc.entities_in_block(MODEL) {
        assert_ne!(entity.common().layer, "0");
    }
    for report in eval.results.iter().filter(|v| v.get("findings").is_some()) {
        assert!(
            report["counts"].get("missing_center_mark").is_none(),
            "{report:#}"
        );
        assert!(report["counts"].get("room_open").is_none(), "{report:#}");
    }
}
