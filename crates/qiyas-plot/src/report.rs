//! Paginated report text uses the plot font subsets, bidi shaping and per-glyph ActualText.
use crate::font::{em_to_text, FontSet};
use lopdf::{dictionary, Document, Object, Stream};
use qiyas_core::deliverables::{display, Language, Report, TraceTarget};
use qiyas_geom::Xform;
use qiyas_render::{LaidText, TextEngine, CAP_EM};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfTrace {
    pub drawing: Option<String>,
    pub pages: Vec<PdfTracePage>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfTracePage {
    pub page: u32,
    pub width: f64,
    pub height: f64,
    pub regions: Vec<PdfTraceRegion>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfTraceRegion {
    /// [left, bottom, width, height], in PDF points from the page's bottom-left.
    pub rect: [f64; 4],
    pub target: TraceTarget,
}

pub(crate) fn laid(
    engine: &mut TextEngine,
    chain: &[u16],
    text: &str,
    x: f64,
    y: f64,
    size: f64,
) -> LaidText {
    let shaped = engine.shape_line(chain, text, false);
    LaidText {
        top: 0,
        entity: 0,
        chain: 0,
        color: 0,
        xf: Xform::scale(size, size).then(&Xform::translate(x, y)),
        world_size: size,
        glyphs: shaped.glyphs,
        text: text.into(),
    }
}
pub(crate) fn wrap(
    engine: &mut TextEngine,
    chain: &[u16],
    text: &str,
    width: f64,
    size: f64,
) -> Vec<String> {
    let mut lines = vec![];
    for paragraph in text.split('\n') {
        let mut line = String::new();
        // Reports collapse breakable whitespace; NBSP and narrow NBSP stay literal glue.
        let mut normalized = String::new();
        for c in paragraph.chars() {
            if c.is_whitespace() && !matches!(c, '\u{00a0}' | '\u{202f}') {
                if !normalized.is_empty() && !normalized.ends_with(' ') {
                    normalized.push(' ');
                }
            } else {
                normalized.push(c);
            }
        }
        let paragraph = normalized.trim_end_matches(' ');
        let mut start = 0;
        let mut separator = "";
        for (end, _) in unicode_linebreak::linebreaks(paragraph) {
            let span = &paragraph[start..end];
            start = end;
            let word = span.trim_end_matches(' ');
            let candidate = if line.is_empty() {
                word.into()
            } else {
                format!("{line}{separator}{word}")
            };
            separator = if span.ends_with(' ') { " " } else { "" };
            if f64::from(engine.shape_line(chain, &candidate, false).width) * size <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if word.contains(['\u{00a0}', '\u{202f}', '\u{2060}', '\u{feff}']) {
                line.push_str(word);
                continue;
            }
            // Emergency breaks for long identifiers must keep combining and emoji clusters.
            for cluster in word.graphemes(true) {
                let candidate = format!("{line}{cluster}");
                if !line.is_empty()
                    && f64::from(engine.shape_line(chain, &candidate, false).width) * size > width
                {
                    lines.push(std::mem::take(&mut line));
                }
                line.push_str(cluster);
            }
        }
        lines.push(line);
    }
    lines
}
/// A cell that is a number reads in Arabic digits in an Arabic report.
pub(crate) fn arabic_number(text: &str, language: Language) -> String {
    if language != Language::Ar || text.parse::<f64>().is_err() {
        return text.to_string();
    }
    text.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('٠' as u32 + c as u32 - '0' as u32).unwrap_or(c),
            '.' => '٫',
            other => other,
        })
        .collect()
}
pub fn write(path: &Path, report: &Report) -> Result<(), String> {
    let mut rows = report.table().map_err(|e| e.to_string())?;
    rows.push(report.status_row());
    let page = &report.layout.page;
    let (width, height, margin) = (
        page.width_mm * crate::PT_PER_MM,
        page.height_mm * crate::PT_PER_MM,
        page.margin_mm * crate::PT_PER_MM,
    );
    let size = page.font_size * f64::from(CAP_EM);
    let pitch = page.font_size * 1.7;
    let available = width - 2. * margin;
    let sum: f64 = report.layout.columns.iter().map(|c| c.width).sum();
    let widths: Vec<_> = report
        .layout
        .columns
        .iter()
        .map(|c| available * c.width / sum)
        .collect();
    let mut engine = TextEngine::new();
    let chain = engine.chain(&report.layout.font);
    let mut pages: Vec<Vec<LaidText>> = vec![vec![]];
    let mut trace_pages = vec![PdfTracePage {
        page: 1,
        width,
        height,
        regions: vec![],
    }];
    let mut y = height - margin - pitch;
    // Warnings stay in the attached lossless record, outside the client pages.
    for row in rows {
        let mut fragment: Option<(usize, usize)> = None;
        // A section or trade title stands apart from the rows above it.
        if row.section {
            y -= pitch * 0.8;
        }
        let wide = row.cells.len() == 1;
        let wrapped: Vec<_> = row
            .cells
            .iter()
            .enumerate()
            .map(|(i, v)| {
                wrap(
                    &mut engine,
                    &chain,
                    &arabic_number(&display(v), report.language),
                    if wide { available - 6. } else { widths[i] - 6. },
                    size,
                )
            })
            .collect();
        for line in 0..wrapped.iter().map(Vec::len).max().unwrap_or(1) {
            if y < margin + pitch {
                pages.push(vec![]);
                trace_pages.push(PdfTracePage {
                    page: pages.len() as u32,
                    width,
                    height,
                    regions: vec![],
                });
                y = height - margin - pitch;
            }
            if let Some(target) = &row.target {
                let page_index = pages.len() - 1;
                let regions = &mut trace_pages[page_index].regions;
                let bottom = y - pitch * 0.25;
                if let Some((_, index)) = fragment.filter(|(p, _)| *p == page_index) {
                    let rect = &mut regions[index].rect;
                    rect[3] += rect[1] - bottom;
                    rect[1] = bottom;
                } else {
                    fragment = Some((page_index, regions.len()));
                    regions.push(PdfTraceRegion {
                        rect: [margin, bottom, available, pitch],
                        target: target.clone(),
                    });
                }
            }
            let mut x = if report.language == Language::Ar {
                width - margin
            } else {
                margin
            };
            for (i, cell) in wrapped.iter().enumerate() {
                let w = if wide { available } else { widths[i] };
                if report.language == Language::Ar {
                    x -= w;
                }
                if let Some(text) = cell.get(line) {
                    let advance = f64::from(engine.shape_line(&chain, text, false).width) * size;
                    let start = if report.language == Language::Ar {
                        x + w - 3. - advance
                    } else {
                        x + 3.
                    };
                    pages
                        .last_mut()
                        .unwrap()
                        .push(laid(&mut engine, &chain, text, start, y, size));
                }
                if report.language == Language::En {
                    x += w;
                }
            }
            y -= pitch;
        }
        y -= 3.;
    }
    let mut fonts = FontSet::default();
    let mut searchable_pages = Vec::new();
    for page in &pages {
        let mut searchable = Vec::new();
        for text in page {
            fonts.prepare(text, &|id| engine.font_face(id));
            let mut fallback = text.clone();
            fallback.glyphs = engine
                .shape_line(&qiyas_render::DEFAULT_CHAIN, &text.text, false)
                .glyphs;
            fonts.prepare(&fallback, &|id| engine.font_face(id));
            searchable.push(fallback);
        }
        searchable_pages.push(searchable);
    }
    fonts.finish();
    let mut doc = Document::with_version("1.7");
    let resources = fonts.write(&mut doc);
    let parent = doc.new_object_id();
    let mut kids = vec![];
    for (page, searchable) in pages.into_iter().zip(searchable_pages) {
        let mut content = String::new();
        for (visual, fallback) in page.into_iter().zip(searchable) {
            let outlined = !fonts.as_text(&visual);
            if outlined {
                // CFF and fonts that forbid embedding follow plotting's outline path. A
                // hidden bundled-font text layer retains searchable logical characters.
                let mut paths = crate::content::Content::default();
                paths.fill([0, 0, 0]);
                for glyph in &visual.glyphs {
                    let xf = em_to_text(fonts.units_per_em(glyph.font))
                        .then(&Xform::translate(f64::from(glyph.x), f64::from(glyph.y)))
                        .then(&visual.xf);
                    fonts.outline(
                        glyph.font,
                        glyph.glyph,
                        &xf,
                        &|id| engine.font_face(id),
                        &mut paths,
                    );
                }
                paths.op("f");
                content.push_str(&paths.buf);
            }
            let text = if outlined { fallback } else { visual };
            if !fonts.as_text(&text) {
                return Err("A report character has no searchable font glyph".into());
            }
            let actual = fonts.actual_text(&text);
            content.push_str(if outlined {
                "BT 3 Tr 0 g\n"
            } else {
                "BT 0 Tr 0 g\n"
            });
            for (i, g) in text.glyphs.iter().enumerate() {
                let x = text.xf.apply([f64::from(g.x), f64::from(g.y)]);
                let em = size / f64::from(CAP_EM);
                content.push_str(&format!(
                    "/F{} 1 Tf {em} 0 0 {em} {} {} Tm\n",
                    g.font, x[0], x[1]
                ));
                if let Some(chars) = actual.get(i).and_then(Option::as_ref) {
                    let hex = std::iter::once(0xfeff)
                        .chain(chars.encode_utf16())
                        .map(|n| format!("{n:04X}"))
                        .collect::<String>();
                    content.push_str(&format!("/Span <</ActualText <{hex}>>> BDC "));
                }
                content.push_str(&format!("<{:04X}> Tj\n", fonts.subset_gid(g.font, g.glyph)));
                if actual.get(i).is_some_and(Option::is_some) {
                    content.push_str("EMC\n");
                }
            }
            content.push_str("ET\n");
        }
        let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
        kids.push(Object::Reference(doc.add_object(dictionary!{"Type"=>"Page","Parent"=>parent,"MediaBox"=>vec![0.into(),0.into(),Object::Real(width as f32),Object::Real(height as f32)],"Resources"=>dictionary!{"Font"=>resources.clone()},"Contents"=>stream})));
    }
    doc.objects.insert(
        parent,
        dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}.into(),
    );
    let record = doc.add_object(Stream::new(
        dictionary! {"Type"=>"EmbeddedFile","Subtype"=>"application/json"},
        vec![],
    ));
    let file=doc.add_object(dictionary!{"Type"=>"Filespec","F"=>Object::string_literal("qiyas-report.json"),"EF"=>dictionary!{"F"=>record}});
    let root=doc.add_object(dictionary!{"Type"=>"Catalog","Pages"=>parent,"QiyasReport"=>record,"Names"=>dictionary!{"EmbeddedFiles"=>dictionary!{"Names"=>vec![Object::string_literal("qiyas-report.json"),file.into()]}}});
    doc.trailer.set("Root", root);
    let mut data = serde_json::to_value(report).map_err(|e| e.to_string())?;
    data["trace"] = json!({
        "version": 1,
        "map": PdfTrace { drawing: report.drawing.clone(), pages: trace_pages },
        "root": root,
        "objects": snapshot(&doc, record),
    });
    doc.objects.insert(
        record,
        Object::Stream(Stream::new(
            dictionary! {"Type"=>"EmbeddedFile","Subtype"=>"application/json"},
            serde_json::to_vec(&data).map_err(|e| e.to_string())?,
        )),
    );
    doc.save(path).map_err(|e| e.to_string())?;
    Ok(())
}

