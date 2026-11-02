use super::*;
use qiyas_ai::StopReason;
use std::sync::atomic::{AtomicUsize, Ordering};

const SECRET: &str = "credential-sentinel-never-export-739104";

struct Scripted {
    calls: AtomicUsize,
}
impl Provider for Scripted {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        _sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            assert!(!format!("{req:?}").contains(SECRET));
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let call = match index {
                0 => Some((
                    "qiyas_read",
                    "commands",
                    json!({"names":["define_block","insert_block","text","dimension.linear","make_sheet"]}),
                )),
                1 => Some((
                    "qiyas_execute",
                    "batch",
                    json!({"commands":[
                        {"type":"define_block","name":"CONCOURSE-BAY","base":[0,0],"entities":[
                            {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":120,"y":0,"z":0}}},
                            {"type":"Line","data":{"start":{"x":120,"y":0,"z":0},"end":{"x":120,"y":60,"z":0}}}
                        ]},
                        {"type":"insert_block","name":"CONCOURSE-BAY","at":[0,0]},
                        {"type":"insert_block","name":"CONCOURSE-BAY","at":[140,0]},
                        {"type":"text","value":"Metropolitan transport interchange","at":[0,70],"height":1},
                        {"type":"dimension","kind":"linear","p1":[0,0],"p2":[260,0],"line_point":[130,-10]}
                    ]}),
                )),
                2 => Some((
                    "qiyas_execute",
                    "run",
                    json!({"command":{"type":"insert_block","name":"MISSING-CONCOURSE-BAY","at":[0,0]}}),
                )),
                3 => Some((
                    "qiyas_execute",
                    "run",
                    json!({"command":{"type":"make_sheet","name":"A-101 Interchange","project":"Metropolitan transport interchange","title":"Concourse plan","drawing_no":"A-101","language":"en"}}),
                )),
                _ => None,
            };
            let has_call = call.is_some();
            let content = match call {
                Some((name, op, args)) => vec![ContentBlock::ToolUse { id:format!("design-{index}"), name:name.into(), input:json!({"op":op,"args":args}) }],
                None => vec![ContentBlock::Text { text:"Two concourse bays and the review sheet are drawn. The missing block call failed.".into() }],
            };
            Ok(TurnResult {
                content,
                stop_reason: if has_call {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                },
                usage: Usage {
                    input_tokens: (index as u64 + 1) * 100,
                    output_tokens: (index as u64 + 1) * 20,
                    ..Usage::default()
                },
            })
        })
    }
}

fn args(out: PathBuf) -> DesignArgs {
    DesignArgs {
        brief: "Design a metropolitan transport interchange.".into(),
        out,
        provider: None,
        model: None,
        max_requests: 10,
        language: None,
    }
}

async fn scripted_run(out: &Path, max_requests: u32) -> Value {
    let args = args(out.into());
    let output = prepare_output(out).unwrap();
    // Settings can contain unrelated credentials; only the selected public profile enters the agent.
    let saved = selected(json!({"provider":"anthropic","providers":{"anthropic":{"model":"scripted","api_key":SECRET}},"language":"en","mcp_token":SECRET}), &args).unwrap();
    run_with_provider(Run {
        provider: Box::new(Scripted {
            calls: AtomicUsize::new(0),
        }),
        session: Arc::new(Mutex::new(Some(Session::new_blank(
            &output.join("drawing.dwg"),
            6,
        )))),
        settings: saved.settings(4096),
        brief: args.brief,
        output,
        max_requests,
        country: None,
        budget: None,
    })
    .await
    .unwrap()
}

fn check_no_secret(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            check_no_secret(&path);
        } else {
            assert!(
                !fs::read(&path)
                    .unwrap()
                    .windows(SECRET.len())
                    .any(|s| s == SECRET.as_bytes()),
                "{}",
                path.display()
            );
        }
    }
}

