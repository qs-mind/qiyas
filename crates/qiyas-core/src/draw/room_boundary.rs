//! Room enclosure sources are independent of plotted annotation and layer visibility.
use super::*;
#[path = "room_islands.rs"]
mod islands;
use crate::object_regions::BoundaryLayers;
use std::{collections::BTreeMap, sync::Arc};

type Rings = Vec<Vec<[f64; 2]>>;
type BlockBounds = BTreeMap<(usize, String, String), Option<[[f64; 2]; 2]>>;

#[derive(Default)]
pub(crate) struct RoomCache {
    graphs: Vec<(Arc<str>, Arc<SourceGraph>)>,
    wall_faces: Vec<(String, Arc<Rings>)>,
}
struct SourceGraph {
    context: String,
    radius: f64,
    whole_sheet: bool,
    segments: Vec<[f64; 4]>,
    /// The segments that may also bound a room's island (a void, not a symbol outline).
    island_segments: Vec<[f64; 4]>,
    rounds: Vec<Round>,
    // The arrangement's tolerance depends on the local search radius. Keep it
    // exact: sharing an arrangement with a merely similar tolerance can change welding.
    faces: std::sync::Mutex<BTreeMap<u64, Vec<Vec<[f64; 2]>>>>,
    boundaries: std::sync::Mutex<BTreeMap<[u64; 2], Boundary>>,
}

