//! A page's vectors and text as drawing entities.

use std::path::Path;

use opencadcodec::entities::{Line, LwPolyline, Text};
use opencadcodec::{Color, EntityType, LineWeight, Vector2, Vector3};
use pdfium_render::prelude::{
    PdfColor, PdfMatrix, PdfPageObject, PdfPageObjectCommon, PdfPageObjectsCommon, PdfPageText,
    PdfPathFillMode, PdfPathSegmentType, PdfPathSegments, PdfPoints,
};

use crate::pages::display_matrix;
use crate::{open, page_error, page_index, run, Result};

/// Stroked paths go on `PDF-RRGGBB` (their stroke colour), filled ones on `PDF-FILL-RRGGBB`.
pub const LAYER_PREFIX: &str = "PDF-";
pub const FILL_LAYER_PREFIX: &str = "PDF-FILL-";
/// Texts go on this layer, in their own colour.
pub const TEXT_LAYER: &str = "PDF-TEXT";

/// Curves are flattened to within this distance of the true curve on the page: 0.1 mm in points.
const FLATNESS_PT: f64 = 0.1 * 72.0 / 25.4;
/// CAD text height is the height of a capital letter; a PDF font size is the em. Same ratio as
/// the viewport's text (qiyas-render `CAP_EM`), so imported text is as big as on the page.
const CAP_EM: f64 = 0.7;
/// Form XObjects inside form XObjects deeper than this are left out.
const MAX_FORM_DEPTH: usize = 16;
/// The line weights a DWG can store, in 1/100 mm.
const LINE_WEIGHTS: [i16; 24] = [
    0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200,
    211,
];

/// The vectors and text of page `page` (from 1) as entities in drawing units, `scale` drawing
/// units per point, the page's bottom-left corner at (0, 0):
///
/// - each stroked subpath becomes a `Line` (two points) or an `LwPolyline` (closed when the PDF
///   closes it) on layer `PDF-RRGGBB` from the stroke colour, colour ByLayer, line weight from the
///   stroke width on paper;
/// - each subpath of a filled path that is not also stroked becomes a closed `LwPolyline` on
///   `PDF-FILL-RRGGBB` from the fill colour (a stroked and filled path keeps only its outline, so
///   no shape is drawn twice);
/// - each visible text object becomes a `Text` on `PDF-TEXT` in its fill colour, placed and turned
///   as on the page, its height the font's capital height at `scale`;
/// - curves are flattened to within 0.1 mm on the page; images, shadings and clipping-only paths
///   are left out; form XObjects are opened.
pub fn extract_vectors(path: &Path, page: u32, scale: f64) -> Result<Vec<EntityType>> {
    let path = path.to_path_buf();
    run(move |pdfium| {
        let doc = open(pdfium, &path)?;
        let index = page_index(&doc, page)?;
        let page = doc.pages().get(index).map_err(page_error)?;
        let display = display_matrix(&page)?;
        let text = page.text().map_err(page_error)?;
        let mut out = Out {
            entities: Vec::new(),
            scale,
            text: &text,
        };
        for object in page.objects().iter() {
            out.object(&object, &display, 0)?;
        }
        Ok(out.entities)
    })
}

struct Out<'t, 'a> {
    entities: Vec<EntityType>,
    scale: f64,
    text: &'t PdfPageText<'a>,
}

