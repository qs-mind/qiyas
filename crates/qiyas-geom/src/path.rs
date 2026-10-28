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

    /// Enclosed even-odd area, 0.0 when open. A bowtie counts both lobes;
    /// repeated coverage cancels and pinched-in holes are subtracted.
    pub fn area(&self) -> f64 {
        if self.closed {
            if self.self_intersects() {
                crate::topology::resolve(&distinct(&self.points, true))
                    .map(|regions| regions.iter().map(crate::walls::Region::area).sum())
                    .unwrap_or(f64::NAN)
            } else {
                crate::predicates::ring_area(&distinct(&self.points, true)).abs()
            }
        } else {
            0.0
        }
    }

    /// A closed path split at its crossings into simple loops (itself when it has none); none
    /// when open. A ring that touches itself round a pinched-in hole gives the hole as a loop.
    /// Clipping failures are errors, never an empty measured region.
    pub fn loops(&self) -> Result<Vec<Vec<[f64; 2]>>, &'static str> {
        if !self.closed {
            return Ok(Vec::new());
        }
        if !self.self_intersects() {
            return Ok(vec![distinct(&self.points, true)]);
        }
        crate::topology::resolve(&distinct(&self.points, true))
            .map(|regions| crate::topology::rings(&regions))
    }

    /// True when non-adjacent segments meet, including collinear overlaps.
    pub fn self_intersects(&self) -> bool {
        let points = distinct(&self.points, self.closed);
        let count = edge_count(points.len(), self.closed);
        (0..count).any(|i| {
            (i + 2..count).any(|j| {
                !(self.closed && i == 0 && j == count - 1)
                    && crate::predicates::intersects(
                        points[i],
                        points[(i + 1) % points.len()],
                        points[j],
                        points[(j + 1) % points.len()],
                    )
            })
        })
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
