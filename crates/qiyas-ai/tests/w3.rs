//! W3: what Mizan hands a client, replayed offline with a scripted provider (no model, no
//! network). The founder's villa request in Arabic: walls and openings, rooms, tags, an
//! itemised BOQ, a real sheet, schedules, a plot and the exports. The replay shows the result
//! is clean: two-decimal areas with units, no ids on a sheet, tagged openings, totals per kind,
//! doors by size, one export folder, and a layout at a true scale with a filled title block.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use opencadcodec::EntityType;
use qiyas_ai::provider::{BoxFuture, Sink};
use qiyas_ai::surface::{EXECUTE, READ};
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, ContentBlock, Provider, StopReason, TurnRequest,
    TurnResult,
};
use qiyas_core::check::CheckScope;
use qiyas_core::Session;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

type Shared = Arc<Mutex<Option<Session>>>;
type Calls = Vec<(&'static str, Value)>;

/// One scripted answer: fixed tool calls, or calls worked out from the drawing as it is then.
/// When the script runs out the model closes the turn with a word.
enum Reply {
    Calls(Calls),
    Then(Box<dyn Fn(&Session) -> Calls + Send>),
}

struct Queue {
    replies: Mutex<VecDeque<Reply>>,
    session: Shared,
}

impl Provider for Queue {
    fn image_input(&self, _model: &str) -> Option<bool> {
        Some(false)
    }

    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        let id = request.messages.len();
        let next = self.replies.lock().unwrap().pop_front();
        let calls = match next {
            Some(Reply::Calls(calls)) => Some(calls),
            Some(Reply::Then(build)) => {
                let guard = self.session.lock().unwrap();
                Some(build(guard.as_ref().unwrap()))
            }
            _ => None,
        };
        Box::pin(async move {
            Ok(match calls {
                Some(calls) => TurnResult {
                    content: calls
                        .into_iter()
                        .enumerate()
                        .map(|(n, (name, input))| ContentBlock::ToolUse {
                            id: format!("call_{id}_{n}"),
                            name: name.into(),
                            input,
                        })
                        .collect(),
                    stop_reason: StopReason::ToolUse,
                    usage: Default::default(),
                },
                None => TurnResult {
                    content: vec![ContentBlock::Text {
                        text: "تم.".into()
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: Default::default(),
                },
            })
        })
    }
}

fn exec(op: &str, args: Value) -> (&'static str, Value) {
    (EXECUTE, json!({"op": op, "args": args}))
}
fn run(command: Value) -> (&'static str, Value) {
    exec("run", json!({"command": command}))
}
fn read(op: &str, args: Value) -> (&'static str, Value) {
    (READ, json!({"op": op, "args": args}))
}

fn texts_on(s: &Session, block: &str) -> Vec<String> {
    s.drawing
        .doc
        .entities_in_block(block)
        .flat_map(|e| match e {
            EntityType::MText(t) => vec![t.value.clone()],
            EntityType::Text(t) => vec![t.value.clone()],
            EntityType::Table(t) => (0..t.rows.len())
                .flat_map(|r| (0..t.columns.len()).map(move |c| (r, c)))
                .filter_map(|(r, c)| t.cell_text(r, c).map(str::to_string))
                .collect(),
            _ => vec![],
        })
        .collect()
}

#[tokio::test]
async fn the_villa_request_in_arabic_gives_a_clean_client_package() {
    let dir = tempfile::tempdir().unwrap();
    // The app's hidden placeholder folder and the visible Documents folder.
    let hidden = dir.path().join("config").join("untitled");
    let documents = dir.path().join("Documents");
    std::fs::create_dir_all(&hidden).unwrap();
    std::fs::create_dir_all(&documents).unwrap();
    let mut session = Session::new_blank(&hidden.join("Untitled-1.dwg"), 6);
    session.set_export_directory(documents.clone());
    let session: Shared = Arc::new(Mutex::new(Some(session)));

    let cite = json!({"source":"Villa assumptions","clause":"1"});
    let count = |id: &str, element: &str| {
        json!({"id":id,"kind":"count","basis":"geometric","element_type":element,"unit":"pcs",
            "terms":[{"label":"Count","operation":"add","factors":["pieces"],"citation":cite}],
            "citation":cite})
    };
    let plan = |active: usize| {
        exec(
            "plan",
            json!({"phases":(["الرسم: فحص العناصر", "الربط: فحص البنود", "الطريقة: فحص القواعد", "التسليم: فحص المخرجات"].iter().enumerate().map(|(i,title)|json!({"id":format!("p{i}"),"title":title,"status":if i<active {"done"} else if i==active {"active"} else {"pending"}})).collect::<Vec<_>>())}),
        )
    };
    let replies = vec![
        Reply::Calls(vec![plan(0)]),
        // The plan: an outer ring, one partition, doors and windows of three sizes.
        Reply::Calls(vec![run(
            json!({"type":"walls","thickness":0.2,"height":3.0,
            "runs":[{"points":[[0,0],[12,0],[12,15],[0,15]],"closed":true},{"points":[[0,7.5],[12,7.5]]}],
            "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"},
                        {"run":0,"at":6,"width":1.2,"kind":"window"},
                        {"run":0,"at":9,"width":1.0,"kind":"door"},
                        {"run":0,"at":18,"width":1.2,"kind":"window"},
                        {"run":1,"at":3,"width":0.9,"kind":"door"}]}),
        )]),
        // Rooms, without a word about language: the user wrote Arabic.
        Reply::Calls(vec![run(json!({"type":"label_spaces","query":{"seeds":[
            {"name":"Living","point":[3,3]},{"name":"Bedroom","point":[3,10]}]}}))]),
        // Tags for every door and window, in one step.
        Reply::Calls(vec![run(
            json!({"type":"object_tags","category":"opening","property":"mark"}),
        )]),
        Reply::Calls(vec![read(
            "records",
            json!({"collection":"objects","fields":["type"]}),
        )]),
        Reply::Calls(vec![plan(1)]),
        // The BOQ: a static "various sizes" description, as the founder's run wrote it.
        Reply::Calls(vec![run(
            json!({"type":"measurement","kind":"set_mappings","mappings":[
                {"rule":"doors","code":"A-D01","description_en":"Doors, various sizes","description_ar":"أبواب بمقاسات متنوعة","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
                {"rule":"windows","code":"A-W01","description_en":"Windows, various sizes","description_ar":"شبابيك بمقاسات متنوعة","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
        )]),
        Reply::Calls(vec![read(
            "run",
            json!({"command":{"type":"measurement","kind":"report"},"detail":"summary"}),
        )]),
        Reply::Calls(vec![plan(2)]),
        Reply::Calls(vec![run(
            json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
                "version":1,"name":"Villa geometric quantities","title":"Villa","based_on":"drawing","source_note":"Villa assumptions",
                "rules":[count("doors","Legacy door"),count("windows","Legacy window")]}}),
        )]),
        Reply::Calls(vec![read(
            "run",
            json!({"command":{"type":"measurement","kind":"report"},"detail":"summary"}),
        )]),
        Reply::Calls(vec![plan(3)]),
        // The sheet, in one step.
        Reply::Calls(vec![run(json!({"type":"make_sheet","name":"A-101",
            "project":"فيلا القاهرة","title":"المسقط الأفقي","drawing_no":"A-101",
            "notes":["الأبعاد بالمتر.","الافتراضات للمراجعة."]}))]),
        // Schedules on the sheet, then back to the model.
        Reply::Then(Box::new(|s| {
            let block = s
                .drawing
                .sheets()
                .into_iter()
                .find(|x| x.name == "A-101")
                .unwrap()
                .block;
            vec![
                run(json!({"type":"set_sheet","block":block})),
                run(json!({"type":"opening_schedule"})),
                run(json!({"type":"room_schedule"})),
                run(json!({"type":"set_sheet","block":"*Model_Space"})),
            ]
        })),
        Reply::Calls(vec![read("takeoff_check", json!({}))]),
        // The plot and the exports: no folder named anywhere.
        Reply::Calls(vec![exec("plot", json!({}))]),
        Reply::Calls(vec![exec(
            "export_takeoff",
            json!({"path":"Villa DRAFT take-off.xlsx"}),
        )]),
        Reply::Calls(vec![run(
            json!({"type":"deliverable","kind":"export","report":"boq","format":"excel","path":"Villa_BOQ.xlsx"}),
        )]),
        Reply::Calls(vec![read("check", json!({}))]),
        Reply::Calls(vec![plan(4)]),
    ];
    let provider = Queue {
        replies: Mutex::new(replies.into()),
        session: session.clone(),
    };
    let mut agent = Agent::new(
        Box::new(provider),
        session.clone(),
        AgentSettings::default(),
    );
    let (tx, mut rx) = mpsc::channel(10_000);
    agent
        .run_turn(
            "ارسم فيلا دورين وطلّع مقايستها".to_string(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    // Every step worked: a refused command would only show as a tool result for the model.
    let failed: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolResult {
                is_error: true,
                summary,
                id,
                ..
            } => {
                let call = events.iter().find_map(|c| match c {
                    AgentEvent::ToolCall {
                        id: call, input, ..
                    } if call == id => Some(input.to_string()),
                    _ => None,
                });
                Some(format!("{summary} <= {}", call.unwrap_or_default()))
            }
            _ => None,
        })
        .collect();
    {
        // The sheet's own title block, notes and schedules are not evidence the quantity check
        // can misread: a sheet number such as A-101 is not a reference to element 101.
        let guard = session.lock().unwrap();
        let check = guard.as_ref().unwrap().takeoff_check(0, &[]).unwrap();
        assert_eq!(check["measurement_errors"], 0, "{check}");
        assert_eq!(check["unmatched_schedule_rows"], 0, "{check}");
    }
    assert!(failed.is_empty(), "{failed:#?}");
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Done { .. })),
        "{events:?}"
    );

    let guard = session.lock().unwrap();
    let s = guard.as_ref().unwrap();

    // 1. Areas: two decimals and the unit, in the user's language.
    let model = texts_on(s, "*Model_Space");
    let labels: Vec<_> = model.iter().filter(|t| t.contains("المساحة")).collect();
    assert_eq!(labels.len(), 2, "{model:?}");
    for label in labels {
        assert!(
            label
                .strip_prefix("المساحة ")
                .and_then(|r| r.strip_suffix(" م²"))
                .is_some_and(|n| n.contains('٫')
                    && n.split('٫').nth(1).is_some_and(|d| d.chars().count() == 2)),
            "{label}"
        );
    }

    // 2. The sheet: a true scale, a filled captioned title block, Arabic throughout.
    let block = s
        .drawing
        .sheets()
        .into_iter()
        .find(|x| x.name == "A-101")
        .unwrap()
        .block;
    let sheet = texts_on(s, &block);
    let all = sheet.join("|");
    for must in [
        "فيلا القاهرة",
        "المسقط الأفقي",
        "A-101",
        "للمراجعة",
        "قياس",
        "مقياس الرسم",
        "ملاحظات عامة",
        "العدد",
        "إجمالي الأبواب",
    ] {
        assert!(all.contains(must), "{must} missing in {all}");
    }
    assert!(
        !all.contains("For review") && !all.contains("Mark"),
        "{all}"
    );
    let views = s.paper_views_of(&block);
    assert_eq!(views.len(), 1);
    let metres_per_unit = s.drawing.units.to_metres();
    let ratio = 1000.0 * metres_per_unit / views[0].scale;
    assert!(
        [50.0, 75.0, 100.0].contains(&ratio.round()) && (ratio - ratio.round()).abs() < 1e-9,
        "a standard scale, got 1:{ratio}"
    );

    // 3. No ids anywhere a client looks, every opening tagged, one total per kind.
    let report = s.check_drawing(&CheckScope {
        rules: vec![
            "internal_id_in_text".into(),
            "opening_untagged".into(),
            "duplicate".into(),
        ],
        ..Default::default()
    });
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    let on_layout = s.check_drawing(&CheckScope {
        sheet: Some(block.clone()),
        rules: vec!["internal_id_in_text".into()],
        ..Default::default()
    });
    assert!(on_layout.findings.is_empty(), "{:?}", on_layout.findings);
    let totals: Vec<_> = sheet.iter().filter(|t| t.starts_with("إجمالي")).collect();
    assert!(totals.len() >= 2, "a total per kind: {sheet:?}");

    // 4. The BOQ lists doors by size, not "various sizes" in one line.
    let measured = s.measurement_report().unwrap();
    let doors = measured
        .items
        .iter()
        .filter(|i| i.code.starts_with("A-D01"))
        .count();
    assert_eq!(
        doors,
        2,
        "{:?}",
        measured
            .items
            .iter()
            .map(|i| &i.description_en)
            .collect::<Vec<_>>()
    );

    // 5. One folder for the sheet and the exports: the visible one, never the hidden one.
    let mut names: Vec<_> = std::fs::read_dir(&documents)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    assert_eq!(names.len(), 3, "{names:?}");
    // No sheet was named: plot printed the layout make_sheet had just made.
    assert!(
        names
            .iter()
            .any(|n| n.ends_with(".pdf") && n.contains("A-101")),
        "{names:?}"
    );
    // The user's language belongs to the turn that asked for it.
    assert_eq!(s.sheet_language(), None);
    assert!(names.contains(&"Villa_BOQ.xlsx".to_string()), "{names:?}");
    let hidden_files = std::fs::read_dir(&hidden).unwrap().count();
    assert_eq!(hidden_files, 0, "nothing in the hidden folder");

    // For the report: the files and what a client reads in them (QIYAS_W3_EVIDENCE=<folder>).
    if let Ok(folder) = std::env::var("QIYAS_W3_EVIDENCE") {
        std::fs::create_dir_all(&folder).unwrap();
        for name in &names {
            std::fs::copy(
                documents.join(name),
                std::path::Path::new(&folder).join(name),
            )
            .unwrap();
        }
        let mut text = String::new();
        text.push_str("ROOM LABELS (Model)\n");
        for t in model.iter().filter(|t| t.contains("المساحة")) {
            text.push_str(&format!("  {t}\n"));
        }
        text.push_str("\nSHEET A-101 (title block, notes, schedules)\n");
        for t in &sheet {
            text.push_str(&format!("  {}\n", t.replace('\n', " | ")));
        }
        text.push_str("\nBOQ items\n");
        for i in &measured.items {
            text.push_str(&format!(
                "  {} | {} | {} {}\n",
                i.code, i.description_en, i.quantity, i.unit
            ));
        }
        std::fs::write(std::path::Path::new(&folder).join("evidence.txt"), text).unwrap();
    }
}