impl Out<'_, '_> {
    /// One page object, `to_page` taking its parent's coordinates to the page as shown.
    fn object(&mut self, object: &PdfPageObject, to_page: &[f64; 6], depth: usize) -> Result<()> {
        match object {
            PdfPageObject::Path(path) => {
                let Ok(m) = path.matrix() else { return Ok(()) };
                let xf = then(&matrix(&m), to_page);
                let stroked = path.is_stroked().unwrap_or(false);
                let filled = !matches!(path.fill_mode(), Ok(PdfPathFillMode::None) | Err(_));
                if !stroked && !filled {
                    return Ok(());
                }
                let subpaths = measured_subpaths(path.segments().raw().iter().map(|s| {
                    let (x, y) = s.point();
                    (
                        s.segment_type(),
                        apply(&xf, f64::from(x.value), f64::from(y.value)),
                        s.is_close(),
                    )
                }))?;
                if stroked {
                    let color = object.stroke_color().unwrap_or(PdfColor::BLACK);
                    let width = object
                        .stroke_width()
                        .unwrap_or(PdfPoints::ZERO)
                        .value
                        .max(0.0);
                    let weight = line_weight(f64::from(width) * mean_scale(&xf) * 25.4 / 72.0);
                    let layer = format!("{LAYER_PREFIX}{}", hex(&color));
                    for (points, closed) in subpaths {
                        self.push_path(points, closed, &layer, weight);
                    }
                } else {
                    let color = object.fill_color().unwrap_or(PdfColor::BLACK);
                    let layer = format!("{FILL_LAYER_PREFIX}{}", hex(&color));
                    for (points, _) in subpaths {
                        if points.len() >= 3 {
                            self.push_path(points, true, &layer, LineWeight::ByLayer);
                        }
                    }
                }
            }
            PdfPageObject::Text(text) => {
                if !text.is_visible() {
                    return Ok(());
                }
                let value = self.text.for_object(text);
                let value = value.trim_end_matches(['\r', '\n', '\0']);
                if value.trim().is_empty() {
                    return Ok(());
                }
                let Ok(m) = text.matrix() else { return Ok(()) };
                let [a, b, c, d, e, f] = then(&matrix(&m), to_page);
                let up = c.hypot(d);
                let size = f64::from(text.unscaled_font_size().value) * up;
                if size.is_nan() || size <= 0.0 {
                    return Ok(());
                }
                let mut t = Text::with_value(
                    value.to_string(),
                    Vector3::new(e * self.scale, f * self.scale, 0.0),
                );
                t.height = size * CAP_EM * self.scale;
                t.rotation = b.atan2(a);
                t.width_factor = a.hypot(b) / up;
                t.common.layer = TEXT_LAYER.to_string();
                let color = object.fill_color().unwrap_or(PdfColor::BLACK);
                t.common.color = match (color.red(), color.green(), color.blue()) {
                    (0, 0, 0) => Color::ByLayer,
                    (r, g, b) => Color::Rgb { r, g, b },
                };
                self.entities.push(EntityType::Text(t));
            }
            PdfPageObject::XObjectForm(form) => {
                if depth >= MAX_FORM_DEPTH {
                    return Ok(());
                }
                let Ok(m) = form.matrix() else { return Ok(()) };
                let xf = then(&matrix(&m), to_page);
                for child in form.iter() {
                    self.object(&child, &xf, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A subpath of page points as a line or polyline in drawing units.
    fn push_path(&mut self, points: Vec<[f64; 2]>, closed: bool, layer: &str, weight: LineWeight) {
        let s = self.scale;
        let mut entity = match points.as_slice() {
            [] | [_] => return,
            [p, q] if !closed => EntityType::Line(Line::from_points(
                Vector3::new(p[0] * s, p[1] * s, 0.0),
                Vector3::new(q[0] * s, q[1] * s, 0.0),
            )),
            _ => {
                let mut line = LwPolyline::from_points(
                    points
                        .iter()
                        .map(|p| Vector2::new(p[0] * s, p[1] * s))
                        .collect(),
                );
                line.is_closed = closed;
                EntityType::LwPolyline(line)
            }
        };
        let common = entity.common_mut();
        common.layer = layer.to_string();
        common.line_weight = weight;
        self.entities.push(entity);
    }
}

/// Splits path segments (type, point on the page, closes the subpath) into subpaths of points,
/// each with whether it is closed. Béziers come as three segments (two control points, then the
/// end) and are flattened. A closed subpath doesn't repeat its first point at the end.
fn measured_subpaths(
    segments: impl Iterator<Item = (PdfPathSegmentType, [f64; 2], bool)>,
) -> Result<Vec<(Vec<[f64; 2]>, bool)>> {
    let mut out: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
    let mut current: Vec<[f64; 2]> = Vec::new();
    let mut closed = false;
    let mut controls: Vec<[f64; 2]> = Vec::new();
    let mut finish = |current: &mut Vec<[f64; 2]>, closed: &mut bool| {
        let mut points = std::mem::take(current);
        if points.len() > 2 && points.first() == points.last() {
            points.pop();
            *closed = true;
        }
        if points.len() >= 2 {
            out.push((points, *closed));
        }
        *closed = false;
    };
    for (kind, p, close) in segments {
        match kind {
            PdfPathSegmentType::MoveTo => {
                finish(&mut current, &mut closed);
                controls.clear();
                current.push(p);
            }
            PdfPathSegmentType::LineTo => {
                if current.last() != Some(&p) {
                    current.push(p);
                }
            }
            PdfPathSegmentType::BezierTo => {
                controls.push(p);
                if controls.len() == 3 {
                    let start = *current.last().unwrap_or(&controls[0]);
                    if current.is_empty() {
                        current.push(start);
                    }
                    current.extend(flatten_bezier(
                        start,
                        controls[0],
                        controls[1],
                        controls[2],
                    )?);
                    controls.clear();
                }
            }
            PdfPathSegmentType::Unknown => {}
        }
        if close {
            closed = true;
        }
    }
    finish(&mut current, &mut closed);
    Ok(out)
}

/// The points after `p0` along a cubic Bézier, within `FLATNESS_PT` of the curve.
fn flatten_bezier(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) -> Result<Vec<[f64; 2]>> {
    use kurbo::{PathEl, Point};
    // Reject pathological curves explicitly instead of relaxing the measuring tolerance.
    // This bounds Kurbo's subdivision work as well as the output allocation.
    const MAX_POINTS: usize = 65_536;
    const MAX_EXTENT_PT: f64 = FLATNESS_PT * 1e8;
    let too_large =
        || crate::PdfError::Page("A PDF curve exceeds the vector import precision budget".into());
    if [p0, p1, p2, p3].iter().flatten().any(|v| !v.is_finite())
        || [p1, p2, p3]
            .iter()
            .any(|p| (p[0] - p0[0]).abs().max((p[1] - p0[1]).abs()) > MAX_EXTENT_PT)
    {
        return Err(too_large());
    }
    // Controls are already transformed to displayed PDF points. The existing 0.1 mm
    // measuring tolerance applies here, before `push_path` converts to drawing units.
    let local = |p: [f64; 2]| Point::new(p[0] - p0[0], p[1] - p0[1]);
    let path = [
        PathEl::MoveTo(local(p0)),
        PathEl::CurveTo(local(p1), local(p2), local(p3)),
    ];
    let mut points = Vec::new();
    kurbo::flatten(path, FLATNESS_PT, |element| {
        if let PathEl::LineTo(p) = element {
            if points.len() <= MAX_POINTS {
                points.push([p.x + p0[0], p.y + p0[1]]);
            }
        }
    });
    if points.len() > MAX_POINTS {
        return Err(too_large());
    }
    // Preserve the PDF's endpoint bits after local-origin arithmetic.
    if let Some(last) = points.last_mut() {
        *last = p3;
    }
    Ok(points)
}

// Keep the existing unit-test entry points; production propagates budget errors to the caller.
#[cfg(test)]
fn bezier(p0: [f64; 2], p1: [f64; 2], p2: [f64; 2], p3: [f64; 2]) -> Vec<[f64; 2]> {
    flatten_bezier(p0, p1, p2, p3).expect("test curve within budget")
}

#[cfg(test)]
fn subpaths(
    segments: impl Iterator<Item = (PdfPathSegmentType, [f64; 2], bool)>,
) -> Vec<(Vec<[f64; 2]>, bool)> {
    measured_subpaths(segments).expect("test paths within budget")
}

fn matrix(m: &PdfMatrix) -> [f64; 6] {
    [m.a(), m.b(), m.c(), m.d(), m.e(), m.f()].map(f64::from)
}

/// `first` then `second`, PDF matrices `[a b c d e f]`.
fn then(first: &[f64; 6], second: &[f64; 6]) -> [f64; 6] {
    let [a, b, c, d, e, f] = *first;
    let [a2, b2, c2, d2, e2, f2] = *second;
    [
        a * a2 + b * c2,
        a * b2 + b * d2,
        c * a2 + d * c2,
        c * b2 + d * d2,
        e * a2 + f * c2 + e2,
        e * b2 + f * d2 + f2,
    ]
}

fn apply(m: &[f64; 6], x: f64, y: f64) -> [f64; 2] {
    [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/// How much `m` stretches lengths, on average over its two axes.
fn mean_scale(m: &[f64; 6]) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}

/// The DWG line weight nearest to `mm` millimetres.
fn line_weight(mm: f64) -> LineWeight {
    let hundredths = (mm * 100.0).round();
    let nearest = LINE_WEIGHTS
        .iter()
        .min_by(|a, b| {
            (f64::from(**a) - hundredths)
                .abs()
                .total_cmp(&(f64::from(**b) - hundredths).abs())
        })
        .copied()
        .unwrap_or(0);
    LineWeight::Value(nearest)
}

fn hex(c: &PdfColor) -> String {
    format!("{:02X}{:02X}{:02X}", c.red(), c.green(), c.blue())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subpaths_close_and_split() {
        use PdfPathSegmentType::{LineTo, MoveTo};
        let segments = [
            (MoveTo, [0.0, 0.0], false),
            (LineTo, [10.0, 0.0], false),
            (LineTo, [10.0, 5.0], false),
            (LineTo, [0.0, 5.0], true),
            (MoveTo, [20.0, 0.0], false),
            (LineTo, [30.0, 0.0], false),
        ];
        let out = subpaths(segments.into_iter());
        assert_eq!(
            out,
            vec![
                (vec![[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]], true),
                (vec![[20.0, 0.0], [30.0, 0.0]], false),
            ]
        );
        // Closed by returning to the start: the start is not repeated.
        let back = [
            (MoveTo, [0.0, 0.0], false),
            (LineTo, [1.0, 0.0], false),
            (LineTo, [1.0, 1.0], false),
            (LineTo, [0.0, 0.0], false),
        ];
        assert_eq!(
            subpaths(back.into_iter()),
            vec![(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]], true)]
        );
    }

    #[test]
    fn a_quarter_circle_bezier_stays_within_a_tenth_of_a_millimetre() {
        // The usual 4-Bézier circle of radius 100 pt (35 mm): one quarter.
        let k = 0.552_284_8 * 100.0;
        let points = bezier([100.0, 0.0], [100.0, k], [k, 100.0], [0.0, 100.0]);
        assert_eq!(points.last(), Some(&[0.0, 100.0]));
        assert!(points.len() > 4);
        // Every chord's midpoint is near the circle (the Bézier itself is within 0.03 pt of it).
        let mut prev = [100.0, 0.0];
        for p in points {
            let mid = [(prev[0] + p[0]) / 2.0, (prev[1] + p[1]) / 2.0];
            let off = 100.0 - mid[0].hypot(mid[1]);
            assert!(off < FLATNESS_PT + 0.03, "{off}");
            prev = p;
        }
    }

    #[test]
    fn line_weights_snap_to_dwg_values() {
        assert_eq!(line_weight(0.0), LineWeight::Value(0));
        assert_eq!(line_weight(0.24), LineWeight::Value(25));
        assert_eq!(line_weight(0.36), LineWeight::Value(35));
        assert_eq!(line_weight(5.0), LineWeight::Value(211));
    }

    #[test]
    fn matrices_compose_first_then_second() {
        let scale = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        let shift = [1.0, 0.0, 0.0, 1.0, 10.0, 0.0];
        assert_eq!(apply(&then(&scale, &shift), 1.0, 1.0), [12.0, 2.0]);
        assert_eq!(apply(&then(&shift, &scale), 1.0, 1.0), [22.0, 2.0]);
    }
}
