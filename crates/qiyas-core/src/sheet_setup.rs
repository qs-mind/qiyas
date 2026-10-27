//! One step from model space to a printable sheet: `Command::MakeSheet` makes a sized layout, a
//! viewport at a standard scale that fits the paper with margins, a captioned title block with
//! its fields filled (status "For review"), a scale bar and the general notes at a readable
//! height. Everything is paper millimetres and generic: no drawing, trade or unit is assumed.

use std::collections::BTreeMap;

use opencadcodec::{EntityType, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layouts::{caption, LayoutOrientation};
use crate::rooms::LabelLanguage;
use crate::viewports::ViewportScale;
use crate::{CoreError, DrawSpec, Session, Units};

/// The standard drawing scales, as denominators of 1:n.
pub const STANDARD_SCALES: [f64; 18] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 500.0, 1000.0,
    2000.0, 5000.0, 10000.0,
];
/// A plan is drawn at 1:100 or larger when the paper allows it.
const PREFERRED_DENOMINATOR: f64 = 100.0;
const MARGIN_MM: f64 = 10.0;
const BINDING_MM: f64 = 20.0;
const GAP_MM: f64 = 5.0;
/// Paper height of the general notes and the scale bar's labels (2.5 mm is the least readable).
const NOTE_MM: f64 = 3.0;
const LABEL_MM: f64 = 2.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetSpec {
    /// The layout's name, such as "A-101 Ground floor".
    pub name: String,
    /// A paper_catalog name. Omitted: the smallest ISO (ANSI in inches and feet) sheet from A3 up
    /// on which the model fits at 1:100 or larger.
    #[serde(default)]
    pub paper: Option<String>,
    /// Omitted: the one that lets the model take the larger standard scale.
    #[serde(default)]
    pub orientation: Option<LayoutOrientation>,
    /// A standard ratio such as "1:50". Omitted: the largest standard scale that fits.
    #[serde(default)]
    pub scale: Option<String>,
    /// The model window to show, in drawing units. Omitted: everything drawn in Model.
    #[serde(default)]
    pub model_bounds: Option<[[f64; 2]; 2]>,
    /// The language of the captions, notes and Arabic digits; the user's language by default.
    #[serde(default)]
    pub language: Option<LabelLanguage>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub drawing_no: Option<String>,
    /// yyyy-MM-dd; today by default.
    #[serde(default)]
    pub date: Option<String>,
    /// "Qiyas" by default (Qiyas draws; the engineer approves).
    #[serde(default)]
    pub drawn_by: Option<String>,
    /// "P01" by default.
    #[serde(default)]
    pub revision: Option<String>,
    /// "For review" by default: no output claims approval.
    #[serde(default)]
    pub status: Option<String>,
    /// General notes and written assumptions, one per entry, in the sheet's language.
    #[serde(default)]
    pub notes: Vec<String>,
    /// A scale bar with the scale written above it (true by default).
    #[serde(default = "yes")]
    pub scale_bar: bool,
}

fn yes() -> bool {
    true
}

/// What `make_sheet` made.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetReport {
    pub layout: String,
    pub paper: String,
    pub orientation: LayoutOrientation,
    /// The drawing scale, such as "1:100".
    pub scale: String,
    pub viewport: u64,
    pub handles: Vec<u64>,
    /// Title block fields left empty because nothing supplied them (never invented).
    pub empty_fields: Vec<String>,
    /// Model text must be at least this tall to read at this scale (2.5 mm on paper), in drawing
    /// units: set the annotation scale to match before writing model text.
    pub min_text_height: f64,
    pub notes_fit: bool,
    pub language: LabelLanguage,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn ratio(text: &str) -> Result<f64, CoreError> {
    let (paper, model) = text
        .split_once(':')
        .ok_or_else(|| invalid("Write the scale as 1:100"))?;
    let (paper, model) = (paper.trim().parse::<f64>(), model.trim().parse::<f64>());
    match (paper, model) {
        (Ok(p), Ok(m)) if p > 0.0 && m > 0.0 && (p / m).is_finite() => Ok(m / p),
        _ => Err(invalid("A scale is two positive numbers, such as 1:100")),
    }
}

