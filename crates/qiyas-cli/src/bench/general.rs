//! Predictions only see the drawing and shared rules, never truth or case-local recipes.
use super::model::{Item, Submission};
use qiyas_core::{recognition::PdfTextRules, Session, Source};
use serde_json::json;
use std::path::Path;
use std::time::Instant;

pub fn measure(root: &Path, drawing: &Path) -> Result<Submission, String> {
    if drawing
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        return pdf(root, drawing);
    }
    let shared = root.join("general/recognition.json");
    if shared.exists() {
        let shared = super::safe_file(root, Path::new("general/recognition.json"))?;
        super::recognition::measure_general(drawing, &shared)
    } else {
        super::baseline::measure(drawing)
    }
}
fn pdf(root: &Path, drawing: &Path) -> Result<Submission, String> {
    let started = Instant::now();
    let rules_path = super::safe_file(root, Path::new("general/pdf.json"))?;
    let rules: PdfTextRules =
        serde_json::from_slice(&std::fs::read(rules_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut submission = Submission::default();
    let pages = qiyas_pdf::page_count(drawing).map_err(|e| e.to_string())?;
    for page in 1..=pages {
        let mut session = Session::new_blank(&drawing.with_extension("analysis.dxf"), 6);
        let command = json!({"type":"recognition","kind":"pdf_text","path":drawing,"page":page,"rule":rules.calibration});
        if let Err(e) = session.run_input(&command, Source::User) {
            submission.warnings.push(format!(
                "Page {page}: {e}; no quantities inferred without calibration"
            ));
            continue;
        }
        let read = session
            .read_pdf_text_areas(&rules)
            .map_err(|e| e.to_string())?;
        submission.warnings.extend(read.warnings);
        // The configured label reader supplies recognition evidence. Materialise a typed
        // element before its explicit area mapping is eligible for submission.
        session.run_input(&json!({"type":"define_object_type","definition":{"name":"LabelledEnclosure","category":"PDF evidence","geometry":"region","quantities":{"surface":"area"}}}),Source::User).map_err(|e|e.to_string())?;
        for (index, area) in read.areas.into_iter().enumerate() {
            let Some(handle) = area
                .boundary_handle
                .or_else(|| area.label_handles.first().copied())
            else {
                continue;
            };
            let rule = format!("labelled-enclosure-{index}");
            session.run_input(&json!({"type":"recognition","kind":"define","rule":{"name":rule,"object_type":"LabelledEnclosure","match":{"handles":[handle]},"confidence":0.95,"sources":[{"handle":handle,"path":drawing,"page":page,"detail":"Enclosure identified by configured label pattern"}]}}),Source::User).map_err(|e|e.to_string())?;
            session
                .run_input(
                    &json!({"type":"recognition","kind":"apply","rules":[rule]}),
                    Source::User,
                )
                .map_err(|e| e.to_string())?;
            let evidence = session.recognition_queue(None).map_err(|e| e.to_string())?;
            let Some(candidate) = evidence.iter().find(|c| {
                c.handle == handle && c.state == qiyas_core::recognition::ReviewState::Accepted
            }) else {
                submission.warnings.push(format!(
                    "{}: unresolved enclosure remains a candidate",
                    area.description
                ));
                continue;
            };
            let objects = session
                .objects(&Default::default())
                .map_err(|e| e.to_string())?;
            let Some(object) = objects
                .iter()
                .find(|o| Some(o.instance.id) == candidate.object && o.valid)
            else {
                continue;
            };
            let quantity = *object
                .quantities
                .get("surface")
                .ok_or("Missing mapped enclosure area")?;
            let description = read
                .legends
                .get(&area.description)
                .cloned()
                .unwrap_or(area.description);
            submission.items.push(Item {
                description,
                kind: "area".into(),
                unit: "m2".into(),
                quantity,
                source_objects: vec![format!(
                    "pdf:{}#page={page};label={};boundary={};seed={},{}",
                    drawing.display(),
                    area.label_handles
                        .iter()
                        .map(|h| format!("{h:X}"))
                        .collect::<Vec<_>>()
                        .join(","),
                    area.boundary_handle
                        .map(|h| format!("{h:X}"))
                        .unwrap_or_else(|| "enclosure".into()),
                    area.point[0],
                    area.point[1]
                )],
                basis: "geometric".into(),
                ..Default::default()
            });
        }
        if submission.items.is_empty() {
            submission.warnings.push(format!(
                "Page {page}: no labelled vector enclosures; raster interpretation requires review"
            ));
        }
    }
    submission.elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(submission)
}

pub fn extend_pdf(root: &Path, drawing: &Path, mut prepared: Submission) -> Submission {
    match pdf(root, drawing) {
        Ok(general) => {
            for item in general.items {
                if !prepared.items.iter().any(|p| {
                    p.kind == item.kind
                        && p.description
                            .trim()
                            .eq_ignore_ascii_case(item.description.trim())
                }) {
                    prepared.items.push(item);
                }
            }
            prepared.warnings.extend(general.warnings);
            prepared.elapsed_ms += general.elapsed_ms;
        }
        Err(error) => prepared.warnings.push(format!("General PDF path: {error}")),
    }
    prepared
}

/// Supplement prepared methods with shared evidence mappings, never raw baseline candidates.
pub fn extend(root: &Path, drawing: &Path, mut prepared: Submission) -> Submission {
    if root.join("general/recognition.json").exists() {
        match measure(root, drawing) {
            Ok(general) => {
                for item in general.items {
                    if !prepared.items.iter().any(|p| {
                        p.kind == item.kind
                            && p.unit == item.unit
                            && p.basis == item.basis
                            && p.source_objects == item.source_objects
                    }) {
                        prepared.items.push(item);
                    }
                }
                prepared.warnings.extend(general.warnings);
                prepared.elapsed_ms += general.elapsed_ms;
            }
            Err(error) => prepared.warnings.push(error),
        }
    }
    prepared
}
