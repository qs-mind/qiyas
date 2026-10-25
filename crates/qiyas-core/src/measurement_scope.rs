//! View-only geometry is drawing data. It stays editable but contributes no quantity.
use std::collections::BTreeSet;

use opencadcodec::{EntityType, Handle};
use qiyas_geom::Xform;
use serde::{Deserialize, Serialize};

use crate::knowledge::{self, Kind};
use crate::{CoreError, Measurement, Session, Target};

const RECORD: &str = "view_only";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Scope {
    Region { handle: u64 },
    Group { handle: u64 },
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

/// Built once per measurement pass; references remain live when regions or groups change.
pub(crate) struct Exclusions {
    handles: BTreeSet<u64>,
    regions: Vec<(Handle, Vec<[f64; 2]>)>,
    tolerance: f64,
}

impl Exclusions {
    pub(crate) fn points(&self, sheet: Handle, points: &[[f64; 2]]) -> bool {
        !points.is_empty()
            && self.regions.iter().any(|(owner, ring)| {
                *owner == sheet
                    && points
                        .iter()
                        .all(|&point| inside(ring, point, self.tolerance))
                    && points.windows(2).all(|pair| {
                        // A segment can leave a concave region although its ends are inside.
                        segment_inside(ring, pair[0], pair[1], self.tolerance)
                    })
            })
    }

    pub(crate) fn entity(&self, session: &Session, handle: u64) -> bool {
        let mut next = Some(handle);
        let mut seen = BTreeSet::new();
        while let Some(handle) = next.filter(|h| seen.insert(*h)) {
            if self.handles.contains(&handle) {
                return true;
            }
            let Some(entity) = session.drawing.doc.get_entity(Handle::new(handle)) else {
                return false;
            };
            if !self.regions.is_empty() && self.entity_in_region(session, entity) {
                return true;
            }
            next = crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            )
            .map(|part| part.owner)
            .filter(|owner| *owner != handle)
            .or_else(|| crate::objects::instance(entity).and_then(|object| object.host));
        }
        false
    }

    fn entity_in_region(&self, session: &Session, entity: &EntityType) -> bool {
        let paths = qiyas_geom::flatten(
            &session.drawing.doc,
            entity,
            &Xform::identity(),
            session.measure_tol(),
        )
        .into_paths();
        if paths.is_empty() {
            let point = match entity {
                EntityType::Insert(insert) => insert.insert_point,
                EntityType::Point(point) => point.location,
                _ => return false,
            };
            return self.points(entity.common().owner_handle, &[[point.x, point.y]]);
        }
        paths.into_iter().all(|mut path| {
            if path.closed {
                if let Some(first) = path.points.first().copied() {
                    path.points.push(first);
                }
            }
            self.points(entity.common().owner_handle, &path.points)
        })
    }

    pub(crate) fn measurement(&self, session: &Session, row: &Measurement) -> bool {
        // A mixed manual selection must be split before it can be measured safely.
        row.chain.first().is_some_and(|h| self.entity(session, *h))
            || row.handles.iter().any(|h| self.entity(session, *h))
            || session
                .drawing
                .doc
                .block_records
                .get(&row.sheet)
                .is_some_and(|sheet| self.points(sheet.handle, &row.points))
    }
}

impl Session {
    pub(crate) fn has_measurement_scope(&self) -> bool {
        knowledge::get(&self.drawing.doc, Kind::Measurement, RECORD)
            .is_ok_and(|entry| entry.is_some())
    }

