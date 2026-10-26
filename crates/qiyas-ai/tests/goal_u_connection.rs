//! A dropped answer keeps completed drawing work when the engineer presses Continue.
use qiyas_ai::{
    Agent, AgentEvent, AgentSettings, AiError, ContentBlock, Provider, StopReason, StreamEvent,
    TurnRequest, TurnResult,
};
use qiyas_core::Session;
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct DropThenContinue(AtomicUsize);
impl Provider for DropThenContinue {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        sink: qiyas_ai::provider::Sink<'a>,
    ) -> qiyas_ai::provider::BoxFuture<'a, Result<TurnResult, AiError>> {
        let step = self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if step == 1 {
                sink(StreamEvent::TextDelta(
                    "The first line is ready. Next".into(),
                ));
                return Err(AiError::Stream(
                    "connection closed after bounded retries".into(),
                ));
            }
            if step >= 2 {
                let completed = request.messages.iter().flat_map(|m| &m.content).filter(|b|
                    matches!(b, ContentBlock::ToolResult { tool_use_id, is_error: false, .. } if tool_use_id == "first-edit")
                ).count();
                assert_eq!(
                    completed, 1,
                    "Continue must retain exactly one completed first step"
                );
            }
            let content = match step {
                0 | 2 => vec![ContentBlock::ToolUse {
                    id: if step == 0 {
                        "first-edit"
                    } else {
                        "second-edit"
                    }
                    .into(),
                    name: "qiyas_execute".into(),
                    input: json!({"op":"run","args":{"command":{"type":"draw","kind":"line","a":[0,step],"b":[3,step]}}}),
                }],
                3 => vec![ContentBlock::Text {
                    text: "Both lines are ready.".into(),
                }],
                _ => panic!("unexpected provider request {step}"),
            };
            Ok(TurnResult {
                content,
                stop_reason: if step == 3 {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                },
                usage: Default::default(),
            })
        })
    }
}

#[tokio::test]
async fn u16_continue_after_a_drop_finishes_without_repeating_the_completed_edit() {
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Mutex::new(Some(Session::new_blank(
        &dir.path().join("drawing.dxf"),
        6,
    ))));
    let mut agent = Agent::new(
        Box::new(DropThenContinue(AtomicUsize::new(0))),
        session.clone(),
        AgentSettings::default(),
    );
    let (tx, mut events) = mpsc::channel(1000);
    let error = agent
        .run_turn(
            "Draw two lines, one at a time.".into(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "stream");
    let count = || {
        session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .count()
    };
    assert_eq!(count(), 1);

    let (tx, mut continued) = mpsc::channel(1000);
    agent
        .run_turn(
            "Continue from the last completed step. Keep the finished work.".into(),
            tx,
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(count(), 2);
    let mut all = vec![];
    while let Ok(event) = events.try_recv() {
        all.push(event);
    }
    while let Ok(event) = continued.try_recv() {
        all.push(event);
    }
    let calls: Vec<_> = all
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolCall { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["first-edit", "second-edit"]);
    let mut guard = session.lock().unwrap();
    let s = guard.as_mut().unwrap();

    assert!(s.undo());
    assert_eq!(s.drawing.doc.entities_in_block("*Model_Space").count(), 1);
}
