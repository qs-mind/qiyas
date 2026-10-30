//! The blind second reading (W6). Before a generated quantity reaches the BOQ, a fresh reader
//! with no conversation history and read-only tools measures the same item again. It sees the
//! item's complete scope and formulas, never the first quantity. Code, not a model, compares the
//! two (`qiyas_core::second_reading`): this module only asks the reader and records its number.
//!
//! What keeps it blind: the reader's tools are a short list of exact reads of the drawing
//! (`ALLOWED_OPS`); a call that names a Qiyas result (the measurement, the take-off, the BOQ) is
//! refused before it runs; and drawing-generated result text is hidden by provenance before the reader sees it.

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
pub const ALLOWED_OPS: [&str; 9] = [
    "query",
    "measure",
    "rooms",
    "commands",
    "record_schema",
    "records",
    "spatial_query",
    "run",
    "evidence_page",
];

/// Exact result selectors. Free drawing text and search strings are not selectors.
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
/// Maximum serialized size of one evidence page, in bytes.
const RESULT_LIMIT: usize = 12_000;

const SYSTEM: &str = "You are the second reader of a quantity surveyor's take-off. Measure ONE item \
from the drawing yourself, with the read tool only, and answer with a number. You are given the \
item's full scope, selection, exclusions, grouping definitions, unit and formulas. You are not told any earlier figure and must \
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
    /// Reader failures, retained in the triggering tool result.
    pub errors: Vec<String>,
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
            "message": "Differing readings remain in the BOQ with both values and an open drawing query.",
            "message_ar": "تبقى القراءات المختلفة في المقايسة بالقيمتين واستفسار رسم مفتوح.",
            "checked": self.read.len(),
            "agree": self.read.len() - differ.len(),
            "differ": differ,
            "not_measured": self.failed,
            "not_checked": self.left,
            "errors": self.errors,
        })
    }
}

/// Whether a call is the point where a quantity reaches a BOQ or the comparison.
pub fn triggers(tool: &str, op: &str, args: &Value) -> bool {
    if tool != surface::EXECUTE {
        return false;
    }
    match op {
        "compare" | "export_takeoff" | "export_evidence" => true,
        "run" => command_triggers(&args["command"]),
        "batch" => args["commands"]
            .as_array()
            .is_some_and(|commands| commands.iter().any(command_triggers)),
        _ => false,
    }
}

fn command_triggers(command: &Value) -> bool {
    if command["type"] == "batch" {
        return command["commands"]
            .as_array()
            .is_some_and(|commands| commands.iter().any(command_triggers));
    }
    command["type"] == "discrepancy_check"
        || (command["type"] == "deliverable"
            && command["kind"] == "export"
            && matches!(
                command["report"].as_str(),
                Some("boq" | "dimensions" | "variation")
            ))
}

/// Refuses a read that could show the reader the first quantity.
pub fn guard(op: &str, args: &Value) -> Result<(), String> {
    if !ALLOWED_OPS.contains(&op) {
        return Err(format!(
            "Not allowed in a second reading. / غير مسموح في القراءة الثانية. Use: {}",
            ALLOWED_OPS.join(", ")
        ));
    }
    if op == "run" && args["command"]["type"] != "spatial_query" {
        return Err("Only spatial_query is allowed here. / يسمح بالاستعلام المكاني فقط هنا.".into());
    }
    if op == "records" {
        let collection = args["collection"].as_str().unwrap_or("entities");
        if !matches!(
            collection,
            "entities" | "objects" | "layers" | "blocks" | "layouts" | "groups"
        ) {
            return Err("This collection can contain Qiyas results. / قد تحتوي هذه المجموعة على نتائج قياس.".into());
        }
        // Projection or predicates on derived results would bypass redaction or disclose
        // the result through the filtered record count.
        for selector in args["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .chain(
                args["where"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["path"].as_str()),
            )
        {
            if selector.split('/').any(|part| {
                matches!(
                    part,
                    "quantity"
                        | "quantities"
                        | "measurement"
                        | "measurement_result"
                        | "generated_quantity"
                        | "rolled_up"
                        | "result"
                        | "extended_data"
                        | "text"
                        | "value"
                        | "cells"
                )
            }) {
                return Err("Result fields are private to this reading. / حقول النتائج محجوبة عن هذه القراءة.".into());
            }
        }
    }
    let selector = match op {
        "query" => args.get("kind"),
        "record_schema" => args.get("name"),
        _ => None,
    };
    let mut selectors = selector
        .into_iter()
        .chain(args.get("collection"))
        .chain(args.get("command").and_then(|c| c.get("type")))
        .filter_map(Value::as_str);
    if let Some(word) = selectors.find(|s| FORBIDDEN.iter().any(|w| s.eq_ignore_ascii_case(w))) {
        return Err(format!(
            "Not allowed in a second reading: {word} is a Qiyas result. / هذه نتيجة قياس محجوبة عن القراءة الثانية."
        ));
    }
    Ok(())
}

