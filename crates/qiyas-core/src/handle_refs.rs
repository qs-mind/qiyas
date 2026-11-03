//! Session references follow replacements and explain removals, including undo and redo.
use crate::{CoreError, Session};
use opencadcodec::Handle;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandleFate {
    pub replacements: Vec<u64>,
    pub step: String,
}

pub(crate) type HandleHistory = BTreeMap<u64, HandleFate>;
pub(crate) type HandleDelta = BTreeMap<u64, Option<HandleFate>>;

pub(crate) fn history_delta(before: &HandleHistory, after: &HandleHistory) -> HandleDelta {
    before
        .keys()
        .chain(after.keys())
        .filter(|h| before.get(*h) != after.get(*h))
        .map(|h| (*h, before.get(h).cloned()))
        .collect()
}

impl Session {
    pub(crate) fn remember_handle_replacements(&mut self, map: &BTreeMap<u64, Vec<u64>>) {
        for (&old, new) in map {
            if new.as_slice() != [old] {
                self.handle_replacements.insert(old, new.clone());
                self.handle_history.insert(
                    old,
                    HandleFate {
                        replacements: new.clone(),
                        step: "rebuild".into(),
                    },
                );
            }
        }
    }

    pub(crate) fn remember_replacements(
        &mut self,
        map: &BTreeMap<u64, Vec<u64>>,
    ) -> Result<(), CoreError> {
        if map.is_empty() {
            return Ok(());
        }
        self.remember_handle_replacements(map);
        let schedules: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                crate::xdata::read_json::<crate::objects::ObjectSchedule>(
                    entity.common(),
                    crate::objects::SCHEDULE_APP,
                )
                .map(|spec| (entity.common().handle, spec))
            })
            .collect();
        for (handle, mut schedule) in schedules {
            let Some(ids) = &mut schedule.ids else {
                continue;
            };
            let previous = ids.clone();
            *ids = ids
                .iter()
                .flat_map(|id| map.get(id).cloned().unwrap_or_else(|| vec![*id]))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            if *ids != previous {
                let app = crate::xdata::app_handle(&self.drawing.doc, crate::objects::SCHEDULE_APP);
                crate::xdata::write_json(
                    self.drawing
                        .doc
                        .get_entity_mut(handle)
                        .expect("schedule")
                        .common_mut(),
                    crate::objects::SCHEDULE_APP,
                    app,
                    &schedule,
                )?;
            }
        }
        Ok(())
    }

    /// Follow a unique replacement chain, or explain a deletion or split.
    pub fn resolve_handle(&self, handle: u64) -> Result<u64, CoreError> {
        let mut current = handle;
        let mut seen = BTreeSet::new();
        while self.drawing.doc.get_entity(Handle::new(current)).is_none() {
            if !seen.insert(current) {
                return Err(CoreError::InvalidCommand(format!(
                    "Replacement cycle for handle {handle}; read wall_groups or records for current elements.")));
            }
            let Some(fate) = self.handle_history.get(&current) else {
                break;
            };
            match fate.replacements.as_slice() {
                [next] => current = *next,
                [] => return Err(CoreError::InvalidCommand(format!(
                    "Handle {handle} (current handle {current}) was erased by step {} without a replacement. Read wall_groups or records to find the current element.", fate.step))),
                replacements => return Err(CoreError::InvalidCommand(format!(
                    "Handle {handle} was replaced by {replacements:?} in step {}. Choose the current element from wall_groups or records.", fate.step))),
            }
        }
        Ok(current)
    }

    /// Resolve literal handles and bound $step values before validation.
    pub fn resolve_input_handles(
        &self,
        input: &mut Value,
    ) -> Result<Vec<crate::validation::InputIssue>, CoreError> {
        let mut warnings = Vec::new();
        let Some(name) = input["type"].as_str() else {
            return Ok(warnings);
        };
        let reg = crate::registry::registry();
        let Some(top) = reg.get(name) else {
            return Ok(warnings);
        };
        let info = input["kind"]
            .as_str()
            .and_then(|kind| reg.get(&format!("{name}.{kind}")))
            .unwrap_or(top);
        self.walk_handle_refs(&info.schema, input, "", "", &mut warnings)?;
        if input["type"] == "set_records"
            && (input["collection"] == "entities" || input["collection"] == "viewports")
        {
            if let Some(updates) = input["updates"].as_array_mut() {
                for (i, update) in updates.iter_mut().enumerate() {
                    self.resolve_handle_value(
                        &mut update["key"],
                        &format!("/updates/{i}/key"),
                        &mut warnings,
                    )?;
                }
            }
        }
        Ok(warnings)
    }

    /// Resolve handles declared by a read or capture schema and return replacement warnings.
    pub fn resolve_schema_handles(
        &self,
        schema: &Value,
        input: &mut Value,
    ) -> Result<Vec<crate::validation::InputIssue>, CoreError> {
        let mut warnings = Vec::new();
        self.walk_handle_refs(schema, input, "", "", &mut warnings)?;
        Ok(warnings)
    }

    fn resolve_handle_value(
        &self,
        value: &mut Value,
        pointer: &str,
        warnings: &mut Vec<crate::validation::InputIssue>,
    ) -> Result<(), CoreError> {
        if let Some(old) = crate::registry::handle_number(value) {
            let new = self.resolve_handle(old)?;
            if old != new {
                *value = json!(new);
                warnings.push(crate::validation::InputIssue {
                    code: "replaced".into(),
                    pointer: pointer.into(),
                    message: format!("Handle {old} was replaced by {new}"),
                    replaced: Some(BTreeMap::from([(old, new)])),
                });
            }
        }
        Ok(())
    }

    fn walk_handle_refs(
        &self,
        schema: &Value,
        value: &mut Value,
        field: &str,
        pointer: &str,
        warnings: &mut Vec<crate::validation::InputIssue>,
    ) -> Result<(), CoreError> {
        if schema["properties"].as_object().is_some_and(|props| {
            props.iter().any(|(key, sub)| {
                value.get(key).is_some_and(|actual| {
                    sub.get("const").is_some_and(|expected| actual != expected)
                        || sub["enum"]
                            .as_array()
                            .is_some_and(|values| !values.contains(actual))
                })
            })
        }) {
            return Ok(());
        }
        if (schema["type"] == "integer"
            || schema["type"]
                .as_array()
                .is_some_and(|types| types.iter().any(|t| t == "integer")))
            && crate::registry::is_handle_field(field)
        {
            return self.resolve_handle_value(value, pointer, warnings);
        }
        for variant in ["anyOf", "oneOf", "allOf"] {
            if let Some(variants) = schema[variant].as_array() {
                for sub in variants {
                    self.walk_handle_refs(sub, value, field, pointer, warnings)?;
                }
            }
        }
        if let Some(items) = value.as_array_mut() {
            for (i, item) in items.iter_mut().enumerate() {
                self.walk_handle_refs(
                    &schema["items"],
                    item,
                    field,
                    &format!("{pointer}/{i}"),
                    warnings,
                )?;
            }
        }
        let foreign = value
            .get("chain")
            .and_then(Value::as_array)
            .is_some_and(|chain| {
                let chain: Vec<_> = chain.iter().filter_map(Value::as_u64).collect();
                crate::session::xref_of_chain(&self.drawing.doc, &chain).is_some()
            });
        if let (Some(map), Some(props)) = (value.as_object_mut(), schema["properties"].as_object())
        {
            for (key, sub) in props {
                if foreign && matches!(key.as_str(), "handle" | "chain") {
                    continue;
                }
                if let Some(value) = map.get_mut(key) {
                    self.walk_handle_refs(sub, value, key, &format!("{pointer}/{key}"), warnings)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Outcome, Source};
    use opencadcodec::{EntityType, Line};

    fn retire(s: &mut Session, old: u64, new: u64) {
        s.record_document_change(Source::Ai, "edit_wall_opening", false, false, |s| {
            s.drawing.doc.remove_entity_arc(Handle::new(old));
            s.remember_replacements(&BTreeMap::from([(old, vec![new])]))
        })
        .unwrap();
    }

    #[test]
    fn logged_opening_handles_resolve_across_commands_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("tower-floor.dxf"), 6);
        // Small before-images stand in for the geometry removed by earlier rebuilds.
        for id in [120, 363] {
            let mut e = EntityType::Line(Line::from_coords(0., 0., 0., 1., 0., 0.));
            e.common_mut().handle = Handle::new(id);
            s.drawing.doc.add_entity(e).unwrap();
        }
        s.run_input(
            &json!({"type":"define_block","name":"DoorOut","base":[0,0],"entities":[
                {"type":"Line","data":{"start":{"x":0,"y":0,"z":0},"end":{"x":1,"y":1,"z":0}}}
            ]}),
            Source::Ai,
        )
        .unwrap();
        let report = s
            .run_input(
                &json!({"type":"walls","runs":[{"points":[[0,0],[20,0]]}],
            "thickness":0.2,"height":3,"openings":[{"run":0,"at":5,"width":1,"kind":"door"}]}),
                Source::Ai,
            )
            .unwrap();
        let Outcome::Walls { result } = report.outcome else {
            panic!("walls")
        };
        let opening = result.inserts[0];
        retire(&mut s, 120, 363);
        retire(&mut s, 363, opening);
        let report = s
            .run_input(
                &json!({"type":"batch","commands":[{"type":"edit_wall_opening","insert":363,"symbol":"DoorOut"}]}),
                Source::Ai,
            )
            .unwrap();
        assert_eq!(
            report.warnings[0].replaced,
            Some(BTreeMap::from([(363, opening)]))
        );
        assert_eq!(report.warnings[0].pointer, "/commands/0/insert");
        let wire = serde_json::to_value(&report).unwrap();
        assert_eq!(wire["warnings"][0]["replaced"]["363"], opening);
        assert_eq!(
            wire["outcomes"][0]["warnings"][0]["replaced"]["363"],
            opening
        );
        let read = s.run_input(&json!({"type":"outside_check","sheet":"Model","opening_handles":[120],"limit":200}), Source::Ai).unwrap();
        assert_eq!(
            read.warnings[0].replaced,
            Some(BTreeMap::from([(120, opening)]))
        );
        assert_eq!(read.warnings[0].pointer, "/opening_handles/0");
        let records = s
            .records(&crate::records::RecordQuery {
                handles: vec![120],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(records.total, 1);
        assert!(records.warnings.iter().any(|w| w.contains("replaced")));
        assert!(s.undo()); // Symbol edit.
        assert!(s.undo()); // Second replacement.
        assert_eq!(s.resolve_handle(120).unwrap(), 363);
        assert!(s.undo()); // First replacement.
        assert_eq!(s.resolve_handle(120).unwrap(), 120);
        assert!(s.redo());
        assert!(s.redo());
        assert_eq!(s.resolve_handle(120).unwrap(), opening);
        let failed = s.transaction(Source::Ai, "failed rebuild", |s| {
            s.run_input(
                &json!({"type":"remove_wall_openings","inserts":[120]}),
                Source::Ai,
            )?;
            Err::<(), _>(CoreError::InvalidCommand("later step failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(s.resolve_handle(120).unwrap(), opening);
        s.run_input(
            &json!({"type":"remove_wall_openings","inserts":[120]}),
            Source::Ai,
        )
        .unwrap();
        let error = s
            .run_input(
                &json!({"type":"outside_check","opening_handles":[120]}),
                Source::Ai,
            )
            .unwrap_err()
            .to_string();
        for text in ["erased", "remove_wall_openings", "wall_groups", "records"] {
            assert!(error.contains(text), "{error}");
        }
        assert!(s.undo());
        assert_eq!(s.resolve_handle(120).unwrap(), opening);
        assert!(s.redo());
        assert!(s.resolve_handle(120).is_err());
    }

    #[test]
    fn undo_reverses_replacements_and_describes_removed_additions() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("tower-floor.dxf"), 6);
        let mut line = EntityType::Line(Line::from_coords(0., 0., 0., 20., 0., 0.));
        line.common_mut().handle = Handle::new(120);
        s.drawing.doc.add_entity(line.clone()).unwrap();
        let new = s
            .record_document_change(Source::Ai, "walls", false, false, |s| {
                s.drawing.doc.remove_entity_arc(Handle::new(120));
                line.common_mut().handle = Handle::new(0);
                let new = s.drawing.doc.add_entity(line).unwrap().value();
                s.remember_replacements(&BTreeMap::from([(120, vec![new])]))?;
                Ok(new)
            })
            .unwrap();
        assert_eq!(s.resolve_handle(120).unwrap(), new);
        s.reload_xrefs();
        assert_eq!(s.resolve_handle(120).unwrap(), new);
        assert!(s.undo());
        assert_eq!(s.resolve_handle(120).unwrap(), 120);
        assert_eq!(s.resolve_handle(new).unwrap(), 120);
        assert!(s.redo());
        assert_eq!(s.resolve_handle(120).unwrap(), new);
        assert_eq!(s.resolve_handle(new).unwrap(), new);
        let report = s
            .run_input(
                &json!({"type":"draw","kind":"line","a":[0,5],"b":[20,5]}),
                Source::Ai,
            )
            .unwrap();
        let Outcome::Added { handles } = report.outcome else {
            panic!("line")
        };
        assert!(s.undo());
        let error = s.resolve_handle(handles[0]).unwrap_err().to_string();
        assert!(
            error.contains("undo draw.line") && error.contains("records"),
            "{error}"
        );
        assert!(s.redo());
        assert_eq!(s.resolve_handle(handles[0]).unwrap(), handles[0]);
    }

    #[test]
    fn split_handles_are_not_silently_assigned_to_one_piece() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("tower-floor.dxf"), 6);
        s.record_document_change(Source::Ai, "walls", false, false, |s| {
            s.remember_replacements(&BTreeMap::from([(120, vec![363, 364])]))
        })
        .unwrap();
        let error = s
            .run_input(
                &json!({"type":"outside_check","opening_handles":[120]}),
                Source::Ai,
            )
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("363") && error.contains("364") && error.contains("wall_groups"),
            "{error}"
        );
        assert!(s.undo());
        assert_eq!(s.resolve_handle(120).unwrap(), 120);
    }
}
