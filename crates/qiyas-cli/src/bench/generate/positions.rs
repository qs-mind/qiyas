//! Position-only maintenance of generated truth. Quantities and other bytes are preserved.
use super::*;

fn page_offset(truth: &Value, session: &Session) -> Result<[f64; 2]> {
    if truth["pdf"]["kind"] == "scan" {
        let bounds: [[f64; 2]; 2] =
            serde_json::from_value(truth["pdf"]["drawing_bounds"].clone()).map_err(error)?;
        return Ok(bounds[0].map(|v| -v));
    }
    if truth["pdf"]["kind"] == "vector" {
        let [lo, hi] = session.batch().bounds;
        // The generator plots centred A3 landscape, fitted inside the plotter's margin.
        let margin = qiyas_plot::FIT_MARGIN_MM;
        let scale = ((420.0 - 2.0 * margin) / (hi[0] - lo[0]))
            .min((297.0 - 2.0 * margin) / (hi[1] - lo[1]));
        return Ok([
            420.0 / (2.0 * scale) - (lo[0] + hi[0]) * 0.5,
            297.0 / (2.0 * scale) - (lo[1] + hi[1]) * 0.5,
        ]);
    }
    Ok([0.0; 2])
}

pub(super) fn geometry(item: &Value, drawing: &qiyas_core::Drawing) -> Result<([f64; 2], f64)> {
    let sources = item
        .get("model_source_objects")
        .unwrap_or(&item["source_objects"])
        .as_array()
        .ok_or("Missing position geometry")?;
    let mut handles = std::collections::BTreeSet::new();
    for source in sources {
        handles.insert(
            u64::from_str_radix(source.as_str().ok_or("Invalid position source")?, 16)
                .map_err(error)?,
        );
    }
    let mut centre = [0.0; 2];
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    let mut diagonal: f64 = 0.0;
    for handle in &handles {
        let metadata = drawing
            .element_metadata(*handle)
            .ok_or("Missing position source geometry")?;
        for axis in 0..2 {
            centre[axis] += metadata.position[axis];
            lo[axis] = lo[axis].min(metadata.position[axis]);
            hi[axis] = hi[axis].max(metadata.position[axis]);
        }
        diagonal = diagonal.max(metadata.diagonal);
    }
    if handles.is_empty() {
        return Err("Empty position sources".into());
    }
    let metres = drawing.units.to_metres();
    Ok((
        centre.map(|v| v * metres / handles.len() as f64),
        diagonal.max((hi[0] - lo[0]).hypot(hi[1] - lo[1])) * metres,
    ))
}

pub(super) fn update(truth: &mut Value, session: &Session) -> Result<()> {
    let offset = page_offset(truth, session)?.map(|v| v * session.drawing.units.to_metres());
    for item in truth["items"].as_array_mut().ok_or("Missing items")? {
        let (position, _) = geometry(item, &session.drawing)?;
        item["position"] = json!([position[0] + offset[0], position[1] + offset[1]]);
    }
    Ok(())
}

fn replace(original: &str, updated: &Value) -> Result<String> {
    // Only replace generated item position arrays. All other bytes, including number
    // literals and whitespace outside these fields, remain exactly as stored.
    let marker = "\n      \"position\": ";
    let mut result = String::new();
    let mut at = 0;
    for item in updated["items"].as_array().ok_or("Missing items")? {
        let start = original[at..]
            .find(marker)
            .ok_or("Missing generated item position")?
            + at
            + marker.len();
        let mut stream =
            serde_json::Deserializer::from_str(&original[start..]).into_iter::<Value>();
        let old = stream
            .next()
            .ok_or("Missing position value")?
            .map_err(error)?;
        let end = start + stream.byte_offset();
        result.push_str(&original[at..start]);
        if old == item["position"] {
            result.push_str(&original[start..end]);
        } else {
            result.push_str(
                &serde_json::to_string_pretty(&item["position"])
                    .map_err(error)?
                    .replace('\n', "\n      "),
            );
        }
        at = end;
    }
    result.push_str(&original[at..]);
    Ok(result)
}

