//! What the batch draws besides outlines: hatch fills and pattern lines, 2D solids and traces,
//! wipeouts, multilines, rays and construction lines. Measuring (`into_paths`) never sees the
//! fills, the pattern lines or the infinite lines.

use std::f64::consts::FRAC_PI_4;

use opencadcodec::entities::{
    BoundaryEdge, BoundaryPath, EntityType, Hatch, HatchPattern, HatchPatternLine, MLine, Point,
    PolylineEdge, Ray, Solid, Spline, Wipeout, XLine,
};
use opencadcodec::{CadDocument, Color, Vector2, Vector3};
use qiyas_geom::{flatten, flatten_within, FlatPath, Paint, Xform, XrefDocs, MAX_WORK};

fn painted(doc: &CadDocument, entity: &EntityType) -> Vec<FlatPath> {
    flatten_within(
        doc,
        &XrefDocs::new(),
        entity,
        &Xform::identity(),
        1e-3,
        &mut MAX_WORK.clone(),
    )
    .paths
}

fn ring(points: &[[f64; 2]]) -> BoundaryPath {
    let mut path = BoundaryPath::external();
    path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
        points.iter().map(|p| Vector2::new(p[0], p[1])).collect(),
        true,
    )));
    path
}

const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
const ISLAND: [[f64; 2]; 4] = [[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0]];

/// A hatch of the square with the island in it.
fn hatch(pattern: Option<HatchPattern>) -> EntityType {
    let mut h = match pattern {
        Some(p) => Hatch::with_pattern(p),
        None => Hatch::solid(),
    };
    h.add_path(ring(&SQUARE));
    h.add_path(ring(&ISLAND));
    EntityType::Hatch(h)
}

fn ansi31(spacing: f64) -> HatchPattern {
    let mut p = HatchPattern::new("ANSI31");
    let (s, c) = FRAC_PI_4.sin_cos();
    p.add_line(HatchPatternLine {
        angle: FRAC_PI_4,
        base_point: Vector2::new(0.0, 0.0),
        offset: Vector2::new(-s * spacing, c * spacing),
        dash_lengths: Vec::new(),
    });
    p
}

fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let n = ring.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x > p[0] {
                odd = !odd;
            }
        }
    }
    odd
}

