//! Vision supplies hints; calibration and snaps determine their measurable coordinates.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Calibration {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub distance_m: f64,
}
impl Calibration {
    pub fn drawing_scale(&self, metres_per_unit: f64) -> Result<f64, CoreError> {
        let length = distance(self.a, self.b);
        if !self.a.iter().chain(self.b.iter()).all(|n| n.is_finite())
            || !length.is_finite()
            || length <= 0.0
            || !self.distance_m.is_finite()
            || self.distance_m <= 0.0
        {
            return Err(invalid(
                "Calibration needs two distinct page points and a positive physical distance",
            ));
        }
        Ok(self.distance_m / length / metres_per_unit)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisionHint {
    pub object_type: String,
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    pub confidence: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisionText {
    /// Verbatim text observed by vision/OCR, in displayed page coordinates.
    pub text: String,
    pub at: [f64; 2],
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisionSpec {
    pub path: String,
    pub page: u32,
    pub calibration: Calibration,
    /// Other visible calibrated spans. Prefer the longest span available on the page.
    #[serde(default)]
    pub calibration_spans: Vec<Calibration>,
    /// All readable dimension strings with their measured endpoints, for scale cross-checks.
    #[serde(default)]
    pub dimensions: Vec<VisionDimension>,
    /// Spatial room labels and notes from vision/OCR. PDF text is also collected automatically.
    #[serde(default)]
    pub texts: Vec<VisionText>,
    #[serde(default)]
    pub insert_at: [f64; 2],
    pub snap_radius_m: f64,
    pub hints: Vec<VisionHint>,
}
impl Session {
    pub(super) fn recognise_vision(&mut self, spec: VisionSpec) -> Result<Value, CoreError> {
        if spec.hints.is_empty() || spec.hints.len() > 200 {
            return Err(invalid("Vision requires 1..200 hints"));
        }
        if !spec.snap_radius_m.is_finite()
            || spec.snap_radius_m <= 0.0
            || !spec.insert_at.iter().all(|n| n.is_finite())
        {
            return Err(invalid(
                "Vision needs a positive snap radius and finite insertion point",
            ));
        }
        let radius = spec.snap_radius_m / self.drawing.units.to_metres();
        let path = std::path::Path::new(&spec.path);
        let (width, height) = qiyas_pdf::page_size(path, spec.page)?;
        let pixels = qiyas_pdf::render_page(path, spec.page, 144.0, |_| {})?;
        let ocr = if qiyas_ocr::available() && !qiyas_pdf::has_text_layer(path, spec.page)? {
            qiyas_pdf::ocr_page(path, spec.page)?
                .into_iter()
                .filter(|text| {
                    // A supplied transcription owns its location, even when OCR disagrees.
                    let pad = (text.bbox[1][1] - text.bbox[0][1]) * 0.5;
                    !spec.texts.iter().any(|given| {
                        (0..2).all(|axis| {
                            given.at[axis] >= text.bbox[0][axis] - pad
                                && given.at[axis] <= text.bbox[1][axis] + pad
                        })
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let (ocr_dimensions, dimension_texts) = super::raster_calibration::ocr_dimensions(
            &ocr,
            &spec.calibration,
            &spec.calibration_spans,
            &spec.dimensions,
        );
        let mut calibrated = calibrate_raster(
            &pixels,
            [width, height],
            &spec.calibration,
            &spec.calibration_spans,
            &spec.dimensions,
        )?;
        super::raster_calibration::cross_check_ocr(&mut calibrated, &ocr_dimensions)?;
        let scale = calibrated
            .calibration
            .drawing_scale(self.drawing.units.to_metres())?;
        let mut texts: Vec<_> = spec
            .texts
            .iter()
            .map(|t| (t.at, t.text.clone(), None))
            .collect();
        let entities = qiyas_pdf::extract_vectors(path, spec.page, 1.0)?;
        texts.extend(
            super::pdf_text::text_runs(entities.iter())
                .into_iter()
                .map(|t| (t.at, t.text, None)),
        );
        texts.extend(ocr.iter().map(|t| (ocr_centre(t), t.text.clone(), Some(t))));
        if texts.iter().any(|(at, text, _)| {
            text.trim().is_empty()
                || !at.iter().all(|v| v.is_finite())
                || at[0] < 0.0
                || at[0] > width
                || at[1] < 0.0
                || at[1] > height
        }) {
            return Err(invalid(
                "Vision texts require nonempty text and finite page coordinates",
            ));
        }
        let mut candidates = Vec::new();
        for hint in spec.hints {
            if !valid_confidence(hint.confidence)
                || hint.points.is_empty()
                || hint.points.len() > 4096
                || hint.points.iter().any(|p| {
                    !p.iter().all(|n| n.is_finite())
                        || p[0] < 0.0
                        || p[0] > width
                        || p[1] < 0.0
                        || p[1] > height
                })
            {
                return Err(invalid(
                    "Vision hint has invalid confidence or page coordinates",
                ));
            }
            let definition = self.object_type(&hint.object_type)?;
            self.validate_recognition_rule(&Rule {
                name: "vision".into(),
                object_type: hint.object_type.clone(),
                selector: Selector::default(),
                extract: BTreeMap::new(),
                confidence: hint.confidence,
                review_below: threshold(),
                priority: 0,
                reject: false,
                sources: vec![],
            })?;
            definition.properties(&hint.props)?;
            let mut evidence = vec![Evidence {
                path: Some(spec.path.clone()),
                page: Some(spec.page),
                detail: format!(
                    "Vision hint; calibrated {} m between {:?} and {:?}; residual uncertainty {:.4}% ({} spans)",
                    calibrated.calibration.distance_m, calibrated.calibration.a, calibrated.calibration.b,
                    calibrated.uncertainty_percent, calibrated.checked_spans
                ),
                ..Default::default()
            }];
            evidence.extend(
                dimension_texts
                    .iter()
                    .map(|t| ocr_evidence(t, &spec.path, spec.page)),
            );
            let mut points = Vec::new();
            let mut issues = calibrated.warnings.clone();
            for page_point in hint.points {
                let at = [
                    spec.insert_at[0] + page_point[0] * scale,
                    spec.insert_at[1] + page_point[1] * scale,
                ];
                let modes = crate::SnapModes {
                    nearest: true,
                    ..Default::default()
                };
                if let Some(snap) = self.snap(at, radius, &modes, None) {
                    points.push(snap.point);
                    evidence.push(Evidence {
                        handle: Some(snap.handle),
                        detail: format!("geometry snap {:?}", snap.kind),
                        ..Default::default()
                    });
                } else if let Some(point) =
                    snap_ink(&pixels, page_point, [width, height], radius / scale)
                {
                    points.push([
                        spec.insert_at[0] + point[0] * scale,
                        spec.insert_at[1] + point[1] * scale,
                    ]);
                    issues
                        .push("Raster ink snap requires review of the interpreted geometry".into());
                } else {
                    points.push(at);
                    issues.push("No geometry or ink within the snap radius".into());
                }
            }
            issues.sort();
            issues.dedup();
            let spec_value = match definition.geometry {
                GeometryKind::Point if points.len() == 1 => json!({"kind":"point","p":points[0]}),
                GeometryKind::Path if points.len() >= 2 => {
                    json!({"kind":"polyline","points":points})
                }
                GeometryKind::Region if points.len() >= 3 => {
                    json!({"kind":"polyline","points":points,"closed":true})
                }
                _ => {
                    return Err(invalid(
                        "Vision hint geometry must match point, path or region type",
                    ))
                }
            };
            let driver = self
                .draw(serde_json::from_value(spec_value).map_err(|e| invalid(e.to_string()))?)?[0];
            if definition.geometry == GeometryKind::Region {
                let centre = self
                    .drawing
                    .element_metadata(driver)
                    .expect("new region")
                    .position;
                let labels = texts
                    .iter()
                    .map(|(at, text, _)| {
                        (
                            [
                                spec.insert_at[0] + at[0] * scale,
                                spec.insert_at[1] + at[1] * scale,
                            ],
                            text.clone(),
                        )
                    })
                    .filter(|(at, _)| crate::element_metadata::inside(*at, &points))
                    .collect();
                if let Some(label) = crate::element_metadata::nearest_label(centre, labels) {
                    // Recover the same nearest spatial source, retaining its OCR provenance.
                    if let Some((_, _, Some(text))) = texts
                        .iter()
                        .filter(|(at, text, _)| {
                            *text == label
                                && crate::element_metadata::inside(
                                    [
                                        spec.insert_at[0] + at[0] * scale,
                                        spec.insert_at[1] + at[1] * scale,
                                    ],
                                    &points,
                                )
                        })
                        .min_by(|a, b| {
                            let d = |p: [f64; 2]| {
                                distance(
                                    centre,
                                    [
                                        spec.insert_at[0] + p[0] * scale,
                                        spec.insert_at[1] + p[1] * scale,
                                    ],
                                )
                            };
                            d(a.0).total_cmp(&d(b.0))
                        })
                    {
                        evidence.push(ocr_evidence(text, &spec.path, spec.page));
                    }
                    let app = crate::element_metadata::REGION_TEXT_APP;
                    let app_handle = crate::xdata::register(&mut self.drawing.doc, app);
                    let entity = self
                        .drawing
                        .doc
                        .get_entity_mut(Handle::from(driver))
                        .expect("new driver");
                    crate::xdata::write_text(entity.common_mut(), app, app_handle, Some(&label));
                    evidence.push(Evidence {
                        path: Some(spec.path.clone()),
                        page: Some(spec.page),
                        detail: format!("Enclosed label: {label}"),
                        ..Default::default()
                    });
                }
            }
            let ocr_confidence = evidence
                .iter()
                .filter(|e| e.kind == Some(crate::measurement::SourceKind::Ocr))
                .filter_map(|e| e.confidence)
                .reduce(f64::min);
            if ocr_confidence.is_some() {
                issues.push("OCR text supports this raster interpretation; review required".into());
            }
            let mut candidate = Candidate {
                handle: driver,
                rule: "vision".into(),
                confidence: if calibrated.uncertainty_percent > 0.5 {
                    hint.confidence.min(0.5)
                } else {
                    hint.confidence
                }
                .min(ocr_confidence.unwrap_or(1.0)),
                state: ReviewState::Pending,
                placement: serde_json::from_value(
                    json!({"type":hint.object_type,"driver":driver,"props":hint.props}),
                )
                .expect("placement"),
                evidence,
                issues,
                object: None,
                unresolved: vec![],
            };
            if candidate.issues.is_empty() && candidate.confidence >= threshold() {
                self.accept_recognition(&mut candidate)?;
            }
            self.stamp_recognition(&candidate)?;
            candidates.push(candidate);
        }
        let mut result = json!({"candidates":candidates,"calibration":calibrated});
        if !ocr.is_empty() {
            result["texts"] = json!(ocr
                .iter()
                .map(|t| ocr_evidence(t, &spec.path, spec.page))
                .collect::<Vec<_>>());
        }
        Ok(result)
    }
}

fn ocr_centre(text: &qiyas_pdf::PageText) -> [f64; 2] {
    [
        (text.bbox[0][0] + text.bbox[1][0]) * 0.5,
        (text.bbox[0][1] + text.bbox[1][1]) * 0.5,
    ]
}

fn ocr_evidence(text: &qiyas_pdf::PageText, path: &str, page: u32) -> Evidence {
    Evidence {
        kind: Some(crate::measurement::SourceKind::Ocr),
        confidence: Some(f64::from(text.confidence)),
        bbox: Some(text.bbox),
        path: Some(path.into()),
        page: Some(page),
        detail: text.text.clone(),
        ..Default::default()
    }
}
/// Find the nearest dark pixel in displayed page coordinates. Never move beyond the requested
/// physical aperture; pixel precision is evidence, not a claim of vector accuracy.
fn snap_ink(
    image: &image::RgbaImage,
    p: [f64; 2],
    size: [f64; 2],
    radius: f64,
) -> Option<[f64; 2]> {
    let sx = f64::from(image.width()) / size[0];
    let sy = f64::from(image.height()) / size[1];
    let x = p[0] * sx;
    let y = (size[1] - p[1]) * sy;
    let rx = (radius * sx).ceil().min(128.0) as i64;
    let ry = (radius * sy).ceil().min(128.0) as i64;
    let mut best: Option<(f64, [f64; 2])> = None;
    for py in ((y as i64 - ry).max(0))..=((y as i64 + ry).min(i64::from(image.height()) - 1)) {
        for px in ((x as i64 - rx).max(0))..=((x as i64 + rx).min(i64::from(image.width()) - 1)) {
            let pixel = image.get_pixel(px as u32, py as u32).0;
            if u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2]) >= 384 {
                continue;
            }
            let point = [(px as f64 + 0.5) / sx, size[1] - (py as f64 + 0.5) / sy];
            let d = distance(p, point);
            if d <= radius && best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, point));
            }
        }
    }
    best.map(|b| b.1)
}
