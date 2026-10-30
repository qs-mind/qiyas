//! Check the user's stated expectations. Names and thresholds come from the caller, not trade rules.
use crate::{surface, ToolError, ToolOutput};
use qiyas_core::{Expectation, Session, Source};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    id: String,
    text: String,
    #[serde(default)]
    verify: Option<Expectation>,
    #[serde(default)]
    read: Option<Read>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    op: String,
    args: Value,
    pointer: String,
    expected: Value,
    #[serde(default)]
    tolerance: f64,
    /// A previously read value, for a signed change such as +1.5 m east. The current
    /// measurement always comes from the drawing; this is only the comparison baseline.
    #[serde(default)]
    subtract: Option<Value>,
}

fn difference(now: &Value, before: &Value) -> Option<Value> {
    if let (Some(a), Some(b)) = (now.as_f64(), before.as_f64()) {
        return Some(json!(a - b));
    }
    if let (Some(a), Some(b)) = (now.as_array(), before.as_array()) {
        if a.len() == b.len() {
            return a
                .iter()
                .zip(b)
                .map(|(x, y)| difference(x, y))
                .collect::<Option<Vec<_>>>()
                .map(Value::Array);
        }
    }
    None
}

fn equal(now: &Value, expected: &Value, tolerance: f64) -> bool {
    if let (Some(a), Some(b)) = (now.as_f64(), expected.as_f64()) {
        return (a - b).abs() <= tolerance;
    }
    if let (Some(a), Some(b)) = (now.as_array(), expected.as_array()) {
        return a.len() == b.len() && a.iter().zip(b).all(|(x, y)| equal(x, y, tolerance));
    }
    now == expected
}