/// Removes result fields and text only when its drawing handle has output provenance.
pub fn redact(value: &mut Value, hidden: &std::collections::BTreeSet<u64>) {
    redact_scoped(value, hidden, &std::collections::BTreeMap::new());
}

fn redact_scoped(
    value: &mut Value,
    hidden: &std::collections::BTreeSet<u64>,
    xrefs: &std::collections::BTreeMap<String, std::collections::BTreeSet<u64>>,
) {
    match value {
        Value::Array(rows) => rows
            .iter_mut()
            .for_each(|row| redact_scoped(row, hidden, xrefs)),
        Value::Object(object) => {
            let empty = std::collections::BTreeSet::new();
            let hidden = if let Some(name) = object.get("xref").and_then(Value::as_str) {
                xrefs
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, handles)| handles)
                    .unwrap_or(&empty)
            } else {
                hidden
            };
            let handle = object
                .get("handle")
                .and_then(Value::as_u64)
                .or_else(|| object.get("source").and_then(Value::as_u64))
                .or_else(|| {
                    object
                        .get("key")
                        .and_then(qiyas_core::registry::handle_number)
                })
                .or_else(|| {
                    object
                        .get("common")
                        .and_then(|v| v.get("handle"))
                        .and_then(Value::as_u64)
                });
            if handle.is_some_and(|h| hidden.contains(&h)) {
                object.retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "handle" | "key" | "type" | "kind" | "layer" | "bounds" | "xref"
                    )
                });
                object.insert("hidden_result".into(), json!(true));
                return;
            }
            for key in [
                "quantity",
                "quantities",
                "measurement",
                "measurement_result",
                "second_reading",
                "generated",
                "generated_quantity",
                "rolled_up",
                "extended_data",
            ] {
                object.remove(key);
            }
            object
                .values_mut()
                .for_each(|value| redact_scoped(value, hidden, xrefs));
        }
        _ => {}
    }
}

/// Lossless JSON pages. The reader concatenates `json` chunks, or reads a JSON pointer
/// from the cached original. Counts and source paging metadata remain intact in the envelope.
fn evidence_page(value: &Value, id: usize, offset: usize, pointer: &str) -> Result<String, String> {
    let selected = if pointer.is_empty() {
        value
    } else {
        value
            .pointer(pointer)
            .ok_or("Evidence pointer not found. / مسار الدليل غير موجود.")?
    };
    let serialized = selected.to_string();
    if offset > serialized.len() || !serialized.is_char_boundary(offset) {
        return Err("Invalid evidence offset. / إزاحة الدليل غير صالحة.".into());
    }
    if offset == 0 && serialized.len() <= RESULT_LIMIT {
        let page = json!({"evidence_id":id,"data":selected,"truncated":false,"next_offset":null});
        if page.to_string().len() <= RESULT_LIMIT {
            return Ok(page.to_string());
        }
    }
    let mut end = (offset + RESULT_LIMIT / 2).min(serialized.len());
    while !serialized.is_char_boundary(end) {
        end -= 1;
    }
    loop {
        let page = json!({"evidence_id":id,"encoding":"json","json":&serialized[offset..end],
            "total_bytes":serialized.len(),"total":value.get("total"),
            "source_next_offset":value.get("next_offset"),"offset":offset,
            "truncated":end < serialized.len(),"next_offset":(end < serialized.len()).then_some(end)}).to_string();
        if page.len() <= RESULT_LIMIT {
            return Ok(page);
        }
        end = offset + (end - offset) / 2;
        while !serialized.is_char_boundary(end) {
            end -= 1;
        }
    }
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
    read.description = "Quantity-blind drawing reads: query, measure, rooms, records (entities or objects with properties), spatial_query (kind/targets/point), commands, record_schema. Evidence pages are lossless: follow evidence_page {id: evidence_id, offset: next_offset, pointer?: JSON pointer} until truncated is false. Concatenate json chunks before parsing. data contains a complete result; follow its own next_offset with the original read.";
    vec![read]
}