fn trimmed(n: f64) -> String {
    let text = format!("{n:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The paper rectangle the drawing may use: inside the frame, above the title strip.
struct Zone {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

/// A title block row stack: 8 mm rows and a 14 mm title row.
fn strip_height() -> f64 {
    8.0 * 8.0 + 14.0
}

fn zone_of(w: f64, h: f64) -> Zone {
    Zone {
        left: BINDING_MM + GAP_MM,
        right: w - MARGIN_MM - GAP_MM,
        bottom: MARGIN_MM + strip_height() + GAP_MM,
        top: h - MARGIN_MM - GAP_MM,
    }
}

/// The smallest standard scale denominator at which a model `w_m` x `h_m` metres fits `zone`
/// with an inner margin on every side; `None` when even 1:10000 does not.
fn best_denominator(w_m: f64, h_m: f64, zone: &Zone) -> Option<f64> {
    let (avail_w, avail_h) = (
        zone.right - zone.left - 2.0 * GAP_MM,
        zone.top - zone.bottom - 2.0 * GAP_MM,
    );
    if avail_w <= 0.0 || avail_h <= 0.0 {
        return None;
    }
    STANDARD_SCALES
        .into_iter()
        .find(|den| w_m * 1000.0 / den <= avail_w && h_m * 1000.0 / den <= avail_h)
}

impl Session {
    fn model_window(&self) -> Option<[[f64; 2]; 2]> {
        let (bounds, empty) = self.with_model(None, |m| {
            (
                m.bounds,
                m.ranges.is_empty() && m.fill_ranges.is_empty() && m.text_ranges.is_empty(),
            )
        });
        let [[x0, y0], [x1, y1]] = bounds;
        (!empty && x1 >= x0 && y1 >= y0 && [x0, y0, x1, y1].iter().all(|v| v.is_finite()))
            .then_some(bounds)
    }

    pub(crate) fn make_sheet(&mut self, spec: SheetSpec) -> Result<SheetReport, CoreError> {
        let language = spec
            .language
            .or(self.sheet_language())
            .unwrap_or(LabelLanguage::En);
        let [[x0, y0], [x1, y1]] = spec
            .model_bounds
            .or_else(|| self.model_window())
            .ok_or_else(|| {
                invalid("Model has nothing to put on a sheet: draw first, or give model_bounds")
            })?;
        let to_m = self.drawing.units.to_metres();
        let (w_m, h_m) = (
            ((x1 - x0).abs() * to_m).max(1e-6),
            ((y1 - y0).abs() * to_m).max(1e-6),
        );
        // Paper, orientation and scale.
        let imperial = matches!(self.drawing.units, Units::Inches | Units::Feet);
        let catalog = crate::layouts::paper_catalog();
        let candidates: Vec<String> = match &spec.paper {
            Some(name) => vec![name.clone()],
            None if imperial => ["ANSI B", "ANSI C", "ANSI D", "ANSI E"]
                .map(String::from)
                .to_vec(),
            None => ["ISO A3", "ISO A2", "ISO A1", "ISO A0"]
                .map(String::from)
                .to_vec(),
        };
        let forced = spec.scale.as_deref().map(ratio).transpose()?;
        let mut chosen = None;
        for (i, name) in candidates.iter().enumerate() {
            let paper = catalog
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| invalid(format!("No paper size {name} in the paper catalog")))?;
            let orientations = match spec.orientation {
                Some(o) => vec![o],
                None => vec![LayoutOrientation::Landscape, LayoutOrientation::Portrait],
            };
            let mut best: Option<(f64, LayoutOrientation)> = None;
            for o in orientations {
                let (w, h) = match o {
                    LayoutOrientation::Portrait => (paper.w_mm, paper.h_mm),
                    LayoutOrientation::Landscape => (paper.h_mm, paper.w_mm),
                };
                let zone = zone_of(w, h);
                let den = match forced {
                    Some(den) => {
                        let (aw, ah) = (
                            zone.right - zone.left - 2.0 * GAP_MM,
                            zone.top - zone.bottom - 2.0 * GAP_MM,
                        );
                        (w_m * 1000.0 / den <= aw && h_m * 1000.0 / den <= ah).then_some(den)
                    }
                    None => best_denominator(w_m, h_m, &zone),
                };
                if let Some(den) = den {
                    if best.is_none_or(|(b, _)| den < b) {
                        best = Some((den, o));
                    }
                }
            }
            if let Some((den, o)) = best {
                let good_enough = forced.is_some() || den <= PREFERRED_DENOMINATOR;
                chosen = Some((paper.clone(), o, den));
                if good_enough || i + 1 == candidates.len() {
                    break;
                }
            }
        }
        let (paper, orientation, den) = chosen.ok_or_else(|| {
            invalid(match forced {
                Some(_) => "The model does not fit the paper at that scale: give a smaller scale or a larger paper".to_string(),
                None => "The model does not fit any sheet at a standard scale: give model_bounds for the part to show".to_string(),
            })
        })?;
        let scale_text = format!("1:{}", trimmed(den));

        // Make the layout and draw on it without moving the user's view. A sheet that is
        // already there is never drawn on twice (a second title block over the first).
        if self
            .drawing
            .sheets()
            .iter()
            .any(|s| s.name.eq_ignore_ascii_case(spec.name.trim()))
        {
            return Err(invalid(format!(
                "The drawing already has a sheet named {}: give the new sheet another name",
                spec.name.trim()
            )));
        }
        self.new_layout(&spec.name, &paper.name, orientation)?;
        let block = self
            .drawing
            .sheets()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(spec.name.trim()))
            .map(|s| s.block)
            .ok_or_else(|| invalid("The new layout could not be found"))?;
        self.set_made_sheet(&block);
        let shown = std::mem::replace(&mut self.sheet, block);
        let made = self.sheet_contents(
            &spec,
            language,
            &paper,
            orientation,
            den,
            &scale_text,
            [[x0, y0], [x1, y1]],
        );
        self.sheet = shown;
        let (viewport, handles, empty_fields, notes_fit) = made?;
        let min_text_height = LABEL_MM * den / 1000.0 / to_m;
        Ok(SheetReport {
            layout: spec.name.trim().to_string(),
            paper: paper.name.clone(),
            orientation,
            scale: scale_text,
            viewport,
            handles,
            empty_fields,
            min_text_height,
            notes_fit,
            language,
        })
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn sheet_contents(
        &mut self,
        spec: &SheetSpec,
        language: LabelLanguage,
        paper: &crate::layouts::Paper,
        orientation: LayoutOrientation,
        den: f64,
        scale_text: &str,
        bounds: [[f64; 2]; 2],
    ) -> Result<(u64, Vec<u64>, Vec<String>, bool), CoreError> {
        let arabic = language == LabelLanguage::Ar;
        let digits = |text: &str| {
            if arabic {
                crate::numbers::arabic_numerals(text)
            } else {
                text.to_string()
            }
        };
        let (w, h) = match orientation {
            LayoutOrientation::Portrait => (paper.w_mm, paper.h_mm),
            LayoutOrientation::Landscape => (paper.h_mm, paper.w_mm),
        };
        let zone = zone_of(w, h);
        let mut handles = Vec::new();

        // The viewport: the window itself (its extents already hold its text), centred in the
        // zone, at the scale; the zone keeps an inner margin round it.
        let [[x0, y0], [x1, y1]] = bounds;
        let model = [[x0, y0], [x1, y1]];
        let centre = [
            (zone.left + zone.right) / 2.0,
            (zone.bottom + zone.top) / 2.0,
        ];
        let frame = self.viewport_frame(
            [0.0, 0.0],
            [0.0, 0.0],
            Some(&ViewportScale::Ratio(scale_text.to_string())),
            None,
            Some(model),
            Some(centre),
        )?;
        let viewport = self.mview(frame)?;
        handles.push(viewport);

        // The title block, captioned, with every field that has a value.
        let mut fields = BTreeMap::new();
        let mut put = |key: &str, value: Option<&str>| {
            if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
                fields.insert(key.to_string(), v.to_string());
            }
        };
        put("project", spec.project.as_deref());
        put("title", spec.title.as_deref());
        put("drawing_no", spec.drawing_no.as_deref());
        put("scale", Some(&digits(scale_text)));
        let today = self.local_date();
        put(
            "date",
            Some(&digits(spec.date.as_deref().unwrap_or(&today))),
        );
        put(
            "drawn_by",
            Some(
                spec.drawn_by
                    .as_deref()
                    .unwrap_or(if arabic { "قياس" } else { "Qiyas" }),
            ),
        );
        put("revision", Some(spec.revision.as_deref().unwrap_or("P01")));
        put(
            "status",
            Some(spec.status.as_deref().unwrap_or(if arabic {
                "للمراجعة"
            } else {
                "For review"
            })),
        );
        put("sheet", Some(spec.name.trim()));
        let empty_fields: Vec<String> = crate::layouts::SHEET_FIELDS
            .iter()
            .filter(|k| !fields.contains_key(**k))
            .map(|k| (*k).to_string())
            .collect();
        handles.extend(self.title_block_with(
            &fields,
            MARGIN_MM,
            BINDING_MM,
            &crate::layouts::default_title_layer(),
            Some(language),
        )?);

        // The strip beside the title block: general notes above, the scale bar below.
        let strip_left = BINDING_MM + GAP_MM;
        let block_left = w - MARGIN_MM - 180.0_f64.min(w - MARGIN_MM - BINDING_MM);
        let strip_right = block_left - GAP_MM;
        let (strip_bottom, strip_top) = (MARGIN_MM, MARGIN_MM + strip_height());
        let mut entities: Vec<EntityType> = Vec::new();
        let width = (strip_right - strip_left).max(20.0);
        let right_aligned = arabic;
        let anchor_x = if arabic { strip_right } else { strip_left };
        let style_note = |this: &mut Session, y: f64, text: &str, size: f64, wrap: f64| {
            let mut e =
                this.mtext_entity(Vector3::new(anchor_x, y, 0.0), wrap, size, text, None)?;
            if let EntityType::MText(t) = &mut e {
                if right_aligned {
                    t.attachment_point = opencadcodec::entities::AttachmentPoint::TopRight;
                }
            }
            Ok::<_, CoreError>(e)
        };
        let mut notes_fit = true;
        let bar_reserve = if spec.scale_bar { 22.0 } else { 0.0 };
        if !spec.notes.is_empty() && width < 40.0 {
            // No room beside the title block on this paper: say so rather than overlap it.
            notes_fit = false;
        }
        if !spec.notes.is_empty() && width >= 40.0 {
            let heading = if arabic {
                "ملاحظات عامة"
            } else {
                "General notes"
            };
            entities.push(style_note(self, strip_top - 2.0, heading, 3.5, width)?);
            let numbered: Vec<String> = spec
                .notes
                .iter()
                .enumerate()
                .map(|(i, n)| format!("{}. {}", digits(&(i + 1).to_string()), n.trim()))
                .collect();
            // Characters per line at this width (a text of height h is about 0.6 h per character).
            let per_line = (width / (NOTE_MM * 0.6)).floor().max(10.0);
            let lines: f64 = numbered
                .iter()
                .map(|n| (n.chars().count() as f64 / per_line).ceil().max(1.0))
                .sum();
            let needed = lines * NOTE_MM * 1.5 + 8.0;
            notes_fit = needed <= strip_top - strip_bottom - bar_reserve - 4.0;
            entities.push(style_note(
                self,
                strip_top - 9.0,
                &numbered.join("\n"),
                NOTE_MM,
                width,
            )?);
        }
        if spec.scale_bar && width >= 40.0 {
            entities.extend(self.scale_bar(
                den,
                language,
                strip_left,
                strip_bottom + 9.0,
                scale_text,
                (width - 10.0).min(90.0),
            )?);
        }
        if !entities.is_empty() {
            let solids: Vec<EntityType> = entities;
            handles.extend(self.add_on_annotation_layer("A-ANNO-NOTE", solids, false)?);
        }
        Ok((viewport, handles, empty_fields, notes_fit))
    }

    /// A scale bar with 4 alternate segments of the longest round model length that fits `max_len_mm`
    /// on paper (90 at most), labelled 0, half and full length in metres, the scale written above it.
    fn scale_bar(
        &mut self,
        den: f64,
        language: LabelLanguage,
        x: f64,
        y: f64,
        scale_text: &str,
        max_len_mm: f64,
    ) -> Result<Vec<EntityType>, CoreError> {
        let arabic = language == LabelLanguage::Ar;
        let digits = |text: &str| {
            if arabic {
                crate::numbers::arabic_digits(text)
            } else {
                text.to_string()
            }
        };
        let length_m = [
            1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
        ]
        .into_iter()
        .rev()
        .find(|m| m * 1000.0 / den <= max_len_mm)
        .unwrap_or(1.0);
        let len = length_m * 1000.0 / den;
        let segment = len / 4.0;
        let bar_h = 2.0;
        let mut out = Vec::new();
        out.extend(self.draw_entities(DrawSpec::Rectangle {
            a: [x, y],
            b: [x + len, y + bar_h],
        })?);
        for i in [0.0, 2.0] {
            out.extend(
                self.draw_entities(DrawSpec::Solid2D {
                    // A 2D solid takes its corners in the order 1, 2, 4, 3.
                    points: vec![
                        [x + i * segment, y],
                        [x + (i + 1.0) * segment, y],
                        [x + i * segment, y + bar_h],
                        [x + (i + 1.0) * segment, y + bar_h],
                    ]
                    .into(),
                })?,
            );
        }
        let unit = if arabic { "م" } else { "m" };
        for (at, label) in [
            (0.0, "0".to_string()),
            (len / 2.0, trimmed(length_m / 2.0)),
            (len, format!("{} {unit}", trimmed(length_m))),
        ] {
            let label = self.mtext_entity(
                Vector3::new(x + at - 1.0, y - 1.5, 0.0),
                0.0,
                LABEL_MM,
                &digits(&label),
                None,
            )?;
            out.push(label);
        }
        let heading = format!("{} {}", caption("scale", language), digits(scale_text));
        out.push(self.mtext_entity(
            Vector3::new(x, y + bar_h + 5.0, 0.0),
            0.0,
            LABEL_MM,
            &heading,
            None,
        )?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_the_largest_standard_one_that_fits() {
        // A3 landscape: a 12 x 15 m plan needs 1:75 or smaller in the 190 mm left above the strip.
        let zone = zone_of(420.0, 297.0);
        assert_eq!(best_denominator(12.0, 15.0, &zone), Some(100.0));
        let zone = zone_of(297.0, 420.0);
        assert_eq!(best_denominator(12.0, 15.0, &zone), Some(50.0));
        assert_eq!(best_denominator(0.5, 0.5, &zone), Some(5.0));
        assert_eq!(best_denominator(1.0e6, 1.0e6, &zone), None);
    }

    #[test]
    fn the_default_date_is_a_whole_local_date() {
        let date = crate::field::local_today();
        assert_eq!(date.len(), 10, "{date}");
        assert_eq!(date.matches('-').count(), 2, "{date}");
        assert!(date.chars().all(|c| c.is_ascii_digit() || c == '-'), "{date}");
    }

    #[test]
    fn ratios_read_and_numbers_trim() {
        assert_eq!(ratio("1:50").unwrap(), 50.0);
        assert!(ratio("0:5").is_err());
        assert!(ratio("fit").is_err());
        assert_eq!(trimmed(2.5), "2.5");
        assert_eq!(trimmed(5.0), "5");
    }
}
