//! The solver: every point's x and y and every circle's radius are unknowns (less the points
//! pinned), each constraint gives one or two residuals that are 0 when it holds, and damped
//! Gauss-Newton (Levenberg-Marquardt) drives them all to 0.
//!
//! Each step is the smallest change of the unknowns that the linearised constraints ask for
//! (`dx = -Jᵀ (J Jᵀ + λI)⁻¹ r`), so an under-constrained sketch moves as little as it can: what
//! no constraint holds stays where it is. The Jacobian is taken by central differences, one
//! constraint's own unknowns at a time, and the sketch is split into independent groups of
//! constraints (connected through shared unknowns) solved one by one, so 200 constraints spread
//! over a drawing are many small solves. A group that cannot reach 0 (conflicting constraints)
//! fails the whole solve and leaves the sketch as it was.

use nalgebra::{DMatrix, DVector};

use crate::model::{Constraint, Curve, CurveId, PointId, Sketch};

/// Most Gauss-Newton steps per group.
const MAX_STEPS: usize = 100;
/// Damping bounds: below it a step is plain Gauss-Newton; above it the group is given up.
const MIN_DAMPING: f64 = 1e-12;
const MAX_DAMPING: f64 = 1e10;

/// How a solve went: the most steps any group took, and the largest residual left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solved {
    pub iterations: usize,
    pub residual: f64,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SolveError {
    /// The constraints cannot all hold together (or with the points pinned): the largest
    /// residual left, in drawing units.
    #[error("The constraints can't all hold together (they conflict, or fix what moved)")]
    Conflict { residual: f64 },
    /// A constraint names a point or curve the sketch has not, or a curve of the wrong kind.
    #[error("{0}")]
    Invalid(String),
}

/// A circle's radius as unknowns see it: its own unknown, or an arc's start's distance from its
/// centre.
#[derive(Debug, Clone, Copy)]
enum Radius {
    Var(usize),
    Arc { centre: PointId, start: PointId },
}

/// One residual group of rows over the unknowns, as the solver works it out.
#[derive(Debug, Clone, Copy)]
enum Row {
    Coincident(PointId, PointId),
    Fixed(PointId, [f64; 2]),
    Horizontal(PointId, PointId),
    Vertical(PointId, PointId),
    Parallel([PointId; 4]),
    Perpendicular([PointId; 4]),
    Collinear([PointId; 4]),
    Concentric(PointId, PointId),
    /// Line `a`-`b`, circle round `centre`: signed distance times `side` equals the radius.
    LineTangent {
        a: PointId,
        b: PointId,
        centre: PointId,
        radius: Radius,
        side: f64,
    },
    /// Two circles: centres apart by the radii's sum (`outside`) or difference.
    CircleTangent {
        c1: PointId,
        r1: Radius,
        c2: PointId,
        r2: Radius,
        outside: bool,
    },
    EqualLength([PointId; 4]),
    EqualRadius(Radius, Radius),
    Symmetric {
        p: PointId,
        q: PointId,
        a: PointId,
        b: PointId,
    },
    Distance {
        a: PointId,
        b: PointId,
        axis: Option<([f64; 2], f64)>,
        value: f64,
    },
    Angle {
        lines: [PointId; 4],
        sign: f64,
        value: f64,
    },
    RadiusIs(Radius, f64),
    /// An arc's end stays on its circle.
    ArcEnds {
        centre: PointId,
        start: PointId,
        end: PointId,
    },
}

impl Row {
    fn rows(&self) -> usize {
        match self {
            Row::Coincident(..)
            | Row::Fixed(..)
            | Row::Collinear(..)
            | Row::Concentric(..)
            | Row::Symmetric { .. } => 2,
            _ => 1,
        }
    }