fn prompt(brief: &Brief) -> String {
    format!(
        "Item {code}\nDescription: {en}\nالوصف: {ar}\nUnit: {unit}\nRule: {rule}\nSheets: {sheets}\nScope: {scope}\n\nMeasure it from the drawing and answer with the JSON.",
        code = brief.code,
        en = brief.description_en,
        ar = brief.description_ar,
        unit = brief.unit,
        rule = if brief.rule.is_empty() { "as described" } else { &brief.rule },
        sheets = brief.sheets.join(", "),
        scope = brief.scope,
    )
}

async fn read_op(
    session: &Arc<Mutex<Option<Session>>>,
    input: &Value,
    _first: f64,
    _unit: &str,
) -> Result<String, String> {
    let mut routed = input.clone();
    if routed["op"] == "spatial_query" {
        let mut command = routed["args"].clone();
        command["type"] = json!("spatial_query");
        routed = json!({"op":"run","args":{"command":command}});
    }
    let route = surface::route(surface::READ, &routed)?;
    let Route::Session { tool, op, mut args } = route else {
        return Err("Not allowed in a second reading. / غير مسموح في القراءة الثانية.".into());
    };
    guard(&op, &args)?;
    if op == "records" {
        args["detail"] = json!("full");
    }
    let session = session.clone();
    let ran = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut guard = session.lock().unwrap_or_else(PoisonError::into_inner);
        let s = guard
            .as_mut()
            .ok_or("No drawing is open. / لا يوجد رسم مفتوح.")?;
        if op == "query" && args["kind"] == "tables" {
            let within = args
                .get("within")
                .filter(|value| !value.is_null())
                .map(|value| serde_json::from_value(value.clone()))
                .transpose()
                .map_err(|e| e.to_string())?;
            let tables = s.blind_tables(args["sheet"].as_str(), within);
            let total = tables.len();
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = (args["limit"].as_u64().unwrap_or(usize::MAX as u64) as usize).max(1);
            let rows: Vec<_> = tables.into_iter().skip(offset).take(limit).collect();
            let end = offset + rows.len();
            return Ok(json!({"rows":rows,"total":total,"offset":offset,
                "truncated":end < total,"next_offset":(end < total).then_some(end)})
            .to_string());
        }
        match surface::run_session(s, &tool, &op, &args, Source::Ai) {
            Ok(ToolOutput::Json(mut value)) => {
                redact_scoped(
                    &mut value,
                    &s.blind_result_handles(),
                    &s.blind_xref_result_handles(),
                );
                Ok(value.to_string())
            }
            Ok(ToolOutput::Image { .. }) => {
                Err("No pictures in a second reading. / لا صور في القراءة الثانية.".to_string())
            }
            Err(e) => Err(e.json().to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(ran)
}

/// One fresh reader for one item. The first reading is never sent to the provider.
pub async fn read_quantity(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    brief: &Brief,
    first: f64,
    cancel: &CancellationToken,
) -> Result<Option<f64>, AiError> {
    let tools = reader_tools();
    let mut evidence = Vec::<Value>::new();
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
            let result = if input["op"] == "evidence_page" {
                let args = &input["args"];
                let id = args["id"].as_u64().unwrap_or(usize::MAX as u64) as usize;
                evidence
                    .get(id)
                    .ok_or_else(|| "Evidence not found. / الدليل غير موجود.".to_string())
                    .and_then(|value| {
                        evidence_page(
                            value,
                            id,
                            args["offset"].as_u64().unwrap_or(0) as usize,
                            args["pointer"].as_str().unwrap_or(""),
                        )
                    })
            } else {
                match read_op(session, &input, first, &brief.unit).await {
                    Ok(text) => match serde_json::from_str::<Value>(&text) {
                        Ok(value) => {
                            let id = evidence.len();
                            evidence.push(value);
                            evidence_page(&evidence[id], id, 0, "")
                        }
                        Err(error) => Err(error.to_string()),
                    },
                    Err(error) => Err(error),
                }
            };
            let (text, is_error) = match result {
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

/// Drain all remaining items, attempting a failed item only once per operation.
pub async fn run_all(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    cancel: &CancellationToken,
) -> Result<Pass, String> {
    let mut combined = Pass::default();
    let mut attempted = std::collections::BTreeSet::new();
    loop {
        let pass = run_pass_excluding(provider, session, settings, cancel, &attempted).await?;
        attempted.extend(pass.read.iter().map(|read| read.0.clone()));
        attempted.extend(pass.failed.iter().cloned());
        let stop = pass.left == 0
            || cancel.is_cancelled()
            || !pass.errors.is_empty()
            || (pass.read.is_empty() && pass.failed.is_empty());
        combined.left = pass.left;
        combined.read.extend(pass.read);
        combined.failed.extend(pass.failed);
        combined.errors.extend(pass.errors);
        if stop {
            return Ok(combined);
        }
    }
}

/// The blocking command worker lends its live session to the asynchronous reader while
/// keeping the outer session lock and batch transaction. Restore it even on unwinding.
pub struct ReadRequest {
    pub session: Arc<Mutex<Option<Session>>>,
    pub finished: std::sync::mpsc::Sender<()>,
}

pub fn observe_operation<T>(
    requests: tokio::sync::mpsc::UnboundedSender<ReadRequest>,
    action: impl FnOnce() -> T,
) -> T {
    qiyas_core::second_reading::with_observer(
        move |session| {
            struct Loan<'a> {
                original: &'a mut Session,
                shared: Arc<Mutex<Option<Session>>>,
            }
            impl Drop for Loan<'_> {
                fn drop(&mut self) {
                    if let Some(session) = self
                        .shared
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                    {
                        *self.original = session;
                    }
                }
            }
            let placeholder = Session::new_blank(&session.drawing.path, 6);
            let shared = Arc::new(Mutex::new(Some(std::mem::replace(session, placeholder))));
            let loan = Loan {
                original: session,
                shared,
            };
            let (finished, wait) = std::sync::mpsc::channel();
            if requests
                .send(ReadRequest {
                    session: loan.shared.clone(),
                    finished,
                })
                .is_ok()
            {
                let _ = wait.recv();
            }
        },
        action,
    )
}

/// Reads the unchecked items twice, the ones with the most value first, and records each
/// reading. `run_all` continues beyond `ITEMS_PER_PASS`. A provider error ends the pass
/// with what was read so far.
pub async fn run_pass(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    cancel: &CancellationToken,
) -> Result<Pass, String> {
    run_pass_excluding(
        provider,
        session,
        settings,
        cancel,
        &std::collections::BTreeSet::new(),
    )
    .await
}

async fn run_pass_excluding(
    provider: &dyn Provider,
    session: &Arc<Mutex<Option<Session>>>,
    settings: &AgentSettings,
    cancel: &CancellationToken,
    attempted: &std::collections::BTreeSet<String>,
) -> Result<Pass, String> {
    let attempted = attempted.clone();
    let shared = session.clone();
    let (items, total) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(s) = guard.as_ref() else {
            return Ok((Vec::new(), 0));
        };
        let all: Vec<_> = s
            .second_reading_brief(None)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|brief| !attempted.contains(&brief.code))
            .collect();
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
    let mut items = items.into_iter();
    while let Some((brief, first)) = items.next() {
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
            Err(error) => {
                pass.errors.push(error.to_string());
                pass.failed.push(brief.code.clone());
                pass.left += items.len();
                break;
            }
        };
        let shared = session.clone();
        let code = brief.code.clone();
        let unit = brief.unit.clone();
        let binding = brief.binding.clone();
        let recorded =
            tokio::task::spawn_blocking(move || -> Result<Option<qiyas_core::Outcome>, String> {
                let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
                let s = guard
                    .as_mut()
                    .ok_or("No drawing is open. / لا يوجد رسم مفتوح.")?;
                let report = s.measurement_report().map_err(|e| e.to_string())?;
                let current = report.items.iter().find(|item| {
                    item.code == code
                        && item.unit == unit
                        && item.quantity == first
                        && !item.informational
                });
                let Some(current) = current else {
                    return Ok(None);
                };
                if s.second_reading_binding(current, &report)
                    .map_err(|e| e.to_string())?
                    != binding
                {
                    return Ok(None);
                }
                s.run_as(
                    Command::SecondReading(Box::new(SecondReadingCommand::Record {
                        code,
                        quantity: second,
                    })),
                    Source::Ai,
                )
                .map(Some)
                .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
        let Some(recorded) = recorded else {
            pass.failed.push(brief.code);
            continue;
        };
        let agrees = match recorded {
            qiyas_core::Outcome::Measurement { result } => result["agrees"].as_bool() == Some(true),
            _ => false,
        };
        pass.read.push((brief.code, first, second, agrees));
    }
    if !pass.read.is_empty() {
        let shared = session.clone();
        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
            let s = guard
                .as_mut()
                .ok_or("No drawing is open. / لا يوجد رسم مفتوح.")?;
            s.run_as(Command::Check { tolerance: None }, Source::Ai)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
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
        assert!(guard("records", &json!({"collection":"entities"})).is_ok());
        assert!(guard("query", &json!({"kind":"texts","text":"takeoff"})).is_ok());
        assert!(guard("query", &json!({"kind":"texts","text":"BOQ"})).is_ok());
        for name in FORBIDDEN {
            assert!(guard(name, &json!({})).is_err(), "{name}");
            assert!(guard("query", &json!({"kind":name})).is_err(), "{name}");
            assert!(guard("records", &json!({"collection":name})).is_err());
            assert!(guard("run", &json!({"command":{"type":name}})).is_err());
        }
    }

    #[test]
    fn drawing_layers_and_search_text_are_not_result_selectors() {
        assert!(guard("query", &json!({"kind":"texts","layer":"BOQ-notes"})).is_ok());
        assert!(guard("rooms", &json!({"text_layer":"knowledge-generated"})).is_ok());
        assert!(guard("commands", &json!({"search":"boq generated knowledge"})).is_ok());
        assert!(guard(
            "query",
            &json!({"kind":"texts","text":"generated knowledge"})
        )
        .is_ok());
        assert!(guard("boq", &json!({})).is_err());
        assert!(guard("records", &json!({"collection":"boq"})).is_err());
        assert!(guard("query", &json!({"kind":"BOQ"})).is_err());
    }

    #[tokio::test]
    async fn the_reader_can_read_boq_notes_on_the_drawing_but_not_the_boq() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("notes.dxf"), 6);
        session
            .run_input(
                &json!({"type":"batch","layer":"BOQ-notes","commands":[
                    {"type":"text","at":[0,0],"height":0.25,"value":"Door width 0.90 m"}
                ]}),
                Source::User,
            )
            .unwrap();
        let shared = Arc::new(Mutex::new(Some(session)));
        let read = read_op(
            &shared,
            &json!({"op":"query","args":{"kind":"texts","layer":"BOQ-notes"}}),
            5.0,
            "m",
        )
        .await
        .unwrap();
        assert!(read.contains("Door width 0.90 m"), "{read}");
        assert!(read_op(
            &shared,
            &json!({"op":"records","args":{"collection":"boq"}}),
            5.0,
            "m",
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn drawing_reads_keep_engineer_text_properties_and_spatial_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("blind.dxf"), 6);
        for source in [Source::User, Source::Ai] {
            session
                .run_input(
                    &json!({"type":"text","at":[0,0],"height":0.2,"value":"Doors 16 pcs"}),
                    source,
                )
                .unwrap();
        }
        let line = session
            .run_input(
                &json!({"type":"draw","kind":"line","a":[0,0],"b":[4,0]}),
                Source::User,
            )
            .unwrap();
        let qiyas_core::Outcome::Added { handles } = line.outcome else {
            panic!("line expected")
        };
        let shared = Arc::new(Mutex::new(Some(session)));
        let texts: Value = serde_json::from_str(
            &read_op(
                &shared,
                &json!({"op":"query","args":{"kind":"texts"}}),
                16.0,
                "pcs",
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            texts["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["text"] == "Doors 16 pcs")
                .count(),
            1
        );
        assert!(texts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["hidden_result"] == true));
        let spatial: Value = serde_json::from_str(&read_op(&shared,
            &json!({"op":"spatial_query","args":{"kind":"nearest","targets":[{"handle":handles[0]}],"point":[2,1]}}),
            16.0, "pcs").await.unwrap()).unwrap();
        assert!(!spatial.is_null());
        let records = read_op(
            &shared,
            &json!({"op":"records","args":{"collection":"entities","detail":"full"}}),
            16.0,
            "pcs",
        )
        .await
        .unwrap();
        assert!(records.contains("Doors 16 pcs"));
        assert!(!records.contains("extended_data"));
    }

    #[tokio::test]
    async fn object_records_keep_properties_but_remove_evaluated_quantities() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("objects.dxf"), 6);
        for command in [
            json!({"type":"define_object_type","definition":{"name":"Observed","category":"test","geometry":"point",
                "properties":{"height":{"kind":"number","unit":"length","default":3.2}},
                "quantities":{"result_count":"83"}}}),
            json!({"type":"place_objects","objects":[{"type":"Observed","points":[[0,0]],"props":{"height":3.2}}]}),
        ] {
            session.run_input(&command, Source::User).unwrap();
        }
        let shared = Arc::new(Mutex::new(Some(session)));
        let text = read_op(
            &shared,
            &json!({"op":"records","args":{"collection":"objects"}}),
            83.0,
            "pcs",
        )
        .await
        .unwrap();
        let page: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(page["total"], 1);
        assert_eq!(page["records"][0]["properties"]["height"], 3.2);
        assert!(!text.contains("\"quantities\""), "{text}");
        assert!(!text.contains("result_count"), "{text}");
    }

    #[tokio::test]
    async fn generated_tables_and_fields_are_hidden_but_engineer_tables_remain() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("tables.dxf"), 6);
        for (source, y) in [(Source::User, 0), (Source::Ai, 10)] {
            session
                .run_input(
                    &json!({"type":"table","at":[0,y],"rows":2,"cols":2,
                "col_width":2,"row_height":1,"data":[["Item","Count"],["Doors","16"]]}),
                    source,
                )
                .unwrap();
        }
        session
            .run_input(
                &json!({"type":"field","at":[0,20],"height":0.2,
            "field":{"kind":"count","block":"DOOR"}}),
                Source::User,
            )
            .unwrap();
        let shared = Arc::new(Mutex::new(Some(session)));
        let tables: Value = serde_json::from_str(
            &read_op(
                &shared,
                &json!({"op":"query","args":{"kind":"tables"}}),
                16.0,
                "pcs",
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(tables["total"], 1);
        assert!(tables.to_string().contains("Doors"));
        let texts: Value = serde_json::from_str(
            &read_op(
                &shared,
                &json!({"op":"query","args":{"kind":"texts"}}),
                16.0,
                "pcs",
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert!(texts["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["hidden_result"] == true));
    }

    #[test]
    fn redaction_uses_output_handles_and_preserves_engineer_numbers() {
        let mut value = json!({"rows":[
            {"handle":1,"text":"Wall length 45.20 m"},
            {"handle":2,"text":"Wall length 45.20 m"},
            {"handle":3,"text":"الأبواب ٢ عدد"}],"quantities":{"count":45.2}});
        redact(&mut value, &std::collections::BTreeSet::from([2]));
        assert_eq!(value["rows"][0]["text"], "Wall length 45.20 m");
        assert_eq!(value["rows"][1]["hidden_result"], true);
        assert_eq!(value["rows"][2]["text"], "الأبواب ٢ عدد");
        assert!(value.get("quantities").is_none());
    }

    #[test]
    fn evidence_pages_are_lossless_and_keep_totals_past_the_old_log_limit() {
        let value = json!({"total":301,"rows":(0..301).map(|i|
            json!({"handle":i,"text":"باب طويل".repeat(20)})).collect::<Vec<_>>()});
        let mut offset = 0;
        let mut joined = String::new();
        loop {
            let text = evidence_page(&value, 0, offset, "").unwrap();
            assert!(text.len() <= RESULT_LIMIT);
            let page: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(page["total"], 301);
            joined.push_str(page["json"].as_str().unwrap());
            if page["truncated"] == false {
                break;
            }
            let next = page["next_offset"].as_u64().unwrap() as usize;
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(serde_json::from_str::<Value>(&joined).unwrap(), value);
    }

    #[test]
    fn result_projection_and_filter_side_channels_are_refused() {
        for args in [
            json!({"collection":"objects","fields":["/quantities/count"]}),
            json!({"collection":"objects","where":[{"path":"/quantities/count","value":16}]}),
        ] {
            assert!(guard("records", &args).is_err());
        }
        assert!(guard("records", &json!({"collection":"objects"})).is_ok());
        assert!(guard("run", &json!({"command":{"type":"spatial_query"}})).is_ok());
    }

    #[test]
    fn only_the_deliverable_boq_compare_and_take_off_export_start_a_pass() {
        assert!(triggers("qiyas_execute", "compare", &json!({})));
        assert!(triggers("qiyas_execute", "export_takeoff", &json!({})));
        assert!(triggers(
            "qiyas_execute",
            "run",
            &json!({"command":{"type":"deliverable","kind":"export","report":"boq"}})
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
        assert!(triggers(
            "qiyas_execute",
            "batch",
            &json!({"commands":[
                {"type":"text","value":"Revision"}, {"type":"discrepancy_check"}
            ]})
        ));
        assert!(!triggers("qiyas_read", "compare", &json!({})));
    }
}
