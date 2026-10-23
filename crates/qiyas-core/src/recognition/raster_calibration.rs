//! Raster calibration uses only visible reference spans and transcribed dimension text.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisionDimension {
    pub a: [f64; 2],
    pub b: [f64; 2],
    /// Verbatim dimension text from vision, OCR or the PDF text layer (for example `4500 mm`).
    pub text: String,
    /// Unit conversion for bare dimension numbers; explicit text units take precedence.
    pub metres_per_unit: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RasterCalibration {
    pub calibration: Calibration,
    pub uncertainty_percent: f64,
    pub refined: bool,
    pub checked_spans: usize,
    pub warnings: Vec<String>,
}

/// Refine every supplied span, prefer the longest verified one and compare independent scales.
/// This function has no access to files, benchmark metadata or truth.
pub fn calibrate_raster(
    image: &image::RgbaImage,
    size: [f64; 2],
    primary: &Calibration,
    additional: &[Calibration],
    dimensions: &[VisionDimension],
) -> Result<RasterCalibration, CoreError> {
    let mut spans = vec![primary.clone()];
    spans.extend_from_slice(additional);
    let mut warnings = Vec::new();
    for dimension in dimensions {
        match dimension_distance(&dimension.text, dimension.metres_per_unit) {
            Some(distance_m) => spans.push(Calibration {
                a: dimension.a,
                b: dimension.b,
                distance_m,
            }),
            None => warnings.push(format!("Unresolved dimension text: {}", dimension.text)),
        }
    }
    let mut measured = Vec::new();
    for span in spans {
        span.drawing_scale(1.0)?;
        let refined = qiyas_pdf::refine_raster_span(image, size, span.a, span.b);
        let (span, uncertainty, found) = if let Some(result) = refined {
            (
                Calibration {
                    a: result.a,
                    b: result.b,
                    distance_m: span.distance_m,
                },
                result.uncertainty,
                true,
            )
        } else {
            // Unverified anchors have at least two page pixels of uncertainty, even when a
            // high-resolution rendering merely interpolates an existing scan.
            let uncertainty = (2.0 / distance(span.a, span.b)).max(0.01);
            (span, uncertainty, false)
        };
        measured.push((span, uncertainty, found));
    }
    measured.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| distance(b.0.a, b.0.b).total_cmp(&distance(a.0.a, a.0.b)))
    });
    let (selected, mut uncertainty, refined) = measured[0].clone();
    let scale = selected.drawing_scale(1.0)?;
    for (span, _, _) in measured.iter().skip(1) {
        uncertainty = uncertainty.max((span.drawing_scale(1.0)? / scale - 1.0).abs());
    }
    if !refined {
        warnings.push("No unambiguous scale ticks; supplied anchors require review".into());
    }
    if measured.len() == 1 {
        warnings.push("No independent dimension span supplied for cross-check".into());
    }
    if !warnings.is_empty()
        && dimensions
            .iter()
            .any(|d| dimension_distance(&d.text, d.metres_per_unit).is_none())
    {
        uncertainty = uncertainty.max(0.01);
    }
    if uncertainty > 0.005 {
        warnings.push("Residual calibration uncertainty exceeds 0.5%; confidence reduced".into());
    }
    Ok(RasterCalibration {
        calibration: selected,
        uncertainty_percent: uncertainty * 100.0,
        refined,
        checked_spans: measured.len(),
        warnings,
    })
}

fn dimension_distance(text: &str, default_unit: f64) -> Option<f64> {
    let regex =
        Regex::new(r#"(?i)^\s*(?:[Ø⌀])?\s*([0-9]+(?:\.[0-9]+)?)\s*(mm|cm|m|ft|in|′|')?\s*$"#)
            .ok()?;
    let captures = regex.captures(text)?;
    let unit = match captures
        .get(2)
        .map(|m| m.as_str().to_lowercase())
        .as_deref()
    {
        Some("mm") => 0.001,
        Some("cm") => 0.01,
        Some("m") => 1.0,
        Some("ft" | "′" | "'") => 0.3048,
        Some("in") => 0.0254,
        None => default_unit,
        _ => return None,
    };
    let value = captures[1].parse::<f64>().ok()? * unit;
    (unit.is_finite() && unit > 0.0 && value.is_finite() && value > 0.0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn longest_span_and_dimension_disagreement_control_uncertainty() {
        let mut image = image::RgbaImage::from_pixel(500, 100, image::Rgba([255; 4]));
        for x in [20, 60, 120, 420] {
            for y in 44..56 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
            }
        }
        for (a, b) in [(20, 60), (120, 420)] {
            for x in a..=b {
                for y in 49..51 {
                    image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
                }
            }
        }
        let short = Calibration {
            a: [20.5, 50.0],
            b: [60.5, 50.0],
            distance_m: 1.0,
        };
        let long = Calibration {
            a: [120.5, 50.0],
            b: [420.5, 50.0],
            distance_m: 7.5,
        };
        let result = calibrate_raster(
            &image,
            [500.0, 100.0],
            &short,
            std::slice::from_ref(&long),
            &[],
        )
        .unwrap();
        assert_eq!(result.calibration.distance_m, 7.5);
        assert!(result.uncertainty_percent < 0.5);
        let dimension = VisionDimension {
            a: long.a,
            b: long.b,
            text: "7600 mm".into(),
            metres_per_unit: 1.0,
        };
        let result =
            calibrate_raster(&image, [500.0, 100.0], &short, &[long], &[dimension]).unwrap();
        assert!(result.uncertainty_percent > 1.0);
        assert_eq!(result.checked_spans, 3);
        assert_eq!(dimension_distance("4500", 0.001), Some(4.5));
        assert_eq!(dimension_distance("2 rooms", 1.0), None);
    }
}
