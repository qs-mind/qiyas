//! Clinic intent regressions. No network or model services are used.
use qiyas_ai::{surface, ToolOutput};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};

use crate::scripted;

fn input(op: &str, args: Value) -> Value {
    json!({"op":op,"args":args.to_string()})
}

fn tool_values(request: &qiyas_ai::TurnRequest) -> Vec<Value> {
    request
        .messages
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|block| {
            if let qiyas_ai::ContentBlock::ToolResult { content, .. } = block {
                content.iter().find_map(|part| {
                    if let qiyas_ai::ResultPart::Text(text) = part {
                        serde_json::from_str(text).ok()
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .collect()
}

#[tokio::test]
async fn recorded_clinic_opening_succeeds_on_the_second_attempt() {
    use scripted::{Script, ScriptCall};
    use std::sync::{Arc, Mutex};
    fn retry(request: &qiyas_ai::TurnRequest) -> (&'static str, Value) {
        let results = tool_values(request);
        let error = results.last().unwrap();
        let at: f64 = error["message"]
            .as_str()
            .unwrap()
            .split("nearest valid `at`: ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/clinic-corner.json")).unwrap();
        let mut command = fixture["command"].clone();
        command["openings"][fixture["failed_opening"].as_u64().unwrap() as usize]["at"] = json!(at);
        (
            surface::EXECUTE,
            input(
                "run",
                json!({"command":command,
            "expected_revision":{"document_revision":error["revision"]["document_revision"]}}),
            ),
        )
    }
    let fixture: Value = serde_json::from_str(include_str!("fixtures/clinic-corner.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("recorded-clinic.dxf"),
        4,
    ))));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = Script::steps(
        vec![
            ScriptCall::Fixed(
                surface::EXECUTE,
                input("run", json!({"command":fixture["command"]})),
            ),
            ScriptCall::FromHistory(retry),
            ScriptCall::Fixed(
                surface::READ,
                input("run", json!({"command":{"type":"wall_groups"}})),
            ),
        ],
        requests.clone(),
    );
    let mut agent = qiyas_ai::Agent::new(Box::new(provider), session.clone(), Default::default());
    let (tx, mut rx) = tokio::sync::mpsc::channel(500);
    agent
        .run_turn(
            "Place the clinic's twelve windows.".into(),
            tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut errors = 0;
    while let Ok(event) = rx.try_recv() {
        if let qiyas_ai::AgentEvent::ToolResult { is_error: true, .. } = event {
            errors += 1;
        }
    }
    assert_eq!(errors, 1);
    let guard = session.lock().unwrap();
    let groups = guard.as_ref().unwrap().wall_groups(None).unwrap();
    assert_eq!(
        groups
            .iter()
            .flat_map(|g| &g.runs)
            .map(|r| r.openings.len())
            .sum::<usize>(),
        12
    );
    assert_eq!(
        scripted::observed_messages(&requests.lock().unwrap()).len(),
        3
    );
}

fn call(s: &mut Session, tool: &str, op: &str, args: Value) -> Result<Value, Value> {
    match surface::run_session(s, tool, op, &args, Source::Mcp) {
        Ok(ToolOutput::Json(v)) => Ok(v),
        Ok(_) => panic!("expected a numeric read"),
        Err(e) => Err(e.json()),
    }
}
fn run(s: &mut Session, c: Value) -> Value {
    call(s, surface::EXECUTE, "run", json!({"command":c})).unwrap()
}

#[test]
fn discovery_suggests_the_command_kind_for_a_bare_or_mistyped_kind() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("kinds.dxf"), 4);
    for name in ["set_method", "set_methd"] {
        let error = call(&mut s, surface::READ, "commands", json!({"name":name})).unwrap_err();
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("measurement.set_method"),
            "{error}"
        );
    }
}