#[tokio::test]
async fn scripted_design_writes_dwg_renders_logs_counts_usage_and_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    let report = scripted_run(&a, 10).await;
    assert_eq!(report["status"], "done", "{report}");
    assert_eq!(report["requests"], 5);
    assert_eq!(report["tool_calls"], 4);
    assert_eq!(report["tool_calls_by_operation"]["qiyas_execute.run"], 2);
    assert_eq!(report["tool_calls_by_kind"]["insert_block"], 3);
    assert_eq!(report["tool_calls_by_kind"]["dimension.linear"], 1);
    assert_eq!(
        report["failed_calls"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    assert!(report["failed_calls"][0]["error"]
        .as_str()
        .unwrap()
        .contains("MISSING-CONCOURSE-BAY"));
    assert_eq!(report["tokens"]["input"]["max"], 500);
    assert_eq!(report["tokens"]["input"]["mean"], 300.0);
    assert_eq!(report["tokens"]["output"]["max"], 100);
    assert_eq!(report["tokens"]["output"]["mean"], 60.0);
    assert_eq!(report["drawing"]["inserts_by_block"]["CONCOURSE-BAY"], 2);
    let block = report["drawing"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "CONCOURSE-BAY")
        .unwrap();
    assert_eq!(block["inserts"], 2);
    assert_eq!(block["entities_by_kind"]["Line"], 2);
    assert_eq!(report["drawing"]["dimensions"], 1);
    assert_eq!(report["drawing"]["entities_by_kind"]["Insert"], 2);
    assert_eq!(report["drawing"]["named_blocks_defined"], 1);
    assert!(report["drawing"]["texts"].as_u64().unwrap() > 1);
    assert_eq!(report["drawing"]["sheets_with_title_blocks"], 1);
    assert_eq!(report["artifact_errors"], json!([]));
    assert!(report["final_answer"]
        .as_str()
        .unwrap()
        .contains("Two concourse bays"));
    for file in [
        "drawing.dwg",
        "model.png",
        "agent.log",
        "steps.jsonl",
        "report.json",
        "report.md",
    ] {
        assert!(a.join(file).is_file(), "{file}");
    }
    let reopened = Session::open_standalone(&a.join("drawing.dwg")).unwrap();
    assert!(reopened
        .blocks()
        .iter()
        .any(|block| block.name == "CONCOURSE-BAY"));
    for sheet in report["drawing"]["sheets"].as_array().unwrap() {
        let bytes = fs::read(a.join(sheet["png"].as_str().unwrap())).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        let image = tiny_skia::Pixmap::decode_png(&bytes).unwrap();
        assert!(image.width() > 0 && image.height() > 0);
        if sheet["is_model"] == true || sheet["title_block"] == true {
            assert!(image
                .data()
                .chunks_exact(4)
                .any(|px| px[..3] != [255, 255, 255]));
        }
    }
    let steps = fs::read_to_string(a.join("steps.jsonl")).unwrap();
    assert!(steps
        .lines()
        .all(|line| serde_json::from_str::<Value>(line).is_ok()));
    assert!(steps.contains("design_response") && steps.contains("tool_result"));
    scripted_run(&b, 1).await;
    let compared = compare(&a, &b).unwrap();
    assert!(compared.contains("| requests | 5 | 1 |"));
    assert!(compared.contains("CONCOURSE-BAY") && compared.contains("MISSING-CONCOURSE-BAY"));
    assert!(compared.contains("tokens.input.max") && compared.contains("final_answer"));
    check_no_secret(dir.path());
}

#[tokio::test]
async fn request_limit_keeps_partial_drawing_and_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let report = scripted_run(&dir.path().join("run"), 1).await;
    assert_eq!(report["requests"], 1);
    assert!(report["status"].as_str().unwrap().contains("request limit"));
    assert!(dir.path().join("run/drawing.dwg").is_file());
    assert!(dir.path().join("run/model.png").is_file());
    assert_eq!(report["tokens"]["input"]["max"], 100);
}

#[test]
fn brief_settings_and_output_validation_are_offline() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("brief.txt");
    fs::write(&file, "صمم محطة نقل إقليمية").unwrap();
    assert_eq!(
        brief(file.to_str().unwrap()).unwrap(),
        "صمم محطة نقل إقليمية"
    );
    assert!(brief("  ").is_err());
    assert!(prepare_output(dir.path()).is_err());
    let mut args = args(dir.path().join("out"));
    args.provider = Some("claude_code".into());
    args.language = Some("ar".into());
    args.model = Some("opus".into());
    let saved = selected(
        json!({"provider":"anthropic","providers":{"claude_code":{"effort":"high"}}}),
        &args,
    )
    .unwrap();
    assert_eq!(saved.name, "claude_code");
    assert_eq!(saved.model, "opus");
    assert_eq!(saved.language, "ar");
    assert_eq!(saved.effort, "high");
    assert!(compare(dir.path(), dir.path()).is_err());
}

