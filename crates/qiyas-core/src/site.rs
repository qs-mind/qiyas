//! A rigid survey placement. Geometry and exported world coordinates are the site coordinates.
use crate::{CoreError, ModifySpec, Session, Target, Ucs};
use opencadcodec::{EntityType, Vector3};
use serde_json::{json, Value};

impl Session {
    pub(crate) fn set_site_origin(
        &mut self,
        base_point: [f64; 2],
        site_point: [f64; 2],
        x_axis: [f64; 2],
        targets: Option<Vec<Target>>,
    ) -> Result<(), CoreError> {
        let ucs = Ucs::new(site_point, x_axis)?;
        if !base_point.iter().all(|v| v.is_finite()) {
            return Err(CoreError::InvalidCommand(
                "The base point must be finite".into(),
            ));
        }
        let targets = targets.unwrap_or_else(|| {
            self.drawing
                .entities_in("*Model_Space")
                .map(|e| Target {
                    handle: e.common().handle.value(),
                    chain: vec![],
                })
                .collect()
        });
        if !targets.is_empty() {
            // Reuse the normal object/host and locked-layer rules. The outer command owns
            // the undo recorder, so a failure also restores all earlier transformed entities.
            let old_sheet = std::mem::replace(&mut self.sheet, "*Model_Space".into());
            let unit = 1. / self.drawing.units.to_metres();
            let along = [
                site_point[0] + ucs.x_axis[0] * unit,
                site_point[1] + ucs.x_axis[1] * unit,
            ];
            let result = self.modify(ModifySpec::Align {
                targets,
                pairs: vec![
                    [base_point, site_point],
                    [[base_point[0] + unit, base_point[1]], along],
                ],
            });
            self.sheet = old_sheet;
            result?;
        }
        crate::knowledge::put(
            &mut self.drawing.doc,
            crate::knowledge::Kind::Mappings,
            "site_origin",
            &json!({"base_point":base_point,"site_point":site_point,"x_axis":ucs.x_axis,"coordinates":"drawing_units","geometry_in_site_coordinates":true}),
        )?;
        ucs.write_to(&mut self.drawing.doc);
        self.drawing.doc.header.model_space_insertion_base =
            Vector3::new(site_point[0], site_point[1], 0.);
        self.ucs = ucs;
        Ok(())
    }

    /// Exact defining points, never vertices sampled along a curve. Each point keeps its
    /// index so a brief can check a particular corner or signed displacement by readback.
    pub fn site_coordinates(&self, targets: &[Target]) -> Result<Value, CoreError> {
        let xrefs = self.xref_docs();
        let mut rows = Vec::new();
        for target in targets {
            let located =
                crate::session::locate(&self.drawing.doc, &xrefs, target.handle, &target.chain)?;
            let points: Vec<[f64; 2]> = match located.entity {
                EntityType::Line(e) => vec![[e.start.x, e.start.y], [e.end.x, e.end.y]],
                EntityType::LwPolyline(e) => e
                    .vertices
                    .iter()
                    .map(|v| [v.location.x, v.location.y])
                    .collect(),
                EntityType::Circle(e) => vec![[e.center.x, e.center.y]],
                EntityType::Arc(e) => vec![[e.center.x, e.center.y]],
                EntityType::Insert(e) => vec![[e.insert_point.x, e.insert_point.y]],
                EntityType::Text(e) => vec![[e.insertion_point.x, e.insertion_point.y]],
                EntityType::MText(e) => vec![[e.insertion_point.x, e.insertion_point.y]],
                EntityType::Point(e) => vec![[e.location.x, e.location.y]],
                _ => {
                    return Err(CoreError::InvalidCommand(format!(
                    "No exact coordinate anchors for handle {}; use records for its defining data",
                    target.handle
                )))
                }
            };
            let site: Vec<_> = points.into_iter().map(|p| located.xf.apply(p)).collect();
            let local: Vec<_> = site.iter().map(|&p| self.ucs.to_ucs(p)).collect();
            let metres: Vec<_> = site
                .iter()
                .map(|p| p.map(|v| v * self.drawing.units.to_metres()))
                .collect();
            rows.push(json!({"target":target,"site":site,"site_metres":metres,"local":local}));
        }
        Ok(
            json!({"rows":rows,"metres_per_unit":self.drawing.units.to_metres(),"ucs":self.ucs,
            "base_point":[self.drawing.doc.header.model_space_insertion_base.x,self.drawing.doc.header.model_space_insertion_base.y],
            "coordinates":"World coordinates are site coordinates; local coordinates use the current UCS. Command JSON points use world coordinates; typed points use the UCS."}),
        )
    }
}
