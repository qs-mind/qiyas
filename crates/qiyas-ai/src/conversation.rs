//! Portable conversation memory. Pictures are deliberately removed before it reaches disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prompt::Lang;
use crate::{AgentEvent, ContentBlock, Message, ResultPart};

pub const PICTURE_REMOVED: &str = "[picture removed]";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// Unix milliseconds, shared with the window's dates.
    pub created: u64,
    pub updated: u64,
    pub drawing: Option<PathBuf>,
    pub provider: String,
    pub model: String,
    pub language: Lang,
    pub system: Option<String>,
    pub history: Vec<Message>,
    pub image_bytes: usize,
    pub provider_state: Value,
    pub transcript: Vec<TranscriptEntry>,
    /// Error attribution must survive an interruption in the middle of a phase too.
    #[serde(default)]
    pub phase_state: PhaseState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseState {
    pub drawing: Option<PathBuf>,
    pub baseline_errors: Option<std::collections::BTreeSet<String>>,
    pub touched: std::collections::BTreeSet<u64>,
    /// The user's checkable brief survives context trimming and a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_check: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    User {
        text: String,
        #[serde(default)]
        pointed: Vec<String>,
        #[serde(default)]
        images: Vec<String>,
    },
    Event {
        event: AgentEvent,
    },
}

impl Conversation {
    /// Every disk writer calls this too, so a caller cannot accidentally persist pictures.
    pub fn without_pictures(mut self) -> Self {
        for message in &mut self.history {
            for block in &mut message.content {
                match block {
                    ContentBlock::Image { .. } => {
                        *block = ContentBlock::Text {
                            text: PICTURE_REMOVED.into(),
                        }
                    }
                    ContentBlock::ToolResult { content, .. } => {
                        for part in content {
                            if matches!(part, ResultPart::Png(_)) {
                                *part = ResultPart::Text(PICTURE_REMOVED.into());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut transcript = Vec::with_capacity(self.transcript.len());
        for mut entry in self.transcript {
            if let TranscriptEntry::User { images, .. } = &mut entry {
                images
                    .iter_mut()
                    .for_each(|image| *image = PICTURE_REMOVED.into());
            }
            append(&mut transcript, entry);
        }
        self.transcript = transcript;
        self
    }

    pub fn turns(&self) -> usize {
        self.transcript
            .iter()
            .filter(|entry| matches!(entry, TranscriptEntry::User { .. }))
            .count()
    }
}

/// Delta merging preserves the exact visible text and keeps long streams small on disk.
pub(crate) fn append(transcript: &mut Vec<TranscriptEntry>, entry: TranscriptEntry) {
    if matches!(
        &entry,
        TranscriptEntry::Event {
            event: AgentEvent::Retrying {
                discard_partial: true,
                ..
            }
        }
    ) {
        // Cache totals belong to the turn, even when its partial answer is discarded.
        let mut at = transcript.len();
        while at > 0 {
            match &transcript[at - 1] {
                TranscriptEntry::Event {
                    event: AgentEvent::CacheUsage { .. },
                } => at -= 1,
                TranscriptEntry::Event {
                    event: AgentEvent::Text { .. } | AgentEvent::Thinking { .. },
                } => {
                    at -= 1;
                    transcript.remove(at);
                }
                _ => break,
            }
        }
    }
    match (transcript.last_mut(), &entry) {
        (
            Some(TranscriptEntry::Event {
                event: AgentEvent::Text { text: previous },
            }),
            TranscriptEntry::Event {
                event: AgentEvent::Text { text },
            },
        )
        | (
            Some(TranscriptEntry::Event {
                event: AgentEvent::Thinking { text: previous },
            }),
            TranscriptEntry::Event {
                event: AgentEvent::Thinking { text },
            },
        ) => previous.push_str(text),
        _ => transcript.push(entry),
    }
}

/// Checkpoints happen at step boundaries; never copy megabytes of image data first.
pub(crate) fn history_without_pictures(history: &[Message]) -> Vec<Message> {
    history
        .iter()
        .map(|message| Message {
            role: message.role,
            content: message
                .content
                .iter()
                .map(|block| match block {
                    ContentBlock::Image { .. } => ContentBlock::Text {
                        text: PICTURE_REMOVED.into(),
                    },
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => ContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        is_error: *is_error,
                        content: content
                            .iter()
                            .map(|part| match part {
                                ResultPart::Png(_) => ResultPart::Text(PICTURE_REMOVED.into()),
                                other => other.clone(),
                            })
                            .collect(),
                    },
                    other => other.clone(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;
    use serde_json::json;

    #[test]
    fn all_blocks_round_trip_without_changing_signed_content() {
        let blocks = vec![
            ContentBlock::Text {
                text: "a\r\nb\t".into(),
            },
            ContentBlock::Thinking {
                thinking: "thought\r\n\t🌍".into(),
                signature: "opaque+/=\r\n".into(),
            },
            ContentBlock::Raw(
                json!({"type":"redacted_thinking","data":"opaque+/=\r\n","nested":[1,true,null]}),
            ),
            ContentBlock::Image {
                media_type: "image/png".into(),
                data: "QUJD".into(),
            },
            ContentBlock::ToolUse {
                id: "c1".into(),
                name: "qiyas_read".into(),
                input: json!({"op":"state"}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "c1".into(),
                content: vec![
                    ResultPart::Text("ok".into()),
                    ResultPart::Png("QUJD".into()),
                ],
                is_error: false,
            },
        ];
        let message = Message {
            role: Role::Assistant,
            content: blocks,
        };
        let encoded = serde_json::to_vec(&message).unwrap();
        let decoded: Message = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, message);
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
    }
}
