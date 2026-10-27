//! Object schedules are persisted data recipes. The same renderer serves every element type.

use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use qiyas_param::expr::Value as ExprValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::objects::{display, record_value, ObjectRecord, ObjectSchedule, SCHEDULE_APP};
use crate::{CoreError, Session};

const STATE_APP: &str = "QIYAS_OBJECT_SCHEDULE_STATE";

#[derive(Serialize, Deserialize)]
struct Rendered {
    cells: Vec<Vec<String>>,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

pub(crate) fn object_value_code(ids: &[u64], name: &str, decimals: usize, arabic: bool) -> String {
    let ids = ids
        .iter()
        .map(|id| format!("{id:X}"))
        .collect::<Vec<_>>()
        .join(",");
    let ar = if arabic { " ar" } else { "" };
    format!(r#"%<\AcVar QiyasObject "{ids}" "{name}" {decimals}{ar}>%"#)
}

pub(crate) fn legacy_recipe(kind: &str) -> Result<ObjectSchedule, CoreError> {
    let recipes: BTreeMap<String, ObjectSchedule> =
        serde_json::from_str(include_str!("../data/legacy-object-schedules.json"))
            .map_err(|e| invalid(e.to_string()))?;
    recipes
        .get(kind)
        .cloned()
        .ok_or_else(|| invalid("Unknown schedule recipe"))
}

fn typed(value: Value) -> Option<ExprValue> {
    match value {
        Value::Number(n) => n.as_f64().map(ExprValue::Number),
        Value::String(s) => Some(ExprValue::Text(s)),
        Value::Bool(b) => Some(ExprValue::Bool(b)),
        Value::Null => Some(ExprValue::Text(String::new())),
        _ => None,
    }
}

fn template(session: &Session, record: &ObjectRecord, text: &str) -> Result<String, CoreError> {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('{') {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| invalid("Unclosed schedule template expression"))?;
        let token = &after[..end];
        let (expression, format) = token
            .rsplit_once(':')
            .filter(|(_, f)| f.trim_end_matches('t').parse::<usize>().is_ok())
            .map_or((token, None), |(e, f)| (e, Some(f)));
        let expr = session.evaluation_expression(expression)?;
        let value = expr.eval_with(&|name: &str| {
            if name == "id_hex" {
                // Keep older stored recipes readable, using the same decimal handle that
                // records and commands expose.
                Some(ExprValue::Text(record.instance.id.to_string()))
            } else {
                record_value(record, name).and_then(typed)
            }
        });
        match value {
            Ok(ExprValue::Number(n)) => {
                if let Some(format) = format {
                    let trim = format.ends_with('t');
                    let decimals = format.trim_end_matches('t').parse::<usize>().map_err(|_| {
                        invalid("Schedule template precision is 0 to 6, optionally followed by t")
                    })?;
                    if decimals > 6 {
                        return Err(invalid("Schedule template precision is 0 to 6"));
                    }
                    let mut number = format!("{n:.decimals$}");
                    if trim && decimals != 0 {
                        number = number
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_owned();
                    }
                    result.push_str(&number);
                } else {
                    result.push_str(&n.to_string());
                }
            }
            Ok(ExprValue::Text(s)) => result.push_str(&s),
            Ok(ExprValue::Bool(b)) => result.push_str(if b { "true" } else { "false" }),
            Err(_) if !record.valid => result.push_str("####"),
            Err(error) => return Err(invalid(error.to_string())),
        }
        remaining = &after[end + 1..];
    }
    result.push_str(remaining);
    Ok(result)
}

impl Session {
    /// A cell as shown: its fields worked out, with the unit after a measured number unless the
    /// column heading gives it.
    fn show_cell(&self, data: &[Vec<String>], row: usize, column: usize, owner: Handle) -> String {
        let units = row == 0 || !crate::numbers::heading_gives_unit(&data[0][column]);
        if units {
            self.evaluate_fields(&data[row][column], owner)
        } else {
            self.evaluate_cell_fields(&data[row][column], owner)
        }
    }

    pub(crate) fn schedule_value(
        &self,
        spec: &ObjectSchedule,
        record: &ObjectRecord,
        name: &str,
    ) -> Result<Option<Value>, CoreError> {
        if name == "mark" {
            if let Some(value) = record
                .instance
                .props
                .get(name)
                .filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            {
                return Ok(Some(value.clone()));
            }
        }
        let arabic = spec.language.as_deref() == Some("ar");
        if let Some(text) = arabic.then(|| spec.ar_templates.get(name)).flatten() {
            // An Arabic sheet reads its numbers in Arabic digits too.
            return template(self, record, text)
                .map(|s| Some(Value::String(crate::numbers::arabic_numerals(&s))));
        }
        if let Some(text) = spec.templates.get(name) {
            return template(self, record, text).map(|s| Some(Value::String(s)));
        }
        Ok(record_value(record, name))
    }

