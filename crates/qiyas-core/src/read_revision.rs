//! Optimistic reads: a token describes the drawing, knowledge and working context.
use crate::{Change, CoreError, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const KEEP_REVISIONS: usize = 128;
const KEEP_HANDLES: usize = 32_768;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub session: String,
    pub document_revision: u64,
    pub knowledge_revision: u64,
    pub geometry_revision: u64,
    pub selection_revision: u64,
    pub sheet_revision: u64,
    #[serde(skip)]
    knowledge_state: u64,
    pub sheet: String,
    pub context: u64,
}

#[derive(Clone)]
pub(crate) struct RevisionChange {
    before: Revision,
    after: Revision,
    change: Change,
    handle_map: BTreeMap<u64, Vec<u64>>,
    complete: bool,
}

pub(crate) struct RevisionHistory {
    session: String,
    geometry: u64,
    knowledge: u64,
    selection: u64,
    selected: Vec<crate::Target>,
    knowledge_cache: std::sync::Mutex<Option<(u64, u64, u64)>>,
    entries: VecDeque<RevisionChange>,
}

impl Default for RevisionHistory {
    fn default() -> Self {
        Self {
            session: uuid::Uuid::new_v4().to_string(),
            geometry: 0,
            knowledge: 0,
            selection: 0,
            selected: Vec::new(),
            knowledge_cache: std::sync::Mutex::new(None),
            entries: VecDeque::new(),
        }
    }
}

pub fn revision_schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "properties":{"session":{"type":["string","null"]},"document_revision":{"type":["integer","null"],"minimum":0},
        "sheet_revision":{"type":["integer","null"],"minimum":0},"geometry_revision":{"type":["integer","null"],"minimum":0},"selection_revision":{"type":["integer","null"],"minimum":0},
        "knowledge_revision":{"type":["integer","null"],"minimum":0},"sheet":{"type":["string","null"]},"context":{"type":["integer","null"],"minimum":0}}})
}

fn matches(expected: &Value, current: &Revision) -> bool {
    let current = json!(current);
    expected.as_object().is_some_and(|fields| {
        fields
            .iter()
            .all(|(key, value)| value.is_null() || current.get(key) == Some(value))
    })
}