// ---- W6: the blind second reading, through the production agent ----

/// A main script, and a second reader that answers by item: the 1000 mm door is read as 2 where
/// the first reading is 1 (a seeded disagreement); everything else is read as the first was.
struct Blind {
    main: Mutex<VecDeque<Calls>>,
    reader_prompts: Arc<Mutex<Vec<String>>>,
    reader_tools: Arc<Mutex<Vec<String>>>,
}

impl Provider for Blind {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        let result = if request.system.contains("second reader") {
            let prompt = match &request.messages[0].content[0] {
                ContentBlock::Text { text } => text.clone(),
                other => panic!("the reader starts with words: {other:?}"),
            };
            if request.messages.len() == 1 {
                self.reader_prompts.lock().unwrap().push(prompt);
                for tool in &request.tools {
                    self.reader_tools
                        .lock()
                        .unwrap()
                        .push(tool.input_schema["properties"]["op"]["enum"].to_string());
                }
                // A fresh reader: nothing but the item. It looks at the drawing's texts, then
                // also tries to read the take-off (refused), then answers.
                TurnResult {
                    content: vec![
                        ContentBlock::ToolUse {
                            id: "r1".into(),
                            name: READ.into(),
                            input: json!({"op":"query","args":{"kind":"texts"}}),
                        },
                        ContentBlock::ToolUse {
                            id: "r2".into(),
                            name: READ.into(),
                            input: json!({"op":"state","args":{}}),
                        },
                    ],
                    stop_reason: StopReason::ToolUse,
                    usage: Default::default(),
                }
            } else {
                let first_line = prompt.lines().next().unwrap_or_default().to_string();
                let quantity = if first_line.contains("1000x") || first_line.contains("900x") {
                    2.0
                } else {
                    1.0
                };
                // The refused read comes back as an error result, not as a figure.
                let refused = request.messages.iter().any(|m| {
                    m.content.iter().any(|b| matches!(b,
                        ContentBlock::ToolResult { is_error: true, content, .. }
                        if content.iter().any(|p| matches!(p, qiyas_ai::ResultPart::Text(t) if t.contains("Not allowed")))))
                });
                assert!(
                    refused,
                    "the state read must be refused in a second reading"
                );
                TurnResult {
                    content: vec![ContentBlock::Text {
                        text: format!("{{\"quantity\": {quantity}}}"),
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: Default::default(),
                }
            }
        } else {
            match self.main.lock().unwrap().pop_front() {
                Some(calls) => TurnResult {
                    content: calls
                        .into_iter()
                        .enumerate()
                        .map(|(n, (name, input))| ContentBlock::ToolUse {
                            id: format!("main_{}_{n}", request.messages.len()),
                            name: name.into(),
                            input,
                        })
                        .collect(),
                    stop_reason: StopReason::ToolUse,
                    usage: Default::default(),
                },
                None => TurnResult {
                    content: vec![ContentBlock::Text {
                        text: "Done.".into(),
                    }],
                    stop_reason: StopReason::EndTurn,
                    usage: Default::default(),
                },
            }
        };
        Box::pin(async move { Ok(result) })
    }
}

fn blind_session() -> (tempfile::TempDir, Shared) {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("w6.dwg"), 6);
    let cite = json!({"source":"Test method","clause":"1"});
    let count = |id: &str, element: &str| {
        json!({"id":id,"kind":"count","basis":"geometric","element_type":element,"unit":"pcs",
            "terms":[{"label":"Count","operation":"add","factors":["pieces"],"citation":cite}],
            "citation":cite})
    };
    for command in [
        json!({"type":"walls","runs":[{"points":[[0,0],[20,0]]}],"thickness":0.2,
            "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"},
                        {"run":0,"at":5,"width":0.9,"kind":"door"},
                        {"run":0,"at":8,"width":1.0,"kind":"door"},
                        {"run":0,"at":12,"width":1.2,"kind":"window"}]}),
        json!({"type":"measurement","kind":"set_mappings","mappings":[
            {"rule":"doors","code":"A-D01","description_en":"Doors","description_ar":"أبواب","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
            {"rule":"windows","code":"A-W01","description_en":"Windows","description_ar":"شبابيك","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
            "version":1,"name":"Test method","title":"Test","based_on":"test","source_note":"test",
            "rules":[count("doors","Legacy door"),count("windows","Legacy window")]}}),
    ] {
        session
            .run_input(&command, qiyas_core::Source::User)
            .unwrap();
    }
    (dir, Arc::new(Mutex::new(Some(session))))
}

