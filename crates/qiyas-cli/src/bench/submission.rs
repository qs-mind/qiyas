use std::path::Path;

use calamine::{open_workbook_auto, Data, Reader};
use qiyas_core::Measurement;
use serde_json::Value;

use super::model::{kind_from_unit, validate_item, Item, Submission};

pub fn from_measurement(m: &Measurement, metres_per_unit: f64) -> Item {
    Item {
        id: m.boq_code.clone().unwrap_or_else(|| m.id.clone()),
        description: m.description.clone(),
        kind: serde_json::to_value(m.kind)
            .expect("kind")
            .as_str()
            .unwrap_or("")
            .into(),
        unit: m.unit.clone(),
        quantity: m.shown,
        source_objects: m.handles.iter().map(|h| format!("{h:X}")).collect(),
        basis: "geometric".into(),
        height_m: m.height.map(|h| h * metres_per_unit),
        ..Default::default()
    }
}

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
