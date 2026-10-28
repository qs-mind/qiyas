//! Memoized quantity work over one immutable generation of the drawing. The codec's Arc
//! snapshots detect writes inside transactions, where document_revision has not advanced.
//! Cached values contain no document references and are never serialized or put in history.
mod encoding;

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use opencadcodec::EntityType;
use qiyas_param::expr::{Expr, Value as ExprValue};
use std::io::{self, Write};

use crate::{
    knowledge::{self, Kind},
    objects::{ObjectInstance, ObjectType},
    CoreError, Session,
};

#[derive(Default)]
pub(crate) struct Evaluation {
    generation: Option<Generation>,
    pub metadata_validated: bool,
    pub records: BTreeMap<u64, crate::objects::ObjectRecord>,
    pub index: Dependencies,
    pub types: BTreeMap<String, ObjectType>,
    pub expressions: BTreeMap<String, Expr>,
    pub contexts: BTreeMap<(String, usize), BTreeMap<String, ExprValue>>,
    pub bands: BTreeMap<String, Arc<crate::object_bands::BandDerived>>,
    pub band_faces: BTreeMap<String, Arc<BTreeMap<u64, crate::object_bands::FaceAreas>>>,
    pub room_keys: BTreeMap<String, String>,
}

/// Source relationships from the same immutable generation as the quantities. Rebuilding
/// on the next read makes additions, deletions, failed transactions and undo symmetric.
#[derive(Clone, Default)]
pub(crate) struct Dependencies {
    pub objects: BTreeMap<u64, ObjectInstance>,
    pub assemblies: BTreeMap<Assembly, Vec<u64>>,
    pub openings: BTreeMap<u64, Vec<u64>>,
    pub parts: BTreeMap<u64, Vec<u64>>,
    pub fields: BTreeMap<u64, BTreeSet<u64>>,
    pub field_holders: BTreeSet<u64>,
    pub field_order: Vec<u64>,
    pub count_fields: BTreeSet<u64>,
    pub owners: BTreeMap<u64, u64>,
    pub schedules: BTreeMap<u64, BTreeSet<u64>>,
    pub regions: BTreeMap<u64, Vec<(u64, crate::object_regions::BoundaryLayers)>>,
    pub source_regions: BTreeMap<u64, BTreeSet<u64>>,
}
type Assembly = (u64, String, Option<String>, Option<String>);
impl Dependencies {
    pub fn assembly(owner: u64, object: &ObjectInstance) -> Assembly {
        (
            owner,
            object.type_name.to_ascii_lowercase(),
            object.level.as_ref().map(|v| v.to_ascii_lowercase()),
            object.assembly.clone(),
        )
    }
    fn capture(s: &Session) -> Self {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("evaluation.capture.0");
        let mut index = Self::default();
        let mut schedules = Vec::new();
        for entity in s.drawing.doc.entities() {
            let handle = entity.common().handle.value();
            let owner = entity.common().owner_handle.value();
            index.owners.insert(handle, owner);
            if let Some(part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            ) {
                index.parts.entry(part.owner).or_default().push(handle);
            }
            if let Some(region) = crate::object_regions::retained_region(entity) {
                index
                    .regions
                    .entry(owner)
                    .or_default()
                    .push((handle, region.boundary_layers));
            }
            if let Some(spec) = crate::xdata::read_json::<crate::objects::ObjectSchedule>(
                entity.common(),
                crate::objects::SCHEDULE_APP,
            ) {
                schedules.push((handle, spec));
            }
            if entity
                .common()
                .extended_data
                .get_record(crate::field::FIELD_APP)
                .is_some()
            {
                index.field_holders.insert(handle);
                index.field_order.push(handle);
                let (sources, counts) = crate::field::field_dependencies(entity);
                for source in sources {
                    index.fields.entry(source).or_default().insert(handle);
                }
                if counts {
                    index.count_fields.insert(handle);
                }
            }
            let Some(mut object) = crate::objects::instance(entity) else {
                continue;
            };
            object.id = entity.common().handle.value();
            let key = Self::assembly(entity.common().owner_handle.value(), &object);
            index.assemblies.entry(key).or_default().push(object.id);
            if let Some(host) = object.host {
                index.openings.entry(host).or_default().push(object.id);
            }
            index.objects.insert(object.id, object);
        }
        // Keep conservative topology candidates. Exact face support and tolerance decide
        // reuse in the room graph; a distant source can still change the weld tolerance.
        for entity in s
            .drawing
            .doc
            .entities()
            .filter(|e| crate::derived::enclosure_source(e))
        {
            let all_layers =
                matches!(entity, EntityType::Insert(_)) || crate::walls::wall_tag(entity).is_some();
            let candidates: BTreeSet<_> = index
                .regions
                .get(&entity.common().owner_handle.value())
                .into_iter()
                .flatten()
                .filter(|(_, policy)| all_layers || policy.accepts(&entity.common().layer))
                .map(|(id, _)| *id)
                .collect();
            if !candidates.is_empty() {
                index
                    .source_regions
                    .insert(entity.common().handle.value(), candidates);
            }
        }
        let types: BTreeMap<_, _> = knowledge::list(&s.drawing.doc, Kind::Types)
            .unwrap_or_default()
            .into_iter()
            .map(|entry| (entry.name.to_ascii_lowercase(), entry.data))
            .collect();
        for (schedule, spec) in schedules {
            let block = spec
                .filter
                .sheet
                .as_ref()
                .map(|sheet| s.sheet_block(Some(sheet)));
            let owner = block
                .as_ref()
                .and_then(|name| s.drawing.doc.block_records.get(name))
                .map(|b| b.handle.value());
            for (&id, object) in &index.objects {
                if spec.ids.as_ref().is_none_or(|ids| ids.contains(&id))
                    && spec
                        .filter
                        .type_name
                        .as_ref()
                        .is_none_or(|name| name.eq_ignore_ascii_case(&object.type_name))
                    && spec.filter.level.as_ref().is_none_or(|level| {
                        object
                            .level
                            .as_ref()
                            .is_some_and(|v| v.eq_ignore_ascii_case(level))
                    })
                    && spec.filter.category.as_ref().is_none_or(|category| {
                        types
                            .get(&object.type_name.to_ascii_lowercase())
                            .and_then(|t| t["category"].as_str())
                            .is_some_and(|v| v.eq_ignore_ascii_case(category))
                    })
                    && (block.is_none() || index.owners.get(&id).copied() == owner)
                {
                    index.schedules.entry(id).or_default().insert(schedule);
                }
            }
        }
        index
    }
}