#[test]
fn run_wraps_command_fields_and_partial_revision_checks_only_supplied_parts() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("inputs.dxf"), 4);
    let revision = s.revision().document_revision;
    let out = call(
        &mut s,
        surface::EXECUTE,
        "run",
        json!({
            "type":"text", "value":"Clinic", "at":[0,0], "height":250,
            "expected_revision":{"document_revision":revision}
        }),
    )
    .unwrap();
    assert!(out["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["code"] == "normalised_input"));
    let h = out["added"][0].as_u64().unwrap();
    let text = call(
        &mut s,
        surface::READ,
        "query",
        json!({"kind":"entity","handle":h}),
    )
    .unwrap();
    assert!(text.to_string().contains("Clinic"));
    let error = call(
        &mut s,
        surface::EXECUTE,
        "run",
        json!({
            "command":{"type":"edit_text","target":{"handle":h},"value":"Wrong"},
            "expected_revision":{"document_revision":revision}
        }),
    )
    .unwrap_err();
    assert_eq!(error["code"], "stale_read", "{error}");
    assert!(error["changed_components"]
        .as_array()
        .unwrap()
        .contains(&json!("document")));
    let current = error["revision"]["document_revision"].clone();
    let out = call(
        &mut s,
        surface::EXECUTE,
        "run",
        json!({
            "command":{"type":"edit_text","target":{"handle":h},"value":"Corrected"},
            "expected_revision":{"document_revision":current}
        }),
    )
    .unwrap();
    assert!(out["revision"]["document_revision"].as_u64().unwrap() > revision);
    let text = call(
        &mut s,
        surface::READ,
        "query",
        json!({"kind":"entity","handle":h}),
    )
    .unwrap();
    assert!(text.to_string().contains("Corrected"));
}

#[test]
fn corner_refusal_recommends_a_position_that_works_in_every_unit() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("corner.dxf"), unit);
        let k = 1.0 / s.drawing.units.to_metres();
        let wall = run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[5.0*k,0],[5.0*k,4.0*k]]}],"thickness":0.2*k}),
        );
        let host = wall["outcome"]["centrelines"][0].clone();
        let before = s.revision();
        let error = call(&mut s, surface::EXECUTE, "run", json!({"command":{
            "type":"add_wall_openings","centreline":host,"openings":[{"at":5.0*k,"width":0.9*k,"kind":"door"}]
        }})).unwrap_err();
        assert_eq!(s.revision(), before);
        let message = error["message"].as_str().unwrap();
        let at: f64 = message
            .split("nearest valid `at`: ")
            .nth(1)
            .expect(message)
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        run(
            &mut s,
            json!({"type":"add_wall_openings","centreline":host,"openings":[{"at":at,"width":0.9*k,"kind":"door"}]}),
        );
        let groups = s.wall_groups(None).unwrap();
        assert_eq!(groups[0].runs[0].openings.len(), 1);
        assert!((groups[0].runs[0].openings[0].opening.at - at).abs() < 1e-6 * k);
    }
}

#[test]
fn every_query_accepts_both_boxes_and_bounds_warns() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("queries.dxf"), 6);
    let h = run(
        &mut s,
        json!({"type":"text","at":[1,1],"height":0.1,"value":"Inside"}),
    )["added"][0]
        .clone();
    run(
        &mut s,
        json!({"type":"text","at":[20,20],"height":0.1,"value":"Outside"}),
    );
    for key in ["within", "bounds"] {
        for kind in surface::QUERY_KINDS {
            let mut args = json!({"kind":kind});
            args[key] = json!([[0, 0], [3, 3]]);
            match kind {
                "entity" => {
                    args["handle"] = h.clone();
                }
                "near" => {
                    args["point"] = json!([1, 1]);
                    args["radius"] = json!(30);
                }
                "boundary" => {
                    args["point"] = json!([20, 20]);
                }
                _ => {}
            }
            let out = call(&mut s, surface::READ, "query", args).unwrap();
            if key == "bounds" {
                assert!(
                    out["warnings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|w| w["code"] == "normalised_input"),
                    "{kind}: {out}"
                );
            }
            if kind == "texts" {
                assert!(out.to_string().contains("Inside"));
                assert!(!out.to_string().contains("Outside"));
            }
            if kind == "boundary" {
                assert_eq!(out["matched"], false);
            }
        }
    }
    assert!(call(
        &mut s,
        surface::READ,
        "query",
        json!({"kind":"texts","bounds":[[0,0],[3,3]],"within":[[5,5],[8,8]]})
    )
    .is_err());
}

