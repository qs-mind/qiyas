//! Room enclosure sources are independent of plotted annotation and layer visibility.
use super::*;
use crate::object_regions::BoundaryLayers;
use std::collections::BTreeMap;

pub(crate) fn is_grid(entity: &EntityType) -> bool {
    entity
        .common()
        .extended_data
        .get_record(crate::conventions::GRID_APP)
        .is_some()
}

impl Session {
    fn room_source(&self, doc: &opencadcodec::CadDocument, entity: &EntityType) -> bool {
        if matches!(
            entity,
            EntityType::Text(_)
                | EntityType::MText(_)
                | EntityType::AttributeDefinition(_)
                | EntityType::AttributeEntity(_)
                | EntityType::Dimension(_)
                | EntityType::Leader(_)
                | EntityType::MultiLeader(_)
                | EntityType::Table(_)
                | EntityType::Point(_)
                | EntityType::Hatch(_)
                | EntityType::Wipeout(_)
                | EntityType::Ray(_)
                | EntityType::XLine(_)
                | EntityType::Tolerance(_)
        ) || is_grid(entity)
            || entity.common().extended_data.get_record("QIYAS").is_some()
            || crate::walls::wall_tag(entity).is_some()
            || crate::object_regions::retained_region(entity).is_some()
            || crate::room_objects::is_room_boundary(entity)
        {
            return false;
        }
        let owner = crate::xdata::read_json::<crate::objects::ObjectPart>(
            entity.common(),
            crate::objects::PART_APP,
        )
        .and_then(|part| doc.get_entity(Handle::new(part.owner)))
        .unwrap_or(entity);
        if crate::object_regions::retained_region(owner).is_some() {
            return false;
        }
        if crate::objects::instance(owner)
            .and_then(|instance| self.object_type(&instance.type_name).ok())
            .is_some_and(|definition| {
                definition.category == "opening"
                    || (definition.geometry == crate::objects::GeometryKind::Hosted
                        && definition.host_cut.is_some())
            })
        {
            return false;
        }
        true
    }

    pub(crate) fn room_boundary_in(
        &self,
        sheet: &str,
        point: [f64; 2],
        policy: &BoundaryLayers,
    ) -> Boundary {
        if !point.iter().all(|v| v.is_finite()) {
            return Boundary::None;
        }
        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        let mut segments = Vec::new();
        let mut rounds = Vec::new();
        let mut walls = BTreeMap::new();
        let mut budget = qiyas_geom::MAX_WORK;
        let sheet = self.sheet_block(Some(sheet));
        let mut add_path = |points: &[[f64; 2]], closed: bool| {
            for pair in points.windows(2) {
                segments.push([pair[0][0], pair[0][1], pair[1][0], pair[1][1]]);
            }
            if closed && points.len() > 2 {
                let a = points[points.len() - 1];
                let b = points[0];
                segments.push([a[0], a[1], b[0], b[1]]);
            }
        };
        for entity in doc.entities_in_block(&sheet) {
            if let Some(tag) = crate::walls::wall_tag(entity) {
                if policy.accepts(&crate::walls::resolved_layers(&tag.spec.layers)[0]) {
                    walls.entry(tag.group).or_insert(tag.spec);
                }
                continue;
            }
            if !self.room_source(doc, entity) {
                continue;
            }
            let flat = flatten_continuous_within(
                doc,
                &xrefs,
                entity,
                &Xform::identity(),
                self.measure_tol(),
                &mut budget,
            );
            if flat.truncated {
                return Boundary::TooDense;
            }
            for path in flat.paths {
                if path.text.is_some()
                    || path.paint != Paint::Outline
                    || !policy.accepts(&path.layer)
                {
                    continue;
                }
                if path
                    .chain
                    .iter()
                    .filter_map(|h| doc.get_entity(Handle::new(*h)))
                    .any(|e| !self.room_source(doc, e))
                {
                    continue;
                }
                let Ok(located) = locate(doc, &xrefs, path.entity, &path.chain) else {
                    continue;
                };
                if !self.room_source(located.doc, located.entity) {
                    continue;
                }
                add_path(&path.path.points, path.path.closed);
                for piece in
                    snap_shapes(located.doc, located.entity, &located.xf, self.measure_tol())
                        .iter()
                        .filter(|s| s.exact)
                        .flat_map(|s| &s.pieces)
                {
                    match piece {
                        Curve::Circle(c) => rounds.push(Round {
                            centre: c.centre,
                            radius: c.radius,
                            start: 0.0,
                            sweep: TAU,
                        }),
                        Curve::Arc(a) => rounds.push(Round {
                            centre: a.centre,
                            radius: a.radius,
                            start: a.start_angle,
                            sweep: a.sweep(),
                        }),
                        _ => {}
                    }
                }
            }
        }
        // Rebuild the joined wall faces without cuts. These virtual edges close doorways at
        // the internal face, while the actual drawing and Hatch keep their opening graphics.
        for spec in walls.values() {
            let Ok(wall) = qiyas_geom::walls::build_walls_justified(
                &crate::walls::geom_runs(spec),
                spec.thickness,
                &[],
                self.measure_tol(),
                crate::walls::geom_justification(spec.justification),
            ) else {
                return Boundary::None;
            };
            for region in wall.regions {
                add_path(&region.outer, true);
                for hole in region.holes {
                    add_path(&hole, true);
                }
            }
        }
        if segments.len() > BOUNDARY_SEGMENTS * 4 {
            return Boundary::TooDense;
        }
        let radius = segments
            .iter()
            .flat_map(|s| [[s[0], s[1]], [s[2], s[3]]])
            .map(|p| (p[0] - point[0]).abs().max((p[1] - point[1]).abs()))
            .fold(1.0 / self.drawing.units.to_metres(), f64::max)
            * 1.1;
        let lo = [point[0] - radius, point[1] - radius];
        let hi = [point[0] + radius, point[1] + radius];
        region_around(&segments, point, radius, lo, hi)
            .map(|found| with_bulges(found, &rounds, self.measure_tol()))
            .unwrap_or(Boundary::None)
    }
}
