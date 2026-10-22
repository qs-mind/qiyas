//! Calibrate from a visible, labelled scale bar. Recognition language and physical units are data.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextCalibration {
    /// Regex with a named `distance` capture. No drawing-specific text is supplied by Qiyas.
    pub label_regex: String,
    pub metres_per_label_unit: f64,
    /// Search radius expressed in this label's text height, not drawing units.
    pub search_heights: f64,
}

struct TextRun {
    text: String,
    at: [f64; 2],
    height: f64,
    handles: Vec<u64>,
}

/// PDF plotters often emit one text object per glyph. Join nearby fragments on the same
/// baseline before reading labels. Font-relative gaps keep separate table columns apart.
fn text_runs<'a>(entities: impl Iterator<Item = &'a EntityType>) -> Vec<TextRun> {
    let mut rows: Vec<Vec<&opencadcodec::Text>> = Vec::new();
    for entity in entities {
        let EntityType::Text(text) = entity else {
            continue;
        };
        let matching = rows.iter_mut().find(|row| {
            let first = row[0];
            let dx = text.insertion_point.x - first.insertion_point.x;
            let dy = text.insertion_point.y - first.insertion_point.y;
            (text.rotation - first.rotation).abs() < 0.01
                && (text.height - first.height).abs() < first.height * 0.15
                && (dy * first.rotation.cos() - dx * first.rotation.sin()).abs()
                    < first.height * 0.15
        });
        if let Some(row) = matching {
            row.push(text);
        } else {
            rows.push(vec![text]);
        }
    }
    let mut fonts = qiyas_render::TextEngine::bundled_only();
    let chain = fonts.chain("");
    let mut result = Vec::<TextRun>::new();
    for mut row in rows {
        let angle = row[0].rotation;
        let along = |text: &opencadcodec::Text| {
            text.insertion_point.x * angle.cos() + text.insertion_point.y * angle.sin()
        };
        row.sort_by(|a, b| along(a).total_cmp(&along(b)));
        let mut end = None;
        for text in row {
            let start = along(text);
            let gap = end.map(|end| start - end);
            let join = gap.is_some_and(|gap| gap >= -text.height * 0.3 && gap <= text.height * 0.7);
            if join {
                let run = result.last_mut().expect("A previous fragment exists");
                if gap.is_some_and(|gap| gap > text.height * 0.18)
                    && !run.text.ends_with(char::is_whitespace)
                    && !text.value.starts_with(char::is_whitespace)
                {
                    run.text.push(' ');
                }
                run.text.push_str(&text.value);
                run.handles.push(text.common.handle.value());
            } else {
                result.push(TextRun {
                    text: text.value.clone(),
                    at: [text.insertion_point.x, text.insertion_point.y],
                    height: text.height,
                    handles: vec![text.common.handle.value()],
                });
            }
            let width = f64::from(fonts.shape_line(&chain, &text.value, false).width)
                * text.height
                * text.width_factor;
            end = Some(start + width);
        }
    }
    result
}