impl SourceGraph {
    fn reuse_unaffected(&self, previous: &Self) {
        if self.context != previous.context {
            return;
        }
        let old = previous.boundaries.lock().expect("room boundaries");
        let mut current = self.boundaries.lock().expect("room boundaries");
        for (&seed, boundary) in old.iter() {
            let Boundary::Ring { ring, .. } = boundary else {
                continue;
            };
            let tolerance = (self.radius * 1e-6).max(1e-6);
            if tolerance.to_bits() != (previous.radius * 1e-6).max(1e-6).to_bits() {
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
            let local = |segments: &[[f64; 4]]| {
                segments
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
            if local(&self.segments) == local(&previous.segments)
                && local(&self.island_segments) == local(&previous.island_segments)
                && local_rounds(self) == local_rounds(previous)
            {
                current.insert(seed, boundary.clone());
            }
        }
    }
}

/// Counts refer to the local search at the point where traversal stopped, never a guessed
/// total for the rest of the sheet. Work is attributed to the root layer/block of each walk;
/// segments are attributed to their resolved child layer and root block.
#[derive(Default)]
struct RoomCounts {
    segments: usize,
    layers: BTreeMap<String, [usize; 2]>,
    blocks: BTreeMap<String, [usize; 2]>,
}

#[derive(Debug, Clone)]
pub(crate) struct RoomBudget {
    pub reason: String,
    pub reason_ar: String,
}

impl RoomCounts {
    fn segment(&mut self, layer: &str, block: Option<&str>) {
        self.segments += 1;
        self.layers.entry(layer.into()).or_default()[0] += 1;
        if let Some(block) = block {
            self.blocks.entry(block.into()).or_default()[0] += 1;
        }
    }

    fn work(&mut self, layer: &str, block: Option<&str>, work: usize) {
        self.layers.entry(layer.into()).or_default()[1] += work;
        if let Some(block) = block {
            self.blocks.entry(block.into()).or_default()[1] += work;
        }
    }

    fn failure(&self, budget: &str, limit: usize, work: usize) -> RoomBudget {
        let index = usize::from(budget != "segments");
        let top = |entries: &BTreeMap<String, [usize; 2]>| {
            let mut entries: Vec<_> = entries
                .iter()
                .filter(|(_, counts)| counts[index] > 0)
                .collect();
            entries.sort_by(|(an, a), (bn, b)| b[index].cmp(&a[index]).then_with(|| an.cmp(bn)));
            entries
                .into_iter()
                .take(5)
                .map(|(name, counts)| format!("{name}: {}", counts[index]))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let layers = top(&self.layers);
        let blocks = top(&self.blocks);
        let (en, ar, count) = match budget {
            "segments" => ("boundary segments", "مقاطع الحدود", self.segments),
            "block_depth" => ("block nesting depth", "عمق تداخل البلوكات", limit + 1),
            _ => (
                "flatten work (block instances and paths)",
                "عمل التفكيك (نسخ البلوكات والمسارات)",
                work,
            ),
        };
        RoomBudget {
            reason: format!("Room search reached the {en} budget: {count}, limit {limit}; local segments: {}, flatten work consumed: {work}. Top layers: [{}]; top blocks: [{}]. Counts stop at refusal; flatten work is attributed to root layers and blocks.", self.segments, layers, blocks),
            reason_ar: format!("بلغ البحث عن الفراغ حد {ar}: {count}، الحد {limit}؛ المقاطع المحلية: {}، عمل التفكيك المستهلك: {work}. أكثر الطبقات مساهمة: [{}]؛ أكثر البلوكات مساهمة: [{}]. تتوقف الأعداد عند الرفض؛ ينسب عمل التفكيك إلى طبقات وبلوكات المراجع الرئيسية.", self.segments, layers, blocks),
        }
    }
}

fn meets(min: [f64; 2], max: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> bool {
    (0..2).all(|axis| min[axis] <= hi[axis] && max[axis] >= lo[axis])
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
        let owner = crate::xdata::read_json::<crate::objects::ObjectPart>(
            entity.common(),
            crate::objects::PART_APP,
        )
        .and_then(|part| doc.get_entity(Handle::new(part.owner)))
        .unwrap_or(entity);
        let declared_void = crate::objects::instance(owner)
            .and_then(|instance| self.object_type(&instance.type_name).ok())
            .is_some_and(|definition| definition.room_void);
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
            || crate::object_tags::is_tag(entity)
            || entity
                .common()
                .extended_data
                .get_record(crate::comment::COMMENT_APP)
                .is_some()
            || entity.common().extended_data.get_record("QIYAS").is_some()
            || crate::walls::wall_tag(entity).is_some()
            || (!declared_void
                && (crate::object_regions::retained_region(entity).is_some()
                    || crate::room_objects::is_room_boundary(entity)))
        {
            return false;
        }
        if !declared_void && crate::object_regions::retained_region(owner).is_some() {
            return false;
        }
        if crate::objects::instance(owner)
            .and_then(|instance| self.object_type(&instance.type_name).ok())
            .is_some_and(|definition| {
                !definition.room_void
                    && (definition.geometry == crate::objects::GeometryKind::Region
                        || definition.category == "opening"
                        || (definition.geometry == crate::objects::GeometryKind::Hosted
                            && definition.host_cut.is_some()))
            })
        {
            return false;
        }
        true
    }

    fn room_island_source(&self, doc: &opencadcodec::CadDocument, entity: &EntityType) -> bool {
        let owner = crate::xdata::read_json::<crate::objects::ObjectPart>(
            entity.common(),
            crate::objects::PART_APP,
        )
        .and_then(|part| doc.get_entity(Handle::new(part.owner)))
        .unwrap_or(entity);
        let definition = crate::objects::instance(owner)
            .and_then(|instance| self.object_type(&instance.type_name).ok());
        if definition.as_ref().is_some_and(|d| d.room_void) {
            return true;
        }
        !matches!(entity, EntityType::Insert(_))
            && !definition.is_some_and(|d| {
                matches!(
                    d.geometry,
                    crate::objects::GeometryKind::Point | crate::objects::GeometryKind::Hosted
                )
            })
    }

    /// Bounds of a block definition are computed once per local walk, without flattening.
    /// Array extrema need only their four corner instances, even for a million cells.
    #[allow(clippy::too_many_arguments)]
    fn room_source_bounds(
        &self,
        doc: &opencadcodec::CadDocument,
        entity: &EntityType,
        xf: &Xform,
        cache: &mut BlockBounds,
        depth: usize,
        layer: &str,
        policy: &BoundaryLayers,
    ) -> Option<[[f64; 2]; 2]> {
        let empty = Some([[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]]);
        if !self.room_source(doc, entity) {
            return empty;
        }
        let EntityType::Insert(insert) = entity else {
            return if policy.accepts(layer) {
                qiyas_geom::FlattenOutcome::source_bounds(doc, entity, xf)
            } else {
                empty
            };
        };
        if policy
            .exclude_layers
            .iter()
            .any(|s| s.eq_ignore_ascii_case(layer))
        {
            return empty;
        }
        if depth >= 32 || qiyas_geom::is_xref_block(doc, &insert.block_name) {
            return None;
        }
        let key = (
            doc as *const _ as usize,
            insert.block_name.to_uppercase(),
            layer.to_owned(),
        );
        if !cache.contains_key(&key) {
            // A cycle has no reliable finite bounds. The flattener reports its depth budget.
            cache.insert(key.clone(), None);
            let mut bounds = Some([[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]]);
            for child in doc
                .entities_in_block(&insert.block_name)
                .filter(|e| !e.common().invisible && self.room_source(doc, e))
            {
                let child_layer = if child.common().layer == "0" {
                    layer
                } else {
                    &child.common().layer
                };
                let Some(child) = self.room_source_bounds(
                    doc,
                    child,
                    &Xform::identity(),
                    cache,
                    depth + 1,
                    child_layer,
                    policy,
                ) else {
                    bounds = None;
                    break;
                };
                if let Some(bounds) = &mut bounds {
                    for axis in 0..2 {
                        bounds[0][axis] = bounds[0][axis].min(child[0][axis]);
                        bounds[1][axis] = bounds[1][axis].max(child[1][axis]);
                    }
                }
            }
            cache.insert(key.clone(), bounds);
        }
        let [lo, hi] = cache.get(&key).copied().flatten()?;
        if lo[0] > hi[0] {
            return Some([lo, hi]);
        }
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        let (sin, cos) = insert.rotation.sin_cos();
        for column in [0, insert.column_count.max(1) - 1] {
            for row in [0, insert.row_count.max(1) - 1] {
                let mut corner = insert.clone();
                corner.column_count = 1;
                corner.row_count = 1;
                let dx = f64::from(column) * insert.column_spacing;
                let dy = f64::from(row) * insert.row_spacing;
                corner.insert_point.x += cos * dx - sin * dy;
                corner.insert_point.y += sin * dx + cos * dy;
                let place = qiyas_geom::insert_instances(doc, &corner, xf).next()?;
                for point in [
                    [lo[0], lo[1]],
                    [lo[0], hi[1]],
                    [hi[0], lo[1]],
                    [hi[0], hi[1]],
                ] {
                    let point = place.apply(point);
                    for axis in 0..2 {
                        bounds[0][axis] = bounds[0][axis].min(point[axis]);
                        bounds[1][axis] = bounds[1][axis].max(point[axis]);
                    }
                }
            }
        }
        bounds
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(bounds)
    }

    pub(crate) fn room_boundary_in(
        &self,
        sheet: &str,
        point: [f64; 2],
        policy: &BoundaryLayers,
    ) -> Boundary {
        self.room_boundary_detailed_in(sheet, point, policy)
            .unwrap_or(Boundary::TooDense)
    }

    pub(crate) fn room_boundary_detailed_in(
        &self,
        sheet: &str,
        point: [f64; 2],
        policy: &BoundaryLayers,
    ) -> Result<Boundary, RoomBudget> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("draw.room_boundary.room_boundary_in");
        if !point.iter().all(|v| v.is_finite()) {
            return Ok(Boundary::None);
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
        let seed = point.map(f64::to_bits);
        // Share the source fingerprint across local graphs. It can be large on an import.
        let key: Arc<str> = {
            let cache = self.room_cache.borrow();
            let mut seen = std::collections::HashSet::new();
            cache
                .graphs
                .iter()
                .rev()
                .find(|(previous, _)| {
                    seen.insert(Arc::as_ptr(previous) as *const ()) && previous.as_ref() == key
                })
                .map(|(previous, _)| previous.clone())
                .unwrap_or_else(|| Arc::from(key))
        };
        for (previous, graph) in &self.room_cache.borrow().graphs {
            if Arc::ptr_eq(previous, &key) {
                if let Some(boundary) = graph.boundaries.lock().expect("room boundaries").get(&seed)
                {
                    return Ok(boundary.clone());
                }
            }
        }
        // A metre is a search scale, not a design assumption. As with Hatch, grow fourfold
        // until the entire face fits, or every eligible source has been considered.
        let mut radius = 1.0 / self.drawing.units.to_metres();
        loop {
            let lo = [point[0] - radius, point[1] - radius];
            let hi = [point[0] + radius, point[1] + radius];
            if !lo.iter().chain(&hi).all(|v| v.is_finite()) {
                return Ok(Boundary::None);
            }
            let mut graph = self.room_source_graph(&sheet, policy, lo, hi, radius)?;
            graph.context = json!([
                sheet,
                policy,
                point,
                radius,
                self.drawing.units.to_metres(),
                self.measure_tol(),
                crate::knowledge::list(&self.drawing.doc, crate::knowledge::Kind::Types).ok()
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
                graph.reuse_unaffected(previous);
            }
            let reused = graph
                .boundaries
                .lock()
                .expect("room boundaries")
                .get(&seed)
                .cloned();
            let boundary = reused.unwrap_or_else(|| {
                let tolerance = (radius * 1e-6).max(1e-6);
                let lines: Vec<_> = graph
                    .segments
                    .iter()
                    .map(|&[x0, y0, x1, y1]| KernelLine {
                        start: [x0, y0],
                        end: [x1, y1],
                    })
                    .collect();
                let faces = bounded_faces(&lines, Tolerance::new(tolerance));
                let found = region_from_faces(&faces, point, lo, hi)
                    .map(|mut found| {
                        if let Boundary::Ring { holes, .. } = &mut found {
                            holes.retain(|hole| {
                                hole.iter()
                                    .zip(hole.iter().cycle().skip(1))
                                    .take(hole.len())
                                    .all(|(&a, &b)| {
                                        islands::supports_edge(
                                            a,
                                            b,
                                            &graph.island_segments,
                                            self.measure_tol() * 2.0,
                                        )
                                    })
                            });
                        }
                        with_bulges(found, &graph.rounds, self.measure_tol())
                    })
                    .unwrap_or(Boundary::None);
                graph
                    .faces
                    .lock()
                    .expect("room faces")
                    .insert(tolerance.to_bits(), faces);
                found
            });
            if matches!(boundary, Boundary::Ring { .. }) || graph.whole_sheet {
                graph
                    .boundaries
                    .lock()
                    .expect("room boundaries")
                    .insert(seed, boundary.clone());
                let mut cache = self.room_cache.borrow_mut();
                // One completed local graph per seed, bounded independently of document history.
                // Keep a complete maximum-size Rooms query warm for retained room readbacks.
                let mut cached_segments: usize =
                    cache.graphs.iter().map(|(_, g)| g.segments.len()).sum();
                while cache.graphs.len() >= 512
                    || cached_segments + graph.segments.len() > BOUNDARY_SEGMENTS * 4 * 8
                {
                    let (_, removed) = cache.graphs.remove(0);
                    cached_segments -= removed.segments.len();
                }
                cache.graphs.push((key, Arc::new(graph)));
                return Ok(boundary);
            }
            radius *= 4.0;
        }
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
            } else if self.room_source(doc, entity)
                && (matches!(entity, EntityType::Insert(_))
                    || policy.accepts(&entity.common().layer))
            {
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
        lo: [f64; 2],
        hi: [f64; 2],
        radius: f64,
    ) -> Result<SourceGraph, RoomBudget> {
        self.room_source_graph_with_limits(
            sheet,
            policy,
            [lo, hi],
            radius,
            [BOUNDARY_SEGMENTS * 4, qiyas_geom::MAX_WORK],
        )
    }

    fn room_source_graph_with_limits(
        &self,
        sheet: &str,
        policy: &BoundaryLayers,
        [lo, hi]: [[f64; 2]; 2],
        radius: f64,
        [segment_limit, work_limit]: [usize; 2],
    ) -> Result<SourceGraph, RoomBudget> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("draw.room_boundary.room_source_graph");
        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        let mut segments = Vec::new();
        let mut island_segments = Vec::new();
        let mut rounds = Vec::new();
        let mut walls = BTreeMap::new();
        let mut budget = work_limit;
        let whole_sheet = std::cell::Cell::new(true);
        let counts = std::cell::RefCell::new(RoomCounts::default());
        let mut bounds_cache = BTreeMap::new();
        let margin = 2.0 * (radius * 1e-6).max(1e-6);
        let near_lo = lo.map(|v| v - margin);
        let near_hi = hi.map(|v| v + margin);
        let sheet = self.sheet_block(Some(sheet));
        let mut add_path =
            |points: &[[f64; 2]], closed: bool, island: bool, layer: &str, block: Option<&str>| {
                let mut edge = |a: [f64; 2], b: [f64; 2]| {
                    let segment = [a[0], a[1], b[0], b[1]];
                    if ![a, b]
                        .iter()
                        .all(|p| (0..2).all(|i| p[i] >= lo[i] && p[i] <= hi[i]))
                    {
                        whole_sheet.set(false);
                    }
                    if !meets(
                        [a[0].min(b[0]), a[1].min(b[1])],
                        [a[0].max(b[0]), a[1].max(b[1])],
                        near_lo,
                        near_hi,
                    ) {
                        return;
                    }
                    counts.borrow_mut().segment(layer, block);
                    segments.push(segment);
                    if island {
                        island_segments.push(segment);
                    }
                };
                for pair in points.windows(2) {
                    edge(pair[0], pair[1]);
                }
                if closed && points.len() > 2 {
                    edge(points[points.len() - 1], points[0]);
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
            let block = match entity {
                EntityType::Insert(i) => Some(i.block_name.as_str()),
                _ => None,
            };
            let before = budget;
            let mut accept = |source_doc: &opencadcodec::CadDocument,
                              source: &EntityType,
                              xf: &Xform,
                              layer: &str| {
                if !self.room_source(source_doc, source) {
                    return false;
                }
                if matches!(source, EntityType::Insert(_)) {
                    // An insert on another layer can contain explicitly selected child layers.
                    if policy
                        .exclude_layers
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case(layer))
                    {
                        return false;
                    }
                } else if !policy.accepts(layer) {
                    return false;
                }
                if let Some([min, max]) = self.room_source_bounds(
                    source_doc,
                    source,
                    xf,
                    &mut bounds_cache,
                    0,
                    layer,
                    policy,
                ) {
                    if min[0] > max[0] {
                        return false;
                    }
                    if !(0..2).all(|i| min[i] >= lo[i] && max[i] <= hi[i]) {
                        whole_sheet.set(false);
                    }
                    if !meets(min, max, near_lo, near_hi) {
                        return false;
                    }
                }
                for piece in snap_shapes(source_doc, source, xf, self.measure_tol())
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
                true
            };
            let flat = qiyas_geom::FlattenOutcome::continuous_filtered_within(
                doc,
                &xrefs,
                entity,
                &Xform::identity(),
                self.measure_tol(),
                &mut budget,
                &mut accept,
            );
            counts
                .borrow_mut()
                .work(&entity.common().layer, block, before - budget);
            let truncated = flat.truncated;
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
                // An explicit void block includes its nested outlines. An unmarked
                // enclosing block must not erase a void marker on a nested reference.
                let declared_block_void = std::iter::once(entity)
                    .chain(
                        path.chain
                            .iter()
                            .filter_map(|h| doc.get_entity(Handle::new(*h))),
                    )
                    .any(|e| matches!(e, EntityType::Insert(_)) && self.room_island_source(doc, e));
                let island = declared_block_void
                    || (self.room_island_source(doc, entity)
                        && self.room_island_source(located.doc, located.entity)
                        && path
                            .chain
                            .iter()
                            .filter_map(|h| doc.get_entity(Handle::new(*h)))
                            .all(|e| self.room_island_source(doc, e)));
                add_path(
                    &path.path.points,
                    path.path.closed,
                    island,
                    &path.layer,
                    block,
                );
            }
            if truncated {
                return Err(counts.borrow().failure(
                    if budget == 0 {
                        "flatten_work"
                    } else {
                        "block_depth"
                    },
                    if budget == 0 { work_limit } else { 32 },
                    work_limit - budget,
                ));
            }
            if counts.borrow().segments > segment_limit {
                return Err(counts.borrow().failure(
                    "segments",
                    segment_limit,
                    work_limit - budget,
                ));
            }
        }
        // Rebuild the joined wall faces without cuts. These virtual edges close doorways at
        // the internal face, while the actual drawing and Hatch keep their opening graphics.
        for spec in walls.values() {
            let runs = crate::walls::geom_runs(spec);
            let source: Vec<_> = runs
                .iter()
                .map(|r| {
                    (
                        &r.points,
                        r.closed,
                        r.thickness,
                        format!("{:?}", r.justification),
                    )
                })
                .collect();
            let key = json!([
                source,
                spec.thickness,
                spec.justification,
                self.measure_tol()
            ])
            .to_string();
            let cached = self
                .room_cache
                .borrow()
                .wall_faces
                .iter()
                .find(|(previous, _)| *previous == key)
                .map(|(_, rings)| rings.clone());
            let rings = if let Some(rings) = cached {
                rings
            } else {
                let Ok(wall) = qiyas_geom::walls::build_walls_justified(
                    &runs,
                    spec.thickness,
                    &[],
                    self.measure_tol(),
                    crate::walls::geom_justification(spec.justification),
                ) else {
                    // Invalid source geometry must not leave a partial enclosure usable.
                    return Ok(SourceGraph {
                        context: String::new(),
                        radius,
                        whole_sheet: true,
                        segments: Vec::new(),
                        island_segments: Vec::new(),
                        rounds: Vec::new(),
                        faces: Default::default(),
                        boundaries: Default::default(),
                    });
                };
                let rings: Arc<Rings> = Arc::new(
                    wall.regions
                        .into_iter()
                        .flat_map(|region| std::iter::once(region.outer).chain(region.holes))
                        .collect(),
                );
                let mut cache = self.room_cache.borrow_mut();
                if cache.wall_faces.len() >= 64 {
                    cache.wall_faces.remove(0);
                }
                cache.wall_faces.push((key, rings.clone()));
                rings
            };
            let layer = &crate::walls::resolved_layers(&spec.layers)[0];
            for ring in rings.iter() {
                add_path(ring, true, true, layer, None);
            }
            if counts.borrow().segments > segment_limit {
                return Err(counts.borrow().failure(
                    "segments",
                    segment_limit,
                    work_limit - budget,
                ));
            }
        }
        if segments.len() > segment_limit {
            return Err(counts
                .borrow()
                .failure("segments", segment_limit, work_limit - budget));
        }
        Ok(SourceGraph {
            context: String::new(),
            radius,
            whole_sheet: whole_sheet.get(),
            segments,
            island_segments,
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
    fn local_budget_refusals_name_counts_layers_and_blocks_in_both_languages() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new_blank(&dir.path().join("budgets.dxf"), 6);
        let doc = &mut session.drawing.doc;
        let mut record = opencadcodec::BlockRecord::new("Dense block");
        record.handle = doc.allocate_handle();
        record.block_entity_handle = doc.allocate_handle();
        record.block_end_handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();
        let mut ring = LwPolyline::new();
        for [x, y] in [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]] {
            ring.add_point(Vector2::new(x, y));
        }
        ring.is_closed = true;
        ring.common.owner_handle = owner;
        doc.add_entity(EntityType::LwPolyline(ring)).unwrap();
        let mut insert = opencadcodec::entities::Insert::new("Dense block", Vector3::ZERO);
        insert.common.layer = "Dense layer".into();
        doc.add_entity(EntityType::Insert(insert)).unwrap();
        for (limits, english, arabic, count) in [
            ([3, 100], "boundary segments", "مقاطع الحدود", "4, limit 3"),
            ([100, 1], "flatten work", "عمل التفكيك", "1, limit 1"),
        ] {
            let error = session
                .room_source_graph_with_limits(
                    "*Model_Space",
                    &BoundaryLayers::default(),
                    [[-1.0; 2], [5.0; 2]],
                    6.0,
                    limits,
                )
                .err()
                .expect("the complete local graph must be refused");
            assert!(error.reason.contains(english), "{}", error.reason);
            assert!(error.reason.contains(count), "{}", error.reason);
            assert!(error.reason_ar.contains(arabic), "{}", error.reason_ar);
            for message in [&error.reason, &error.reason_ar] {
                assert!(message.contains("Dense layer"), "{message}");
                assert!(message.contains("Dense block"), "{message}");
            }
        }
    }

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
