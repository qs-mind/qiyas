//! Exact spatial evidence for a drawing brief. No subject-specific placement rules.
use crate::{CoreError, Session, Target};
use opencadkernel::geom2d::{closest_point, contains, intersect, Curve, Tolerance};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Query {
    /// Whether a point lies in a closed exact boundary.
    #[schemars(example = json!({"kind":"contains","target":{"handle":1234},"point":[0,0]}))]
    Contains { target: Target, point: [f64; 2] },
    /// The closest exact point on the supplied targets.
    #[schemars(example = json!({"kind":"nearest","targets":[{"handle":1234}],"point":[0,0]}))]
    Nearest {
        targets: Vec<Target>,
        point: [f64; 2],
    },
    /// Exact crossing points between two targets.
    #[schemars(example = json!({"kind":"intersections","a":{"handle":1234},"b":{"handle":1235}}))]
    Intersections { a: Target, b: Target },
}

fn invalid(message: &str) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

/// Pieces can arrive in any order. Every boundary vertex must have an even degree;
/// a single open line must never be treated as an enclosing half-plane.
pub(crate) fn closed_boundary(curves: &[Curve], tolerance: f64) -> bool {
    let ends: Vec<_> = curves
        .iter()
        .flat_map(|c| [c.point_at(0.), c.point_at(1.)])
        .collect();
    !ends.is_empty()
        && ends.iter().all(|p| {
            ends.iter()
                .filter(|q| (p[0] - q[0]).hypot(p[1] - q[1]) <= tolerance)
                .count()
                % 2
                == 0
        })
}

impl Session {
    fn spatial_curves(&self, target: &Target) -> Result<Vec<Curve>, CoreError> {
        let xrefs = self.xref_docs();
        let located =
            crate::session::locate(&self.drawing.doc, &xrefs, target.handle, &target.chain)?;
        let shapes =
            qiyas_geom::snap_shapes(located.doc, located.entity, &located.xf, self.measure_tol());
        if shapes.is_empty() || shapes.iter().any(|s| !s.exact || s.pieces.is_empty()) {
            return Err(invalid("This target has no exact curves for a spatial query. Use lines, arcs, circles, ellipses or planar polylines."));
        }
        Ok(shapes.into_iter().flat_map(|s| s.pieces).collect())
    }

    pub fn spatial_query(&self, query: &Query) -> Result<Value, CoreError> {
        let tolerance = Tolerance::new(self.measure_tol());
        let factor = self.drawing.units.to_metres();
        let valid_point = |p: &[f64; 2]| {
            if p.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err(invalid("The query point must be finite"))
            }
        };
        let result = match query {
            Query::Contains { target, point } => {
                valid_point(point)?;
                let curves = self.spatial_curves(target)?;
                // A closed circle has identical ends; a polyline's consecutive pieces meet.
                // Open curves cannot prove containment, even if ray casting would find a hit.
                if !closed_boundary(&curves, self.measure_tol()) {
                    return Err(invalid("Contains needs a closed exact boundary"));
                }
                json!({"contains":contains(&curves,*point,tolerance),"target":target,"point":point})
            }
            Query::Nearest { targets, point } => {
                valid_point(point)?;
                let mut best: Option<(Target, opencadkernel::geom2d::Closest)> = None;
                for target in targets {
                    for curve in self.spatial_curves(target)? {
                        let closest = closest_point(&curve, *point);
                        if best
                            .as_ref()
                            .is_none_or(|(_, old)| closest.distance < old.distance)
                        {
                            best = Some((target.clone(), closest));
                        }
                    }
                }
                let (target, closest) =
                    best.ok_or_else(|| invalid("Nearest needs at least one exact target"))?;
                json!({"target":target,"point":closest.point,"distance":closest.distance,"distance_m":closest.distance*factor})
            }
            Query::Intersections { a, b } => {
                let first = self.spatial_curves(a)?;
                let second = self.spatial_curves(b)?;
                let mut points: Vec<[f64; 2]> = Vec::new();
                for x in &first {
                    for y in &second {
                        for crossing in intersect(x, y, tolerance) {
                            let p = crossing.point;
                            if !points
                                .iter()
                                .any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) <= self.measure_tol())
                            {
                                points.push(p);
                            }
                        }
                    }
                }
                json!({"a":a,"b":b,"count":points.len(),"points":points})
            }
        };
        Ok(
            json!({"result":result,"tolerance_m":self.measure_tol()*factor,"coordinates":"drawing_units","metres_per_unit":factor,"exact_geometry":true}),
        )
    }
}