#[test]
fn a_solid_hatch_fills_its_rings_and_still_outlines_them() {
    let doc = CadDocument::new();
    let paths = painted(&doc, &hatch(None));
    let outlines: Vec<_> = paths.iter().filter(|p| p.paint == Paint::Outline).collect();
    assert_eq!(outlines.len(), 2);
    let fills: Vec<_> = paths
        .iter()
        .filter_map(|p| match &p.paint {
            Paint::Fill(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1);
    assert_eq!(
        fills[0].rings.len(),
        2,
        "the island is a ring of the same fill"
    );
    assert!(!fills[0].background);
    // Measuring sees the outlines only.
    let measured = flatten(&doc, &hatch(None), &Xform::identity(), 1e-3).into_paths();
    assert_eq!(measured.len(), 2);
}

#[test]
fn pattern_lines_cross_the_hatch_and_skip_its_island() {
    let doc = CadDocument::new();
    let paths = painted(&doc, &hatch(Some(ansi31(1.0))));
    assert!(!paths.iter().any(|p| matches!(p.paint, Paint::Fill(_))));
    let spans: Vec<_> = paths.iter().filter(|p| p.paint == Paint::Pattern).collect();
    // Lines 1 apart across a 10 x 10 square's diagonal reach: about 14 of them, some split in two
    // by the island.
    assert!(spans.len() > 12 && spans.len() < 30, "{}", spans.len());
    for s in &spans {
        assert_eq!(s.path.points.len(), 2);
        let [a, b] = [s.path.points[0], s.path.points[1]];
        // At 45 degrees.
        assert!(((b[1] - a[1]) - (b[0] - a[0])).abs() < 1e-9, "{a:?} {b:?}");
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        assert!(inside(&SQUARE, mid) && !inside(&ISLAND, mid), "{mid:?}");
        for p in [a, b] {
            assert!(p.iter().all(|v| *v > -1e-9 && *v < 10.0 + 1e-9), "{p:?}");
        }
    }
    // Every line's length inside: the square's area less the island's, over the spacing.
    let total: f64 = spans.iter().map(|s| s.path.length()).sum();
    assert!((total - 96.0).abs() < 1.5, "{total}");
    // Never measured.
    let measured = flatten(&doc, &hatch(Some(ansi31(1.0))), &Xform::identity(), 1e-3);
    assert_eq!(measured.into_paths().len(), 2);
}

#[test]
fn dashed_pattern_lines_draw_their_dashes_only() {
    let doc = CadDocument::new();
    let mut p = HatchPattern::new("DASH");
    p.add_line(HatchPatternLine {
        angle: 0.0,
        base_point: Vector2::new(0.0, 0.5),
        offset: Vector2::new(0.0, 2.0),
        dash_lengths: vec![1.0, -1.0],
    });
    let mut h = Hatch::with_pattern(p);
    h.add_path(ring(&SQUARE));
    let spans: Vec<_> = painted(&doc, &EntityType::Hatch(h))
        .into_iter()
        .filter(|p| p.paint == Paint::Pattern)
        .collect();
    // Five lines (y = 0.5, 2.5 ... 8.5), five dashes of 1 each.
    assert_eq!(spans.len(), 25);
    assert!(spans.iter().all(|s| (s.path.length() - 1.0).abs() < 1e-9));
}

#[test]
fn a_pattern_too_dense_to_see_is_left_out() {
    let doc = CadDocument::new();
    let paths = painted(&doc, &hatch(Some(ansi31(1e-4))));
    assert!(!paths.iter().any(|p| p.paint == Paint::Pattern));
    assert_eq!(paths.len(), 2, "the outline stays");
}

#[test]
fn two_d_solids_and_traces_are_filled_outlines() {
    let doc = CadDocument::new();
    let v = |x, y| Vector3::new(x, y, 0.0);
    // AutoCAD's corner order: the outline runs 1, 2, 4, 3.
    let solid = EntityType::Solid(Solid::new(
        v(0.0, 0.0),
        v(2.0, 0.0),
        v(0.0, 1.0),
        v(2.0, 1.0),
    ));
    let paths = painted(&doc, &solid);
    let outline = paths.iter().find(|p| p.paint == Paint::Outline).unwrap();
    assert!(outline.path.closed);
    assert!((outline.path.area() - 2.0).abs() < 1e-9);
    let Some(Paint::Fill(fill)) = paths
        .iter()
        .map(|p| &p.paint)
        .find(|p| matches!(p, Paint::Fill(_)))
    else {
        panic!("a fill");
    };
    assert_eq!(fill.rings.len(), 1);
    assert_eq!(fill.rings[0].len(), 4);
    // Measured as its outline.
    let measured = flatten(&doc, &solid, &Xform::identity(), 1e-3).into_paths();
    assert_eq!(measured.len(), 1);
}

#[test]
fn a_wipeout_fills_with_the_background_inside_its_frame() {
    let doc = CadDocument::new();
    let points = [[1.0, 1.0], [5.0, 1.0], [3.0, 4.0], [1.0, 1.0]];
    let w = Wipeout::polygonal(&points.map(|p| Vector2::new(p[0], p[1])), 0.0);
    let paths = painted(&doc, &EntityType::Wipeout(w));
    let outline = paths.iter().find(|p| p.paint == Paint::Outline).unwrap();
    assert!(outline.path.closed);
    assert_eq!(
        outline.path.points.len(),
        3,
        "the closing point is not repeated"
    );
    assert!((outline.path.area() - 6.0).abs() < 1e-9);
    let Some(Paint::Fill(fill)) = paths
        .iter()
        .map(|p| &p.paint)
        .find(|p| matches!(p, Paint::Fill(_)))
    else {
        panic!("a fill");
    };
    assert!(fill.background);
    // A rectangular wipeout's two clip corners make four.
    let rect = Wipeout::from_corners(Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.0, 3.0, 0.0));
    let paths = painted(&doc, &EntityType::Wipeout(rect));
    let outline = paths.iter().find(|p| p.paint == Paint::Outline).unwrap();
    assert!((outline.path.area() - 6.0).abs() < 1e-9);
}

#[test]
fn a_multiline_draws_one_line_per_style_element() {
    let doc = CadDocument::new();
    let mut m = MLine::new();
    for p in [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]] {
        m.add_vertex(Vector3::new(p[0], p[1], 0.0));
    }
    // The drawing's Standard style: offsets 0.5 and -0.5.
    m.style_handle = doc.objects.iter().find_map(|(h, o)| {
        matches!(o, opencadcodec::objects::ObjectType::MLineStyle(_)).then_some(*h)
    });
    m.scale_factor = 2.0;
    let entity = EntityType::MLine(m);
    // Measured along its centre line, the points it was drawn through: 4 + 4.
    let measured = flatten(&doc, &entity, &Xform::identity(), 1e-3).into_paths();
    assert_eq!(measured.len(), 1);
    assert!((measured[0].length() - 8.0).abs() < 1e-9);
    let paths = painted(&doc, &entity);
    let lines: Vec<_> = paths.iter().filter(|p| p.paint == Paint::Outline).collect();
    assert_eq!(lines.len(), 2);
    // Offsets 1 and -1 at scale 2, mitred at the corner.
    let mut starts: Vec<[f64; 2]> = lines.iter().map(|l| l.path.points[0]).collect();
    starts.sort_by(|a, b| a[1].total_cmp(&b[1]));
    assert_eq!(starts, [[0.0, -1.0], [0.0, 1.0]]);
    let corners: Vec<[f64; 2]> = lines.iter().map(|l| l.path.points[1]).collect();
    for c in corners {
        assert!(
            (c[0] - 3.0).abs() < 1e-9 && (c[1] - 1.0).abs() < 1e-9
                || (c[0] - 5.0).abs() < 1e-9 && (c[1] + 1.0).abs() < 1e-9,
            "{c:?}"
        );
    }
}

