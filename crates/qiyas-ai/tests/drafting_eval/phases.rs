//! A second villa evaluation deliberately recovers from an opening refused at a junction.
//! Every decision reads the conversation, including handles and the offered fitting ranges.

use super::*;
use qiyas_ai::surface::{self, Route};
use qiyas_ai::ToolOutput;
use qiyas_core::Source;

const PHASES: [(&str, &str); 8] = [
    (
        "plan",
        "12 x 10 m; four 6 x 5 m centreline bays; 250/150 mm walls",
    ),
    ("grid", "Grid"),
    ("outer", "Outer walls"),
    ("inner", "Partitions"),
    ("openings", "Doors and windows"),
    ("rooms", "Room labels"),
    ("dimensions", "Dimensions and table"),
    ("sheet", "Sheet"),
];

fn phase(active: usize) -> ScriptCall {
    call(EXECUTE, "plan", json!({"phases": phase_values(active)}))
}

fn phase_values(active: usize) -> Vec<Value> {
    PHASES
        .iter()
        .enumerate()
        .map(|(i, (id, title))| {
            let status = if i >= 6 {
                "skipped"
            } else if i < active {
                "done"
            } else if i == active {
                "active"
            } else {
                "pending"
            };
            json!({"id": id, "title": title, "status": status})
        })
        .collect()
}

fn history(request: &TurnRequest) -> Vec<Value> {
    request
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|block| {
            if let ContentBlock::ToolResult {
                content, is_error, ..
            } = block
            {
                let result = content.iter().find_map(|part| match part {
                    ResultPart::Text(text) => serde_json::from_str::<Value>(text).ok(),
                    ResultPart::Png(_) => None,
                })?;
                if *is_error {
                    let message = result["message"].as_str().unwrap_or("");
                    assert!(
                        message.contains("junction") && message.contains("fits"),
                        "{result:#}"
                    );
                    None
                } else {
                    Some(result)
                }
            } else {
                None
            }
        })
        .collect()
}

fn latest_groups(request: &TurnRequest) -> Value {
    history(request)
        .into_iter()
        .rev()
        .find(|v| v["outcome"]["type"] == "wall_groups")
        .expect("wall read before placing an opening")["outcome"]["groups"]
        .clone()
}

fn window_at_junction(request: &TurnRequest) -> (&'static str, Value) {
    let groups = latest_groups(request);
    assert_eq!(groups.as_array().unwrap().len(), 1);
    (
        EXECUTE,
        input(
            "run",
            json!({"command": {
                "type": "add_wall_openings", "centreline": groups[0]["runs"][0]["centreline"],
                "openings": [{"at": 6, "width": 1.2, "kind": "window"}]
            }}),
        ),
    )
}

fn window_in_free_range(request: &TurnRequest) -> (&'static str, Value) {
    let groups = latest_groups(request);
    let run = &groups[0]["runs"][0];
    let ranges = run["free_ranges"].as_array().unwrap();
    assert!(!ranges.is_empty());
    let at = (ranges[0][0].as_f64().unwrap() + ranges[0][1].as_f64().unwrap()) / 2.0;
    (
        EXECUTE,
        input(
            "run",
            json!({"command": {
                "type": "add_wall_openings", "centreline": run["centreline"],
                "openings": [{"at": at, "width": 1.2, "kind": "window"}]
            }}),
        ),
    )
}

fn partition_door(request: &TurnRequest) -> (&'static str, Value) {
    let groups = latest_groups(request);
    (
        EXECUTE,
        input(
            "run",
            json!({"command": {
                "type": "add_wall_openings", "centreline": groups[0]["runs"][1]["centreline"],
                "openings": [{"at": 2.5, "width": 0.9, "kind": "door"}]
            }}),
        ),
    )
}

