//! Shared geometry for typed path bands and vertically bounded cuts. Inputs contain only
//! geometry and evaluated values; trade defaults and drawing choices belong to type data.

use std::collections::BTreeMap;

use qiyas_geom::walls::{self, Justification};

use crate::objects::PlanRecipe;
use crate::{CoreError, DrawSpec, HatchPattern};

#[derive(Clone)]
pub(crate) struct BandMember {
    pub id: u64,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub width_m: f64,
    pub base_m: f64,
    pub height_m: f64,
    pub justification: Justification,
}

#[derive(Clone)]
pub(crate) struct BandCut {
    pub host: u64,
    pub at_m: f64,
    pub width_m: f64,
    pub base_m: f64,
    pub height_m: f64,
}

pub(crate) struct BandDerived {
    pub geometry: walls::Walls,
    pub gross_volume: f64,
    pub net_volume: f64,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

pub(crate) fn derive_band(
    members: &[BandMember],
    cuts: &[BandCut],
    factor: f64,
    tolerance: f64,
) -> Result<BandDerived, CoreError> {
    #[cfg(any(test, feature = "fixtures"))]
    let _profile = crate::fixtures::profile::span("object_bands.derive_band");
    if members.is_empty() || !factor.is_finite() || factor <= 0.0 {
        return Err(invalid(
            "A band assembly needs members and valid drawing units",
        ));
    }
    let mut indices = BTreeMap::new();
    let mut levels = Vec::new();
    let mut runs = Vec::new();
    for (index, member) in members.iter().enumerate() {
        if !member.width_m.is_finite()
            || member.width_m <= 0.0
            || !member.base_m.is_finite()
            || !member.height_m.is_finite()
            || member.height_m < 0.0
            || !(member.base_m + member.height_m).is_finite()
        {
            return Err(invalid(
                "Band width must be positive and its vertical span finite",
            ));
        }
        if indices.insert(member.id, index).is_some() {
            return Err(invalid("Band assembly has duplicate driver identities"));
        }
        levels.extend([member.base_m, member.base_m + member.height_m]);
        runs.push(walls::Run {
            points: member.points.clone(),
            closed: member.closed,
            thickness: Some(member.width_m / factor),
            justification: Some(member.justification),
        });
    }
    let mut openings = Vec::new();
    let mut void_volume = 0.0;
    for cut in cuts {
        let index = *indices
            .get(&cut.host)
            .ok_or_else(|| invalid("A band cut's host is outside its assembly"))?;
        if !cut.at_m.is_finite()
            || !cut.width_m.is_finite()
            || cut.width_m <= 0.0
            || !cut.base_m.is_finite()
            || !cut.height_m.is_finite()
            || cut.height_m < 0.0
            || !(cut.base_m + cut.height_m).is_finite()
        {
            return Err(invalid(
                "Band cut dimensions must be finite and its width positive",
            ));
        }
        let host = &members[index];
        let low = cut.base_m.max(host.base_m);
        let high = (cut.base_m + cut.height_m).min(host.base_m + host.height_m);
        void_volume += cut.width_m * host.width_m * (high - low).max(0.0);
        openings.push(walls::Opening {
            run: index,
            at: cut.at_m / factor,
            width: cut.width_m / factor,
        });
    }
    let width = runs[0].thickness.expect("evaluated width");
    let geometry =
        walls::build_walls_justified(&runs, width, &openings, tolerance, Justification::Centre)
            .map_err(|error| invalid(error.to_string()))?;
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut gross_volume = 0.0;
    for span in levels.windows(2) {
        let active: Vec<_> = runs
            .iter()
            .zip(members)
            .filter(|(_, member)| {
                member.base_m < span[1] && member.base_m + member.height_m > span[0]
            })
            .map(|(run, _)| run.clone())
            .collect();
        if active.is_empty() {
            continue;
        }
        let area =
            walls::build_walls_justified(&active, width, &[], tolerance, Justification::Centre)
                .map_err(|error| invalid(error.to_string()))?
                .gross_area;
        gross_volume += area * factor * factor * (span[1] - span[0]);
    }
    let net_volume = gross_volume - void_volume;
    if !gross_volume.is_finite() || !net_volume.is_finite() {
        return Err(invalid("Band volume exceeds the finite numeric range"));
    }
    Ok(BandDerived {
        geometry,
        gross_volume,
        net_volume,
    })
}

/// The same recipe produces faces, filled regions and path graphics for every element type.
pub(crate) fn band_graphics(
    members: &[BandMember],
    derived: &BandDerived,
    plan: &PlanRecipe,
    include_centrelines: bool,
) -> Vec<(String, DrawSpec)> {
    let mut graphics = Vec::new();
    if plan.outline || plan.band_faces.is_some() {
        for region in &derived.geometry.regions {
            for ring in std::iter::once(&region.outer).chain(&region.holes) {
                graphics.push((
                    "outline".into(),
                    DrawSpec::Polyline {
                        points: ring.clone().into(),
                        closed: true,
                        bulges: Vec::new(),
                    },
                ));
            }
        }
    }
    if plan.hatch {
        for region in &derived.geometry.regions {
            graphics.push((
                "hatch".into(),
                DrawSpec::Hatch {
                    points: region.outer.clone().into(),
                    holes: region.holes.iter().cloned().map(Into::into).collect(),
                    boundary_handles: Vec::new(),
                    bulges: Vec::new(),
                    pattern: HatchPattern::Solid,
                },
            ));
        }
    }
    if plan.centreline && include_centrelines {
        for member in members {
            graphics.push((
                "centreline".into(),
                DrawSpec::Polyline {
                    points: member.points.clone().into(),
                    closed: member.closed,
                    bulges: Vec::new(),
                },
            ));
        }
    }
    graphics
}
