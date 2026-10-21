//! Walls from centrelines: each run widened to its thickness, runs that meet end to end joined
//! with mitred corners, T and X junctions merged, openings cut through, and the result as
//! regions (an outline and its holes: the rooms a ring of walls closes). Areas and lengths come
//! with it, for the take-off.
//!
//! 1. Ends within `tol` are one point; two runs meeting end to end at a point no other run
//!    reaches are one chain (mitred there; a closed chain where it closes).
//! 2. Each chain becomes simple polygons: its left and right offsets, mitred at every corner
//!    (limited to four offset widths), split into overlapping pieces when it closes on itself.
//! 3. Where three or more ends meet, the convex hull of their end corners fills the notches.
//! 4. Everything is united into regions (`union_into`), then each opening's rectangle, 10% wider
//!    than the wall on each face, is cut out.

use cavalier_contours::polyline::{
    BooleanOp, PlineCreation, PlineSource, PlineSourceMut, PlineVertex, Polyline,
};

/// One run of wall. Curves are represented by points along a polyline approximation.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
}

/// The side of the directed run occupied by the wall.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Justification {
    #[default]
    Centre,
    Left,
    Right,
}

/// An opening: in run `run`, its middle `at` along the run's centreline from its start, `width`
/// wide.
#[derive(Debug, Clone, PartialEq)]
pub struct Opening {
    pub run: usize,
    pub at: f64,
    pub width: f64,
}

/// A region: its outline (counter-clockwise) and holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub outer: Vec<[f64; 2]>,
    pub holes: Vec<Vec<[f64; 2]>>,
}

impl Region {
    pub fn area(&self) -> f64 {
        ring_area(&self.outer).abs() - self.holes.iter().map(|h| ring_area(h).abs()).sum::<f64>()
    }
}

