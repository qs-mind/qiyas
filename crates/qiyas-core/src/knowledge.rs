//! Drawing-owned knowledge: NOD / QIYAS_KNOWLEDGE / kind / named JSON XRecord.
//! There are no trade rules here. Consumers validate their own definition schema.

use opencadcodec::objects::{Dictionary, ObjectType, XRecord, XRecordValue};
use opencadcodec::{CadDocument, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::CoreError;

pub const DICTIONARY: &str = "QIYAS_KNOWLEDGE";
pub const VERSION: u32 = 1;
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Types,
    Levels,
    Tables,
    Rules,
    Sheets,
    Mappings,
    Views,
    Measurement,
    Recognition,
    Checks,
    Queries,
    Revisions,
}

impl Kind {
    pub const ALL: [Self; 12] = [
        Self::Types,
        Self::Levels,
        Self::Tables,
        Self::Rules,
        Self::Sheets,
        Self::Mappings,
        Self::Views,
        Self::Measurement,
        Self::Recognition,
        Self::Checks,
        Self::Queries,
        Self::Revisions,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::Levels => "levels",
            Self::Tables => "tables",
            Self::Rules => "rules",
            Self::Sheets => "sheets",
            Self::Mappings => "mappings",
            Self::Views => "views",
            Self::Measurement => "measurement",
            Self::Recognition => "recognition",
            Self::Checks => "checks",
            Self::Queries => "queries",
            Self::Revisions => "revisions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Entry {
    pub name: String,
    pub version: u32,
    pub data: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    data: Value,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn validate_name(name: &str) -> Result<(), CoreError> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.len() > 255
        || name.chars().any(char::is_control)
    {
        return Err(invalid("Knowledge names must be nonempty, trimmed, at most 255 bytes and contain no control characters"));
    }
    Ok(())
}

fn dictionary(doc: &CadDocument, handle: Handle) -> Result<&Dictionary, CoreError> {
    match doc.objects.get(&handle) {
        Some(ObjectType::Dictionary(d)) => Ok(d),
        _ => Err(invalid(format!(
            "Knowledge dictionary {handle:X} is missing or invalid"
        ))),
    }
}

fn child(doc: &CadDocument, parent: Handle, name: &str) -> Result<Option<Handle>, CoreError> {
    let d = dictionary(doc, parent)?;
    let entries: Vec<_> = d
        .entries
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .collect();
    match entries.as_slice() {
        [] => Ok(None),
        [(_, h)] => Ok(Some(*h)),
        _ => Err(invalid(format!(
            "Knowledge dictionary contains duplicate name {name}"
        ))),
    }
}

/// Each dictionary and record belongs to exactly one knowledge name. Accepting aliases
/// would let an edit or deletion change another entry, including one in a different kind.
fn validate_ownership(doc: &CadDocument, root: Handle, knowledge: Handle) -> Result<(), CoreError> {
    if dictionary(doc, root)?
        .entries
        .iter()
        .filter(|(_, handle)| *handle == knowledge)
        .count()
        != 1
    {
        return Err(invalid(
            "The knowledge dictionary has aliases in the named-object dictionary",
        ));
    }
    let knowledge_dictionary = dictionary(doc, knowledge)?;
    if knowledge_dictionary.handle != knowledge || knowledge_dictionary.owner != root {
        return Err(invalid(
            "The knowledge dictionary has an inconsistent handle or owner",
        ));
    }
    let mut kind_names = std::collections::BTreeSet::new();
    let mut kind_handles = std::collections::BTreeSet::new();
    let mut entry_handles = std::collections::BTreeSet::new();
    for (kind, handle) in &knowledge_dictionary.entries {
        validate_name(kind)?;
        if !kind_names.insert(kind.to_ascii_lowercase()) {
            return Err(invalid(format!(
                "Knowledge dictionary contains duplicate name {kind}"
            )));
        }
        if !kind_handles.insert(*handle) {
            return Err(invalid(format!(
                "Knowledge kind {kind} aliases another dictionary"
            )));
        }
        let entries = dictionary(doc, *handle)?;
        if entries.handle != *handle || entries.owner != knowledge {
            return Err(invalid(format!(
                "Knowledge kind {kind} has an inconsistent handle or owner"
            )));
        }
        let mut names = std::collections::BTreeSet::new();
        for (name, entry_handle) in &entries.entries {
            validate_name(name)?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "Knowledge dictionary contains duplicate name {name}"
                )));
            }
            if !entry_handles.insert(*entry_handle) {
                return Err(invalid(format!(
                    "Knowledge entry {name} aliases another XRecord"
                )));
            }
            let Some(ObjectType::XRecord(record)) = doc.objects.get(entry_handle) else {
                return Err(invalid(format!("Knowledge entry {name} is not an XRecord")));
            };
            if record.handle != *entry_handle || record.owner != *handle {
                return Err(invalid(format!(
                    "Knowledge entry {name} has an inconsistent handle or owner"
                )));
            }
        }
    }
    Ok(())
}

