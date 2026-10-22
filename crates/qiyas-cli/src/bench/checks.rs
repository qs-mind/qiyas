//! Prediction reads only the drawing, client inputs, submitted evidence and shared check data.
use super::model::{Finding, Item, Submission};
use qiyas_core::{Boq, BoqItem, Command, ConflictKind, Kind, Measurement, Session, Source};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

/// Evidence identity is resolved to truth ids only by the scorer, after prediction ends.
pub fn evidence_id(item: &Item) -> String {
    let mut handles = item.source_objects.clone();
    handles.sort();
    format!(
        "evidence:{}:{}:{}",
        item.kind,
        item.basis,
        handles.join(",")
    )
}
#[derive(Deserialize)]
struct Data {
    tolerance: f64,
    missing_scopes: Vec<Scope>,
    citation: qiyas_core::measurement::Citation,
    commands: Vec<Value>,
    schedules: Vec<ScheduleRule>,
}
#[derive(Deserialize)]
struct Scope {
    kind: String,
    basis: String,
}
#[derive(Deserialize)]
struct ScheduleRule {
    columns: Vec<String>,
    elements: Value,
    property: String,
    key_column: usize,
    value_column: usize,
    question_en: String,
    question_ar: String,
}

pub fn predict(root: &Path, drawing: &Path, submission: &mut Submission) -> Result<(), String> {
    if drawing
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        return Ok(());
    }
    let directory = drawing.parent().ok_or("Missing drawing directory")?;
    let input = ["input-boq.json", "clean-boq.json"]
        .into_iter()
        .find(|n| directory.join(n).exists());
    let Some(input) = input else { return Ok(()) };
    let data_path = root.join("general/checks.json");
    if !data_path.exists() {
        submission
            .warnings
            .push("No shared cited check data supplied".into());
        return Ok(());
    }
    let data: Data = serde_json::from_slice(
        &std::fs::read(super::safe_file(root, Path::new("general/checks.json"))?)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    super::recognition::validate_general_commands(&data.commands)?;
    if data.citation.source.trim().is_empty() || data.citation.clause.trim().is_empty() {
        return Err("Shared checks need a source and clause".into());
    }
    let value: Value = serde_json::from_slice(
        &std::fs::read(super::safe_file(directory, Path::new(input))?)
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let client: Vec<Item> =
        serde_json::from_value(value["items"].clone()).map_err(|e| e.to_string())?;
    let mut s = Session::open_standalone(drawing).map_err(|e| e.to_string())?;
    for command in data.commands {
        s.run_input(&command, Source::User)
            .map_err(|e| e.to_string())?;
    }
    let mut measured = submission.items.clone();
    // Supplemental interpretations may verify client scope but cannot invent extra BOQ scope.
    let supplemental = root.join("general/check-recognition.json");
    if supplemental.exists() {
        let extra = super::recognition::measure_general(
            drawing,
            &super::safe_file(root, Path::new("general/check-recognition.json"))?,
        )?;
        for item in extra.items {
            if client.iter().any(|c| same_sources(c, &item))
                && !measured.iter().any(|m| same_sources(m, &item))
            {
                measured.push(item);
            }
        }
    }
    let mut measured = comparison_items(&measured, &client);
    let unverified = client
        .iter()
        .filter(|c| !measured.iter().any(|m| same_sources(c, m)))
        .count();
    if unverified > 0 {
        submission.warnings.push(format!("{unverified} client rows have no verified source mapping; quantities and drawing absence remain unchecked"));
    }
    s.boq = Some(Boq::new(
        input.into(),
        client
            .iter()
            .filter(|c| measured.iter().any(|m| same_sources(c, m)))
            .enumerate()
            .map(|(i, c)| BoqItem {
                code: c.id.clone(),
                description: c.description.clone(),
                unit: c.unit.clone(),
                quantity: c.quantity,
                rate: None,
                section: String::new(),
                tolerance: Some(data.tolerance),
                row: (i + 1) as u32,
            })
            .collect(),
        0,
    ));
    // Only scopes declared complete by the supplied interpretation can establish missing work.
    // Other interpretations can still verify a matching client item, without asserting completeness.
    measured.retain(|m| {
        client.iter().any(|c| same_sources(c, m))
            || data
                .missing_scopes
                .iter()
                .any(|scope| scope.kind == m.kind && scope.basis == m.basis)
    });
    for item in &measured {
        let candidates: Vec<_> = client.iter().filter(|c| same_sources(c, item)).collect();
        let code = if candidates.len() == 1 {
            candidates[0].id.clone()
        } else {
            evidence_id(item)
        };
        let kind = match item.kind.as_str() {
            "area" => Kind::Area,
            "length" => Kind::Length,
            "volume" => Kind::Volume,
            "count" => Kind::Count,
            _ => Kind::Item,
        };
        let mut m = Measurement::new(
            kind,
            item.quantity,
            s.drawing.units,
            item.description.clone(),
            String::new(),
            "*Model_Space".into(),
        );
        m.shown = item.quantity;
        m.unit = item.unit.clone();
        m.id = code.clone();
        m.boq_code = Some(code);
        m.handles = item
            .source_objects
            .iter()
            .filter_map(|h| u64::from_str_radix(h, 16).ok())
            .collect();
        s.takeoff.items.push(m);
    }
    for sheet in s.drawing.sheets() {
        for table in s.read_tables(Some(&sheet.block)) {
            for rule in data.schedules.iter().filter(|r| r.columns == table.columns) {
                let set: qiyas_core::discrepancy::Set =
                    serde_json::from_value(rule.elements.clone()).map_err(|e| e.to_string())?;
                if s.objects(&set.filter)
                    .map_err(|e| e.to_string())?
                    .is_empty()
                {
                    submission.warnings.push(format!(
                        "Table {} has no recognised instance set; schedule check remains unchecked",
                        table.source
                    ));
                    continue;
                }

                s.run_input(&json!({"type":"define_check","name":format!("table-{}",table.source),"rule":{"citation":data.citation,"question_en":rule.question_en,"question_ar":rule.question_ar,"test":{"kind":"schedule","table":{"kind":"drawing","handle":table.source,"sheet":sheet.block},"key_column":rule.key_column,"value_column":rule.value_column,"elements":rule.elements,"property":rule.property}}}),Source::User).map_err(|e|e.to_string())?;
            }
        }
    }
    s.run_as(
        Command::Check {
            tolerance: Some(data.tolerance),
        },
        Source::User,
    )
    .map_err(|e| e.to_string())?;
    for conflict in &s.conflicts {
        let kind = match conflict.kind {
            ConflictKind::MissingFromBoq => "missing",
            ConflictKind::QuantityMismatch => "wrong_quantity",
            ConflictKind::UnitDoubt => "wrong_unit",
            ConflictKind::ScheduleVsPlan => "schedule_mismatch",
            ConflictKind::NotInDrawings => "not_in_drawings",
            ConflictKind::DuplicateItem => "duplicate",
            _ => "relation",
        };
        let item_id = if let Some(code) = &conflict.boq_code {
            code.clone()
        } else {
            let handles: Vec<_> = conflict
                .query
                .iter()
                .flat_map(|q| &q.references)
                .map(|r| format!("{:X}", r.handle))
                .collect();
            let candidates: Vec<_> = client
                .iter()
                .filter(|c| {
                    c.kind == "count"
                        && !c.source_objects.is_empty()
                        && c.source_objects.iter().all(|h| handles.contains(h))
                })
                .collect();
            if candidates.len() == 1 {
                candidates[0].id.clone()
            } else {
                format!("query:{}", conflict.id)
            }
        };
        submission.findings.push(Finding {
            item_id,
            kind: kind.into(),
        });
    }
    Ok(())
}
// Compare at the client's granularity, or retain complete totals for missing-scope checks.
// A dimension row and its verified total must never become two independent measurements.
fn comparison_items(items: &[Item], client: &[Item]) -> Vec<Item> {
    let mut omitted = std::collections::BTreeSet::new();
    for index in 0..items.len() {
        let Some(parts) = super::submission::aggregate_components(items, index) else {
            continue;
        };
        if client.iter().any(|c| same_sources(c, &items[index]))
            || !parts
                .iter()
                .any(|i| client.iter().any(|c| same_sources(c, &items[*i])))
        {
            omitted.extend(parts);
        } else {
            omitted.insert(index);
        }
    }
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| !omitted.contains(i))
        .map(|(_, item)| item.clone())
        .collect()
}

