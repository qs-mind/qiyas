//! Shared UTF-8 chunks and application-owned metadata. Mutations invalidate the original
//! DWG bytes for that application so saving cannot silently restore an older value.

use opencadcodec::entities::EntityCommon;
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, CadDocument};
use serde::{de::DeserializeOwned, Serialize};

use crate::CoreError;

/// Leave room below DXF's 255-byte xdata string limit, splitting only between characters.
pub fn string_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut end = rest.len().min(250);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (head, tail) = rest.split_at(end);
        chunks.push(head);
        rest = tail;
    }
    chunks
}

pub fn chunks(text: &str) -> Vec<XDataValue> {
    string_chunks(text)
        .into_iter()
        .map(|s| XDataValue::String(s.to_owned()))
        .collect()
}

pub fn read_text(common: &EntityCommon, app: &str) -> Option<String> {
    Some(
        common
            .extended_data
            .get_record(app)?
            .values
            .iter()
            .filter_map(|v| match v {
                XDataValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect(),
    )
}

pub fn read_json<T: DeserializeOwned>(common: &EntityCommon, app: &str) -> Option<T> {
    serde_json::from_str(&read_text(common, app)?).ok()
}

/// Register an application before mutating its metadata, within the command's table snapshot.
pub fn register(doc: &mut CadDocument, name: &str) -> u64 {
    if !doc.app_ids.contains(name) {
        let mut app = AppId::new(name);
        app.handle = doc.allocate_handle();
        doc.app_ids.add(app).expect("new metadata application");
    }
    app_handle(doc, name)
}

/// Zero means the application is absent and therefore cannot own an original DWG EED blob.
pub fn app_handle(doc: &CadDocument, name: &str) -> u64 {
    doc.app_ids.get(name).map_or(0, |app| app.handle.value())
}

pub fn write_values(
    common: &mut EntityCommon,
    app: &str,
    app_handle: u64,
    values: Option<Vec<XDataValue>>,
) {
    let record = values.map(|values| ExtendedDataRecord {
        application_name: app.to_owned(),
        values,
    });
    if common.extended_data.get_record(app) == record.as_ref() {
        return;
    }
    common.raw_record = None;
    common
        .extended_data
        .raw_dwg_eed
        .retain(|(handle, _)| *handle != app_handle);
    match record {
        Some(record) => common.extended_data.upsert_record(record),
        None => {
            common.extended_data.remove_record(app);
        }
    }
}

pub fn write_text(common: &mut EntityCommon, app: &str, app_handle: u64, text: Option<&str>) {
    write_values(common, app, app_handle, text.map(chunks));
}

pub fn write_json(
    common: &mut EntityCommon,
    app: &str,
    app_handle: u64,
    value: &impl Serialize,
) -> Result<(), CoreError> {
    let text = serde_json::to_string(value)
        .map_err(|e| CoreError::InvalidCommand(format!("Invalid {app} metadata: {e}")))?;
    write_text(common, app, app_handle, Some(&text));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn unicode_chunks_and_metadata_edits_preserve_other_applications() {
        let text = "قياس 🏠 ".repeat(100);
        let parts = string_chunks(&text);
        assert!(parts.iter().all(|s| s.len() <= 250));
        assert_eq!(parts.concat(), text);
        let mut common = EntityCommon::default();
        write_json(&mut common, "TEST", 42, &json!({"text":text})).unwrap();
        common.extended_data.raw_dwg_eed = vec![(42, vec![1]), (43, vec![2])];
        write_json(&mut common, "TEST", 42, &json!({"changed":true})).unwrap();
        assert_eq!(common.extended_data.raw_dwg_eed, [(43, vec![2])]);
        assert_eq!(
            read_json::<Value>(&common, "TEST"),
            Some(json!({"changed":true}))
        );
        common.extended_data.raw_dwg_eed.push((42, vec![3]));
        write_text(&mut common, "TEST", 42, None);
        assert!(read_text(&common, "TEST").is_none());
        assert_eq!(common.extended_data.raw_dwg_eed, [(43, vec![2])]);
    }
}
