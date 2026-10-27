//! The blind second reading of a generated quantity.
//!
//! Before a generated quantity reaches the BOQ, a fresh reader with no conversation history and
//! read-only tools measures the same item again (the reader lives in `qiyas-ai`). It is shown the
//! item's description, unit and rule, never the first quantity. This module holds what is not the
//! model's to decide: the tolerance per unit, the comparison, the stored readings and the status of
//! each item. An item whose two readings differ is not written as a BOQ row; it becomes a
//! drawing query with both readings (see `discrepancy`).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::measurement::{GeneratedItem, MeasurementReport};
use crate::{CoreError, Session};

/// The knowledge entry (under the measurement kind) that holds the readings.
const STORE: &str = "second_readings";

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

/// How the two readings of an item compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondStatus {
    /// Within the tolerance for the item's unit.
    Agrees,
    /// Beyond the tolerance: the item is kept out of the BOQ and raised as a drawing query.
    Differs,
    /// Beyond the tolerance, and the engineer has answered the query: the item goes in the BOQ.
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
pub fn item_label(item: &GeneratedItem, arabic: bool) -> &'static str {
    item.second_reading
        .as_ref()
        .map_or_else(|| not_checked_label(arabic), |s| s.label(arabic))
}

/// The tolerance for a unit, in percent of the larger reading. Counts must match exactly.
pub fn tolerance_percent(unit: &str) -> f64 {
    match unit.trim().to_lowercase().as_str() {
        "pcs" | "nr" | "no" | "ea" | "each" | "set" | "sets" | "item" | "number" | "عدد" => 0.0,
        "m" | "lm" | "rm" | "ml" => 1.0,
        "m2" | "m²" | "sqm" => 2.0,
        "m3" | "m³" | "cum" => 3.0,
        _ => 2.0,
    }
}

/// Whether the two readings agree for `unit`. Code decides this, not the model. The absolute
/// allowance is half of the last printed digit (two decimals), so rounding alone never differs.
pub fn agrees(first: f64, second: f64, unit: &str) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    let percent = tolerance_percent(unit);
    if percent == 0.0 {
        return (first - second).abs() < 0.5;
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
}

/// The stable key of the query a differing item raises (see `discrepancy`).
pub(crate) fn conflict_key(code: &str) -> String {
    format!("second_reading|{code}")
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

impl Session {
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
                    return Err(invalid(
                        "A second reading is a finite number of 0 or more",
                    ));
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
                    },
                );
                self.put_second_readings(&stored)?;
                Ok(json!({
                    "code": code,
                    "first": first,
                    "second": quantity,
                    "unit": unit,
                    "agrees": agrees(first, quantity, &unit),
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
    /// while the item still has the quantity that was checked; after an edit the item is "not
    /// checked" again.
    pub(crate) fn attach_second_readings(
        &self,
        report: &mut MeasurementReport,
    ) -> Result<(), CoreError> {
        let stored = self.stored_second_readings()?;
        if stored.is_empty() {
            return Ok(());
        }
        for item in &mut report.items {
            let Some(s) = stored.get(&item.code) else {
                continue;
            };
            if item.informational || s.unit != item.unit || !same(s.first, item.quantity) {
                continue;
            }
            let status = if agrees(s.first, s.second, &s.unit) {
                SecondStatus::Agrees
            } else {
                let key = format!(
                    "{}|{}",
                    crate::ConflictKind::QuantityMismatch.name(),
                    conflict_key(item.base_code())
                );
                let answered = self
                    .conflicts
                    .iter()
                    .any(|c| c.hash_key == key && c.status != crate::ConflictStatus::Open);
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
                tolerance_percent: tolerance_percent(&s.unit),
            });
        }
        Ok(())
    }

    /// The items still to read twice, the ones with the most value first, at most `limit`
    /// (all when `None`). An item's value is its quantity times the client BOQ's rate when the
    /// BOQ has one, and otherwise the number of drawing elements behind it.
    pub fn second_reading_brief(&self, limit: Option<usize>) -> Result<Vec<Brief>, CoreError> {
        let report = self.measurement_report()?;
        let rate = |item: &GeneratedItem| {
            self.boq
                .as_ref()
                .and_then(|boq| boq.items.iter().find(|b| b.code == item.base_code()))
                .and_then(|b| b.rate)
        };
        let elements = |item: &GeneratedItem| {
            item.lines
                .iter()
                .map(|l| l.sources.len())
                .sum::<usize>()
        };
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
        Ok(open
            .into_iter()
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
                Brief {
                    code: item.code.clone(),
                    description_en: item.description_en.clone(),
                    description_ar: item.description_ar.clone(),
                    unit: item.unit.clone(),
                    rule,
                    sheets,
                }
            })
            .collect())
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
                }
                .label(arabic);
                assert!(!label.to_lowercase().contains("confiden"));
            }
        }
    }
}
