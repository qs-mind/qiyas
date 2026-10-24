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
    sequence: u64,
    entries: VecDeque<RevisionChange>,
}

impl Default for RevisionHistory {
    fn default() -> Self {
        Self {
            session: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            entries: VecDeque::new(),
        }
    }
}

pub fn revision_schema() -> Value {
    json!({"type":"object","additionalProperties":false,
        "required":["session","document_revision","knowledge_revision","sheet","context"],
        "properties":{"session":{"type":"string"},"document_revision":{"type":"integer","minimum":0},
        "knowledge_revision":{"type":"integer","minimum":0},"sheet":{"type":"string"},"context":{"type":"integer","minimum":0}}})
}

impl Session {
    pub fn revision(&self) -> Revision {
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
        Revision {
            session: self.revision_history.session.clone(),
            document_revision: self.document_revision(),
            knowledge_revision: self.revision_history.sequence,
            sheet: self
                .automation_sheet
                .as_deref()
                .unwrap_or(&self.sheet)
                .into(),
            context: hash & ((1 << 53) - 1),
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
        self.revision_history.sequence += 1;
        after.knowledge_revision = self.revision_history.sequence;
        let mut change = self.last_change.clone();
        let mut handle_map = self.handle_map();
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
        let expected: Revision = serde_json::from_value(expected.clone()).map_err(|e| {
            CoreError::InvalidCommand(format!(
                "expected_revision must be the complete revision from a read: {e}"
            ))
        })?;
        let current = self.revision();
        if expected == current {
            return Ok(());
        }
        let mut added = BTreeSet::<u64>::new();
        let mut changed = BTreeSet::<u64>::new();
        let mut removed = BTreeSet::<u64>::new();
        let mut maps = Vec::new();
        let mut replacements = BTreeMap::<u64, Vec<u64>>::new();
        let same_session = expected.session == current.session;
        let entries: Vec<_> = self
            .revision_history
            .entries
            .iter()
            .filter(|e| same_session && e.after.knowledge_revision > expected.knowledge_revision)
            .collect();
        let complete = entries.first().is_some_and(|e| e.before == expected)
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
        if !same_session {
            components.push("drawing");
        }
        if expected.document_revision != current.document_revision {
            components.push("document");
        }
        if expected.knowledge_revision != current.knowledge_revision {
            components.push("knowledge_or_tables");
        }
        if expected.sheet != current.sheet {
            components.push("sheet");
        }
        if expected.context != current.context {
            components.push("working_context");
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
