//! General placement of annotations against plotted ink, in drawing units and paper clearances.
use crate::ink::{self, Bounds};
use crate::{CoreError, Session, Target};
use opencadcodec::EntityType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Any,
    Right,
    Below,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SpaceQuery {
    /// Rectangle width and height in drawing units. Supply size or entity, not both.
    pub size: Option<[f64; 2]>,
    /// An existing annotation's handle, measured with rendered text and excluded as an obstacle.
    pub entity: Option<u64>,
    /// Preferred top-left corner, in drawing units.
    pub near: Option<[f64; 2]>,
    pub side: Side,
    /// Clear space around the annotation, in paper millimetres; default 2.5.
    pub clearance_mm: Option<f64>,
    pub sheet: Option<String>,
    /// Optional closed region in drawing units. The entire rectangle must fit inside it.
    pub within: Option<Vec<[f64; 2]>>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Space {
    /// Top-left corner.
    pub at: [f64; 2],
    pub bounds: Bounds,
    pub delta: Option<[f64; 2]>,
}
#[derive(Debug, Clone, Serialize)]
pub struct SpaceReport {
    pub sheet: String,
    pub occupied_bounds: Option<Bounds>,
    pub clearance: f64,
    pub positions: Vec<Space>,
    pub collisions: Vec<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Placement {
    pub handle: u64,
    pub bounds: Bounds,
    pub collisions: Vec<u64>,
}
fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}
impl Session {
    pub(crate) fn paper_distance(&self, mm: f64) -> f64 {
        let (to_m, scale) = self.check_space(&self.sheet);
        mm * 0.001 * scale / to_m
    }
    pub fn free_space(&self, q: &SpaceQuery) -> Result<SpaceReport, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("placement.free_space");
        let sheet = self.sheet_block(q.sheet.as_deref());
        if !self.drawing.doc.block_records.contains(&sheet) {
            return Err(invalid("The drawing has no such sheet"));
        }
        let ink = self.sheet_ink(&sheet);
        if !ink.incomplete.is_empty() {
            return Err(invalid(
                "Space could not be checked completely; simplify the nested drawing first",
            ));
        }
        let original = q
            .entity
            .and_then(|h| ink::union(ink.pieces.iter().filter(|p| p.owner == h).map(|p| p.bounds)));
        let size = match (q.size, q.entity) {
            (Some(size), None) => size,
            (None, Some(_)) => original
                .map(|b| [b[1][0] - b[0][0], b[1][1] - b[0][1]])
                .ok_or_else(|| invalid("The annotation has no plotted geometry on this sheet"))?,
            _ => return Err(invalid("Give a rectangle size or an annotation handle")),
        };
        let mm = q.clearance_mm.unwrap_or(2.5);
        if !size.iter().all(|v| v.is_finite() && *v > 0.0)
            || !mm.is_finite()
            || mm < 0.0
            || q.near.is_some_and(|p| !p.iter().all(|v| v.is_finite()))
        {
            return Err(invalid(
                "Size must be positive and clearance non-negative, with finite coordinates",
            ));
        }
        if q.within
            .as_ref()
            .is_some_and(|p| p.len() < 3 || !p.iter().flatten().all(|v| v.is_finite()))
        {
            return Err(invalid(
                "The placement region needs at least three finite points",
            ));
        }
        let (to_m, scale) = self.check_space(&sheet);
        let clearance = mm * 0.001 * scale / to_m;
        let exclude: Vec<_> = q.entity.into_iter().collect();
        let occupied = ink::union(
            ink.pieces
                .iter()
                .filter(|p| !exclude.contains(&p.owner))
                .map(|p| p.bounds),
        );
        let b = occupied.unwrap_or([[0.0; 2]; 2]);
        let near = q
            .near
            .or(original.map(|b| [b[0][0], b[1][1]]))
            .unwrap_or([b[1][0] + clearance, b[1][1]]);
        let rect = |p: [f64; 2]| [[p[0], p[1] - size[1]], [p[0] + size[0], p[1]]];
        let mut candidates = vec![
            near,
            [b[1][0] + clearance, near[1]],
            [near[0], b[0][1] - clearance],
            [b[0][0] - size[0] - clearance, near[1]],
            [near[0], b[1][1] + size[1] + clearance],
        ];
        if let Some(ring) = &q.within {
            let b = ink::bounds(ring);
            for y in 0..=24 {
                for x in 0..=24 {
                    candidates.push([
                        b[0][0] + (b[1][0] - b[0][0] - size[0]) * x as f64 / 24.0,
                        b[1][1] - (b[1][1] - b[0][1] - size[1]) * y as f64 / 24.0,
                    ]);
                }
            }
        } else {
            for p in &ink.pieces {
                if exclude.contains(&p.owner) {
                    continue;
                }
                candidates.extend([
                    [p.bounds[1][0] + clearance, near[1]],
                    [near[0], p.bounds[0][1] - clearance],
                    [p.bounds[1][0] + clearance, p.bounds[1][1]],
                    [p.bounds[0][0], p.bounds[0][1] - clearance],
                ]);
            }
        }
        let score = |p: [f64; 2]| (p[0] - near[0]).hypot(p[1] - near[1]);
        candidates.sort_by(|a, b| {
            score(*a)
                .total_cmp(&score(*b))
                .then(a[0].total_cmp(&b[0]))
                .then(a[1].total_cmp(&b[1]))
        });
        candidates.dedup();
        let positions = candidates
            .into_iter()
            .filter(|p| match q.side {
                Side::Any => true,
                Side::Right => p[0] >= b[1][0] + clearance,
                Side::Below => p[1] <= b[0][1] - clearance,
            })
            .filter_map(|at| {
                let bounds = rect(at);
                let padded = ink::expand(bounds, clearance * 0.999);
                if q.within.as_ref().is_some_and(|r| {
                    !crate::check_layout::polygon_inside(&ink::corners(padded), r, &[])
                }) || !ink.collisions(padded, &exclude).is_empty()
                {
                    return None;
                }
                Some(Space {
                    at,
                    bounds,
                    delta: original.map(|b| [at[0] - b[0][0], at[1] - b[1][1]]),
                })
            })
            .take(8)
            .collect();
        let collisions = original
            .map(|b| ink.collisions(ink::expand(b, clearance), &exclude))
            .unwrap_or_default();
        Ok(SpaceReport {
            sheet,
            occupied_bounds: occupied,
            clearance,
            positions,
            collisions,
        })
    }
    pub(crate) fn auto_place(
        &mut self,
        targets: &[Target],
        near: Option<[f64; 2]>,
        side: Side,
        clearance_mm: Option<f64>,
    ) -> Result<Vec<Placement>, CoreError> {
        if near.is_some_and(|p| !p.iter().all(|v| v.is_finite()))
            || clearance_mm.is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err(invalid(
                "Placement needs finite coordinates and non-negative clearance",
            ));
        }
        let handles = self.sheet_targets(targets)?;
        if handles.is_empty() {
            return Err(invalid("Choose annotations to place"));
        }
        let mut placed = Vec::new();
        for handle in handles {
            let e = self
                .drawing
                .doc
                .get_entity(handle)
                .ok_or_else(|| invalid("Annotation not found"))?;
            let target = Target {
                handle: handle.value(),
                chain: vec![],
            };
            if let Ok(boundary) = self.room_from_label(&target) {
                let region = self
                    .drawing
                    .doc
                    .get_entity(opencadcodec::Handle::new(boundary))
                    .ok_or_else(|| invalid("Room boundary not found"))?;
                let paths = qiyas_geom::flatten(
                    &self.drawing.doc,
                    region,
                    &qiyas_geom::Xform::identity(),
                    0.0002 / self.drawing.units.to_metres(),
                )
                .into_paths();
                let ring = paths
                    .first()
                    .ok_or_else(|| invalid("Room boundary has no outline"))?
                    .points
                    .clone();
                let metadata = crate::object_regions::retained_region(region)
                    .ok_or_else(|| invalid("Room boundary has no retained geometry"))?;
                let boundary = crate::Boundary::Ring {
                    ring,
                    holes: metadata.holes,
                    bulges: std::iter::once(vec![])
                        .chain(metadata.hole_bulges)
                        .collect(),
                };
                self.fit_room_label(handle.value(), &boundary, near.unwrap_or(metadata.seed))?;
            } else if let Some((boundary, point)) = match e {
                EntityType::Text(t) => Some([t.insertion_point.x, t.insertion_point.y]),
                EntityType::MText(t) => Some([t.insertion_point.x, t.insertion_point.y]),
                _ => None,
            }
            .and_then(|point| {
                let point = near.unwrap_or(point);
                let boundary = self.room_boundary_in(&self.sheet, point, &Default::default());
                matches!(boundary, crate::Boundary::Ring { .. }).then_some((boundary, point))
            }) {
                self.fit_room_label(handle.value(), &boundary, point)?;
            } else if matches!(e, EntityType::Dimension(_)) {
                self.place_dimension_clear(handle, clearance_mm.unwrap_or(2.5))?;
            } else {
                if !matches!(
                    e,
                    EntityType::Text(_) | EntityType::MText(_) | EntityType::Table(_)
                ) {
                    return Err(invalid("Choose labels, tables or linear dimensions"));
                }
                let space = self.free_space(&SpaceQuery {
                    entity: Some(handle.value()),
                    near,
                    side,
                    clearance_mm,
                    ..Default::default()
                })?;
                let best = space
                    .positions
                    .first()
                    .ok_or_else(|| invalid("No clear position was found"))?;
                let delta = best.delta.unwrap();
                if delta != [0.0, 0.0] {
                    self.modify(crate::modify::ModifySpec::Move {
                        targets: vec![Target {
                            handle: handle.value(),
                            chain: vec![],
                        }],
                        delta,
                    })?;
                }
            }
            let ink = self.sheet_ink(&self.sheet);
            let bounds = ink::union(
                ink.pieces
                    .iter()
                    .filter(|p| p.owner == handle.value())
                    .map(|p| p.bounds),
            )
            .ok_or_else(|| invalid("Placed annotation has no visible geometry"))?;
            let mut findings = Vec::new();
            let (to_m, scale) = self.check_space(&self.sheet);
            self.check_layout_ink(&ink, &self.sheet, to_m, scale, &|_| true, &mut |f| {
                findings.push(f)
            });
            let mut collisions: Vec<_> = findings
                .into_iter()
                .filter(|f| f.rule != "text_height_vs_scale" && f.handles.contains(&handle.value()))
                .flat_map(|f| {
                    let other: Vec<_> = f
                        .handles
                        .into_iter()
                        .filter(|h| *h != handle.value())
                        .collect();
                    if other.is_empty() {
                        vec![handle.value()]
                    } else {
                        other
                    }
                })
                .collect();
            collisions.sort_unstable();
            collisions.dedup();
            placed.push(Placement {
                handle: handle.value(),
                bounds,
                collisions,
            });
        }
        Ok(placed)
    }
    pub(crate) fn place_new_table(
        &mut self,
        handle: u64,
        automatic: bool,
    ) -> Result<(), CoreError> {
        if automatic {
            self.auto_place(
                &[Target {
                    handle,
                    chain: vec![],
                }],
                None,
                Side::Right,
                Some(10.0),
            )?;
        }
        Ok(())
    }
}

