//! Opening schedules read wall definitions and retain their source sheet in the drawing.

use std::collections::BTreeMap;

use opencadcodec::entities::Table;
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle};

use crate::field::{chunks, set_cell_fields};
use crate::{CoreError, Session};

pub(crate) const SCHEDULE_APP: &str = "QIYAS_OPENING_SCHEDULE";
pub(crate) const MARK_APP: &str = "QIYAS_OPENING_MARK";

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

fn source(table: &Table) -> Option<Handle> {
    let record = table.common.extended_data.get_record(SCHEDULE_APP)?;
    record.values.iter().find_map(|v| match v {
        XDataValue::String(s) => u64::from_str_radix(s, 16).ok().map(Handle::new),
        _ => None,
    })
}

fn size(mm: f64) -> String {
    format!("{mm:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

impl Session {
    fn schedule_openings(&self, source: Handle) -> Vec<(u64, String, String, String)> {
        crate::walls::schedule_openings(&self.drawing.doc, source)
            .into_iter()
            .map(|(handle, opening)| {
                let (kind, prefix) = match opening.kind {
                    crate::walls::OpeningKind::Door => ("Door", "D"),
                    crate::walls::OpeningKind::Window => ("Window", "W"),
                };
                let width = size(opening.width * self.drawing.units.to_metres() * 1000.0);
                let height =
                    size((opening.head.unwrap_or(2.1) - opening.sill.unwrap_or(0.0)) * 1000.0);
                (
                    handle,
                    format!("{prefix}-{width}x{height}"),
                    kind.into(),
                    format!("{width} x {height}"),
                )
            })
            .collect()
    }

    pub(crate) fn opening_count(&self, source: Handle, key: &str) -> Option<usize> {
        self.drawing
            .doc
            .block_records
            .iter()
            .find(|b| b.handle == source)?;
        Some(
            self.schedule_openings(source)
                .iter()
                .filter(|(_, mark, _, _)| key == "*" || mark == key)
                .count(),
        )
    }

    fn opening_schedule_data(&self, source: Handle) -> Vec<Vec<String>> {
        let groups: BTreeMap<_, _> = self
            .schedule_openings(source)
            .into_iter()
            .map(|(_, mark, kind, size)| (mark, (kind, size)))
            .collect();
        let mut data = vec![vec![
            "Mark".into(),
            "Kind".into(),
            "Width x height (mm)".into(),
            "Count".into(),
        ]];
        for (mark, (kind, size)) in groups {
            data.push(vec![mark.clone(), kind, size, count_code(source, &mark)]);
        }
        data.push(vec![
            "Total".into(),
            String::new(),
            String::new(),
            count_code(source, "*"),
        ]);
        data
    }

    fn opening_app(&mut self, name: &str) -> Result<(), CoreError> {
        if !self.drawing.doc.app_ids.contains(name) {
            let mut app = AppId::new(name);
            app.handle = self.drawing.doc.allocate_handle();
            self.drawing.doc.app_ids.add(app).map_err(invalid)?;
        }
        Ok(())
    }

    fn mark_openings(&mut self, source: Handle) -> Result<(), CoreError> {
        self.opening_app(MARK_APP)?;
        let app = self
            .drawing
            .doc
            .app_ids
            .get(MARK_APP)
            .expect("registered app")
            .handle
            .value();
        for (handle, mark, _, _) in self.schedule_openings(source) {
            if self
                .drawing
                .doc
                .get_entity(Handle::new(handle))
                .and_then(crate::opening_schedule::mark)
                .as_deref()
                == Some(&mark)
            {
                continue;
            }
            let entity = self
                .drawing
                .doc
                .get_entity_mut(Handle::new(handle))
                .expect("opening exists");
            let mut record = ExtendedDataRecord::new(MARK_APP);
            for value in chunks(&mark) {
                record.add_value(value);
            }
            entity.common_mut().extended_data.upsert_record(record);
            entity
                .common_mut()
                .extended_data
                .raw_dwg_eed
                .retain(|(id, _)| *id != app);
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
        self.mark_openings(source)?;
        let data = self.opening_schedule_data(source);
        let owner = self
            .sheet_owner()
            .ok_or_else(|| invalid("Choose a sheet first"))?;
        let shown = data
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| self.evaluate_fields(cell, owner))
                    .collect()
            })
            .collect::<Vec<Vec<_>>>();
        // Size cells from the visible quantities, not their longer field expressions.
        let (width, height) = self.table_sizes(0.0, 0.0, &shown);
        let saved = self.current.clone();
        let added = (|| {
            if self.current.layer == "0" {
                if !self.drawing.doc.layers.contains("A-ANNO-TABL") {
                    self.new_layer("A-ANNO-TABL")?;
                }
                self.current.layer = "A-ANNO-TABL".into();
            }
            self.add_table(at, data.len(), 4, width, height, &data)
        })();
        self.current = saved;
        let handle = added?;
        self.opening_app(SCHEDULE_APP)?;
        let mut record = ExtendedDataRecord::new(SCHEDULE_APP);
        record.add_value(XDataValue::String(format!("{:X}", source.value())));
        self.drawing
            .doc
            .get_entity_mut(Handle::new(handle))
            .expect("new table")
            .common_mut()
            .extended_data
            .upsert_record(record);
        Ok(handle)
    }

    pub(crate) fn has_opening_schedules(&self) -> bool {
        self.drawing
            .doc
            .entities()
            .any(|entity| matches!(entity, EntityType::Table(t) if source(t).is_some()))
    }

    /// Rebuild grouping as well as field values: a newly introduced size needs its own row.
    pub(crate) fn refresh_opening_schedules(&mut self) -> Result<(), CoreError> {
        let tables: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Table(t) => source(t).map(|s| (t.clone(), s)),
                _ => None,
            })
            .collect();
        for (mut table, source) in tables {
            self.mark_openings(source)?;
            let data = self.opening_schedule_data(source);
            let Some(prototype) = table.rows.get(1).cloned() else {
                continue;
            };
            table.rows.resize(data.len(), prototype);
            let mut fields = Vec::new();
            for (r, row) in data.iter().enumerate() {
                for (c, text) in row.iter().enumerate() {
                    if crate::field::has_fields(text) {
                        fields.push((r, c, text.clone()));
                    }
                    let shown = self.evaluate_fields(text, table.common.owner_handle);
                    crate::table::set_cell_value(
                        &mut table,
                        r,
                        c,
                        &crate::text::escape_mtext(&shown),
                    );
                }
            }
            set_cell_fields(&mut table, &fields);
            let handle = table.common.handle;
            if matches!(self.drawing.doc.get_entity(handle), Some(EntityType::Table(before)) if *before == table)
            {
                continue;
            }
            // An imported DWG's raw field payload must not overwrite the refreshed grouping.
            if let Some(app) = self.drawing.doc.app_ids.get(crate::field::FIELD_APP) {
                table
                    .common
                    .extended_data
                    .raw_dwg_eed
                    .retain(|(id, _)| *id != app.handle.value());
            }
            *self
                .drawing
                .doc
                .get_entity_mut(handle)
                .expect("schedule exists") = EntityType::Table(table);
            self.rebuild_table(handle)?;
        }
        Ok(())
    }
}

fn count_code(source: Handle, mark: &str) -> String {
    format!(
        r#"%<\AcVar QiyasOpeningCount "{:X}" "{mark}">%"#,
        source.value()
    )
}