    /// The unknowns it reads (some maybe pinned), each once.
    fn vars(&self, np: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut point = |p: PointId, out: &mut Vec<usize>| {
            for v in [2 * p, 2 * p + 1] {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        };
        let radius = |r: &Radius,
                      out: &mut Vec<usize>,
                      point: &mut dyn FnMut(PointId, &mut Vec<usize>)| match *r
        {
            Radius::Var(k) => {
                if !out.contains(&(2 * np + k)) {
                    out.push(2 * np + k);
                }
            }
            Radius::Arc { centre, start } => {
                point(centre, out);
                point(start, out);
            }
        };
        match self {
            Row::Coincident(p, q)
            | Row::Horizontal(p, q)
            | Row::Vertical(p, q)
            | Row::Concentric(p, q) => {
                point(*p, &mut out);
                point(*q, &mut out);
            }
            Row::Fixed(p, _) => point(*p, &mut out),
            Row::Parallel(ps)
            | Row::Perpendicular(ps)
            | Row::Collinear(ps)
            | Row::EqualLength(ps)
            | Row::Angle { lines: ps, .. } => ps.iter().for_each(|p| point(*p, &mut out)),
            Row::LineTangent {
                a,
                b,
                centre,
                radius: r,
                ..
            } => {
                for p in [*a, *b, *centre] {
                    point(p, &mut out);
                }
                radius(r, &mut out, &mut point);
            }
            Row::CircleTangent { c1, r1, c2, r2, .. } => {
                point(*c1, &mut out);
                point(*c2, &mut out);
                radius(r1, &mut out, &mut point);
                radius(r2, &mut out, &mut point);
            }
            Row::EqualRadius(r1, r2) => {
                radius(r1, &mut out, &mut point);
                radius(r2, &mut out, &mut point);
            }
            Row::Symmetric { p, q, a, b } => {
                for p in [*p, *q, *a, *b] {
                    point(p, &mut out);
                }
            }
            Row::Distance { a, b, .. } => {
                point(*a, &mut out);
                point(*b, &mut out);
            }
            Row::RadiusIs(r, _) => radius(r, &mut out, &mut point),
            Row::ArcEnds { centre, start, end } => {
                for p in [*centre, *start, *end] {
                    point(p, &mut out);
                }
            }
        }
        out
    }

    /// Its residuals at `x` into `out` (`rows()` of them); `scale` turns angles into lengths.
    fn eval(&self, x: &[f64], np: usize, scale: f64, out: &mut [f64]) {
        let p = |i: PointId| [x[2 * i], x[2 * i + 1]];
        let r = |r: &Radius| match *r {
            Radius::Var(k) => x[2 * np + k],
            Radius::Arc { centre, start } => len(sub(p(start), p(centre))),
        };
        let dir = |a: PointId, b: PointId| unit(sub(p(b), p(a)));
        match self {
            Row::Coincident(a, b) => {
                let d = sub(p(*a), p(*b));
                out[0] = d[0];
                out[1] = d[1];
            }
            Row::Fixed(a, at) => {
                let d = sub(p(*a), *at);
                out[0] = d[0];
                out[1] = d[1];
            }
            Row::Horizontal(a, b) => out[0] = p(*b)[1] - p(*a)[1],
            Row::Vertical(a, b) => out[0] = p(*b)[0] - p(*a)[0],
            Row::Parallel([a, b, c, d]) => out[0] = scale * cross(dir(*a, *b), dir(*c, *d)),
            Row::Perpendicular([a, b, c, d]) => out[0] = scale * dot(dir(*a, *b), dir(*c, *d)),
            Row::Collinear([a, b, c, d]) => {
                let u = dir(*a, *b);
                out[0] = cross(u, sub(p(*c), p(*a)));
                out[1] = cross(u, sub(p(*d), p(*a)));
            }
            Row::Concentric(a, b) => {
                let d = sub(p(*a), p(*b));
                out[0] = d[0];
                out[1] = d[1];
            }
            Row::LineTangent {
                a,
                b,
                centre,
                radius,
                side,
            } => {
                out[0] = side * cross(dir(*a, *b), sub(p(*centre), p(*a))) - r(radius);
            }
            Row::CircleTangent {
                c1,
                r1,
                c2,
                r2,
                outside,
            } => {
                let apart = len(sub(p(*c2), p(*c1)));
                let (r1, r2) = (r(r1), r(r2));
                out[0] = if *outside {
                    apart - (r1 + r2)
                } else {
                    apart - (r1 - r2).abs()
                };
            }
            Row::EqualLength([a, b, c, d]) => {
                out[0] = len(sub(p(*b), p(*a))) - len(sub(p(*d), p(*c)));
            }
            Row::EqualRadius(r1, r2) => out[0] = r(r1) - r(r2),
            Row::Symmetric {
                p: a,
                q: b,
                a: la,
                b: lb,
            } => {
                let u = dir(*la, *lb);
                let (pa, pb) = (p(*a), p(*b));
                let mid = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
                out[0] = cross(u, sub(mid, p(*la)));
                out[1] = dot(u, sub(pb, pa));
            }
            Row::Distance { a, b, axis, value } => {
                let d = sub(p(*b), p(*a));
                out[0] = match axis {
                    Some((axis, sign)) => sign * dot(d, *axis) - value,
                    None => len(d) - value,
                };
            }
            Row::Angle {
                lines: [a, b, c, d],
                sign,
                value,
            } => {
                let (u, v) = (dir(*a, *b), dir(*c, *d));
                out[0] = scale * (sign * cross(u, v).atan2(dot(u, v)) - value);
            }
            Row::RadiusIs(radius, value) => out[0] = r(radius) - value,
            Row::ArcEnds { centre, start, end } => {
                out[0] = len(sub(p(*end), p(*centre))) - len(sub(p(*start), p(*centre)));
            }
        }
    }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn len(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}
fn unit(a: [f64; 2]) -> [f64; 2] {
    let n = len(a);
    if n > 0.0 {
        [a[0] / n, a[1] / n]
    } else {
        [0.0, 0.0]
    }
}

/// `+1` or `-1` as `v` is now (0 counts as `+1`).
fn sign(v: f64) -> f64 {
    if v < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// The sketch's constraints (and its arcs' own) as residual rows, checked against its points
/// and curves; sides and turns are taken as they are now.
fn rows(s: &Sketch) -> Result<Vec<Row>, SolveError> {
    let bad = |why: String| SolveError::Invalid(why);
    let point = |p: PointId| {
        if p < s.points.len() {
            Ok(p)
        } else {
            Err(bad(format!("There is no point {p}")))
        }
    };
    let curve = |c: CurveId| {
        s.curves
            .get(c)
            .copied()
            .ok_or_else(|| bad(format!("There is no curve {c}")))
    };
    let line = |c: CurveId| match curve(c)? {
        Curve::Line { a, b } => Ok((point(a)?, point(b)?)),
        _ => Err(bad("This constraint takes lines".into())),
    };
    let round = |c: CurveId| match curve(c)? {
        Curve::Circle { centre, radius } if radius < s.radii.len() => {
            Ok((point(centre)?, Radius::Var(radius)))
        }
        Curve::Arc { centre, start, end } => {
            point(end)?;
            Ok((
                point(centre)?,
                Radius::Arc {
                    centre,
                    start: point(start)?,
                },
            ))
        }
        _ => Err(bad("This constraint takes circles and arcs".into())),
    };
    let at = |p: PointId| s.points[p];
    let radius_now = |r: Radius| match r {
        Radius::Var(k) => s.radii[k],
        Radius::Arc { centre, start } => len(sub(at(start), at(centre))),
    };
    let mut out = Vec::new();
    for c in &s.curves {
        if let Curve::Arc { centre, start, end } = *c {
            out.push(Row::ArcEnds {
                centre: point(centre)?,
                start: point(start)?,
                end: point(end)?,
            });
        }
    }
    for c in &s.constraints {
        out.push(match *c {
            Constraint::Coincident(a, b) => Row::Coincident(point(a)?, point(b)?),
            Constraint::Fixed(a, p) => Row::Fixed(point(a)?, p),
            Constraint::Horizontal(l) => {
                let (a, b) = line(l)?;
                Row::Horizontal(a, b)
            }
            Constraint::HorizontalPoints(a, b) => Row::Horizontal(point(a)?, point(b)?),
            Constraint::Vertical(l) => {
                let (a, b) = line(l)?;
                Row::Vertical(a, b)
            }
            Constraint::VerticalPoints(a, b) => Row::Vertical(point(a)?, point(b)?),
            Constraint::Parallel(l1, l2) => {
                let ((a, b), (c, d)) = (line(l1)?, line(l2)?);
                Row::Parallel([a, b, c, d])
            }
            Constraint::Perpendicular(l1, l2) => {
                let ((a, b), (c, d)) = (line(l1)?, line(l2)?);
                Row::Perpendicular([a, b, c, d])
            }
            Constraint::Collinear(l1, l2) => {
                let ((a, b), (c, d)) = (line(l1)?, line(l2)?);
                Row::Collinear([a, b, c, d])
            }
            Constraint::Concentric(c1, c2) => Row::Concentric(round(c1)?.0, round(c2)?.0),
            Constraint::Tangent(c1, c2) => match (line(c1), line(c2)) {
                (Ok(_), Ok(_)) => return Err(bad("Two lines can't be tangent".into())),
                (Ok((a, b)), Err(_)) | (Err(_), Ok((a, b))) => {
                    let other = if line(c1).is_ok() { c2 } else { c1 };
                    let (centre, radius) = round(other)?;
                    let side = sign(cross(unit(sub(at(b), at(a))), sub(at(centre), at(a))));
                    Row::LineTangent {
                        a,
                        b,
                        centre,
                        radius,
                        side,
                    }
                }
                (Err(_), Err(_)) => {
                    let ((c1, r1), (c2, r2)) = (round(c1)?, round(c2)?);
                    let apart = len(sub(at(c2), at(c1)));
                    let (n1, n2) = (radius_now(r1), radius_now(r2));
                    let outside = (apart - (n1 + n2)).abs() <= (apart - (n1 - n2).abs()).abs();
                    Row::CircleTangent {
                        c1,
                        r1,
                        c2,
                        r2,
                        outside,
                    }
                }
            },
            Constraint::Equal(c1, c2) => match (line(c1), line(c2)) {
                (Ok((a, b)), Ok((c, d))) => Row::EqualLength([a, b, c, d]),
                (Err(_), Err(_)) => Row::EqualRadius(round(c1)?.1, round(c2)?.1),
                _ => return Err(bad("Equal takes two lines, or two circles or arcs".into())),
            },
            Constraint::Symmetric(p, q, l) => {
                let (a, b) = line(l)?;
                Row::Symmetric {
                    p: point(p)?,
                    q: point(q)?,
                    a,
                    b,
                }
            }
            Constraint::Distance { a, b, axis, value } => {
                let (a, b) = (point(a)?, point(b)?);
                let axis = match axis {
                    Some(u) => {
                        let u = unit(u);
                        if u == [0.0, 0.0] {
                            return Err(bad("A linear distance needs a direction".into()));
                        }
                        Some((u, sign(dot(sub(at(b), at(a)), u))))
                    }
                    None => None,
                };
                Row::Distance { a, b, axis, value }
            }
            Constraint::Angle {
                first,
                second,
                value,
            } => {
                let ((a, b), (c, d)) = (line(first)?, line(second)?);
                let (u, v) = (unit(sub(at(b), at(a))), unit(sub(at(d), at(c))));
                Row::Angle {
                    lines: [a, b, c, d],
                    sign: sign(cross(u, v).atan2(dot(u, v))),
                    value,
                }
            }
            Constraint::Radius { curve, value } => Row::RadiusIs(round(curve)?.1, value),
        });
    }
    Ok(out)
}

/// Evaluates the same residuals as the solver without moving any point. Length residuals are
/// in sketch units; angle residuals are multiplied by `angle_scale` (use 1 for radians and
/// dimensionless parallel/perpendicular residuals). Arc closure residuals precede constraints.
pub fn residuals(sketch: &Sketch, angle_scale: f64) -> Result<Vec<f64>, SolveError> {
    if !angle_scale.is_finite()
        || angle_scale <= 0.0
        || sketch.points.iter().flatten().any(|v| !v.is_finite())
        || sketch.radii.iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err(SolveError::Invalid(
            "Residuals need finite points, positive radii and a positive angle scale".into(),
        ));
    }
    let rows = rows(sketch)?;
    let x: Vec<f64> = sketch
        .points
        .iter()
        .flatten()
        .copied()
        .chain(sketch.radii.iter().copied())
        .collect();
    let mut out = Vec::new();
    for row in rows {
        let start = out.len();
        out.resize(start + row.rows(), 0.0);
        row.eval(&x, sketch.points.len(), angle_scale, &mut out[start..]);
    }
    if out.iter().any(|v| !v.is_finite()) {
        return Err(SolveError::Invalid("The residual is not finite".into()));
    }
    Ok(out)
}

/// Union-find over the unknowns.
fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

/// Solves `sketch` in place, the points `pinned` held where they are. On failure the sketch is
/// left as it was.
///
/// The work is done on a copy moved to its middle and scaled to about one unit across, so a
/// detail millimetres wide on a site grid billions of units from the origin solves as well as one
/// at the origin (finite differences, tolerances). Points the solve leaves where they were are
/// given back exactly as they came, never rounded through the move and back.
pub fn solve(sketch: &mut Sketch, pinned: &[PointId]) -> Result<Solved, SolveError> {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &sketch.points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    if sketch.points.is_empty() {
        (lo, hi) = ([0.0; 2], [0.0; 2]);
    }
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let biggest_radius = sketch.radii.iter().fold(0.0f64, |m, r| m.max(r.abs()));
    let size = len(sub(hi, lo)).max(biggest_radius);
    let unit = if size > 0.0 && size.is_finite() {
        size
    } else {
        1.0
    };
    let there = |p: [f64; 2]| [(p[0] - centre[0]) / unit, (p[1] - centre[1]) / unit];
    let mut n = sketch.clone();
    for p in &mut n.points {
        *p = there(*p);
    }
    for r in &mut n.radii {
        *r /= unit;
    }
    for c in &mut n.constraints {
        match c {
            Constraint::Fixed(_, at) => *at = there(*at),
            Constraint::Distance { value, .. } | Constraint::Radius { value, .. } => *value /= unit,
            _ => {}
        }
    }
    let before = n.clone();
    let mut solved = solve_here(&mut n, pinned)?;
    for (i, p) in sketch.points.iter_mut().enumerate() {
        if n.points[i] != before.points[i] {
            *p = [
                n.points[i][0] * unit + centre[0],
                n.points[i][1] * unit + centre[1],
            ];
        }
    }
    for (i, r) in sketch.radii.iter_mut().enumerate() {
        if n.radii[i] != before.radii[i] {
            *r = n.radii[i] * unit;
        }
    }
    solved.residual *= unit;
    Ok(solved)
}

/// `solve` on a sketch about one unit across round the origin.
fn solve_here(sketch: &mut Sketch, pinned: &[PointId]) -> Result<Solved, SolveError> {
    let rows = rows(sketch)?;
    let np = sketch.points.len();
    let mut x: Vec<f64> = sketch.points.iter().flat_map(|p| [p[0], p[1]]).collect();
    x.extend(&sketch.radii);
    let n = x.len();
    let mut free = vec![true; n];
    for &p in pinned {
        if p < np {
            free[2 * p] = false;
            free[2 * p + 1] = false;
        }
    }
    // Angles count as lengths this long, and residuals this small as 0.
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &sketch.points {
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let extent = if np > 0 { len(sub(hi, lo)) } else { 0.0 };
    let scale = extent.max(1.0);
    let biggest = sketch
        .points
        .iter()
        .map(|p| p[0].abs().max(p[1].abs()))
        .fold(scale, f64::max);
    let tol = 1e-10 * biggest;

    let vars: Vec<Vec<usize>> = rows.iter().map(|r| r.vars(np)).collect();
    // Groups: rows joined through the free unknowns they share.
    let mut parent: Vec<usize> = (0..n).collect();
    for vs in &vars {
        let mut own = vs.iter().copied().filter(|&v| free[v]);
        if let Some(first) = own.next() {
            for v in own {
                let (a, b) = (root(&mut parent, first), root(&mut parent, v));
                parent[a] = b;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    let mut worst = 0.0f64;
    let mut buf = [0.0; 2];
    for (i, vs) in vars.iter().enumerate() {
        match vs.iter().copied().find(|&v| free[v]) {
            Some(v) => groups.entry(root(&mut parent, v)).or_default().push(i),
            None => {
                // Nothing can move: it holds already, or never will.
                let r = &rows[i];
                r.eval(&x, np, scale, &mut buf[..r.rows()]);
                worst = buf[..r.rows()].iter().fold(worst, |w, v| w.max(v.abs()));
            }
        }
    }
    if worst > tol {
        return Err(SolveError::Conflict { residual: worst });
    }
    let mut iterations = 0;
    for members in groups.values() {
        let (steps, left) = solve_group(&rows, &vars, members, &free, &mut x, np, scale, tol)?;
        iterations = iterations.max(steps);
        worst = worst.max(left);
    }
    for (i, p) in sketch.points.iter_mut().enumerate() {
        *p = [x[2 * i], x[2 * i + 1]];
    }
    sketch.radii.copy_from_slice(&x[2 * np..]);
    Ok(Solved {
        iterations,
        residual: worst,
    })
}

/// Levenberg-Marquardt over one group of rows; `x` changes only when it succeeds. The steps
/// taken and the largest residual left.
#[allow(clippy::too_many_arguments)]
fn solve_group(
    rows: &[Row],
    vars: &[Vec<usize>],
    members: &[usize],
    free: &[bool],
    x: &mut [f64],
    np: usize,
    scale: f64,
    tol: f64,
) -> Result<(usize, f64), SolveError> {
    // The group's free unknowns, as columns.
    let mut cols: Vec<usize> = members
        .iter()
        .flat_map(|&i| vars[i].iter().copied().filter(|&v| free[v]))
        .collect();
    cols.sort_unstable();
    cols.dedup();
    let col_of = |v: usize| cols.binary_search(&v).ok();
    let offsets: Vec<usize> = members
        .iter()
        .scan(0, |at, &i| {
            let here = *at;
            *at += rows[i].rows();
            Some(here)
        })
        .collect();
    let m = members
        .last()
        .map_or(0, |&i| offsets[offsets.len() - 1] + rows[i].rows());
    let residuals = |x: &[f64]| {
        let mut r = DVector::zeros(m);
        for (k, &i) in members.iter().enumerate() {
            let n = rows[i].rows();
            rows[i].eval(
                x,
                np,
                scale,
                &mut r.as_mut_slice()[offsets[k]..offsets[k] + n],
            );
        }
        r
    };
    let mut work = x.to_vec();
    let mut r = residuals(&work);
    let mut damping = MIN_DAMPING;
    let mut steps = 0;
    while r.amax() > tol {
        if steps == MAX_STEPS {
            return Err(SolveError::Conflict { residual: r.amax() });
        }
        steps += 1;
        // The Jacobian, by columns: (row, value) pairs, from each row's own unknowns.
        let mut by_col: Vec<Vec<(usize, f64)>> = vec![Vec::new(); cols.len()];
        let (mut plus, mut minus) = ([0.0; 2], [0.0; 2]);
        for (k, &i) in members.iter().enumerate() {
            let row = &rows[i];
            let n = row.rows();
            for &v in &vars[i] {
                let Some(c) = col_of(v) else { continue };
                let h = 1e-7 * work[v].abs().max(1.0);
                let keep = work[v];
                work[v] = keep + h;
                row.eval(&work, np, scale, &mut plus[..n]);
                work[v] = keep - h;
                row.eval(&work, np, scale, &mut minus[..n]);
                work[v] = keep;
                for j in 0..n {
                    let d = (plus[j] - minus[j]) / (2.0 * h);
                    if d != 0.0 {
                        by_col[c].push((offsets[k] + j, d));
                    }
                }
            }
        }
        // J Jᵀ, from each column's pairs.
        let mut jjt = DMatrix::<f64>::zeros(m, m);
        for entries in &by_col {
            for &(a, va) in entries {
                for &(b, vb) in entries {
                    jjt[(a, b)] += va * vb;
                }
            }
        }
        let biggest = (0..m).map(|i| jjt[(i, i)]).fold(1.0, f64::max);
        let before = r.norm_squared();
        loop {
            let mut damped = jjt.clone();
            for i in 0..m {
                damped[(i, i)] += damping * biggest;
            }
            let Some(chol) = damped.cholesky() else {
                damping *= 10.0;
                if damping > MAX_DAMPING {
                    return Err(SolveError::Conflict { residual: r.amax() });
                }
                continue;
            };
            let y = chol.solve(&r);
            let mut trial = work.clone();
            for (c, entries) in by_col.iter().enumerate() {
                let step: f64 = entries.iter().map(|&(row, v)| v * y[row]).sum();
                trial[cols[c]] -= step;
            }
            let after = residuals(&trial);
            if after.norm_squared() < before {
                work = trial;
                r = after;
                damping = (damping * 0.1).max(MIN_DAMPING);
                break;
            }
            damping *= 10.0;
            if damping > MAX_DAMPING {
                return Err(SolveError::Conflict { residual: r.amax() });
            }
        }
    }
    let left = r.amax();
    x.copy_from_slice(&work);
    Ok((steps, left))
}