pub(crate) fn run(session: &mut Session, args: &Value, source: Source) -> Result<Value, ToolError> {
    let items: Vec<Item> = serde_json::from_value(args["expectations"].clone())
        .map_err(|e| ToolError::BadInput(e.to_string()))?;
    if items.is_empty() || items.len() > 100 {
        return Err(ToolError::BadInput(
            "brief_check needs 1 to 100 expectations".into(),
        ));
    }
    if items.iter().any(|item| {
        item.verify.as_ref().is_some_and(|expectation| {
            expectation.targets.is_empty() && expectation.kind != qiyas_core::VerifyKind::Count
        })
    }) {
        return Err(ToolError::BadInput(
            "Literal arithmetic cannot verify the drawing: use the drawing handles as `targets`, or a `read` item (op, args, pointer, expected) that measures the drawing. لا تتحقق العمليات الحسابية المجردة من الرسم: استخدم معرفات عناصر الرسم في `targets`، أو بند `read` (op, args, pointer, expected) يقيس الرسم.".into(),
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut results = Vec::new();
    for item in items {
        if item.id.trim().is_empty() || item.text.trim().is_empty() || !ids.insert(item.id.clone())
        {
            return Err(ToolError::BadInput(
                "Each brief item needs a unique id and the user's requirement in text".into(),
            ));
        }
        let evidence_handles = item
            .verify
            .as_ref()
            .and_then(|v| {
                if v.kind == qiyas_core::VerifyKind::Count {
                    return None;
                }
                explicit_handles(&json!({"targets":v.targets}))
            })
            .or_else(|| item.read.as_ref().and_then(|r| explicit_handles(&r.args)));
        let checked = match (item.verify, item.read) {
            (Some(expectation), None) => {
                let r = session.verify(&[expectation])?.remove(0);
                json!({"passed":r.passed,"measured":r.measured,"expected":r.expected,"tolerance":r.tolerance,
                    "evidence":r,"message":r.message})
            }
            (None, Some(read)) => {
                // No recursive checklist, captures, file mutations or conversational reads.
                if !matches!(
                    read.op.as_str(),
                    "records"
                        | "query"
                        | "state"
                        | "run"
                        | "rooms"
                        | "measure"
                        | "check"
                        | "history"
                        | "artifact"
                ) || !read.tolerance.is_finite()
                    || read.tolerance < 0.
                {
                    return Err(ToolError::BadInput(
                        "Use a drawing read and a nonnegative tolerance".into(),
                    ));
                }
                let evidence = if read.op == "artifact" {
                    artifact(session, &read.args).map(ToolOutput::Json)
                } else {
                    surface::run_session(session, surface::READ, &read.op, &read.args, source)
                };
                match evidence {
                    Ok(ToolOutput::Json(value)) => {
                        let tied = if read.op == "measure" {
                            value["handle"].is_u64()
                        } else if value["outcome"]["type"] == "verified" {
                            value["outcome"]["results"].as_array().is_some_and(|rows| {
                                rows.iter().all(|row| row["tied_to_drawing"] == true)
                            })
                        } else {
                            true
                        };
                        let raw = value.pointer(&read.pointer);
                        let measured = raw.and_then(|v| match &read.subtract {
                            Some(before) => difference(v, before),
                            None => Some(v.clone()),
                        });
                        let passed = tied
                            && measured
                                .as_ref()
                                .is_some_and(|v| equal(v, &read.expected, read.tolerance));
                        json!({"passed":passed,"measured":measured,"observed":raw,"expected":read.expected,"tolerance":read.tolerance,
                            "evidence":{"op":read.op,"args":read.args,"pointer":read.pointer,"subtract":read.subtract},
                            "message":if !tied {"Literal arithmetic does not verify the drawing"} else if passed {"Matches the stated expectation"} else {"The measured value does not match, or that value is missing"}})
                    }
                    Ok(_) => {
                        json!({"passed":false,"measured":null,"message":"A picture cannot prove this expectation"})
                    }
                    Err(e) => json!({"passed":false,"measured":null,"message":e.json()}),
                }
            }
            _ => {
                return Err(ToolError::BadInput(
                    "Each brief item needs exactly one verify or read expectation".into(),
                ))
            }
        };
        let mut result = checked;
        result["evidence_handles"] = json!(evidence_handles);
        result["id"] = json!(item.id);
        result["text"] = json!(item.text);
        results.push(result);
    }
    let passed = results.iter().filter(|r| r["passed"] == true).count();
    Ok(
        json!({"passed":passed,"failed":results.len()-passed,"results":results,
        "basis":"The caller supplied these expectations from the user's brief. This checks the stated facts, not whether the brief was interpreted completely."}),
    )
}

/// Check an export's existence and size without reading its contents. Exports belong beside
/// the drawing; canonical paths also keep a symlink from naming a different directory.
fn artifact(session: &Session, args: &Value) -> Result<Value, ToolError> {
    let given = args["path"]
        .as_str()
        .ok_or_else(|| ToolError::BadInput("An artifact expectation needs a path".into()))?;
    let directory = session
        .drawing
        .path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let path = if std::path::Path::new(given).is_absolute() {
        std::path::PathBuf::from(given)
    } else {
        directory.join(given)
    };
    let allowed = directory
        .canonicalize()
        .map_err(|e| ToolError::BadInput(e.to_string()))?;
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::BadInput("An export needs a filename".into()))?
        .canonicalize()
        .map_err(|e| ToolError::BadInput(e.to_string()))?;
    if !parent.starts_with(&allowed) {
        return Err(ToolError::BadInput(
            "Check an export in the drawing's folder".into(),
        ));
    }
    match path.canonicalize() {
        Ok(resolved) if !resolved.starts_with(&allowed) => Err(ToolError::BadInput(
            "The export points outside the drawing's folder".into(),
        )),
        Ok(resolved) => {
            let meta = std::fs::metadata(resolved).map_err(|e| ToolError::Failed(e.to_string()))?;
            Ok(
                json!({"exists":meta.is_file(),"bytes":meta.len(),"nonempty":meta.is_file() && meta.len()>0,
                "modified_unix":meta.modified().ok().and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|t|t.as_secs()),
                "meaning":"Existence and size only; file contents have not been checked."}),
            )
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({"exists":false,"bytes":0,"nonempty":false}))
        }
        Err(e) => Err(ToolError::Failed(e.to_string())),
    }
}