struct Delegated;
impl Provider for Delegated {
    fn owns_turn(&self) -> bool {
        true
    }
    fn stream_turn<'a>(
        &'a self,
        _req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            for (id, input_tokens) in [("z-first", 100), ("z-first", 150), ("a-second", 200)] {
                sink(StreamEvent::Diagnostic { kind:"CLAUDE REQUEST USAGE".into(), text:json!({"id":id,"usage":Usage {input_tokens,output_tokens:20,..Usage::default()}}).to_string() });
            }
            Ok(TurnResult {
                content: vec![ContentBlock::Text {
                    text: "The interchange needs a programme before drawing.".into(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: Usage {
                    input_tokens: 350,
                    output_tokens: 40,
                    ..Usage::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn delegated_usage_keeps_request_order_and_deduplicates_message_parts() {
    let dir = tempfile::tempdir().unwrap();
    let report = run_with_provider(Run {
        provider: Box::new(Delegated),
        session: Arc::new(Mutex::new(Some(Session::new_blank(
            &dir.path().join("drawing.dwg"),
            6,
        )))),
        settings: AgentSettings::default(),
        brief: "Design a regional interchange.".into(),
        output: dir.path().into(),
        max_requests: 10,
        country: None,
        budget: None,
    })
    .await
    .unwrap();
    assert_eq!(report["requests"], 2);
    assert_eq!(report["request_usage"][0]["usage"]["input_tokens"], 150);
    assert_eq!(report["tokens"]["input"]["total"], 350);
    assert_eq!(report["tokens"]["input"]["mean"], 175.0);
    assert!(fs::read_to_string(dir.path().join("steps.jsonl"))
        .unwrap()
        .contains("delegated_request_usage"));
}

#[test]
fn design_host_refuses_settings_reads_but_allows_record_pointers() {
    let dir = tempfile::tempdir().unwrap();
    let host = DesignHost {
        session: Arc::new(Mutex::new(None)),
        root: dir.path().into(),
    };
    assert!(host.before_tool("qiyas_read", &json!({"op":"document","args":{"path":"/home/example/.config/com.qsmind.qiyas/settings.json"}})).is_err());
    assert!(host.before_tool("qiyas_execute", &json!({"op":"update_records","args":{"updates":[{"path":"/properties/height","value":4}]}})).is_ok());
}

#[tokio::test]
async fn budget_refusal_is_not_counted_as_a_provider_request() {
    let dir = tempfile::tempdir().unwrap();
    let output = prepare_output(&dir.path().join("run")).unwrap();
    let ledger = dir.path().join("ledger");
    fs::create_dir(&ledger).unwrap();
    let report = run_with_provider(Run {
        provider: Box::new(Scripted {
            calls: AtomicUsize::new(0),
        }),
        session: Arc::new(Mutex::new(Some(Session::new_blank(
            &output.join("drawing.dwg"),
            6,
        )))),
        settings: AgentSettings::default(),
        brief: "Design a metropolitan interchange.".into(),
        output,
        max_requests: 10,
        country: None,
        budget: Some(PaidBudget {
            dir: ledger,
            daily: 0.0,
            prices: Prices {
                endpoint: None,
                input_per_million: 1.0,
                output_per_million: 1.0,
                cache_read_per_million: 1.0,
                cache_write_per_million: 1.0,
            },
        }),
    })
    .await
    .unwrap();
    assert_eq!(report["requests"], 0);
    assert_eq!(report["tokens"]["input"]["known_requests"], 0);
    assert_eq!(report["tokens"]["input"]["mean"], Value::Null);
    assert!(report["status"].as_str().unwrap().contains("cost limit"));
    assert!(dir.path().join("run/drawing.dwg").is_file());
    assert!(dir.path().join("run/steps.jsonl").is_file());
    assert!(dir.path().join("run/agent.log").is_file());
}