#[test]
fn brief_checks_measure_sizes_site_direction_and_every_stated_deliverable() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("brief.dxf"), 6);
    let h = run(
        &mut s,
        json!({"type":"draw","kind":"line","a":[0,0],"b":[4,0]}),
    )["added"][0]
        .clone();
    run(
        &mut s,
        json!({"type":"modify","kind":"move","targets":[h],"delta":[-0.5,0]}),
    );
    let before = s.revision();
    let out=call(&mut s,surface::READ,"brief_check",json!({"expectations":[
        {"id":"size","text":"A four metre edge","verify":{"kind":"length","targets":[{"handle":h}],"value":4,"in_metres":true}},
        {"id":"east","text":"Move the west corner 1.5 m east","read":{"op":"run","args":{"command":{"type":"site_coordinates","targets":[h]}},"pointer":"/outcome/report/rows/0/site_metres/0","subtract":[0,0],"expected":[1.5,0],"tolerance":0.000001}},
        {"id":"takeoff","text":"Deliver the requested take-off","read":{"op":"state","args":{},"pointer":"/takeoff_rows","expected":1}},
        {"id":"rooms","text":"Two room outlines","verify":{"kind":"count","entity_type":"LwPolyline","value":2}},
        {"id":"untied","text":"Four metres must be drawn","verify":{"kind":"distance","points":[[0,0],[4,0]],"value":4}}
    ]})).unwrap();
    assert_eq!(s.revision(), before);
    assert_eq!(out["passed"], 1, "{out}");
    assert_eq!(out["failed"], 4, "{out}");
    assert_eq!(out["results"][1]["measured"], json!([-0.5, 0.]));
    assert_eq!(out["results"][2]["measured"], 0);
    let refused=call(&mut s,surface::READ,"brief_check",json!({"expectations":[{"id":"unsafe","text":"Cannot use an edit as evidence","read":{"op":"run","args":{"command":{"type":"delete_entities","targets":[h]}},"pointer":"/added","expected":[]}}]})).unwrap();
    assert_eq!(refused["failed"], 1);
    assert_eq!(s.revision(), before);
    for read in [
        json!({"op":"measure","args":{"points":[[0,0],[4,0]]},"pointer":"/length_m","expected":4}),
        json!({"op":"run","args":{"command":{"type":"verify","expectations":[{"kind":"distance","points":[[0,0],[4,0]],"value":4}]}},"pointer":"/outcome/results/0/passed","expected":true}),
    ] {
        let report = call(
            &mut s,
            surface::READ,
            "brief_check",
            json!({"expectations":[{
                "id":"drawn","text":"Four metres must exist in the drawing","read":read
            }]}),
        )
        .unwrap();
        assert_eq!(report["failed"], 1, "{report}");
        assert!(report["results"][0]["message"]
            .as_str()
            .unwrap()
            .contains("Literal arithmetic"));
    }
}

#[tokio::test]
async fn clinic_corner_and_partial_revision_intents_need_only_one_advised_retry() {
    use scripted::{Script, ScriptCall};
    use std::sync::{Arc, Mutex};
    fn opening(request: &qiyas_ai::TurnRequest) -> (&'static str, Value) {
        let results = tool_values(request);
        let wall = results
            .iter()
            .find(|v| v["outcome"]["centrelines"].is_array())
            .unwrap();
        (
            surface::EXECUTE,
            input(
                "run",
                json!({"command":{"type":"add_wall_openings","centreline":wall["outcome"]["centrelines"][0],"openings":[{"at":5,"width":0.9,"kind":"door"}]}}),
            ),
        )
    }
    fn advised_retry(request: &qiyas_ai::TurnRequest) -> (&'static str, Value) {
        let results = tool_values(request);
        let wall = results
            .iter()
            .find(|v| v["outcome"]["centrelines"].is_array())
            .unwrap();
        let error = results.last().unwrap();
        let at: f64 = error["message"]
            .as_str()
            .unwrap()
            .split("nearest valid `at`: ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        (
            surface::EXECUTE,
            input(
                "run",
                json!({"command":{"type":"add_wall_openings","centreline":wall["outcome"]["centrelines"][0],"openings":[{"at":at,"width":0.9,"kind":"door"}]},"expected_revision":{"geometry_revision":wall["revision"]["geometry_revision"]}}),
            ),
        )
    }
    fn latest_revision(request: &qiyas_ai::TurnRequest) -> (&'static str, Value) {
        let results = tool_values(request);
        (
            surface::EXECUTE,
            input(
                "run",
                json!({"type":"text","at":[1,1],"height":0.1,"value":"Survey point","expected_revision":{"document_revision":results.last().unwrap()["revision"]["document_revision"]}}),
            ),
        )
    }
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("clinic.dxf"),
        6,
    ))));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = Script::steps(
        vec![
            ScriptCall::Fixed(
                surface::EXECUTE,
                input(
                    "run",
                    json!({"command":{"type":"walls","runs":[{"points":[[0,0],[5,0],[5,4]]}],"thickness":0.2}}),
                ),
            ),
            ScriptCall::FromHistory(opening),
            ScriptCall::FromHistory(advised_retry),
            ScriptCall::FromHistory(latest_revision),
            ScriptCall::Fixed(
                surface::READ,
                input("query", json!({"kind":"texts","bounds":[[0,0],[2,2]]})),
            ),
        ],
        requests.clone(),
    );
    let mut agent = qiyas_ai::Agent::new(Box::new(provider), session.clone(), Default::default());
    let (tx, mut rx) = tokio::sync::mpsc::channel(500);
    agent
        .run_turn(
            "Add a door at the corner and label the survey point.".into(),
            tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut errors = 0;
    while let Ok(event) = rx.try_recv() {
        if let qiyas_ai::AgentEvent::ToolResult { is_error: true, .. } = event {
            errors += 1;
        }
    }
    assert_eq!(errors, 1, "only the impossible corner placement is refused");
    assert_eq!(
        session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .wall_groups(None)
            .unwrap()[0]
            .runs[0]
            .openings
            .len(),
        1
    );
    let requests = requests.lock().unwrap();
    assert!(tool_values(requests.last().unwrap())
        .last()
        .unwrap()
        .to_string()
        .contains("Survey point"));
    assert_eq!(scripted::observed_messages(&requests).len(), 5);
}

