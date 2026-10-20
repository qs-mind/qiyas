/// Points closer than this (drawing units) are treated as the same point.
pub const SAME_POINT: f64 = 1e-9;
/// Far from the origin, points this share of their coordinates' size apart are the same point
/// (float steps there exceed `SAME_POINT`: 5e-7 at a 3.3e9 mm survey coordinate).
const SAME_POINT_REL: f64 = 1e-13;

/// A flattened polyline in drawing units. A closed path does not repeat its first point.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
}

impl Path {
    /// Total segment length, including the closing segment of a closed path.
    pub fn length(&self) -> f64 {
        self.segments().map(|(a, b)| dist(a, b)).sum()
    }

    /// Enclosed area, 0.0 when open. A self-intersecting path is split at its
    /// crossings and the loop areas are added, so a bowtie counts both lobes.
    pub fn area(&self) -> f64 {
        if self.closed {
            loops_area(&self.points)
        } else {
            0.0
        }
    }

    /// A closed path split at its crossings into simple loops (itself when it has none); none
    /// when open. A ring that touches itself round a pinched-in hole gives the hole as a loop.
    pub fn loops(&self) -> Vec<Vec<[f64; 2]>> {
        let mut out = Vec::new();
        if self.closed {
            split_loops(distinct(&self.points, true), &mut out);
        }
        out
    }

    /// True when two non-adjacent segments meet (parallel overlaps are not detected).
    pub fn self_intersects(&self) -> bool {
        first_crossing(&distinct(&self.points, self.closed), self.closed).is_some()
    }

    /// `[[min_x, min_y], [max_x, max_y]]`, or `None` for an empty path.
    pub fn bounds(&self) -> Option<[[f64; 2]; 2]> {
        let first = *self.points.first()?;
        Some(self.points.iter().fold([first, first], |[lo, hi], p| {
            [
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            ]
        }))
    }

    fn segments(&self) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
        let n = self.points.len();
        (0..edge_count(n, self.closed)).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }
}

/// `points` without consecutive repeats; a closed path also drops a last point equal to
/// its first. Repeats would make neighbouring segments look like they touch.
fn distinct(points: &[[f64; 2]], closed: bool) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|&last| !same_point(last, p)) {
            out.push(p);
        }
    }
    if closed && out.len() > 1 && same_point(out[0], out[out.len() - 1]) {
        out.pop();
    }
    out
}

/// Same-point tolerance near coordinates of this size. `absolute` is expressed in the
/// caller's drawing units; the relative part protects survey-coordinate arithmetic.
pub fn tol_near(size: f64, absolute: f64) -> f64 {
    absolute.max(size.abs() * SAME_POINT_REL)
}

pub(crate) fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    let size = a[0].abs().max(a[1].abs()).max(b[0].abs()).max(b[1].abs());
    let tol = tol_near(size, SAME_POINT);
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// Segments in a path of `n` points; a closed path of three or more adds the closing one.
fn edge_count(n: usize, closed: bool) -> usize {
    if closed && n > 2 {
        n
    } else {
        n.saturating_sub(1)
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(u: [f64; 2], v: [f64; 2]) -> f64 {
    u[0] * v[1] - u[1] * v[0]
}

fn shoelace(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    let twice: f64 = (0..n).map(|i| cross(points[i], points[(i + 1) % n])).sum();
    (twice / 2.0).abs()
}

/// The loop areas of a closed ring split at its crossings, added.
fn loops_area(points: &[[f64; 2]]) -> f64 {
    let mut loops = Vec::new();
    split_loops(distinct(points, true), &mut loops);
    loops.iter().map(|l| shoelace(l)).sum()
}

/// Splits a closed ring at its first crossing until every loop is simple, into `out`. Each
/// split leaves both loops with fewer points, so it ends.
fn split_loops(points: Vec<[f64; 2]>, out: &mut Vec<Vec<[f64; 2]>>) {
    match first_crossing(&points, true) {
        None => out.push(points),
        Some((i, j, x)) => {
            let mut outer: Vec<[f64; 2]> = points[..=i].to_vec();
            outer.push(x);
            outer.extend_from_slice(&points[j + 1..]);
            let mut inner = vec![x];
            inner.extend_from_slice(&points[i + 1..=j]);
            split_loops(distinct(&outer, true), out);
            split_loops(distinct(&inner, true), out);
        }
    }
}

/// The first pair of non-adjacent segments `(i, j)` with `i < j` that meet, and where.
/// Segment `k` runs from point `k` to point `k + 1` (wrapping when closed).
fn first_crossing(points: &[[f64; 2]], closed: bool) -> Option<(usize, usize, [f64; 2])> {
    let n = points.len();
    let count = edge_count(n, closed);
    let seg = |k: usize| (points[k], points[(k + 1) % n]);
    for i in 0..count {
        for j in i + 2..count {
            if closed && i == 0 && j == count - 1 {
                continue;
            }
            let ((a, b), (c, d)) = (seg(i), seg(j));
            if let Some(x) = segment_hit(a, b, c, d) {
                return Some((i, j, x));
            }
        }
    }
    None
}

/// Where segments `ab` and `cd` meet, ends included; `None` when parallel.
fn segment_hit(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<[f64; 2]> {
    let (r, s, q) = (sub(b, a), sub(d, c), sub(c, a));
    let denom = cross(r, s);
    if denom == 0.0 {
        return None;
    }
    let (t, u) = (cross(q, s) / denom, cross(q, r) / denom);
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then(|| [a[0] + t * r[0], a[1] + t * r[1]])
}