    pub(crate) fn measurement_exclusions(&self) -> Result<Exclusions, CoreError> {
        let scopes: Vec<Scope> = knowledge::get(&self.drawing.doc, Kind::Measurement, RECORD)?
            .map(|entry| serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string())))
            .transpose()?
            .unwrap_or_default();
        let mut exclusions = Exclusions {
            handles: BTreeSet::new(),
            regions: Vec::new(),
            tolerance: self.measure_tol(),
        };
        for scope in scopes {
            match scope {
                Scope::Group { handle } => {
                    if let Some(opencadcodec::objects::ObjectType::Group(group)) =
                        self.drawing.doc.objects.get(&Handle::new(handle))
                    {
                        exclusions
                            .handles
                            .extend(group.entities.iter().map(|h| h.value()));
                    }
                }
                Scope::Region { handle } => {
                    let Some(entity) = self.drawing.doc.get_entity(Handle::new(handle)) else {
                        continue;
                    };
                    let paths = qiyas_geom::flatten(
                        &self.drawing.doc,
                        entity,
                        &Xform::identity(),
                        self.measure_tol(),
                    )
                    .into_paths();
                    let ring = paths.into_iter().find(|path| path.closed && path.points.len() >= 3)
                        .ok_or_else(|| invalid("A view-only region is no longer closed. Close it or remove its measurement marker."))?;
                    exclusions.handles.insert(handle);
                    exclusions
                        .regions
                        .push((entity.common().owner_handle, ring.points));
                }
            }
        }
        Ok(exclusions)
    }

    pub(crate) fn set_measurement_scope(
        &mut self,
        region: Option<Target>,
        group: Option<String>,
        measured: bool,
    ) -> Result<(), CoreError> {
        let scope = match (region, group) {
            (Some(target), None) => {
                self.sheet_targets(std::slice::from_ref(&target))?;
                if !measured {
                    let entity = self
                        .drawing
                        .doc
                        .get_entity(Handle::new(target.handle))
                        .ok_or(CoreError::NoSuchEntity(target.handle))?;
                    let paths = qiyas_geom::flatten(
                        &self.drawing.doc,
                        entity,
                        &Xform::identity(),
                        self.measure_tol(),
                    )
                    .into_paths();
                    if !paths.iter().any(|path| {
                        path.closed && path.points.len() >= 3 && !path.self_intersects()
                    }) {
                        return Err(invalid(
                            "Use a closed, non-crossing region for a view-only marker.",
                        ));
                    }
                }
                Scope::Region {
                    handle: target.handle,
                }
            }
            (None, Some(name)) => {
                let (_, handle) = self.find_group(&name)?;
                Scope::Group {
                    handle: handle.value(),
                }
            }
            _ => {
                return Err(invalid(
                    "Choose one region or one group for the measurement marker.",
                ))
            }
        };
        let mut scopes: Vec<Scope> = knowledge::get(&self.drawing.doc, Kind::Measurement, RECORD)?
            .map(|entry| serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string())))
            .transpose()?
            .unwrap_or_default();
        scopes.retain(|existing| existing != &scope);
        if !measured {
            scopes.push(scope);
        }
        if scopes.is_empty() {
            knowledge::remove(&mut self.drawing.doc, Kind::Measurement, RECORD)?;
        } else {
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Measurement,
                RECORD,
                &serde_json::json!(scopes),
            )?;
        }
        self.refresh_measurement_scope()
    }

    pub(crate) fn refresh_measurement_scope(&mut self) -> Result<(), CoreError> {
        let exclusions = self.measurement_exclusions()?;
        let flags: Vec<_> = self
            .takeoff
            .items
            .iter()
            .map(|row| exclusions.measurement(self, row))
            .collect();
        for (row, view_only) in self.takeoff.items.iter_mut().zip(flags) {
            row.view_only = view_only;
        }
        Ok(())
    }

    pub(crate) fn require_measured(&self, row: &Measurement) -> Result<(), CoreError> {
        if self.measurement_exclusions()?.measurement(self, row) {
            return Err(invalid("This selection includes view-only geometry. Remove its measurement marker or measure the other geometry separately."));
        }
        Ok(())
    }
}

fn inside(ring: &[[f64; 2]], point: [f64; 2], tolerance: f64) -> bool {
    let mut inside = false;
    for (&a, &b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        let length2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
        if length2 > 0.0 {
            let t = (((point[0] - a[0]) * (b[0] - a[0]) + (point[1] - a[1]) * (b[1] - a[1]))
                / length2)
                .clamp(0.0, 1.0);
            if (point[0] - a[0] - t * (b[0] - a[0])).hypot(point[1] - a[1] - t * (b[1] - a[1]))
                <= tolerance
            {
                return true;
            }
        }
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn segment_inside(ring: &[[f64; 2]], a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    let d = [b[0] - a[0], b[1] - a[1]];
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let mut cuts = vec![0.0, 1.0];
    for (&p, &q) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        let e = [q[0] - p[0], q[1] - p[1]];
        let denominator = cross(d, e);
        if denominator.abs() <= f64::EPSILON {
            continue;
        }
        let relative = [p[0] - a[0], p[1] - a[1]];
        let t = cross(relative, e) / denominator;
        let u = cross(relative, d) / denominator;
        if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
            cuts.push(t);
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).all(|pair| {
        let t = (pair[0] + pair[1]) * 0.5;
        inside(ring, [a[0] + t * d[0], a[1] + t * d[1]], tolerance)
    })
}
