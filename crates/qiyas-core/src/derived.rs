//! Ordered derivation inside the command's recorder. Dependencies are compared with the
//! state before this edit, so undo, rollback and reopened drawings never rely on a stale cache.
use crate::{CoreError, Session};
use opencadcodec::EntityType;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Entity snapshots share the codec's immutable storage; only edited entities are copied.
pub(crate) struct Inputs {
    entities: BTreeMap<u64, Arc<EntityType>>,
    dependencies: crate::evaluation::Dependencies,
    types: BTreeMap<String, Value>,
    legacy_objects: bool,
    objects: Value,
    measurement: Value,
    takeoff: Value,
    blocks: BTreeMap<u64, String>,
    visible_layers: Value,
}
impl Inputs {
    pub(crate) fn capture(s: &Session) -> Result<Self, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("derived.capture");
        use crate::knowledge::{self, Kind};
        let entries = |kind| knowledge::list(&s.drawing.doc, kind).map(|rows| json!(rows));
        let mut measurement = knowledge::list(&s.drawing.doc, Kind::Measurement)?;
        measurement.retain(|entry| entry.name != "result");
        let used_layers: BTreeSet<_> = s
            .drawing
            .doc
            .entities()
            .map(|e| e.common().layer.to_lowercase())
            .collect();
        let dependencies = {
            let _read = s.evaluation_read()?;
            s.evaluation.borrow().index.clone()
        };
        let legacy_objects = s.drawing.doc.entities().any(|entity| {
            dependencies
                .objects
                .get(&entity.common().handle.value())
                .map_or_else(
                    || {
                        crate::walls::wall_tag(entity)
                            .is_some_and(|tag| matches!(tag.role.as_str(), "run" | "opening"))
                            || crate::room_objects::room_object(entity).is_some()
                    },
                    |object| object.compatibility.is_some(),
                )
        });
        let types = knowledge::list(&s.drawing.doc, Kind::Types)?
            .into_iter()
            .map(|entry| (entry.name.to_ascii_lowercase(), json!(entry)))
            .collect();
        Ok(Self {
            dependencies,
            types,
            legacy_objects,
            entities: s
                .drawing
                .doc
                .entities()
                .map(|e| {
                    let h = e.common().handle;
                    (
                        h.value(),
                        s.drawing.doc.get_entity_arc(h).expect("listed entity"),
                    )
                })
                .collect(),
            objects: json!([
                entries(Kind::Levels)?,
                entries(Kind::Tables)?,
                s.drawing.units.to_metres(),
                s.annotation_scale()
            ]),
            takeoff: json!(s.takeoff),
            measurement: json!([
                measurement,
                entries(Kind::Recognition)?,
                entries(Kind::Revisions)?
            ]),
            blocks: s
                .drawing
                .doc
                .block_records
                .iter()
                .map(|b| (b.handle.value(), b.name.clone()))
                .collect(),
            visible_layers: json!(s
                .drawing
                .doc
                .layers
                .iter()
                .filter(|l| used_layers.contains(&l.name.to_lowercase()))
                .map(|l| (&l.name, l.flags.off, l.flags.frozen))
                .collect::<Vec<_>>()),
        })
    }
    fn types_changed(&self, now: &Self) -> bool {
        // New unused definitions do not change existing objects. Both catalogues stay
        // available so placing the first object of an existing type is not a type edit.
        if self.legacy_objects || now.legacy_objects {
            return self.types != now.types;
        }
        self.dependencies
            .objects
            .values()
            .chain(now.dependencies.objects.values())
            .any(|object| {
                let name = object.type_name.to_ascii_lowercase();
                self.types.get(&name) != now.types.get(&name)
            })
    }
    fn changed(&self, now: &Self) -> BTreeSet<u64> {
        self.entities
            .keys()
            .chain(now.entities.keys())
            .copied()
            .filter(|h| match (self.entities.get(h), now.entities.get(h)) {
                (Some(a), Some(b)) => !Arc::ptr_eq(a, b) && !crate::net_effect::same_entity(a, b),
                _ => true,
            })
            .collect()
    }
    /// Changes to block children also affect every reference of that block, recursively.
    fn expand_blocks(&self, now: &Self, dirty: &mut BTreeSet<u64>) {
        loop {
            let owners: BTreeSet<_> = dirty
                .iter()
                .flat_map(|h| [self.entities.get(h), now.entities.get(h)])
                .flatten()
                .filter_map(|e| {
                    self.blocks
                        .get(&e.common().owner_handle.value())
                        .or_else(|| now.blocks.get(&e.common().owner_handle.value()))
                        .map(|n| n.to_lowercase())
                })
                .collect();
            let old_len = dirty.len();
            for (h, e) in self.entities.iter().chain(&now.entities) {
                if let EntityType::Insert(i) = e.as_ref() {
                    if owners.contains(&i.block_name.to_lowercase()) {
                        dirty.insert(*h);
                    }
                }
            }
            if old_len == dirty.len() {
                break;
            }
        }
    }
    fn object_scope(&self, now: &Self, s: &Session) -> Result<BTreeSet<u64>, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("derived.object_scope");
        let _read = s.evaluation_read()?;
        let mut dirty = self.changed(now);
        self.expand_blocks(now, &mut dirty);
        let mut ids = BTreeSet::new();
        for h in &dirty {
            for state in [self, now] {
                if state.dependencies.objects.contains_key(h) {
                    ids.insert(*h);
                }
                if let Some(part) = state.entities.get(h).and_then(|e| {
                    crate::xdata::read_json::<crate::objects::ObjectPart>(
                        e.common(),
                        crate::objects::PART_APP,
                    )
                }) {
                    ids.insert(part.owner);
                }
            }
        }
        // Traverse both generations: removals and changed assembly membership also affect
        // the group left behind. Only true host/assembly edges expand the object scope.
        let mut pending: Vec<_> = ids.iter().copied().collect();
        while let Some(id) = pending.pop() {
            for state in [self, now] {
                let index = &state.dependencies;
                let Some(object) = index.objects.get(&id) else {
                    continue;
                };
                let mut neighbours: Vec<_> = index
                    .openings
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect();
                neighbours.extend(object.host);
                if matches!(
                    s.object_type(&object.type_name).map(|d| d.join),
                    Ok(crate::objects::JoinRule::Union)
                ) {
                    let owner = index.owners[&id];
                    let key = crate::evaluation::Dependencies::assembly(owner, object);
                    neighbours.extend(index.assemblies.get(&key).into_iter().flatten().copied());
                }
                for neighbour in neighbours {
                    if ids.insert(neighbour) {
                        pending.push(neighbour);
                    }
                }
            }
        }
        Ok(ids)
    }
    fn regions(&self, now: &Self, full: bool) -> BTreeSet<u64> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("derived.regions");
        let mut dirty = self.changed(now);
        self.expand_blocks(now, &mut dirty);
        let mut regions: BTreeSet<_> = now
            .dependencies
            .regions
            .values()
            .flatten()
            .filter_map(|(id, _)| (full || dirty.contains(id)).then_some(*id))
            .collect();
        for source in dirty {
            for index in [&self.dependencies, &now.dependencies] {
                regions.extend(
                    index
                        .source_regions
                        .get(&source)
                        .into_iter()
                        .flatten()
                        .filter(|id| now.entities.contains_key(*id))
                        .copied(),
                );
            }
        }
        regions
    }

    fn schedules(
        &self,
        now: &Self,
        object_ids: &BTreeSet<u64>,
        full: bool,
        _s: &Session,
    ) -> BTreeSet<u64> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("derived.schedules");
        let dirty = self.changed(now);
        let mut wanted = dirty;
        for index in [&self.dependencies, &now.dependencies] {
            for id in object_ids {
                wanted.extend(index.schedules.get(id).into_iter().flatten().copied());
            }
        }
        now.entities
            .iter()
            .filter_map(|(h, e)| {
                (full || wanted.contains(h))
                    .then(|| {
                        crate::xdata::read_json::<crate::objects::ObjectSchedule>(
                            e.common(),
                            crate::objects::SCHEDULE_APP,
                        )
                    })
                    .flatten()
                    .map(|_| *h)
            })
            .collect()
    }
}
pub(crate) fn enclosure_source(e: &EntityType) -> bool {
    !matches!(
        e,
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
    ) && crate::object_regions::retained_region(e).is_none()
        && !crate::object_tags::is_tag(e)
        && e.common()
            .extended_data
            .get_record(crate::comment::COMMENT_APP)
            .is_none()
        && e.common()
            .extended_data
            .get_record(crate::conventions::GRID_APP)
            .is_none()
}