#[test]
fn clinic_brief_catches_missing_rooms_wrong_thickness_scale_survey_and_deliverables() {
    for unit in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("brief-items.dxf"), unit);
        let k = 1. / s.drawing.units.to_metres();
        let room=call(&mut s,surface::EXECUTE,"batch",json!({"layer":"Rooms","commands":[{"type":"draw","kind":"polyline","points":[[0,0],[4.*k,0],[4.*k,5.*k],[0,5.*k]],"closed":true}]})).unwrap()["added"][0].clone();
        run(
            &mut s,
            json!({"type":"walls","runs":[{"points":[[0,0],[6.*k,0]]}],"thickness":0.25*k}),
        );
        run(&mut s, json!({"type":"annotation_scale","name":"1:100"}));
        let coordinates = json!({"command":{"type":"site_coordinates","targets":[room]}});
        let out=call(&mut s,surface::READ,"brief_check",json!({"expectations":[
            {"id":"width","text":"Bedroom width 4 m","read":{"op":"run","args":coordinates,"pointer":"/outcome/report/rows/0/site_metres/1","subtract":[0,0],"expected":[4,0],"tolerance":0.000001}},
            {"id":"depth","text":"Bedroom depth 5 m","read":{"op":"run","args":coordinates,"pointer":"/outcome/report/rows/0/site_metres/2","subtract":[4,0],"expected":[0,5],"tolerance":0.000001}},
            {"id":"area","text":"Bedroom area 20 m²","verify":{"kind":"area","targets":[{"handle":room}],"value":20,"in_metres":true}},
            {"id":"rooms","text":"Two bedrooms, not one","verify":{"kind":"count","entity_type":"LwPolyline","layer":"Rooms","value":2}},
            {"id":"wall","text":"Wall thickness 300 mm","read":{"op":"run","args":{"command":{"type":"wall_groups"}},"pointer":"/outcome/groups/0/runs/0/thickness","expected":0.3*k,"tolerance":0.001*k}},
            {"id":"scale","text":"Scale 1:50","read":{"op":"state","args":{},"pointer":"/annotation_scale","expected":"1:50"}},
            {"id":"survey","text":"Base at E 674250000 / N 2734180000 mm","read":{"op":"run","args":coordinates,"pointer":"/outcome/report/rows/0/site_metres/0","expected":[674250,2734180],"tolerance":0.000001}},
            {"id":"takeoff","text":"Produce the take-off","read":{"op":"state","args":{},"pointer":"/takeoff_rows","expected":1}},
            {"id":"boq","text":"Produce the BOQ","read":{"op":"state","args":{},"pointer":"/boq_items","expected":1}},
            {"id":"revision","text":"Deliver the revision report","read":{"op":"artifact","args":{"path":"revision.json"},"pointer":"/nonempty","expected":true}},
            {"id":"export","text":"Deliver the exported workbook","read":{"op":"artifact","args":{"path":"quantities.xlsx"},"pointer":"/nonempty","expected":true}}
        ]})).unwrap();
        assert_eq!(out["passed"], 3, "{out}");
        assert_eq!(out["failed"], 8, "{out}");
        assert_eq!(out["results"][3]["measured"].as_f64(), Some(1.0));
        assert!((out["results"][4]["measured"].as_f64().unwrap() - 0.25 * k).abs() < 1e-6 * k);
        assert_eq!(out["results"][5]["measured"], "1:100");
        assert_eq!(out["results"][6]["measured"], json!([0.0, 0.0]));
    }
}