    fn schedule_numeric(
        &self,
        spec: &ObjectSchedule,
        records: &[&ObjectRecord],
        name: &str,
    ) -> Result<bool, CoreError> {
        if spec.templates.contains_key(name)
            || spec.ar_templates.contains_key(name)
            || name == "id"
            || spec.group_by.iter().any(|n| n == name)
        {
            return Ok(false);
        }
        if records.is_empty() {
            return Ok(spec.decimals.contains_key(name));
        }
        for record in records {
            if let Some(value) = record_value(record, name) {
                if !value.is_number() {
                    return Ok(false);
                }
            } else if !self
                .object_type(&record.instance.type_name)?
                .quantities
                .contains_key(name)
            {
                return Err(invalid(format!(
                    "Unknown schedule column {name} for {}",
                    record.instance.type_name
                )));
            }
        }
        Ok(true)
    }

    fn schedule_cell(
        &self,
        spec: &ObjectSchedule,
        records: &[&ObjectRecord],
        ids: &[u64],
        name: &str,
    ) -> Result<String, CoreError> {
        if self.schedule_numeric(spec, records, name)? {
            return Ok(object_value_code(
                ids,
                name,
                spec.decimals
                    .get(name)
                    .copied()
                    .unwrap_or_else(|| crate::numbers::default_decimals(name)),
                spec.language.as_deref() == Some("ar"),
            ));
        }
        if records.is_empty() {
            return Ok("####".into());
        }
        let mut values = BTreeSet::new();
        for record in records {
            let value = self.schedule_value(spec, record, name)?.ok_or_else(|| {
                invalid(format!(
                    "Unknown schedule column {name} for {}",
                    record.instance.type_name
                ))
            })?;
            values.insert(match (value.as_f64(), spec.decimals.get(name)) {
                (Some(number), Some(decimals)) => {
                    format!("{number:.precision$}", precision = *decimals)
                }
                _ => display(&value),
            });
        }
        Ok(values.into_iter().collect::<Vec<_>>().join(", "))
    }