#[test]
fn rays_and_construction_lines_are_painted_only() {
    let doc = CadDocument::new();
    let ray = EntityType::Ray(Ray::new(
        Vector3::new(1.0, 2.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ));
    let xline = EntityType::XLine(XLine::new(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
    ));
    for (entity, both) in [(ray, false), (xline, true)] {
        let paths = painted(&doc, &entity);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].paint, Paint::Infinite { both_ways: both });
        assert_eq!(paths[0].path.points.len(), 2);
        assert!(flatten(&doc, &entity, &Xform::identity(), 1e-3)
            .into_paths()
            .is_empty());
    }
}

#[test]
fn a_point_is_painted_at_its_place_and_never_measured() {
    let mut doc = CadDocument::new();
    let point = EntityType::Point(Point::at(Vector3::new(3.0, 4.0, 0.0)));
    let paths = painted(&doc, &point);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].paint, Paint::Point { size: 0.0, mode: 0 });
    assert_eq!(paths[0].path.points, [[3.0, 4.0]]);
    assert!(flatten(&doc, &point, &Xform::identity(), 1e-3)
        .into_paths()
        .is_empty());
    // The drawing's PDSIZE, when positive, is its size.
    doc.header.point_display_size = 0.5;
    assert_eq!(
        painted(&doc, &point)[0].paint,
        Paint::Point { size: 0.5, mode: 0 }
    );
}

/// The spline through four fit points, as the draw command stores it: the cubic's control
/// points and knots besides the fit points.
fn smooth_spline() -> (Spline, Vec<[f64; 2]>) {
    use opencadkernel::geom2d::{NurbsCurve, Parameterization};
    let fit = vec![[0.0, 0.0], [3.0, 2.0], [6.0, -2.0], [9.0, 0.0]];
    let curve = NurbsCurve::interpolate(&fit, None, None, Parameterization::Chord).unwrap();
    let v = |p: &[f64; 2]| Vector3::new(p[0], p[1], 0.0);
    let mut s = Spline::from_fit_points(fit.iter().map(v).collect());
    s.degree = curve.degree() as i32;
    s.control_points = curve.control_points().iter().map(v).collect();
    s.knots = curve.knots().to_vec();
    (s, fit)
}

fn chord_length(points: &[[f64; 2]]) -> f64 {
    points
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

#[test]
fn a_spline_is_drawn_and_measured_along_its_curve_not_its_fit_points() {
    let doc = CadDocument::new();
    let (spline, fit) = smooth_spline();
    let fit_length = chord_length(&fit);
    for fit_only in [false, true] {
        let mut spline = spline.clone();
        if fit_only {
            // Only fit points: the curve through them, still.
            spline.control_points.clear();
            spline.knots.clear();
        }
        let entity = EntityType::Spline(spline);
        let measured = flatten(&doc, &entity, &Xform::identity(), 1e-4).into_paths();
        assert_eq!(measured.len(), 1);
        let points = &measured[0].points;
        assert!(points.len() > 8, "{points:?}");
        // Through the fit points, longer than the polyline between them.
        for f in &fit {
            assert!(points
                .iter()
                .any(|p| (p[0] - f[0]).hypot(p[1] - f[1]) < 1e-3));
        }
        assert!(
            measured[0].length() > fit_length + 0.1,
            "{}",
            measured[0].length()
        );
        // Half way between the first two fit points the curve is off their chord.
        let mid = points
            .iter()
            .min_by(|a, b| (a[0] - 1.5).abs().total_cmp(&(b[0] - 1.5).abs()))
            .unwrap();
        assert!((mid[1] - 1.0).abs() > 0.05, "{mid:?}");
        // Drawn along the same curve (coarser).
        let drawn = painted(&doc, &entity);
        assert!((drawn[0].path.length() - measured[0].length()).abs() < 1e-2);
    }
}

#[test]
fn a_gradient_hatch_is_filled_with_its_first_colour() {
    let doc = CadDocument::new();
    let EntityType::Hatch(mut h) = hatch(None) else {
        unreachable!()
    };
    h.gradient_color.enabled = true;
    h.gradient_color.add_color(0.0, Color::from_index(5));
    h.gradient_color.add_color(1.0, Color::from_index(2));
    let paths = painted(&doc, &EntityType::Hatch(h));
    let fill = paths
        .iter()
        .find(|p| matches!(p.paint, Paint::Fill(_)))
        .expect("a fill");
    assert_eq!(fill.color, Color::from_index(5));
    let Paint::Fill(f) = &fill.paint else {
        unreachable!()
    };
    assert_eq!(f.rings.len(), 2);
    assert!(!f.background);
}
