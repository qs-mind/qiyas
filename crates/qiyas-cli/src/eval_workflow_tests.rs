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
            assert!(req
                .messages
                .iter()
                .flat_map(|m| &m.content)
                .all(|b| !format!("{b:?}").contains("truth.json")));
            let (tool,op,args)=match index {
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
                _=>return Ok(TurnResult{content:vec![ContentBlock::Text{text:"BOQ generated from recognised boundaries using the geometric method. Unmeasured symbols, paths, annotations and low-confidence interpretations are listed in the end report; review is optional.".into()}],stop_reason:StopReason::EndTurn,usage:Usage{input_tokens:100,output_tokens:20,..Default::default()}}),
            };
            Ok(TurnResult {
                content: vec![ContentBlock::ToolUse {
                    id: format!("workflow-{index}"),
                    name: tool.into(),
                    input: json!({"op":op,"args":args}),
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
        let dir = tempfile::tempdir().unwrap();
        let drawing = dir.path().join("drawing.dxf");
        let (session, truth) = crate::bench::generate::offline_case(&drawing, typ, seed).unwrap();
        assert!(session.object_types().unwrap().is_empty());
        assert!(session.recognition_rules().unwrap().is_empty());
        assert!(session.takeoff.items.is_empty());
        let session = Arc::new(Mutex::new(Some(session)));
        let calls = Arc::new(AtomicUsize::new(0));
        let totals = Arc::new(Mutex::new(RunUsage::default()));
        let metered = Metered {
            inner: Box::new(Workflow {
                calls: calls.clone(),
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
            max_requests: 10,
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
        assert_eq!(calls.load(Ordering::SeqCst), 10);
        let guard = session.lock().unwrap();
        let session = guard.as_ref().unwrap();
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
            "commands",
            "recognition",
            "set_method",
            "set_mappings",
            "takeoff_check",
        ] {
            assert!(log.contains(operation), "{operation}");
        }
        let steps = fs::read_to_string(dir.path().join("steps.jsonl")).unwrap();
        assert_eq!(steps.lines().count(), 20);
        assert!(steps.contains("reserved_usd") && steps.contains("estimated_usd"));
        assert!(totals.lock().unwrap().estimated_usd > 0.0);
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
