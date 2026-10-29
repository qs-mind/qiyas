//! The blind second reading of a generated quantity.
//!
//! Before a generated quantity reaches the BOQ, a fresh reader with no conversation history and
//! read-only tools measures the same item again (the reader lives in `qiyas-ai`). It is shown the
//! item's description, unit and rule, never the first quantity. This module holds what is not the
//! model's to decide: comparison against the declared kind/tolerance, stored evidence and status of
//! each item. An item whose two readings differ remains in the BOQ with both readings and
//! also becomes a drawing query (see `discrepancy`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::measurement::{GeneratedItem, MeasurementReport};
use crate::{CoreError, Session};

/// The knowledge entry (under the measurement kind) that holds the readings.
const STORE: &str = "second_readings";

type Observer = Box<dyn FnMut(&mut Session)>;
thread_local! {
    static OBSERVER: std::cell::RefCell<Option<Observer>> = const { std::cell::RefCell::new(None) };
}

/// A host installs a reader only for the duration of its operation. Core supplies the exact
/// execution boundary, including a comparison/export reached after earlier batch edits.
pub fn with_observer<T>(
    observer: impl FnMut(&mut Session) + 'static,
    action: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Observer>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OBSERVER.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(OBSERVER.with(|slot| slot.borrow_mut().replace(Box::new(observer))));
    action()
}

pub fn observe(session: &mut Session) {
    let observer = OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(mut observer) = observer {
        observer(session);
        OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
    }
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

/// How the two readings of an item compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondStatus {
    /// Within the tolerance for the item's declared kind.
    Agrees,
    /// Beyond the tolerance: retained in the BOQ and raised as a drawing query.
    Differs,
    /// Beyond the tolerance, and the engineer has answered the query.
    Accepted,
}

/// One item's second reading, as the report shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondRead {
    pub status: SecondStatus,
    /// The generated quantity that was checked.
    pub first: f64,
    /// What the blind reader measured.
    pub second: f64,
    /// The tolerance that applied, in percent (0 for counts).
    pub tolerance_percent: f64,
    #[serde(default)]
    pub tolerance_absolute: f64,
    #[serde(default)]
    pub evidence_key: String,
}

impl SecondRead {
    /// The words the BOQ column and the Mizan panel use. No confidence wording.
    pub fn label(&self, arabic: bool) -> &'static str {
        match (self.status, arabic) {
            (SecondStatus::Agrees, false) => "Second reading agrees",
            (SecondStatus::Differs, false) => "Second reading differs",
            (SecondStatus::Accepted, false) => "Second reading differs, accepted",
            (SecondStatus::Agrees, true) => "القراءة الثانية متطابقة",
            (SecondStatus::Differs, true) => "القراءة الثانية مختلفة",
            (SecondStatus::Accepted, true) => "القراءة الثانية مختلفة، تم قبولها",
        }
    }
}

/// The label of an item nobody has read twice.
pub fn not_checked_label(arabic: bool) -> &'static str {
    if arabic {
        "لم تُراجع بقراءة ثانية"
    } else {
        "Second reading not checked"
    }
}

/// The words for an item with or without a second reading.
pub fn item_label(item: &GeneratedItem, arabic: bool) -> String {
    match &item.second_reading {
        Some(read) if read.status != SecondStatus::Agrees => {
            let number = read.second.to_string();
            let number = if arabic {
                crate::numbers::arabic_digits(&number)
            } else {
                number
            };
            format!("{}: {number}", read.label(arabic))
        }
        Some(read) => read.label(arabic).into(),
        None => not_checked_label(arabic).into(),
    }
}

/// The tolerance for a unit, in percent of the larger reading. Counts must match exactly.
pub fn tolerance_percent(unit: &str) -> f64 {
    match unit.trim().to_lowercase().as_str() {
        "pcs" | "nr" | "no" | "ea" | "each" | "set" | "sets" | "item" | "number" | "عدد" => 0.0,
        "m" | "lm" | "rm" | "ml" => 1.0,
        "m2" | "m²" | "sqm" => 2.0,
        "m3" | "m³" | "cum" => 3.0,
        _ => 0.0,
    }
}

