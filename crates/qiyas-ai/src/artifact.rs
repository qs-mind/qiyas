//! Immutable evidence, kept outside the model's context. Offsets are UTF-8 byte offsets.
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::ToolError;

pub(crate) const PAGE_BYTES: usize = 12_000;

pub(crate) fn hash(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Sort recursively even when another crate enables serde_json's preserve_order feature.
pub(crate) fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), canonical(&map[k])))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
        _ => value.clone(),
    }
}

/// Tool argument objects sometimes contain a JSON-encoded args/command object.
pub(crate) fn arguments(value: &Value) -> Value {
    let mut out = canonical(value);
    if let Some(object) = out.as_object_mut() {
        for key in ["args", "command"] {
            if let Some(text) = object.get(key).and_then(Value::as_str) {
                if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                    object.insert(key.into(), Value::String(arguments(&parsed).to_string()));
                }
            }
        }
    }
    out
}

fn folder() -> Result<PathBuf, ToolError> {
    let path = qiyas_core::data_home::qiyas_dir()
        .map_err(ToolError::Failed)?
        .join("artifacts");
    std::fs::create_dir_all(&path).map_err(failed)?;
    Ok(path)
}

fn failed(error: std::io::Error) -> ToolError {
    ToolError::Failed(format!("Could not keep or read the full evidence: {error}"))
}

pub(crate) fn save(value: &Value) -> Result<String, ToolError> {
    save_in(&folder()?, value)
}

fn save_in(folder: &Path, value: &Value) -> Result<String, ToolError> {
    let data = canonical(value).to_string();
    let id = hash(data.as_bytes());
    let path = folder.join(format!("{id}.json"));
    if path.is_file() {
        return Ok(id);
    }
    let temp = folder.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = (|| {
        let mut file = options.open(&temp)?;
        writeln!(file, "{}", json!({"revision":value["revision"]}))?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(failed(error));
    }
    Ok(id)
}

pub(crate) fn read(args: &Value) -> Result<Value, ToolError> {
    read_in(&folder()?, args)
}

fn read_in(folder: &Path, args: &Value) -> Result<Value, ToolError> {
    let id = args["id"].as_str().unwrap_or_default();
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ToolError::BadInput(
            "Use the artifact id from a result.".into(),
        ));
    }
    let path = folder.join(format!("{id}.json"));
    if std::fs::symlink_metadata(&path)
        .map_err(failed)?
        .file_type()
        .is_symlink()
    {
        return Err(ToolError::BadInput(
            "An evidence file cannot be a link.".into(),
        ));
    }
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut header = String::new();
    std::io::BufReader::new((&mut file).take(4096))
        .read_line(&mut header)
        .map_err(failed)?;
    let metadata: Value = serde_json::from_str(&header)
        .map_err(|_| ToolError::BadInput("The evidence header could not be read.".into()))?;
    let total = file
        .metadata()
        .map_err(failed)?
        .len()
        .saturating_sub(header.len() as u64);
    let offset = args["offset"].as_u64().unwrap_or(0);
    let pointer = args["pointer"]
        .as_str()
        .filter(|p| !p.is_empty() && *p != "/");
    if let Some(pointer) = pointer {
        // One part of the evidence: parse it, take the part, and page that.
        if total > MAX_POINTER_BYTES {
            return Err(ToolError::BadInput(
                "This evidence is too large to read by pointer; page it with offset.".into(),
            ));
        }
        let mut text = String::new();
        file.seek(SeekFrom::Start(header.len() as u64))
            .map_err(failed)?;
        file.read_to_string(&mut text).map_err(failed)?;
        let whole: Value = serde_json::from_str(&text)
            .map_err(|_| ToolError::BadInput("The evidence could not be read.".into()))?;
        let resolved = resolve(&whole, pointer);
        let Some(part) = resolved.value else {
            let mut out = resolved.missing(pointer);
            out["id"] = json!(id);
            out["artifact_revision"] = metadata["revision"].clone();
            return Ok(out);
        };
        let part = part.to_string();
        let mut out = page(&part, offset, id, metadata["revision"].clone())?;
        out["pointer"] = json!(pointer);
        out["resolved_pointer"] = json!(resolved.path);
        out["found"] = json!(true);
        return Ok(out);
    }
    if offset > total {
        return Err(ToolError::BadInput(format!(
            "Offset must be at most {total}."
        )));
    }
    file.seek(SeekFrom::Start(offset + header.len() as u64))
        .map_err(failed)?;
    let mut bytes = Vec::new();
    file.take(PAGE_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    let end = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Err(_) => {
            return Err(ToolError::BadInput(
                "Offset must be a UTF-8 character boundary; use next_offset.".into(),
            ))
        }
    };
    let next = offset + end as u64;
    let data = std::str::from_utf8(&bytes[..end]).expect("checked UTF-8");
    Ok(
        json!({"id":id,"format":"json","offset":offset,"total_bytes":total,
        "next_offset":(next < total).then_some(next),"found":true,"pointer":args["pointer"],"resolved_pointer":"","data":data,"artifact_revision":metadata["revision"],
        "evidence":"Immutable result at artifact_revision. Later edits do not change it. Read current objects before editing."}),
    )
}