/// Kept as the reference implementation for opening drawings and differential tests.
pub(crate) fn full(s: &mut Session) -> Result<(), CoreError> {
    #[cfg(any(test, feature = "fixtures"))]
    let _profile = crate::fixtures::profile::span("derived.full");
    s.migrate_legacy_objects()?;
    s.refresh_objects()?;
    if s.has_object_regions() {
        s.sync_legacy_wall_tags()?;
        s.park();
        s.refresh_object_regions()?;
        s.sync_legacy_room_regions()?;
        s.refresh_objects()?;
    }
    s.sync_legacy_wall_tags()?;
    s.refresh_opening_schedules()?;
    s.migrate_object_schedules()?;
    s.refresh_object_schedules()?;
    s.refresh_measurement()?;
    s.refresh_measurement_scope()?;
    s.follow_fields()
}
impl Session {
    pub(crate) fn derived_extras(&self) -> (bool, bool) {
        let exists = crate::knowledge::get(
            &self.drawing.doc,
            crate::knowledge::Kind::Measurement,
            "selection",
        )
        .is_ok_and(|entry| entry.is_some())
            || self.has_rooms()
            || self.has_object_regions()
            || self.has_opening_schedules()
            || self
                .drawing
                .doc
                .app_ids
                .contains(crate::objects::OBJECT_APP)
            || self.drawing.doc.app_ids.contains(crate::WALL_APP);
        let exists = exists
            || crate::knowledge::get(
                &self.drawing.doc,
                crate::knowledge::Kind::Measurement,
                "view_only",
            )
            .is_ok_and(|entry| entry.is_some());
        (exists, exists)
    }
    pub(crate) fn rederive(
        &mut self,
        before: Option<&Inputs>,
        requested: &mut crate::net_effect::RequestedChanges,
    ) -> Result<(), CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("derived.rederive");
        let Some(before) = before else {
            if self.derived_extras().0 {
                full(self)?;
            } else {
                self.follow_fields()?;
            }
            requested.observe(&self.drawing.doc, "new derived objects and linked fields");
            self.last_advice.extend(requested.advice(&self.drawing.doc));
            return Ok(());
        };
        let mut now = Inputs::capture(self)?;
        let all = before.objects != now.objects || before.types_changed(&now);
        let mut object_ids = before.object_scope(&now, self)?;
        if all || !object_ids.is_empty() {
            self.migrate_legacy_objects()?;
            self.refresh_objects_scoped(if all { None } else { Some(&object_ids) })?;
            self.sync_legacy_wall_tags()?;
            now = Inputs::capture(self)?;
        }
        let regions = before.regions(&now, all);
        if !regions.is_empty() {
            self.park();
            self.refresh_object_regions_scoped(Some(&regions))?;
            self.sync_legacy_room_regions()?;
            let after = Inputs::capture(self)?;
            let changed = now.object_scope(&after, self)?;
            self.refresh_objects_scoped(Some(&changed))?;
            object_ids.extend(changed);
            self.sync_legacy_wall_tags()?;
            now = Inputs::capture(self)?;
        }
        if regions.is_empty() && before.takeoff != now.takeoff {
            // New or edited take-off rows still need the retained region's validity and
            // quantity metadata. An empty geometry scope avoids retracing any enclosure.
            self.refresh_object_regions_scoped(Some(&regions))?;
        }
        requested.observe(&self.drawing.doc, "object geometry and hosted positions");
        let schedules = before.schedules(&now, &object_ids, all, self);
        if all || !object_ids.is_empty() || !schedules.is_empty() {
            self.refresh_opening_schedules()?;
            self.migrate_object_schedules()?;
            now = Inputs::capture(self)?;
            object_ids.extend(before.object_scope(&now, self)?);
            let schedules = before.schedules(&now, &object_ids, all, self);
            self.refresh_object_schedules_scoped(if all { None } else { Some(&schedules) })?;
        }
        requested.observe(&self.drawing.doc, "live schedules");
        if all
            || !object_ids.is_empty()
            || before.measurement != now.measurement
            || self.has_measurement_scope()
        {
            self.refresh_measurement()?;
        }
        self.refresh_measurement_scope()?;
        requested.observe(&self.drawing.doc, "measurement rules");
        let now = Inputs::capture(self)?;
        let mut dirty = before.changed(&now);
        before.expand_blocks(&now, &mut dirty);
        dirty.extend(object_ids);
        // Named block/opening counts also depend on insert membership and layer visibility.
        let counts = before.visible_layers != now.visible_layers
            || dirty.iter().any(|h| {
                [before.entities.get(h), now.entities.get(h)]
                    .into_iter()
                    .flatten()
                    .any(|e| {
                        matches!(e.as_ref(), EntityType::Insert(_))
                            || crate::objects::instance(e).is_some()
                    })
            });
        self.follow_fields_scoped(if all { None } else { Some(&dirty) }, counts)?;
        requested.observe(&self.drawing.doc, "linked fields");
        self.last_advice.extend(requested.advice(&self.drawing.doc));
        Ok(())
    }
}
#[cfg(test)]
#[path = "derived_tests.rs"]
mod tests;