/// Where an opening went: its middle, the run's direction there, its width and the wall's
/// thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub centre: [f64; 2],
    pub dir: [f64; 2],
    pub width: f64,
    pub thickness: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Walls {
    pub regions: Vec<Region>,
    /// The centrelines' length.
    pub centreline: f64,
    /// Plan area before and after the openings.
    pub gross_area: f64,
    pub net_area: f64,
    pub openings: Vec<Placed>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WallError {
    Bad(String),
}

impl std::fmt::Display for WallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for WallError {}

fn bad(why: impl Into<String>) -> WallError {
    WallError::Bad(why.into())
}

/// The walls `runs` make at `thickness`, with `openings` cut out; points within `tol` are one.
pub fn build_walls(
    runs: &[Run],
    thickness: f64,
    openings: &[Opening],
    tol: f64,
) -> Result<Walls, WallError> {
    build_walls_justified(runs, thickness, openings, tol, Justification::Centre)
}

/// Builds walls on the centre, left or right of each directed run, in the input units.
/// Boolean operations use a local origin and one wall thickness as their unit, so survey
/// coordinates and drawings in millimetres have the same numerical accuracy.
pub fn build_walls_justified(
    runs: &[Run],
    thickness: f64,
    openings: &[Opening],
    tol: f64,
    justification: Justification,
) -> Result<Walls, WallError> {
    if thickness <= 0.0 || !thickness.is_finite() {
        return Err(bad("The wall thickness must be more than 0"));
    }
    if tol <= 0.0 || !tol.is_finite() {
        return Err(bad("The wall tolerance must be finite and more than 0"));
    }
    if runs.is_empty() {
        return Err(bad("Give at least one run of wall"));
    }
    for (i, r) in runs.iter().enumerate() {
        let need = if r.closed { 3 } else { 2 };
        if r.points.len() < need || r.points.iter().flatten().any(|c| !c.is_finite()) {
            return Err(bad(format!("Run {i} needs {need} finite points or more")));
        }
        if r.points.windows(2).any(|w| dist(w[0], w[1]) <= tol) {
            return Err(bad(format!("Run {i} has two points in the same place")));
        }
    }
    let origin = runs[0].points[0];
    let normalized: Vec<Run> = runs
        .iter()
        .map(|r| {
            let mut points: Vec<_> = r
                .points
                .iter()
                .map(|p| {
                    [
                        (p[0] - origin[0]) / thickness,
                        (p[1] - origin[1]) / thickness,
                    ]
                })
                .collect();
            let closed = r.closed
                || (points.len() >= 3 && dist(r.points[0], *r.points.last().expect("2+")) <= tol);
            if closed && dist(r.points[0], *r.points.last().expect("2+")) <= tol {
                points.pop();
            }
            Run { points, closed }
        })
        .collect();
    if normalized.iter().any(|r| r.closed && r.points.len() < 3) {
        return Err(bad("A closed run needs three distinct points"));
    }
    let openings: Vec<Opening> = openings
        .iter()
        .map(|o| Opening {
            run: o.run,
            at: o.at / thickness,
            width: o.width / thickness,
        })
        .collect();
    let mut walls = build_normalized(
        &normalized,
        &openings,
        (tol / thickness).max(1e-8),
        justification,
        thickness,
    )?;
    for region in &mut walls.regions {
        for point in region
            .outer
            .iter_mut()
            .chain(region.holes.iter_mut().flatten())
        {
            point[0] = point[0] * thickness + origin[0];
            point[1] = point[1] * thickness + origin[1];
        }
    }
    for opening in &mut walls.openings {
        opening.centre[0] = opening.centre[0] * thickness + origin[0];
        opening.centre[1] = opening.centre[1] * thickness + origin[1];
        opening.width *= thickness;
        opening.thickness *= thickness;
    }
    walls.centreline *= thickness;
    walls.gross_area *= thickness * thickness;
    walls.net_area *= thickness * thickness;
    Ok(walls)
}

/// `unit`: one normalised unit in drawing units (the thickness), for messages.
fn build_normalized(
    runs: &[Run],
    openings: &[Opening],
    tol: f64,
    justification: Justification,
    unit: f64,
) -> Result<Walls, WallError> {
    check_overlaps(runs, tol, justification)?;
    check_openings(runs, openings, tol, justification, unit)?;
    let centreline: f64 = runs.iter().map(run_length).sum();
    let chains = join_runs(runs, tol, justification == Justification::Centre);
    let (left, right, shift) = match justification {
        Justification::Centre => (0.5, -0.5, 0.0),
        Justification::Left => (1.0, 0.0, 0.5),
        Justification::Right => (0.0, -1.0, -0.5),
    };
    let mut polygons: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut caps: Vec<EndCap> = Vec::new();
    for chain in &chains {
        let (pieces, ends) = strip(chain, left, right);
        polygons.extend(pieces);
        caps.extend(ends);
    }
    // Where three or more ends meet: the hull of their corners.
    let mut done = vec![false; caps.len()];
    for i in 0..caps.len() {
        if done[i] {
            continue;
        }
        let at = caps[i].0;
        let group: Vec<usize> = (i..caps.len())
            .filter(|&j| !done[j] && dist(caps[j].0, at) <= tol)
            .collect();
        for &j in &group {
            done[j] = true;
        }
        if group.len() >= 2 {
            let pts: Vec<[f64; 2]> = group
                .iter()
                .flat_map(|&j| [caps[j].1, caps[j].2])
                .chain([at])
                .collect();
            let hull = convex_hull(&pts);
            if hull.len() >= 3 {
                polygons.push(hull);
            }
        }
    }
    let mut regions: Vec<Region> = Vec::new();
    for p in &polygons {
        union_into(&mut regions, p);
    }
    let gross_area = regions.iter().map(Region::area).sum();
    let mut placed = Vec::new();
    for o in openings {
        let (point, dir) = point_along(&runs[o.run], o.at).expect("validated opening");
        let centre = [point[0] - dir[1] * shift, point[1] + dir[0] * shift];
        let cut = rectangle(centre, dir, o.width, 1.2);
        cut_from(&mut regions, &cut);
        placed.push(Placed {
            centre,
            dir,
            width: o.width,
            thickness: 1.0,
        });
    }
    let net_area = regions.iter().map(Region::area).sum();
    Ok(Walls {
        regions,
        centreline,
        gross_area,
        net_area,
        openings: placed,
    })
}

#[derive(Clone, Copy)]
struct Segment {
    run: usize,
    a: [f64; 2],
    b: [f64; 2],
    length: f64,
    from: f64,
}

fn segments(runs: &[Run]) -> Vec<Segment> {
    let mut result = Vec::new();
    for (run, r) in runs.iter().enumerate() {
        let mut from = 0.0;
        let count = r.points.len() - usize::from(!r.closed);
        for index in 0..count {
            let a = r.points[index];
            let b = r.points[(index + 1) % r.points.len()];
            let length = dist(a, b);
            result.push(Segment {
                run,
                a,
                b,
                length,
                from,
            });
            from += length;
        }
    }
    result
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn difference(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn check_overlaps(runs: &[Run], tol: f64, justification: Justification) -> Result<(), WallError> {
    let shift = match justification {
        Justification::Centre => 0.0,
        Justification::Left => 0.5,
        Justification::Right => -0.5,
    };
    let segments: Vec<_> = segments(runs)
        .into_iter()
        .map(|mut segment| {
            let offset = [
                -(segment.b[1] - segment.a[1]) * shift / segment.length,
                (segment.b[0] - segment.a[0]) * shift / segment.length,
            ];
            for point in [&mut segment.a, &mut segment.b] {
                point[0] += offset[0];
                point[1] += offset[1];
            }
            segment
        })
        .collect();
    for (index, first) in segments.iter().enumerate() {
        let delta = difference(first.b, first.a);
        let dir = [delta[0] / first.length, delta[1] / first.length];
        for second in &segments[index + 1..] {
            let other = difference(second.b, second.a);
            if cross(dir, other).abs() > second.length * 1e-9 {
                continue;
            }
            let offset = difference(second.a, first.a);
            // Parallel strips that overlap cannot be independently measured.
            if cross(dir, offset).abs() >= 1.0 - tol {
                continue;
            }
            let along = |p: [f64; 2]| {
                let d = difference(p, first.a);
                d[0] * dir[0] + d[1] * dir[1]
            };
            let a = along(second.a);
            let b = along(second.b);
            if first.length.min(a.max(b)) - 0.0_f64.max(a.min(b)) > tol {
                return Err(bad(format!(
                    "Runs {} and {} have overlapping parallel walls",
                    first.run, second.run
                )));
            }
        }
    }
    Ok(())
}

fn check_openings(
    runs: &[Run],
    openings: &[Opening],
    tol: f64,
    justification: Justification,
    unit: f64,
) -> Result<(), WallError> {
    let segments = segments(runs);
    let clearance = if justification == Justification::Centre {
        0.5
    } else {
        1.0
    };
    let fits = |index| where_it_fits(&segments, openings, index, tol, clearance, unit);
    for (index, opening) in openings.iter().enumerate() {
        let run = runs
            .get(opening.run)
            .ok_or_else(|| bad(format!("No run {} for an opening", opening.run)))?;
        if !opening.at.is_finite()
            || !opening.width.is_finite()
            || opening.width <= tol
            || opening.at - opening.width / 2.0 < -tol
            || opening.at + opening.width / 2.0 > run_length(run) + tol
        {
            return Err(bad(format!(
                "Opening {index} does not fit in run {} ({} long){}",
                opening.run,
                number(run_length(run) * unit),
                fits(index)
            )));
        }
        for other in &openings[..index] {
            if other.run == opening.run
                && (other.at - opening.at).abs() < (other.width + opening.width) / 2.0 - tol
            {
                return Err(bad(format!(
                    "Opening {index} overlaps another opening{}",
                    fits(index)
                )));
            }
        }
        let host = segments
            .iter()
            .find(|s| {
                s.run == opening.run
                    && opening.at >= s.from - tol
                    && opening.at <= s.from + s.length + tol
            })
            .expect("opening fits run");
        let host_delta = difference(host.b, host.a);
        let half = opening.width / 2.0;
        for other in &segments {
            let other_delta = difference(other.b, other.a);
            if cross(host_delta, other_delta).abs() <= host.length * other.length * 1e-9 {
                continue;
            }
            if other.run == opening.run
                && other.from < opening.at + half - tol
                && other.from + other.length > opening.at - half + tol
            {
                return Err(bad(format!(
                    "Opening {index} crosses a wall corner{}",
                    fits(index)
                )));
            }
        }
        for (at, sine) in junctions(host, &segments, tol) {
            if (opening.at - at).abs() <= half + clearance / sine + tol {
                return Err(bad(format!(
                    "Opening {index} crosses a wall junction or corner at {} along run {}{}",
                    number(at * unit),
                    opening.run,
                    fits(index)
                )));
            }
        }
    }
    Ok(())
}

/// Where other walls cross or meet `host`: the distance along its run and the sine of the
/// angle between them.
fn junctions(host: &Segment, segments: &[Segment], tol: f64) -> Vec<(f64, f64)> {
    let host_delta = difference(host.b, host.a);
    segments
        .iter()
        .filter_map(|other| {
            let other_delta = difference(other.b, other.a);
            let determinant = cross(host_delta, other_delta);
            if determinant.abs() <= host.length * other.length * 1e-9 {
                return None;
            }
            let offset = difference(other.a, host.a);
            let along_host = cross(offset, other_delta) / determinant;
            let along_other = cross(offset, host_delta) / determinant;
            if along_host < -tol / host.length
                || along_host > 1.0 + tol / host.length
                || along_other < -tol / other.length
                || along_other > 1.0 + tol / other.length
            {
                return None;
            }
            let sine = determinant.abs() / (host.length * other.length);
            Some((host.from + along_host * host.length, sine))
        })
        .collect()
}

/// Where opening `index` could go on its run, for an error: the ranges its centre may take,
/// in drawing units along the run, clear of corners, junctions and the run's other openings
/// (the same rules as `check_openings`), or that it fits nowhere there. A junction's side of a
/// range is shown rounded inwards (touching its clearance is refused); a run's end and a
/// neighbouring opening may be touched.
fn where_it_fits(
    segments: &[Segment],
    openings: &[Opening],
    index: usize,
    tol: f64,
    clearance: f64,
    unit: f64,
) -> String {
    let opening = &openings[index];
    let half = opening.width / 2.0;
    // (from, to, whether each end may be touched), in normalised units along the run.
    let mut centres: Vec<(f64, bool, f64, bool)> = Vec::new();
    for host in segments.iter().filter(|s| s.run == opening.run) {
        let mut blocked: Vec<(f64, f64, bool)> = junctions(host, segments, tol)
            .into_iter()
            .map(|(at, sine)| {
                (
                    at - clearance / sine - tol,
                    at + clearance / sine + tol,
                    false,
                )
            })
            .chain(
                openings
                    .iter()
                    .enumerate()
                    .filter(|&(i, o)| i != index && o.run == opening.run)
                    .map(|(_, o)| (o.at - o.width / 2.0, o.at + o.width / 2.0, true)),
            )
            .collect();
        blocked.sort_by(|a, b| a.0.total_cmp(&b.0));
        let end = host.from + host.length;
        let (mut start, mut start_ok) = (host.from, true);
        let mut free = Vec::new();
        for (a, b, ok) in blocked {
            if a > start {
                free.push((start, start_ok, a.min(end), a >= end || ok));
            }
            if b > start {
                (start, start_ok) = (b, ok);
            }
            if start >= end {
                break;
            }
        }
        if start < end {
            free.push((start, start_ok, end, true));
        }
        centres.extend(
            free.into_iter()
                .map(|(a, a_ok, b, b_ok)| (a + half, a_ok, b - half, b_ok))
                .filter(|(a, _, b, _)| b - a >= -tol),
        );
    }
    let shown: Vec<String> = centres
        .iter()
        .filter_map(|&(a, a_ok, b, b_ok)| {
            let (a, b) = (a * unit * 1000.0, b * unit * 1000.0);
            let a = if a_ok { a.round() } else { a.floor() + 1.0 } / 1000.0;
            let b = if b_ok { b.round() } else { b.ceil() - 1.0 } / 1000.0;
            (a <= b).then(|| format!("{} to {}", number(a), number(b)))
        })
        .collect();
    if shown.is_empty() {
        return format!(
            "; an opening {} wide fits nowhere on run {}: make it narrower or put it on another run",
            number(opening.width * unit),
            opening.run
        );
    }
    format!(
        "; an opening {} wide fits on run {} with its `at` from {}",
        number(opening.width * unit),
        opening.run,
        shown.join(", or from ")
    )
}

/// A length for a message: at most 3 decimals, no trailing zeros.
fn number(x: f64) -> String {
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

/// A chain: points in order, closed or not.
#[derive(Debug, Clone)]
struct Chain {
    points: Vec<[f64; 2]>,
    closed: bool,
}

/// Runs joined end to end where exactly two ends meet (and no run passes through).
fn join_runs(runs: &[Run], tol: f64, reversible: bool) -> Vec<Chain> {
    let chains: Vec<Chain> = runs
        .iter()
        .map(|r| Chain {
            points: r.points.clone(),
            closed: r.closed,
        })
        .collect();
    let mut links = vec![[None; 2]; chains.len()];
    for (i, chain) in chains.iter().enumerate() {
        if chain.closed {
            continue;
        }
        for end in [false, true] {
            let p = end_of(chain, end);
            let meeting: Vec<_> = chains
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.closed)
                .flat_map(|(j, c)| {
                    [false, true]
                        .into_iter()
                        .filter(move |&e| dist(end_of(c, e), p) <= tol)
                        .map(move |e| (j, e))
                })
                .collect();
            if meeting.len() != 2 || passes_through(&chains, p, tol) {
                continue;
            }
            let (other, other_end) = meeting
                .into_iter()
                .find(|&v| v != (i, end))
                .expect("two ends");
            if other != i && (reversible || end != other_end) {
                links[i][usize::from(end)] = Some((other, other_end));
            }
        }
    }
    // Visit open chains from a free end first, then the remaining closed loops.
    let mut starts: Vec<_> = (0..chains.len()).collect();
    starts.sort_by_key(|&i| {
        if reversible {
            links[i].iter().all(Option::is_some)
        } else {
            links[i][0].is_some()
        }
    });
    let mut seen = vec![false; chains.len()];
    let mut result = Vec::new();
    for start in starts {
        if seen[start] {
            continue;
        }
        if chains[start].closed {
            seen[start] = true;
            result.push(chains[start].clone());
            continue;
        }
        let mut current = start;
        let mut entering = reversible && links[start][0].is_some() && links[start][1].is_none();
        let mut points = Vec::new();
        let mut closed = false;
        loop {
            seen[current] = true;
            let mut next = chains[current].points.clone();
            if entering {
                next.reverse();
            }
            points.extend(next.into_iter().skip(usize::from(!points.is_empty())));
            match links[current][usize::from(!entering)] {
                Some((other, _)) if other == start => {
                    points.pop();
                    closed = true;
                    break;
                }
                Some((other, end)) if !seen[other] => {
                    current = other;
                    entering = end;
                }
                _ => break,
            }
        }
        result.push(Chain { points, closed });
    }
    result
}

fn end_of(c: &Chain, last: bool) -> [f64; 2] {
    if last {
        *c.points.last().expect("2+")
    } else {
        c.points[0]
    }
}

/// Whether some chain passes through `p` (not at one of its ends).
fn passes_through(chains: &[Chain], p: [f64; 2], tol: f64) -> bool {
    chains.iter().any(|c| {
        let n = c.points.len();
        let segs = if c.closed { n } else { n - 1 };
        (0..segs).any(|k| {
            let (a, b) = (c.points[k], c.points[(k + 1) % n]);
            point_segment(p, a, b) <= tol
                && (c.closed || (dist(p, c.points[0]) > tol && dist(p, c.points[n - 1]) > tol))
        })
    })
}

/// The polygons between the two offsets, and the free ends' caps.
type EndCap = ([f64; 2], [f64; 2], [f64; 2]);
type Strip = (Vec<Vec<[f64; 2]>>, Vec<EndCap>);

fn strip(c: &Chain, left_offset: f64, right_offset: f64) -> Strip {
    let pts = &c.points;
    let n = pts.len();
    // The offset corners at each point: left and right.
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let prev = if i > 0 {
            Some(pts[i - 1])
        } else if c.closed {
            Some(pts[n - 1])
        } else {
            None
        };
        let next = if i + 1 < n {
            Some(pts[i + 1])
        } else if c.closed {
            Some(pts[0])
        } else {
            None
        };
        let (offset, _) = corner(prev, pts[i], next, 1.0);
        let delta = [offset[0] - pts[i][0], offset[1] - pts[i][1]];
        left.push([
            pts[i][0] + delta[0] * left_offset,
            pts[i][1] + delta[1] * left_offset,
        ]);
        right.push([
            pts[i][0] + delta[0] * right_offset,
            pts[i][1] + delta[1] * right_offset,
        ]);
    }
    let mut caps = Vec::new();
    if c.closed {
        // Overlap a full segment at each seam. Touching mitre edges alone can be
        // returned as separate polygons by the boolean library.
        let k = n / 2;
        let half = |from: usize, to: usize| -> Vec<[f64; 2]> {
            let idx: Vec<usize> = if from < to {
                (from..=to).collect()
            } else {
                (from..n).chain(0..=to).collect()
            };
            let mut ring: Vec<[f64; 2]> = idx.iter().map(|&i| left[i]).collect();
            ring.extend(idx.iter().rev().map(|&i| right[i]));
            ring
        };
        let pieces = if n == 3 {
            vec![half(0, 2), half(1, 0), half(2, 1)]
        } else {
            vec![half(0, k + 1), half(k, 1)]
        };
        return (pieces, caps);
    }
    caps.push((pts[0], left[0], right[0]));
    caps.push((pts[n - 1], left[n - 1], right[n - 1]));
    let mut ring: Vec<[f64; 2]> = left.clone();
    ring.extend(right.iter().rev());
    (vec![ring], caps)
}

/// The offset corners at `p`, with the mitre limited to four offset widths.
fn corner(
    prev: Option<[f64; 2]>,
    p: [f64; 2],
    next: Option<[f64; 2]>,
    h: f64,
) -> ([f64; 2], [f64; 2]) {
    let dir = |a: [f64; 2], b: [f64; 2]| {
        let l = dist(a, b);
        [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
    };
    let normal = |d: [f64; 2]| [-d[1], d[0]];
    match (prev, next) {
        (None, Some(n)) => {
            let nn = normal(dir(p, n));
            (
                [p[0] + nn[0] * h, p[1] + nn[1] * h],
                [p[0] - nn[0] * h, p[1] - nn[1] * h],
            )
        }
        (Some(q), None) => {
            let nn = normal(dir(q, p));
            (
                [p[0] + nn[0] * h, p[1] + nn[1] * h],
                [p[0] - nn[0] * h, p[1] - nn[1] * h],
            )
        }
        (Some(q), Some(n)) => {
            let (n1, n2) = (normal(dir(q, p)), normal(dir(p, n)));
            let m = [n1[0] + n2[0], n1[1] + n2[1]];
            let len = m[0].hypot(m[1]);
            if len < 1e-9 {
                // Turning straight back: the segment's own offset.
                return (
                    [p[0] + n1[0] * h, p[1] + n1[1] * h],
                    [p[0] - n1[0] * h, p[1] - n1[1] * h],
                );
            }
            let m = [m[0] / len, m[1] / len];
            let cos = (m[0] * n1[0] + m[1] * n1[1]).max(0.25);
            let k = h / cos;
            (
                [p[0] + m[0] * k, p[1] + m[1] * k],
                [p[0] - m[0] * k, p[1] - m[1] * k],
            )
        }
        (None, None) => (p, p),
    }
}

fn to_pline(ring: &[[f64; 2]]) -> Polyline<f64> {
    let mut pl = Polyline::with_capacity(ring.len(), true);
    // Counter-clockwise for the booleans.
    let ccw = ring_area(ring) > 0.0;
    let mut pts: Vec<[f64; 2]> = ring.to_vec();
    if !ccw {
        pts.reverse();
    }
    for p in pts {
        pl.add(p[0], p[1], 0.0);
    }
    pl
}

fn from_pline(pl: &Polyline<f64>) -> Vec<[f64; 2]> {
    (0..pl.vertex_count())
        .map(|i| {
            let v: PlineVertex<f64> = pl.at(i);
            [v.x, v.y]
        })
        .collect()
}

/// Unites the simple polygon `p` into `regions`.
fn union_into(regions: &mut Vec<Region>, p: &[[f64; 2]]) {
    let mut outer = to_pline(p);
    let mut holes: Vec<Polyline<f64>> = Vec::new();
    let mut keep = Vec::new();
    for r in regions.drain(..) {
        if !bounds_overlap(&r.outer, &from_pline(&outer)) {
            keep.push(r);
            continue;
        }
        // An isolated wall inside a room is a separate region, not another hole.
        if r.holes.iter().any(|hole| {
            matches!(
                to_pline(hole).boolean(&outer, BooleanOp::Or).result_info,
                cavalier_contours::polyline::BooleanResultInfo::Pline2InsidePline1
            )
        }) {
            keep.push(r);
            continue;
        }
        let ro = to_pline(&r.outer);
        let result = ro.boolean(&outer, BooleanOp::Or);
        let merged = result.pos_plines.len() == 1
            && !matches!(
                result.result_info,
                cavalier_contours::polyline::BooleanResultInfo::Disjoint
            );
        if !merged {
            keep.push(r);
            continue;
        }
        outer = result.pos_plines[0].pline.clone();
        holes.extend(result.neg_plines.into_iter().map(|n| n.pline));
        // The region's own holes, less what `p` covers.
        for hole in &r.holes {
            let h = to_pline(hole);
            let cut = h.boolean(&to_pline(p), BooleanOp::Not);
            holes.extend(cut.pos_plines.into_iter().map(|x| x.pline));
        }
    }
    keep.push(Region {
        outer: from_pline(&outer),
        holes: holes
            .iter()
            .map(from_pline)
            .filter(|h| ring_area(h).abs() > 1e-12)
            .collect(),
    });
    *regions = keep;
}

/// Cuts the simple polygon `cut` out of `regions`: each region's outline less the cut, then less
/// each of its holes (a cut that joins a hole to the outside leaves the hole part of the outline).
fn cut_from(regions: &mut Vec<Region>, cut: &[[f64; 2]]) {
    let c = to_pline(cut);
    let mut out = Vec::new();
    for r in regions.drain(..) {
        if !bounds_overlap(&r.outer, cut) {
            out.push(r);
            continue;
        }
        let result = to_pline(&r.outer).boolean(&c, BooleanOp::Not);
        let mut pieces: Vec<(Polyline<f64>, Vec<Polyline<f64>>)> = match result.result_info {
            // Untouched: the region as it was.
            cavalier_contours::polyline::BooleanResultInfo::Disjoint => {
                out.push(r);
                continue;
            }
            _ => result
                .pos_plines
                .into_iter()
                .map(|p| (p.pline, Vec::new()))
                .collect(),
        };
        for hole in &r.holes {
            let h = to_pline(hole);
            let mut next = Vec::new();
            for (piece, mut holes) in pieces {
                let res = piece.boolean(&h, BooleanOp::Not);
                match res.result_info {
                    cavalier_contours::polyline::BooleanResultInfo::Disjoint => {
                        next.push((piece, holes))
                    }
                    _ if !res.neg_plines.is_empty() => {
                        // Wholly inside: a hole of this piece.
                        holes.extend(res.neg_plines.into_iter().map(|n| n.pline));
                        for p in res.pos_plines {
                            next.push((p.pline, std::mem::take(&mut holes)));
                        }
                    }
                    // Joined to the outside: the pieces keep the holes that fall inside them.
                    _ => {
                        for p in res.pos_plines {
                            let ring = from_pline(&p.pline);
                            let mine = holes
                                .iter()
                                .filter(|hh| {
                                    hh.vertex_count() > 0 && inside([hh.at(0).x, hh.at(0).y], &ring)
                                })
                                .cloned()
                                .collect();
                            next.push((p.pline, mine));
                        }
                    }
                }
            }
            pieces = next;
        }
        for (piece, holes) in pieces {
            out.push(Region {
                outer: from_pline(&piece),
                holes: holes.iter().map(from_pline).collect(),
            });
        }
    }
    *regions = out;
}

fn bounds_overlap(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    let bounds = |ring: &[[f64; 2]]| {
        ring.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |bb, p| {
                [
                    bb[0].min(p[0]),
                    bb[1].min(p[1]),
                    bb[2].max(p[0]),
                    bb[3].max(p[1]),
                ]
            },
        )
    };
    let a = bounds(a);
    let b = bounds(b);
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn rectangle(c: [f64; 2], dir: [f64; 2], along: f64, across: f64) -> Vec<[f64; 2]> {
    let (a, b) = (along / 2.0, across / 2.0);
    let n = [-dir[1], dir[0]];
    [(-a, -b), (a, -b), (a, b), (-a, b)]
        .iter()
        .map(|&(s, t)| [c[0] + dir[0] * s + n[0] * t, c[1] + dir[1] * s + n[1] * t])
        .collect()
}

fn run_length(r: &Run) -> f64 {
    let n = r.points.len();
    let open: f64 = r.points.windows(2).map(|w| dist(w[0], w[1])).sum();
    if r.closed {
        open + dist(r.points[n - 1], r.points[0])
    } else {
        open
    }
}

/// The point `at` along a run from its start, and the run's direction there.
fn point_along(r: &Run, at: f64) -> Option<([f64; 2], [f64; 2])> {
    let n = r.points.len();
    let segs = if r.closed { n } else { n - 1 };
    let mut gone = 0.0;
    for k in 0..segs {
        let (a, b) = (r.points[k], r.points[(k + 1) % n]);
        let l = dist(a, b);
        if at <= gone + l + 1e-12 {
            let t = ((at - gone) / l).clamp(0.0, 1.0);
            let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
            return Some(([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t], d));
        }
        gone += l;
    }
    None
}

fn convex_hull(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup_by(|a, b| dist(*a, *b) < 1e-12);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut lower: Vec<[f64; 2]> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<[f64; 2]> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// Signed area (positive counter-clockwise).
pub fn ring_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    let Some(origin) = ring.first() else {
        return 0.0;
    };
    (0..n)
        .map(|i| {
            let (p, q) = (ring[i], ring[(i + 1) % n]);
            (p[0] - origin[0]) * (q[1] - origin[1]) - (q[0] - origin[0]) * (p[1] - origin[1])
        })
        .sum::<f64>()
        / 2.0
}

fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let n = ring.len();
    let mut c = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn point_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(points: &[[f64; 2]], closed: bool) -> Run {
        Run {
            points: points.to_vec(),
            closed,
        }
    }

    const T: f64 = 0.2;
    const TOL: f64 = 1e-6;

    #[test]
    fn a_straight_wall_is_its_length_times_its_thickness() {
        let w = build_walls(&[run(&[[0.0, 0.0], [5.0, 0.0]], false)], T, &[], TOL).unwrap();
        assert!((w.gross_area - 1.0).abs() < 1e-9, "{}", w.gross_area);
        assert_eq!(w.centreline, 5.0);
    }

    #[test]
    fn an_l_corner_is_mitred_whether_one_run_or_two() {
        // Centrelines 3 + 4 = 7 m; area = 7 * 0.2 (the mitre adds and takes away the same).
        let one = build_walls(
            &[run(&[[0.0, 3.0], [0.0, 0.0], [4.0, 0.0]], false)],
            T,
            &[],
            TOL,
        )
        .unwrap();
        let two = build_walls(
            &[
                run(&[[0.0, 3.0], [0.0, 0.0]], false),
                run(&[[0.0, 0.0], [4.0, 0.0]], false),
            ],
            T,
            &[],
            TOL,
        )
        .unwrap();
        for w in [one, two] {
            assert!((w.gross_area - 1.4).abs() < 1e-9, "{}", w.gross_area);
            assert_eq!(w.regions.len(), 1);
        }
    }

    #[test]
    fn a_t_junction_is_one_region() {
        // Through wall 10 m, stem 4 m to its centreline: 10*0.2 + (4 - 0.1)*0.2 = 2.78.
        let w = build_walls(
            &[
                run(&[[0.0, 0.0], [10.0, 0.0]], false),
                run(&[[5.0, 0.0], [5.0, 4.0]], false),
            ],
            T,
            &[],
            TOL,
        )
        .unwrap();
        assert_eq!(w.regions.len(), 1);
        assert!((w.gross_area - 2.78).abs() < 1e-9, "{}", w.gross_area);
    }

    #[test]
    fn an_x_junction_counts_the_overlap_once() {
        // Two 6 m runs overlap in one 0.2 x 0.2 square: 2.4 - 0.04 = 2.36 m².
        let walls = build_walls(
            &[
                run(&[[-3.0, 0.0], [3.0, 0.0]], false),
                run(&[[0.0, -3.0], [0.0, 3.0]], false),
            ],
            T,
            &[],
            TOL,
        )
        .unwrap();
        assert_eq!(walls.regions.len(), 1);
        assert!((walls.gross_area - 2.36).abs() < 1e-9);
        assert!((walls.centreline - 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_closed_room_has_a_hole_and_a_door_cuts_it() {
        // 5 x 4 on the centreline: outer 5.2 x 4.2, inner 4.8 x 3.8: 21.84 - 18.24 = 3.6.
        let room = run(&[[0.0, 0.0], [5.0, 0.0], [5.0, 4.0], [0.0, 4.0]], true);
        let w = build_walls(std::slice::from_ref(&room), T, &[], TOL).unwrap();
        assert_eq!(w.regions.len(), 1);
        assert_eq!(w.regions[0].holes.len(), 1);
        assert!((w.gross_area - 3.6).abs() < 1e-9, "{}", w.gross_area);
        let door = Opening {
            run: 0,
            at: 2.5,
            width: 0.9,
        };
        let w = build_walls(&[room], T, &[door], TOL).unwrap();
        assert!(
            (w.net_area - (3.6 - 0.9 * 0.2)).abs() < 1e-9,
            "{}",
            w.net_area
        );
        assert_eq!(w.regions.len(), 1);
        assert!(
            w.regions[0].holes.is_empty(),
            "The door connects the room to the outside"
        );
    }

    #[test]
    fn a_curve_uses_polyline_chords_and_mitred_offsets() {
        // A quarter circle of radius 5 m, approximated by 32 equal chords.
        // Every chord is 10 sin(pi/128) m; symmetric mitres conserve length x thickness.
        let points: Vec<_> = (0..=32)
            .map(|i| {
                let angle = f64::from(i) * std::f64::consts::FRAC_PI_2 / 32.0;
                [5.0 * angle.cos(), 5.0 * angle.sin()]
            })
            .collect();
        let walls = build_walls(&[run(&points, false)], T, &[], TOL).unwrap();
        let length = 320.0 * (std::f64::consts::PI / 128.0).sin();
        assert!((walls.centreline - length).abs() < 1e-9);
        assert!((walls.gross_area - length * T).abs() < 1e-9);
        assert_eq!(walls.regions.len(), 1);
    }

    #[test]
    fn survey_coordinates_preserve_room_and_opening_areas() {
        let origin = [500_000.0, 2_700_000.0];
        let points: Vec<_> = [[0.0, 0.0], [5.0, 0.0], [5.0, 4.0], [0.0, 4.0]]
            .iter()
            .map(|p| [p[0] + origin[0], p[1] + origin[1]])
            .collect();
        let walls = build_walls(
            &[run(&points, true)],
            T,
            &[Opening {
                run: 0,
                at: 2.5,
                width: 0.9,
            }],
            TOL,
        )
        .unwrap();
        assert!((walls.gross_area - 3.6).abs() < 1e-9);
        assert!((walls.net_area - 3.42).abs() < 1e-9);
        assert!((walls.regions.iter().map(Region::area).sum::<f64>() - 3.42).abs() < 1e-8);
        assert_eq!(walls.openings[0].centre, [500_002.5, 2_700_000.0]);
    }

    #[test]
    fn an_opening_crossing_a_junction_or_corner_is_refused() {
        let through = run(&[[0.0, 0.0], [10.0, 0.0]], false);
        for branch in [
            run(&[[5.0, 0.0], [5.0, 4.0]], false),
            run(&[[5.0, -4.0], [5.0, 4.0]], false),
        ] {
            let result = build_walls(
                &[through.clone(), branch],
                T,
                &[Opening {
                    run: 0,
                    at: 5.0,
                    width: 0.9,
                }],
                TOL,
            );
            assert!(result.unwrap_err().to_string().contains("junction"));
        }
        let corner = run(&[[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], false);
        let result = build_walls(
            &[corner],
            T,
            &[Opening {
                run: 0,
                at: 3.0,
                width: 0.9,
            }],
            TOL,
        );
        assert!(result.unwrap_err().to_string().contains("corner"));
    }

    #[test]
    fn a_refused_opening_says_where_it_fits_and_the_answer_works() {
        let through = run(&[[0.0, 0.0], [10.0, 0.0]], false);
        let branch = run(&[[5.0, 0.0], [5.0, 4.0]], false);
        let at = |at| Opening {
            run: 0,
            at,
            width: 0.9,
        };
        let err = build_walls(&[through.clone(), branch.clone()], T, &[at(5.0)], TOL)
            .unwrap_err()
            .to_string();
        assert!(err.contains("junction or corner at 5 along run 0"), "{err}");
        assert!(
            err.contains("fits on run 0 with its `at` from 0.45 to 4.449, or from 5.551 to 9.55"),
            "{err}"
        );
        for ok in [0.45, 4.449, 5.551, 9.55] {
            if let Err(e) = build_walls(&[through.clone(), branch.clone()], T, &[at(ok)], TOL) {
                panic!("{ok}: {e}");
            }
        }
        let short = run(&[[0.0, 0.0], [0.5, 0.0]], false);
        let err = build_walls(&[short], T, &[at(0.25)], TOL)
            .unwrap_err()
            .to_string();
        assert!(err.contains("fits nowhere on run 0"), "{err}");
    }

    #[test]
    fn overlapping_openings_and_walls_are_refused() {
        let straight = run(&[[0.0, 0.0], [10.0, 0.0]], false);
        let result = build_walls(
            std::slice::from_ref(&straight),
            T,
            &[
                Opening {
                    run: 0,
                    at: 3.0,
                    width: 1.0,
                },
                Opening {
                    run: 0,
                    at: 3.5,
                    width: 1.0,
                },
            ],
            TOL,
        );
        assert!(result.unwrap_err().to_string().contains("overlaps"));
        let result = build_walls(
            &[straight, run(&[[3.0, 0.0], [7.0, 0.0]], false)],
            T,
            &[],
            TOL,
        );
        assert!(result.unwrap_err().to_string().contains("overlapping"));
    }

    #[test]
    fn justification_places_the_wall_and_opening_on_the_directed_side() {
        for (justification, lower, upper) in [
            (Justification::Centre, -0.1, 0.1),
            (Justification::Left, 0.0, 0.2),
            (Justification::Right, -0.2, 0.0),
        ] {
            let walls = build_walls_justified(
                &[run(&[[0.0, 0.0], [5.0, 0.0]], false)],
                T,
                &[Opening {
                    run: 0,
                    at: 2.5,
                    width: 0.9,
                }],
                TOL,
                justification,
            )
            .unwrap();
            assert!((walls.gross_area - 1.0).abs() < 1e-9);
            assert!((walls.net_area - 0.82).abs() < 1e-9);
            let ys: Vec<_> = walls
                .regions
                .iter()
                .flat_map(|r| r.outer.iter().map(|p| p[1]))
                .collect();
            assert!((ys.iter().copied().fold(f64::INFINITY, f64::min) - lower).abs() < 1e-9);
            assert!((ys.iter().copied().fold(f64::NEG_INFINITY, f64::max) - upper).abs() < 1e-9);
            assert!((walls.openings[0].centre[1] - (lower + upper) / 2.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_directed_corner_is_the_same_as_separate_runs() {
        for (justification, area) in [(Justification::Left, 1.36), (Justification::Right, 1.44)] {
            let one = build_walls_justified(
                &[run(&[[0.0, 0.0], [3.0, 0.0], [3.0, 4.0]], false)],
                T,
                &[],
                TOL,
                justification,
            )
            .unwrap();
            // Start with the downstream run to check joining does not depend on input order.
            let two = build_walls_justified(
                &[
                    run(&[[3.0, 0.0], [3.0, 4.0]], false),
                    run(&[[0.0, 0.0], [3.0, 0.0]], false),
                ],
                T,
                &[],
                TOL,
                justification,
            )
            .unwrap();
            assert!((one.gross_area - area).abs() < 1e-9);
            assert!((two.gross_area - area).abs() < 1e-9);
        }
    }

    #[test]
    fn a_partition_splits_a_rooms_hole() {
        let walls = build_walls(
            &[
                run(&[[0.0, 0.0], [5.0, 0.0], [5.0, 4.0], [0.0, 4.0]], true),
                run(&[[2.5, 0.0], [2.5, 4.0]], false),
            ],
            T,
            &[],
            TOL,
        )
        .unwrap();
        // Outer ring 3.6 m² plus 3.8 m of internal partition at 0.2 m.
        assert!((walls.gross_area - 4.36).abs() < 1e-9);
        assert_eq!(walls.regions.len(), 1);
        assert_eq!(walls.regions[0].holes.len(), 2);
    }

    #[test]
    fn walls_speed() {
        // 100 rooms, each made from four separate walls and an internal partition.
        let mut runs = Vec::new();
        for row in 0..10 {
            for column in 0..10 {
                let x = f64::from(column) * 8.0;
                let y = f64::from(row) * 7.0;
                for points in [
                    [[x, y], [x + 5.0, y]],
                    [[x + 5.0, y], [x + 5.0, y + 4.0]],
                    [[x + 5.0, y + 4.0], [x, y + 4.0]],
                    [[x, y + 4.0], [x, y]],
                    [[x + 2.5, y], [x + 2.5, y + 4.0]],
                ] {
                    runs.push(run(&points, false));
                }
            }
        }
        assert_eq!(runs.len(), 500);
        let start = std::time::Instant::now();
        let walls = build_walls(&runs, T, &[], TOL).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(walls.regions.len(), 100);
        assert!((walls.net_area - 436.0).abs() < 1e-7);
        if !cfg!(debug_assertions) {
            assert!(elapsed.as_secs_f64() < 2.0, "500 walls took {elapsed:?}");
        }
    }
}
