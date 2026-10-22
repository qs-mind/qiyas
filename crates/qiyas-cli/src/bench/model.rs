use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Truth {
    pub schema_version: u32,
    pub id: String,
    pub project_type: String,
    pub drawing: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub rules: BTreeMap<String, Rule>,
    #[serde(default)]
    pub seeded_errors: Vec<Finding>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Item {
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default, alias = "code")]
    pub id: String,
    #[serde(default)]
    pub description: String,
    pub kind: String,
    pub unit: String,
    pub quantity: f64,
    #[serde(default)]
    pub source_objects: Vec<String>,
    #[serde(default = "geometric")]
    pub basis: String,
    #[serde(default)]
    pub tolerance_class: String,
    #[serde(default)]
    pub tolerance_percent: Option<f64>,
    #[serde(default)]
    pub position: Option<[f64; 2]>,
    #[serde(default)]
    pub height_m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<qiyas_core::measurement::Citation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dimension_lines: Vec<qiyas_core::measurement::DimensionLine>,
    /// IDs of the submitted dimension items making up this BOQ total.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aggregate_of: Vec<String>,
}

fn geometric() -> String {
    "geometric".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Rule {
    pub operation: String,
    pub clause: String,
    pub height_m: Option<f64>,
    pub factor: Option<f64>,
    pub deduction: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub item_id: String,
    pub kind: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Submission {
    pub items: Vec<Item>,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub elapsed_ms: f64,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl Truth {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.items.is_empty() {
            return Err("truth needs schema_version 1 and nonempty items".into());
        }
        let mut ids = BTreeSet::new();
        for item in &self.items {
            validate_item(item)?;
            if item.id.is_empty() || !ids.insert(&item.id) || item.source_objects.is_empty() {
                return Err("truth ids must be unique and every item needs source objects".into());
            }
            if !matches!(
                item.tolerance_class.as_str(),
                "count" | "geometry" | "rule" | "judgement"
            ) || (item.kind == "count" && item.tolerance_class != "count")
            {
                return Err(format!("{}: invalid tolerance class", item.id));
            }
            if item
                .tolerance_percent
                .is_some_and(|p| !p.is_finite() || !(2.0..=5.0).contains(&p))
            {
                return Err("rule tolerance_percent must be within 2..5".into());
            }
            if let Some(name) = item.basis.strip_prefix("rule:") {
                let rule = self
                    .rules
                    .get(name)
                    .ok_or_else(|| format!("undeclared rule {name}"))?;
                if rule.clause.is_empty()
                    || !matches!(
                        rule.operation.as_str(),
                        "area_times_height" | "length_times_factor" | "area_net"
                    )
                {
                    return Err(format!("{name}: unsupported or uncited rule"));
                }
                for value in [rule.height_m, rule.factor, rule.deduction]
                    .into_iter()
                    .flatten()
                {
                    if !value.is_finite() || value < 0.0 {
                        return Err("invalid rule parameter".into());
                    }
                }
                let complete = match rule.operation.as_str() {
                    "area_times_height" => rule.height_m.is_some(),
                    "length_times_factor" => rule.factor.is_some(),
                    "area_net" => rule.deduction.is_some(),
                    _ => false,
                };
                if !complete {
                    return Err(format!("{name}: missing rule parameter"));
                }
            } else if item.basis != "geometric" {
                return Err("basis must be geometric or rule:<declared name>".into());
            }
        }
        let mut errors = BTreeSet::new();
        for finding in &self.seeded_errors {
            if !ids.contains(&finding.item_id) || !errors.insert(finding) {
                return Err(
                    "seeded errors need unique (item_id, kind) and existing item ids".into(),
                );
            }
        }
        Ok(())
    }
}

pub fn validate_item(item: &Item) -> Result<(), String> {
    if item
        .confidence
        .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
        || !matches!(item.kind.as_str(), "count" | "length" | "area" | "volume")
        || !item.quantity.is_finite()
        || item.quantity < 0.0
        || item.unit.trim().is_empty()
        || item
            .position
            .is_some_and(|p| p.iter().any(|v| !v.is_finite()))
        || item.height_m.is_some_and(|v| !v.is_finite() || v <= 0.0)
    {
        return Err(format!(
            "{}: invalid kind, quantity, unit or position",
            item.id
        ));
    }
    Ok(())
}

pub fn unit(text: &str) -> String {
    match text.trim().to_lowercase().as_str() {
        "m²" | "sqm" => "m2".into(),
        "m³" | "cum" => "m3".into(),
        "nr" | "no" | "no." | "ea" | "each" | "piece" | "pieces" => "pcs".into(),
        s => s.into(),
    }
}

pub fn kind_from_unit(text: &str) -> Result<String, String> {
    match unit(text).as_str() {
        "pcs" => Ok("count".into()),
        "m" => Ok("length".into()),
        "m2" => Ok("area".into()),
        "m3" => Ok("volume".into()),
        _ => Err(format!(
            "cannot infer kind from unit {text}; supply JSON kind explicitly"
        )),
    }
}

pub fn objects(item: &Item) -> BTreeSet<String> {
    item.source_objects
        .iter()
        .map(|s| {
            u64::from_str_radix(s.trim_start_matches("0x"), 16)
                .map(|n| format!("{n:X}"))
                .unwrap_or_else(|_| s.clone())
        })
        .collect()
}