#[tokio::test]
async fn stale_plan_unmet_brief_and_blind_state_reach_the_final_answer_and_resume() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("plan.dxf"),
        6,
    ))));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut calls = vec![(
        surface::EXECUTE,
        input(
            "plan",
            json!({"phases":[{"id":"rooms","title":"Two bedrooms","status":"active"},{"id":"boq","title":"BOQ","status":"pending"}]}),
        ),
    )];
    for i in 0..8 {
        calls.push((surface::EXECUTE,input("run",json!({"command":{"type":"text","at":[i,0],"height":0.1,"value":format!("Note {i}")}}))));
    }
    calls.push((surface::READ,input("brief_check",json!({"expectations":[{"id":"rooms","text":"Two bedrooms of the requested sizes","verify":{"kind":"count","entity_type":"LwPolyline","value":2}}]}))));
    calls.push((surface::READ, input("state", json!({}))));
    calls.push((surface::CAPTURE, json!({"source":"picture","args":"{}"})));
    let settings = qiyas_ai::AgentSettings {
        image_input: Some(false),
        ..Default::default()
    };
    let mut agent = qiyas_ai::Agent::new(
        Box::new(scripted::Script::new(calls, requests.clone())),
        session.clone(),
        settings.clone(),
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(500);
    agent
        .run_turn_with(
            qiyas_ai::UserMessage {
                text: "Draw two bedrooms and deliver a BOQ.".into(),
                images: vec![qiyas_ai::UserImage {
                    media_type: "image/png".into(),
                    data: "image-that-must-not-reach-the-provider".into(),
                }],
                ..Default::default()
            },
            tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut stale = false;
    let mut answer = String::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            qiyas_ai::AgentEvent::PlanProgress { stale: true, .. } => stale = true,
            qiyas_ai::AgentEvent::ToolResult {
                is_error: true,
                summary,
                ..
            } => panic!("{summary}"),
            qiyas_ai::AgentEvent::Text { text } => answer.push_str(&text),
            _ => {}
        }
    }
    assert!(stale);
    assert!(answer.contains("Unfinished phases"));
    assert!(answer.contains("BOQ"));
    assert!(answer.contains("Unmet requirements"));
    assert!(answer.contains("Two bedrooms of the requested sizes"));
    for request in requests.lock().unwrap().iter() {
        for block in request.messages.iter().flat_map(|m| &m.content) {
            assert!(!matches!(block, qiyas_ai::ContentBlock::Image { .. }));
            if let qiyas_ai::ContentBlock::ToolResult { content, .. } = block {
                assert!(!content
                    .iter()
                    .any(|p| matches!(p, qiyas_ai::ResultPart::Png(_))));
            }
        }
    }
    let observed = tool_values(requests.lock().unwrap().last().unwrap());
    assert!(observed
        .iter()
        .any(|v| v["image_input"]["supported"] == false));
    let snapshot = agent.snapshot();
    let mut resumed = qiyas_ai::Agent::resume(
        snapshot,
        Box::new(scripted::Script::new(vec![], Arc::default())),
        session,
        settings,
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    resumed
        .run_turn(
            "Finish.".into(),
            tx,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let qiyas_ai::AgentEvent::Text { text: t } = event {
            text.push_str(&t);
        }
    }
    assert!(text.contains("BOQ"));
    assert!(text.contains("Two bedrooms of the requested sizes"));
    assert!(text.contains("Requirements needing a check"));
}