/// Explicit target reads can be invalidated precisely; aggregate queries remain uncertain.
fn explicit_handles(args: &Value) -> Option<Vec<u64>> {
    let targets = args.get("targets").or_else(|| args.get("handles"));
    if let Some(values) = targets.and_then(Value::as_array) {
        if values.is_empty() {
            return None;
        }
        return values
            .iter()
            .map(|v| {
                qiyas_core::registry::handle_number(v)
                    .or_else(|| qiyas_core::registry::handle_number(&v["handle"]))
            })
            .collect();
    }
    args.get("handle")
        .and_then(qiyas_core::registry::handle_number)
        .map(|h| vec![h])
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Watch {
    report: Option<Value>,
    #[serde(default)]
    dirty: bool,
}
impl Watch {
    pub(crate) fn saved(&self) -> Option<Value> {
        self.report.as_ref().map(|_| json!(self))
    }
    pub(crate) fn restore(value: Option<Value>) -> Self {
        let mut watch: Self = value
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        watch.changed(None);
        watch
    }
    pub(crate) fn checked(&mut self, report: Value) {
        self.report = Some(report);
        self.dirty = false;
    }
    /// Consume the raw edit result before result formatting samples its handle arrays.
    pub(crate) fn changed(&mut self, output: Option<&Value>) {
        let touched = output.and_then(|v| {
            let mut handles = std::collections::BTreeSet::new();
            for key in ["added", "changed", "removed"] {
                let values = v.get(key)?.as_array()?;
                for value in values {
                    handles.insert(value.as_u64()?);
                }
            }
            (!handles.is_empty()).then_some(handles)
        });
        if let Some(rows) = self
            .report
            .as_mut()
            .and_then(|r| r["results"].as_array_mut())
        {
            for row in rows {
                let evidence = row["evidence_handles"].as_array();
                let recheck = match (&touched, evidence) {
                    (Some(touched), Some(evidence)) if !evidence.is_empty() => {
                        if evidence
                            .iter()
                            .filter_map(Value::as_u64)
                            .any(|h| touched.contains(&h))
                        {
                            Some("needed")
                        } else {
                            None
                        }
                    }
                    _ => Some("maybe"),
                };
                if let Some(recheck) = recheck {
                    if row["recheck"] != "needed" {
                        row["recheck"] = json!(recheck);
                    }
                }
            }
        }
    }
    pub(crate) fn status(&self, lang: crate::prompt::Lang) -> Option<crate::agent::AgentEvent> {
        use crate::prompt::Lang;
        let items: Vec<_> = self.report.as_ref()?["results"]
            .as_array()?
            .iter()
            .filter_map(|r| {
                let text = r["text"].as_str()?;
                let state = r["recheck"].as_str().or(self.dirty.then_some("maybe"));
                let label = match (lang, state, r["passed"] == true) {
                    (Lang::En, Some("needed"), _) => "Needs a recheck",
                    (Lang::Ar, Some("needed"), _) => "يحتاج إعادة فحص",
                    (Lang::En, Some(_), _) => "May need a recheck",
                    (Lang::Ar, Some(_), _) => "قد يحتاج إعادة فحص",
                    (Lang::En, None, false) => "Unmet",
                    (Lang::Ar, None, false) => "لم يتحقق",
                    (_, None, true) => return None,
                };
                Some(format!("{label}: {text}"))
            })
            .collect();
        (!items.is_empty()).then(|| crate::agent::status("requirements", items, lang))
    }
}

pub(crate) fn contract() -> (Value, Value) {
    let verify = qiyas_core::registry::registry()
        .get("verify")
        .unwrap()
        .schema["properties"]["expectations"]["items"]
        .clone();
    (
        json!({"type":"object","additionalProperties":false,"required":["expectations"],"properties":{"expectations":{"type":"array","minItems":1,"maxItems":100,"items":{
        "type":"object","additionalProperties":false,"required":["id","text"],"properties":{
            "id":{"type":"string"},"text":{"type":"string"},"verify":verify,
            "read":{"type":"object","additionalProperties":false,"required":["op","args","pointer","expected"],"properties":{
                "op":{"type":"string","enum":["records","query","state","run","rooms","measure","check","history","artifact"]},"args":{"type":"object"},"pointer":{"type":"string"},"expected":{},"subtract":{},"tolerance":{"type":"number","minimum":0}
            }}}}}}}),
        json!({"expectations":[{"id":"count","text":"The brief asks for four circles","verify":{"kind":"count","entity_type":"Circle","value":4}}]}),
    )
}

#[cfg(test)]
mod watch_tests {
    use super::*;
    use crate::prompt::Lang;

    #[test]
    fn only_touched_evidence_needs_recheck_and_unknown_scope_is_uncertain() {
        let mut watch = Watch::default();
        watch.checked(json!({"results":[
            {"id":"a","text":"Length A","passed":true,"evidence_handles":[10]},
            {"id":"b","text":"Length B","passed":true,"evidence_handles":[20]},
            {"id":"count","text":"Object count","passed":true,"evidence_handles":null}
        ]}));
        watch.changed(Some(&json!({"added":[],"changed":[10],"removed":[]})));
        let value = json!(watch.status(Lang::En).unwrap());
        assert_eq!(
            value["items"],
            json!([
                "Needs a recheck: Length A",
                "May need a recheck: Object count"
            ])
        );
        watch.changed(Some(&json!({"added":[],"changed":[99],"removed":[]})));
        assert_eq!(
            json!(watch.status(Lang::En).unwrap())["items"],
            value["items"]
        );
        watch.changed(None);
        let value = json!(watch.status(Lang::En).unwrap());
        assert_eq!(value["items"][1], "May need a recheck: Length B");
        assert!(json!(watch.status(Lang::Ar).unwrap())["items"][1]
            .as_str()
            .unwrap()
            .starts_with("قد يحتاج"));
        watch.checked(
            json!({"results":[{"id":"a","text":"Length A","passed":true,"evidence_handles":[10]}]}),
        );
        assert!(watch.status(Lang::En).is_none());
        let resumed = Watch::restore(watch.saved());
        assert_eq!(
            json!(resumed.status(Lang::En).unwrap())["items"],
            json!(["May need a recheck: Length A"])
        );
    }
}
