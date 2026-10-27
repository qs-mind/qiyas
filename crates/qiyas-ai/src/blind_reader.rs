//! The blind second reading (W6). Before a generated quantity reaches the BOQ, a fresh reader
//! with no conversation history and read-only tools measures the same item again. It sees the
//! item's description, unit and rule, never the first quantity. Code, not a model, compares the
//! two (`qiyas_core::second_reading`): this module only asks the reader and records its number.
//!
//! What keeps it blind: the reader's tools are a short list of exact reads of the drawing
//! (`ALLOWED_OPS`); a call that names a Qiyas result (the measurement, the take-off, the BOQ) is
//! refused before it runs; and a text in a result that prints the first quantity with its unit
//! is replaced before the reader sees it.

use std::sync::{Arc, Mutex, PoisonError};

use qiyas_core::second_reading::{Brief, SecondReadingCommand};
use qiyas_core::{Command, Session, Source};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::agent::AgentSettings;
use crate::provider::{
    AiError, ContentBlock, Message, Provider, ResultPart, StopReason, StreamEvent, TurnRequest,
};
use crate::surface::{self, Route};
use crate::ToolOutput;

/// The reads a second reader may use: exact facts of the drawing and nothing Qiyas derived.
pub const ALLOWED_OPS: [&str; 5] = ["query", "measure", "rooms", "commands", "record_schema"];

/// Words that name a Qiyas result. A call carrying one is refused.
const FORBIDDEN: [&str; 9] = [
    "measurement",
    "generated",
    "boq",
    "takeoff",
    "take-off",
    "second_reading",
    "queries",
    "knowledge",
    "deliverable",
];

/// At most this many items are read per pass, the ones with the most value first.
pub const ITEMS_PER_PASS: usize = 12;
/// At most this many tool rounds for one item.
const ROUNDS: usize = 14;
/// A result is cut to this many characters.
const RESULT_LIMIT: usize = 12_000;

const SYSTEM: &str = "You are the second reader of a quantity surveyor's take-off. Measure ONE item \
from the drawing yourself, with the read tool only, and answer with a number. You are given the \
item's description, its unit and the measurement rule. You are not told any earlier figure and must \
not look for one: do not read the BOQ, the take-off, measurement results or any figure Qiyas wrote; \
measure the drawing's own geometry, texts and schedules. Apply the rule as written, deductions \
included. Work by exact queries and measure numbers, and keep it short. Your last message is only \
JSON: {\"quantity\": <number>} in the unit given, or {\"quantity\": null} when you cannot measure it.";

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pass {
    /// Items recorded, as (code, first, second, agrees).
    pub read: Vec<(String, f64, f64, bool)>,
    /// Items the reader could not measure.
    pub failed: Vec<String>,
    /// Items still unchecked because of the per-pass limit.
    pub left: usize,
}

impl Pass {
    pub fn differing(&self) -> impl Iterator<Item = &(String, f64, f64, bool)> {
        self.read.iter().filter(|r| !r.3)
    }

    /// The advice put on the tool result that triggered the pass.
    pub fn advice(&self) -> Value {
        let differ: Vec<Value> = self
            .differing()
            .map(|(code, first, second, _)| json!({"code": code, "first": first, "second": second}))
            .collect();
        json!({
            "code": "second_reading",
            "message": "A fresh reader measured the items again without seeing the first quantity. An item whose reading differs is not in the BOQ: it is a query for the engineer. Tell the engineer which, with both readings.",
            "checked": self.read.len(),
            "agree": self.read.len() - differ.len(),
            "differ": differ,
            "not_measured": self.failed,
            "not_checked": self.left,
        })
    }
}

/// Whether a call is the point where a quantity reaches a BOQ or the comparison.
pub fn triggers(tool: &str, op: &str, args: &Value) -> bool {
    if tool != surface::EXECUTE {
        return false;
    }
    match op {
        "compare" | "export_takeoff" => true,
        "run" => {
            // Canonical wire is kind "export" with report "boq"; the shorthand puts the report in kind.
            let command = &args["command"];
            command["type"] == "deliverable"
                && (command["report"] == "boq" || command["kind"] == "boq")
        }
        _ => false,
    }
}

/// Refuses a read that could show the reader the first quantity.
pub fn guard(op: &str, args: &Value) -> Result<(), String> {
    if !ALLOWED_OPS.contains(&op) {
        return Err(format!(
            "Not allowed in a second reading. Use: {}",
            ALLOWED_OPS.join(", ")
        ));
    }
    let text = args.to_string().to_lowercase();
    if let Some(word) = FORBIDDEN.iter().find(|w| text.contains(*w)) {
        return Err(format!(
            "Not allowed in a second reading: {word} is a Qiyas result. Measure the drawing itself."
        ));
    }
    Ok(())
}

