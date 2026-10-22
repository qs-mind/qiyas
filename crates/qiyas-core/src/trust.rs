//! Read-only evidence and confidence. Confidence describes an interpretation, never permission.
use crate::{measurement::DimensionLine, objects::ObjectFilter, CoreError, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn unscored() -> f64 {
    0.5
}
pub fn band(value: f64) -> &'static str {
    if value < 0.6 {
        "low"
    } else if value < 0.9 {
        "medium"
    } else {
        "high"
    }
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceSummary {
    pub low: usize,
    pub medium: usize,
    pub high: usize,
}
impl ConfidenceSummary {
    pub fn of(values: impl IntoIterator<Item = f64>) -> Self {
        let mut result = Self::default();
        for value in values {
            match band(value) {
                "low" => result.low += 1,
                "medium" => result.medium += 1,
                _ => result.high += 1,
            }
        }
        result
    }
}
fn missing(item: &str) -> CoreError {
    CoreError::InvalidCommand(format!("No quantity item {item}; use generated:<code>, boq:<code>, takeoff:<id>, or dimension:<decimal handle>:<rule>"))
}
impl Session {
    /// One read resolves an item to its stored arithmetic, elements, recognition and cited rules.
    pub fn explain(&self, item: &str) -> Result<Value, CoreError> {
        let report = self.measurement_report()?;
        let (kind, key) = item.split_once(':').unwrap_or(("", item));
        let generated = report.items.iter().find(|i| i.code == key);
        let manual = self.takeoff.items.iter().find(|i| i.id == key);
        let mut lines: Vec<DimensionLine> = vec![];
        let mut handles = BTreeSet::new();
        let mut notes = vec![];
        let (quantity, unit, confidence, original) = if kind == "dimension" {
            let (handle, rule) = key.split_once(':').ok_or_else(|| missing(item))?;
            let q = report
                .quantities
                .iter()
                .find(|q| q.object.to_string() == handle && q.rule == rule)
                .ok_or_else(|| missing(item))?;
            lines = q.lines.clone();
            (q.quantity, q.unit.clone(), q.confidence, json!(q))
        } else if let Some(i) = generated.filter(|_| matches!(kind, "" | "generated")) {
            lines = i.lines.clone();
            (i.quantity, i.unit.clone(), i.confidence, json!(i))
        } else if let Some(i) = manual.filter(|_| matches!(kind, "" | "takeoff")) {
            handles.extend(i.handles.iter().copied());
            handles.extend(i.chain.first().copied());
            notes
                .push("Manual measurement: no authored recognition or measurement rule is implied");
            lines.push(crate::measurement::DimensionLine {
                label: i.description.clone(),
                operation: crate::measurement::Operation::Add,
                timesing: 1.,
                factors: vec![i.shown],
                source_expressions: vec!["stored manual measurement".into()],
                expression: i.shown.to_string(),
                value: i.shown,
                unit: i.unit.clone(),
                method: "manual".into(),
                citations: vec![crate::measurement::Citation {
                    source: i.source.clone(),
                    clause: i.id.clone(),
                }],
                sources: i
                    .handles
                    .iter()
                    .map(|h| crate::measurement::DrawingReference {
                        handle: *h,
                        sheet: i.sheet.clone(),
                        grid: None,
                        evidence: vec![],
                    })
                    .collect(),
                included: true,
                decision: Some("Stored measurement; no authored rule".into()),
            });
            (i.shown, i.unit.clone(), i.confidence, json!(i))
        } else if matches!(kind, "" | "boq" | "boq-row") {
            let boq = self
                .boq
                .as_ref()
                .and_then(|b| {
                    if kind == "boq-row" {
                        key.parse::<usize>().ok().and_then(|i| b.items.get(i))
                    } else {
                        let mut rows = b.items.iter().filter(|i| i.code == key);
                        let first = rows.next();
                        if rows.next().is_none() {
                            first
                        } else {
                            None
                        }
                    }
                })
                .ok_or_else(|| missing(item))?;
            for i in self.takeoff.items.iter().filter(|i| {
                !boq.code.is_empty() && i.boq_code.as_deref() == Some(boq.code.as_str())
            }) {
                handles.extend(i.handles.iter().copied());
                handles.extend(i.chain.first().copied());
                let explained = self.explain(&format!("takeoff:{}", i.id))?;
                let manual_lines: Vec<DimensionLine> =
                    serde_json::from_value(explained["dimension_lines"].clone())
                        .map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
                lines.extend(manual_lines);
            }
            let confidence = if let Some(i) = report.items.iter().find(|i| i.code == boq.code) {
                lines.extend(i.lines.clone());
                i.confidence
            } else {
                unscored()
            };
            notes.push("Imported quantity is a client value; linked measurements explain its drawing evidence");
            (boq.quantity, boq.unit.clone(), confidence, json!(boq))
        } else {
            return Err(missing(item));
        };
        handles.extend(
            lines
                .iter()
                .flat_map(|l| l.sources.iter().map(|s| s.handle)),
        );
        let objects = self
            .objects(&ObjectFilter::default())?
            .into_iter()
            .filter(|o| handles.contains(&o.instance.id))
            .collect::<Vec<_>>();
        let candidates = self
            .recognition_evidence()?
            .into_iter()
            .filter(|c| {
                handles.contains(&c.handle) || c.object.is_some_and(|h| handles.contains(&h))
            })
            .collect::<Vec<_>>();
        let recognition_rules = self
            .recognition_rules()?
            .into_iter()
            .filter(|r| candidates.iter().any(|c| c.rule == r.name))
            .collect::<Vec<_>>();
        let measured = report
            .quantities
            .iter()
            .filter(|q| handles.contains(&q.object) && q.lines.iter().any(|l| lines.contains(l)))
            .collect::<Vec<_>>();
        let rules = report
            .selection
            .effective()
            .map(|m| {
                m.rules
                    .iter()
                    .filter(|r| measured.iter().any(|q| q.rule == r.id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut evidence = lines
            .iter()
            .flat_map(|l| {
                l.sources
                    .iter()
                    .map(|s| json!({"handle":s.handle,"sheet":s.sheet,"drawing":self.drawing.path}))
            })
            .collect::<Vec<_>>();
        for c in &candidates {
            for h in std::iter::once(c.handle).chain(c.evidence.iter().filter_map(|e| e.handle)) {
                handles.insert(h);
            }
        }
        for h in &handles {
            for sheet in self.drawing.sheets() {
                if self
                    .drawing
                    .entities_in(&sheet.block)
                    .any(|e| e.common().handle.value() == *h)
                {
                    let entry = json!({"handle":h,"sheet":sheet.block,"drawing":self.drawing.path});
                    if !evidence.contains(&entry) {
                        evidence.push(entry);
                    }
                }
            }
        }
        Ok(
            json!({"item":item,"quantity":quantity,"unit":unit,"confidence":confidence,"confidence_band":band(confidence),"confidence_policy":"Minimum recognition and measurement-rule confidence; unscored data uses 0.5","dimension_lines":lines,"elements":objects,"handles":handles,"recognition":candidates,"recognition_rules":recognition_rules,"measurement_rules":rules,"drawings":[self.drawing.path],"evidence":evidence,"original":original,"notes":notes}),
        )
    }
}
/// Project contributions retain their explanation from the same run as the quantity.
pub fn explain_project(path: &str, item: &str) -> Result<Value, CoreError> {
    let path = crate::projects::project_path(path)?;
    let set = crate::projects::load(&path)?;
    let data = set.project.ok_or_else(|| missing(item))?;
    let row = if let Some(index) = item.strip_prefix("project:") {
        index.parse::<usize>().ok().and_then(|i| data.boq.get(i))
    } else {
        let found = data
            .boq
            .iter()
            .filter(|r| r.item.code == item)
            .collect::<Vec<_>>();
        if found.len() == 1 {
            Some(found[0])
        } else {
            None
        }
    }
    .ok_or_else(|| missing(item))?;
    let contributions = &row.contributions;
    let collect = |field: &str| {
        contributions
            .iter()
            .flat_map(|c| {
                c.explanation[field]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |value| json!({"drawing":c.drawing,"data":value}))
            })
            .collect::<Vec<_>>()
    };
    Ok(
        json!({"elements":collect("elements"),"handles":collect("handles"),"recognition_rules":collect("recognition_rules"),"measurement_rules":collect("measurement_rules"),"item":item,"quantity":row.item.quantity,"unit":row.item.unit,"confidence":row.item.confidence,"confidence_band":band(row.item.confidence),"drawings":contributions.iter().map(|c| &c.drawing).collect::<Vec<_>>(),"contributions":contributions,"dimension_lines":contributions.iter().flat_map(|c| &c.lines).collect::<Vec<_>>(),"evidence":contributions.iter().flat_map(|c| c.explanation["evidence"].as_array().into_iter().flatten()).collect::<Vec<_>>()}),
    )
}