/// Whether the two readings agree for `unit`. Code decides this, not the model. The absolute
/// allowance is half of the last printed digit (two decimals), so rounding alone never differs.
pub fn agrees(first: f64, second: f64, unit: &str) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    agrees_with_percent(first, second, tolerance_percent(unit))
}

fn agrees_with_percent(first: f64, second: f64, percent: f64) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    if percent == 0.0 {
        return first == second;
    }
    (first - second).abs() <= first.abs().max(second.abs()) * percent / 100.0 + 0.005
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

/// What the stored reading remembers: the quantity that was checked and what was read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stored {
    first: f64,
    second: f64,
    unit: String,
    #[serde(default)]
    binding: Value,
}

/// The stable key of the query a differing item raises (see `discrepancy`).
pub(crate) fn conflict_key(code: &str, evidence_key: &str) -> String {
    format!("second_reading|{code}|{evidence_key}")
}
fn evidence_key(binding: &Value) -> String {
    use std::hash::Hasher;
    let mut hash = fnv::FnvHasher::default();
    hash.write(binding.to_string().as_bytes());
    format!("{:016x}", hash.finish())
}

/// What the blind reader is shown of an item: its description, unit and rule. Never a quantity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Brief {
    pub code: String,
    pub description_en: String,
    pub description_ar: String,
    pub unit: String,
    /// The measurement rules and clauses the quantity is made by, as text.
    pub rule: String,
    /// The sheets the item's elements are on.
    pub sheets: Vec<String>,
    /// Complete definitions and grouping predicates, without evaluated operands or totals.
    pub scope: Value,
    /// Host-only identity of the evidence; never included in the reader prompt.
    #[serde(skip)]
    pub binding: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SecondReadingCommand {
    /// Stores the quantity the blind reader measured for the BOQ item `code`. Qiyas compares it
    /// with the quantity now in the report; the first quantity is never taken from here.
    #[schemars(example = json!({"kind":"record","code":"03.01","quantity":12.5}))]
    Record { code: String, quantity: f64 },
    /// Forgets the second reading of the item `code`, or of every item.
    #[schemars(example = json!({"kind":"clear"}))]
    Clear {
        #[serde(default)]
        code: Option<String>,
    },
}

fn prints_result(text: &str) -> bool {
    // The output marker establishes provenance; numeric text alone never does.
    let text = crate::numbers::western(text);
    text.split_whitespace().any(|word| {
        matches!(
            word.trim_matches(|c: char| !c.is_alphanumeric() && c != '²' && c != '³'),
            "m" | "m2" | "m3" | "m²" | "m³" | "pcs" | "nr" | "عدد" | "م" | "م²" | "م³"
        )
    }) && text.chars().any(|c| c.is_ascii_digit())
}

fn generated_reading_text(entity: &opencadcodec::EntityType) -> bool {
    let data = &entity.common().extended_data;
    if [
        "QIYAS_FIELD",
        "QIYAS_OBJECT_SCHEDULE",
        "QIYAS_OPENING_SCHEDULE",
        "QIYAS_ROOM_SCHEDULE",
        "QIYAS_ROOM_LABEL",
    ]
    .iter()
    .any(|app| data.get_record(app).is_some())
    {
        return true;
    }
    let engineer = crate::xdata::read_json::<Value>(entity.common(), "QIYAS_TEXT")
        .is_some_and(|value| value["author"] == "engineer");
    !engineer
        && data.get_record("QIYAS_TEXT").is_some()
        && match entity {
            opencadcodec::EntityType::Text(text) => prints_result(&text.value),
            opencadcodec::EntityType::MText(text) => prints_result(&text.value),
            opencadcodec::EntityType::Table(_) => true,
            _ => false,
        }
}

fn rules_for<'a>(
    item: &GeneratedItem,
    report: &'a MeasurementReport,
) -> Vec<&'a crate::measurement::Rule> {
    let ids: std::collections::BTreeSet<_> = report
        .quantities
        .iter()
        .filter(|q| {
            q.lines
                .iter()
                .any(|line| item.lines.iter().any(|l| l.id == line.id))
        })
        .map(|q| q.rule.as_str())
        .collect();
    report
        .selection
        .effective()
        .into_iter()
        .flat_map(|m| &m.rules)
        .filter(|r| ids.contains(r.id.as_str()))
        .collect()
}