#[tokio::test]
async fn a_seeded_disagreement_never_reaches_the_boq_and_appears_as_a_query() {
    let (_dir, session) = blind_session();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let tools = Arc::new(Mutex::new(Vec::new()));
    let provider = Blind {
        main: Mutex::new(vec![vec![exec("compare", json!({}))]].into()),
        reader_prompts: prompts.clone(),
        reader_tools: tools.clone(),
    };
    let mut agent = Agent::new(
        Box::new(provider),
        session.clone(),
        AgentSettings::default(),
    )
    .with_second_reading(true);
    let (tx, mut rx) = mpsc::channel(10_000);
    agent
        .run_turn("compare".to_string(), tx, CancellationToken::new())
        .await
        .unwrap();
    while let Ok(_event) = rx.try_recv() {}

    // Three items were read, each by a fresh reader that saw no quantity and had only reads.
    {
        let prompts = prompts.lock().unwrap();
        assert_eq!(prompts.len(), 3, "{prompts:?}");
        for p in prompts.iter() {
            assert!(!p.to_lowercase().contains("quantity"), "{p}");
            assert!(p.starts_with("Item "), "{p}");
        }
        for t in tools.lock().unwrap().iter() {
            assert!(!t.contains("records") && !t.contains("state"), "{t}");
        }

        let guard = session.lock().unwrap();
        let s = guard.as_ref().unwrap();
        let report = s.measurement_report().unwrap();
        let differing: Vec<_> = report.items.iter().filter(|i| i.second_differs()).collect();
        assert_eq!(differing.len(), 1, "{:?}", report.items);
        assert!(differing[0].code.contains("1000x"));
        assert!(report
            .items
            .iter()
            .filter(|i| !i.second_differs())
            .all(|i| i.second_reading.is_some()));

        // It is not a BOQ row; it is a query with both readings.
        let boq = qiyas_core::deliverables::drawing(
            s,
            qiyas_core::deliverables::Kind::Boq,
            qiyas_core::deliverables::Language::En,
            None,
        )
        .unwrap();
        assert!(boq
            .rows
            .iter()
            .all(|r| r["code"] != differing[0].code.as_str()));
        assert_eq!(boq.rows.len(), 2);
        let query = s
            .conflicts
            .iter()
            .find(|c| c.hash_key.contains("second_reading"))
            .expect("the disagreement is a query");
        assert_eq!((query.expected, query.found), (Some(1.0), Some(2.0)));
    }
    // A second pass in the same conversation has nothing left to read.
    let again = qiyas_ai::blind_reader::run_pass(
        &Blind {
            main: Mutex::new(VecDeque::new()),
            reader_prompts: Arc::default(),
            reader_tools: Arc::default(),
        },
        &session,
        &AgentSettings::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        again.read.is_empty() && again.failed.is_empty() && again.left == 0,
        "{again:?}"
    );
}