pub fn regenerate(root: &Path) -> Result<Value> {
    let mut changes = Vec::new();
    for id in super::super::dev_ids(root, false)? {
        let directory = root.join("cases").join(&id);
        let path = directory.join("truth.json");
        let original = fs::read_to_string(&path).map_err(error)?;
        let mut truth: Value = serde_json::from_str(&original).map_err(error)?;
        if truth["source"] != "qiyas-generator-v1" {
            continue;
        }
        let source = if truth["pdf"].is_object() {
            root.join("cases")
                .join(id.split('/').next().ok_or("Missing case")?)
                .join("drawing.dxf")
        } else {
            directory.join(truth["drawing"].as_str().ok_or("Missing drawing")?)
        };
        super::super::reject_symlink(&source)?;
        let session = Session::open_standalone(&source).map_err(error)?;
        update(&mut truth, &session)?;
        let updated = replace(&original, &truth)?;
        if original == updated {
            continue;
        }
        safe_output(&path)?;
        fs::write(&path, &updated).map_err(error)?;
        changes.push(
            json!({"case":id,"before_sha256":format!("{:x}",Sha256::digest(original.as_bytes())),
            "after_sha256":format!("{:x}",Sha256::digest(updated.as_bytes()))}),
        );
    }
    let result = json!({"operation":"geometry_positions_v1","changed_files":changes.len(),
        "fields":["items[].position"],"quantity_bytes":"unchanged",
        "convention":"region centroid; open path half-length point; insert origin; distinct-source mean; metres; PDF page origin",
        "changes":changes});
    if !changes.is_empty() {
        use std::io::Write;
        let log = root.join("sources/qiyas-generator/position-provenance.jsonl");
        safe_output(&log)?;
        fs::create_dir_all(log.parent().ok_or("Missing log parent")?).map_err(error)?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(error)?;
        writeln!(file, "{}", serde_json::to_string(&result).map_err(error)?).map_err(error)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn position_refresh_changes_only_positions_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let case = dir.path().join("cases/sample");
        fs::create_dir_all(&case).unwrap();
        let mut project = build(&case.join("drawing.dxf"), TYPES[0], 41, false).unwrap();
        project.truth["id"] = json!("sample");
        save(&project, &case).unwrap();
        let path = case.join("truth.json");
        let original = fs::read_to_string(&path).unwrap();
        let mut old = project.truth.clone();
        for item in old["items"].as_array_mut().unwrap() {
            item["position"] = json!([-99.0, -99.0]);
        }
        let old = replace(&original, &old).unwrap();
        fs::write(&path, &old).unwrap();
        assert_eq!(regenerate(dir.path()).unwrap()["changed_files"], 1);
        let updated = fs::read_to_string(&path).unwrap();
        assert_eq!(updated, original);
        assert_eq!(regenerate(dir.path()).unwrap()["changed_files"], 0);
        let log = fs::read_to_string(
            dir.path()
                .join("sources/qiyas-generator/position-provenance.jsonl"),
        )
        .unwrap();
        assert_eq!(log.lines().count(), 1);
        assert!(!dir.path().join("holdout").exists());
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;
    #[test]
    fn scan_centroids_use_calibrated_page_origin_and_keep_quantities() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = build(&dir.path().join("drawing.dxf"), TYPES[0], 41, false).unwrap();
        let original = project.truth["items"].clone();
        project.truth["drawing"] = json!("drawing.pdf");
        project.truth["pdf"] = json!({"kind":"scan","drawing_bounds":[[-2,-12],[20,10]]});
        update(&mut project.truth, &project.session).unwrap();
        for (before, after) in original
            .as_array()
            .unwrap()
            .iter()
            .zip(project.truth["items"].as_array().unwrap())
        {
            assert_eq!(before["quantity"], after["quantity"]);
            assert_eq!(
                after["position"],
                json!([
                    before["position"][0].as_f64().unwrap() + 2.0,
                    before["position"][1].as_f64().unwrap() + 12.0
                ])
            );
        }
        let truth: crate::bench::model::Truth = serde_json::from_value(project.truth).unwrap();
        let mut observed = truth.items[0].clone();
        observed.id = "observed".into();
        observed.source_objects = vec!["pdf:source.pdf#page=1;object=1".into()];
        observed.position.as_mut().unwrap()[0] += 0.2;
        let report = crate::bench::score::score(
            &truth,
            &crate::bench::model::Submission {
                items: vec![observed],
                ..Default::default()
            },
            None,
        );
        assert_eq!(report.metrics.matched, 1);
        assert_eq!(
            report.items[0].matched_by.as_deref(),
            Some("description_position")
        );
    }
}
