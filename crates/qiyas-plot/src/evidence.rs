//! W6: one review PDF of the source sheets behind the generated BOQ.
//! The drawing stays untouched; marks and the legend are added to plot snapshots.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use lopdf::{dictionary, Document, Object};
use qiyas_core::deliverables::{self, Kind, Language};
use qiyas_core::Session;
use qiyas_geom::Xform;
use qiyas_render::{Batch, Range, TextEngine, DEFAULT_CHAIN};

use super::{absolute, pdf_document, save_document, LayerPlan, PlotSnapshot};
use crate::look::Look;
use crate::{Area, Orientation, Paper, PlotError, PlotJob, PlotOutput, Scale, PT_PER_MM};

const WIDTH: f64 = 420.0;
const HEIGHT: f64 = 297.0;
const DRAW_WIDTH: f64 = 285.0;
const FONT: f64 = 2.5 * PT_PER_MM;
const PITCH: f64 = 4.5 * PT_PER_MM;
const COLORS: [u32; 6] = [0xB32936, 0x006E90, 0x7446A0, 0x187344, 0xA85B00, 0xAF287D];

/// Plain plot data, captured while the caller holds the session.
#[derive(Debug, Clone)]
pub struct EvidenceSnapshot {
    pages: Vec<PlotSnapshot>,
}

/// Uses the same default language as the BOQ deliverable. Pass the BOQ's explicit language
/// when it was exported with an override.
pub fn language(session: &Session) -> Language {
    match session.sheet_language() {
        Some(qiyas_core::rooms::LabelLanguage::Ar) => Language::Ar,
        _ => Language::En,
    }
}

/// The same export folder and path restrictions as a drawing plot.
pub fn output_path(session: &Session, name: Option<&str>) -> Result<PathBuf, PlotError> {
    let stem = session
        .drawing
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "drawing".into());
    let mut job = job("*Model_Space");
    job.output = Some(
        name.map(str::to_string)
            .unwrap_or_else(|| format!("{stem} evidence.pdf")),
    );
    crate::output_path(session, &job)
}

pub fn export(
    session: &Session,
    name: Option<&str>,
    language: Language,
) -> Result<PlotOutput, PlotError> {
    let path = output_path(session, name)?;
    write(&snapshot(session, language)?, &path)
}

fn job(sheet: &str) -> PlotJob {
    PlotJob {
        sheet: sheet.into(),
        area: Area::Extents,
        paper: Paper {
            name: "Evidence drawing area".into(),
            w_mm: DRAW_WIDTH,
            h_mm: HEIGHT - 30.0,
        },
        orientation: Orientation::Portrait,
        scale: Scale::Fit,
        centre: true,
        style: None,
        lineweights: true,
        output: None,
    }
}

fn error(message: impl ToString) -> PlotError {
    PlotError::Output(message.to_string())
}

fn evidence_title(title: &str, language: Language) -> String {
    if language == Language::Ar {
        let title = if title == "Model" {
            "النموذج"
        } else {
            title
        };
        format!("دليل الحصر - {title}")
    } else {
        format!("Take-off evidence - {title}")
    }
}

