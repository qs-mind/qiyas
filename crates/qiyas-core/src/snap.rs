//! Object snaps (end, middle, centre, node, intersection, nearest, perpendicular) and the
//! drafting settings the window draws and measures with: ortho, polar tracking, the grid and the
//! display precision.

use opencadkernel::geom2d::{
    characteristic_points, distance_to, intersect, nearest_to, perpendicular_from, Curve,
    SnapKind as KernelKind, Tolerance,
};
use qiyas_geom::snap_shapes;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::locate;
use crate::{CoreError, Session};

/// What kind of point a snap found. Declared best first: a snap prefers the earlier kind, and
/// the nearer point between two of one kind.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    /// An end of a line or arc, or a polyline vertex.
    End,
    /// Where two entities cross.
    Intersection,
    /// The middle of a line, an arc or a polyline segment.
    Mid,
    /// The centre of a circle, arc, ellipse or bulged polyline segment (offered while the cursor
    /// is on the curve).
    Centre,
    /// A point entity, or a polyline vertex.
    Node,
    /// The foot of the perpendicular from the previous point.
    Perpendicular,
    /// The point on a curve nearest the cursor.
    Nearest,
}

/// Which snaps are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SnapModes {
    pub end: bool,
    pub mid: bool,
    pub centre: bool,
    pub node: bool,
    pub intersection: bool,
    pub nearest: bool,
    pub perpendicular: bool,
}

impl Default for SnapModes {
    /// All on except nearest and perpendicular.
    fn default() -> Self {
        SnapModes {
            end: true,
            mid: true,
            centre: true,
            node: true,
            intersection: true,
            nearest: false,
            perpendicular: false,
        }
    }
}

impl SnapModes {
    fn allows(&self, kind: SnapKind) -> bool {
        match kind {
            SnapKind::End => self.end,
            SnapKind::Intersection => self.intersection,
            SnapKind::Mid => self.mid,
            SnapKind::Centre => self.centre,
            SnapKind::Node => self.node,
            SnapKind::Perpendicular => self.perpendicular,
            SnapKind::Nearest => self.nearest,
        }
    }
}

/// How the window draws and measures: object snaps, ortho, polar tracking, the grid and the
/// number of decimals shown. Saved in the project file; not an undo step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Drafting {
    /// Object snaps on at all (F3).
    pub osnap: bool,
    pub snap_modes: SnapModes,
    /// Each next point straight across or up from the one before (F8).
    pub ortho: bool,
    /// Each next point's direction snapped to a multiple of `polar_step_deg` when close (F10).
    pub polar: bool,
    /// Degrees, above 0 and at most 180.
    pub polar_step_deg: f64,
    /// The dot grid shown (F7).
    pub grid: bool,
    /// Drawing units between grid dots, above 0; every tenth dot is a major one.
    pub grid_spacing: f64,
    /// Points go to the nearest grid dot when no object snap is found.
    pub grid_snap: bool,
    /// Decimals shown in the take-off and the status bar: 2, 3 or 4.
    pub precision: u8,
}

impl Default for Drafting {
    fn default() -> Self {
        Drafting {
            osnap: true,
            snap_modes: SnapModes::default(),
            ortho: false,
            polar: false,
            polar_step_deg: 45.0,
            grid: false,
            grid_spacing: 1.0,
            grid_snap: false,
            precision: 3,
        }
    }
}

impl Drafting {
    /// The settings with every number brought into range, for ones read from a project file
    /// someone may have edited: precision 0 to 6, polar step 1 to 180 degrees, a grid spacing
    /// above 0 (else the default).
    pub(crate) fn clamped(self) -> Drafting {
        let default = Drafting::default();
        Drafting {
            precision: self.precision.min(6),
            polar_step_deg: if self.polar_step_deg.is_nan() {
                default.polar_step_deg
            } else {
                self.polar_step_deg.clamp(1.0, 180.0)
            },
            grid_spacing: if self.grid_spacing.is_finite() && self.grid_spacing > 0.0 {
                self.grid_spacing
            } else {
                default.grid_spacing
            },
            ..self
        }
    }

    /// Fails when a number is out of range.
    pub(crate) fn check(&self) -> Result<(), CoreError> {
        let invalid = |m: &str| Err(CoreError::InvalidCommand(m.to_string()));
        if !(self.polar_step_deg > 0.0 && self.polar_step_deg <= 180.0) {
            return invalid("The polar angle must be above 0 and at most 180 degrees");
        }
        if !(self.grid_spacing.is_finite() && self.grid_spacing > 0.0) {
            return invalid("The grid spacing must be above 0");
        }
        if !(2..=4).contains(&self.precision) {
            return invalid("The precision must be 2, 3 or 4 decimals");
        }
        Ok(())
    }
}