// These wrappers occurred in saved results before the shared result envelope.
const WRAPPERS: &[&str] = &["outcome", "data", "result", "report", "groups", "page"];

pub(crate) struct Resolved<'a> {
    pub value: Option<&'a Value>,
    pub path: String,
    deepest: &'a Value,
    wanted: String,
}

impl Resolved<'_> {
    pub(crate) fn missing(&self, pointer: &str) -> Value {
        let available: Vec<String> = match self.deepest {
            Value::Object(map) => map
                .keys()
                .filter(|key| key.len() <= 160)
                .take(40)
                .cloned()
                .collect(),
            Value::Array(items) => (0..items.len().min(40)).map(|i| i.to_string()).collect(),
            _ => Vec::new(),
        };
        let mut suggestions: Vec<_> = available
            .iter()
            .filter_map(|key| {
                let distance = distance(&self.wanted, key);
                (distance <= 3 || key.contains(&self.wanted) || self.wanted.contains(key))
                    .then_some((distance, format!("{}/{}", self.path, escape(key))))
            })
            .collect();
        suggestions.sort();
        json!({"found":false,"pointer":pointer,"deepest_pointer":self.path,
            "available":available,"did_you_mean":suggestions.into_iter().take(5).map(|(_, path)| path).collect::<Vec<_>>()})
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn distance(a: &str, b: &str) -> usize {
    let mut row: Vec<_> = (0..=b.chars().count()).collect();
    for (i, a) in a.chars().take(160).enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in b.chars().enumerate() {
            let old = row[j + 1];
            row[j + 1] = (diagonal + usize::from(a != b))
                .min(row[j] + 1)
                .min(old + 1);
            diagonal = old;
        }
    }
    *row.last().unwrap()
}

fn aliases(key: &str) -> &'static [&'static str] {
    match key {
        "result" | "report" | "groups" | "data" | "page" => {
            &["data", "result", "report", "groups", "page"]
        }
        "errors" | "warnings" => &["warnings", "errors"],
        "summary" | "confidence_summary" => &["confidence_summary", "summary"],
        "counts" | "totals" => &["totals", "counts"],
        "documentation" | "doc" => &["doc", "documentation"],
        _ => &[],
    }
}