/// Every BOQ source must be markable; refuse a partial evidence package rather than silently
/// leave an item out. Informational items and unresolved second-reading disagreements follow
/// the BOQ's own filtering in `deliverables::drawing`.
pub fn snapshot(session: &Session, language: Language) -> Result<EvidenceSnapshot, PlotError> {
    let mut report = deliverables::drawing(session, Kind::Boq, language, None).map_err(error)?;
    report.layout.columns.retain(|c| {
        matches!(
            c.field.as_str(),
            "code" | "description" | "quantity" | "unit"
        )
    });
    let columns = &report.layout.columns;
    let field = |name: &str| {
        columns
            .iter()
            .position(|c| c.field == name)
            .expect("default BOQ column")
    };
    let mut legends = BTreeMap::new();
    for row in report
        .table()
        .map_err(error)?
        .into_iter()
        .filter(|r| !r.heading)
    {
        let cell = |name| deliverables::display(&row.cells[field(name)]);
        let number = crate::report::arabic_number(&cell("quantity"), language);
        legends.insert(
            cell("code"),
            format!(
                "{} - {} - {} {}",
                cell("code"),
                cell("description"),
                number,
                cell("unit")
            ),
        );
    }
    if legends.is_empty() {
        return Err(error("No generated BOQ items to mark"));
    }
    let measured = session.measurement_report().map_err(error)?;
    let mut sheets: BTreeMap<String, BTreeMap<String, BTreeSet<u64>>> = BTreeMap::new();
    for item in &measured.items {
        if !legends.contains_key(&item.code) {
            continue;
        }
        let mut found = false;
        for source in item.lines.iter().flat_map(|line| &line.sources) {
            sheets
                .entry(source.sheet.clone())
                .or_default()
                .entry(item.code.clone())
                .or_default()
                .insert(source.handle);
            found = true;
        }
        if !found {
            return Err(error(format!(
                "BOQ item {} has no drawing evidence",
                item.code
            )));
        }
    }
    let colors: BTreeMap<_, _> = legends
        .keys()
        .enumerate()
        .map(|(i, code)| (code.clone(), COLORS[i % COLORS.len()]))
        .collect();
    let mut pages = Vec::new();
    let mut engine = TextEngine::new();
    for (sheet, items) in sheets {
        let mut base = super::snapshot(session, &job(&sheet))?;
        let shift = Xform::translate(0.0, 15.0 * PT_PER_MM);
        for layer in &mut base.layers {
            layer.xf = layer.xf.then(&shift);
            layer.clip = layer.clip.map(|corners| corners.map(|p| shift.apply(p)));
        }
        base.page.xf = base.page.xf.then(&shift);
        base.page.size = [WIDTH * PT_PER_MM, HEIGHT * PT_PER_MM];
        let batch = session.sheet_batch(&sheet);
        // Split long legends into continuation pages; each page repeats its drawing and marks.
        let mut legend_pages: Vec<Vec<(u32, String)>> = vec![vec![]];
        let capacity = ((HEIGHT - 45.0) * PT_PER_MM / PITCH) as usize;
        for code in items.keys() {
            let lines = crate::report::wrap(
                &mut engine,
                &DEFAULT_CHAIN,
                &legends[code],
                (WIDTH - DRAW_WIDTH - 15.0) * PT_PER_MM,
                FONT,
            );
            for line in lines {
                if legend_pages.last().unwrap().len() == capacity {
                    legend_pages.push(vec![]);
                }
                legend_pages.last_mut().unwrap().push((colors[code], line));
            }
            if legend_pages.last().unwrap().len() < capacity {
                legend_pages.last_mut().unwrap().push((0, String::new()));
            }
        }
        let mut marks = Batch::default();
        let mut labels = BTreeMap::<u64, usize>::new();
        for (code, handles) in &items {
            for handle in handles {
                let bounds = outline(&mut marks, &batch, *handle, colors[code], &base.page.xf)
                    .ok_or_else(|| {
                        error(format!(
                            "BOQ item {code}: source {handle} on {sheet} has no visible geometry"
                        ))
                    })?;
                let lane = labels.entry(*handle).or_default();
                let width = f64::from(engine.shape_line(&DEFAULT_CHAIN, code, false).width) * FONT;
                let x = bounds[1][0]
                    .min(DRAW_WIDTH * PT_PER_MM - width - 4.0)
                    .max(4.0);
                let y =
                    (bounds[1][1] + 4.0 + *lane as f64 * PITCH).min((HEIGHT - 22.0) * PT_PER_MM);
                add_text(&mut marks, &mut engine, code, [x, y], colors[code], false);
                *lane += 1;
            }
        }
        for legend in legend_pages {
            let mut page = base.clone();
            let mut overlay = marks.clone();
            let arabic = language == Language::Ar;
            let title = evidence_title(&base.title, language);
            add_text(
                &mut overlay,
                &mut engine,
                &title,
                [
                    if arabic {
                        (WIDTH - 10.0) * PT_PER_MM
                    } else {
                        10.0 * PT_PER_MM
                    },
                    (HEIGHT - 10.0) * PT_PER_MM,
                ],
                0,
                arabic,
            );
            add_text(
                &mut overlay,
                &mut engine,
                if arabic {
                    "للمراجعة"
                } else {
                    "For review"
                },
                [
                    if arabic {
                        (WIDTH - 10.0) * PT_PER_MM
                    } else {
                        10.0 * PT_PER_MM
                    },
                    7.0 * PT_PER_MM,
                ],
                0,
                arabic,
            );
            let heading = if arabic {
                "البند - الوصف - الكمية والوحدة"
            } else {
                "Item - Description - Quantity / unit"
            };
            let x = if arabic {
                (WIDTH - 10.0) * PT_PER_MM
            } else {
                (DRAW_WIDTH + 5.0) * PT_PER_MM
            };
            add_text(
                &mut overlay,
                &mut engine,
                heading,
                [x, (HEIGHT - 23.0) * PT_PER_MM],
                0,
                arabic,
            );
            for (i, (color, line)) in legend.iter().enumerate() {
                add_text(
                    &mut overlay,
                    &mut engine,
                    line,
                    [x, (HEIGHT - 30.0) * PT_PER_MM - i as f64 * PITCH],
                    *color,
                    arabic,
                );
            }
            page.looks.insert(
                (0, vec![]),
                Look {
                    weight_mm: 0.8,
                    logical: true,
                    ..Look::default()
                },
            );
            for text in &overlay.texts {
                for glyph in &text.glyphs {
                    if let Some(bytes) = engine.font_face(glyph.font) {
                        page.fonts.insert(glyph.font, bytes);
                    }
                }
            }
            page.layers.push(LayerPlan {
                batch: page.batches.len(),
                xf: Xform::identity(),
                clip: None,
                frozen: Default::default(),
                pictures: vec![],
            });
            page.batches.push(overlay);
            pages.push(page);
        }
    }
    Ok(EvidenceSnapshot { pages })
}

