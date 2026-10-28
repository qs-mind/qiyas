//! Shared planar predicates. Coordinates and absolute tolerances are in drawing units.
use geo::coordinate_position::{CoordPos, CoordinatePosition};
use geo::line_intersection::{line_intersection, LineIntersection};
use geo::{
    Area, Centroid, ConvexHull, Coord, Distance, Euclidean, Line, LineString, Polygon, Triangle,
};

type Point = [f64; 2];

fn local(p: Point, origin: Point) -> Coord<f64> {
    Coord {
        x: p[0] - origin[0],
        y: p[1] - origin[1],
    }
}

pub fn orientation(a: Point, b: Point, c: Point) -> f64 {
    let coord = |p: Point| robust::Coord { x: p[0], y: p[1] };
    // Let the adaptive predicate retain the original input bits, including survey coordinates.
    robust::orient2d(coord(a), coord(b), coord(c))
}

pub fn point_segment(p: Point, a: Point, b: Point) -> f64 {
    Euclidean.distance(local(p, a), &Line::new(local(a, a), local(b, a)))
}

pub fn point_line(p: Point, a: Point, b: Point) -> f64 {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    if length == 0.0 {
        point_segment(p, a, b)
    } else {
        orientation(a, b, p).abs() / length
    }
}

/// The selection tool's relative edge tolerance and square endpoint padding.
pub fn on_edge(p: Point, a: Point, b: Point) -> bool {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    let tol = length.max(f64::MIN_POSITIVE) * 1e-9;
    orientation(a, b, p).abs() <= tol * length
        && (0..2).all(|i| p[i] >= a[i].min(b[i]) - tol && p[i] <= a[i].max(b[i]) + tol)
}

fn intersection(a: Point, b: Point, c: Point, d: Point) -> Option<LineIntersection<f64>> {
    line_intersection(
        Line::new(local(a, a), local(b, a)),
        Line::new(local(c, a), local(d, a)),
    )
}

/// A single meeting point, including ends. An overlapping interval has no unique point.
pub fn crossing(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    match intersection(a, b, c, d)? {
        LineIntersection::SinglePoint {
            intersection: p, ..
        } => Some([p.x + a[0], p.y + a[1]]),
        LineIntersection::Collinear { .. } => None,
    }
}

pub fn intersects(a: Point, b: Point, c: Point, d: Point) -> bool {
    intersection(a, b, c, d).is_some()
}

pub fn cross_properly(a: Point, b: Point, c: Point, d: Point) -> bool {
    intersection(a, b, c, d).is_some_and(|hit| hit.is_proper())
}

pub fn edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
}

/// Even-odd containment, retaining the old half-open boundary convention.
/// XOR of a triangle fan gives even-odd for concave and self-crossing rings too;
/// geo's whole-polygon containment instead uses nonzero winding. Fan seams and true
/// boundaries use geo intersections with the old strict-right, half-open ray policy.
pub fn inside(p: Point, ring: &[Point]) -> bool {
    let Some(&first) = ring.first() else {
        return false;
    };
    let query = local(p, p);
    let mut odd = false;
    for pair in ring[1..].windows(2) {
        if orientation(first, pair[0], pair[1]) == 0.0 {
            continue;
        }
        let triangle =
            Triangle::unchecked_winding(local(first, p), local(pair[0], p), local(pair[1], p));
        match triangle.coordinate_position(&query) {
            CoordPos::Inside => odd = !odd,
            CoordPos::Outside => {}
            CoordPos::OnBoundary => {
                let right = ring.iter().map(|v| v[0] - p[0]).fold(0.0, f64::max);
                let ray = Line::new(query, Coord { x: right, y: 0.0 });
                return edges(ring).filter(|&(a, b)| {
                    if (a[1] > p[1]) == (b[1] > p[1]) { return false; }
                    matches!(line_intersection(ray, Line::new(local(a, p), local(b, p))),
                        Some(LineIntersection::SinglePoint { intersection, .. }) if intersection.x > 0.0)
                }).count() % 2 == 1;
            }
        }
    }
    odd
}