impl Session {
    pub fn revision(&self) -> Revision {
        // Several reads can report the same revision. Hash knowledge once per committed
        // document/history version, without making geometry edits increment its part.
        let knowledge_state = {
            let mut cache = self
                .revision_history
                .knowledge_cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let key = (self.document_revision(), self.history_revision());
            match *cache {
                Some((document, history, fingerprint)) if (document, history) == key => fingerprint,
                _ => {
                    let fingerprint = crate::knowledge::revision_fingerprint(&self.drawing.doc);
                    *cache = Some((key.0, key.1, fingerprint));
                    fingerprint
                }
            }
        };
        // This includes non-entity choices which affect defaults and reads. Knowledge and
        // definitions live in the document; table-only edits also move the history revision.
        let context = json!([
            self.version_of(self.automation_sheet.as_deref().unwrap_or(&self.sheet)),
            self.history_revision(),
            self.history_steps(),
            self.current,
            self.drafting,
            self.ucs,
            self.drawing.units.to_metres(),
            self.drawing.path
        ]);
        let mut hash = 0xcbf29ce484222325u64;
        for byte in context.to_string().bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        let mut sheets = self
            .drawing
            .doc
            .objects
            .iter()
            .filter(|(_, object)| matches!(object, opencadcodec::objects::ObjectType::Layout(_)))
            .map(|(handle, object)| (handle.value(), format!("{object:?}")))
            .collect::<Vec<_>>();
        sheets.sort_by_key(|(handle, _)| *handle);
        let sheet_state = json!([
            self.automation_sheet.as_deref().unwrap_or(&self.sheet),
            self.drawing.sheets(),
            self.annotation_scale(),
            sheets
        ]);
        let sheet_revision = sheet_state
            .to_string()
            .bytes()
            .fold(0xcbf29ce484222325u64, |hash, b| {
                (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
            })
            & ((1 << 53) - 1);
        Revision {
            sheet_revision,
            session: self.revision_history.session.clone(),
            document_revision: self.document_revision(),
            knowledge_revision: self.revision_history.knowledge,
            geometry_revision: self.revision_history.geometry,
            selection_revision: self.revision_history.selection,
            knowledge_state,
            sheet: self
                .automation_sheet
                .as_deref()
                .unwrap_or(&self.sheet)
                .into(),
            context: hash & ((1 << 53) - 1),
        }
    }

    /// Hosts report the live selection independently of drawing edits.
    pub fn set_read_selection(&mut self, targets: &[crate::Target]) {
        if self.revision_history.selected != targets {
            let before = self.revision();
            self.revision_history.selected = targets.to_vec();
            self.revision_history.selection += 1;
            self.record_revision(before);
        }
    }

    /// Hosts call this after a successful operation outside the native command bus.
    pub fn record_tool_revision(&mut self, before: Revision) {
        self.record_revision(before);
    }

    pub(crate) fn record_revision(&mut self, before: Revision) {
        if self.tx_depth != 0 {
            return;
        }
        let mut after = self.revision();
        if before == after
            || self
                .revision_history
                .entries
                .back()
                .is_some_and(|e| e.after == after)
        {
            return;
        }
        if before.knowledge_state != after.knowledge_state {
            self.revision_history.knowledge += 1;
        }
        if before.document_revision != after.document_revision
            && (!self.last_change.added.is_empty()
                || !self.last_change.changed.is_empty()
                || !self.last_change.removed.is_empty())
        {
            self.revision_history.geometry += 1;
        }
        after.knowledge_revision = self.revision_history.knowledge;
        after.geometry_revision = self.revision_history.geometry;
        let drawing_changed = before.document_revision != after.document_revision;
        let mut change = if drawing_changed {
            self.last_change.clone()
        } else {
            Change::default()
        };
        let mut handle_map = if drawing_changed {
            self.handle_map()
        } else {
            BTreeMap::new()
        };
        let mut left = KEEP_HANDLES;
        let mut complete = true;
        for handles in [&mut change.added, &mut change.changed, &mut change.removed] {
            complete &= handles.len() <= left;
            handles.truncate(left);
            left -= handles.len();
        }
        handle_map.retain(|_, handles| {
            if handles.len() + 1 > left {
                complete = false;
                false
            } else {
                left -= handles.len() + 1;
                true
            }
        });
        self.revision_history.entries.push_back(RevisionChange {
            before,
            after,
            change,
            handle_map,
            complete,
        });
        while self.revision_history.entries.len() > KEEP_REVISIONS {
            self.revision_history.entries.pop_front();
        }
    }

    /// Completed commits since a cursor. Batch substeps never appear before their atomic commit.
    /// A missing interval or truncated handle set requires a full reread.
    pub fn events_since(&self, after: u64, session: Option<&str>) -> Value {
        let history = &self.revision_history;
        let oldest = history
            .entries
            .front()
            .map_or(history.sequence + 1, |entry| entry.after.knowledge_revision);
        let entries: Vec<_> = history
            .entries
            .iter()
            .filter(|entry| entry.after.knowledge_revision > after)
            .collect();
        let resync = session.is_some_and(|id| id != history.session)
            || after > history.sequence
            || after.saturating_add(1) < oldest
            || entries.iter().any(|entry| !entry.complete);
        json!({"cursor":history.sequence,"session":history.session,"resync":resync,
            "events":entries.iter().map(|entry| json!({
                "cursor":entry.after.knowledge_revision,"operation":"commit","status":"completed",
                "revision":entry.after,"added":entry.change.added,"changed":entry.change.changed,
                "removed":entry.change.removed,"handle_map":entry.handle_map,
            })).collect::<Vec<_>>()})
    }

    /// All replacement identities from this edit, including retained and removed handles.
    pub fn handle_map(&self) -> BTreeMap<u64, Vec<u64>> {
        let mut map = self.handle_replacements.clone();
        for handle in &self.last_change.changed {
            map.entry(*handle).or_insert_with(|| vec![*handle]);
        }
        for handle in &self.last_change.removed {
            map.entry(*handle).or_default();
        }
        map
    }

    pub fn check_revision(&self, expected: &Value) -> Result<(), CoreError> {
        let report = crate::validation::validate_schema(&revision_schema(), expected);
        if !report.valid || !report.warnings.is_empty() {
            return Err(CoreError::InvalidCommand(format!(
                "expected_revision accepts any subset of the revision parts: {}",
                report.message()
            )));
        }
        let current = self.revision();
        if matches(expected, &current) {
            return Ok(());
        }
        let mut added = BTreeSet::<u64>::new();
        let mut changed = BTreeSet::<u64>::new();
        let mut removed = BTreeSet::<u64>::new();
        let mut maps = Vec::new();
        let mut replacements = BTreeMap::<u64, Vec<u64>>::new();
        let same_session = expected
            .get("session")
            .filter(|v| !v.is_null())
            .is_none_or(|v| v == &json!(current.session));
        let start = same_session
            .then(|| {
                self.revision_history
                    .entries
                    .iter()
                    .position(|e| matches(expected, &e.before))
            })
            .flatten();
        let entries: Vec<_> = start
            .map(|start| self.revision_history.entries.iter().skip(start).collect())
            .unwrap_or_default();
        let complete = start.is_some()
            && entries.iter().all(|e| e.complete)
            && entries
                .windows(2)
                .all(|pair| pair[0].after == pair[1].before)
            && entries.last().is_some_and(|e| e.after == current);
        for entry in entries {
            added.extend(&entry.change.added);
            changed.extend(&entry.change.changed);
            removed.extend(&entry.change.removed);
            for handles in replacements.values_mut() {
                *handles = handles
                    .iter()
                    .flat_map(|h| entry.handle_map.get(h).cloned().unwrap_or_else(|| vec![*h]))
                    .collect();
                handles.sort_unstable();
                handles.dedup();
            }
            for (old, new) in &entry.handle_map {
                replacements.entry(*old).or_insert_with(|| new.clone());
            }
            if !entry.handle_map.is_empty() {
                maps.push(json!({"revision":entry.after,"handle_map":entry.handle_map}));
            }
        }
        let mut components = Vec::new();
        let now = json!(current);
        for (field, part) in [
            ("session", "drawing"),
            ("document_revision", "document"),
            ("geometry_revision", "geometry"),
            ("knowledge_revision", "knowledge"),
            ("sheet", "sheet"),
            ("selection_revision", "selection"),
            ("sheet_revision", "sheet"),
            ("context", "working_context"),
        ] {
            if expected
                .get(field)
                .is_some_and(|v| !v.is_null() && v != &now[field])
                && !components.contains(&part)
            {
                components.push(part);
            }
        }
        Err(CoreError::StaleRead {
            details: json!({"revision":current,"expected_revision":expected,
            "changed_components":components,"added":added,"changed":changed,"removed":removed,
            "handle_map":replacements,"handle_maps":maps,"history_complete":complete,
            "history_limit":KEEP_REVISIONS,
            "next":if complete {"Read the changed handles and use that read's revision."} else {"The retained history is incomplete. Read the drawing again and use its revision."}}),
        })
    }
}