fn add_text(
    batch: &mut Batch,
    engine: &mut TextEngine,
    text: &str,
    at: [f64; 2],
    color: u32,
    right: bool,
) {
    let width = f64::from(engine.shape_line(&DEFAULT_CHAIN, text, false).width) * FONT;
    let mut laid = crate::report::laid(
        engine,
        &DEFAULT_CHAIN,
        text,
        at[0] - if right { width } else { 0.0 },
        at[1],
        FONT,
    );
    laid.color = color;
    batch.texts.push(laid);
}

fn segment(batch: &mut Batch, a: [f64; 2], b: [f64; 2], color: u32) {
    let first = batch.colors.len() as u32;
    batch
        .segments
        .extend([a[0] as f32, a[1] as f32, b[0] as f32, b[1] as f32]);
    batch.colors.push(color);
    batch.ranges.push(Range {
        top: 0,
        entity: 0,
        chain: 0,
        first,
        count: 1,
        flags: 0,
    });
}

/// Trace source linework, including descendants of a measured block. Filled-only elements
/// get a heavy enclosing frame. Non-plotting retained room boundaries can still be evidence.
fn outline(
    out: &mut Batch,
    batch: &Batch,
    handle: u64,
    color: u32,
    xf: &Xform,
) -> Option<[[f64; 2]; 2]> {
    let mut points = Vec::new();
    for r in &batch.ranges {
        if r.entity != handle && r.top != handle {
            continue;
        }
        for i in r.first..r.first + r.count {
            let s = &batch.segments[i as usize * 4..i as usize * 4 + 4];
            let a = xf.apply(absolute(batch, s[0], s[1]));
            let b = xf.apply(absolute(batch, s[2], s[3]));
            points.extend([a, b]);
            segment(out, a, b, color);
        }
    }
    let has_lines = !points.is_empty();
    for r in &batch.fill_ranges {
        if r.entity != handle && r.top != handle {
            continue;
        }
        for i in r.first..r.first + r.count {
            for p in batch.fills[i as usize * 6..i as usize * 6 + 6]
                .as_chunks::<2>()
                .0
            {
                points.push(xf.apply(absolute(batch, p[0], p[1])));
            }
        }
    }
    let first = *points.first()?;
    let mut bounds = [first, first];
    for p in points {
        for axis in 0..2 {
            bounds[0][axis] = bounds[0][axis].min(p[axis]);
            bounds[1][axis] = bounds[1][axis].max(p[axis]);
        }
    }
    if !has_lines || bounds[0] == bounds[1] {
        let [lo, hi] = bounds;
        let corners = [
            [lo[0] - 2.0, lo[1] - 2.0],
            [hi[0] + 2.0, lo[1] - 2.0],
            [hi[0] + 2.0, hi[1] + 2.0],
            [lo[0] - 2.0, hi[1] + 2.0],
        ];
        for i in 0..4 {
            segment(out, corners[i], corners[(i + 1) % 4], color);
        }
    }
    Some(bounds)
}

/// Writes without a session lock. Each page is drawn by the ordinary plot writer, including
/// viewports, images and embedded searchable Arabic fonts; only the page tree is combined.
pub fn write(snapshot: &EvidenceSnapshot, path: &Path) -> Result<PlotOutput, PlotError> {
    if snapshot.pages.is_empty() {
        return Err(PlotError::Nothing);
    }
    let mut doc = Document::with_version("1.7");
    let parent = doc.new_object_id();
    let mut kids = Vec::new();
    let mut skipped = Vec::new();
    for page in &snapshot.pages {
        let (mut part, warnings) = pdf_document(page);
        skipped.extend(warnings);
        part.renumber_objects_with(doc.max_id + 1);
        for id in part.get_pages().into_values() {
            part.get_object_mut(id)
                .and_then(Object::as_dict_mut)
                .map_err(error)?
                .set("Parent", parent);
            kids.push(Object::Reference(id));
        }
        doc.max_id = part.max_id;
        doc.objects.extend(part.objects);
    }
    doc.objects.insert(
        parent,
        dictionary! { "Type" => "Pages", "Count" => kids.len() as i64, "Kids" => kids }.into(),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => parent });
    doc.trailer.set("Root", root);
    skipped.sort();
    skipped.dedup();
    save_document(&mut doc, path)?;
    Ok(PlotOutput {
        path: path.to_path_buf(),
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_titles_translate_only_the_exact_model_title_in_arabic() {
        assert_eq!(
            evidence_title("Model", Language::Ar),
            "دليل الحصر - النموذج"
        );
        assert_eq!(
            evidence_title("Model", Language::En),
            "Take-off evidence - Model"
        );
        for title in ["A-101", "Model details", "model", "المسقط الأفقي"] {
            assert_eq!(
                evidence_title(title, Language::Ar),
                format!("دليل الحصر - {title}")
            );
            assert_eq!(
                evidence_title(title, Language::En),
                format!("Take-off evidence - {title}")
            );
        }
    }
}
