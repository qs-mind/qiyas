//! A dropped answer retries within the turn without repeating completed drawing work.
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

struct DropThenContinue(Arc<AtomicUsize>);
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
                    "connection closed before the answer finished".into(),
                ));
            }
            if step >= 2 {
                let completed = request.messages.iter().flat_map(|m| &m.content).filter(|b|
                    matches!(b, ContentBlock::ToolResult { tool_use_id, is_error: false, .. } if tool_use_id == "first-edit")
                ).count();
                assert_eq!(
                    completed, 1,
                    "Retry must retain exactly one completed first step"
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
    let requests = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent::new(
        Box::new(DropThenContinue(requests.clone())),
        session.clone(),
        AgentSettings::default(),
    );
    let (tx, mut events) = mpsc::channel(1000);
    qiyas_ai::provider::with_retry_delay(
        std::time::Duration::ZERO,
        agent.run_turn(
            "Draw two lines, one at a time.".into(),
            tx,
            CancellationToken::new(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 4);
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
    assert_eq!(count(), 2);
    let mut all = vec![];
    while let Ok(event) = events.try_recv() {
        all.push(event);
    }
    assert_eq!(
        all.iter()
            .filter(|e| matches!(
                e,
                AgentEvent::Retrying {
                    seconds: 5,
                    discard_partial: true,
                    ..
                }
            ))
            .count(),
        1
    );
    assert!(all.iter().any(|e| matches!(e, AgentEvent::Done { .. })));
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
