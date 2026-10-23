//! Retained geometry from a seeded enclosure. The binding has no trade or quantity rules.

use opencadcodec::{CadDocument, EntityType, Handle, LwPolyline, Vector2};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Boundary, CoreError, Kind, Session};

pub(crate) const REGION_APP: &str = "QIYAS_OBJECT_REGION";

fn subtract_islands() -> bool {
    true
}

/// Explicit enclosure layers for imported drawings. Empty includes every geometric layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundaryLayers {
    #[serde(default)]
    pub boundary_layers: Vec<String>,
    #[serde(default)]
    pub exclude_layers: Vec<String>,
}
impl BoundaryLayers {
    pub(crate) fn accepts(&self, layer: &str) -> bool {
        (self.boundary_layers.is_empty()
            || self
                .boundary_layers
                .iter()
                .any(|s| s.eq_ignore_ascii_case(layer)))
            && !self
                .exclude_layers
                .iter()
                .any(|s| s.eq_ignore_ascii_case(layer))
    }
}

/// Follow the closed drawing geometry surrounding a point, including its island policy.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RegionSource {
    #[serde(default, flatten)]
    pub boundary_layers: BoundaryLayers,
    pub seed: [f64; 2],
    #[serde(default = "subtract_islands")]
    pub subtract_islands: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RetainedRegion {
    #[serde(default, flatten)]
    pub boundary_layers: BoundaryLayers,
    pub seed: [f64; 2],
    pub subtract_islands: bool,
    pub holes: Vec<Vec<[f64; 2]>>,
    pub hole_bulges: Vec<Vec<f64>>,
    pub valid: bool,
    #[serde(default)]
    pub app_handle: u64,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

pub(crate) fn retained_region(entity: &EntityType) -> Option<RetainedRegion> {
    crate::xdata::read_json(entity.common(), REGION_APP)
}

pub(crate) fn valid_region_metadata(region: &RetainedRegion) -> bool {
    region.seed.iter().all(|coordinate| coordinate.is_finite())
        && region
            .holes
            .iter()
            .flatten()
            .flatten()
            .all(|coordinate| coordinate.is_finite())
        && region
            .hole_bulges
            .iter()
            .flatten()
            .all(|bulge| bulge.is_finite())
        && region.hole_bulges.len() <= region.holes.len()
        && region.holes.iter().enumerate().all(|(index, hole)| {
            let bulges: &[f64] = region.hole_bulges.get(index).map_or(&[], Vec::as_slice);
            let area = crate::rooms::ring_area(hole, bulges);
            hole.len() >= 2
                && (bulges.is_empty() || bulges.len() == hole.len())
                && area.is_finite()
                && area > 0.0
        })
}

pub(crate) fn transform_region_source(
    entity: &mut EntityType,
    transform: &opencadcodec::types::Transform,
) {
    let Some(mut region) = retained_region(entity) else {
        return;
    };
    let place = |point: &mut [f64; 2]| {
        let result = transform.apply(opencadcodec::Vector3::new(point[0], point[1], 0.0));
        *point = [result.x, result.y];
    };
    place(&mut region.seed);
    for point in region.holes.iter_mut().flatten() {
        place(point);
    }
    let x = transform.apply_rotation(opencadcodec::Vector3::UNIT_X);
    let y = transform.apply_rotation(opencadcodec::Vector3::UNIT_Y);
    if x.x * y.y - x.y * y.x < 0.0 {
        for bulge in region.hole_bulges.iter_mut().flatten() {
            *bulge = -*bulge;
        }
    }
    crate::xdata::write_json(entity.common_mut(), REGION_APP, region.app_handle, &region)
        .expect("finite transformed region source");
}

pub(crate) fn region_polyline(ring: &[[f64; 2]], bulges: &[f64]) -> LwPolyline {
    let mut polyline = LwPolyline::new();
    polyline.is_closed = true;
    for (index, point) in ring.iter().enumerate() {
        polyline.add_point_with_bulge(
            Vector2::new(point[0], point[1]),
            bulges.get(index).copied().unwrap_or(0.0),
        );
    }
    polyline
}

/// Exact area in drawing units squared, with curved edges and the retained island policy.
pub(crate) fn retained_region_area(entity: &EntityType) -> Option<f64> {
    let region = retained_region(entity)?;
    let EntityType::LwPolyline(polyline) = entity else {
        return None;
    };
    if !region.valid || !polyline.is_closed {
        return None;
    }
    let ring: Vec<_> = polyline
        .vertices
        .iter()
        .map(|vertex| [vertex.location.x, vertex.location.y])
        .collect();
    let bulges: Vec<_> = polyline
        .vertices
        .iter()
        .map(|vertex| vertex.bulge)
        .collect();
    let gross = crate::rooms::ring_area(&ring, &bulges);
    let islands = if region.subtract_islands {
        region
            .holes
            .iter()
            .enumerate()
            .map(|(index, hole)| {
                crate::rooms::ring_area(
                    hole,
                    region.hole_bulges.get(index).map_or(&[], Vec::as_slice),
                )
            })
            .sum::<f64>()
    } else {
        0.0
    };
    Some((gross - islands).max(0.0))
}

/// Drawing recipes use the same islands as the exact quantity calculation.
pub(crate) fn region_graphics(
    document: &CadDocument,
    entity: &EntityType,
    tolerance: f64,
) -> Option<qiyas_geom::walls::Region> {
    let region = retained_region(entity)?;
    if !region.valid {
        return None;
    }
    let path = |entity: &EntityType| {
        qiyas_geom::flatten(document, entity, &qiyas_geom::Xform::identity(), tolerance)
            .into_paths()
            .into_iter()
            .find(|path| path.closed)
            .map(|path| path.points)
    };
    let outer = path(entity)?;
    let holes = if region.subtract_islands {
        region
            .holes
            .iter()
            .enumerate()
            .filter_map(|(index, ring)| {
                path(&EntityType::LwPolyline(region_polyline(
                    ring,
                    region.hole_bulges.get(index).map_or(&[], Vec::as_slice),
                )))
            })
            .collect()
    } else {
        Vec::new()
    };
    Some(qiyas_geom::walls::Region { outer, holes })
}

impl Session {
    pub(crate) fn has_object_regions(&self) -> bool {
        self.drawing.doc.app_ids.contains(REGION_APP)
    }

    pub(crate) fn stamp_retained_region(
        &mut self,
        handle: u64,
        region: &RetainedRegion,
    ) -> Result<(), CoreError> {
        let app = self.object_app(REGION_APP)?;
        let mut region = region.clone();
        region.app_handle = app;
        let entity = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(handle))
            .ok_or(CoreError::NoSuchEntity(handle))?;
        crate::xdata::write_json(entity.common_mut(), REGION_APP, app, &region)
    }

    pub(crate) fn bind_object_region(
        &mut self,
        handle: u64,
        source: RegionSource,
    ) -> Result<(), CoreError> {
        if !source.seed.iter().all(|coordinate| coordinate.is_finite()) {
            return Err(invalid("A region seed must have finite coordinates"));
        }
        let entity = self
            .drawing
            .doc
            .get_entity(Handle::new(handle))
            .ok_or(CoreError::NoSuchEntity(handle))?;
        if !matches!(entity, EntityType::LwPolyline(_)) {
            return Err(invalid("A retained region requires a polyline driver"));
        }
        self.stamp_retained_region(
            handle,
            &RetainedRegion {
                seed: source.seed,
                boundary_layers: source.boundary_layers,
                subtract_islands: source.subtract_islands,
                holes: Vec::new(),
                hole_bulges: Vec::new(),
                valid: true,
                app_handle: 0,
            },
        )?;
        self.park();
        self.refresh_object_regions()?;
        if self
            .drawing
            .doc
            .get_entity(Handle::new(handle))
            .and_then(retained_region)
            .is_none_or(|region| !region.valid)
        {
            return Err(invalid(
                "The region seed is not inside a closed drawn enclosure",
            ));
        }
        Ok(())
    }

    /// Follow source geometry inside the current command's entity and table snapshots.
    pub(crate) fn refresh_object_regions(&mut self) -> Result<(), CoreError> {
        self.refresh_object_regions_scoped(None)
    }
    pub(crate) fn refresh_object_regions_scoped(
        &mut self,
        only: Option<&std::collections::BTreeSet<u64>>,
    ) -> Result<(), CoreError> {
        if !self.has_object_regions() {
            return Ok(());
        }
        let regions: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                if only.is_some_and(|ids| !ids.contains(&entity.common().handle.value())) {
                    return None;
                }
                let region = retained_region(entity)?;
                let sheet = self
                    .drawing
                    .doc
                    .block_records
                    .iter()
                    .find(|block| block.handle == entity.common().owner_handle)?
                    .name
                    .clone();
                Some((entity.common().handle, region, sheet))
            })
            .collect();
        let app = self.object_app(REGION_APP)?;
        for (handle, mut region, sheet) in regions {
            region.app_handle = app;
            match self.room_boundary_in(&sheet, region.seed, &region.boundary_layers) {
                Boundary::Ring {
                    ring,
                    holes,
                    bulges,
                } => {
                    region.holes = holes;
                    region.hole_bulges = bulges.iter().skip(1).cloned().collect();
                    region.valid = true;
                    let mut replacement =
                        region_polyline(&ring, bulges.first().map_or(&[], Vec::as_slice));
                    replacement.common = self
                        .drawing
                        .doc
                        .get_entity(handle)
                        .expect("listed region")
                        .common()
                        .clone();
                    replacement.common.raw_record = None;
                    replacement.common.graphic_data = None;
                    crate::xdata::write_json(&mut replacement.common, REGION_APP, app, &region)?;
                    let entity = EntityType::LwPolyline(replacement);
                    if self.drawing.doc.get_entity(handle) != Some(&entity) {
                        *self
                            .drawing
                            .doc
                            .get_entity_mut(handle)
                            .expect("listed region") = entity;
                    }
                }
                Boundary::None | Boundary::TooDense => {
                    region.valid = false;
                    // Imported metadata may still name the source drawing's application
                    // handle. Restamp invalid regions too, so later transforms cannot retain
                    // stale DWG EED under the destination application's handle.
                    self.stamp_retained_region(handle.value(), &region)?;
                }
            }
        }
        let quantities: Vec<_> = self
            .takeoff
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, row)| {
                if row.kind != Kind::Area || row.handles.len() != 1 {
                    return None;
                }
                let entity = self.drawing.doc.get_entity(Handle::new(row.handles[0]))?;
                retained_region(entity)?;
                Some((index, retained_region_area(entity)))
            })
            .collect();
        for (index, area) in quantities {
            let row = &mut self.takeoff.items[index];
            row.quantity_valid = area.is_some();
            if let Some(area) = area {
                row.value = area;
                row.shown = area * self.drawing.units.to_metres().powi(2);
                row.note =
                    "Unapproved candidate region area; linked to the retained boundary".into();
            } else {
                row.value = f64::NAN;
                row.shown = f64::NAN;
                row.note =
                    "Region enclosure is no longer closed; quantity unavailable and excluded from totals"
                        .into();
            }
        }
        Ok(())
    }
}
