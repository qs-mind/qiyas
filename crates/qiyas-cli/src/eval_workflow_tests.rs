//! A provider with no filesystem or Session access. Its decisions use conversation evidence.
use super::*;
use qiyas_ai::{ContentBlock, ResultPart, StopReason};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) fn definition_commands() -> Vec<Value> {
    vec![
        json!({"type":"define_object_type","definition":{"name":"ObservedRegion","category":"drawing evidence","geometry":"region","quantities":{"surface":"area"}}}),
        json!({"type":"recognition","kind":"define","rule":{"name":"observed-regions","object_type":"ObservedRegion","match":{"entity_type":"LwPolyline","closed_region":true},"confidence":0.6}}),
    ]
}
pub(super) fn mapping_commands() -> Vec<Value> {
    let citation = json!({"source":"drawing geometry","clause":"closed source boundary"});
    vec![
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{"name":"observed-geometry","title":"Geometric measurement","based_on":"geometry","source_note":"No standard selected; quantities from the drawing","rules":[{"id":"surface","element_type":"ObservedRegion","unit":"m2","terms":[{"label":"source area","operation":"add","factors":["surface"],"citation":citation}],"citation":citation}]}}),
        json!({"type":"measurement","kind":"set_mappings","mappings":[{"rule":"surface","code":"surface-{id}","description_en":"Measured surface","description_ar":"Measured surface","section":"Observed","trade":"","unit":"m2","citation":citation}]}),
    ]
}
fn results(req: &TurnRequest) -> Vec<Value> {
    req.messages
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|b| match b {
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                assert!(!is_error, "Tool failed: {content:?}");
                content.iter().find_map(|p| match p {
                    ResultPart::Text(t) => serde_json::from_str(t).ok(),
                    _ => None,
                })
            }
            _ => None,
        })
        .collect()
}
struct Workflow {
    calls: Arc<AtomicUsize>,
    first_boq: Arc<AtomicUsize>,
    kit: bool,
}

struct BusyProvider {
    calls: Arc<AtomicUsize>,
    failures: usize,
    retry_after: Option<u64>,
}
impl Provider for BusyProvider {
    fn stream_turn<'a>(
        &'a self,
        _req: TurnRequest,
        _sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            if self.calls.fetch_add(1, Ordering::SeqCst) < self.failures {
                Err(AiError::RateLimited {
                    retry_after: self.retry_after,
                })
            } else {
                Ok(TurnResult {
                    content: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage {
                        input_tokens: 100,
                        output_tokens: 20,
                        ..Default::default()
                    },
                })
            }
        })
    }
}

