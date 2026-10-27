//! Paginated report text uses the plot font subsets, bidi shaping and per-glyph ActualText.
use crate::font::{em_to_text, FontSet};
use lopdf::{dictionary, Document, Object, Stream};
use qiyas_core::deliverables::{display, Language, Report};
use qiyas_geom::Xform;
use qiyas_render::{LaidText, TextEngine, CAP_EM};
use std::path::Path;

fn laid(engine: &mut TextEngine, chain: &[u16], text: &str, x: f64, y: f64, size: f64) -> LaidText {
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
fn wrap(engine: &mut TextEngine, chain: &[u16], text: &str, width: f64, size: f64) -> Vec<String> {
    let mut lines = vec![];
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.into()
            } else {
                format!("{line} {word}")
            };
            if f64::from(engine.shape_line(chain, &candidate, false).width) * size <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // Break long identifiers without discarding references or evidence.
            for ch in word.chars() {
                let candidate = format!("{line}{ch}");
                if !line.is_empty()
                    && f64::from(engine.shape_line(chain, &candidate, false).width) * size > width
                {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(ch);
            }
        }
        lines.push(line);
    }
    lines
}
/// A cell that is a number reads in Arabic digits in an Arabic report.
fn arabic_number(text: &str, language: Language) -> String {
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
    let mut y = height - margin - pitch;
    // Warnings stay in the attached lossless record, outside the client pages.
    for row in rows {
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
                y = height - margin - pitch;
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
            fonts.prepare(text, &|id| engine.font_bytes(id));
            let mut fallback = text.clone();
            fallback.glyphs = engine
                .shape_line(&qiyas_render::DEFAULT_CHAIN, &text.text, false)
                .glyphs;
            fonts.prepare(&fallback, &|id| engine.font_bytes(id));
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
                        &|id| engine.font_bytes(id),
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
        serde_json::to_vec(report).map_err(|e| e.to_string())?,
    ));
    let file=doc.add_object(dictionary!{"Type"=>"Filespec","F"=>Object::string_literal("qiyas-report.json"),"EF"=>dictionary!{"F"=>record}});
    let root=doc.add_object(dictionary!{"Type"=>"Catalog","Pages"=>parent,"QiyasReport"=>record,"Names"=>dictionary!{"EmbeddedFiles"=>dictionary!{"Names"=>vec![Object::string_literal("qiyas-report.json"),file.into()]}}});
    doc.trailer.set("Root", root);
    doc.save(path).map_err(|e| e.to_string())?;
    Ok(())
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