    fn object_schedule_data(&self, spec: &ObjectSchedule) -> Result<Vec<Vec<String>>, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("object_schedules.object_schedule_data");
        let _read = self.evaluation_read()?;
        if spec.columns.is_empty() || spec.columns.len() > 100 {
            return Err(invalid("Object schedules need 1 to 100 columns"));
        }
        if !spec.headers.is_empty() && spec.headers.len() != spec.columns.len() {
            return Err(invalid("Schedule headers must match its columns"));
        }
        if spec.decimals.values().any(|n| *n > 6) {
            return Err(invalid("Schedule decimals is 0 to 6"));
        }
        if spec.totals.keys().any(|name| !spec.columns.contains(name)) {
            return Err(invalid("Schedule totals must name displayed columns"));
        }
        let records = self.object_records_for(&spec.filter, spec.ids.as_deref(), true)?;
        let selected: Vec<_> = records
            .iter()
            .filter(|r| {
                spec.ids
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&r.instance.id))
            })
            .collect();
        let mut groups: BTreeMap<String, (Vec<&ObjectRecord>, Vec<u64>)> = BTreeMap::new();
        for record in &selected {
            let key = if spec.group_by.is_empty() {
                format!("{:020}", record.instance.id)
            } else {
                let mut values = Vec::new();
                for name in &spec.group_by {
                    values.push(
                        self.schedule_value(spec, record, name)?
                            .ok_or_else(|| invalid(format!("Unknown schedule group {name}")))?,
                    );
                }
                serde_json::to_string(&values).map_err(|e| invalid(e.to_string()))?
            };
            let group = groups.entry(key).or_default();
            group.0.push(record);
            group.1.push(record.instance.id);
        }
        if let Some(ids) = &spec.ids {
            if spec.group_by.is_empty() {
                for id in ids {
                    groups
                        .entry(format!("{id:020}"))
                        .or_insert_with(|| (Vec::new(), vec![*id]));
                }
            }
        }
        let arabic = spec.language.as_deref() == Some("ar");
        let mut rows = vec![if arabic && !spec.ar_headers.is_empty() {
            spec.ar_headers.clone()
        } else if spec.headers.is_empty() {
            spec.columns.clone()
        } else {
            spec.headers.clone()
        }];
        for (records, ids) in groups.values() {
            let mut row = Vec::new();
            for name in &spec.columns {
                row.push(self.schedule_cell(spec, records, ids, name)?);
            }
            rows.push(row);
        }
        if !spec.totals.is_empty() {
            // One total per kind when the schedule says so; otherwise over what it shows.
            let mut parts: BTreeMap<String, (Vec<u64>, String)> = BTreeMap::new();
            if spec.total_by.is_empty() {
                let ids = spec
                    .ids
                    .clone()
                    .unwrap_or_else(|| selected.iter().map(|r| r.instance.id).collect());
                parts.insert(String::new(), (ids, String::new()));
            } else {
                for record in &selected {
                    let mut labels = Vec::new();
                    for name in &spec.total_by {
                        labels.push(display(
                            &self.schedule_value(spec, record, name)?.ok_or_else(|| {
                                invalid(format!("Unknown schedule total_by {name}"))
                            })?,
                        ));
                    }
                    let label = labels.join(" / ");
                    parts
                        .entry(label.clone())
                        .or_insert_with(|| (Vec::new(), label))
                        .0
                        .push(record.instance.id);
                }
            }
            for (ids, label) in parts.into_values() {
                let mut total = Vec::new();
                for name in &spec.columns {
                    let text = arabic
                        .then(|| spec.ar_totals.get(name))
                        .flatten()
                        .or_else(|| spec.totals.get(name));
                    total.push(match text {
                        Some(text) if text.is_empty() => object_value_code(
                            &ids,
                            name,
                            spec.decimals
                                .get(name)
                                .copied()
                                .unwrap_or_else(|| crate::numbers::default_decimals(name)),
                            arabic,
                        ),
                        Some(text) if label.is_empty() => text.clone(),
                        Some(text) => format!("{text} {label}"),
                        None => String::new(),
                    });
                }
                rows.push(total);
            }
        }
        if rows.len() == 1 {
            rows.push(vec![String::new(); spec.columns.len()]);
        }
        if rows.len() * spec.columns.len() > 10000 {
            return Err(invalid(
                "Object schedule exceeds 10000 cells; narrow the filter",
            ));
        }
        Ok(rows)
    }

    pub(crate) fn object_field_value(
        &self,
        ids: &[u64],
        name: &str,
        decimals: usize,
    ) -> Option<String> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("object_schedules.object_field_value");
        let records = self.objects_for(ids).ok()?;
        let mut sum = 0.0;
        for id in ids {
            let record = records.iter().find(|r| r.instance.id == *id)?;
            if !record.valid {
                return None;
            }
            sum += record_value(record, name)?.as_f64()?;
        }
        sum.is_finite().then(|| format!("{sum:.decimals$}"))
    }

    pub(crate) fn stamp_object_schedule(
        &mut self,
        handle: u64,
        spec: &ObjectSchedule,
    ) -> Result<(), CoreError> {
        let app = self.object_app(SCHEDULE_APP)?;
        let entity = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(handle))
            .ok_or(CoreError::NoSuchEntity(handle))?;
        crate::xdata::write_json(entity.common_mut(), SCHEDULE_APP, app, spec)
    }

    pub(crate) fn add_object_schedule(
        &mut self,
        mut spec: ObjectSchedule,
    ) -> Result<u64, CoreError> {
        if let Some(ids) = &mut spec.ids {
            ids.sort_unstable();
            ids.dedup();
        }
        let data = self.object_schedule_data(&spec)?;
        let owner = self
            .sheet_owner()
            .ok_or_else(|| invalid("Choose a sheet"))?;
        let shown: Vec<Vec<String>> = (0..data.len())
            .map(|r| {
                (0..data[r].len())
                    .map(|c| self.show_cell(&data, r, c, owner))
                    .collect()
            })
            .collect();
        let (width, height) = self.table_sizes(0.0, 0.0, &shown);
        let saved = self.current.clone();
        let result = (|| {
            if self.current.layer == "0" {
                if let Some(layer) = &spec.default_layer {
                    if !self.drawing.doc.layers.contains(layer) {
                        self.new_layer(layer)?;
                    }
                    self.current.layer = layer.clone();
                }
            }
            self.add_table(
                spec.at,
                data.len(),
                spec.columns.len(),
                width,
                height,
                &data,
            )
        })();
        self.current = saved;
        let handle = result?;
        self.stamp_object_schedule(handle, &spec)?;
        let app = self.object_app(STATE_APP)?;
        crate::xdata::write_json(
            self.drawing
                .doc
                .get_entity_mut(Handle::new(handle))
                .expect("new table")
                .common_mut(),
            STATE_APP,
            app,
            &Rendered { cells: shown },
        )?;
        Ok(handle)
    }

    pub(crate) fn migrate_object_schedules(&mut self) -> Result<(), CoreError> {
        if !["QIYAS_OPENING_SCHEDULE", "QIYAS_ROOM_SCHEDULE"]
            .iter()
            .any(|app| self.drawing.doc.app_ids.contains(app))
        {
            return Ok(());
        }
        let tables: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Table(table)
                    if table
                        .common
                        .extended_data
                        .get_record(SCHEDULE_APP)
                        .is_none() =>
                {
                    Some(table.clone())
                }
                _ => None,
            })
            .collect();
        for table in tables {
            let mut spec = if let Some(source) = crate::opening_schedule::source(&table) {
                let mut spec = legacy_recipe("opening")?;
                spec.filter.sheet = self
                    .drawing
                    .doc
                    .block_records
                    .iter()
                    .find(|b| b.handle == source)
                    .map(|b| b.name.clone());
                if spec.filter.sheet.is_none() {
                    continue;
                }
                spec
            } else if let Some(old) =
                crate::xdata::read_json::<Value>(&table.common, "QIYAS_ROOM_SCHEDULE")
            {
                let mut spec = legacy_recipe("room")?;
                spec.ids = Some(
                    serde_json::from_value(old["rooms"].clone())
                        .map_err(|e| invalid(e.to_string()))?,
                );
                let decimals = old["decimals"].as_u64().unwrap_or(2) as usize;
                spec.decimals.insert("net_floor_area".into(), decimals);
                spec
            } else {
                continue;
            };
            spec.at = [table.insertion_point.x, table.insertion_point.y];
            self.stamp_object_schedule(table.common.handle.value(), &spec)?;
            let data = self.object_schedule_data(&spec)?;
            let shown = (0..data.len())
                .map(|r| {
                    (0..data[r].len())
                        .map(|c| self.show_cell(&data, r, c, table.common.owner_handle))
                        .collect()
                })
                .collect();
            let app = self.object_app(STATE_APP)?;
            crate::xdata::write_json(
                self.drawing
                    .doc
                    .get_entity_mut(table.common.handle)
                    .expect("table")
                    .common_mut(),
                STATE_APP,
                app,
                &Rendered { cells: shown },
            )?;
        }
        Ok(())
    }

    pub(crate) fn refresh_object_schedules(&mut self) -> Result<(), CoreError> {
        self.refresh_object_schedules_scoped(None)
    }
    pub(crate) fn refresh_object_schedules_scoped(
        &mut self,
        only: Option<&std::collections::BTreeSet<u64>>,
    ) -> Result<(), CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile =
            crate::fixtures::profile::span("object_schedules.refresh_object_schedules_scoped");
        if !self.drawing.doc.app_ids.contains(SCHEDULE_APP) {
            return Ok(());
        }
        let tables: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Table(t) => {
                    crate::xdata::read_json::<ObjectSchedule>(&t.common, SCHEDULE_APP)
                        .map(|s| (t.clone(), s))
                }
                _ => None,
            })
            .collect();
        for (mut table, spec) in tables {
            if only.is_some_and(|ids| !ids.contains(&table.common.handle.value())) {
                continue;
            }
            let data = self.object_schedule_data(&spec)?;
            let previous = crate::xdata::read_json::<Rendered>(&table.common, STATE_APP);
            let Some(prototype) = table.rows.get(1).cloned() else {
                return Err(invalid("Object schedule lost its data row"));
            };
            table.rows.resize(data.len(), prototype);
            let mut fields = Vec::new();
            let mut shown_rows = Vec::new();
            for (r, row) in data.iter().enumerate() {
                let mut shown_row = Vec::new();
                for (c, text) in row.iter().enumerate() {
                    let shown = self.show_cell(&data, r, c, table.common.owner_handle);
                    let escaped = crate::text::escape_mtext(&shown);
                    let keep_override = spec.preserve_overrides
                        && previous
                            .as_ref()
                            .and_then(|p| p.cells.get(r))
                            .and_then(|row| row.get(c))
                            .is_some_and(|old| {
                                old == &shown && table.cell_text(r, c) != Some(escaped.as_str())
                            })
                        && !crate::field::cell_fields(&table)
                            .iter()
                            .any(|(row, col, _)| *row == r && *col == c);
                    shown_row.push(shown);
                    if keep_override {
                        continue;
                    }
                    if crate::field::has_fields(text) {
                        fields.push((r, c, text.clone()));
                    }
                    crate::table::set_cell_value(&mut table, r, c, &escaped);
                }
                shown_rows.push(shown_row);
            }
            let field_app = if fields.is_empty() {
                crate::xdata::app_handle(&self.drawing.doc, crate::field::FIELD_APP)
            } else {
                self.object_app(crate::field::FIELD_APP)?
            };
            crate::field::set_cell_fields(&mut table, &fields, field_app);
            let state_app = self.object_app(STATE_APP)?;
            crate::xdata::write_json(
                &mut table.common,
                STATE_APP,
                state_app,
                &Rendered { cells: shown_rows },
            )?;
            let handle = table.common.handle;
            if self.drawing.doc.get_entity(handle) != Some(&EntityType::Table(table.clone())) {
                *self.drawing.doc.get_entity_mut(handle).expect("table") = EntityType::Table(table);
                self.rebuild_table(handle)?;
            }
        }
        Ok(())
    }
}