fn kind_dictionary(doc: &CadDocument, kind: Kind) -> Result<Option<Handle>, CoreError> {
    let root = doc.header.named_objects_dict_handle;
    if root.is_null() {
        return Ok(None);
    }
    let Some(knowledge) = child(doc, root, DICTIONARY)? else {
        return Ok(None);
    };
    validate_ownership(doc, root, knowledge)?;
    let kind = child(doc, knowledge, kind.as_str())?;
    if let Some(h) = kind {
        dictionary(doc, h)?;
    }
    Ok(kind)
}

fn read_entry(doc: &CadDocument, name: &str, handle: Handle) -> Result<Entry, CoreError> {
    let Some(ObjectType::XRecord(record)) = doc.objects.get(&handle) else {
        return Err(invalid(format!("Knowledge entry {name} is not an XRecord")));
    };
    if !record.entries_complete {
        return Err(invalid(format!(
            "Knowledge entry {name} has an incompletely decoded payload"
        )));
    }
    let mut text = String::new();
    for entry in &record.entries {
        let XRecordValue::String(value) = &entry.value else {
            return Err(invalid(format!(
                "Knowledge entry {name} contains non-string JSON data"
            )));
        };
        if entry.code != 1 || text.len().saturating_add(value.len()) > MAX_BYTES {
            return Err(invalid(format!(
                "Knowledge entry {name} has invalid or oversized JSON chunks"
            )));
        }
        text.push_str(value);
    }
    let envelope: Envelope = serde_json::from_str(&text)
        .map_err(|e| invalid(format!("Knowledge entry {name} contains invalid JSON: {e}")))?;
    if envelope.version != VERSION {
        return Err(invalid(format!(
            "Knowledge entry {name} uses unsupported version {} (expected {VERSION})",
            envelope.version
        )));
    }
    Ok(Entry {
        name: name.to_owned(),
        version: envelope.version,
        data: envelope.data,
    })
}

/// Read one definition without creating dictionaries or changing the drawing.
pub fn get(doc: &CadDocument, kind: Kind, name: &str) -> Result<Option<Entry>, CoreError> {
    validate_name(name)?;
    let Some(dictionary) = kind_dictionary(doc, kind)? else {
        return Ok(None);
    };
    let Some(handle) = child(doc, dictionary, name)? else {
        return Ok(None);
    };
    let stored_name = &self::dictionary(doc, dictionary)?
        .entries
        .iter()
        .find(|(_, h)| *h == handle)
        .expect("named entry")
        .0;
    read_entry(doc, stored_name, handle).map(Some)
}

/// Definitions in a stable order; malformed entries are reported, never silently omitted.
pub fn list(doc: &CadDocument, kind: Kind) -> Result<Vec<Entry>, CoreError> {
    let Some(handle) = kind_dictionary(doc, kind)? else {
        return Ok(Vec::new());
    };
    let d = dictionary(doc, handle)?;
    let mut result = Vec::new();
    for (name, handle) in &d.entries {
        validate_name(name)?;
        child(doc, d.handle, name)?;
        result.push(read_entry(doc, name, *handle)?);
    }
    result.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.name.cmp(&b.name))
    });
    Ok(result)
}

fn add_dictionary(doc: &mut CadDocument, owner: Handle, name: &str) -> Handle {
    let mut dictionary = Dictionary::new();
    dictionary.handle = doc.allocate_handle();
    dictionary.owner = owner;
    dictionary.hard_owner = true;
    let handle = dictionary.handle;
    doc.objects
        .insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(parent)) = doc.objects.get_mut(&owner) {
        parent.add_entry(name, handle);
        parent.set_entry_hard_owner(name, true);
    }
    handle
}

