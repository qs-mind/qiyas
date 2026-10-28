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
use cavalier_contours::static_aabb2d_index::StaticAABB2DIndexBuilder;

/// One run of wall. Curves are represented by points along a polyline approximation.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    /// Overrides the system thickness, in the same units as the points.
    pub thickness: Option<f64>,
    /// Overrides the system justification.
    pub justification: Option<Justification>,
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

/// True when two wall regions share positive area, rather than only an edge or point.
/// Used to follow measured hatches through splits and merges without attaching unrelated
/// pieces. The boolean calculations use a shared local origin and scale, including holes.
pub fn regions_overlap(a: &Region, b: &Region, tol: f64) -> bool {
    if a.outer.len() < 3 || b.outer.len() < 3 || !bounds_overlap(&a.outer, &b.outer) {
        return false;
    }
    let origin = a.outer[0];
    let scale = [&a.outer, &b.outer]
        .into_iter()
        .chain(a.holes.iter())
        .chain(b.holes.iter())
        .flat_map(|ring| {
            ring.iter()
                .zip(ring.iter().cycle().skip(1))
                .take(ring.len())
        })
        .map(|(&p, &q)| dist(p, q))
        .filter(|length| *length > tol)
        .fold(f64::INFINITY, f64::min);
    if !scale.is_finite() {
        return false;
    }
    let local = |region: &Region| {
        let ring = |points: &Vec<[f64; 2]>| {
            points
                .iter()
                .map(|p| [(p[0] - origin[0]) / scale, (p[1] - origin[1]) / scale])
                .collect::<Vec<_>>()
        };
        Region {
            outer: ring(&region.outer),
            holes: region.holes.iter().map(ring).collect(),
        }
    };
    let (a, b) = (local(a), local(b));
    let intersection = |first: &[[f64; 2]], second: &[[f64; 2]]| {
        let result = to_pline(first).boolean(&to_pline(second), BooleanOp::And);
        result
            .pos_plines
            .iter()
            .map(|p| p.pline.area().abs())
            .sum::<f64>()
            - result
                .neg_plines
                .iter()
                .map(|p| p.pline.area().abs())
                .sum::<f64>()
    };
    // Inclusion-exclusion keeps a region inside a room's hole unrelated to its walls.
    let mut area = intersection(&a.outer, &b.outer);
    for hole in &a.holes {
        area -= intersection(hole, &b.outer);
    }
    for hole in &b.holes {
        area -= intersection(&a.outer, hole);
    }
    for first in &a.holes {
        for second in &b.holes {
            area += intersection(first, second);
        }
    }
    area > (tol / scale).powi(2)
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
    let (normalized, origin) = normalize_runs(runs, thickness, tol, justification)?;
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

/// One vertical face of the uncut union. `run` indexes the supplied runs; callers order
/// runs by driver identity to break shared-face ties. Coordinates and lengths are drawing units.
#[derive(Debug, Clone)]
pub struct BoundaryFace {
    pub run: usize,
    pub external: bool,
    pub a: [f64; 2],
    pub b: [f64; 2],
}

impl BoundaryFace {
    pub fn length(&self) -> f64 {
        dist(self.a, self.b)
    }

    /// Remaining face length after the union of opening rectangles. This clips the original
    /// face, so opening jambs never become measured faces and overlapping cuts count once.
    pub fn net_length(&self, openings: &[Placed]) -> f64 {
        let mut intervals = Vec::new();
        for opening in openings {
            let local = |p| {
                let d = difference(p, opening.centre);
                [
                    d[0] * opening.dir[0] + d[1] * opening.dir[1],
                    cross(opening.dir, d),
                ]
            };
            let a = local(self.a);
            let b = local(self.b);
            let mut low: f64 = 0.0;
            let mut high: f64 = 1.0;
            // Match the through-wall rectangle used by build_normalized.
            for (axis, half) in [(0, opening.width / 2.0), (1, opening.thickness * 0.6)] {
                let delta = b[axis] - a[axis];
                if delta.abs() <= f64::EPSILON * opening.thickness {
                    if a[axis].abs() > half {
                        high = -1.0;
                    }
                } else {
                    let p = (-half - a[axis]) / delta;
                    let q = (half - a[axis]) / delta;
                    low = low.max(p.min(q));
                    high = high.min(p.max(q));
                }
            }
            if high > low {
                intervals.push((low, high));
            }
        }
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut end: f64 = 0.0;
        let mut removed = 0.0;
        for (low, high) in intervals {
            removed += (high - low.max(end)).max(0.0);
            end = end.max(high);
        }
        self.length() * (1.0 - removed).max(0.0)
    }
}

/// Attribute the gross union boundary to its generating strip faces. Split collinear edges
/// at ownership changes; the first run owns ties. Junction hull edges belong to the first
/// member contributing an endpoint. Work in the same local frame as the wall booleans.
pub fn boundary_faces(
    runs: &[Run],
    thickness: f64,
    tol: f64,
) -> Result<Vec<BoundaryFace>, WallError> {
    let (runs, origin) = normalize_runs(runs, thickness, tol, Justification::Centre)?;
    let tol = (tol / thickness).max(1e-8);
    let gross = build_normalized(&runs, &[], tol, thickness)?;
    let mut candidates = Vec::new();
    let mut caps = Vec::new();
    let mut add = |run, a, b| {
        if dist(a, b) > tol {
            candidates.push(BoundaryFace {
                run,
                external: false,
                a,
                b,
            });
        }
    };
    for chain in join_runs(&runs, tol) {
        let (left, right) = strip_corners(&chain);
        let n = chain.points.len();
        for side in [&left, &right] {
            for (i, &owner) in chain.owners.iter().enumerate() {
                add(
                    owner,
                    *side[i].last().expect("corner"),
                    side[(i + 1) % n][0],
                );
            }
            for (i, corner) in side.iter().enumerate() {
                if corner.len() > 1 {
                    let before = chain.owners[(i + chain.owners.len() - 1) % chain.owners.len()];
                    let after = chain.owners[i % chain.owners.len()];
                    add(before.min(after), corner[0], corner[1]);
                }
            }
        }
        if !chain.closed {
            for (i, owner) in [
                (0, chain.owners[0]),
                (n - 1, *chain.owners.last().expect("segment")),
            ] {
                add(owner, left[i][0], right[i][0]);
                caps.push((chain.points[i], left[i][0], right[i][0], owner));
            }
        }
    }
    let mut cap_index = StaticAABB2DIndexBuilder::new(caps.len());
    for cap in &caps {
        cap_index.add(cap.0[0], cap.0[1], cap.0[0], cap.0[1]);
    }
    let cap_index = cap_index.build().map_err(|error| bad(error.to_string()))?;
    let mut hull_candidates = Vec::new();
    for cap in &caps {
        let group: Vec<_> = cap_index
            .query(
                cap.0[0] - tol,
                cap.0[1] - tol,
                cap.0[0] + tol,
                cap.0[1] + tol,
            )
            .into_iter()
            .map(|i| &caps[i])
            .filter(|other| dist(cap.0, other.0) <= tol)
            .collect();
        if group.len() < 2 {
            continue;
        }
        let hull = convex_hull(
            &group
                .iter()
                .flat_map(|c| [c.1, c.2])
                .chain([cap.0])
                .collect::<Vec<_>>(),
        );
        for (&a, &b) in hull
            .iter()
            .zip(hull.iter().cycle().skip(1))
            .take(hull.len())
        {
            let owner = group
                .iter()
                .filter(|c| {
                    [c.1, c.2]
                        .iter()
                        .any(|p| dist(*p, a) <= tol || dist(*p, b) <= tol)
                })
                .map(|c| c.3)
                .min()
                .unwrap_or(cap.3);
            if dist(a, b) > tol {
                hull_candidates.push(BoundaryFace {
                    run: owner,
                    external: false,
                    a,
                    b,
                });
            }
        }
    }
    // Spatial queries select only nearby candidates before testing exact collinearity.
    // Preserve strip priority and id tie-breaking independently of index traversal order.
    let candidates: Vec<_> = candidates
        .into_iter()
        .map(|c| (c, false))
        .chain(hull_candidates.into_iter().map(|c| (c, true)))
        .collect();
    let mut candidate_index = StaticAABB2DIndexBuilder::new(candidates.len());
    for (c, _) in &candidates {
        candidate_index.add(
            c.a[0].min(c.b[0]),
            c.a[1].min(c.b[1]),
            c.a[0].max(c.b[0]),
            c.a[1].max(c.b[1]),
        );
    }
    let candidate_index = candidate_index
        .build()
        .map_err(|error| bad(error.to_string()))?;
    let mut faces = Vec::new();
    for region in gross.regions {
        for (ring, external) in
            std::iter::once((&region.outer, true)).chain(region.holes.iter().map(|h| (h, false)))
        {
            for (&a, &b) in ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .take(ring.len())
            {
                let length = dist(a, b);
                if length <= tol {
                    continue;
                }
                let dir = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
                let along = |p| {
                    let d = difference(p, a);
                    d[0] * dir[0] + d[1] * dir[1]
                };
                let padding = 2.0 * tol;
                let matching: Vec<_> = candidate_index
                    .query(
                        a[0].min(b[0]) - padding,
                        a[1].min(b[1]) - padding,
                        a[0].max(b[0]) + padding,
                        a[1].max(b[1]) + padding,
                    )
                    .into_iter()
                    .map(|i| (&candidates[i].0, candidates[i].1))
                    .filter(|(c, _)| {
                        cross(dir, difference(c.a, a)).abs() <= tol
                            && cross(dir, difference(c.b, a)).abs() <= tol
                    })
                    .map(|(c, hull)| {
                        (
                            c.run,
                            along(c.a).min(along(c.b)),
                            along(c.a).max(along(c.b)),
                            hull,
                        )
                    })
                    .collect();
                let mut stops = vec![0.0, length];
                for &(_, low, high, _) in &matching {
                    stops.extend([low.clamp(0.0, length), high.clamp(0.0, length)]);
                }
                stops.sort_by(f64::total_cmp);
                stops.dedup();
                for pair in stops.windows(2) {
                    if pair[1] <= pair[0] {
                        continue;
                    }
                    let mid = (pair[0] + pair[1]) / 2.0;
                    let owner = matching
                        .iter()
                        .filter(|(_, low, high, _)| mid >= low - tol && mid <= high + tol)
                        // Prefer a real strip face over the junction fill's fallback owner.
                        .min_by_key(|(owner, _, _, hull)| (*hull, *owner))
                        .map(|(owner, _, _, _)| *owner)
                        .ok_or_else(|| bad("Union boundary has no generating strip face"))?;
                    let point = |at| {
                        [
                            (a[0] + dir[0] * at) * thickness + origin[0],
                            (a[1] + dir[1] * at) * thickness + origin[1],
                        ]
                    };
                    let (a, b) = (point(pair[0]), point(pair[1]));
                    // Nearby projected stops can round to the same drawing coordinate.
                    if a != b {
                        faces.push(BoundaryFace {
                            run: owner,
                            external,
                            a,
                            b,
                        });
                    }
                }
            }
        }
    }
    Ok(faces)
}

/// Checks dimensions before moving the drawing to a local, thickness-normalised frame.
fn normalize_runs(
    runs: &[Run],
    thickness: f64,
    tol: f64,
    justification: Justification,
) -> Result<(Vec<Run>, [f64; 2]), WallError> {
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
        if r.thickness
            .is_some_and(|width| !width.is_finite() || width <= 0.0)
        {
            return Err(bad(format!(
                "Run {i} thickness must be finite and more than 0"
            )));
        }
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
            Run {
                points,
                closed,
                thickness: Some(r.thickness.unwrap_or(thickness) / thickness),
                justification: Some(r.justification.unwrap_or(justification)),
            }
        })
        .collect();
    if normalized.iter().any(|r| r.closed && r.points.len() < 3) {
        return Err(bad("A closed run needs three distinct points"));
    }
    Ok((normalized, origin))
}