fn verify(request: &TurnRequest) -> (&'static str, Value) {
    let results = history(request);
    let walls = &results
        .iter()
        .rev()
        .find(|v| v["command"] == "add_wall_openings")
        .unwrap()["outcome"];
    let outer = &walls["centrelines"][0];
    let pair = json!([{"handle":outer}, {"handle":outer}]);
    (
        READ,
        input(
            "run",
            json!({"command": {
                "type":"verify", "expectations": [
                    {"kind":"distance","targets":pair,"points":[[0,0],[12,0]],"value":12,"in_metres":true},
                    {"kind":"distance","targets":pair,"points":[[0,0],[0,10]],"value":10,"in_metres":true},
                    {"kind":"closed","targets":[{"handle":outer}]},
                    {"kind":"area","targets":targets(&walls["hatches"]),"value":13.7675,"tolerance":1e-6,"in_metres":true},
                    {"kind":"count","layer":"GRID","entity_type":"Line","value":6},
                    {"kind":"count","layer":"A-ROOM-NAME","entity_type":"Text","value":4},
                    {"kind":"count","layer":"A-DOOR","entity_type":"Insert","value":1},
                    {"kind":"count","layer":"A-GLAZ","entity_type":"Insert","value":1}
                ]
            }}),
        ),
    )
}

fn read_walls(width: f64) -> ScriptCall {
    call(
        READ,
        "run",
        json!({"command":{"type":"wall_groups","width":width}}),
    )
}

fn inspect(steps: &mut Vec<ScriptCall>) {
    steps.push(call(CAPTURE, "picture", json!({"width":700})));
    steps.push(call(
        READ,
        "check",
        json!({"packs":["general","construction"]}),
    ));
}

fn surface_command(session: &mut Session, command: Value) -> Value {
    let Route::Session { tool, op, args } =
        surface::route(EXECUTE, &input("run", json!({"command":command}))).unwrap()
    else {
        panic!("drawing command")
    };
    let ToolOutput::Json(result) =
        surface::run_session(session, &tool, &op, &args, Source::Ai).unwrap()
    else {
        panic!("command report")
    };
    result
}