fn ensure_kind_dictionary(doc: &mut CadDocument, kind: Kind) -> Result<Handle, CoreError> {
    let mut root = doc.header.named_objects_dict_handle;
    if root.is_null() {
        root = add_dictionary(doc, Handle::NULL, "");
        doc.header.named_objects_dict_handle = root;
    }
    let knowledge = match child(doc, root, DICTIONARY)? {
        Some(h) => {
            dictionary(doc, h)?;
            h
        }
        None => add_dictionary(doc, root, DICTIONARY),
    };
    // Validate the existing graph before adding missing kinds.
    for k in Kind::ALL {
        if let Some(h) = child(doc, knowledge, k.as_str())? {
            dictionary(doc, h)?;
        }
    }
    for k in Kind::ALL {
        if child(doc, knowledge, k.as_str())?.is_none() {
            add_dictionary(doc, knowledge, k.as_str());
        }
    }
    Ok(child(doc, knowledge, kind.as_str())?.expect("created kind dictionary"))
}

/// Insert or replace a definition. Call within a document change with a tables snapshot.
pub fn put(doc: &mut CadDocument, kind: Kind, name: &str, data: &Value) -> Result<(), CoreError> {
    validate_name(name)?;
    let text = serde_json::to_string(&Envelope {
        version: VERSION,
        data: data.clone(),
    })
    .map_err(|e| invalid(format!("Invalid knowledge JSON: {e}")))?;
    if text.len() > MAX_BYTES {
        return Err(invalid("Knowledge entry exceeds 4 MiB"));
    }
    // An existing corrupt record must not be overwritten without being reviewed.
    get(doc, kind, name)?;
    let owner = ensure_kind_dictionary(doc, kind)?;
    let handle = match child(doc, owner, name)? {
        Some(h) => h,
        None => {
            let h = doc.allocate_handle();
            let mut record = XRecord::named(name);
            record.handle = h;
            record.owner = owner;
            doc.objects.insert(h, ObjectType::XRecord(record));
            if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
                d.add_entry(name, h);
            }
            h
        }
    };
    let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&handle) else {
        unreachable!("validated XRecord")
    };
    record.entries.clear();
    record.raw_dwg_data = None;
    record.raw_dwg_version = None;
    record.raw_dwg_handle_bits = 0;
    record.raw_data.clear();
    record.object_references.clear();
    record.preserve_object_reference_stream = false;
    record.entries_complete = true;
    for chunk in crate::xdata::string_chunks(&text) {
        record.add_string(1, chunk);
    }
    Ok(())
}