impl Session {
    /// Fit a live area label without changing its text height or field binding. Try its full
    /// width first, then wrap; an impossible room is an error rather than unreadable text.
    pub(crate) fn fit_room_label(
        &mut self,
        handle: u64,
        boundary: &crate::Boundary,
        near: [f64; 2],
    ) -> Result<([f64; 2], bool), CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("placement.fit_room_label");
        let crate::Boundary::Ring {
            ring,
            holes,
            bulges,
        } = boundary
        else {
            return Err(invalid("The room has no closed boundary"));
        };
        let ring = self.layout_ring(ring, bulges.first().map_or(&[], Vec::as_slice));
        let holes: Vec<_> = holes
            .iter()
            .enumerate()
            .map(|(i, h)| self.layout_ring(h, bulges.get(i + 1).map_or(&[], Vec::as_slice)))
            .collect();
        let h = opencadcodec::Handle::new(handle);
        let original = self
            .drawing
            .doc
            .get_entity(h)
            .ok_or_else(|| invalid("Label not found"))?
            .clone();
        let b = ink::bounds(&ring);
        // Try both directions without shrinking the text. A narrow room can have
        // enough clear space along its length even when a word cannot wrap across it.
        for turn in [0.0, std::f64::consts::FRAC_PI_2] {
            let mut original = original.clone();
            match &mut original {
                EntityType::Text(text) => text.rotation += turn,
                EntityType::MText(text) => text.rotation += turn,
                _ => return Err(invalid("Choose a text label")),
            }
            *self.drawing.doc.get_entity_mut(h).expect("label exists") = original.clone();
            let axis = usize::from(turn != 0.0);
            let width = b[1][axis] - b[0][axis] - self.paper_distance(2.0);
            for fraction in [0.0, 1.0, 0.75, 0.5, 0.3] {
                if fraction > 0.0 {
                    let mut m = match &original {
                        EntityType::Text(t) => {
                            let mut m = opencadcodec::MText::with_value(
                                crate::text::escape_mtext(&t.value),
                                t.insertion_point,
                            )
                            .with_height(t.height);
                            m.common = t.common.clone();
                            m.style = t.style.clone();
                            m.rotation = t.rotation;
                            m
                        }
                        EntityType::MText(m) => m.clone(),
                        _ => return Err(invalid("Choose a text label")),
                    };
                    m.rectangle_width = (width * fraction).max(m.height * 2.0);
                    // Keep a live field's value and following unit together when wrapping.
                    if let Some(code) = crate::field::text_field(&m.common) {
                        let code = code.replace(">% ", ">%\u{00a0}");
                        m.value = crate::text::escape_mtext(
                            &self.evaluate_fields(&code, m.common.owner_handle),
                        );
                        crate::field::set_text_field(
                            &mut m.common,
                            Some(&code),
                            crate::xdata::app_handle(&self.drawing.doc, crate::field::FIELD_APP),
                        );
                    }
                    *self.drawing.doc.get_entity_mut(h).expect("label exists") =
                        EntityType::MText(m);
                }
                let report = self.free_space(&SpaceQuery {
                    entity: Some(handle),
                    near: Some(near),
                    within: Some(ring.clone()),
                    clearance_mm: Some(1.0),
                    ..Default::default()
                })?;
                if let Some(best) = report.positions.iter().find(|p| {
                    crate::check_layout::polygon_inside(&ink::corners(p.bounds), &ring, &holes)
                }) {
                    self.modify(crate::modify::ModifySpec::Move {
                        targets: vec![Target {
                            handle,
                            chain: vec![],
                        }],
                        delta: best.delta.unwrap(),
                    })?;
                    let at = match self.drawing.doc.get_entity(h).expect("label exists") {
                        EntityType::Text(t) => [t.insertion_point.x, t.insertion_point.y],
                        EntityType::MText(t) => [t.insertion_point.x, t.insertion_point.y],
                        _ => best.at,
                    };
                    return Ok((
                        at,
                        fraction > 0.0 || matches!(original, EntityType::MText(_)),
                    ));
                }
            }
        }
        *self.drawing.doc.get_entity_mut(h).expect("label exists") = original;
        Err(invalid("The room is too small for this label at its current height. Use a larger drawing scale or place a linked note outside the room"))
    }
}