struct Generation {
    entities: Vec<Arc<EntityType>>,
    inputs: Vec<u8>,
}

/// Compare exact serialized bytes without constructing an intermediate JSON tree or a
/// second output buffer. This also distinguishes signed zero; no hash can hide a change.
struct CompareBytes<'a>(&'a [u8]);
impl Write for CompareBytes<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.0.starts_with(bytes) {
            return Err(io::Error::other("evaluation inputs changed"));
        }
        self.0 = &self.0[bytes.len()..];
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Generation {
    fn write_inputs(s: &Session, output: impl Write) -> Result<(), serde_json::Error> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("evaluation.inputs");
        let doc = &s.drawing.doc;
        encoding::write(
            output,
            &(
                // Knowledge lives in doc.objects below. Compare its raw source records:
                // decoding an unrelated malformed definition must not make this read fail.
                s.drawing.units.to_metres(),
                s.measure_tol(),
                &doc.block_records,
                &doc.layers,
                &doc.header,
                &doc.dim_styles,
                &doc.text_styles,
                &doc.line_types,
                &doc.objects,
                &doc.context_scales,
                &doc.block_visibility_params,
                &doc.block_representations,
            ),
        )
    }
    fn matches(&self, s: &Session) -> bool {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("evaluation.matches");
        let mut compared = CompareBytes(&self.inputs);
        self.entities.len() == s.drawing.doc.entities().count()
            && self
                .entities
                .iter()
                .zip(s.drawing.doc.entities())
                .all(|(old, now)| std::ptr::eq(Arc::as_ptr(old), now))
            && Self::write_inputs(s, &mut compared).is_ok()
            && compared.0.is_empty()
    }
    fn capture(s: &Session) -> Result<Self, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("evaluation.capture.1");
        let mut inputs = Vec::new();
        Self::write_inputs(s, &mut inputs).map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
        Ok(Self {
            entities: s
                .drawing
                .doc
                .entities()
                .map(|e| {
                    s.drawing
                        .doc
                        .get_entity_arc(e.common().handle)
                        .expect("listed entity")
                })
                .collect(),
            inputs,
        })
    }
}

/// Holding this read guard borrows Session immutably, so no nested quantity read can write.
/// The next outer read compares actual inputs even if an earlier write failed or was undone.
pub(crate) struct Read<'a>(&'a Cell<usize>);
impl Drop for Read<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
impl Session {
    pub(crate) fn evaluation_read(&self) -> Result<Read<'_>, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("evaluation.evaluation_read");
        if self.evaluation_depth.get() == 0 {
            let mut cache = self.evaluation.borrow_mut();
            if cache.generation.as_ref().is_none_or(|g| !g.matches(self)) {
                *cache = Evaluation {
                    generation: Some(Generation::capture(self)?),
                    index: Dependencies::capture(self),
                    ..Evaluation::default()
                };
            }
        }
        // Keep guard depth outside the borrowed payload: a returned cache value's temporary
        // Ref can outlive this guard under Rust 2021's temporary-drop rules.
        self.evaluation_depth.set(self.evaluation_depth.get() + 1);
        Ok(Read(&self.evaluation_depth))
    }
    pub(crate) fn evaluation_expression(&self, text: &str) -> Result<Expr, CoreError> {
        if let Some(expr) = self.evaluation.borrow().expressions.get(text) {
            return Ok(expr.clone());
        }
        let expr = Expr::parse(text).map_err(|e| CoreError::InvalidCommand(e.to_string()))?;
        self.evaluation
            .borrow_mut()
            .expressions
            .insert(text.into(), expr.clone());
        Ok(expr)
    }
    pub(crate) fn discard_evaluation(&mut self) {
        *self.evaluation.get_mut() = Evaluation::default();
    }
}