/// Replaces any text that prints the first quantity with its unit ("17.81 m²").
pub fn redact(value: &mut Value, first: f64, unit: &str) {
    let number = format!("{first:.2}");
    let unit = qiyas_core::deliverables::display_unit(unit).to_string();
    fn walk(value: &mut Value, number: &str, unit: &str) {
        match value {
            Value::String(s) => {
                let squeezed: String = s.chars().filter(|c| !c.is_whitespace()).collect();
                if squeezed.contains(&format!("{number}{unit}")) {
                    *s = "[a Qiyas result, hidden from this reading]".into();
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|v| walk(v, number, unit)),
            Value::Object(o) => o.values_mut().for_each(|v| walk(v, number, unit)),
            _ => {}
        }
    }
    walk(value, &number, &unit);
}

/// The quantity in the reader's last message: `{"quantity": 12.5}`.
pub fn parse_reading(text: &str) -> Option<f64> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    let value: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    value["quantity"]
        .as_f64()
        .filter(|q| q.is_finite() && *q >= 0.0)
}

fn reader_tools() -> Vec<crate::ToolSpec> {
    let mut read = surface::tools()
        .into_iter()
        .find(|t| t.name == surface::READ)
        .expect("the read tool");
    read.input_schema["properties"]["op"]["enum"] = json!(ALLOWED_OPS);
    vec![read]
}

fn prompt(brief: &Brief) -> String {
    format!(
        "Item {code}\nDescription: {en}\nالوصف: {ar}\nUnit: {unit}\nRule: {rule}\nSheets: {sheets}\n\nMeasure it from the drawing and answer with the JSON.",
        code = brief.code,
        en = brief.description_en,
        ar = brief.description_ar,
        unit = brief.unit,
        rule = if brief.rule.is_empty() { "as described" } else { &brief.rule },
        sheets = brief.sheets.join(", "),
    )
}

