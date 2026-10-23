//! References within a transaction follow replacements made by earlier steps.
use crate::{CoreError, Session};
use opencadcodec::Handle;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

impl Session {
    pub(crate) fn remember_replacements(
        &mut self,
        map: &BTreeMap<u64, Vec<u64>>,
    ) -> Result<(), CoreError> {
        if map.is_empty() {
            return Ok(());
        }
        for (&old, new) in map {
            if new.as_slice() != [old] {
                self.handle_replacements.insert(old, new.clone());
            }
        }
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

    fn current_handle(&self, handle: u64) -> Result<u64, CoreError> {
        let mut current = handle;
        let mut seen = BTreeSet::new();
        while self.drawing.doc.get_entity(Handle::new(current)).is_none() && seen.insert(current) {
            match self.handle_replacements.get(&current).map(Vec::as_slice) {
                Some([next]) => current = *next,
                Some(replacements) => return Err(CoreError::InvalidCommand(format!(
                    "Entity {handle} was replaced by {replacements:?} earlier in this batch. Choose one of the current entities explicitly."))),
                None => break,
            }
        }
        Ok(current)
    }

    /// Resolve both literal handles and already-bound $step values before validation.
    pub fn resolve_input_handles(&self, input: &mut Value) -> Result<(), CoreError> {
        if self.tx_depth == 0 || self.handle_replacements.is_empty() {
            return Ok(());
        }
        let Some(name) = input["type"].as_str() else {
            return Ok(());
        };
        let reg = crate::registry::registry();
        let Some(top) = reg.get(name) else {
            return Ok(());
        };
        let info = input["kind"]
            .as_str()
            .and_then(|kind| reg.get(&format!("{name}.{kind}")))
            .unwrap_or(top);
        self.walk_handle_refs(&info.schema, input, "")?;
        if input["type"] == "set_records" && input["collection"] == "entities" {
            if let Some(updates) = input["updates"].as_array_mut() {
                for update in updates {
                    if let Some(handle) = crate::registry::handle_number(&update["key"]) {
                        update["key"] = json!(self.current_handle(handle)?);
                    }
                }
            }
        }
        Ok(())
    }

    fn walk_handle_refs(
        &self,
        schema: &Value,
        value: &mut Value,
        field: &str,
    ) -> Result<(), CoreError> {
        if schema["type"] == "integer" && crate::registry::is_handle_field(field) {
            if let Some(handle) = crate::registry::handle_number(value) {
                *value = json!(self.current_handle(handle)?);
            }
            return Ok(());
        }
        for variant in ["anyOf", "oneOf", "allOf"] {
            if let Some(variants) = schema[variant].as_array() {
                for sub in variants {
                    self.walk_handle_refs(sub, value, field)?;
                }
            }
        }
        if let Some(items) = value.as_array_mut() {
            for item in items {
                self.walk_handle_refs(&schema["items"], item, field)?;
            }
        }
        if let Some(map) = value.as_object_mut() {
            if let Some(props) = schema["properties"].as_object() {
                for (key, sub) in props {
                    if let Some(value) = map.get_mut(key) {
                        self.walk_handle_refs(sub, value, key)?;
                    }
                }
            }
        }
        Ok(())
    }
}