#[tokio::test]
async fn a_reading_pass_creates_queries_without_a_later_compare() {
    let (_dir, session) = blind_session();
    let pass = qiyas_ai::blind_reader::run_pass(
        &Blind {
            main: Mutex::default(),
            reader_prompts: Arc::default(),
            reader_tools: Arc::default(),
        },
        &session,
        &AgentSettings::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(pass.differing().count(), 1);
    let guard = session.lock().unwrap();
    let s = guard.as_ref().unwrap();
    let queries: Vec<_> = s
        .conflicts
        .iter()
        .filter(|c| c.hash_key.contains("second_reading"))
        .collect();
    assert_eq!(queries.len(), 1);
    assert!(queries[0].query.is_some());
    assert_eq!(
        (queries[0].expected, queries[0].found),
        (Some(1.0), Some(2.0))
    );
}

struct ChangedDuringReading {
    session: Shared,
}

struct FailedReader;

impl Provider for FailedReader {
    fn stream_turn<'a>(
        &'a self,
        _: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async { Err(AiError::Network("offline test".into())) })
    }
}

#[tokio::test]
async fn a_failed_reader_counts_every_unattempted_item() {
    let (_dir, session) = blind_session();
    let pass = qiyas_ai::blind_reader::run_pass(
        &FailedReader,
        &session,
        &AgentSettings::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(pass.read.is_empty());
    assert_eq!(pass.failed.len(), 1);
    assert_eq!(pass.left, 2);
}

impl Provider for ChangedDuringReading {
    fn stream_turn<'a>(
        &'a self,
        _: TurnRequest,
        _: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        let mut guard = self.session.lock().unwrap();
        let session = guard.as_mut().unwrap();
        let y = 5.0 * session.wall_groups(None).unwrap().len() as f64;
        session
            .run_input(
                &json!({"type":"walls","runs":[{"points":[[0,y],[20,y]]}],"thickness":0.2,
                "openings":[{"run":0,"at":2,"width":0.9,"kind":"door"}]}),
                qiyas_core::Source::User,
            )
            .unwrap();
        Box::pin(async {
            Ok(TurnResult {
                content: vec![ContentBlock::Text {
                    text: "{\"quantity\":2}".into(),
                }],
                stop_reason: StopReason::EndTurn,
                usage: Default::default(),
            })
        })
    }
}

