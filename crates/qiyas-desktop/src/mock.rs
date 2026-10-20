//! A canned AI provider for trying the assistant panel without a key or the network:
//! `QIYAS_AI_MOCK=1` in a debug build. It says a sentence, measures the area of the first polyline
//! on layer A-WALL, and closes with another sentence. With a selection in the message's `[state]`
//! block it looks at the screen (`qiyas_capture`) and selects what was selected
//! (`qiyas_execute select`) instead. Not compiled into release builds.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use qiyas_ai::provider::{BoxFuture, Sink};
use qiyas_ai::{
    AiError, ContentBlock, Provider, Role, StopReason, StreamEvent, TurnRequest, TurnResult, Usage,
};
use qiyas_core::{EntityFilter, Session};
use serde_json::json;

pub struct Mock {
    session: Arc<Mutex<Option<Session>>>,
}

impl Mock {
    pub fn new(session: Arc<Mutex<Option<Session>>>) -> Mock {
        Mock { session }
    }

    /// The first polyline on A-WALL on the sheet shown (else its first entity), or 0 when there
    /// is none; the tool then reports the error.
    fn first_wall(&self) -> u64 {
        let guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(session) = guard.as_ref() else {
            return 0;
        };
        let first = |kind: Option<&str>| {
            let filter = EntityFilter {
                layer: Some("A-WALL".into()),
                kind: kind.map(str::to_string),
                limit: 1,
                ..EntityFilter::default()
            };
            session
                .find_entities(&filter)
                .rows
                .first()
                .map(|r| r.handle)
        };
        first(Some("LwPolyline"))
            .or_else(|| first(None))
            .unwrap_or(0)
    }
}

/// Streams `text` a few words at a time, like a real answer.
async fn stream(text: &str, sink: Sink<'_>) {
    for word in text.split_inclusive(' ') {
        sink(StreamEvent::TextDelta(word.to_string()));
        tokio::time::sleep(Duration::from_millis(60)).await;
    }
}

/// The handles the `[state]` block of the user's last message lists as selected or marked.
fn pointed_at(req: &TurnRequest) -> Vec<u64> {
    let Some(last) = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User && !m.has_tool_results())
    else {
        return Vec::new();
    };
    last.content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } if text.starts_with("[state]") => Some(text),
            _ => None,
        })
        .flat_map(|text| text.lines())
        .filter_map(|line| {
            line.strip_prefix("- handle ")?
                .split(' ')
                .next()?
                .parse()
                .ok()
        })
        .collect()
}

impl Provider for Mock {
    fn stream_turn<'a>(
        &'a self,
        req: TurnRequest,
        sink: Sink<'a>,
    ) -> BoxFuture<'a, Result<TurnResult, AiError>> {
        Box::pin(async move {
            let answered = req.messages.last().is_some_and(|m| m.has_tool_results());
            let pointed = pointed_at(&req);
            if answered && !pointed.is_empty() {
                let text = format!(
                    "I looked at your screen and selected the {} entities you pointed at.",
                    pointed.len()
                );
                stream(&text, sink).await;
                return Ok(TurnResult {
                    content: vec![ContentBlock::Text { text }],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                });
            }
            if !pointed.is_empty() {
                let text = "Let me look at what you see.";
                stream(text, sink).await;
                return Ok(TurnResult {
                    content: vec![
                        ContentBlock::Text { text: text.into() },
                        ContentBlock::ToolUse {
                            id: "mock_screen".into(),
                            name: "qiyas_capture".into(),
                            input: json!({ "source": "screen", "args": "{}" }),
                        },
                        ContentBlock::ToolUse {
                            id: "mock_select".into(),
                            name: "qiyas_execute".into(),
                            input: json!({ "op": "select", "args": json!({ "handles": pointed, "zoom": true }).to_string() }),
                        },
                    ],
                    stop_reason: StopReason::ToolUse,
                    usage: Usage::default(),
                });
            }
            if answered {
                let text = "Done. The **wall area** is in the take-off:\n\n- source: `ai`\n- \
                            press Undo on the card to take it out";
                stream(text, sink).await;
                return Ok(TurnResult {
                    content: vec![ContentBlock::Text { text: text.into() }],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                });
            }
            let text = "I'll measure the first wall on A-WALL.";
            stream(text, sink).await;
            Ok(TurnResult {
                content: vec![
                    ContentBlock::Text { text: text.into() },
                    ContentBlock::ToolUse {
                        id: "mock_1".into(),
                        name: "qiyas_execute".into(),
                        input: json!({ "op": "run", "args": json!({ "command": { "type": "measure_entity", "handle": self.first_wall() } }).to_string() }),
                    },
                ],
                stop_reason: StopReason::ToolUse,
                usage: Usage::default(),
            })
        })
    }
}