/// `unit`: one normalised unit in drawing units (the thickness), for messages.
fn build_normalized(
    runs: &[Run],
    openings: &[Opening],
    tol: f64,
    unit: f64,
) -> Result<Walls, WallError> {
    check_overlaps(runs, tol)?;
    check_openings(runs, openings, tol, unit)?;
    let centreline: f64 = runs.iter().map(run_length).sum();
    let chains = join_runs(runs, tol);
    let mut polygons: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut caps: Vec<EndCap> = Vec::new();
    for chain in &chains {
        let (pieces, ends) = strip(chain);
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
        let (left, right) = offsets(&runs[o.run]);
        let thickness = left - right;
        let shift = (left + right) / 2.0;
        let centre = [point[0] - dir[1] * shift, point[1] + dir[0] * shift];
        let cut = rectangle(centre, dir, o.width, thickness * 1.2);
        cut_from(&mut regions, &cut);
        placed.push(Placed {
            centre,
            dir,
            width: o.width,
            thickness,
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
    left: f64,
    right: f64,
}

fn offsets(run: &Run) -> (f64, f64) {
    let width = run.thickness.expect("normalised run thickness");
    match run.justification.expect("normalised run justification") {
        Justification::Centre => (width / 2.0, -width / 2.0),
        Justification::Left => (width, 0.0),
        Justification::Right => (0.0, -width),
    }
}

fn segments(runs: &[Run]) -> Vec<Segment> {
    let mut result = Vec::new();
    for (run, r) in runs.iter().enumerate() {
        let mut from = 0.0;
        let (left, right) = offsets(r);
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
                left,
                right,
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

fn check_overlaps(runs: &[Run], tol: f64) -> Result<(), WallError> {
    let segments: Vec<_> = segments(runs)
        .into_iter()
        .map(|mut segment| {
            let shift = (segment.left + segment.right) / 2.0;
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
            let half_widths = (first.left - first.right + second.left - second.right) / 2.0;
            if cross(dir, offset).abs() >= half_widths - tol {
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
    unit: f64,
) -> Result<(), WallError> {
    let segments = segments(runs);
    let fits = |index| fit_message(&segments, openings, index, tol, unit);
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
        for (at, from, to) in junctions(host, &segments, tol) {
            if opening.at + half >= from - tol && opening.at - half <= to + tol {
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

/// Where other walls cross or meet `host`: the junction distance and its occupied range.
/// Project both wall strips onto the host, so unequal widths, oblique intersections and
/// directed justification all reserve enough room for the entire opening rectangle.
fn junctions(host: &Segment, segments: &[Segment], tol: f64) -> Vec<(f64, f64, f64)> {
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
            let sine = determinant / (host.length * other.length);
            let cosine = (host_delta[0] * other_delta[0] + host_delta[1] * other_delta[1])
                / (host.length * other.length);
            let at = host.from + along_host * host.length;
            let distances = [host.right, host.left].into_iter().flat_map(|offset| {
                [other.right, other.left].map(move |face| at + (cosine * offset - face) / sine)
            });
            let (from, to) = distances.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
                (a.min(x), b.max(x))
            });
            Some((at, from, to))
        })
        .collect()
}

/// Where opening `index` could go on its run, for an error: the ranges its centre may take,
/// in drawing units along the run, clear of corners, junctions and the run's other openings
/// (the same rules as `check_openings`), or that it fits nowhere there. A junction's side of a
/// range is shown rounded inwards (touching its clearance is refused); a run's end and a
/// neighbouring opening may be touched.
fn fit_message(
    segments: &[Segment],
    openings: &[Opening],
    index: usize,
    tol: f64,
    unit: f64,
) -> String {
    let opening = &openings[index];
    if !opening.width.is_finite() || opening.width <= tol {
        return format!(
            "; choose a finite width greater than {} drawing units",
            tol * unit
        );
    }
    let centres = free_centres(segments, openings, index, tol);
    let nearest = centres
        .iter()
        .filter_map(|&(a, a_ok, b, b_ok)| {
            let a = if a_ok { a } else { a + tol };
            let b = if b_ok { b } else { b - tol };
            (a <= b && opening.at.is_finite()).then(|| opening.at.clamp(a, b))
        })
        .min_by(|a, b| (a - opening.at).abs().total_cmp(&(b - opening.at).abs()));
    let shown: Vec<String> = centres
        .iter()
        .filter_map(|&(a, a_ok, b, b_ok)| {
            let (a, b) = (a * unit * 1000.0, b * unit * 1000.0);
            let a = if a_ok { a.ceil() } else { a.floor() + 1.0 } / 1000.0;
            let b = if b_ok { b.floor() } else { b.ceil() - 1.0 } / 1000.0;
            (a <= b).then(|| format!("{} to {}", number(a), number(b)))
        })
        .collect();
    if nearest.is_none() {
        return format!(
            "; an opening {} wide fits nowhere on run {}: make it narrower or put it on another run",
            number(opening.width * unit),
            opening.run
        );
    }
    format!(
        "; an opening {} wide fits on run {} with its `at` from {}; nearest valid `at`: {} (drawing units). This is advice; no opening was moved",
        number(opening.width * unit),
        opening.run,
        if shown.is_empty() { (nearest.unwrap() * unit).to_string() } else { shown.join(", or from ") },
        nearest.unwrap() * unit
    )
}

fn free_centres(
    segments: &[Segment],
    openings: &[Opening],
    index: usize,
    tol: f64,
) -> Vec<(f64, bool, f64, bool)> {
    let opening = &openings[index];
    let half = opening.width / 2.0;
    // (from, to, whether each end may be touched), in normalised units along the run.
    let mut centres: Vec<(f64, bool, f64, bool)> = Vec::new();
    for host in segments.iter().filter(|s| s.run == opening.run) {
        let mut blocked: Vec<(f64, f64, bool)> = junctions(host, segments, tol)
            .into_iter()
            .map(|(_, from, to)| (from - tol, to + tol, false))
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
    centres
}

/// Inclusive `at` ranges for a new opening, in drawing units. Existing openings, run ends,
/// corners and junctions use exactly the same clearances as wall construction. The tiny
/// inward margin on strict junction boundaries makes both returned ends valid placements.
pub fn where_it_fits(
    runs: &[Run],
    thickness: f64,
    openings: &[Opening],
    run: usize,
    width: f64,
    tol: f64,
    justification: Justification,
) -> Result<Vec<[f64; 2]>, WallError> {
    let (normalized, _) = normalize_runs(runs, thickness, tol, justification)?;
    if run >= runs.len() {
        return Err(bad(format!("No run {run} for an opening")));
    }
    if !width.is_finite() || width <= tol {
        return Err(bad(
            "The opening width must be finite and greater than the tolerance",
        ));
    }
    let tolerance = (tol / thickness).max(1e-8);
    let mut proposed: Vec<_> = openings
        .iter()
        .map(|opening| Opening {
            run: opening.run,
            at: opening.at / thickness,
            width: opening.width / thickness,
        })
        .collect();
    proposed.push(Opening {
        run,
        at: 0.0,
        width: width / thickness,
    });
    Ok(free_centres(
        &segments(&normalized),
        &proposed,
        proposed.len() - 1,
        tolerance,
    )
    .into_iter()
    .filter_map(|(a, a_ok, b, b_ok)| {
        let a = if a_ok { a } else { a + tolerance } * thickness;
        let b = if b_ok { b } else { b - tolerance } * thickness;
        (a <= b).then_some([a, b])
    })
    .collect())
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
    /// Left and right offsets for each segment, following its direction.
    offsets: Vec<(f64, f64)>,
    owners: Vec<usize>,
}

/// Runs joined end to end where exactly two ends meet (and no run passes through).
fn join_runs(runs: &[Run], tol: f64) -> Vec<Chain> {
    let chains: Vec<Chain> = runs
        .iter()
        .enumerate()
        .map(|(owner, r)| Chain {
            points: r.points.clone(),
            closed: r.closed,
            offsets: vec![offsets(r); r.points.len() - usize::from(!r.closed)],
            owners: vec![owner; r.points.len() - usize::from(!r.closed)],
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
            if other != i {
                links[i][usize::from(end)] = Some((other, other_end));
            }
        }
    }
    // Visit open chains from a free end first, then the remaining closed loops.
    let mut starts: Vec<_> = (0..chains.len()).collect();
    starts.sort_by_key(|&i| links[i].iter().all(Option::is_some));
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
        let mut entering = links[start][0].is_some() && links[start][1].is_none();
        let mut points = Vec::new();
        let mut offsets = Vec::new();
        let mut owners = Vec::new();
        let mut closed = false;
        loop {
            seen[current] = true;
            let mut next = chains[current].points.clone();
            let mut next_offsets = chains[current].offsets.clone();
            let mut next_owners = chains[current].owners.clone();
            if entering {
                next.reverse();
                next_offsets.reverse();
                next_owners.reverse();
                for (left, right) in &mut next_offsets {
                    (*left, *right) = (-*right, -*left);
                }
            }
            points.extend(next.into_iter().skip(usize::from(!points.is_empty())));
            offsets.extend(next_offsets);
            owners.extend(next_owners);
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
        result.push(Chain {
            points,
            closed,
            offsets,
            owners,
        });
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

type Corners = Vec<Vec<[f64; 2]>>;

fn strip_corners(c: &Chain) -> (Corners, Corners) {
    let pts = &c.points;
    let n = pts.len();
    // Each corner may have two vertices: a collinear change in thickness is a step.
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let previous = if i > 0 {
            Some(i - 1)
        } else if c.closed {
            Some(n - 1)
        } else {
            None
        };
        let next = (i + 1 < n || c.closed).then_some(i);
        let side = |left: bool| {
            let offset = |segment: usize| {
                let (l, r) = c.offsets[segment];
                if left {
                    l
                } else {
                    r
                }
            };
            let before = previous.map(|segment| (pts[segment], offset(segment)));
            let after = next.map(|segment| (pts[(segment + 1) % n], offset(segment)));
            offset_corner(before, pts[i], after)
        };
        left.push(side(true));
        right.push(side(false));
    }
    (left, right)
}

fn strip(c: &Chain) -> Strip {
    let pts = &c.points;
    let n = pts.len();
    let (left, right) = strip_corners(c);
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
            let mut ring: Vec<[f64; 2]> =
                idx.iter().flat_map(|&i| left[i].iter().copied()).collect();
            ring.extend(
                idx.iter()
                    .rev()
                    .flat_map(|&i| right[i].iter().rev().copied()),
            );
            ring
        };
        let pieces = if n == 3 {
            vec![half(0, 2), half(1, 0), half(2, 1)]
        } else {
            vec![half(0, k + 1), half(k, 1)]
        };
        return (pieces, caps);
    }
    caps.push((pts[0], left[0][0], right[0][0]));
    caps.push((pts[n - 1], left[n - 1][0], right[n - 1][0]));
    let mut ring: Vec<[f64; 2]> = left.into_iter().flatten().collect();
    ring.extend(
        right
            .into_iter()
            .rev()
            .flat_map(|points| points.into_iter().rev()),
    );
    (vec![ring], caps)
}

/// Intersects the two face lines at a corner. Each segment keeps its own offset, and
/// mitres remain limited to four offset widths. Collinear unequal widths make a step.
fn offset_corner(
    previous: Option<([f64; 2], f64)>,
    point: [f64; 2],
    next: Option<([f64; 2], f64)>,
) -> Vec<[f64; 2]> {
    let direction = |a: [f64; 2], b: [f64; 2]| {
        let length = dist(a, b);
        [(b[0] - a[0]) / length, (b[1] - a[1]) / length]
    };
    let shifted = |direction: [f64; 2], offset: f64| {
        [
            point[0] - direction[1] * offset,
            point[1] + direction[0] * offset,
        ]
    };
    match (previous, next) {
        (None, Some((next, offset))) => vec![shifted(direction(point, next), offset)],
        (Some((previous, offset)), None) => vec![shifted(direction(previous, point), offset)],
        (Some((previous, before)), Some((next, after))) => {
            let a = direction(previous, point);
            let b = direction(point, next);
            let p = shifted(a, before);
            let q = shifted(b, after);
            let determinant = cross(a, b);
            if determinant.abs() < 1e-9 {
                return if dist(p, q) < 1e-9 || a[0] * b[0] + a[1] * b[1] < 0.0 {
                    vec![p]
                } else {
                    vec![p, q]
                };
            }
            let along = cross(difference(q, p), b) / determinant;
            let intersection = [p[0] + a[0] * along, p[1] + a[1] * along];
            let delta = difference(intersection, point);
            let length = delta[0].hypot(delta[1]);
            let limit = 4.0 * before.abs().max(after.abs());
            if length > limit {
                vec![[
                    point[0] + delta[0] * limit / length,
                    point[1] + delta[1] * limit / length,
                ]]
            } else {
                vec![intersection]
            }
        }
        (None, None) => vec![point],
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
    use crate::performance;

    fn run(points: &[[f64; 2]], closed: bool) -> Run {
        Run {
            points: points.to_vec(),
            closed,
            thickness: None,
            justification: None,
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
    fn mixed_t_l_and_x_junctions_have_exact_areas_in_every_unit() {
        // In metres: T = 10*.25 + (4-.125)*.15 = 3.08125;
        // L = 4*.25 + 3*.15 = 1.45 (the two mitre quadrants cancel);
        // X = 10*.25 + 8*.15 - .25*.15 = 3.6625.
        // Every case has a .9 m opening in the thick run and a .8 m opening
        // in the thin run: subtract .9*.25 + .8*.15 = .345 m2 once.
        for units_per_metre in [1000.0, 100.0, 1.0, 1000.0 / 25.4] {
            for origin_metres in [[0.0, 0.0], [500_000.0, 2_700_000.0]] {
                for (thick, thin, gross) in [
                    ([[0.0, 0.0], [10.0, 0.0]], [[5.0, 0.0], [5.0, 4.0]], 3.08125),
                    ([[0.0, 0.0], [4.0, 0.0]], [[4.0, 0.0], [4.0, 3.0]], 1.45),
                    ([[0.0, 0.0], [10.0, 0.0]], [[5.0, -4.0], [5.0, 4.0]], 3.6625),
                ] {
                    let make_run = |points: [[f64; 2]; 2], width| Run {
                        points: points
                            .iter()
                            .map(|p| {
                                [
                                    (p[0] + origin_metres[0]) * units_per_metre,
                                    (p[1] + origin_metres[1]) * units_per_metre,
                                ]
                            })
                            .collect(),
                        closed: false,
                        thickness: Some(width * units_per_metre),
                        justification: None,
                    };
                    let runs = [make_run(thick, 0.25), make_run(thin, 0.15)];
                    let openings = [
                        Opening {
                            run: 0,
                            at: 2.0 * units_per_metre,
                            width: 0.9 * units_per_metre,
                        },
                        Opening {
                            run: 1,
                            at: 2.0 * units_per_metre,
                            width: 0.8 * units_per_metre,
                        },
                    ];
                    let walls = build_walls(
                        &runs,
                        0.25 * units_per_metre,
                        &openings,
                        TOL * units_per_metre,
                    )
                    .unwrap();
                    let area_scale = units_per_metre * units_per_metre;
                    assert!(
                        (walls.gross_area / area_scale - gross).abs() < 1e-8,
                        "unit={units_per_metre}, origin={origin_metres:?}: gross {} != {gross}",
                        walls.gross_area / area_scale
                    );
                    assert!(
                        (walls.net_area / area_scale - (gross - 0.345)).abs() < 1e-8,
                        "unit={units_per_metre}, origin={origin_metres:?}: net {}",
                        walls.net_area / area_scale
                    );
                    assert!((walls.openings[0].thickness / units_per_metre - 0.25).abs() < 1e-12);
                    assert!((walls.openings[1].thickness / units_per_metre - 0.15).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn opening_clearance_uses_the_meeting_walls_width_and_ranges_are_usable() {
        let mut through = run(&[[0.0, 0.0], [10.0, 0.0]], false);
        through.thickness = Some(0.25);
        let mut branch = run(&[[5.0, 0.0], [5.0, 4.0]], false);
        branch.thickness = Some(0.15);
        let runs = [through, branch];
        let opening = |run, at, width| Opening { run, at, width };
        build_walls(&runs, 0.25, &[opening(0, 4.45, 0.9)], TOL).unwrap();
        let refused = build_walls(&runs, 0.25, &[opening(0, 4.5, 0.9)], TOL)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("junction or corner at 5"), "{refused}");
        assert!(refused.contains("4.474"), "{refused}");
        assert!(build_walls(&runs, 0.25, &[opening(1, 0.5, 0.8)], TOL).is_err());
        build_walls(&runs, 0.25, &[opening(1, 0.526, 0.8)], TOL).unwrap();
        let existing = [opening(0, 2.0, 0.9)];
        for (run, width) in [(0, 0.9), (1, 0.8)] {
            let ranges = where_it_fits(
                &runs,
                0.25,
                &existing,
                run,
                width,
                TOL,
                Justification::Centre,
            )
            .unwrap();
            assert!(!ranges.is_empty());
            for range in ranges {
                for at in range {
                    let mut openings = existing.to_vec();
                    openings.push(opening(run, at, width));
                    build_walls(&runs, 0.25, &openings, TOL).unwrap();
                }
            }
        }
    }

    #[test]
    fn mixed_opening_read_ranges_fit_in_every_drawing_unit() {
        for units_per_metre in [1000.0, 100.0, 1.0, 1000.0 / 25.4] {
            let mut through = run(&[[0.0, 0.0], [10.0 * units_per_metre, 0.0]], false);
            through.thickness = Some(0.25 * units_per_metre);
            let mut branch = run(
                &[
                    [5.0 * units_per_metre, 0.0],
                    [5.0 * units_per_metre, 4.0 * units_per_metre],
                ],
                false,
            );
            branch.thickness = Some(0.15 * units_per_metre);
            let runs = [through, branch];
            let width = 0.9007 * units_per_metre;
            let existing = [Opening {
                run: 0,
                at: 2.0 * units_per_metre,
                width,
            }];
            let thickness = 0.25 * units_per_metre;
            let tolerance = TOL * units_per_metre;
            let ranges = where_it_fits(
                &runs,
                thickness,
                &existing,
                0,
                width,
                tolerance,
                Justification::Centre,
            )
            .unwrap();
            assert_eq!(ranges.len(), 3);
            for range in ranges {
                for at in range {
                    let mut openings = existing.to_vec();
                    openings.push(Opening { run: 0, at, width });
                    build_walls(&runs, thickness, &openings, tolerance).unwrap();
                }
            }
        }
    }

    #[test]
    fn a_thin_partition_stops_at_both_faces_of_a_thick_outer_wall() {
        let mut outer = run(&[[0.0, 0.0], [5.0, 0.0], [5.0, 4.0], [0.0, 4.0]], true);
        outer.thickness = Some(0.25);
        let mut partition = run(&[[2.5, 0.0], [2.5, 4.0]], false);
        partition.thickness = Some(0.15);
        let walls = build_walls(&[outer, partition], 0.25, &[], TOL).unwrap();
        // 18*.25 for the ring, plus 3.75*.15 between its two inner faces.
        assert!((walls.gross_area - 5.0625).abs() < 1e-10);
        assert_eq!(walls.regions.len(), 1);
        assert_eq!(walls.regions[0].holes.len(), 2);
    }

    #[test]
    fn collinear_runs_change_width_without_tapering() {
        let mut thick = run(&[[0.0, 0.0], [3.0, 0.0]], false);
        thick.thickness = Some(0.25);
        let mut thin = run(&[[3.0, 0.0], [7.0, 0.0]], false);
        thin.thickness = Some(0.15);
        let walls = build_walls(&[thick, thin], 0.2, &[], TOL).unwrap();
        assert!((walls.gross_area - 1.35).abs() < 1e-10);
        assert_eq!(walls.regions.len(), 1);
        assert!(walls.regions[0].outer.contains(&[3.0, 0.125]));
        assert!(walls.regions[0].outer.contains(&[3.0, 0.075]));
    }

    #[test]
    fn each_run_can_override_justification_and_reverse_its_direction() {
        let mut thick = run(&[[0.0, 0.0], [4.0, 0.0]], false);
        thick.thickness = Some(0.25);
        thick.justification = Some(Justification::Left);
        let mut thin = run(&[[4.0, 0.0], [4.0, 3.0]], false);
        thin.thickness = Some(0.15);
        thin.justification = Some(Justification::Left);
        let first = build_walls(&[thick.clone(), thin.clone()], T, &[], TOL).unwrap();
        thin.points.reverse();
        thin.justification = Some(Justification::Right);
        let reversed = build_walls(&[thin, thick], T, &[], TOL).unwrap();
        // The inside corner removes one .25 by .15 rectangle from length times width.
        assert!((first.gross_area - (1.45 - 0.25 * 0.15)).abs() < 1e-10);
        assert!((first.gross_area - reversed.gross_area).abs() < 1e-10);
    }

    #[test]
    fn rebuilt_regions_follow_only_overlapping_material_in_every_unit() {
        for unit in [1000.0, 100.0, 1.0, 1000.0 / 25.4] {
            let ring = |x: f64, y: f64, w: f64, h: f64| {
                [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
                    .map(|p| [(p[0] + 500_000.0) * unit, (p[1] + 2_700_000.0) * unit])
                    .to_vec()
            };
            let wall = Region {
                outer: ring(0.0, 0.0, 10.0, 0.25),
                holes: Vec::new(),
            };
            let left = Region {
                outer: ring(0.0, 0.0, 4.5, 0.25),
                holes: Vec::new(),
            };
            let right = Region {
                outer: ring(5.5, 0.0, 4.5, 0.25),
                holes: Vec::new(),
            };
            let touching = Region {
                outer: ring(0.0, 0.25, 4.5, 0.25),
                holes: Vec::new(),
            };
            let room = Region {
                outer: ring(-0.125, -0.125, 10.25, 5.25),
                holes: vec![ring(0.125, 0.125, 9.75, 4.75)],
            };
            let inside = Region {
                outer: ring(1.0, 1.0, 4.0, 0.15),
                holes: Vec::new(),
            };
            for part in [&left, &right] {
                assert!(regions_overlap(&wall, part, TOL * unit));
                assert!(regions_overlap(part, &wall, TOL * unit));
            }
            assert!(!regions_overlap(&left, &right, TOL * unit));
            assert!(!regions_overlap(&left, &touching, TOL * unit));
            assert!(!regions_overlap(&room, &inside, TOL * unit));
            assert!(regions_overlap(&room, &wall, TOL * unit));
            assert!(regions_overlap(&room, &room, TOL * unit));
        }
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
        let reference = (!cfg!(debug_assertions)).then(performance::reference);
        let start = std::time::Instant::now();
        let walls = build_walls(&runs, T, &[], TOL).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(walls.regions.len(), 100);
        assert!((walls.net_area - 436.0).abs() < 1e-7);
        if let Some(reference) = reference {
            performance::assert_relative(elapsed, reference, 5.0, "500 walls");
        }
    }
}
