//! Findings shared by drawing, quantity, pack and discrepancy checks.
use std::hash::Hasher;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::measurement::Citation;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    #[default]
    Warning,
    #[serde(alias = "information")]
    Info,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quantity {
    pub name: String,
    pub value: f64,
    pub unit: String,
}

impl Quantity {
    pub fn new(name: &str, value: f64, unit: &str) -> Self {
        Self {
            name: name.into(),
            value,
            unit: unit.into(),
        }
    }
}

/// Common evidence. `details` retains rule-specific data and legacy wire records losslessly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Finding {
    pub id: String,
    pub rule: String,
    pub pack: String,
    pub source: String,
    pub severity: Severity,
    pub message: String,
    pub message_ar: String,
    pub message_key: String,
    pub params: Value,
    pub handles: Vec<u64>,
    pub sheet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    pub quantities: Vec<Quantity>,
    pub citations: Vec<Citation>,
    /// None means an observation, not a pass/fail judgement.
    pub passed: Option<bool>,
    pub details: Value,
}

impl Finding {
    pub fn drawing() -> Self {
        Self {
            source: "check".into(),
            ..Self::default()
        }
    }

    /// Identity excludes wording, severity and measured values. The subject identifies the rule's case.
    pub fn identify(&mut self, subject: &Value) {
        let mut handles = self.handles.clone();
        handles.sort_unstable();
        handles.dedup();
        let key = json!([
            self.source,
            self.pack,
            self.rule,
            self.sheet,
            handles,
            subject
        ]);
        let mut hash = fnv::FnvHasher::default();
        hash.write(key.to_string().as_bytes());
        self.id = format!("{:016x}", hash.finish());
        if self.message_ar.is_empty() {
            self.message_key = if self.source == "check" {
                format!("ai.finding.{}", self.rule)
            } else {
                format!("{}.{}", self.source, self.rule)
            };
        }
    }

    pub fn quantity(&mut self, name: &str, value: f64, unit: &str) {
        self.quantities.push(Quantity {
            name: name.into(),
            value,
            unit: unit.into(),
        });
    }