/// Exact paths win; only known wrappers and aliases may supply a missing path.
pub(crate) fn resolve<'a>(whole: &'a Value, pointer: &str) -> Resolved<'a> {
    if pointer.is_empty() || pointer == "/" {
        return Resolved {
            value: Some(whole),
            path: String::new(),
            deepest: whole,
            wanted: String::new(),
        };
    }
    if let Some(value) = whole.pointer(pointer) {
        return Resolved {
            value: Some(value),
            path: pointer.into(),
            deepest: value,
            wanted: String::new(),
        };
    }
    let tokens: Vec<_> = pointer
        .strip_prefix('/')
        .unwrap_or(pointer)
        .split('/')
        .map(|key| key.replace("~1", "/").replace("~0", "~"))
        .collect();
    let mut queue = std::collections::VecDeque::from([(whole, String::new(), 0, true)]);
    let mut seen = std::collections::BTreeSet::new();
    let mut best = Resolved {
        value: None,
        path: String::new(),
        deepest: whole,
        wanted: tokens[0].clone(),
    };
    let mut progress = 0;
    while let Some((value, path, index, wrappers)) = queue.pop_front() {
        if !seen.insert((path.clone(), index)) || seen.len() > 1024 {
            continue;
        }
        if index == tokens.len() {
            return Resolved {
                value: Some(value),
                path,
                deepest: value,
                wanted: String::new(),
            };
        }
        if index > progress || (index == progress && path.len() > best.path.len()) {
            progress = index;
            best.path = path.clone();
            best.deepest = value;
            best.wanted = tokens[index].clone();
        }
        let key = &tokens[index];
        let child = |key: &str| value.pointer(&format!("/{}", escape(key)));
        for alias in aliases(key) {
            if *alias != key.as_str() {
                if let Some(next) = child(alias) {
                    queue.push_back((
                        next,
                        format!("{path}/{}", escape(alias)),
                        index + 1,
                        wrappers,
                    ));
                }
            }
        }
        if wrappers {
            // Missing envelope/page segments in old artifacts are optional.
            if WRAPPERS.contains(&key.as_str()) && child(key).is_none() {
                queue.push_back((value, path.clone(), index + 1, true));
            }
            // A model may omit a wrapper in either an old or a current result.
            for wrapper in WRAPPERS.iter().rev() {
                if let Some(next) = child(wrapper) {
                    queue.push_front((next, format!("{path}/{wrapper}"), index, true));
                }
            }
        }
        if let Some(next) = child(key) {
            queue.push_front((
                next,
                format!("{path}/{}", escape(key)),
                index + 1,
                wrappers && WRAPPERS.contains(&key.as_str()),
            ));
        }
    }
    best
}

/// The largest evidence file read whole to take one part of it.
const MAX_POINTER_BYTES: u64 = 64 * 1024 * 1024;

