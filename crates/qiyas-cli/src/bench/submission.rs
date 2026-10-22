use std::collections::BTreeSet;
use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};
use serde_json::Value;

use super::model::{kind_from_unit, objects, unit, validate_item, Item, Submission};

pub fn read(path: &Path) -> Result<Submission, String> {
    let mut submission = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        let value: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        from_json(value)?
    } else {
        excel(path)?
    };
    if !submission.elapsed_ms.is_finite() || submission.elapsed_ms < 0.0 {
        return Err("invalid elapsed_ms".into());
    }
    for item in &mut submission.items {
        validate_item(item)?;
        item.source_objects.sort();
        if item
            .source_objects
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err("duplicate source object citation".into());
        }
    }
    Ok(submission)
}

pub fn from_json(mut value: Value) -> Result<Submission, String> {
    if value.is_array() {
        value = serde_json::json!({"items":value});
    }
    let rows = value
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .ok_or("submission needs an items array")?;
    for row in rows {
        let obj = row
            .as_object_mut()
            .ok_or("submission item must be an object")?;
        if !obj.contains_key("quantity") {
            let shown = obj
                .get("shown")
                .cloned()
                .ok_or("submission item needs quantity or shown")?;
            obj.insert("quantity".into(), shown);
        }
        if !obj.contains_key("kind") {
            let kind = kind_from_unit(obj.get("unit").and_then(Value::as_str).unwrap_or(""))?;
            obj.insert("kind".into(), kind.into());
        }
        if let Some(code) = obj.get("boq_code").filter(|v| v.is_string()).cloned() {
            obj.insert("id".into(), code);
        }
        if !obj.contains_key("source_objects") {
            if let Some(handles) = obj.get("handles").and_then(Value::as_array) {
                let sources: Result<Vec<_>, _> = handles
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .map(|h| format!("{h:X}"))
                            .ok_or("handles must be integers")
                    })
                    .collect();
                obj.insert("source_objects".into(), serde_json::json!(sources?));
            }
        }
        // Native take-off height is in drawing units. Only explicit height_m is portable.
    }
    serde_json::from_value(value).map_err(|e| e.to_string())
}

