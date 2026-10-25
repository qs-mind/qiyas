//! Window-owned presentation is mirrored in Rust so a webview reload can reattach mid-turn.
use qiyas_ai::conversation::TranscriptEntry;
use qiyas_ai::AgentEvent;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;

const KEEP_EVENTS: usize = 512;

#[derive(Clone, Serialize)]
pub struct WindowEvent {
    pub workspace: u32,
    pub cursor: u64,
    pub event: Value,
}
#[derive(Default)]
pub struct WorkspaceView {
    pub cursor: u64,
    pub transcript: Vec<TranscriptEntry>,
    pub status: &'static str,
    pub error: Option<String>,
    pub away: Option<String>,
    events: VecDeque<WindowEvent>,
}
impl WorkspaceView {
    pub fn record(&mut self, workspace: u32, event: Value) -> WindowEvent {
        self.cursor += 1;
        let event = WindowEvent {
            workspace,
            cursor: self.cursor,
            event,
        };
        self.events.push_back(event.clone());
        while self.events.len() > KEEP_EVENTS {
            self.events.pop_front();
        }
        event
    }
    pub fn agent_event(&mut self, workspace: u32, event: AgentEvent) -> WindowEvent {
        if matches!(event, AgentEvent::AwaitingApproval { .. }) {
            self.status = "waiting";
        }
        if matches!(
            event,
            AgentEvent::Retrying {
                discard_partial: true,
                ..
            }
        ) {
            while matches!(
                self.transcript.last(),
                Some(TranscriptEntry::Event {
                    event: AgentEvent::Text { .. } | AgentEvent::Thinking { .. }
                })
            ) {
                self.transcript.pop();
            }
        }
        // Streaming deltas stay compact, while the replay journal retains their cursors.
        let merged = match (self.transcript.last_mut(), &event) {
            (
                Some(TranscriptEntry::Event {
                    event: AgentEvent::Text { text: old },
                }),
                AgentEvent::Text { text },
            )
            | (
                Some(TranscriptEntry::Event {
                    event: AgentEvent::Thinking { text: old },
                }),
                AgentEvent::Thinking { text },
            ) => {
                old.push_str(text);
                true
            }
            _ => false,
        };
        if !merged {
            self.transcript.push(TranscriptEntry::Event {
                event: event.clone(),
            });
        }
        self.record(workspace, json!(event))
    }
    pub fn snapshot(&self, workspace: u32) -> Value {
        json!({"workspace":workspace,"cursor":self.cursor,"transcript":self.transcript,
            "status":self.status,"error":self.error,"away":self.away})
    }
    pub fn since(&self, after: u64) -> Value {
        let oldest = self.events.front().map_or(self.cursor + 1, |e| e.cursor);
        json!({"cursor":self.cursor,"resync":after > self.cursor || after.saturating_add(1) < oldest,
            "events":self.events.iter().filter(|e| e.cursor > after).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_loss_requires_resync_and_snapshot_keeps_running_cards() {
        let mut view = WorkspaceView::default();
        for _ in 0..600 {
            view.agent_event(7, AgentEvent::Text { text: "a".into() });
        }
        assert_eq!(view.transcript.len(), 1);
        assert_eq!(view.since(0)["resync"], true);
        assert_eq!(view.since(599)["events"].as_array().unwrap().len(), 1);
        assert_eq!(view.since(600)["resync"], false);
        assert_eq!(view.since(601)["resync"], true);
        assert_eq!(
            view.snapshot(7)["transcript"][0]["event"]["text"]
                .as_str()
                .unwrap()
                .len(),
            600
        );
    }
}
