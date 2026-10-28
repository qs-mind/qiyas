//! Read-only evidence and confidence. Confidence describes an interpretation, never permission.
use crate::{measurement::DimensionLine, CoreError, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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
    CoreError::InvalidCommand(format!("No quantity item {item}; use generated:<code>, boq:<code>, takeoff:<id>, dimension:<decimal handle>:<rule> or element:<decimal handle>"))
}
/// What one element gives an item: the host's own lines summed, and each void it deducts.
fn contributions(lines: &[DimensionLine], parts: &BTreeMap<u64, Vec<u64>>) -> Vec<Value> {
    let mut hosts: BTreeMap<u64, (String, f64, Vec<String>, Vec<Value>)> = BTreeMap::new();
    for line in lines {
        let Some(host) = line.sources.first() else {
            continue;
        };
        let entry = hosts
            .entry(host.handle)
            .or_insert_with(|| (host.sheet.clone(), 0.0, vec![], vec![]));
        if line.included {
            entry.1 += line.value;
        }
        if !line.id.is_empty() {
            entry.2.push(line.id.clone());
        }
        if let Some(void) = line.sources.get(1) {
            entry.3.push(json!({"handle":void.handle,"sheet":void.sheet,"value":line.value,"included":line.included,"line":line.id}));
        }
    }
    let unit = lines.first().map(|l| l.unit.clone()).unwrap_or_default();
    hosts
        .into_iter()
        .map(|(handle, (sheet, value, ids, voids))| {
            let drawn = parts.get(&handle).cloned().unwrap_or_default();
            json!({"handle":handle,"sheet":sheet,"value":value,"unit":unit,"lines":ids,"voids":voids,"parts":drawn})
        })
        .collect()
}
impl Session {
    /// One read resolves an item to its stored arithmetic, elements, recognition and cited rules.
    pub fn explain(&self, item: &str) -> Result<Value, CoreError> {
        let (kind, key) = item.split_once(':').unwrap_or(("", item));
        if kind == "element" {
            let handle = key.parse::<u64>().map_err(|_| missing(item))?;
            return self.element_items(handle);
        }
        let report = self.measurement_report()?;
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
                id: format!("takeoff:{}", i.id),
                operands: vec![crate::measurement::Operand {
                    label: "Measured".into(),
                    expression: i.shown.to_string(),
                    value: i.shown,
                    unit: i.unit.clone(),
                    sources: i
                        .handles
                        .iter()
                        .map(|h| crate::measurement::NamedSource {
                            name: format!("{h}"),
                            source: crate::measurement::PropSource {
                                kind: crate::measurement::SourceKind::Measured,
                                handle: Some(*h),
                                text: String::new(),
                            },
                        })
                        .collect(),
                }],
                label: i.description.clone(),
                operation: crate::measurement::Operation::Add,
                timesing: 1.,
                factors: vec![i.shown],
                source_expressions: vec!["stored manual measurement".into()],
                expression: if i.available() {
                    i.shown.to_string()
                } else {
                    "Unavailable".into()
                },
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
                included: i.available(),
                decision: Some(
                    if i.available() {
                        "Stored measurement; no authored rule"
                    } else {
                        "Source enclosure unavailable; excluded from totals"
                    }
                    .into(),
                ),
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
            // As the BOQ panel shows it: a generated item replaces the manual rows of its code.
            let confidence = if let Some(i) = report
                .items
                .iter()
                .find(|i| !boq.code.is_empty() && i.code == boq.code)
            {
                lines.extend(i.lines.clone());
                i.confidence
            } else {
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
        let objects = self.objects_for(&handles.iter().copied().collect::<Vec<_>>())?;
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
            json!({"item":item,"quantity":quantity,"unit":unit,"confidence":confidence,"confidence_band":band(confidence),"confidence_policy":"Minimum recognition and measurement-rule confidence; unscored data uses 0.5","dimension_lines":lines,"elements":objects,"handles":handles,"recognition":candidates,"recognition_rules":recognition_rules,"measurement_rules":rules,"drawings":[self.drawing.path],"evidence":evidence,"original":original,"notes":notes,"by_element":contributions(&lines, &self.parts_by_owner())}),
        )
    }
    /// The graphics each element's recipe generated (hatches, outlines, symbols), by owner, so a
    /// traced element lights up as drawn and not only its driver.
    pub(crate) fn parts_by_owner(&self) -> BTreeMap<u64, Vec<u64>> {
        let mut parts: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for entity in self.drawing.doc.entities() {
            if let Some(part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            ) {
                parts
                    .entry(part.owner)
                    .or_default()
                    .push(entity.common().handle.value());
            }
        }
        parts
    }
    /// The reverse of an item: every generated item, measured quantity and manual row an element
    /// feeds, as its host or as a void it is deducted from, with what it gives each.
    pub fn element_items(&self, picked: u64) -> Result<Value, CoreError> {
        // A generated part (a wall's hatch, a symbol) answers for the element that owns it.
        let handle = self
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(picked))
            .and_then(|e| {
                crate::xdata::read_json::<crate::objects::ObjectPart>(
                    e.common(),
                    crate::objects::PART_APP,
                )
            })
            .map_or(picked, |part| part.owner);
        let report = self.measurement_report()?;
        let mut items = vec![];
        for item in &report.items {
            let own = item
                .lines
                .iter()
                .filter(|l| l.sources.iter().any(|s| s.handle == handle))
                .collect::<Vec<_>>();
            if own.is_empty() {
                continue;
            }
            let hosted = own
                .iter()
                .any(|l| l.sources.first().is_some_and(|s| s.handle == handle));
            let value: f64 = own
                .iter()
                .filter(|l| l.included)
                .filter(|l| hosted || l.sources.get(1).is_some_and(|s| s.handle == handle))
                .map(|l| l.value)
                .sum();
            items.push(json!({
                "item": format!("generated:{}", item.code),
                "code": item.code,
                "description_en": item.description_en,
                "description_ar": item.description_ar,
                "unit": item.unit,
                "quantity": item.quantity,
                "role": if hosted { "measured" } else { "deducted" },
                "value": value,
                "lines": own.iter().map(|l| l.id.clone()).collect::<Vec<_>>(),
            }));
        }
        let quantities = report
            .quantities
            .iter()
            .filter(|q| q.object == handle)
            .map(|q| {
                let mapped = report
                    .items
                    .iter()
                    .any(|i| i.lines.iter().any(|l| q.lines.iter().any(|m| m.id == l.id)));
                json!({"item": format!("dimension:{}:{}", q.object, q.rule), "rule": q.rule, "unit": q.unit, "quantity": q.quantity, "mapped": mapped})
            })
            .collect::<Vec<_>>();
        let manual = self
            .takeoff
            .items
            .iter()
            .filter(|i| i.handles.contains(&handle) || i.chain.first() == Some(&handle))
            .map(|i| json!({"item": format!("takeoff:{}", i.id), "description": i.description, "unit": i.unit, "quantity": i.shown, "boq_code": i.boq_code}))
            .collect::<Vec<_>>();
        let element = self.objects_for(&[handle])?.pop();
        Ok(json!({
            "picked": picked,
            "element": handle,
            "parts": self.parts_by_owner().remove(&handle).unwrap_or_default(),
            "object": element,
            "items": items,
            "quantities": quantities,
            "takeoff": manual,
        }))
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