pub fn in_ring(p: Point, ring: &[Point]) -> bool {
    edges(ring).any(|(a, b)| on_edge(p, a, b)) || inside(p, ring)
}

pub fn inside_with_tolerance(ring: &[Point], p: Point, tolerance: f64) -> bool {
    edges(ring).any(|(a, b)| a != b && point_segment(p, a, b) <= tolerance) || inside(p, ring)
}

/// Every interval between boundary meetings must remain inside the ring.
pub fn segment_inside(ring: &[Point], a: Point, b: Point, tolerance: f64) -> bool {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let axis = usize::from(delta[1].abs() > delta[0].abs());
    if delta[axis] == 0.0 {
        return inside_with_tolerance(ring, a, tolerance);
    }
    let mut cuts = vec![0.0, 1.0];
    for (p, q) in edges(ring) {
        let mut add = |v: Coord<f64>| {
            cuts.push(((if axis == 0 { v.x } else { v.y }) / delta[axis]).clamp(0.0, 1.0))
        };
        match intersection(a, b, p, q) {
            Some(LineIntersection::SinglePoint { intersection, .. }) => add(intersection),
            Some(LineIntersection::Collinear { intersection }) => {
                add(intersection.start);
                add(intersection.end);
            }
            None => {}
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).all(|pair| {
        let t = (pair[0] + pair[1]) * 0.5;
        inside_with_tolerance(ring, [a[0] + t * delta[0], a[1] + t * delta[1]], tolerance)
    })
}

fn polygon(ring: &[Point], origin: Point) -> Polygon<f64> {
    Polygon::new(
        LineString::new(ring.iter().map(|&p| local(p, origin)).collect()),
        vec![],
    )
}

pub fn ring_area(ring: &[Point]) -> f64 {
    ring.first()
        .map_or(0.0, |&origin| polygon(ring, origin).signed_area())
}

pub fn ring_centroid(ring: &[Point]) -> Option<Point> {
    let origin = *ring.first()?;
    polygon(ring, origin)
        .centroid()
        .map(|p| [p.x() + origin[0], p.y() + origin[1]])
}

pub fn convex_hull(points: &[Point]) -> Vec<Point> {
    let mut points = points.to_vec();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-12);
    if points.len() < 3 {
        return points;
    }
    let origin = points[0];
    let hull = LineString::new(points.iter().map(|&p| local(p, origin)).collect()).convex_hull();
    let mut out: Vec<_> = hull
        .exterior()
        .0
        .iter()
        .map(|p| [p.x + origin[0], p.y + origin[1]])
        .collect();
    if out.first() == out.last() {
        out.pop();
    }
    canonical_start(&mut out);
    out
}

pub(crate) fn canonical_start(ring: &mut [Point]) {
    if let Some((i, _)) = ring
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])))
    {
        ring.rotate_left(i);
    }
}

/// Whether the segment `s` (`x0 y0 x1 y1`) lies wholly inside the polygon `ring`: both ends and
/// its middle inside, never crossing an edge.
pub fn inside_ring([x0, y0, x1, y1]: [f64; 4], ring: &[[f64; 2]]) -> bool {
    let (p, q) = ([x0, y0], [x1, y1]);
    in_ring(p, ring)
        && in_ring(q, ring)
        && in_ring([(x0 + x1) / 2.0, (y0 + y1) / 2.0], ring)
        && !ring
            .iter()
            .enumerate()
            .any(|(k, a)| cross_properly(p, q, *a, ring[(k + 1) % ring.len()]))
}

/// Whether the segment `s` touches the polygon `ring`: an end inside it, or meeting an edge.
pub fn touches_ring([x0, y0, x1, y1]: [f64; 4], ring: &[[f64; 2]]) -> bool {
    let (p, q) = ([x0, y0], [x1, y1]);
    in_ring(p, ring)
        || in_ring(q, ring)
        || ring.iter().enumerate().any(|(k, a)| {
            let b = ring[(k + 1) % ring.len()];
            cross_properly(p, q, *a, b) || on_edge(*a, p, q) || on_edge(b, p, q)
        })
}
