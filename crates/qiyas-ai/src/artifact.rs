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
    let pointer = args["pointer"].as_str().filter(|p| !p.is_empty());
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
        let Some(part) = whole.pointer(pointer) else {
            let keys = |value: &Value| -> String {
                match value {
                    Value::Object(map) => {
                        map.keys().take(40).cloned().collect::<Vec<_>>().join(", ")
                    }
                    Value::Array(items) => format!("indices 0..{} (exclusive)", items.len()),
                    _ => "(no child keys)".into(),
                }
            };
            let mut deepest = pointer;
            while whole.pointer(deepest).is_none() {
                deepest = deepest.rsplit_once('/').map_or("", |(parent, _)| parent);
            }
            return Err(ToolError::BadInput(format!(
                "The evidence has nothing at {pointer}. Top level: {}. Keys at {}: {}",
                keys(&whole),
                if deepest.is_empty() {
                    "(root)"
                } else {
                    deepest
                },
                keys(whole.pointer(deepest).expect("existing pointer"))
            )));
        };
        let part = part.to_string();
        let mut out = page(&part, offset, id, metadata["revision"].clone())?;
        out["pointer"] = json!(pointer);
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
        "next_offset":(next < total).then_some(next),"data":data,"artifact_revision":metadata["revision"],
        "evidence":"Immutable result at artifact_revision. Later edits do not change it. Read current objects before editing."}),
    )
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
    fn missing_pointers_show_the_deepest_existing_keys_including_escaped_paths() {
        let dir = tempfile::tempdir().unwrap();
        let id = save_in(
            dir.path(),
            &json!({"commands":[{"name":"text","examples":[]}],
            "a/b":{"~key":{"present":1}}}),
        )
        .unwrap();
        for (pointer, expected) in [
            ("/commands/0/example", "Keys at /commands/0: examples, name"),
            ("/commands/99/example", "Keys at /commands: indices 0..1"),
            ("/a~1b/~0key/missing/child", "Keys at /a~1b/~0key: present"),
        ] {
            let error = read_in(dir.path(), &json!({"id":id,"pointer":pointer}))
                .unwrap_err()
                .to_string();
            assert!(error.contains("Top level: a/b, commands"), "{error}");
            assert!(error.contains(expected), "{error}");
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
            let page = read_in(dir.path(), &json!({"id":id,"offset":offset})).unwrap();
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
        assert!(read_in(dir.path(), &json!({"id":id,"pointer":"/nothing"}))
            .unwrap_err()
            .to_string()
            .contains("Top level"));
        assert!(read_in(dir.path(), &json!({"id":"../../settings"})).is_err());
        assert!(read_in(dir.path(), &json!({"id":id,"offset":whole.len()+1})).is_err());
    }
}
