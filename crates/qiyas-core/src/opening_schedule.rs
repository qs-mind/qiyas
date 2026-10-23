//! Compatibility aliases for the opening schedule data recipe and old drawing field codes.

use std::collections::BTreeSet;

use opencadcodec::entities::Table;
use opencadcodec::xdata::XDataValue;
use opencadcodec::{EntityType, Handle};

use crate::object_schedules::legacy_recipe;
use crate::objects::ObjectFilter;
use crate::{CoreError, Session};

pub(crate) const SCHEDULE_APP: &str = "QIYAS_OPENING_SCHEDULE";
pub(crate) const MARK_APP: &str = "QIYAS_OPENING_MARK";
pub(crate) const ID_SOURCE_APP: &str = "QIYAS_OPENING_ID_SOURCE";

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

pub(crate) fn mark(entity: &EntityType) -> Option<String> {
    let record = entity.common().extended_data.get_record(MARK_APP)?;
    record.values.iter().find_map(|v| match v {
        XDataValue::String(s) => Some(s.clone()),
        _ => None,
    })
}

pub(crate) fn source(table: &Table) -> Option<Handle> {
    let record = table.common.extended_data.get_record(SCHEDULE_APP)?;
    record.values.iter().find_map(|v| match v {
        XDataValue::String(s) => u64::from_str_radix(s, 16).ok().map(Handle::new),
        _ => None,
    })
}

impl Session {
    pub(crate) fn opening_count(&self, source: Handle, key: &str) -> Option<usize> {
        let sheet = self
            .drawing
            .doc
            .block_records
            .iter()
            .find(|b| b.handle == source)?
            .name
            .clone();
        let mut spec = legacy_recipe("opening").ok()?;
        spec.filter.sheet = Some(sheet);
        let objects = self.objects(&spec.filter).ok()?;
        let mut count = 0;
        for record in objects {
            let mark = self.schedule_value(&spec, &record, "mark").ok()??;
            if key == "*" || mark.as_str() == Some(key) {
                count += 1;
            }
        }
        Some(count)
    }

    fn mark_openings(&mut self, source: Handle) -> Result<(), CoreError> {
        let sheet = self
            .drawing
            .doc
            .block_records
            .iter()
            .find(|b| b.handle == source)
            .ok_or_else(|| invalid("Opening schedule source sheet no longer exists"))?
            .name
            .clone();
        let mut spec = legacy_recipe("opening")?;
        spec.filter.sheet = Some(sheet);
        let objects = self.objects(&spec.filter)?;
        if objects.is_empty() {
            return Ok(());
        }
        let app = self.object_app(MARK_APP)?;
        for mut record in objects {
            let handle = record.instance.id;
            let entity = self
                .drawing
                .doc
                .get_entity(Handle::new(handle))
                .expect("object driver");
            let legacy_default = entity
                .common()
                .extended_data
                .get_record(ID_SOURCE_APP)
                .is_none()
                && mark(entity).is_some_and(|old| {
                    record
                        .instance
                        .props
                        .get("mark")
                        .and_then(serde_json::Value::as_str)
                        == Some(old.as_str())
                });
            if legacy_default {
                // Previous versions wrote the size label into the editable ID. Keep that
                // value automatic; explicit edits carry their own source marker now.
                record
                    .instance
                    .props
                    .insert("mark".into(), serde_json::json!(""));
                self.stamp_object(handle, &record.instance)?;
            }
            let source_app = self.object_app(ID_SOURCE_APP)?;
            crate::xdata::write_text(
                self.drawing
                    .doc
                    .get_entity_mut(Handle::new(handle))
                    .expect("object driver")
                    .common_mut(),
                ID_SOURCE_APP,
                source_app,
                Some("explicit_or_empty"),
            );
            let value = self
                .schedule_value(&spec, &record, "mark")?
                .ok_or_else(|| invalid("Opening schedule recipe requires a mark"))?;
            let mark = value
                .as_str()
                .ok_or_else(|| invalid("Opening mark template must be text"))?;
            let handle = record.instance.id;
            let entity = self
                .drawing
                .doc
                .get_entity_mut(Handle::new(handle))
                .expect("object driver");
            if crate::opening_schedule::mark(entity).as_deref() != Some(mark) {
                crate::xdata::write_text(entity.common_mut(), MARK_APP, app, Some(mark));
            }
        }
        Ok(())
    }

    pub(crate) fn add_opening_schedule(
        &mut self,
        at: [f64; 2],
        sheet: Option<&str>,
    ) -> Result<u64, CoreError> {
        let name = sheet.unwrap_or(if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            &self.sheet
        } else {
            "*Model_Space"
        });
        let source = self
            .drawing
            .doc
            .block_records
            .get(name)
            .ok_or_else(|| {
                invalid("Opening schedule source_sheet must name a drawing sheet block")
            })?
            .handle;
        let mut spec = legacy_recipe("opening")?;
        spec.at = at;
        spec.filter = ObjectFilter {
            sheet: Some(name.into()),
            ..spec.filter
        };
        self.mark_openings(source)?;
        let handle = self.add_object_schedule(spec)?;
        let app = self.object_app(SCHEDULE_APP)?;
        crate::xdata::write_text(
            self.drawing
                .doc
                .get_entity_mut(Handle::new(handle))
                .expect("new table")
                .common_mut(),
            SCHEDULE_APP,
            app,
            Some(&format!("{:X}", source.value())),
        );
        Ok(handle)
    }

    pub(crate) fn has_opening_schedules(&self) -> bool {
        self.drawing.doc.app_ids.contains(SCHEDULE_APP)
            && self
                .drawing
                .doc
                .entities()
                .any(|entity| matches!(entity, EntityType::Table(t) if source(t).is_some()))
    }

    /// Only old mark metadata needs a compatibility projection; table rendering is shared.
    pub(crate) fn refresh_opening_schedules(&mut self) -> Result<(), CoreError> {
        if !self.drawing.doc.app_ids.contains(SCHEDULE_APP) {
            return Ok(());
        }
        let sources: BTreeSet<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Table(table) => source(table).map(|h| h.value()),
                _ => None,
            })
            .collect();
        for source in sources {
            self.mark_openings(Handle::new(source))?;
        }
        self.migrate_object_schedules()
    }
}
