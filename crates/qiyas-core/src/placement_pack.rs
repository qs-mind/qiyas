//! Optional spatial checks. Trade choices, regions, tolerances and citations are editable data.
use crate::{
    check::{Finding, Severity},
    knowledge::{self, Kind},
    objects::{self, ObjectType},
    CoreError, Session,
};
use opencadcodec::{EntityType, Handle};
use opencadkernel::geom2d::{contains, distance_to, intersect, Curve, Line, Tolerance};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    pub name: String,
    pub citation: String,
    pub zone: String,
    pub relation: Relation,
    pub tolerance_m: f64,
    #[serde(default)]
    pub against_zones: Vec<String>,
    #[serde(default)]
    pub against_entities: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Relation {
    TouchHost,
    Disjoint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pack {
    name: String,
    citations: Vec<String>,
    types: Vec<ObjectType>,
    #[serde(default)]
    zones_for_types: BTreeMap<String, BTreeMap<String, Vec<Value>>>,
    checks: Vec<Rule>,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

pub(crate) fn validate_checks(value: &Value) -> Result<Vec<Rule>, CoreError> {
    let rules: Vec<Rule> =
        serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
    if rules.is_empty() || rules.len() > 100 {
        return Err(invalid("A placement pack needs 1 to 100 checks"));
    }
    for rule in &rules {
        if rule.name.trim().is_empty()
            || rule.citation.trim().is_empty()
            || rule.zone.trim().is_empty()
            || !rule.tolerance_m.is_finite()
            || rule.tolerance_m <= 0.
        {
            return Err(invalid(
                "Each placement check needs a name, citation, zone and positive tolerance_m",
            ));
        }
        if matches!(rule.relation, Relation::Disjoint)
            && rule.against_zones.is_empty()
            && rule.against_entities.is_empty()
        {
            return Err(invalid(
                "A disjoint check must name the zones or entity types to compare",
            ));
        }
    }
    Ok(rules)
}

pub fn shipped_pack(name: &str) -> Result<Value, CoreError> {
    match name {
        "interiors" => serde_json::from_str(include_str!("../data/interiors-pack.json"))
            .map_err(|e| invalid(e.to_string())),
        _ => Err(invalid(
            "Unknown object pack. Available: interiors. Or supply your own pack data.",
        )),
    }
}

impl Session {
    pub(crate) fn install_object_pack(&mut self, value: Value) -> Result<(), CoreError> {
        let pack: Pack =
            serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
        if pack.name.trim().is_empty() || pack.citations.is_empty() {
            return Err(invalid("A pack needs a name and citations"));
        }
        validate_checks(&json!(pack.checks))?;
        for definition in pack.types {
            if let Some(existing) =
                knowledge::get(&self.drawing.doc, Kind::Types, &definition.name)?
            {
                let stored: ObjectType =
                    serde_json::from_value(existing.data).map_err(|e| invalid(e.to_string()))?;
                if json!(stored) != json!(definition) {
                    return Err(invalid(format!(
                        "Type {} already has different data. Review it before redefining it.",
                        definition.name
                    )));
                }
            } else {
                self.define_object_type(definition, false)?;
            }
        }
        for (name, zones) in pack.zones_for_types {
            if knowledge::get(&self.drawing.doc, Kind::Types, &name)?.is_none() {
                continue;
            }
            let mut definition = self.object_type(&name)?;
            for (zone, recipe) in zones {
                if definition
                    .plan
                    .zones
                    .get(&zone)
                    .is_some_and(|old| old != &recipe)
                {
                    return Err(invalid(format!(
                        "Zone {zone} in {name} has different data; review it first"
                    )));
                }
                definition.plan.zones.insert(zone, recipe);
            }
            self.define_object_type(definition, true)?;
        }
        knowledge::put(
            &mut self.drawing.doc,
            Kind::Placement,
            &pack.name,
            &json!(pack.checks),
        )?;
        knowledge::put(
            &mut self.drawing.doc,
            Kind::Mappings,
            &format!("pack:{}", pack.name),
            &value,
        )?;
        Ok(())
    }

    pub(crate) fn has_placement_pack(&self) -> bool {
        knowledge::list(&self.drawing.doc, Kind::Placement)
            .map_or(true, |entries| !entries.is_empty())
    }

    pub fn object_zones(&self, targets: &[crate::Target]) -> Result<Value, CoreError> {
        let mut rows = Vec::new();
        for target in targets {
            if !target.chain.is_empty() {
                return Err(invalid(
                    "Object zones currently require a local top-level object",
                ));
            }
            let object = self
                .drawing
                .doc
                .get_entity(Handle::new(target.handle))
                .and_then(objects::instance)
                .ok_or_else(|| invalid("The target is not an object"))?;
            let definition = self.object_type(&object.type_name)?;
            for (name, entities) in self.object_zone_entities(&object, &definition)? {
                let mut bounds = None::<[[f64; 2]; 2]>;
                let mut exact = true;
                for entity in &entities {
                    let shapes = qiyas_geom::snap_shapes(
                        &self.drawing.doc,
                        entity,
                        &qiyas_geom::Xform::identity(),
                        self.measure_tol(),
                    );
                    exact &= !shapes.is_empty() && shapes.iter().all(|s| s.exact);
                    let curves: Vec<_> = shapes.into_iter().flat_map(|s| s.pieces).collect();
                    if let Some((lo, hi)) = opencadkernel::geom2d::analytic_curve_bounds(&curves) {
                        bounds = Some(bounds.map_or([lo, hi], |[a, b]| {
                            [
                                [a[0].min(lo[0]), a[1].min(lo[1])],
                                [b[0].max(hi[0]), b[1].max(hi[1])],
                            ]
                        }));
                    }
                }
                rows.push(json!({"handle":object.id,"host":object.host,"name":name,"bounds":bounds,"exact":exact,"geometry":entities}));
            }
        }
        Ok(json!({"rows":rows,"metres_per_unit":self.drawing.units.to_metres()}))
    }

    pub(crate) fn placement_findings(&self, sheet: &str) -> Result<Vec<Finding>, CoreError> {
        let mut checks = Vec::new();
        for entry in knowledge::list(&self.drawing.doc, Kind::Placement)? {
            checks.extend(validate_checks(&entry.data)?);
        }
        if checks.is_empty() {
            return Err(invalid("No placement check data is installed"));
        }
        let objects: Vec<_> = self
            .drawing
            .entities_in(sheet)
            .filter_map(objects::instance)
            .collect();
        let mut zones = Vec::new();
        for object in &objects {
            let definition = self.object_type(&object.type_name)?;
            for (name, entities) in self.object_zone_entities(object, &definition)? {
                let curves = self.exact_zone_curves(&entities)?;
                zones.push((object.id, object.host, name, curves));
            }
        }
        let mut findings = Vec::new();
        for rule in checks {
            let tol = rule.tolerance_m / self.drawing.units.to_metres();
            let tolerance = Tolerance::new(tol);
            for (owner, host, _, source) in zones.iter().filter(|z| z.2 == rule.zone) {
                match rule.relation {
                    Relation::TouchHost => {
                        let wall = host
                            .and_then(|id| self.drawing.doc.get_entity(Handle::new(id)))
                            .and_then(objects::instance);
                        let faces = if let Some(wall) = &wall {
                            let definition = self.object_type(&wall.type_name)?;
                            let band = self
                                .band_geometry(wall, &definition, None)?
                                .ok_or_else(|| invalid("The host has no band faces"))?;
                            band.geometry
                                .regions
                                .into_iter()
                                .flat_map(|r| std::iter::once(r.outer).chain(r.holes))
                                .flat_map(|ring| {
                                    let mut lines = Vec::new();
                                    for i in 0..ring.len() {
                                        lines.push(Curve::Line(Line {
                                            start: ring[i],
                                            end: ring[(i + 1) % ring.len()],
                                        }));
                                    }
                                    lines
                                })
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        };
                        if source.is_empty()
                            || !source.iter().all(|line| covered(line, &faces, tol))
                        {
                            findings.push(finding(
                                &rule,
                                vec![*owner].into_iter().chain(*host).collect(),
                                source.first().map(|c| c.point_at(0.)),
                                "The back region is not fully touching its host face",
                            ));
                        }
                    }
                    Relation::Disjoint => {
                        let mut clashes = std::collections::BTreeSet::new();
                        for (other, _, name, region) in &zones {
                            if owner != other
                                && rule.against_zones.contains(name)
                                && overlaps(source, region, tolerance)
                            {
                                clashes.insert(*other);
                            }
                        }
                        for entity in self.drawing.entities_in(sheet) {
                            if !rule.against_entities.iter().any(|name| {
                                name.eq_ignore_ascii_case(&crate::session::variant_name(entity))
                            }) {
                                continue;
                            }
                            let handle = entity.common().handle.value();
                            let part: Option<objects::ObjectPart> =
                                crate::xdata::read_json(entity.common(), objects::PART_APP);
                            let other = part.map_or(handle, |p| p.owner);
                            if other == *owner
                                || Some(other) == *host
                                || objects::instance(entity).is_some()
                            {
                                continue;
                            }
                            let curves = self.exact_zone_curves(std::slice::from_ref(entity))?;
                            if overlaps(source, &curves, tolerance) {
                                clashes.insert(other);
                            }
                        }
                        for other in clashes {
                            findings.push(finding(
                                &rule,
                                vec![*owner, other],
                                source.first().map(|c| c.point_at(0.)),
                                "The checked region crosses an occupied region",
                            ));
                        }
                    }
                }
            }
        }
        Ok(findings)
    }

    fn exact_zone_curves(&self, entities: &[EntityType]) -> Result<Vec<Curve>, CoreError> {
        let mut curves = Vec::new();
        for entity in entities {
            let shapes = qiyas_geom::snap_shapes(
                &self.drawing.doc,
                entity,
                &qiyas_geom::Xform::identity(),
                self.measure_tol(),
            );
            if shapes.is_empty() || shapes.iter().any(|s| !s.exact) {
                return Err(invalid("A placement region could not be checked exactly. Use lines, arcs, circles or planar polylines."));
            }
            curves.extend(shapes.into_iter().flat_map(|s| s.pieces));
        }
        Ok(curves)
    }
}

fn finding(rule: &Rule, handles: Vec<u64>, at: Option<[f64; 2]>, message: &str) -> Finding {
    Finding {
        rule: "placement",
        pack: "placement",
        severity: Severity::Warning,
        handles,
        at,
        message: format!(
            "{}: {message}. Tolerance {} m. {}",
            rule.name, rule.tolerance_m, rule.citation
        ),
    }
}

fn overlaps(a: &[Curve], b: &[Curve], tolerance: Tolerance) -> bool {
    a.iter()
        .any(|x| b.iter().any(|y| !intersect(x, y, tolerance).is_empty()))
        || (crate::spatial::closed_boundary(b, tolerance.linear())
            && a.first()
                .is_some_and(|c| contains(b, c.point_at(0.), tolerance)))
        || (crate::spatial::closed_boundary(a, tolerance.linear())
            && b.first()
                .is_some_and(|c| contains(a, c.point_at(0.), tolerance)))
}

/// Complete coverage of a straight back by collinear face segments, including split faces.
fn covered(back: &Curve, faces: &[Curve], tolerance: f64) -> bool {
    let Curve::Line(line) = back else {
        return false;
    };
    let d = [line.end[0] - line.start[0], line.end[1] - line.start[1]];
    let length = d[0].hypot(d[1]);
    if length <= tolerance {
        return faces
            .iter()
            .any(|face| distance_to(face, line.start) <= tolerance);
    }
    let along =
        |p: [f64; 2]| ((p[0] - line.start[0]) * d[0] + (p[1] - line.start[1]) * d[1]) / length;
    let off = |p: [f64; 2]| {
        ((p[0] - line.start[0]) * d[1] - (p[1] - line.start[1]) * d[0]).abs() / length
    };
    let mut intervals = Vec::new();
    for face in faces {
        if let Curve::Line(f) = face {
            if off(f.start) <= tolerance && off(f.end) <= tolerance {
                let (a, b) = (along(f.start), along(f.end));
                intervals.push((a.min(b), a.max(b)));
            }
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = 0.;
    for (a, b) in intervals {
        if a > end + tolerance {
            break;
        }
        if b > end {
            end = b;
        }
    }
    end >= length - tolerance
}
