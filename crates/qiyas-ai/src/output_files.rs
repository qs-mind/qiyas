//! Keep written paths in the existing transcript summary, including nested batch results.
use serde_json::{json, Value};

pub(crate) fn summary(value: &Value) -> Option<String> {
    fn collect<'a>(value: &'a Value, paths: &mut Vec<&'a str>) {
        match value {
            Value::Object(fields) => {
                for (key, child) in fields {
                    if key == "written" {
                        match child {
                            Value::String(path) => paths.push(path),
                            Value::Array(list) => {
                                paths.extend(list.iter().filter_map(Value::as_str))
                            }
                            _ => {}
                        }
                    } else {
                        collect(child, paths);
                    }
                }
            }
            Value::Array(list) => {
                for child in list {
                    collect(child, paths);
                }
            }
            _ => {}
        }
    }
    let mut paths = Vec::new();
    collect(value, &mut paths);
    (!paths.is_empty()).then(|| json!({ "written": paths }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_paths_survive_the_transcript_without_copying_the_whole_result() {
        let value = json!({"outcome": {"steps": [
            {"written": "samples/plan.pdf", "message": "Plotted", "skipped": []},
            {"written": ["samples/boq.xlsx", "samples/evidence.pdf"]}
        ]}, "input": {"path": "samples/not-written.pdf"}});
        let kept: Value = serde_json::from_str(&summary(&value).unwrap()).unwrap();
        assert_eq!(
            kept,
            json!({"written": [
                "samples/plan.pdf", "samples/boq.xlsx", "samples/evidence.pdf"
            ]})
        );
        assert!(summary(&json!({"message": "Done", "path": "samples/plan.pdf"})).is_none());
    }
}
