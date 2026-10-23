//! Offline calibration audit. Generated geometry is opened only after production calibration.
use qiyas_core::recognition::{calibrate_raster, VisionSpec};
use serde_json::{json, Value};
use std::path::Path;

pub fn scans(root: &Path, output: &Path, agent_report: Option<&Path>) -> Result<Value, String> {
    super::refuse_holdout_path(output)?;
    if output.exists() {
        return Err("Calibration output already exists".into());
    }
    let mut rows = Vec::new();
    for id in super::dev_ids(root, false)?
        .into_iter()
        .filter(|id| id.ends_with("/variants/scanned-pdf"))
    {
        let dir = root.join("cases").join(&id);
        let recipe: Value = serde_json::from_slice(
            &std::fs::read(dir.join("recognition.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let spec: VisionSpec = serde_json::from_value(
            recipe["commands"]
                .as_array()
                .ok_or("Missing commands")?
                .iter()
                .find(|v| v["kind"] == "vision")
                .ok_or("Missing vision calibration")?["spec"]
                .clone(),
        )
        .map_err(|e| e.to_string())?;
        rows.push(audit(&dir, &dir.join("drawing.pdf"), &spec, &id)?);
    }
    let mut saved_agent = Vec::new();
    if let Some(path) = agent_report {
        super::refuse_holdout_path(path)?;
        super::reject_symlink(path)?;
        let report: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for case in report["reports"].as_array().ok_or("Missing reports")? {
            let id = case["case"].as_str().ok_or("Missing case")?;
            if !id.ends_with("/variants/scanned-pdf") {
                continue;
            }
            let saved = Path::new(
                case["report_directory"]
                    .as_str()
                    .ok_or("Missing report directory")?,
            );
            super::reject_symlink(saved)?;
            let mut spec = None;
            for line in std::fs::read_to_string(saved.join("agent-events.jsonl"))
                .map_err(|e| e.to_string())?
                .lines()
            {
                let event: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
                if event["type"] != "item.completed" {
                    continue;
                }
                if let Some(args) = event["item"]["arguments"]["args"]
                    .as_str()
                    .and_then(|v| serde_json::from_str::<Value>(v).ok())
                {
                    if let Some(commands) = args["commands"].as_array() {
                        for command in commands.iter().filter(|c| c["kind"] == "vision") {
                            spec = Some(
                                serde_json::from_value::<VisionSpec>(command["spec"].clone())
                                    .map_err(|e| e.to_string())?,
                            );
                        }
                    }
                }
            }
            if let Some(spec) = spec {
                saved_agent.push(audit(
                    &root.join("cases").join(id),
                    &saved.join("source.pdf"),
                    &spec,
                    id,
                )?);
            }
        }
    }
    let summary = |field: &str| {
        let mut values: Vec<_> = rows.iter().map(|r| r[field].as_f64().unwrap()).collect();
        values.sort_by(f64::total_cmp);
        if values.is_empty() {
            return Value::Null;
        }
        let median = (values[(values.len() - 1) / 2] + values[values.len() / 2]) * 0.5;
        json!({"median_percent":median,"maximum_percent":values.last(),
            "under_0_5_percent":values.iter().filter(|v| **v < 0.5).count()})
    };
    let result = json!({"cases":rows.len(),"baseline":"Existing prepared vision anchors, unrefined; no agent run",
        "before":summary("before_length_error_percent"),"after":summary("after_length_error_percent"),"rows":rows,"saved_agent":saved_agent});
    super::write_json(output, &result)?;
    Ok(result)
}

fn audit(dir: &Path, path: &Path, spec: &VisionSpec, id: &str) -> Result<Value, String> {
    let (w, h) = qiyas_pdf::page_size(path, spec.page).map_err(|e| e.to_string())?;
    let pixels =
        qiyas_pdf::render_page(path, spec.page, 144.0, |_| {}).map_err(|e| e.to_string())?;
    let calibrated = calibrate_raster(
        &pixels,
        [w, h],
        &spec.calibration,
        &spec.calibration_spans,
        &spec.dimensions,
    )
    .map_err(|e| e.to_string())?;
    // The function above has finished. Truth is confined to this benchmark audit.
    let truth: Value =
        serde_json::from_slice(&std::fs::read(dir.join("truth.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let bounds: [[f64; 2]; 2] = serde_json::from_value(truth["pdf"]["drawing_bounds"].clone())
        .map_err(|e| e.to_string())?;
    let units = match truth["generator"]["units"].as_str() {
        Some("metres") => 1.0,
        Some("inches") => 0.0254,
        _ => return Err("Unknown generated units".into()),
    };
    let span = &spec.calibration;
    let page_length = (span.b[0] - span.a[0]).hypot(span.b[1] - span.a[1]);
    let generated_length = ((span.b[0] - span.a[0]) * (bounds[1][0] - bounds[0][0]) / w)
        .hypot((span.b[1] - span.a[1]) * (bounds[1][1] - bounds[0][1]) / h)
        * units;
    let error = |scale: f64| (page_length * scale / generated_length - 1.0).abs() * 100.0;
    Ok(
        json!({"case":id,"before_length_error_percent":error(span.drawing_scale(1.0).map_err(|e|e.to_string())?),
            "after_length_error_percent":error(calibrated.calibration.drawing_scale(1.0).map_err(|e|e.to_string())?),"calibration":calibrated}),
    )
}