// Exact object snapshots avoid guessing links from extracted text. Include page trees,
// boxes, transforms, resources and content, so an external edit invalidates the map.
fn object_value(object: &Object) -> Value {
    match object {
        Object::Null => Value::Null,
        Object::Boolean(value) => json!(value),
        Object::Integer(value) => json!(*value as f64),
        Object::Real(value) => json!(f64::from(*value)),
        Object::Name(value) => json!({"name": value}),
        Object::String(value, _) => json!({"string": value}),
        Object::Reference(value) => json!({"ref": value}),
        Object::Array(values) => values.iter().map(object_value).collect(),
        Object::Dictionary(values) => {
            let entries: std::collections::BTreeMap<_, _> = values
                .iter()
                .map(|(key, value)| {
                    (
                        key.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        object_value(value),
                    )
                })
                .collect();
            json!({"dictionary": entries})
        }
        Object::Stream(stream) => json!({
            "dictionary": object_value(&Object::Dictionary(stream.dict.clone())),
            "content": stream.content.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        }),
    }
}

fn snapshot(doc: &Document, record: lopdf::ObjectId) -> Value {
    let objects: Vec<_> = doc
        .objects
        .iter()
        .filter(|(id, _)| **id != record)
        // A saved file's cross-reference stream is regenerated by lopdf.
        .filter(|(_, object)| object.type_name().ok() != Some(b"XRef".as_slice()))
        .map(|(id, object)| json!({"id": id, "value": object_value(object)}))
        .collect();
    json!(objects)
}

