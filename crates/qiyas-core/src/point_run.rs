//! Point sequences share their wire shape, unit-aware endpoint comparison and closure policy.
//! The const parameter declares whether returning to the start closes this run. Open paths
//! retain their last vertex; boundaries remove a repeated endpoint before making their edges.

use std::ops::{Deref, DerefMut};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};

use crate::{Command, DrawSpec, Kind, MarkupKind, Units, WallSpec};

/// A path or boundary in drawing coordinates. `CLOSE` declares what returning to the start
/// means; it does not force an unfinished wall run to close.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PointRun<const CLOSE: bool = false>(Vec<[f64; 2]>);

impl<const CLOSE: bool> JsonSchema for PointRun<CLOSE> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        if CLOSE {
            "ClosingPointRun"
        } else {
            "OpenPointRun"
        }
        .into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let mut schema = Vec::<[f64; 2]>::json_schema(generator);
        schema.insert("description".into(), if CLOSE {
            "Path in drawing coordinates. Returning to the start within the unit-aware same-point tolerance drops the repeated point and closes the boundary."
        } else {
            "Open path in drawing coordinates. Returning to the start keeps the last vertex and does not close the path; explicit closed=true removes the repeated endpoint."
        }.into());
        schema
    }
}

impl<const CLOSE: bool> From<Vec<[f64; 2]>> for PointRun<CLOSE> {
    fn from(points: Vec<[f64; 2]>) -> Self {
        Self(points)
    }
}

impl<const CLOSE: bool> From<PointRun<CLOSE>> for Vec<[f64; 2]> {
    fn from(run: PointRun<CLOSE>) -> Self {
        run.0
    }
}

impl<const CLOSE: bool> Deref for PointRun<CLOSE> {
    type Target = Vec<[f64; 2]>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const CLOSE: bool> DerefMut for PointRun<CLOSE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const CLOSE: bool> IntoIterator for PointRun<CLOSE> {
    type Item = [f64; 2];
    type IntoIter = std::vec::IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, const CLOSE: bool> IntoIterator for &'a PointRun<CLOSE> {
    type Item = &'a [f64; 2];
    type IntoIter = std::slice::Iter<'a, [f64; 2]>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<const CLOSE: bool> FromIterator<[f64; 2]> for PointRun<CLOSE> {
    fn from_iter<T: IntoIterator<Item = [f64; 2]>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl<const CLOSE: bool> PointRun<CLOSE> {
    /// The absolute SAME_POINT convention uses millimetres; the relative tolerance also
    /// protects distant survey coordinates. Comparison uses both endpoints' coordinate size.
    pub fn returns_to_start(&self, units: Units) -> bool {
        let (Some(a), Some(b)) = (self.first(), self.last()) else {
            return false;
        };
        let size = a.iter().chain(b).fold(0.0_f64, |size, v| size.max(v.abs()));
        let tolerance =
            qiyas_geom::tol_near(size, qiyas_geom::SAME_POINT * 0.001 / units.to_metres());
        self.len() > 2 && (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance
    }

    /// Apply this input's declared policy and return its effective closed flag.
    pub fn normalize(&mut self, units: Units, closed: bool) -> bool {
        if self.returns_to_start(units) && (CLOSE || closed) {
            self.pop();
            true
        } else {
            closed
        }
    }
}

impl DrawSpec {
    pub(crate) fn normalize_runs(&mut self, units: Units) {
        match self {
            Self::Polyline {
                points,
                closed,
                bulges,
            } => {
                let before = points.len();
                *closed = points.normalize(units, *closed);
                if points.len() != before && bulges.len() == before {
                    bulges.pop();
                }
            }
            Self::Hatch {
                points,
                holes,
                bulges,
                ..
            } => {
                for (i, run) in std::iter::once(points).chain(holes).enumerate() {
                    let before = run.len();
                    run.normalize(units, true);
                    if run.len() != before {
                        if let Some(values) =
                            bulges.get_mut(i).filter(|values| values.len() == before)
                        {
                            values.pop();
                        }
                    }
                }
            }
            Self::RevCloud { points, .. } | Self::Wipeout { points } | Self::Solid2D { points } => {
                points.normalize(units, true);
            }
            // These commands deliberately retain a return vertex; their type's schema says so.
            Self::Spline { .. } | Self::Trace { .. } | Self::MLine { .. } | Self::Sketch { .. } => {
            }
            _ => {}
        }
    }
}

impl WallSpec {
    pub(crate) fn normalize_runs(&mut self, units: Units) {
        for run in &mut self.runs {
            run.closed = run.points.normalize(units, run.closed);
        }
    }
}

impl Command {
    /// Normalize path inputs once, before any caller executes this command. Drawing primitives
    /// and walls repeat the operation at their internal entry points for generated geometry.
    pub(crate) fn normalize_runs(&mut self, units: Units) {
        match self {
            Self::Draw(spec) => spec.normalize_runs(units),
            Self::Walls(spec) => spec.normalize_runs(units),
            Self::MeasureArea { points } | Self::Comment { points, .. } => {
                points.normalize(units, true);
            }
            Self::AddMeasurement {
                points,
                kind: Kind::Area | Kind::Volume,
                ..
            }
            | Self::AddMarkup {
                points,
                kind: MarkupKind::Cloud,
                ..
            } => {
                points.normalize(units, true);
            }
            _ => {}
        }
    }
}
