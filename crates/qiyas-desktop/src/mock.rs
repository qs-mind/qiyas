//! A canned AI provider for trying the assistant panel without a key or the network:
//! `QIYAS_AI_MOCK=1` in a debug build. It says a sentence, measures the area of the first polyline
//! on layer A-WALL, and closes with another sentence. Ask "draw a demo" or "ارسم" to draw an
//! outline and then its partitions in two steps. With a selection in the message's `[state]`
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

    fn drawing_turn(&self, req: &TurnRequest) -> Option<TurnResult> {
        let user = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User && !m.has_tool_results())?;
        let drawing = user.content.iter().any(|block| {
            matches!(block, ContentBlock::Text { text }
                if !text.starts_with("[state]")
                    && (text.to_lowercase().contains("draw") || text.contains("رسم")))
        });
        if !drawing {
            return None;
        }
        let result = req.messages.last().and_then(|message| {
            message.content.iter().find_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    is_error,
                    ..
                } => Some((tool_use_id.as_str(), *is_error)),
                _ => None,
            })
        });
        let (text, step) = match result {
            Some((_, true)) => ("The drawing step failed. Check the tool card.", None),
            Some(("mock_draw_1", false)) => {
                ("Now I'll add two partitions inside the outline.", Some(1))
            }
            Some(_) => (
                "Done. The outline and partitions are two undoable steps.",
                None,
            ),
            None => (
                "I'll draw the outline first, then add its partitions.",
                Some(0),
            ),
        };
        let mut content = vec![ContentBlock::Text { text: text.into() }];
        if let Some(step) = step {
            let metres = self
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .map_or(1.0, |session| session.drawing.units.to_metres());
            let lines = if step == 0 {
                vec![
                    ([0.0, 0.0], [6.0, 0.0]),
                    ([6.0, 0.0], [6.0, 4.0]),
                    ([6.0, 4.0], [0.0, 4.0]),
                    ([0.0, 4.0], [0.0, 0.0]),
                ]
            } else {
                vec![([3.0, 0.0], [3.0, 4.0]), ([3.0, 2.0], [6.0, 2.0])]
            };
            let commands: Vec<_> = lines
                .into_iter()
                .map(|(a, b)| {
                    json!({
                        "type": "draw", "kind": "line",
                        "a": a.map(|v| v / metres), "b": b.map(|v| v / metres)
                    })
                })
                .collect();
            content.push(ContentBlock::ToolUse {
                id: format!("mock_draw_{}", step + 1),
                name: "qiyas_execute".into(),
                input: json!({
                    "op": "batch",
                    "args": json!({ "commands": commands, "layer": "MIZAN-DEMO" }).to_string()
                }),
            });
        }
        Some(TurnResult {
            content,
            stop_reason: if step.is_some() {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            },
            usage: Usage::default(),
        })
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
            let session = self.session.clone();
            let drawing =
                tokio::task::spawn_blocking(move || Mock::new(session).drawing_turn(&req))
                    .await
                    .map_err(|error| {
                        AiError::Stream(format!("Mock drawing read failed: {error}"))
                    })?;
            if let Some(turn) = drawing {
                if let ContentBlock::Text { text } = &turn.content[0] {
                    stream(text, sink).await;
                }
                return Ok(turn);
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
            let session = self.session.clone();
            let wall = tokio::task::spawn_blocking(move || Mock::new(session).first_wall())
                .await
                .map_err(|error| AiError::Stream(format!("Mock drawing read failed: {error}")))?;
            Ok(TurnResult {
                content: vec![
                    ContentBlock::Text { text: text.into() },
                    ContentBlock::ToolUse {
                        id: "mock_1".into(),
                        name: "qiyas_execute".into(),
                        input: json!({ "op": "run", "args": json!({ "command": { "type": "measure_entity", "handle": wall } }).to_string() }),
                    },
                ],
                stop_reason: StopReason::ToolUse,
                usage: Usage::default(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use qiyas_ai::{surface, Message, ResultPart, ToolOutput};
    use qiyas_core::Source;

    use super::*;

    #[test]
    fn drawing_demo_returns_two_reveals_and_two_undo_steps() {
        for units in [4, 6] {
            let session = Arc::new(Mutex::new(Some(Session::new_blank(
                Path::new("mock.dxf"),
                units,
            ))));
            let mock = Mock::new(session.clone());
            let mut req = TurnRequest {
                system: String::new(),
                messages: vec![Message::user(vec![ContentBlock::Text {
                    text: "draw a demo".into(),
                }])],
                tools: Vec::new(),
                model: "mock".into(),
                effort: "low".into(),
                max_tokens: 1024,
            };
            for count in [4, 2] {
                let turn = mock.drawing_turn(&req).unwrap();
                assert_eq!(turn.stop_reason, StopReason::ToolUse);
                let ContentBlock::ToolUse { id, name, input } = &turn.content[1] else {
                    panic!("a drawing tool call");
                };
                let surface::Route::Session { tool, op, args } =
                    surface::route(name, input).unwrap()
                else {
                    panic!("a session operation");
                };
                let output = surface::run_session(
                    session.lock().unwrap().as_mut().unwrap(),
                    &tool,
                    &op,
                    &args,
                    Source::Ai,
                )
                .unwrap();
                let ToolOutput::Json(value) = output else {
                    panic!("a drawing result");
                };
                assert_eq!(value["ui"]["action"], "reveal");
                assert_eq!(value["ui"]["handles"].as_array().unwrap().len(), count);
                let result = ContentBlock::ToolResult {
                    tool_use_id: id.clone(),
                    content: vec![ResultPart::Text(value.to_string())],
                    is_error: false,
                };
                req.messages.push(Message::assistant(turn.content));
                req.messages.push(Message::user(vec![result]));
            }
            assert_eq!(
                mock.drawing_turn(&req).unwrap().stop_reason,
                StopReason::EndTurn
            );
            let mut guard = session.lock().unwrap();
            let session = guard.as_mut().unwrap();
            let count = |session: &Session| {
                session
                    .find_entities(&EntityFilter {
                        layer: Some("MIZAN-DEMO".into()),
                        ..EntityFilter::default()
                    })
                    .rows
                    .len()
            };
            assert_eq!(count(session), 6);
            assert!(session.undo());
            assert_eq!(count(session), 4);
            assert!(session.undo());
            assert_eq!(count(session), 0);
        }
    }
}