/// Remove one definition and its owned XRecord; retain the collection dictionaries.
pub fn remove(doc: &mut CadDocument, kind: Kind, name: &str) -> Result<bool, CoreError> {
    validate_name(name)?;
    let Some(owner) = kind_dictionary(doc, kind)? else {
        return Ok(false);
    };
    let Some(handle) = child(doc, owner, name)? else {
        return Ok(false);
    };
    read_entry(doc, name, handle)?;
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&owner) {
        d.entries.retain(|(_, h)| *h != handle);
        d.hard_owner_entries
            .retain(|key| !key.eq_ignore_ascii_case(name));
    }
    doc.objects.remove(&handle);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, Source};
    use serde_json::json;

    #[test]
    fn every_collection_round_trips_and_edits_discard_original_dwg_bytes() {
        for extension in ["dwg", "dxf"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(format!("knowledge.{extension}"));
            let mut s = Session::new_blank(&path, 6);
            for kind in Kind::ALL {
                put(
                    &mut s.drawing.doc,
                    kind,
                    "تعريف",
                    &json!({"text":"قياس ".repeat(200), "kind":kind.as_str()}),
                )
                .unwrap();
            }
            s.save_as(&path).unwrap();
            let mut s = Session::open(&path).unwrap();
            for kind in Kind::ALL {
                let entry = get(&s.drawing.doc, kind, "تعريف").unwrap().unwrap();
                assert_eq!(entry.data["text"], "قياس ".repeat(200));
                put(&mut s.drawing.doc, kind, "تعريف", &json!({"changed":true})).unwrap();
            }
            s.save().unwrap();
            let s = Session::open(&path).unwrap();
            for kind in Kind::ALL {
                assert_eq!(
                    list(&s.drawing.doc, kind).unwrap()[0].data,
                    json!({"changed":true})
                );
            }
        }
    }

    #[test]
    fn dictionary_and_records_follow_one_undo_step_and_failed_mutations_roll_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("undo.dxf"), 6);
        let before = s.drawing.doc.objects.clone();
        s.record_document_change(Source::User, "knowledge", true, false, |s| {
            put(
                &mut s.drawing.doc,
                Kind::Types,
                "example",
                &json!({"height":3}),
            )
        })
        .unwrap();
        assert_eq!(s.history_steps().0.len(), 1);
        assert!(s.undo());
        assert_eq!(s.drawing.doc.objects, before);
        assert!(get(&s.drawing.doc, Kind::Types, "example")
            .unwrap()
            .is_none());
        assert!(s.redo());
        assert_eq!(
            get(&s.drawing.doc, Kind::Types, "EXAMPLE")
                .unwrap()
                .unwrap()
                .data["height"],
            3
        );
        let result: Result<(), CoreError> =
            s.record_document_change(Source::User, "failed", true, false, |s| {
                remove(&mut s.drawing.doc, Kind::Types, "example")?;
                Err(invalid("stop"))
            });
        assert!(result.is_err());
        assert!(get(&s.drawing.doc, Kind::Types, "example")
            .unwrap()
            .is_some());
    }

    #[test]
    fn unknown_versions_and_broken_records_are_reported_without_mutation() {
        let mut doc = CadDocument::new();
        assert!(put(&mut doc, Kind::Types, " bad ", &json!({})).is_err());
        put(&mut doc, Kind::Types, "test", &json!({})).unwrap();
        let owner = kind_dictionary(&doc, Kind::Types).unwrap().unwrap();
        let handle = child(&doc, owner, "test").unwrap().unwrap();
        if let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&handle) {
            record.entries.clear();
            record.add_string(1, r#"{"version":999,"data":{}}"#);
        }
        let before = doc.objects.clone();
        assert!(list(&doc, Kind::Types)
            .unwrap_err()
            .to_string()
            .contains("unsupported version"));
        assert!(put(&mut doc, Kind::Types, "test", &json!({"replace":true})).is_err());
        assert_eq!(doc.objects, before);
    }

    #[test]
    fn aliases_and_wrong_owners_are_refused_without_changing_knowledge() {
        let mut original = CadDocument::new();
        put(&mut original, Kind::Types, "first", &json!({"value":1})).unwrap();
        let owner = kind_dictionary(&original, Kind::Types).unwrap().unwrap();
        let entry = child(&original, owner, "first").unwrap().unwrap();
        let root = original.header.named_objects_dict_handle;
        let knowledge = child(&original, root, DICTIONARY).unwrap().unwrap();

        for corruption in [
            "entry_alias",
            "kind_alias",
            "root_alias",
            "entry_owner",
            "kind_owner",
            "knowledge_owner",
        ] {
            let mut doc = original.clone();
            match corruption {
                "entry_alias" => {
                    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&owner)
                    else {
                        panic!("kind dictionary")
                    };
                    dictionary.add_entry("second", entry);
                }
                "kind_alias" => {
                    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&knowledge)
                    else {
                        panic!("knowledge dictionary")
                    };
                    dictionary.add_entry("another_kind", owner);
                }
                "root_alias" => {
                    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&root)
                    else {
                        panic!("named-object dictionary")
                    };
                    dictionary.add_entry("OTHER_APPLICATION", knowledge);
                }
                "entry_owner" => {
                    let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&entry) else {
                        panic!("XRecord")
                    };
                    record.owner = knowledge;
                }
                "kind_owner" => {
                    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&owner)
                    else {
                        panic!("kind dictionary")
                    };
                    dictionary.owner = root;
                }
                "knowledge_owner" => {
                    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&knowledge)
                    else {
                        panic!("knowledge dictionary")
                    };
                    dictionary.owner = Handle::NULL;
                }
                _ => unreachable!(),
            }
            let before = doc.objects.clone();
            assert!(get(&doc, Kind::Types, "first").is_err(), "{corruption}");
            assert!(list(&doc, Kind::Types).is_err(), "{corruption}");
            assert!(
                put(&mut doc, Kind::Types, "first", &json!({"value":2})).is_err(),
                "{corruption}"
            );
            assert!(
                put(&mut doc, Kind::Levels, "unrelated", &json!({"elevation":2})).is_err(),
                "{corruption}"
            );
            assert!(
                remove(&mut doc, Kind::Types, "first").is_err(),
                "{corruption}"
            );
            assert_eq!(doc.objects, before, "{corruption}");
        }
    }
}
