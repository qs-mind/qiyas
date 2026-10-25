//! Room enclosure sources are independent of plotted annotation and layer visibility.
use super::*;
use crate::object_regions::BoundaryLayers;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Default)]
pub(crate) struct RoomCache {
    graphs: Vec<(String, Arc<SourceGraph>)>,
}
struct SourceGraph {
    context: String,
    segments: Vec<[f64; 4]>,
    rounds: Vec<Round>,
    // The arrangement's tolerance depends on the seed and whole source extent. Keep it
    // exact: sharing an arrangement with a merely similar tolerance can change welding.
    faces: std::sync::Mutex<BTreeMap<u64, Vec<Vec<[f64; 2]>>>>,
    boundaries: std::sync::Mutex<BTreeMap<[u64; 2], Boundary>>,
}

impl SourceGraph {
    fn radius(&self, point: [f64; 2], factor: f64) -> f64 {
        self.segments
            .iter()
            .flat_map(|s| [[s[0], s[1]], [s[2], s[3]]])
            .map(|p| (p[0] - point[0]).abs().max((p[1] - point[1]).abs()))
            .fold(1.0 / factor, f64::max)
            * 1.1
    }
    fn reuse_unaffected(&self, previous: &Self, factor: f64) {
        if self.context != previous.context {
            return;
        }
        let old = previous.boundaries.lock().expect("room boundaries");
        let mut current = self.boundaries.lock().expect("room boundaries");
        for (&seed, boundary) in old.iter() {
            let Boundary::Ring { ring, .. } = boundary else {
                continue;
            };
            let point = seed.map(f64::from_bits);
            let tolerance = (self.radius(point, factor) * 1e-6).max(1e-6);
            if tolerance.to_bits() != (previous.radius(point, factor) * 1e-6).max(1e-6).to_bits() {
                continue;
            }
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for point in ring {
                for axis in 0..2 {
                    lo[axis] = lo[axis].min(point[axis]);
                    hi[axis] = hi[axis].max(point[axis]);
                }
            }
            // This is the face's topology support, enlarged only by the graph's weld
            // tolerance. Segments crossing the face and interior islands are included.
            let local = |graph: &Self| {
                graph
                    .segments
                    .iter()
                    .filter(|s| {
                        s[0].min(s[2]) <= hi[0] + 2.0 * tolerance
                            && s[0].max(s[2]) >= lo[0] - 2.0 * tolerance
                            && s[1].min(s[3]) <= hi[1] + 2.0 * tolerance
                            && s[1].max(s[3]) >= lo[1] - 2.0 * tolerance
                    })
                    .map(|s| s.map(f64::to_bits))
                    .collect::<Vec<_>>()
            };
            let local_rounds = |graph: &Self| {
                graph
                    .rounds
                    .iter()
                    .filter(|round| {
                        (0..2).all(|axis| {
                            round.centre[axis] - round.radius <= hi[axis] + 2.0 * tolerance
                                && round.centre[axis] + round.radius >= lo[axis] - 2.0 * tolerance
                        })
                    })
                    .map(|round| {
                        [
                            round.centre[0],
                            round.centre[1],
                            round.radius,
                            round.start,
                            round.sweep,
                        ]
                        .map(f64::to_bits)
                    })
                    .collect::<Vec<_>>()
            };
            // Preserve source order too: it determines welding, ring starts and bulges.
            if local(self) == local(previous) && local_rounds(self) == local_rounds(previous) {
                current.insert(seed, boundary.clone());
            }
        }
    }
}

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
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("draw.room_boundary.room_boundary_in");
        if !point.iter().all(|v| v.is_finite()) {
            return Boundary::None;
        }
        let _read = self.evaluation_read().ok();
        let sheet = self.sheet_block(Some(sheet));
        let query = json!([sheet, policy]).to_string();
        let cached_key = _read
            .as_ref()
            .and_then(|_| self.evaluation.borrow().room_keys.get(&query).cloned());
        let key = cached_key.unwrap_or_else(|| {
            let key = self.room_source_key(&sheet, policy);
            if _read.is_some() {
                self.evaluation
                    .borrow_mut()
                    .room_keys
                    .insert(query, key.clone());
            }
            key
        });
        let cached = self
            .room_cache
            .borrow()
            .graphs
            .iter()
            .find(|(previous, _)| *previous == key)
            .map(|(_, graph)| graph.clone());
        let graph = if let Some(graph) = cached {
            graph
        } else {
            let graph = match self.room_source_graph(&sheet, policy) {
                Ok(mut graph) => {
                    graph.context = json!([
                        sheet,
                        policy,
                        self.drawing.units.to_metres(),
                        self.measure_tol(),
                        crate::knowledge::list(&self.drawing.doc, crate::knowledge::Kind::Types)
                            .ok()
                    ])
                    .to_string();
                    if let Some((_, previous)) = self
                        .room_cache
                        .borrow()
                        .graphs
                        .iter()
                        .rev()
                        .find(|(_, previous)| previous.context == graph.context)
                    {
                        graph.reuse_unaffected(previous, self.drawing.units.to_metres());
                    }
                    Arc::new(graph)
                }
                Err(boundary) => return boundary,
            };
            let mut cache = self.room_cache.borrow_mut();
            // Keep only bounded read memoization. No history or saved document owns it.
            if cache.graphs.len() >= 8 {
                cache.graphs.remove(0);
            }
            cache.graphs.push((key, graph.clone()));
            graph
        };
        let seed = point.map(f64::to_bits);
        if let Some(boundary) = graph.boundaries.lock().expect("room boundaries").get(&seed) {
            return boundary.clone();
        }
        let radius = graph.radius(point, self.drawing.units.to_metres());
        let lo = [point[0] - radius, point[1] - radius];
        let hi = [point[0] + radius, point[1] + radius];
        let tolerance = (radius * 1e-6).max(1e-6);
        let mut faces = graph.faces.lock().expect("room faces");
        let faces = faces.entry(tolerance.to_bits()).or_insert_with(|| {
            let lines: Vec<_> = graph
                .segments
                .iter()
                .map(|&[x0, y0, x1, y1]| KernelLine {
                    start: [x0, y0],
                    end: [x1, y1],
                })
                .collect();
            bounded_faces(&lines, Tolerance::new(tolerance))
        });
        let boundary = region_from_faces(faces, point, lo, hi)
            .map(|found| with_bulges(found, &graph.rounds, self.measure_tol()))
            .unwrap_or(Boundary::None);
        graph
            .boundaries
            .lock()
            .expect("room boundaries")
            .insert(seed, boundary.clone());
        boundary
    }

    fn room_source_key(&self, sheet: &str, policy: &BoundaryLayers) -> String {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("draw.room_boundary.room_source_key");
        let doc = &self.drawing.doc;
        let mut walls = BTreeMap::new();
        let mut sources = Vec::new();
        let mut pending = Vec::new();
        for entity in doc.entities_in_block(sheet) {
            if let Some(tag) = crate::walls::wall_tag(entity) {
                if !walls.contains_key(&tag.group)
                    && policy.accepts(&crate::walls::resolved_layers(&tag.spec.layers)[0])
                {
                    let runs: Vec<_> = crate::walls::geom_runs(&tag.spec)
                        .into_iter()
                        .map(|r| {
                            (
                                r.points,
                                r.closed,
                                r.thickness,
                                format!("{:?}", r.justification),
                            )
                        })
                        .collect();
                    walls.entry(tag.group).or_insert(json!([
                        runs,
                        tag.spec.thickness,
                        tag.spec.justification
                    ]));
                }
            } else if self.room_source(doc, entity) {
                sources.push(json!(entity));
                if let EntityType::Insert(insert) = entity {
                    pending.push(insert.block_name.clone());
                }
            }
        }
        // Block children and external geometry can change without changing their INSERT.
        // Include their source records; annotations and opening symbols still stay excluded.
        let mut definitions = BTreeMap::new();
        while let Some(name) = pending.pop() {
            let name = name.to_uppercase();
            if definitions.contains_key(&name) {
                continue;
            }
            let children: Vec<_> = doc
                .entities_in_block(&name)
                .filter(|e| self.room_source(doc, e))
                .collect();
            for child in &children {
                if let EntityType::Insert(insert) = child {
                    pending.push(insert.block_name.clone());
                }
            }
            definitions.insert(
                name.clone(),
                json!([doc.block_records.get(&name), children]),
            );
        }
        let xrefs: BTreeMap<_, _> = self
            .xref_docs()
            .into_iter()
            .map(|(name, doc)| {
                (
                    name,
                    json!([
                        doc.block_records.iter().collect::<Vec<_>>(),
                        doc.entities()
                            .filter(|e| self.room_source(doc, e))
                            .collect::<Vec<_>>()
                    ]),
                )
            })
            .collect();
        json!([
            sheet,
            policy,
            self.drawing.units.to_metres(),
            self.measure_tol(),
            walls,
            sources,
            definitions,
            xrefs,
            crate::knowledge::list(doc, crate::knowledge::Kind::Types).ok()
        ])
        .to_string()
    }

    fn room_source_graph(
        &self,
        sheet: &str,
        policy: &BoundaryLayers,
    ) -> Result<SourceGraph, Boundary> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("draw.room_boundary.room_source_graph");
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
                return Err(Boundary::TooDense);
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
                return Err(Boundary::None);
            };
            for region in wall.regions {
                add_path(&region.outer, true);
                for hole in region.holes {
                    add_path(&hole, true);
                }
            }
        }
        if segments.len() > BOUNDARY_SEGMENTS * 4 {
            return Err(Boundary::TooDense);
        }
        Ok(SourceGraph {
            context: String::new(),
            segments,
            rounds,
            faces: Default::default(),
            boundaries: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;

    #[test]
    fn openings_reuse_uncut_rooms_and_history_discards_them() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut session = Session::new_blank(&dir.path().join("room-cache.dxf"), units);
            let k = 1.0 / session.drawing.units.to_metres();
            let walls = session
                .run_input(
                    &json!({"type":"walls","thickness":0.2*k,
                        "runs":[{"points":[[0,0],[6.0*k,0],[6.0*k,4.0*k],[0,4.0*k]],"closed":true}],
                        "openings":[{"run":0,"at":2.0*k,"width":0.8*k,"kind":"door"}]
                    }),
                    Source::User,
                )
                .unwrap();
            let door = json!(walls)["inserts"][0].as_u64().unwrap();
            let point = [3.0 * k, 2.0 * k];
            let policy = BoundaryLayers::default();
            let before = json!(session.room_boundary_in("*Model_Space", point, &policy));
            let graph = session.room_cache.borrow().graphs.last().unwrap().1.clone();
            assert_eq!(graph.boundaries.lock().expect("room boundaries").len(), 1);
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", point, &policy)),
                before
            );
            assert_eq!(graph.faces.lock().expect("room faces").len(), 1);
            session
                .run_input(
                    &json!({"type":"edit_wall_opening","insert":door,"width":1.0*k}),
                    Source::User,
                )
                .unwrap();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", point, &policy)),
                before
            );
            assert!(
                Arc::ptr_eq(
                    &graph,
                    &session.room_cache.borrow().graphs.last().unwrap().1
                ),
                "an opening changed the uncut source graph in unit {units}"
            );
            // A cold trace is the independent reference, including exact ring/bulge order.
            *session.room_cache.get_mut() = RoomCache::default();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", point, &policy)),
                before
            );
            assert!(session.undo());
            assert!(session.room_cache.borrow().graphs.is_empty());
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", point, &policy)),
                before
            );
            assert!(session.redo());
            assert!(session.room_cache.borrow().graphs.is_empty());
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", point, &policy)),
                before
            );
        }
    }
    #[test]
    fn topology_change_reuses_only_untouched_faces() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut session = Session::new_blank(&dir.path().join("topology.dxf"), units);
            let k = 1.0 / session.drawing.units.to_metres();
            for x in [0.0, 20.0] {
                session.run_input(&json!({"type":"walls","thickness":0.2*k,
                    "runs":[{"points":[[x*k,0],[(x+6.0)*k,0],[(x+6.0)*k,4.0*k],[x*k,4.0*k]],"closed":true}]
                }),Source::User).unwrap();
            }
            let policy = BoundaryLayers::default();
            let left = [2.0 * k, 2.0 * k];
            let right = [23.0 * k, 2.0 * k];
            let old_left = json!(session.room_boundary_in("*Model_Space", left, &policy));
            let old_right = json!(session.room_boundary_in("*Model_Space", right, &policy));
            let wall = session
                .run_input(
                    &json!({"type":"walls","thickness":0.2*k,"join":false,
                        "runs":[{"points":[[3.0*k,0],[3.0*k,4.0*k]]}]
                    }),
                    Source::User,
                )
                .unwrap();
            let right_after = json!(session.room_boundary_in("*Model_Space", right, &policy));
            assert_eq!(right_after, old_right);
            let graph = session.room_cache.borrow().graphs.last().unwrap().1.clone();
            assert!(
                graph.faces.lock().unwrap().is_empty(),
                "untouched face was retraced in unit {units}"
            );
            let left_after = json!(session.room_boundary_in("*Model_Space", left, &policy));
            assert_ne!(left_after, old_left);
            *session.room_cache.get_mut() = RoomCache::default();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", right, &policy)),
                right_after
            );
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", left, &policy)),
                left_after
            );
            let handles = json!(wall)["handles"].clone();
            session
                .run_input(
                    &json!({"type":"delete_entities","handles":handles}),
                    Source::User,
                )
                .unwrap();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", left, &policy)),
                old_left
            );
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", right, &policy)),
                old_right
            );
            // A curved island in one face does not invalidate a separate face's arcs.
            session
                .run_input(
                    &json!({"type":"draw","kind":"circle",
                "centre":[4.0*k,2.0*k],"radius":0.3*k}),
                    Source::User,
                )
                .unwrap();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", right, &policy)),
                old_right
            );
            assert!(session
                .room_cache
                .borrow()
                .graphs
                .last()
                .unwrap()
                .1
                .faces
                .lock()
                .unwrap()
                .is_empty());
            let with_island = json!(session.room_boundary_in("*Model_Space", left, &policy));
            assert_ne!(with_island, old_left);
            *session.room_cache.get_mut() = RoomCache::default();
            assert_eq!(
                json!(session.room_boundary_in("*Model_Space", left, &policy)),
                with_island
            );
        }
    }
    #[test]
    fn a_scoped_check_traces_only_selected_room_labels() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut session = Session::new_blank(&dir.path().join("scoped-rooms.dxf"), units);
            let k = 1.0 / session.drawing.units.to_metres();
            let mut labels = Vec::new();
            for i in 0..25 {
                let result = session
                    .run_input(
                        &json!({"type":"text", "at":[i as f64*20.0*k, 0],
                    "height":0.2*k, "value":"5.25m x 3.25m"}),
                        Source::User,
                    )
                    .unwrap();
                labels.push(json!(result)["handles"][0].as_u64().unwrap());
            }
            *session.room_cache.get_mut() = RoomCache::default();
            let scope = crate::check::CheckScope {
                handles: vec![labels[0]],
                rules: vec!["room_open".into()],
                ..Default::default()
            };
            let scoped = session.check_drawing(&scope);
            assert_eq!(scoped.findings.len(), 1);
            assert_eq!(
                session
                    .room_cache
                    .borrow()
                    .graphs
                    .iter()
                    .map(|(_, g)| g.boundaries.lock().unwrap().len())
                    .sum::<usize>(),
                1
            );
            let full = session.check_drawing(&crate::check::CheckScope {
                handles: vec![],
                ..scope
            });
            assert_eq!(full.findings.len(), 25);
            assert_eq!(
                session
                    .room_cache
                    .borrow()
                    .graphs
                    .iter()
                    .map(|(_, g)| g.boundaries.lock().unwrap().len())
                    .sum::<usize>(),
                25
            );
        }
    }
}
