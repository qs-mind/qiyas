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

/// JSON with every non-ASCII character as a `\uXXXX` escape. DWG 2000 and 2004 store text
/// in a code page and write other characters as `\U+XXXX`, which a JSON reader refuses;
/// ASCII JSON reads back the same in every DWG version and in DXF.
pub fn ascii_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut units = [0u16; 2];
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

/// Turn the `\U+XXXX` escapes that DWG 2000 and 2004 leave in stored text back into
/// characters (surrogate pairs joined). An escape after an odd run of backslashes is a
/// literal backslash followed by text, and stays as it is.
pub fn decode_legacy_unicode(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("\\U+") {
        return std::borrow::Cow::Borrowed(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let unit_at = |i: usize| -> Option<u16> {
        if i + 7 > chars.len() || chars[i] != '\\' || chars[i + 1] != 'U' || chars[i + 2] != '+' {
            return None;
        }
        let hex: String = chars[i + 3..i + 7].iter().collect();
        u16::from_str_radix(&hex, 16).ok()
    };
    let mut out = String::with_capacity(text.len());
    let mut slashes = 0usize;
    let mut i = 0;
    while i < chars.len() {
        if slashes.is_multiple_of(2) {
            if let Some(unit) = unit_at(i) {
                let mut next = i + 7;
                let decoded = if (0xD800..0xDC00).contains(&unit) {
                    match unit_at(next).filter(|low| (0xDC00..0xE000).contains(low)) {
                        Some(low) => {
                            next += 7;
                            char::decode_utf16([unit, low]).next().and_then(Result::ok)
                        }
                        None => None,
                    }
                } else {
                    char::from_u32(u32::from(unit))
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i = next;
                    slashes = 0;
                    continue;
                }
            }
        }
        slashes = if chars[i] == '\\' { slashes + 1 } else { 0 };
        out.push(chars[i]);
        i += 1;
    }
    std::borrow::Cow::Owned(out)
}

pub fn chunks(text: &str) -> Vec<XDataValue> {
    string_chunks(text)
        .into_iter()
        .map(|s| XDataValue::String(s.to_owned()))
        .collect()
}

pub fn read_text(common: &EntityCommon, app: &str) -> Option<String> {
    let text: String = common
        .extended_data
        .get_record(app)?
        .values
        .iter()
        .filter_map(|v| match v {
            XDataValue::String(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    Some(decode_legacy_unicode(&text).into_owned())
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
    write_text(common, app, app_handle, Some(&ascii_json(&text)));
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
