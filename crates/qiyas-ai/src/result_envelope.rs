//! One wire shape for model replies and their immutable evidence.
use serde_json::{json, Value};

const METADATA: &[&str] = &[
    "advice",
    "warnings",
    "defaults_used",
    "revision",
    "artifact",
    "ui",
];

pub(crate) fn is_envelope(value: &Value) -> bool {
    matches!(value["type"].as_str(), Some("result" | "error"))
        && value["outcome"].get("data").is_some()
        && value["outcome"]["type"].is_string()
}

pub(crate) fn wrap(op: &str, mut value: Value) -> Value {
    if is_envelope(&value) {
        return value;
    }
    let mut out = json!({"type":if op == "error" {"error"} else {"result"},
        "advice":[],"warnings":[],"defaults_used":[],"revision":null});
    let mut outcome = value.as_object_mut().and_then(|map| map.remove("outcome"));
    for key in METADATA {
        let outer = take_metadata(&mut value, key);
        let inner = outcome.as_mut().and_then(|value| take_metadata(value, key));
        let metadata = match (outer, inner) {
            (Some(Value::Array(mut outer)), Some(Value::Array(inner))) => {
                for item in inner {
                    if !outer.contains(&item) {
                        outer.push(item);
                    }
                }
                Some(Value::Array(outer))
            }
            (outer, inner) => outer.or(inner),
        };
        if let Some(metadata) = metadata {
            out[*key] = metadata;
        }
    }
    if let Some(mut outcome) = outcome {
        let kind = outcome
            .as_object_mut()
            .and_then(|map| map.remove("type"))
            .filter(Value::is_string)
            .unwrap_or_else(|| json!(op));
        let payload = ["data", "result", "report", "groups", "page"]
            .into_iter()
            .find_map(|key| outcome.as_object_mut().and_then(|map| map.remove(key)));
        out["outcome"] = if let Some(data) = payload {
            outcome["type"] = kind;
            outcome["data"] = data;
            outcome
        } else {
            json!({"type":kind,"data":outcome})
        };
        // Edit bookkeeping (handles, checks and UI routing) keeps its established address.
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                if METADATA.contains(&key.as_str()) {
                    out["outcome"][key] = value.clone();
                } else if key != "type" {
                    out[key] = value.clone();
                }
            }
        }
    } else {
        let kind = value
            .as_object_mut()
            .and_then(|map| map.remove("type"))
            .filter(Value::is_string)
            .unwrap_or_else(|| json!(op));
        out["outcome"] = json!({"type":kind,"data":value});
    }
    out
}

pub(crate) fn image(caption: &str) -> Value {
    let (text, revision) = caption
        .rsplit_once("\nrevision: ")
        .and_then(|(text, revision)| {
            serde_json::from_str::<Value>(revision)
                .ok()
                .map(|revision| (text, revision))
        })
        .unwrap_or((caption, Value::Null));
    wrap("capture", json!({"caption":text,"revision":revision}))
}

fn take_metadata(value: &mut Value, key: &str) -> Option<Value> {
    if matches!(key, "advice" | "warnings" | "defaults_used") && !value[key].is_array() {
        return None;
    }
    value.as_object_mut().and_then(|map| map.remove(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_counts_stay_in_the_payload_and_capture_revision_is_structured() {
        let check = wrap("check", json!({"warnings":2,"errors":1}));
        assert_eq!(check["warnings"], json!([]));
        assert_eq!(check["outcome"]["data"]["warnings"], 2);
        let capture = image("Tower floor bounds\nrevision: {\"document_revision\":7}");
        assert_eq!(capture["revision"], json!({"document_revision":7}));
        assert_eq!(capture["outcome"]["data"]["caption"], "Tower floor bounds");
    }

    #[test]
    fn legacy_report_groups_and_records_share_one_payload_address() {
        for (field, payload) in [
            ("result", json!({"items":[{"quantity":1.23456789012345}]})),
            ("report", json!({"rooms":[{"area":80.0}]})),
            ("groups", json!([{"length":42.0}])),
            (
                "page",
                json!({"records":[{"key":42,"data":{"radius":2.5}}]}),
            ),
            ("data", json!({"objects":[]})),
        ] {
            let raw = json!({"revision":{"document_revision":7},"added":[42],"outcome":{
                "type":"measurement",(field):payload,"handle_map":{"42":"A"},
                "warnings":[{"code":"default"}],"defaults_used":[],"advice":[]}});
            let wire = wrap("run", raw);
            assert_eq!(wire["type"], "result");
            assert_eq!(wire["outcome"]["type"], "measurement");
            assert_eq!(wire["outcome"]["data"], payload);
            assert_eq!(wire["outcome"]["handle_map"], json!({"42":"A"}));
            assert_eq!(wire["revision"], json!({"document_revision":7}));
            assert_eq!(wire["added"], json!([42]));
            assert_eq!(wire["warnings"], json!([{"code":"default"}]));
            assert_eq!(wrap("run", wire.clone()), wire);
        }
    }

    #[test]
    fn direct_record_pages_and_wrapped_record_pages_have_identical_payloads() {
        let page = json!({"records":[{"key":42,"data":{"radius":2.5}}],"next_offset":null});
        let direct = wrap("records", page.clone());
        let wrapped = wrap("run", json!({"outcome":{"type":"records","page":page}}));
        assert_eq!(direct, wrapped);
    }
}