#[tokio::test]
async fn provider_busy_retries_back_off_honor_retry_after_and_charge_only_success() {
    for (failures, retry_after, minimum_seconds) in [(2, None, 3), (2, Some(1), 2), (9, Some(0), 0)]
    {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let totals = Arc::new(Mutex::new(RunUsage::default()));
        let provider = Metered {
            inner: Box::new(BusyProvider {
                calls: calls.clone(),
                failures,
                retry_after,
            }),
            dir: dir.path().into(),
            run: "retry".into(),
            prices: Prices {
                endpoint: None,
                input_per_million: 0.75,
                output_per_million: 3.75,
                cache_read_per_million: 0.75,
                cache_write_per_million: 0.75,
            },
            daily: 1.0,
            max_cost: 1.0,
            max_tokens: 200,
            max_requests: 1,
            totals: totals.clone(),
            steps: Some(dir.path().join("steps.jsonl")),
        };
        let started = std::time::Instant::now();
        let result = provider
            .stream_turn(
                TurnRequest {
                    system: "offline".into(),
                    messages: vec![],
                    tools: vec![],
                    model: "scripted".into(),
                    effort: "low".into(),
                    max_tokens: 200,
                },
                &mut |_| {},
            )
            .await;
        assert!(started.elapsed().as_secs() >= minimum_seconds);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let usage = totals.lock().unwrap();
        assert_eq!(usage.provider_retries, 2);
        assert_eq!(usage.unknown_usage_requests, 0);
        assert_eq!(usage.requests, if failures == 2 { 1 } else { 0 });
        assert_eq!(result.is_ok(), failures == 2);
        assert_eq!(ledger(dir.path()).unwrap().len(), 1);
        assert!((ledger(dir.path()).unwrap()[0].charged_usd - usage.estimated_usd).abs() < 1e-9);
        if failures > 2 {
            assert_eq!(usage.estimated_usd, 0.0);
        }
        let steps = fs::read_to_string(dir.path().join("steps.jsonl")).unwrap();
        assert_eq!(steps.matches("provider_retry").count(), 2);
    }
}
fn has_boq(value: &Value) -> bool {
    if value["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|v| v["code"].is_string() && v["quantity"].is_number())
    }) {
        return true;
    }
    match value {
        Value::Object(fields) => fields.values().any(has_boq),
        Value::Array(values) => values.iter().any(has_boq),
        _ => false,
    }
}
impl Provider for Workflow {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        _sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let seen = results(&req);
            if self.first_boq.load(Ordering::SeqCst) == 0 && seen.iter().any(has_boq) {
                self.first_boq.store(index, Ordering::SeqCst);
            }
            assert!(req
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .all(|b| !format!("{b:?}").contains("truth.json")));
            let kit_step = if self.kit {
                Some(match index {
                0 => ("qiyas_read", "digest", json!({})),
                1 => {
                    assert_eq!(seen.last().unwrap()["takeoff_kit"]["op"],"takeoff_kit");
                    ("qiyas_read", "takeoff_kit", json!({}))
                },
                2 => ("qiyas_capture", "picture", json!({"width":640})),
                3 => {
                    let kit = seen.iter().find(|v|v["batch"]["commands"].is_array()).expect("kit response");
                    let digest = seen.iter().find(|v|v["counts"].is_object()).unwrap();
                    assert!(digest["closed_regions"]["count"].as_u64().unwrap()>0);
                    let mut batch = kit["batch"].clone();
                    let commands = batch["commands"].as_array_mut().unwrap();
                    for command in commands.iter_mut() {
                        match (command["type"].as_str(),command["kind"].as_str()) {
                            (Some("define_object_type"),_) => {
                                command["definition"]["geometry"] = json!("region");
                                command["definition"]["quantities"] = json!({"surface":"geometry.area"});
                            },
                            (_,Some("define")) => command["rule"]["match"] = json!({"entity_type":"LwPolyline","closed_region":true}),
                            (_,Some("set_method")) => {
                                let rule = &mut command["method"]["rules"][0];
                                rule["id"] = json!("surface");
                                rule["unit"] = json!("m2");
                                rule["citation"]["clause"] = json!("closed source boundary");
                                rule["terms"][0]["citation"] = rule["citation"].clone();
                                rule["terms"][0]["label"] = json!("Measured surface");
                                rule["terms"][0]["factors"] = json!(["geometry.area"]);
                            },
                            (_,Some("set_mappings")) => {
                                command["mappings"][0]["rule"] = json!("surface");
                                command["mappings"][0]["unit"] = json!("m2");
                                command["mappings"][0]["code"] = json!("surface-{id}");
                                command["mappings"][0]["description_en"] = json!("Measured surface");
                                command["mappings"][0]["description_ar"] = json!("Measured surface");
                                command["mappings"][0]["citation"]["clause"] = json!("closed source boundary");
                            },
                            _ => {}
                        }
                    }
                    commands.push(kit["commands"]["check"]["example"].clone());
                    ("qiyas_execute", "batch", batch)
                },
                4 => ("qiyas_read", "takeoff_check", json!({})),
                _ => return Ok(TurnResult{content:vec![ContentBlock::Text{text:"Geometric BOQ generated. Remaining symbols and paths are unresolved; evidence coverage is incomplete.".into()}],stop_reason:StopReason::EndTurn,usage:Usage{input_tokens:100,output_tokens:20,..Default::default()}}),
            })
            } else {
                None
            };
            let (tool, op, args) = if let Some(step) = kit_step {
                step
            } else {
                match index {
                0=>("qiyas_read","digest",json!({})),
                1=>{assert_eq!(seen.last().unwrap()["counts"]["element_types"],0);("qiyas_read","commands",json!({"names":["define_object_type","recognition.define","recognition.apply","measurement.set_method","measurement.set_mappings"]}))},
                2=>{
                    let digest=seen.iter().find(|v|v["counts"].is_object()).unwrap();
                    assert!(digest["closed_regions"]["count"].as_u64().unwrap()>0);
                    ("qiyas_execute","batch",json!({"commands":definition_commands()}))
                },
                3=>("qiyas_execute","run",json!({"command":{"type":"recognition","kind":"apply"}})),
                4=>("qiyas_read","run",json!({"command":{"type":"recognition","kind":"queue","state":"pending"}})),
                5=>{
                    let queue=seen.last().unwrap();
                    let candidates=queue["outcome"]["result"]["candidates"].as_array().expect("candidate result");
                    assert!(!candidates.is_empty());
                    let commands:Vec<_>=candidates.iter().map(|c|json!({"type":"recognition","kind":"review","handle":c["handle"],"decision":"accepted"})).collect();
                    ("qiyas_execute","batch",json!({"commands":commands}))
                },
                6=>("qiyas_execute","batch",json!({"commands":mapping_commands()})),
                7=>("qiyas_read","run",json!({"command":{"type":"measurement","kind":"report"}})),
                8=>{
                    let digest=seen.iter().find(|v|v["counts"].is_object()).unwrap();
                    let count:u64=digest["entries"].as_array().unwrap().iter().filter(|v|v["section"]=="layers").filter_map(|v|v["kinds"]["LwPolyline"].as_u64()).sum();
                    ("qiyas_read","takeoff_check",json!({"expectations":[{"kind":"count","entity_type":"LwPolyline","value":count}]}))
                },
                9=>("qiyas_execute","run",json!({"command":{"type":"check"}})),
                _=>return Ok(TurnResult{content:vec![ContentBlock::Text{text:"BOQ generated from recognised boundaries using the geometric method. Unmeasured symbols, paths, annotations and low-confidence interpretations are listed in the end report; review is optional.".into()}],stop_reason:StopReason::EndTurn,usage:Usage{input_tokens:100,output_tokens:20,..Default::default()}}),
            }
            };
            Ok(TurnResult {
                content: vec![ContentBlock::ToolUse {
                    id: format!("workflow-{index}"),
                    name: tool.into(),
                    input: if tool == "qiyas_capture" {
                        json!({"source":op,"args":args})
                    } else {
                        json!({"op":op,"args":args})
                    },
                }],
                stop_reason: StopReason::ToolUse,
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    ..Default::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn mizan_alone_authors_recognises_maps_checks_and_reports_two_generated_cases() {
    for (typ, seed) in [("office", 15127), ("apartment", 17139)] {
        for kit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let drawing = dir.path().join("drawing.dxf");
            let (session, truth) =
                crate::bench::generate::offline_case(&drawing, typ, seed).unwrap();
            assert!(session.object_types().unwrap().is_empty());
            assert!(session.recognition_rules().unwrap().is_empty());
            assert!(session.takeoff.items.is_empty());
            let session = Arc::new(Mutex::new(Some(session)));
            let calls = Arc::new(AtomicUsize::new(0));
            let first_boq = Arc::new(AtomicUsize::new(0));
            let totals = Arc::new(Mutex::new(RunUsage::default()));
            let metered = Metered {
                inner: Box::new(Workflow {
                    calls: calls.clone(),
                    first_boq: first_boq.clone(),
                    kit,
                }),
                dir: dir.path().into(),
                run: typ.into(),
                prices: Prices {
                    endpoint: None,
                    input_per_million: 0.1,
                    output_per_million: 0.2,
                    cache_read_per_million: 0.1,
                    cache_write_per_million: 0.1,
                },
                daily: 5.0,
                max_cost: 1.0,
                max_tokens: 4096,
                max_requests: if kit { 8 } else { 11 },
                totals: totals.clone(),
                steps: Some(dir.path().join("steps.jsonl")),
            };
            let status = run_agent(
                Box::new(metered),
                session.clone(),
                AgentSettings::default(),
                "",
                Some(dir.path()),
            )
            .await;
            assert_eq!(status, "done");
            assert_eq!(calls.load(Ordering::SeqCst), if kit { 6 } else { 11 });
            assert_eq!(first_boq.load(Ordering::SeqCst), if kit { 4 } else { 8 });
            let guard = session.lock().unwrap();
            let session = guard.as_ref().unwrap();
            assert!(qiyas_core::knowledge::get(
                &session.drawing.doc,
                qiyas_core::knowledge::Kind::Queries,
                "result"
            )
            .unwrap()
            .is_some());
            let submission = crate::bench::submission::from_session(session).unwrap();
            let truth: crate::bench::Truth = serde_json::from_value(truth).unwrap();
            let score = crate::bench::score::score(&truth, &submission, Some(session));
            eprintln!("scripted {typ}: {}", score.rates);
            assert!(score.metrics.matched > 0, "{score:?}");
            assert_eq!(score.metrics.extras, 0);
            assert_eq!(score.metrics.matched, score.metrics.passed);
            assert!(
                session.takeoff.items.is_empty(),
                "No raw candidates submitted"
            );
            let log = fs::read_to_string(dir.path().join("agent.log")).unwrap();
            for operation in [
                "digest",
                if kit { "takeoff_kit" } else { "commands" },
                "recognition",
                "set_method",
                "set_mappings",
                "takeoff_check",
                "check",
            ] {
                assert!(log.contains(operation), "{operation}");
            }
            let steps = fs::read_to_string(dir.path().join("steps.jsonl")).unwrap();
            assert_eq!(steps.lines().count(), if kit { 13 } else { 23 });
            assert!(steps.contains("end_report"));
            assert!(steps.contains("reserved_usd") && steps.contains("estimated_usd"));
            assert!(totals.lock().unwrap().estimated_usd > 0.0);
            let evidence = json!({"case":typ,"kit":kit,"requests":calls.load(Ordering::SeqCst),"first_boq":first_boq.load(Ordering::SeqCst),"score":score.rates});
            let artifacts =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/mizan-discovery");
            fs::create_dir_all(&artifacts).unwrap();
            write_json(
                &artifacts.join(format!(
                    "{typ}-{}.json",
                    if kit { "after" } else { "before" }
                )),
                &evidence,
            )
            .unwrap();
        }
    }
}

#[test]
fn reservation_is_per_request_and_leaves_room_after_twenty_three_cents() {
    let req = TurnRequest {
        system: "Read the digest. ".repeat(18000),
        messages: Vec::new(),
        tools: Vec::new(),
        model: "scripted".into(),
        effort: "low".into(),
        max_tokens: 4096,
    };
    let prices = Prices {
        endpoint: None,
        input_per_million: 0.5,
        output_per_million: 3.0,
        cache_read_per_million: 0.1,
        cache_write_per_million: 0.5,
    };
    let reserve = prices.upper_bound(&req).unwrap();
    assert!(reserve > 0.0 && reserve < 0.2);
    assert!(0.23 + reserve < 1.0);
}

#[derive(Default)]
struct ReplayTrace {
    before: Vec<u64>,
    after: Vec<u64>,
    original: BTreeMap<String, ContentBlock>,
    system: Option<String>,
    finish_seen: bool,
}

/// Offline protocol-token estimate: UTF-8 bytes / 4, plus 1,024 tokens per picture.
/// This is deterministic accounting for comparison, not a claim about Gemini's tokenizer.
fn replay_tokens(req: &TurnRequest) -> u64 {
    fn size(value: &Value) -> usize {
        match value {
            Value::Object(fields)
                if matches!(
                    fields.get("type").and_then(Value::as_str),
                    Some("image" | "png")
                ) =>
            {
                4096
            }
            Value::Object(fields) => fields.iter().map(|(k, v)| k.len() + 4 + size(v)).sum(),
            Value::Array(values) => values.iter().map(size).sum(),
            Value::String(s) => s.len() + 2,
            _ => value.to_string().len(),
        }
    }
    let bytes = req.system.len()
        + size(&json!(req.messages))
        + req
            .tools
            .iter()
            .map(|t| t.description.len() + t.name.len() + size(&t.input_schema))
            .sum::<usize>();
    bytes.div_ceil(4) as u64
}

struct LivePattern {
    trace: Arc<Mutex<ReplayTrace>>,
    finish: bool,
}
impl Provider for LivePattern {
    fn stream_turn<'a>(
        &'a self,
        mut req: TurnRequest,
        _sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let finish = req
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .any(|b| matches!(b,ContentBlock::Text{text} if text.contains("[finish now]")));
            // Both estimates include identical budget notices; only old tool evidence differs.
            let input_tokens = replay_tokens(&req);
            let mut trace = self.trace.lock().unwrap();
            let index = trace.after.len();
            if let Some(system) = &trace.system {
                assert_eq!(system, &req.system);
            } else {
                trace.system = Some(req.system.clone());
            }
            trace.finish_seen |= finish;
            trace.after.push(input_tokens);
            let seen = results(&req);
            for message in &mut req.messages {
                for block in &mut message.content {
                    if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                        let id = tool_use_id.clone();
                        *block = trace
                            .original
                            .entry(id)
                            .or_insert_with(|| block.clone())
                            .clone();
                    }
                }
            }
            trace.before.push(replay_tokens(&req));
            drop(trace);
            let usage = Usage {
                input_tokens,
                output_tokens: 200,
                ..Default::default()
            };
            if self.finish && finish && index > 7 {
                return Ok(TurnResult{content:vec![ContentBlock::Text{text:"Partial geometric BOQ submitted. Remaining scope and raster uncertainty are unresolved; budget reserved for this end report.".into()}],stop_reason:StopReason::EndTurn,usage});
            }
            let (tool, op, args) = match index {
                0 => ("qiyas_read", "digest", json!({})),
                1 => (
                    "qiyas_execute",
                    "plan",
                    json!({"phases":[{"id":"evidence","title":"Read evidence","status":"active"},{"id":"takeoff","title":"Recognise and map quantities","status":"pending"}]}),
                ),
                2 => (
                    "qiyas_read",
                    "commands",
                    json!({"names":["define_object_type","recognition.define","recognition.schedule","recognition.apply","measurement.set_method","measurement.set_mappings"]}),
                ),
                3 => (
                    "qiyas_capture",
                    "picture",
                    json!({"width":640,"marks":true}),
                ),
                4 => ("qiyas_read", "records", json!({"collection":"entities"})),
                5 => {
                    let mut commands = definition_commands();
                    commands[1]["rule"]["confidence"] = json!(1.0);
                    commands.push(json!({"type":"recognition","kind":"apply"}));
                    commands.extend(mapping_commands());
                    ("qiyas_execute", "batch", json!({"commands":commands}))
                }
                6 => (
                    "qiyas_read",
                    "run",
                    json!({"command":{"type":"measurement","kind":"report"}}),
                ),
                7 => ("qiyas_read", "takeoff_check", json!({})),
                _ => {
                    assert!(
                        seen.iter().any(|v| v["counts"].is_object()),
                        "The digest remains available"
                    );
                    (
                        "qiyas_read",
                        "records",
                        json!({"collection":"entities","detail":"full"}),
                    )
                }
            };
            Ok(TurnResult {
                content: vec![ContentBlock::ToolUse {
                    id: format!("replay-{index}"),
                    name: tool.into(),
                    input: if tool == "qiyas_capture" {
                        json!({"source":op,"args":args})
                    } else {
                        json!({"op":op,"args":args})
                    },
                }],
                stop_reason: StopReason::ToolUse,
                usage,
            })
        })
    }
}