/// Old, malformed or externally changed reports remain viewable without trace links.
pub fn read_trace(path: &Path) -> Option<PdfTrace> {
    let doc = Document::load(path).ok()?;
    let record = doc
        .catalog()
        .ok()?
        .get(b"QiyasReport")
        .ok()?
        .as_reference()
        .ok()?;
    let stream = doc.get_object(record).ok()?.as_stream().ok()?;
    let data: Value = serde_json::from_slice(&stream.content).ok()?;
    let trace = &data["trace"];
    if trace["version"] != 1
        || trace["root"] != json!(doc.trailer.get(b"Root").ok()?.as_reference().ok()?)
        || trace["objects"] != snapshot(&doc, record)
    {
        return None;
    }
    let map: PdfTrace = serde_json::from_value(trace["map"].clone()).ok()?;
    if map.pages.len() != doc.get_pages().len() {
        return None;
    }
    for (index, page) in map.pages.iter().enumerate() {
        if page.page as usize != index + 1
            || !page.width.is_finite()
            || !page.height.is_finite()
            || page.width <= 0.0
            || page.height <= 0.0
            || page.regions.iter().any(|region| {
                let [x, y, w, h] = region.rect;
                region.target.item.is_empty()
                    || region.rect.iter().any(|v| !v.is_finite())
                    || x < 0.0
                    || y < 0.0
                    || w <= 0.0
                    || h <= 0.0
                    || x + w > page.width + 0.01
                    || y + h > page.height + 0.01
            })
        {
            return None;
        }
    }
    Some(map)
}
pub fn read(path: &Path) -> Result<Report, String> {
    let doc = Document::load(path).map_err(|e| e.to_string())?;
    let stream = doc
        .catalog()
        .and_then(|d| d.get(b"QiyasReport"))
        .and_then(Object::as_reference)
        .and_then(|id| doc.get_object(id))
        .and_then(Object::as_stream)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&stream.content).map_err(|e| e.to_string())
}