/// A point found by `Session::snap`, on the entity `handle` inside the block references `chain`
/// (outermost first; empty on the sheet). For an intersection, the first of the two entities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snap {
    pub point: [f64; 2],
    pub kind: SnapKind,
    pub handle: u64,
    pub chain: Vec<u64>,
}

/// At most this many entities are looked at around the cursor.
const MAX_ENTITIES: usize = 8;
/// At most this many segments of the pick index are walked to find them.
const MAX_SEGMENTS: usize = 4096;

/// The best snap so far.
struct Best<'m> {
    modes: &'m SnapModes,
    p: [f64; 2],
    tol: f64,
    found: Option<(Snap, f64)>,
}

impl Best<'_> {
    /// Keeps `point` when its kind is on, it is within the tolerance, and it beats the best so
    /// far: a better kind, or the same kind nearer the cursor. `reach` is how far the cursor is
    /// from it (for a centre, from its curve), the distance to `point` when `None`.
    fn offer(&mut self, kind: SnapKind, point: [f64; 2], reach: Option<f64>, at: &(u64, &[u64])) {
        let d = reach.unwrap_or_else(|| dist(self.p, point));
        if !self.modes.allows(kind) || d > self.tol || !point.iter().all(|c| c.is_finite()) {
            return;
        }
        let better = self
            .found
            .as_ref()
            .is_none_or(|(s, best)| (kind, d) < (s.kind, *best));
        if better {
            let snap = Snap {
                point,
                kind,
                handle: at.0,
                chain: at.1.to_vec(),
            };
            self.found = Some((snap, d));
        }
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Session {
    /// The best snap within `world_tol` of `p` (drawing coordinates) among the entities drawn
    /// near it, with `modes` on; `from` is the previous point, for perpendicular snaps. Ranked
    /// End, Intersection, Mid, Centre, Node, Perpendicular, Nearest, then by distance.
    pub fn snap(
        &self,
        p: [f64; 2],
        world_tol: f64,
        modes: &SnapModes,
        from: Option<[f64; 2]>,
    ) -> Option<Snap> {
        if !(world_tol.is_finite() && world_tol > 0.0 && p.iter().all(|c| c.is_finite())) {
            return None;
        }
        let (entities, points) = self.near_entities(p, world_tol, MAX_ENTITIES, MAX_SEGMENTS);
        let mut best = Best {
            modes,
            p,
            tol: world_tol,
            found: None,
        };
        for (point, handle) in points {
            best.offer(SnapKind::Node, point, None, &(handle, &[]));
        }

        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        // Pieces of the entities that pass within the tolerance, with their entity's index.
        let mut near: Vec<(usize, Curve)> = Vec::new();
        for (i, (handle, chain)) in entities.iter().enumerate() {
            let at = (*handle, chain.as_slice());
            let Ok(located) = locate(doc, &xrefs, *handle, chain) else {
                continue;
            };
            let shapes = snap_shapes(located.doc, located.entity, &located.xf, world_tol / 4.0);
            for shape in shapes {
                for &v in &shape.vertices {
                    best.offer(SnapKind::Node, v, None, &at);
                }
                for &end in &shape.ends {
                    best.offer(SnapKind::End, end, None, &at);
                }
                for piece in shape.pieces {
                    let d = distance_to(&piece, p);
                    if d.is_nan() || d > world_tol {
                        continue;
                    }
                    if shape.exact {
                        for sp in characteristic_points(&piece) {
                            match sp.kind {
                                KernelKind::Endpoint => {
                                    best.offer(SnapKind::End, sp.point, None, &at)
                                }
                                KernelKind::Midpoint => {
                                    best.offer(SnapKind::Mid, sp.point, None, &at)
                                }
                                KernelKind::Centre => {
                                    let reach = d.min(dist(p, sp.point));
                                    best.offer(SnapKind::Centre, sp.point, Some(reach), &at);
                                }
                                _ => {}
                            }
                        }
                    }
                    if modes.nearest {
                        best.offer(SnapKind::Nearest, nearest_to(&piece, p).point, None, &at);
                    }
                    if let Some(from) = from.filter(|_| modes.perpendicular) {
                        for foot in perpendicular_from(&piece, from) {
                            best.offer(SnapKind::Perpendicular, foot.point, None, &at);
                        }
                    }
                    near.push((i, piece));
                }
            }
        }

        if modes.intersection {
            let tolerance = Tolerance::new((world_tol * 1e-6).max(1e-12));
            for (a, (ia, ca)) in near.iter().enumerate() {
                let (handle, chain) = &entities[*ia];
                for (ib, cb) in &near[a + 1..] {
                    // Crossings of an entity with itself are left out.
                    if ia == ib {
                        continue;
                    }
                    for crossing in intersect(ca, cb, tolerance) {
                        let at = (*handle, chain.as_slice());
                        best.offer(SnapKind::Intersection, crossing.point, None, &at);
                    }
                }
            }
        }
        best.found.map(|(snap, _)| snap)
    }
}
