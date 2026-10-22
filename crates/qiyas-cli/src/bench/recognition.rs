//! Submitted recognition recipes contain interpretation, never quantities. This runner only
//! reads drawing files and recipe data; the scorer owns truth.json and the gold quantities.
use super::model::{Item, Submission};
use qiyas_core::{objects::ObjectFilter, recognition::ReviewState, Session, Source};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recipe {
    commands: Vec<Value>,
    measurements: Vec<Measure>,
    #[serde(default)]
    include_baseline: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Measure {
    #[serde(default = "qiyas_core::trust::unscored")]
    confidence: f64,
    object_type: String,
    quantity: String,
    kind: String,
    unit: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    description: String,
    #[serde(default = "geometric")]
    basis: String,
    #[serde(default)]
    aggregate: bool,
    #[serde(default)]
    group_by: Option<String>,
    #[serde(default)]
    height_property: Option<String>,
}
fn geometric() -> String {
    "geometric".into()
}

pub fn measure_general(drawing: &Path, recipe_path: &Path) -> Result<Submission, String> {
    measure_with_scope(drawing, recipe_path, true)
}
pub fn measure(drawing: &Path, recipe_path: &Path) -> Result<Submission, String> {
    measure_with_scope(drawing, recipe_path, false)
}
fn measure_with_scope(
    drawing: &Path,
    recipe_path: &Path,
    general: bool,
) -> Result<Submission, String> {
    let started = Instant::now();
    let recipe: Recipe =
        serde_json::from_slice(&std::fs::read(recipe_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if general {
        validate_general(&recipe)?;
    }
    let pdf = drawing
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let mut session = if pdf {
        Session::new_blank(&drawing.with_extension("recognition.dxf"), 6)
    } else {
        Session::open_standalone(drawing).map_err(|e| e.to_string())?
    };
    let mut submission = Submission::default();
    if recipe.include_baseline {
        submission.warnings.push(
            "Legacy include_baseline ignored: candidates need explicit element mappings".into(),
        );
    }
    let mut commands = Vec::new();
    for mut input in recipe.commands {
        let command = input["type"].as_str().ok_or("Recipe command needs type")?;
        if !matches!(
            command,
            "define_object_type"
                | "redefine_object_type"
                | "recognition"
                | "import_pdf"
                | "attach_pdf_underlay"
        ) {
            return Err(format!("{command} is not a recognition recipe command"));
        }
        // File-bearing operations may only read files in this development case directory.
        let file = if command == "recognition" && input["kind"] == "vision" {
            input.get_mut("spec").and_then(|s| s.get_mut("path"))
        } else {
            input.get_mut("path")
        };
        if let Some(file) = file {
            let relative = file.as_str().ok_or("Recipe path must be text")?;
            let path = super::safe_file(
                drawing.parent().unwrap_or(Path::new(".")),
                Path::new(relative),
            )?
            .canonicalize()
            .map_err(|e| e.to_string())?;
            *file = Value::String(path.to_string_lossy().into_owned());
        }
        commands.push(input);
    }
    qiyas_ai::surface::run_session(
        &mut session,
        qiyas_ai::surface::EXECUTE,
        "batch",
        &serde_json::json!({"commands":commands}),
        Source::User,
    )
    .map_err(|e| e.to_string())?;
    let objects = session
        .objects(&ObjectFilter::default())
        .map_err(|e| e.to_string())?;
    let evidence = session.recognition_queue(None).map_err(|e| e.to_string())?;
    for (measurement_index, measurement) in recipe.measurements.into_iter().enumerate() {
        if !measurement.confidence.is_finite() || !(0.0..=1.0).contains(&measurement.confidence) {
            return Err("Measurement confidence must be in 0..1".into());
        }
        let found: Vec<_> = objects
            .iter()
            .filter(|o| o.instance.type_name == measurement.object_type && o.valid)
            .collect();
        let mut items = Vec::new();
        let mut groups = std::collections::BTreeMap::<String, Vec<Item>>::new();
        for object in found {
            let quantity = *object
                .quantities
                .get(&measurement.quantity)
                .or_else(|| object.facts.get(&measurement.quantity))
                .ok_or_else(|| {
                    format!(
                        "No quantity {} on {}",
                        measurement.quantity, measurement.object_type
                    )
                })?;
            let candidate = evidence
                .iter()
                .find(|c| c.object == Some(object.instance.id));
            if candidate.is_none_or(|c| c.state != ReviewState::Accepted) {
                continue;
            }
            let candidate = candidate.expect("accepted candidate");
            let metadata = session
                .drawing
                .element_metadata(object.instance.id)
                .ok_or_else(|| format!("No element position for {:X}", object.instance.id))?;
            let mut item = Item {
                confidence: Some(candidate.confidence.min(measurement.confidence)),
                id: measurement.id.clone(),
                description: metadata
                    .label
                    .unwrap_or_else(|| measurement.description.clone()),
                position: Some(metadata.position),
                kind: measurement.kind.clone(),
                unit: measurement.unit.clone(),
                quantity,
                source_objects: super::submission::source_objects(candidate),
                basis: measurement.basis.clone(),
                height_m: measurement
                    .height_property
                    .as_ref()
                    .and_then(|p| object.instance.props.get(p))
                    .and_then(Value::as_f64),
                ..Default::default()
            };
            super::model::validate_item(&item)?;
            if measurement.aggregate || measurement.group_by.is_some() {
                item.id = format!("dimension:{}:{:X}", measurement_index, object.instance.id);
            }
            if let Some(property) = &measurement.group_by {
                let group = object
                    .instance
                    .props
                    .get(property)
                    .ok_or_else(|| format!("Missing grouping property {property}"))?
                    .to_string();
                groups.entry(group).or_default().push(item);
            } else {
                items.push(item);
            }
        }
        for (_, group) in groups {
            append_group(&mut submission, group, &measurement.id);
        }
        if measurement.aggregate && !items.is_empty() {
            append_group(&mut submission, items, &measurement.id);
        } else {
            submission.items.extend(items);
        }
    }
    let pending = evidence
        .iter()
        .filter(|c| c.state == ReviewState::Pending)
        .count();
    if pending > 0 {
        submission.warnings.push(format!(
            "{pending} recognition candidates await review and are not submitted"
        ));
    }
    submission.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(submission)
}

fn append_group(submission: &mut Submission, items: Vec<Item>, id: &str) {
    let mut total = items[0].clone();
    total.id = id.into();
    total.position = super::submission::group_position(&items);
    total.quantity = items.iter().map(|i| i.quantity).sum();
    total.confidence = items
        .iter()
        .map(|i| i.confidence.unwrap_or(qiyas_core::trust::unscored()))
        .reduce(f64::min);
    total.source_objects = items
        .iter()
        .flat_map(|i| i.source_objects.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    total.aggregate_of = items.iter().map(|i| i.id.clone()).collect();
    submission.items.extend(items);
    submission.items.push(total);
}

// Shared recipes may use selectors and extracted evidence, never object handles, literal
// placements, case IDs, known quantities or source files from an individual case.
fn validate_general(recipe: &Recipe) -> Result<(), String> {
    fn inspect(value: &Value) -> Result<(), String> {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.as_str(),
                        "handle"
                            | "handles"
                            | "points"
                            | "path"
                            | "ids"
                            | "within"
                            | "at"
                            | "calibration"
                    ) && !value.is_null()
                        && value.as_array().is_none_or(|a| !a.is_empty())
                    {
                        return Err(format!("General rules cannot contain case-specific {key}"));
                    }
                    if key == "kind" && value == "constant" {
                        return Err("General rules must extract property evidence, not supply case constants".into());
                    }
                    inspect(value)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    inspect(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for command in &recipe.commands {
        if !matches!(
            command["type"].as_str(),
            Some("define_object_type" | "redefine_object_type" | "recognition")
        ) {
            return Err("General recipes only define types and recognition".into());
        }
        if command["type"] == "recognition"
            && !matches!(command["kind"].as_str(), Some("define" | "apply"))
        {
            return Err("General recipes only define or apply recognition rules".into());
        }
        inspect(command)?;
    }
    if recipe.measurements.iter().any(|m| !m.id.is_empty()) {
        return Err("General rules cannot prescribe truth item IDs".into());
    }
    Ok(())
}

/// Shared diagnostic commands have the same no-path, no-handle boundary as shared recognition.
pub(super) fn validate_general_commands(commands: &[Value]) -> Result<(), String> {
    validate_general(&Recipe {
        commands: commands.to_vec(),
        measurements: vec![],
        include_baseline: false,
    })
}

/// Build an ordinary measured A from the supplied interpretation. The revision predictor
/// receives only drawing paths and this recipe, never the benchmark's expected deltas.
pub(super) fn revision_base(
    drawing: &Path,
    recipe_path: &Path,
    general: bool,
    supplement: Option<&Path>,
) -> Result<Session, String> {
    let mut recipe: Recipe =
        serde_json::from_slice(&std::fs::read(recipe_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if general {
        validate_general(&recipe)?;
    }
    let primary_count = recipe.measurements.len();
    if let Some(path) = supplement {
        let mut extra: Recipe =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        validate_general(&extra)?;
        // Prepared interpretation owns its matched scope. Shared rules fill remaining
        // sources, with the same precedence when carry reapplies every stored rule on B.
        let defines = |c: &&Value| c["type"] == "recognition" && c["kind"] == "define";
        let primary_min = recipe
            .commands
            .iter()
            .filter(defines)
            .map(|c| c["rule"]["priority"].as_i64().unwrap_or(0))
            .min()
            .unwrap_or(0);
        let extra_max = extra
            .commands
            .iter()
            .filter(defines)
            .map(|c| c["rule"]["priority"].as_i64().unwrap_or(0))
            .max()
            .unwrap_or(0);
        let shift = primary_min - extra_max - 1;
        for command in extra
            .commands
            .iter_mut()
            .filter(|c| c["type"] == "recognition" && c["kind"] == "define")
        {
            let priority = command["rule"]["priority"].as_i64().unwrap_or(0) + shift;
            let priority = i32::try_from(priority)
                .map_err(|_| "Supplemental recognition priorities exceed their supported range")?;
            command["rule"]["priority"] = serde_json::json!(priority);
        }
        recipe.commands.extend(extra.commands);
        recipe.measurements.extend(extra.measurements);
    }
    let mut s = Session::open_standalone(drawing).map_err(|e| e.to_string())?;
    for command in recipe.commands {
        s.run_input(&command, Source::User)
            .map_err(|e| e.to_string())?;
    }
    let mut rules = vec![];
    let mut mappings = vec![];
    for (index, m) in recipe.measurements.iter().enumerate() {
        let source = if index < primary_count {
            recipe_path
        } else {
            supplement.expect("supplemental mapping")
        };
        let citation = serde_json::json!({"source":source.to_string_lossy(),"clause":"Supplied recognition quantity mapping"});
        let id = format!("quantity-{index}");
        rules.push(serde_json::json!({"id":id,"element_type":m.object_type,"unit":m.unit,"terms":[{"label":m.quantity,"operation":"add","factors":[m.quantity],"citation":citation}],"citation":citation}));
        let base = if m.id.is_empty() {
            format!("mapping-{index}")
        } else {
            m.id.clone()
        };
        let code = if let Some(property) = &m.group_by {
            format!("{base}-{{{property}}}")
        } else if m.aggregate {
            base
        } else {
            format!("{base}-{{id}}")
        };
        let description = if m.description.is_empty() {
            m.object_type.clone()
        } else {
            m.description.clone()
        };
        mappings.push(serde_json::json!({"rule":id,"code":code,"description_en":description,"description_ar":description,"section":"","trade":"","unit":m.unit,"citation":citation}));
    }
    s.run_input(&serde_json::json!({"type":"measurement","kind":"set_method","scope":"project","method":{"name":"supplied","title":"Supplied geometric mapping","based_on":"Drawing interpretation","source_note":"Engine diagnostic","rules":rules}}),Source::User).map_err(|e|e.to_string())?;
    s.run_input(
        &serde_json::json!({"type":"measurement","kind":"set_mappings","mappings":mappings}),
        Source::User,
    )
    .map_err(|e| e.to_string())?;
    Ok(s)
}