#[tokio::test]
async fn live_pattern_has_bounded_history_finishes_at_eighty_percent_and_scores_at_caps() {
    for (name, finish, max_requests, max_cost) in [
        ("finish", true, 25, 1.0),
        ("request-cap", false, 25, 1.0),
        ("cost-cap", false, 40, 0.22),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let drawing = dir.path().join("drawing.dxf");
        let (session, truth) =
            crate::bench::generate::offline_case(&drawing, "office", 15127).unwrap();
        let truth: crate::bench::Truth = serde_json::from_value(truth).unwrap();
        let session = Arc::new(Mutex::new(Some(session)));
        let trace = Arc::new(Mutex::new(ReplayTrace::default()));
        let totals = Arc::new(Mutex::new(RunUsage::default()));
        let provider = Metered {
            inner: Box::new(LivePattern {
                trace: trace.clone(),
                finish,
            }),
            dir: dir.path().into(),
            run: name.into(),
            prices: Prices {
                endpoint: None,
                input_per_million: 0.75,
                output_per_million: 3.75,
                cache_read_per_million: 0.75,
                cache_write_per_million: 0.75,
            },
            daily: 5.0,
            max_cost,
            max_tokens: 4096,
            max_requests,
            totals: totals.clone(),
            steps: Some(dir.path().join("steps.jsonl")),
        };
        let status = run_agent(
            Box::new(provider),
            session.clone(),
            AgentSettings::default(),
            "",
            Some(dir.path()),
        )
        .await;
        if finish {
            assert_eq!(status, "done");
        } else if name == "request-cap" {
            assert!(status.contains("request limit"), "{status}");
        } else {
            assert!(status.contains("cost limit"), "{status}");
        }
        let report: Value = read_json(&dir.path().join("end-report.json")).unwrap();
        assert!(report["facts"]["submitted_items"].as_u64().unwrap() > 0);
        assert!(!report["answer"].as_str().unwrap().is_empty());
        let mut guard = session.lock().unwrap();
        let session = guard.as_mut().unwrap();
        let path = save_outputs(session, dir.path(), &drawing, 1, &truth).unwrap();
        let submission: crate::bench::model::Submission = read_json(&path).unwrap();
        let score = crate::bench::score::score(&truth, &submission, Some(session));
        assert!(score.metrics.matched > 0);
        assert_eq!(score.metrics.matched, score.metrics.passed);
        let trace = trace.lock().unwrap();
        assert!(trace.finish_seen);
        assert!(trace.after.len() > 8);
        assert!(
            trace.after.last().unwrap() * 2 < *trace.before.last().unwrap(),
            "before {:?}; after {:?}",
            trace.before,
            trace.after
        );
        assert!(
            trace.after.iter().max().unwrap() < &20_000,
            "{:?}",
            trace.after
        );
        let usage = totals.lock().unwrap();
        if finish {
            assert!(usage.estimated_usd < 0.25, "{}", usage.estimated_usd);
        }
        let evidence = json!({"run":name,"before":trace.before,"after":trace.after,"estimated_usd":usage.estimated_usd,"status":status,
            "counting":"UTF-8 bytes / 4, 1024 tokens per image; same calls and outputs, before retains all history","score":score.rates});
        eprintln!("offline token replay: {evidence}");
        let artifacts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/mizan-live-fixes");
        fs::create_dir_all(&artifacts).unwrap();
        write_json(&artifacts.join(format!("tokens-{name}.json")), &evidence).unwrap();
    }
}

#[tokio::test]
async fn done_with_zero_items_is_a_logged_investigation_failure() {
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("empty.dxf"),
        6,
    ))));
    let provider = super::tests::Scripted {
        calls: Arc::new(std::sync::atomic::AtomicU32::new(0)),
        usage: Usage {
            input_tokens: 100,
            output_tokens: 20,
            ..Default::default()
        },
        command: None,
    };
    let status = run_agent(
        Box::new(provider),
        session,
        AgentSettings {
            max_tokens: 4096,
            ..Default::default()
        },
        "Scan could not be calibrated",
        Some(dir.path()),
    )
    .await;
    assert!(status.contains("done with 0 submitted items"), "{status}");
    let report: Value = read_json(&dir.path().join("end-report.json")).unwrap();
    assert_eq!(report["facts"]["submitted_items"], 0);
    assert!(fs::read_to_string(dir.path().join("steps.jsonl"))
        .unwrap()
        .contains("investigate capture"));
}