fn item_tolerance(
    item: &GeneratedItem,
    report: &MeasurementReport,
) -> crate::discrepancy::Tolerance {
    use crate::objects::UnitKind;
    let fallback = || {
        let percent = tolerance_percent(&item.unit);
        crate::discrepancy::Tolerance {
            percent,
            absolute: if percent == 0.0 { 0.0 } else { 0.005 },
        }
    };
    rules_for(item, report)
        .iter()
        .map(|rule| {
            let kind = match rule.kind.as_deref() {
                Some("count") => Some(UnitKind::Count),
                Some("length") => Some(UnitKind::Length),
                Some("area") => Some(UnitKind::Area),
                Some("volume") => Some(UnitKind::Volume),
                _ => None,
            };
            if matches!(kind, Some(UnitKind::Count)) {
                return crate::discrepancy::Tolerance {
                    percent: 0.0,
                    absolute: 0.0,
                };
            }
            if let Some(tolerance) = &rule.tolerance {
                return tolerance.clone();
            }
            let percent = match kind {
                Some(UnitKind::Length) => 1.0,
                Some(UnitKind::Area) => 2.0,
                Some(UnitKind::Volume) => 3.0,
                _ => return fallback(),
            };
            crate::discrepancy::Tolerance {
                percent,
                absolute: 0.005,
            }
        })
        .reduce(|a, b| crate::discrepancy::Tolerance {
            percent: a.percent.min(b.percent),
            absolute: a.absolute.min(b.absolute),
        })
        .unwrap_or_else(fallback)
}

fn item_agrees(first: f64, second: f64, tolerance: &crate::discrepancy::Tolerance) -> bool {
    first.is_finite()
        && second.is_finite()
        && (first - second).abs()
            <= first.abs().max(second.abs()) * tolerance.percent / 100.0 + tolerance.absolute
}

impl Session {
    fn second_reading_scope(
        &self,
        item: &GeneratedItem,
        report: &MeasurementReport,
    ) -> Result<Value, CoreError> {
        let rules = rules_for(item, report);
        let mappings = knowledge::get(&self.drawing.doc, Kind::Measurement, "mappings")?
            .map(|entry| entry.data)
            .unwrap_or_else(|| json!([]));
        let mappings: Vec<_> = mappings
            .as_array()
            .into_iter()
            .flatten()
            .filter(|mapping| rules.iter().any(|rule| mapping["rule"] == rule.id))
            .collect();
        // Distinct property definitions specify size, zone, level and mapping groups without
        // disclosing how many source objects belong to each group.
        let ids: Vec<_> = report
            .quantities
            .iter()
            .filter(|q| {
                q.lines
                    .iter()
                    .any(|line| item.lines.iter().any(|l| l.id == line.id))
            })
            .map(|q| q.object)
            .collect();
        let objects =
            self.object_records_for(&crate::objects::ObjectFilter::default(), Some(&ids), false)?;
        let groups: std::collections::BTreeSet<_> = objects.iter().map(|o|
            json!({"type":o.instance.type_name,"level":o.instance.level,"properties":o.instance.props}).to_string()).collect();
        let groups: Vec<Value> = groups
            .into_iter()
            .map(|s| serde_json::from_str(&s))
            .collect::<Result<_, _>>()
            .map_err(|e| invalid(e.to_string()))?;
        let grids: std::collections::BTreeSet<_> = item
            .lines
            .iter()
            .flat_map(|line| &line.sources)
            .filter_map(|source| source.grid.as_deref())
            .collect();
        let exclusions = knowledge::get(&self.drawing.doc, Kind::Measurement, "view_only")?
            .map(|entry| entry.data)
            .unwrap_or_else(|| json!([]));
        Ok(
            json!({"rules":rules,"mappings":mappings,"groups":groups,"grids":grids,"exclusions":exclusions,
            "recognition":self.recognition_rules()?,"type_definitions":self.object_types()?,
            "trade":item.trade,"coverage":item.coverage,"description_en":item.description_en,"description_ar":item.description_ar,"unit":item.unit}),
        )
    }