/// One page of `text` from byte `offset`, on a character boundary.
fn page(text: &str, offset: u64, id: &str, revision: Value) -> Result<Value, ToolError> {
    let total = text.len() as u64;
    if offset > total {
        return Err(ToolError::BadInput(format!(
            "Offset must be at most {total}."
        )));
    }
    let start = offset as usize;
    if !text.is_char_boundary(start) {
        return Err(ToolError::BadInput(
            "Offset must be a UTF-8 character boundary; use next_offset.".into(),
        ));
    }
    let mut end = (start + PAGE_BYTES).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"id":id,"format":"json","offset":offset,"total_bytes":total,
        "next_offset":(end < text.len()).then_some(end),"data":&text[start..end],"artifact_revision":revision,
        "evidence":"Immutable result at artifact_revision. Later edits do not change it. Read current objects before editing."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pointers_are_soft_and_show_deepest_keys_and_suggestions() {
        let dir = tempfile::tempdir().unwrap();
        let id = save_in(
            dir.path(),
            &json!({"commands":[{"name":"text","examples":[]}],
            "a/b":{"~key":{"present":1}}}),
        )
        .unwrap();
        for (pointer, deepest, keys) in [
            (
                "/commands/0/example",
                "/commands/0",
                json!(["examples", "name"]),
            ),
            ("/commands/99/example", "/commands", json!(["0"])),
            (
                "/a~1b/~0key/missing/child",
                "/a~1b/~0key",
                json!(["present"]),
            ),
        ] {
            let result = read_in(dir.path(), &json!({"id":id,"pointer":pointer})).unwrap();
            assert_eq!(result["found"], false);
            assert_eq!(result["deepest_pointer"], deepest);
            assert_eq!(result["available"], keys);
        }
        let result = read_in(
            dir.path(),
            &json!({"id":id,"pointer":"/commands/0/example"}),
        )
        .unwrap();
        assert_eq!(result["did_you_mean"], json!(["/commands/0/examples"]));
    }

    #[test]
    fn tower_and_office_failed_artifact_paths_are_readable_or_soft() {
        let dir = tempfile::tempdir().unwrap();
        let kit = json!({"commands":{"measurement.set_method":{"example":{}}},"phases":{},"measurement_rules":[]});
        let quantities = json!({"outcome":{"type":"measurement","result":{
            "items":[{"code":"TOWER-WALL","quantity":12.5}],"confidence_summary":{"review":1},"warnings":["Review source"]}}});
        let records = json!({"collection":"entities","records":[{"key":42,"data":{"radius":2.5}}],"offset":0,"next_offset":null});
        let items = quantities["outcome"]["result"]["items"].clone();
        let warnings = quantities["outcome"]["result"]["warnings"].clone();
        let cases = vec![
            ("tower method", kit.clone(), "/method_example", None),
            (
                "tower items",
                quantities.clone(),
                "/outcome/items",
                Some(items),
            ),
            (
                "tower decisions",
                json!({"outcome":{"type":"measurement","result":{"decisions":[{"name":"height","value":3.2}]}}}),
                "/outcome/decisions",
                Some(json!([{"name":"height","value":3.2}])),
            ),
            (
                "tower report",
                json!({"outcome":{"type":"rooms","report":{"rooms":[{"area":80.0}]}}}),
                "/outcome/result",
                Some(json!({"rooms":[{"area":80.0}]})),
            ),
            (
                "tower documentation",
                json!({"name":"measurement.set_method","doc":"Method syntax","examples":[]}),
                "/documentation",
                Some(json!("Method syntax")),
            ),
            (
                "tower records",
                records.clone(),
                "/page/records",
                Some(records["records"].clone()),
            ),
            (
                "tower warnings",
                quantities.clone(),
                "/outcome/result/errors",
                Some(warnings.clone()),
            ),
            (
                "tower record data",
                records.clone(),
                "/outcome/page/records/0/data",
                Some(json!({"radius":2.5})),
            ),
            (
                "tower absent summary",
                json!({"counts":{"overlap":1},"errors":[],"warnings":[],"findings":[]}),
                "/summary",
                None,
            ),
            (
                "tower repeated warnings",
                quantities.clone(),
                "/outcome/result/errors",
                Some(warnings.clone()),
            ),
            (
                "tower groups",
                json!({"outcome":{"type":"wall_groups","groups":[{"name":"Core","length":42.0}]}}),
                "/outcome/result",
                Some(json!([{"name":"Core","length":42.0}])),
            ),
            (
                "tower wrapped records",
                records.clone(),
                "/outcome/page/records",
                Some(records["records"].clone()),
            ),
            (
                "tower confidence",
                quantities.clone(),
                "/outcome/result/summary",
                Some(json!({"review":1})),
            ),
            (
                "tower result records",
                json!({"outcome":{"type":"records","page":records}}),
                "/outcome/result/records",
                Some(records["records"].clone()),
            ),
            ("office root", kit.clone(), "/", Some(kit)),
            (
                "office report warnings",
                quantities.clone(),
                "/outcome/report/errors",
                Some(warnings.clone()),
            ),
            (
                "office result warnings",
                quantities,
                "/outcome/result/errors",
                Some(warnings),
            ),
            (
                "office totals",
                json!({"outcome":{"type":"measurement","result":{"totals":{"walls":12},"entries":[]}}}),
                "/outcome/result/counts",
                Some(json!({"walls":12})),
            ),
            (
                "office written rows",
                json!({"written":"tower.xlsx","rows":30,"dimension_lines":40}),
                "/outcome",
                Some(json!({"written":"tower.xlsx","rows":30,"dimension_lines":40})),
            ),
            (
                "office written export",
                json!({"written":"tower.pdf","skipped":0}),
                "/outcome",
                Some(json!({"written":"tower.pdf","skipped":0})),
            ),
        ];
        assert_eq!(cases.len(), 20);
        for (name, old, pointer, expected) in cases {
            for canonical in [false, true] {
                let value = if canonical {
                    crate::result_envelope::wrap("run", old.clone())
                } else {
                    old.clone()
                };
                let id = save_in(dir.path(), &value).unwrap();
                let result = read_in(dir.path(), &json!({"id":id,"pointer":pointer})).unwrap();
                if let Some(expected) = &expected {
                    assert_eq!(result["found"], true, "{name}: {result}");
                    let selected: Value =
                        serde_json::from_str(result["data"].as_str().unwrap()).unwrap();
                    let expected = if canonical && pointer == "/" {
                        &value
                    } else if canonical && pointer == "/outcome" {
                        &value["outcome"]
                    } else {
                        expected
                    };
                    assert_eq!(&selected, expected, "{name}, canonical={canonical}");
                } else {
                    assert_eq!(result["found"], false, "{name}: {result}");
                    assert!(!result["available"].as_array().unwrap().is_empty());
                    assert!(result["did_you_mean"].is_array());
                }
            }
        }
    }

    #[test]
    fn exact_fields_win_and_aliases_do_not_walk_into_unrelated_records() {
        let check = crate::result_envelope::wrap("check", json!({"errors":2,"warnings":0}));
        assert_eq!(resolve(&check, "/errors").value, Some(&json!(2)));
        let value = json!({"outcome":{"result":{"errors":["error"],"warnings":["warning"],
            "records":[{"data":{"warnings":["private"]}}]}}});
        assert_eq!(
            resolve(&value, "/outcome/report/errors").value,
            Some(&json!(["error"]))
        );
        assert!(
            resolve(&value, "/outcome/result/records/0/missing/warnings")
                .value
                .is_none()
        );
        for (left, right) in [
            ("counts", "totals"),
            ("summary", "confidence_summary"),
            ("documentation", "doc"),
            ("errors", "warnings"),
        ] {
            for (asked, stored) in [(left, right), (right, left)] {
                let value = json!({(stored):17});
                assert_eq!(
                    resolve(&value, &format!("/{asked}")).value,
                    Some(&json!(17))
                );
            }
        }
    }

    #[test]
    fn pages_reassemble_exact_quantities_and_arabic_and_reject_paths() {
        let dir = tempfile::tempdir().unwrap();
        let evidence = json!({"revision":{"document_revision":7},"quantity":1.23456789012345,
            "items":(0..1000).map(|i| json!({"id":i,"label":"المساحة","quantity":2.9876543210987})).collect::<Vec<_>>()});
        let id = save_in(dir.path(), &evidence).unwrap();
        assert_eq!(save_in(dir.path(), &evidence).unwrap(), id);
        let mut offset = 0;
        let mut whole = String::new();
        loop {
            let page =
                read_in(dir.path(), &json!({"id":id,"offset":offset,"pointer":"/"})).unwrap();
            assert!(page["data"].as_str().unwrap().len() <= PAGE_BYTES);
            whole.push_str(page["data"].as_str().unwrap());
            match page["next_offset"].as_u64() {
                Some(next) => offset = next,
                None => break,
            }
        }
        assert_eq!(serde_json::from_str::<Value>(&whole).unwrap(), evidence);
        let part = read_in(dir.path(), &json!({"id":id,"pointer":"/items/3"})).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(part["data"].as_str().unwrap()).unwrap(),
            evidence["items"][3]
        );
        assert_eq!(
            read_in(dir.path(), &json!({"id":id,"pointer":"/nothing"})).unwrap()["found"],
            false
        );
        assert!(read_in(dir.path(), &json!({"id":"../../settings"})).is_err());
        assert!(read_in(dir.path(), &json!({"id":id,"offset":whole.len()+1})).is_err());
    }
}