fn excel(path: &Path) -> Result<Submission, String> {
    let mut workbook = open_workbook_auto(path).map_err(|e| e.to_string())?;
    if workbook.sheet_names().iter().any(|s| s == "Take-off") {
        let range = workbook
            .worksheet_range("Take-off")
            .map_err(|e| e.to_string())?;
        let mut rows = range.rows();
        let header = rows.next().ok_or("empty Take-off sheet")?;
        let column = |name: &str| {
            header
                .iter()
                .position(|v| v.to_string().eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("missing Take-off column {name}"))
        };
        let description = column("Description")?;
        let kind = column("Kind")?;
        let quantity = column("Quantity")?;
        let unit = column("Unit")?;
        let id = column("Id")?;
        let mut items = Vec::new();
        for row in rows {
            if row.iter().all(|v| matches!(v, Data::Empty)) {
                break;
            }
            let text = |i: usize| row.get(i).map(ToString::to_string).unwrap_or_default();
            items.push(Item {
                id: text(id),
                description: text(description),
                kind: text(kind),
                unit: text(unit),
                quantity: text(quantity)
                    .parse()
                    .map_err(|_| "invalid take-off quantity")?,
                basis: "geometric".into(),
                ..Default::default()
            });
        }
        return Ok(Submission {items,warnings:vec!["Qiyas Excel exports do not contain source handles; use JSON for handle traceability.".into()],..Default::default()});
    }
    let boq = qiyas_boq::import_boq(path).map_err(|e| e.to_string())?;
    if boq.skipped_rows != 0 {
        return Err(format!(
            "BOQ has {} nonnumeric item rows; correct them before scoring",
            boq.skipped_rows
        ));
    }
    let items = boq
        .items
        .into_iter()
        .map(|i| {
            Ok(Item {
                kind: kind_from_unit(&i.unit)?,
                id: i.code,
                description: i.description,
                unit: i.unit,
                quantity: i.quantity,
                basis: "geometric".into(),
                ..Default::default()
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Submission {
        items,
        ..Default::default()
    })
}

/// Submit measured elements and retain generated BOQ totals with their exact components.
/// This boundary takes only prediction artefacts, never benchmark truth.
pub fn from_session(session: &qiyas_core::Session) -> Result<Submission, String> {
    from_report(
        &session.measurement_report().map_err(|e| e.to_string())?,
        &session.recognition_evidence().map_err(|e| e.to_string())?,
        &session.drawing,
    )
}

pub fn from_report(
    report: &qiyas_core::measurement::MeasurementReport,
    evidence: &[qiyas_core::recognition::Candidate],
    drawing: &qiyas_core::Drawing,
) -> Result<Submission, String> {
    let mut submission = Submission {
        warnings: report.warnings.clone(),
        ..Default::default()
    };
    for q in &report.quantities {
        let Some(candidate) = evidence.iter().find(|c| {
            c.object == Some(q.object) && c.state == qiyas_core::recognition::ReviewState::Accepted
        }) else {
            continue;
        };
        let rule = report
            .selection
            .effective()
            .and_then(|m| m.rules.iter().find(|r| r.id == q.rule));
        let kind = rule
            .and_then(|r| r.kind.clone())
            .map(Ok)
            .unwrap_or_else(|| kind_from_unit(&q.unit))?;
        let basis = rule
            .and_then(|r| r.basis.clone())
            .unwrap_or_else(|| legacy_basis(q, &kind));
        if rule.is_none_or(|r| r.basis.is_none()) && basis != "geometric" {
            submission.warnings.push(format!(
                "{} has no declared basis; retained as {} from its saved recipe",
                q.rule, basis
            ));
        }
        let height_m = geometric_height(q, &kind);
        let metadata = drawing
            .element_metadata(q.object)
            .ok_or_else(|| format!("No element position for {:X}", q.object))?;
        submission.items.push(Item {
            id: format!("dimension:{:X}:{}:{}", q.object, q.rule, q.unit),
            description: metadata.label.unwrap_or_else(|| {
                q.lines
                    .first()
                    .map(|l| l.label.clone())
                    .unwrap_or_else(|| q.rule.clone())
            }),
            position: Some(metadata.position),
            kind,
            unit: q.unit.clone(),
            quantity: q.quantity,
            confidence: Some(q.confidence),
            source_objects: source_objects(candidate),
            basis,
            height_m,
            rule: Some(q.rule.clone()),
            citation: Some(q.citation.clone()),
            dimension_lines: q.lines.clone(),
            ..Default::default()
        });
    }
    for item in &report.items {
        // Consume whole dimension recipes, including excluded deductions and rounding lines.
        let mut remaining = item.lines.clone();
        let mut components = Vec::new();
        for q in &submission.items {
            if !q.aggregate_of.is_empty() || q.dimension_lines.is_empty() {
                continue;
            }
            let mut rest = remaining.clone();
            let fits = q.dimension_lines.iter().all(|line| {
                if let Some(i) = rest.iter().position(|l| l == line) {
                    rest.remove(i);
                    true
                } else {
                    false
                }
            });
            if fits {
                remaining = rest;
                components.push(q.clone());
            }
        }
        if !remaining.is_empty() || components.is_empty() {
            submission.warnings.push(format!(
                "{} lacks complete accepted dimension sources; not submitted",
                item.code
            ));
            continue;
        }
        let first = &components[0];
        let homogeneous = components
            .iter()
            .all(|q| q.kind == first.kind && q.basis == first.basis);
        let sources: std::collections::BTreeSet<_> = components
            .iter()
            .flat_map(|q| q.source_objects.clone())
            .collect();
        submission.items.push(Item {
            id: item.code.clone(),
            description: item.description_en.clone(),
            position: group_position(&components),
            kind: if homogeneous {
                first.kind.clone()
            } else {
                kind_from_unit(&item.unit)?
            },
            basis: if homogeneous {
                first.basis.clone()
            } else {
                format!("rule:mixed:{}", item.code)
            },
            unit: item.unit.clone(),
            quantity: item.quantity,
            confidence: Some(item.confidence),
            source_objects: sources.into_iter().collect(),
            aggregate_of: components.iter().map(|q| q.id.clone()).collect(),
            dimension_lines: item.lines.clone(),
            ..Default::default()
        });
    }
    Ok(submission)
}

// Legacy artefacts did not declare basis. Only direct geometry and an explicit area-times-
// height recipe establish geometric quantities. Other interpretations keep their own rule ID;
// no drawing-note name or benchmark label is guessed.
fn legacy_basis(q: &qiyas_core::measurement::MeasuredQuantity, kind: &str) -> String {
    let direct = q.lines.len() == 1
        && q.lines[0].included
        && q.lines[0].operation == qiyas_core::measurement::Operation::Add
        && q.lines[0].source_expressions == ["1".to_string(), format!("geometry.{kind}")];
    if direct || geometric_height(q, kind).is_some() {
        "geometric".into()
    } else {
        format!("rule:{}", q.rule)
    }
}

fn geometric_height(q: &qiyas_core::measurement::MeasuredQuantity, kind: &str) -> Option<f64> {
    let line = q.lines.first()?;
    if kind != "volume"
        || q.lines.len() != 1
        || !line.included
        || line.operation != qiyas_core::measurement::Operation::Add
        || line.source_expressions.len() != 3
        || line.source_expressions[0] != "1"
        || line.source_expressions[1] != "geometry.area"
    {
        return None;
    }
    // The saved factor, not a truth quantity or ratio, is the cited height in metres.
    let height = line.source_expressions[2].parse::<f64>().ok()?;
    (height.is_finite() && height > 0.0).then_some(height)
}

/// An aggregate is redundant only when every complete dimension item is present and the
/// quantities, sources and dimension lines reproduce it. A label alone earns no exemption.
pub fn aggregate_components(items: &[Item], index: usize) -> Option<Vec<usize>> {
    let aggregate = &items[index];
    if aggregate.aggregate_of.is_empty() {
        return None;
    }
    let mut components = Vec::new();
    let mut sources = BTreeSet::new();
    let mut lines = aggregate.dimension_lines.clone();
    let mut total = 0.0;
    for id in &aggregate.aggregate_of {
        let found: Vec<_> = items
            .iter()
            .enumerate()
            .filter(|(_, q)| &q.id == id)
            .collect();
        if found.len() != 1 {
            return None;
        }
        let (i, q) = found[0];
        if i == index
            || components.contains(&i)
            || !q.aggregate_of.is_empty()
            || q.kind != aggregate.kind
            || q.basis != aggregate.basis
            || unit(&q.unit) != unit(&aggregate.unit)
            || q.source_objects.is_empty()
        {
            return None;
        }
        for line in &q.dimension_lines {
            let at = lines.iter().position(|l| l == line)?;
            lines.remove(at);
        }
        sources.extend(objects(q));
        total += q.quantity;
        components.push(i);
    }
    if !lines.is_empty()
        || sources != objects(aggregate)
        || (total - aggregate.quantity).abs() > 1e-10 * total.abs().max(1.0)
    {
        return None;
    }
    Some(components)
}

/// Page evidence must not masquerade as handles in the original CAD drawing.
pub(super) fn source_objects(candidate: &qiyas_core::recognition::Candidate) -> Vec<String> {
    let pages: BTreeSet<_> = candidate
        .evidence
        .iter()
        .filter_map(|e| {
            Some(format!(
                "pdf:{}#page={};object={:X}",
                e.path.as_ref()?,
                e.page?,
                candidate.handle
            ))
        })
        .collect();
    if pages.is_empty() {
        vec![format!("{:X}", candidate.handle)]
    } else {
        pages.into_iter().collect()
    }
}

/// Totals use the unweighted centre of their distinct source elements, never quantities.
pub(super) fn group_position(items: &[Item]) -> Option<[f64; 2]> {
    let mut sources = BTreeSet::new();
    let mut points = Vec::new();
    for item in items {
        if sources.insert(&item.source_objects) {
            points.push(item.position?);
        }
    }
    (!points.is_empty()).then(|| {
        [
            points.iter().map(|p| p[0]).sum::<f64>() / points.len() as f64,
            points.iter().map(|p| p[1]).sum::<f64>() / points.len() as f64,
        ]
    })
}