#[tokio::test]
async fn a_quantity_changed_during_the_reader_call_stays_unchecked() {
    let (_dir, session) = blind_session();
    let pass = qiyas_ai::blind_reader::run_pass(
        &ChangedDuringReading {
            session: session.clone(),
        },
        &session,
        &AgentSettings::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(pass.failed.iter().any(|code| code.contains("900x")));
    let guard = session.lock().unwrap();
    let report = guard.as_ref().unwrap().measurement_report().unwrap();
    let changed = report
        .items
        .iter()
        .find(|i| i.code.contains("900x"))
        .unwrap();
    assert!(changed.quantity > 2.0);
    assert!(changed.second_reading.is_none());
}

#[tokio::test]
async fn a_model_cannot_write_a_second_reading_through_the_tools() {
    let dir = tempfile::tempdir().unwrap();
    let session: Shared = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("x.dwg"),
        6,
    ))));
    let mut guard = session.lock().unwrap();
    let out = qiyas_ai::surface::run_session(
        guard.as_mut().unwrap(),
        EXECUTE,
        "run",
        &json!({"command":{"type":"second_reading","kind":"record","code":"A","quantity":1.0}}),
        qiyas_core::Source::Ai,
    );
    assert!(out.is_err(), "an internal command is refused: {out:?}");
}