pub fn calibrate_pdf_text(
    path: &std::path::Path,
    page: u32,
    rule: &TextCalibration,
) -> Result<Calibration, CoreError> {
    let entities = qiyas_pdf::extract_vectors(path, page, 1.0)?;
    calibrate_entities(&entities, rule)
}
fn calibrate_entities(
    entities: &[EntityType],
    rule: &TextCalibration,
) -> Result<Calibration, CoreError> {
    let regex = Regex::new(&rule.label_regex).map_err(|e| invalid(e.to_string()))?;
    if !regex.capture_names().flatten().any(|n| n == "distance")
        || !rule.metres_per_label_unit.is_finite()
        || rule.metres_per_label_unit <= 0.0
        || !rule.search_heights.is_finite()
        || rule.search_heights <= 0.0
    {
        return Err(invalid(
            "Text calibration needs a distance capture, positive unit conversion and search radius",
        ));
    }
    let lines = entities
        .iter()
        .filter_map(|e| match e {
            EntityType::Line(l) => Some(([l.start.x, l.start.y], [l.end.x, l.end.y])),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for label in text_runs(entities.iter()) {
        let Some(capture) = regex.captures(&label.text) else {
            continue;
        };
        let distance = capture
            .name("distance")
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.0)
            .ok_or_else(|| invalid("The scale bar's distance is not a positive number"))?;
        let radius = label.height * rule.search_heights;
        let at = label.at;
        // Ticks must cross both ends of the bar. A nearby unmarked drawing edge is not calibration.
        for &(a, b) in &lines {
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            if length <= 0.0
                || (a[0] - at[0])
                    .hypot(a[1] - at[1])
                    .min((b[0] - at[0]).hypot(b[1] - at[1]))
                    > radius
            {
                continue;
            }
            let tangent = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
            let tolerance = label.height * 0.02;
            let tick = |p: [f64; 2]| {
                lines.iter().any(|&(c, d)| {
                    let along =
                        |v: [f64; 2]| (v[0] - p[0]) * tangent[0] + (v[1] - p[1]) * tangent[1];
                    let across =
                        |v: [f64; 2]| (v[0] - p[0]) * tangent[1] - (v[1] - p[1]) * tangent[0];
                    along(c).abs() <= tolerance
                        && along(d).abs() <= tolerance
                        && across(c) * across(d) < 0.0
                })
            };
            if tick(a) && tick(b) {
                candidates.push(Calibration {
                    a,
                    b,
                    distance_m: distance * rule.metres_per_label_unit,
                });
            }
        }
    }
    if candidates.len() != 1 {
        return Err(invalid(format!("Expected one unambiguous labelled scale bar; found {}. Supply explicit calibration for this page", candidates.len())));
    }
    Ok(candidates.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencadcodec::{Line, Text, Vector3};
    #[test]
    fn scale_comes_from_text_and_ticks_and_ambiguity_is_refused() {
        let mut label = Text::new();
        label.value = "Reference 2.5 units".into();
        label.height = 2.0;
        label.insertion_point = Vector3::new(10.0, 15.0, 0.0);
        let mut entities = vec![EntityType::Text(label)];
        for (a, b) in [
            ([10.0, 20.0], [110.0, 20.0]),
            ([10.0, 18.0], [10.0, 22.0]),
            ([110.0, 18.0], [110.0, 22.0]),
        ] {
            let mut line = Line::new();
            line.start = Vector3::new(a[0], a[1], 0.0);
            line.end = Vector3::new(b[0], b[1], 0.0);
            entities.push(EntityType::Line(line));
        }
        let rule = TextCalibration {
            label_regex: "Reference (?P<distance>[0-9.]+) units".into(),
            metres_per_label_unit: 0.01,
            search_heights: 4.0,
        };
        let result = calibrate_entities(&entities, &rule).unwrap();
        assert!((result.drawing_scale(0.001).unwrap() - 0.25).abs() < 1e-12);
        entities.push(entities[1].clone());
        assert!(calibrate_entities(&entities, &rule).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PdfTextRules {
    pub calibration: TextCalibration,
    /// Named `description` capture identifies enclosed region labels in the drawing's language.
    pub area_label_regex: String,
    /// Optional legend row syntax with named `symbol` and `description` captures.
    #[serde(default)]
    pub legend_regex: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct PdfTextArea {
    pub description: String,
    pub quantity_m2: f64,
    pub label_handles: Vec<u64>,
    pub boundary_handle: Option<u64>,
    pub point: [f64; 2],
}
#[derive(Debug, Clone, Serialize)]
pub struct PdfTextReading {
    pub areas: Vec<PdfTextArea>,
    pub legends: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}
impl Session {
    /// Reads imported vector text and legends, and measures each labelled enclosure from geometry.
    pub fn read_pdf_text_areas(&self, rules: &PdfTextRules) -> Result<PdfTextReading, CoreError> {
        let labels = Regex::new(&rules.area_label_regex).map_err(|e| invalid(e.to_string()))?;
        if !labels.capture_names().flatten().any(|n| n == "description") {
            return Err(invalid("Area label regex needs a description capture"));
        }
        let legend = rules
            .legend_regex
            .as_deref()
            .map(Regex::new)
            .transpose()
            .map_err(|e| invalid(e.to_string()))?;
        let mut result = PdfTextReading {
            areas: Vec::new(),
            legends: BTreeMap::new(),
            warnings: Vec::new(),
        };
        let mut used = std::collections::BTreeSet::new();
        for label in text_runs(self.drawing.entities_in(&self.sheet)) {
            if let Some(c) = legend.as_ref().and_then(|r| r.captures(&label.text)) {
                if let (Some(symbol), Some(description)) = (c.name("symbol"), c.name("description"))
                {
                    result
                        .legends
                        .insert(symbol.as_str().into(), description.as_str().into());
                }
            }
            let Some(c) = labels.captures(&label.text) else {
                continue;
            };
            let Some(name) = c.name("description") else {
                continue;
            };
            let description = name.as_str().trim().to_owned();
            let rooms = self.rooms(&crate::rooms::RoomsQuery {
                seeds: Some(vec![crate::rooms::RoomSeed {
                    name: Some(description.clone()),
                    point: label.at,
                }]),
                ..Default::default()
            })?;
            for skipped in rooms.skipped {
                result
                    .warnings
                    .push(format!("{}: {}", skipped.text, skipped.reason));
            }
            for room in rooms.rooms {
                if room.boundary_handle.is_some_and(|h| !used.insert(h)) {
                    result
                        .warnings
                        .push(format!("Multiple labels in one enclosure: {description}"));
                    continue;
                }
                result.areas.push(PdfTextArea {
                    description: description.clone(),
                    quantity_m2: room.candidate_area_m2,
                    label_handles: label.handles.clone(),
                    boundary_handle: room.boundary_handle,
                    point: label.at,
                });
            }
        }
        Ok(result)
    }
}