    /// Adapter for the paged evidence records consumed by existing MCP clients.
    pub fn from_record(source: &str, record: Value) -> Self {
        let text = |key: &str| record[key].as_str().unwrap_or_default().to_owned();
        let rule = if source == "outside_check" {
            text("kind")
        } else {
            let section = text("section");
            if matches!(
                section.as_str(),
                "measurement_errors" | "questions" | "advisories"
            ) && record["code"].is_string()
            {
                text("code")
            } else {
                section
            }
        };
        let severity = match record["severity"].as_str() {
            Some("error") => Severity::Error,
            Some("warning" | "question") => Severity::Warning,
            Some("info" | "information") => Severity::Info,
            _ => match rule.as_str() {
                "raw_recount"
                | "outline"
                | "design_decision"
                | "layers"
                | "blocks"
                | "excluded"
                | "measurement_summary"
                | "outside_check_sheets" => Severity::Info,
                _ => Severity::Warning,
            },
        };
        let mut handles: Vec<u64> = record["handles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect();
        handles.extend(record["handle"].as_u64());
        handles.sort_unstable();
        handles.dedup();
        let mut finding = Self {
            source: source.into(),
            pack: source.into(),
            rule,
            severity,
            message: text("message"),
            message_ar: text("message_ar"),
            handles,
            sheet: record["sheet"].as_str().map(str::to_owned),
            at: ["at", "point", "centroid"]
                .iter()
                .find_map(|key| serde_json::from_value(record[*key].clone()).ok()),
            passed: record["result"]["passed"].as_bool(),
            ..Self::default()
        };
        if finding.message.is_empty() {
            finding.message = record["reason"].as_str().unwrap_or_default().into();
        }
        for key in [
            "quantity",
            "raw_quantity",
            "boq_quantity",
            "difference",
            "value",
            "expected",
            "found",
        ] {
            if let (Some(value), Some(unit)) = (record[key].as_f64(), record["unit"].as_str()) {
                finding.quantity(key, value, unit);
            }
        }
        for (key, unit) in [
            ("area_m2", "m2"),
            ("raw_count", "pcs"),
            ("boq_count", "pcs"),
        ] {
            if let Some(value) = record[key].as_f64() {
                finding.quantity(key, value, unit);
            }
        }
        for citation in record["citations"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(std::iter::once(&record["citation"]))
        {
            if let Ok(citation) = serde_json::from_value(citation.clone()) {
                finding.citations.push(citation);
            }
        }
        // Only subject fields participate: changing a measured quantity must retain status.
        let mut subject = serde_json::Map::new();
        for key in [
            "rule",
            "rules",
            "code",
            "boq_code",
            "property",
            "item",
            "note",
            "name",
            "row",
            "term",
            "net_name",
            "check",
            "group",
            "end",
            "answer_bytes",
            "text",
        ] {
            if let Some(value) = record.get(key) {
                subject.insert(key.into(), value.clone());
            }
        }
        if let Some(name) = record["decision"]["name"].as_str() {
            subject.insert("decision".into(), json!(name));
        }
        if finding.rule == "expectations" {
            for key in ["kind", "expected", "tolerance", "unit"] {
                subject.insert(key.into(), record["result"][key].clone());
            }
            finding.message = record["result"]["message"]
                .as_str()
                .unwrap_or_default()
                .into();
            finding.severity = if finding.passed == Some(true) {
                Severity::Info
            } else {
                Severity::Warning
            };
            for key in ["expected", "measured", "tolerance"] {
                if let (Some(value), Some(unit)) = (
                    record["result"][key].as_f64(),
                    record["result"]["unit"].as_str(),
                ) {
                    finding.quantity(key, value, unit);
                }
            }
        }
        if subject.is_empty() && finding.handles.is_empty() {
            subject.insert("message".into(), json!(finding.message));
        }
        finding.params = json!({"message":finding.message,"rule":record["rule"],"property":record["property"],"code":record["code"]});
        finding.identify(&Value::Object(subject));
        finding.details = record;
        finding
    }

    /// Original record, including rule-specific fields unknown to a generic findings panel.
    pub fn into_record(self) -> Value {
        self.details
    }
}

/// A complete check before its public adapter applies paging.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingReport {
    pub overview: Value,
    /// Report-specific summary records retain their own shape.
    #[serde(default)]
    pub summaries: Vec<Value>,
    pub findings: Vec<Finding>,
}

impl From<&crate::Conflict> for Finding {
    fn from(conflict: &crate::Conflict) -> Self {
        let query = conflict.query.as_ref();
        let mut finding = Self {
            id: conflict.id.clone(),
            rule: query
                .and_then(|q| q.rule.clone())
                .unwrap_or_else(|| conflict.kind.name().into()),
            pack: "discrepancy".into(),
            source: "discrepancy".into(),
            severity: query.map_or(Severity::Warning, |q| match q.severity {
                crate::discrepancy::Severity::Error => Severity::Error,
                crate::discrepancy::Severity::Warning => Severity::Warning,
                crate::discrepancy::Severity::Information => Severity::Info,
            }),
            message: conflict.reason.clone(),
            message_ar: conflict.reason_ar.clone(),
            handles: query
                .into_iter()
                .flat_map(|q| q.references.iter().map(|r| r.handle))
                .collect(),
            sheet: conflict.sheets.first().cloned(),
            citations: query.map(|q| q.citations.clone()).unwrap_or_default(),
            details: serde_json::to_value(conflict).expect("conflict record"),
            ..Self::default()
        };
        if let Some(value) = conflict.expected {
            finding.quantity("expected", value, &conflict.unit);
        }
        if let Some(value) = conflict.found {
            finding.quantity("found", value, &conflict.unit);
        }
        finding
    }
}

impl TryFrom<Finding> for crate::Conflict {
    type Error = serde_json::Error;
    fn try_from(finding: Finding) -> Result<Self, Self::Error> {
        let mut conflict: Self = serde_json::from_value(finding.details)?;
        conflict.id = finding.id;
        conflict.reason = finding.message;
        conflict.reason_ar = finding.message_ar;
        conflict.expected = finding
            .quantities
            .iter()
            .find(|q| q.name == "expected")
            .map(|q| q.value);
        conflict.found = finding
            .quantities
            .iter()
            .find(|q| q.name == "found")
            .map(|q| q.value);
        if let Some(query) = &mut conflict.query {
            query.citations = finding.citations;
            query.severity = match finding.severity {
                Severity::Error => crate::discrepancy::Severity::Error,
                Severity::Warning => crate::discrepancy::Severity::Warning,
                Severity::Info => crate::discrepancy::Severity::Information,
            };
        }
        Ok(conflict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_uses_subject_and_location_not_wording_values_or_handle_order() {
        let mut first = Finding {
            source: "check".into(),
            pack: "general".into(),
            rule: "overlap".into(),
            sheet: Some("Tower 12".into()),
            handles: vec![902, 901, 901],
            message: "Overlap".into(),
            ..Finding::default()
        };
        first.identify(&json!("junction"));
        let mut second = first.clone();
        second.handles = vec![901, 902];
        second.message = "Reworded".into();
        second.message_ar = "تداخل".into();
        second.severity = Severity::Error;
        second.quantity("length", 900.0, "m");
        second.identify(&json!("junction"));
        assert_eq!(first.id, second.id);
        second.sheet = Some("Tower 13".into());
        second.identify(&json!("junction"));
        assert_ne!(first.id, second.id);
        second.sheet = first.sheet;
        second.identify(&json!("another junction"));
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn paged_records_keep_extra_evidence_and_use_typed_quantities() {
        let record = json!({"kind":"quantity_mismatch","sheet":"Podium",
            "handles":[900,901],"raw_quantity":18.5,"boq_quantity":19.0,"unit":"m2",
            "group":{"kind":"room","layer":"A-ROOM","block":""},
            "message":"Areas differ","message_ar":"المساحات مختلفة",
            "citations":[{"source":"Drawing","clause":"P-01"}],
            "comparison_complete":false,"unallocated_items":["P01"]});
        let finding = Finding::from_record("outside_check", record.clone());
        assert_eq!(finding.rule, "quantity_mismatch");
        assert_eq!(finding.sheet.as_deref(), Some("Podium"));
        assert_eq!(
            finding.quantities[0],
            Quantity {
                name: "raw_quantity".into(),
                value: 18.5,
                unit: "m2".into()
            }
        );
        assert_eq!(finding.citations.len(), 1);
        let restored: Finding =
            serde_json::from_value(serde_json::to_value(&finding).unwrap()).unwrap();
        assert_eq!(restored.into_record(), record);
        let mut changed = record;
        changed["raw_quantity"] = json!(20.0);
        changed["message"] = json!("Updated wording");
        assert_eq!(
            finding.id,
            Finding::from_record("outside_check", changed).id
        );
    }

    #[test]
    fn query_round_trip_keeps_status_notes_clouds_references_and_old_id() {
        let record = json!({"id":"a123456789012345","kind":"quantity_mismatch","boq_code":"P01",
            "measurements":["m1"],"schedule":"Podium","sheets":["P-01","P-02"],
            "expected":900.0,"found":850.0,"unit":"m2","reason":"Areas differ",
            "reason_ar":"المساحات مختلفة","status":"resolved","note":"Accepted by engineer",
            "created":"2026-11-03T00:00:00Z","hash_key":"old stable key",
            "query":{"severity":"information","question_en":"Confirm area","question_ar":"تأكيد المساحة",
            "references":[{"drawing":"tower.dwg","handle":900,"sheet":"P-01","grid":"A1"}],
            "citations":[{"source":"Specification","clause":"4"}],"rule":"podium_area","clouds":[950]}});
        let conflict: crate::Conflict = serde_json::from_value(record).unwrap();
        let finding = Finding::from(&conflict);
        assert_eq!(finding.id, conflict.id);
        assert_eq!(finding.rule, "podium_area");
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(finding.handles, [900]);
        assert_eq!(finding.quantities.len(), 2);
        assert_eq!(crate::Conflict::try_from(finding).unwrap(), conflict);
    }

    #[test]
    fn all_read_only_reports_produce_the_common_item_type() {
        let mut session = crate::Session::new_blank(std::path::Path::new("airport.dwg"), 6);
        session
            .run(crate::Command::Draw(crate::DrawSpec::Line {
                a: [0.0, 0.0],
                b: [900.0, 0.0],
            }))
            .unwrap();
        let pack = session.pack_check().unwrap();
        let units: &Finding = pack.line("units").unwrap();
        assert_eq!(units.passed, Some(true));
        assert_eq!(units.id, "0a1965748e88242a");
        assert_eq!(units.severity, Severity::Info);
        assert_eq!(crate::pack_check::as_json(&pack)["lines"][0]["id"], "units");
        assert!(!units.message_ar.is_empty());
        let takeoff = session.takeoff_findings(&[], "").unwrap();
        let legacy = session.takeoff_check(0, &[]).unwrap();
        assert_eq!(takeoff.overview["passed"], legacy["passed"]);
        assert!(takeoff
            .findings
            .iter()
            .all(|f| !f.id.is_empty() && f.source == "takeoff_check"));
        let query = crate::outside_check::OutsideQuery::default();
        let outside = session.outside_findings(&query).unwrap();
        assert_eq!(
            outside.overview["totals"],
            session.outside_check(&query).unwrap()["totals"]
        );
        assert!(outside
            .findings
            .iter()
            .all(|f| !f.id.is_empty() && f.source == "outside_check"));
        let drawing = session.check_drawing_all(&crate::check::CheckScope::default());
        assert!(drawing
            .findings
            .iter()
            .all(|f| !f.id.is_empty() && f.source == "check"));
    }
}
