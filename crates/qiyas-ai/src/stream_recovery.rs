//! Recovery at a terminal output boundary. Transport retries stay with the provider.
use crate::{ContentBlock, StopReason, TurnResult};

pub(crate) fn reason(value: Option<&str>) -> String {
    value
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .unwrap_or("unknown")
        .into()
}

pub(crate) fn stop(provider: &str, reason: &str) -> StopReason {
    match reason {
        "max_tokens" | "max_output_tokens" | "length" => StopReason::MaxTokens,
        _ => StopReason::Incomplete {
            provider: provider.into(),
            reason: self::reason(Some(reason)),
        },
    }
}

pub(crate) fn continuation(arabic: bool) -> String {
    if arabic {
        "توقفت الإجابة عند حد طول المخرجات. تابع من العمل المحفوظ ونتائج الأدوات أعلاه. لا تكرر العمليات المكتملة؛ أعد إرسال أي عملية غير مكتملة كاملة.".into()
    } else {
        "The response reached its output limit. Continue from the retained output and tool results above. Do not repeat completed operations; resend any unfinished call in full.".into()
    }
}

/// The first attempt has not executed any tools. Match repeated calls by input, not new IDs.
/// Each occurrence is matched once so intentional repeated calls remain repeated.
pub(crate) fn retain(first: Vec<ContentBlock>, retry: &mut TurnResult) {
    let mut matched = vec![false; retry.content.len()];
    let mut retained = Vec::new();
    for block in first {
        let duplicate = retry.content.iter().enumerate().position(|(i, other)| {
            !matched[i]
                && match (&block, other) {
                    (
                        ContentBlock::ToolUse {
                            name: a, input: x, ..
                        },
                        ContentBlock::ToolUse {
                            name: b, input: y, ..
                        },
                    ) => a == b && x == y,
                    (ContentBlock::Text { text: a }, ContentBlock::Text { text: b }) => {
                        b.starts_with(a)
                    }
                    _ => &block == other,
                }
        });
        if let Some(index) = duplicate {
            matched[index] = true;
        } else {
            retained.push(block);
        }
    }
    retained.append(&mut retry.content);
    retry.content = retained;
    if retry.stop_reason == StopReason::EndTurn
        && retry
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    {
        retry.stop_reason = StopReason::ToolUse;
    }
}