fn same_sources(a: &Item, b: &Item) -> bool {
    let mut a_sources = a.source_objects.clone();
    a_sources.sort();
    let mut b_sources = b.source_objects.clone();
    b_sources.sort();
    a.kind == b.kind && a.basis == b.basis && !a_sources.is_empty() && a_sources == b_sources
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn predicts_from_client_and_geometry_with_unreadable_truth() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let case = root.join("case");
        std::fs::create_dir_all(root.join("general")).unwrap();
        std::fs::create_dir(&case).unwrap();
        let drawing = case.join("drawing.dxf");
        let mut s = Session::new_blank(&drawing, 6);
        s.run_input(
            &json!({"type":"draw","kind":"rectangle","a":[0,0],"b":[4,3]}),
            Source::User,
        )
        .unwrap();
        let h = s.find_entities(&Default::default()).rows[0].handle;
        s.save_as(&drawing).unwrap();
        std::fs::write(
            case.join("truth.json"),
            b"Not readable JSON; prediction must never open this",
        )
        .unwrap();
        std::fs::write(root.join("general/checks.json"),json!({"tolerance":1,"missing_scopes":[{"kind":"area","basis":"geometric"}],"citation":{"source":"User brief","clause":"One percent"},"commands":[],"schedules":[]}).to_string()).unwrap();
        let item = Item {
            kind: "area".into(),
            unit: "m2".into(),
            quantity: 12.,
            source_objects: vec![format!("{h:X}")],
            basis: "geometric".into(),
            ..Default::default()
        };
        let mut client = item.clone();
        client.id = "CLIENT".into();
        client.quantity = 20.;
        std::fs::write(
            case.join("input-boq.json"),
            json!({"items":[client]}).to_string(),
        )
        .unwrap();
        let mut dimension = item.clone();
        dimension.id = "dimension".into();
        let mut total = item.clone();
        total.id = "total".into();
        total.aggregate_of = vec![dimension.id.clone()];
        let mut submission = Submission {
            items: vec![dimension, total],
            ..Default::default()
        };
        predict(root, &drawing, &mut submission).unwrap();
        assert_eq!(
            submission.findings,
            vec![Finding {
                item_id: "CLIENT".into(),
                kind: "wrong_quantity".into()
            }]
        );
        client.quantity = 12.;
        std::fs::write(
            case.join("input-boq.json"),
            json!({"items":[client]}).to_string(),
        )
        .unwrap();
        submission.findings.clear();
        predict(root, &drawing, &mut submission).unwrap();
        assert!(
            submission.findings.is_empty(),
            "A correct total and its dimension must not create an alarm"
        );
        client.unit = "m".into();
        std::fs::write(
            case.join("input-boq.json"),
            json!({"items":[client]}).to_string(),
        )
        .unwrap();
        submission.findings.clear();
        predict(root, &drawing, &mut submission).unwrap();
        assert_eq!(submission.findings[0].kind, "wrong_unit");
        std::fs::write(case.join("input-boq.json"), json!({"items":[]}).to_string()).unwrap();
        submission.findings.clear();
        predict(root, &drawing, &mut submission).unwrap();
        assert_eq!(submission.findings[0].item_id, evidence_id(&item));
        assert_eq!(submission.findings[0].kind, "missing");
    }
}