#[tokio::test]
async fn villa_in_phases_joins_walls_and_recovers_using_the_wall_read() {
    let outer = json!({"points":[[0,0],[12,0],[12,10],[0,10]],"closed":true});
    let vertical = json!({"points":[[6,0],[6,10]],"thickness":0.15});
    let horizontal = json!({"points":[[0,5],[12,5]],"thickness":0.15});
    let mut steps = vec![
        call(READ, "state", json!({})),
        phase(0),
        call(
            READ,
            "validate",
            json!({"command":{"type":"walls","thickness":0.25,"runs":[outer.clone()]}}),
        ),
        phase(1),
        run(json!({"type":"grid",
            "x":[{"label":"A","at":0},{"label":"B","at":6},{"label":"C","at":12}],
            "y":[{"label":"1","at":0},{"label":"2","at":5},{"label":"3","at":10}],
            "extent":{"min":[-1,-1],"max":[13,11]}
        })),
    ];
    inspect(&mut steps);
    steps.extend([
        phase(2),
        run(json!({"type":"walls","thickness":0.25,"runs":[outer.clone()]})),
    ]);
    inspect(&mut steps);
    steps.extend([
        phase(3),
        run(json!({"type":"walls","thickness":0.15,"runs":[vertical.clone(),horizontal.clone()]})),
    ]);
    inspect(&mut steps);
    steps.extend([
        phase(4),
        read_walls(1.2),
        ScriptCall::FromHistory(window_at_junction),
        read_walls(1.2),
        ScriptCall::FromHistory(window_in_free_range),
        read_walls(0.9),
        ScriptCall::FromHistory(partition_door),
    ]);
    inspect(&mut steps);
    steps.extend([
        phase(5),
        batch(
            "A-ROOM-NAME",
            ROOMS
                .iter()
                .zip([[1.0, 2.5], [7.0, 2.5], [1.0, 7.5], [7.0, 7.5]])
                .map(|(value, at)| json!({"type":"text","value":value,"at":at}))
                .collect(),
        ),
        call(READ, "rooms", json!({"text_layer":"A-ROOM-NAME"})),
        ScriptCall::FromHistory(verify),
    ]);
    inspect(&mut steps);
    steps.extend([read_walls(1.2), phase(6)]);

    let dir = tempfile::tempdir().unwrap();
    let session = Session::new_blank(&dir.path().join("phased.dwg"), 6);
    let shared = Arc::new(Mutex::new(Some(session)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let call_count = steps.len();
    let mut agent = Agent::new(
        Box::new(Script::steps(steps, requests.clone())),
        shared.clone(),
        AgentSettings::default(),
    );
    let (tx, mut rx) = mpsc::channel(1_000);
    agent
        .run_turn(
            "Draft a 12 x 10 m villa with four 6 x 5 m centreline bays. Outer walls 250 mm, \
        partitions 150 mm, all 3 m high. Work in phases and check after each. Draw grid axes on \
        the wall centrelines at x=0,6,12 and y=0,5,10, extending around the walls. Add one 1200 mm \
        window and one 900 mm partition door, then name the rooms. Dimensions, table and sheet \
        are covered by the original villa evaluation; mark those phases skipped here."
                .into(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut plans = Vec::new();
    let mut errors = Vec::new();
    let mut calls = 0;
    let mut completed = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::Plan { phases } => plans.push(serde_json::to_value(phases).unwrap()),
            AgentEvent::ToolCall { name, .. } => {
                assert!([READ, EXECUTE, CAPTURE].contains(&name.as_str()));
                calls += 1;
            }
            AgentEvent::ToolResult {
                is_error: true,
                summary,
                ..
            } => errors.push(summary),
            AgentEvent::Done { .. } => completed = true,
            AgentEvent::Stopped { reason } => panic!("{reason}"),
            _ => {}
        }
    }
    assert!(completed);
    assert_eq!(calls, call_count);
    assert_eq!(requests.lock().unwrap().len(), call_count + 1);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("junction") && errors[0].contains("fits"),
        "{}",
        errors[0]
    );
    assert_eq!(
        plans,
        [0, 1, 2, 3, 4, 5, 6]
            .into_iter()
            .map(|i| json!(phase_values(i)))
            .collect::<Vec<_>>()
    );
    let pictures = agent
        .history()
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|block| {
            if let ContentBlock::ToolResult { content, .. } = block {
                Some(content)
            } else {
                None
            }
        })
        .flatten()
        .filter(|part| matches!(part, ResultPart::Png(_)))
        .count();
    assert_eq!(pictures, 5, "look after every drawing phase");
    let requests = requests.lock().unwrap();
    let results = history(requests.last().unwrap());
    checks_and_verification(&results, 5);
    for report in results.iter().filter(|v| v.get("findings").is_some()) {
        assert_eq!(report["warnings"], 0, "{report:#}");
    }
    let snapshots: Vec<_> = results.iter().filter_map(|v| v.get("plan")).collect();
    assert_eq!(
        snapshots.len(),
        plans.len(),
        "plan snapshots stay in the conversation"
    );
    let rooms = results.iter().find(|v| v.get("rooms").is_some()).unwrap();
    assert_eq!(rooms["rooms"].as_array().unwrap().len(), 4);
    assert_eq!(rooms["skipped"], json!([]));
    let reads: Vec<_> = results
        .iter()
        .filter(|v| v["outcome"]["type"] == "wall_groups")
        .collect();
    assert_eq!(
        reads[0], reads[1],
        "a refused opening changes no wall handle or definition"
    );
    let final_runs = &reads.last().unwrap()["outcome"]["groups"][0]["runs"];
    let window_at = final_runs[0]["openings"][0]["at"].as_f64().unwrap();
    let mut reference = Session::new_blank(&dir.path().join("one-command.dwg"), 6);
    let expected = surface_command(
        &mut reference,
        json!({"type":"walls","thickness":0.25,
        "runs":[outer,vertical,horizontal],"openings":[
            {"run":0,"at":window_at,"width":1.2,"kind":"window"},
            {"run":1,"at":2.5,"width":0.9,"kind":"door"}
        ]}),
    );
    let actual = &results
        .iter()
        .rev()
        .find(|v| v["command"] == "add_wall_openings")
        .unwrap()["outcome"];
    // Outer ring: 12.25*10.25 - 11.75*9.75 = 11 m². Partitions: .15*(9.75+11.75)-.15².
    // Openings remove .25*1.2 + .15*.9; void heights are 1.2 m and 2.1 m respectively.
    near(&actual["gross_area"], 14.2025);
    near(&actual["net_area"], 13.7675);
    near(&actual["volume"], 41.964);
    for field in ["centreline_length", "gross_area", "net_area", "volume"] {
        near(&actual[field], expected["outcome"][field].as_f64().unwrap());
    }
    for field in ["handles", "outlines", "hatches", "centrelines", "inserts"] {
        assert_eq!(
            actual[field].as_array().unwrap().len(),
            expected["outcome"][field].as_array().unwrap().len(),
            "{field}"
        );
    }
    let session = shared.lock().unwrap();
    assert_eq!(
        session.as_ref().unwrap().history_steps().0.len(),
        6,
        "plan, read and captures add no undo steps"
    );
}