async fn read_op(
    session: &Arc<Mutex<Option<Session>>>,
    input: &Value,
    first: f64,
    unit: &str,
) -> Result<String, String> {
    let route = surface::route(surface::READ, input)?;
    let Route::Session { tool, op, args } = route else {
        return Err("Not allowed in a second reading".into());
    };
    guard(&op, &args)?;
    let session = session.clone();
    let unit = unit.to_string();
    let ran = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut guard = session.lock().unwrap_or_else(PoisonError::into_inner);
        let s = guard.as_mut().ok_or("No drawing is open")?;
        match surface::run_session(s, &tool, &op, &args, Source::Ai) {
            Ok(ToolOutput::Json(mut value)) => {
                redact(&mut value, first, &unit);
                Ok(value.to_string())
            }
            Ok(ToolOutput::Image { .. }) => Ok("No pictures in a second reading".to_string()),
            Err(e) => Err(e.json().to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(crate::log::clip(&ran).chars().take(RESULT_LIMIT).collect())
}

/// One fresh reader for one item. `first` is only used to keep it from being printed back.
pub async fn read_quantity(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    brief: &Brief,
    first: f64,
    cancel: &CancellationToken,
) -> Result<Option<f64>, AiError> {
    let tools = reader_tools();
    let mut messages = vec![Message::user(vec![ContentBlock::Text {
        text: prompt(brief),
    }])];
    for _ in 0..ROUNDS {
        if cancel.is_cancelled() {
            return Ok(None);
        }
        let req = TurnRequest {
            cache_key: format!("second-reading-{}", brief.code),
            system: SYSTEM.to_string(),
            messages: messages.clone(),
            tools: tools.clone(),
            model: settings.model.clone(),
            effort: settings.effort.clone(),
            max_tokens: settings.max_tokens.min(4_000),
        };
        let mut sink = |_event: StreamEvent| {};
        let turn = provider.stream_turn(req, &mut sink).await?;
        messages.push(Message::assistant(turn.content.clone()));
        let calls: Vec<(String, Value)> = messages
            .last()
            .map(|m| {
                m.tool_uses()
                    .map(|(id, _, input)| (id.to_string(), input.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if calls.is_empty() || turn.stop_reason != StopReason::ToolUse {
            let text: String = turn
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(parse_reading(&text));
        }
        let mut results = Vec::new();
        for (id, input) in calls {
            let (text, is_error) = match read_op(session, &input, first, &brief.unit).await {
                Ok(text) => (text, false),
                Err(why) => (why, true),
            };
            results.push(ContentBlock::ToolResult {
                tool_use_id: id,
                content: vec![ResultPart::Text(text)],
                is_error,
            });
        }
        messages.push(Message::user(results));
    }
    Ok(None)
}

/// Reads the unchecked items twice, the ones with the most value first, and records each
/// reading. Items beyond `ITEMS_PER_PASS` stay "not checked". A provider error ends the pass
/// with what was read so far.
pub async fn run_pass(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    cancel: &CancellationToken,
) -> Result<Pass, String> {
    let shared = session.clone();
    let (items, total) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = guard.as_ref() else {
            return Ok((Vec::new(), 0));
        };
        let all = s.second_reading_brief(None).map_err(|e| e.to_string())?;
        let total = all.len();
        let report = s.measurement_report().map_err(|e| e.to_string())?;
        let items: Vec<(Brief, f64)> = all
            .into_iter()
            .take(ITEMS_PER_PASS)
            .filter_map(|b| {
                let q = report.items.iter().find(|i| i.code == b.code)?.quantity;
                Some((b, q))
            })
            .collect();
        Ok((items, total))
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut pass = Pass {
        left: total.saturating_sub(items.len()),
        ..Default::default()
    };
    for (brief, first) in items {
        if cancel.is_cancelled() {
            pass.left += 1;
            continue;
        }
        let second = match read_quantity(provider, session, settings, &brief, first, cancel).await {
            Ok(Some(q)) => q,
            Ok(None) => {
                pass.failed.push(brief.code.clone());
                continue;
            }
            Err(_) => {
                pass.failed.push(brief.code.clone());
                pass.left += 1;
                break;
            }
        };
        let shared = session.clone();
        let code = brief.code.clone();
        let recorded = tokio::task::spawn_blocking(move || -> Result<qiyas_core::Outcome, String> {
            let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
            let s = guard.as_mut().ok_or("No drawing is open")?;
            s.run_as(
                Command::SecondReading(Box::new(SecondReadingCommand::Record {
                    code,
                    quantity: second,
                })),
                Source::Ai,
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        let agrees = match recorded {
            qiyas_core::Outcome::Measurement { result } => result["agrees"].as_bool() == Some(true),
            _ => false,
        };
        pass.read.push((brief.code, first, second, agrees));
    }
    Ok(pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reading_is_the_number_in_the_json() {
        assert_eq!(parse_reading("{\"quantity\": 12.5}"), Some(12.5));
        assert_eq!(parse_reading("Done. {\"quantity\": 3}"), Some(3.0));
        assert_eq!(parse_reading("{\"quantity\": null}"), None);
        assert_eq!(parse_reading("{\"quantity\": -2}"), None);
        assert_eq!(parse_reading("about twelve"), None);
    }

    #[test]
    fn the_guard_refuses_what_could_show_the_first_quantity() {
        assert!(guard("query", &json!({"kind":"texts"})).is_ok());
        assert!(guard("measure", &json!({"handle": 12})).is_ok());
        assert!(guard("state", &json!({})).is_err());
        assert!(guard("records", &json!({"collection":"entities"})).is_err());
        assert!(guard("query", &json!({"kind":"texts","text":"takeoff"})).is_err());
        assert!(guard("query", &json!({"kind":"texts","text":"BOQ"})).is_err());
    }

    #[test]
    fn a_text_that_prints_the_first_quantity_is_hidden() {
        let mut v = json!({"rows":[{"text":"Wall length 45.20 m"},{"text":"Door 3"}],"n":45.2});
        redact(&mut v, 45.2, "m");
        assert_eq!(v["rows"][1]["text"], "Door 3");
        assert!(v["rows"][0]["text"].as_str().unwrap().contains("hidden"));
        assert_eq!(v["n"], 45.2, "numbers of the drawing stay");
    }

    #[test]
    fn only_the_deliverable_boq_compare_and_take_off_export_start_a_pass() {
        assert!(triggers("qiyas_execute", "compare", &json!({})));
        assert!(triggers("qiyas_execute", "export_takeoff", &json!({})));
        assert!(triggers(
            "qiyas_execute",
            "run",
            &json!({"command":{"type":"deliverable","kind":"boq"}})
        ));
        assert!(triggers(
            "qiyas_execute",
            "run",
            &json!({"command":{"type":"deliverable","kind":"export","report":"boq"}})
        ));
        assert!(!triggers(
            "qiyas_execute",
            "run",
            &json!({"command":{"type":"deliverable","kind":"queries"}})
        ));
        assert!(!triggers("qiyas_read", "compare", &json!({})));
    }
}
