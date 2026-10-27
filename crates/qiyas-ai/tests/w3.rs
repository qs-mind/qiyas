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
    let replies = vec![
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
        // The BOQ: a static "various sizes" description, as the founder's run wrote it.
        Reply::Calls(vec![
            run(
                json!({"type":"measurement","kind":"set_mappings","mappings":[
                {"rule":"doors","code":"A-D01","description_en":"Doors, various sizes","description_ar":"أبواب بمقاسات متنوعة","section":"Openings","trade":"Doors","unit":"pcs","citation":cite},
                {"rule":"windows","code":"A-W01","description_en":"Windows, various sizes","description_ar":"شبابيك بمقاسات متنوعة","section":"Openings","trade":"Windows","unit":"pcs","citation":cite}]}),
            ),
            run(
                json!({"type":"measurement","kind":"set_method","scope":"settings","method":{
                "version":1,"name":"Villa geometric quantities","title":"Villa","based_on":"drawing","source_note":"Villa assumptions",
                "rules":[count("doors","Legacy door"),count("windows","Legacy window")]}}),
            ),
        ]),
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
        names.iter().any(|n| n.ends_with(".pdf") && n.contains("A-101")),
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