    fn second_reading_drawing_revision(&self) -> Result<String, CoreError> {
        use std::hash::Hasher;
        // Content revision also sees edits inside an uncommitted batch. Query clouds are
        // derived review output, so creating them cannot stale the evidence they describe.
        let query_outputs = self.second_reading_query_outputs();
        let mut hash = fnv::FnvHasher::default();
        hash.write(
            json!([self.drawing.sheets(), self.annotation_scale()])
                .to_string()
                .as_bytes(),
        );
        let mut drawings: Vec<_> = self.xref_docs().into_iter().collect();
        drawings.push((String::new(), &self.drawing.doc));
        drawings.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, document) in drawings {
            hash.write(name.as_bytes());
            let mut entities: Vec<_> = document
                .entities()
                .filter(|entity| {
                    !name.is_empty() || !query_outputs.contains(&entity.common().handle.value())
                })
                .collect();
            entities.sort_by_key(|entity| entity.common().handle.value());
            for entity in entities {
                hash.write(
                    serde_json::to_string(entity)
                        .map_err(|e| invalid(e.to_string()))?
                        .as_bytes(),
                );
            }
        }
        Ok(format!("{:016x}", hash.finish()))
    }

    fn second_reading_binding_at(
        &self,
        item: &GeneratedItem,
        report: &MeasurementReport,
        drawing_revision: &str,
    ) -> Result<Value, CoreError> {
        use std::hash::Hasher;
        let mut sources: Vec<_> = item
            .lines
            .iter()
            .flat_map(|l| &l.sources)
            .map(|s| (s.handle, s.sheet.clone(), s.grid.clone()))
            .collect();
        sources.sort();
        sources.dedup();
        let mut hash = fnv::FnvHasher::default();
        hash.write(
            self.second_reading_scope(item, report)?
                .to_string()
                .as_bytes(),
        );
        Ok(json!({"drawing_revision":drawing_revision,
            "units":self.drawing.units.to_metres(),"rules_mapping":format!("{:016x}",hash.finish()),"sources":sources}))
    }

    /// A host-only evidence token, also checked after an asynchronous reader returns.
    pub fn second_reading_binding(
        &self,
        item: &GeneratedItem,
        report: &MeasurementReport,
    ) -> Result<Value, CoreError> {
        self.second_reading_binding_at(item, report, &self.second_reading_drawing_revision()?)
    }

    /// Add author provenance to the existing logical-text marker, inside the text's undo step.
    pub(crate) fn record_text_origin(
        &mut self,
        handle: u64,
        source: crate::Source,
    ) -> Result<(), CoreError> {
        let app = crate::xdata::app_handle(&self.drawing.doc, "QIYAS_TEXT");
        if let Some(entity) = self
            .drawing
            .doc
            .get_entity_mut(opencadcodec::Handle::new(handle))
        {
            if entity
                .common()
                .extended_data
                .get_record("QIYAS_TEXT")
                .is_some()
            {
                crate::xdata::write_json(
                    entity.common_mut(),
                    "QIYAS_TEXT",
                    app,
                    &json!({"author": if source == crate::Source::User { "engineer" } else { "mizan" }}),
                )?;
            }
        }
        Ok(())
    }

    fn second_reading_query_outputs(&self) -> std::collections::BTreeSet<u64> {
        let mut handles: std::collections::BTreeSet<u64> = self
            .conflicts
            .iter()
            .filter_map(|conflict| conflict.query.as_ref())
            .flat_map(|query| query.clouds.iter().copied())
            .collect();
        for comment in self.list_comments() {
            if handles.contains(&comment.handle) {
                handles.extend(comment.label);
            }
        }
        handles
    }

    /// Filter before table/text-grid grouping loses the source of individual cells.
    pub fn blind_tables(
        &self,
        sheet: Option<&str>,
        within: Option<[[f64; 2]; 2]>,
    ) -> Vec<crate::Schedule> {
        let query_outputs = self.second_reading_query_outputs();
        self.read_tables_filtered(sheet, within, |entity, local| {
            (!local || !query_outputs.contains(&entity.common().handle.value()))
                && !generated_reading_text(entity)
        })
    }

    /// Drawing output provenance, rather than a coincidentally equal number, hides results.
    pub fn blind_result_handles(&self) -> std::collections::BTreeSet<u64> {
        let mut handles = self.second_reading_query_outputs();
        handles.extend(
            self.drawing
                .doc
                .entities()
                .filter(|entity| generated_reading_text(entity))
                .map(|entity| entity.common().handle.value()),
        );
        handles
    }

    pub fn blind_xref_result_handles(&self) -> BTreeMap<String, std::collections::BTreeSet<u64>> {
        self.xref_docs()
            .into_iter()
            .map(|(name, document)| {
                let handles = document
                    .entities()
                    .filter(|entity| generated_reading_text(entity))
                    .map(|entity| entity.common().handle.value())
                    .collect();
                (name, handles)
            })
            .collect()
    }

    fn stored_second_readings(&self) -> Result<BTreeMap<String, Stored>, CoreError> {
        match knowledge::get(&self.drawing.doc, Kind::Measurement, STORE)? {
            Some(entry) => serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string())),
            None => Ok(BTreeMap::new()),
        }
    }

    fn put_second_readings(&mut self, stored: &BTreeMap<String, Stored>) -> Result<(), CoreError> {
        let value = serde_json::to_value(stored).map_err(|e| invalid(e.to_string()))?;
        if knowledge::get(&self.drawing.doc, Kind::Measurement, STORE)?
            .is_none_or(|entry| entry.data != value)
        {
            knowledge::put(&mut self.drawing.doc, Kind::Measurement, STORE, &value)?;
        }
        Ok(())
    }

    pub(crate) fn run_second_reading(
        &mut self,
        command: SecondReadingCommand,
    ) -> Result<Value, CoreError> {
        match command {
            SecondReadingCommand::Record { code, quantity } => {
                if !quantity.is_finite() || quantity < 0.0 {
                    return Err(invalid("A second reading is a finite number of 0 or more"));
                }
                let report = self.measurement_report()?;
                let item = report
                    .items
                    .iter()
                    .find(|i| i.code == code && !i.informational)
                    .ok_or_else(|| invalid(format!("No BOQ item {code} to read a second time")))?;
                let (first, unit) = (item.quantity, item.unit.clone());
                let mut stored = self.stored_second_readings()?;
                stored.insert(
                    code.clone(),
                    Stored {
                        first,
                        second: quantity,
                        unit: unit.clone(),
                        binding: self.second_reading_binding(item, &report)?,
                    },
                );
                self.put_second_readings(&stored)?;
                Ok(json!({
                    "code": code,
                    "first": first,
                    "second": quantity,
                    "unit": unit,
                    "agrees": item_agrees(first, quantity, &item_tolerance(item, &report)),
                }))
            }
            SecondReadingCommand::Clear { code } => {
                let mut stored = self.stored_second_readings()?;
                match code {
                    Some(code) => {
                        stored.remove(&code);
                    }
                    None => stored.clear(),
                }
                self.put_second_readings(&stored)?;
                Ok(json!({"changed": true}))
            }
        }
    }

    /// Puts each item's second-reading status into the report. A stored reading counts only
    /// while its drawing revision, rule/mapping definitions and source set are unchanged.
    pub(crate) fn attach_second_readings(
        &self,
        report: &mut MeasurementReport,
    ) -> Result<(), CoreError> {
        let stored = self.stored_second_readings()?;
        if stored.is_empty() {
            return Ok(());
        }
        let drawing_revision = self.second_reading_drawing_revision()?;
        let bindings: Vec<_> = report
            .items
            .iter()
            .map(|item| {
                Ok((
                    self.second_reading_binding_at(item, report, &drawing_revision)?,
                    item_tolerance(item, report),
                ))
            })
            .collect::<Result<_, CoreError>>()?;
        for (item, (binding, tolerance)) in report.items.iter_mut().zip(bindings) {
            let Some(s) = stored.get(&item.code) else {
                continue;
            };
            if item.informational
                || s.unit != item.unit
                || !same(s.first, item.quantity)
                || s.binding != binding
            {
                continue;
            }
            let status = if item_agrees(s.first, s.second, &tolerance) {
                SecondStatus::Agrees
            } else {
                let key = format!(
                    "{}|{}",
                    crate::ConflictKind::QuantityMismatch.name(),
                    conflict_key(&item.code, &evidence_key(&binding))
                );
                let answered = self.conflicts.iter().any(|c| {
                    c.hash_key == key
                        && c.status != crate::ConflictStatus::Open
                        && c.expected == Some(s.first)
                        && c.found == Some(s.second)
                        && c.unit == s.unit
                });
                if answered {
                    SecondStatus::Accepted
                } else {
                    SecondStatus::Differs
                }
            };
            item.second_reading = Some(SecondRead {
                status,
                first: s.first,
                second: s.second,
                tolerance_percent: tolerance.percent,
                tolerance_absolute: tolerance.absolute,
                evidence_key: evidence_key(&binding),
            });
        }
        Ok(())
    }

    /// The items still to read twice, the ones with the most value first, at most `limit`
    /// (all when `None`). An item's value is its quantity times the client BOQ's rate when the
    /// BOQ has one, and otherwise the number of drawing elements behind it.
    pub fn second_reading_brief(&self, limit: Option<usize>) -> Result<Vec<Brief>, CoreError> {
        let report = self.measurement_report()?;
        let drawing_revision = self.second_reading_drawing_revision()?;
        let rate = |item: &GeneratedItem| {
            self.boq
                .as_ref()
                .and_then(|boq| boq.items.iter().find(|b| b.code == item.base_code()))
                .and_then(|b| b.rate)
        };
        let elements =
            |item: &GeneratedItem| item.lines.iter().map(|l| l.sources.len()).sum::<usize>();
        let mut open: Vec<(f64, usize, &GeneratedItem)> = report
            .items
            .iter()
            .filter(|i| !i.informational && i.second_reading.is_none())
            .map(|i| {
                let value = rate(i).map_or(0.0, |r| r * i.quantity.abs());
                (value, elements(i), i)
            })
            .collect();
        open.sort_by(|a, b| {
            b.0.total_cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then(a.2.code.cmp(&b.2.code))
        });
        open.into_iter()
            .take(limit.unwrap_or(usize::MAX))
            .map(|(_, _, item)| {
                let mut sheets: Vec<String> = item
                    .lines
                    .iter()
                    .flat_map(|l| l.sources.iter().map(|s| s.sheet.clone()))
                    .collect();
                sheets.sort();
                sheets.dedup();
                let mut rule = item.methods();
                let clauses: Vec<String> = item
                    .rule_citations()
                    .iter()
                    .map(|c| format!("{} {}", c.source, c.clause))
                    .collect();
                if !clauses.is_empty() {
                    if !rule.is_empty() {
                        rule.push_str(". ");
                    }
                    rule.push_str(&clauses.join("; "));
                }
                Ok(Brief {
                    code: item.code.clone(),
                    description_en: item.description_en.clone(),
                    description_ar: item.description_ar.clone(),
                    unit: item.unit.clone(),
                    rule,
                    sheets,
                    scope: self.second_reading_scope(item, &report)?,
                    binding: self.second_reading_binding_at(item, &report, &drawing_revision)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_must_match_and_lengths_have_a_small_allowance() {
        assert!(agrees(12.0, 12.0, "pcs"));
        assert!(!agrees(12.0, 13.0, "pcs"));
        assert!(agrees(100.0, 100.9, "m"));
        assert!(!agrees(100.0, 102.0, "m"));
        assert!(agrees(50.0, 50.9, "m2"));
        assert!(!agrees(50.0, 52.0, "m2"));
        assert!(agrees(0.0, 0.0, "m3"));
        assert!(!agrees(f64::NAN, 1.0, "m"));
    }

    #[test]
    fn rounding_alone_never_differs() {
        assert!(agrees(0.12, 0.124, "m"));
    }

    #[test]
    fn the_labels_do_not_use_confidence_words() {
        for arabic in [false, true] {
            for s in [
                SecondStatus::Agrees,
                SecondStatus::Differs,
                SecondStatus::Accepted,
            ] {
                let label = SecondRead {
                    status: s,
                    first: 1.0,
                    second: 1.0,
                    tolerance_percent: 0.0,
                    tolerance_absolute: 0.0,
                    evidence_key: String::new(),
                }
                .label(arabic);
                assert!(!label.to_lowercase().contains("confiden"));
            }
        }
    }
}
