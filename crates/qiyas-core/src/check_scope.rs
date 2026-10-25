//! The computation scope of an edit check includes previous positions and reverse links.
//! Spatial boxes select candidates; each existing check still uses its exact predicate.
use std::collections::BTreeSet;

use opencadcodec::{EntityType, Handle};
use qiyas_geom::Xform;

use crate::{
    check::CheckScope,
    ink::{self, Bounds, SheetInk},
    Session,
};

fn overlaps(a: Bounds, b: Bounds) -> bool {
    (0..2).all(|i| a[0][i] <= b[1][i] && a[1][i] >= b[0][i])
}

impl Session {
    pub(crate) fn check_scope_handles(
        &self,
        scope: &CheckScope,
        sheet: &str,
        ink: &SheetInk,
        near: f64,
    ) -> Option<BTreeSet<u64>> {
        if scope.handles.is_empty() {
            return None;
        }
        let mut ids: BTreeSet<_> = scope.handles.iter().copied().collect();
        let Ok(_read) = self.evaluation_read() else {
            return Some(ids);
        };
        let index = self.evaluation.borrow().index.clone();
        let mut old_parts = std::collections::BTreeMap::<u64, Vec<u64>>::new();
        for (&id, entity) in &self.check_before {
            if let Some(part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            ) {
                old_parts.entry(part.owner).or_default().push(id);
            }
        }
        let mut pending: Vec<_> = ids.iter().copied().collect();
        while let Some(id) = pending.pop() {
            let mut next = Vec::new();
            next.extend(index.parts.get(&id).into_iter().flatten().copied());
            next.extend(old_parts.get(&id).into_iter().flatten().copied());
            next.extend(index.openings.get(&id).into_iter().flatten().copied());
            next.extend(index.fields.get(&id).into_iter().flatten().copied());
            next.extend(index.schedules.get(&id).into_iter().flatten().copied());
            for entity in [
                self.drawing.doc.get_entity(Handle::new(id)),
                self.check_before.get(&id).map(|e| e.as_ref()),
            ]
            .into_iter()
            .flatten()
            {
                if let Some(part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                    entity.common(),
                    crate::objects::PART_APP,
                ) {
                    next.push(part.owner);
                }
                if let Some(object) = crate::objects::instance(entity) {
                    next.extend(object.host);
                    if self
                        .object_type(&object.type_name)
                        .is_ok_and(|t| matches!(t.join, crate::objects::JoinRule::Union))
                    {
                        let key = crate::evaluation::Dependencies::assembly(
                            entity.common().owner_handle.value(),
                            &object,
                        );
                        next.extend(index.assemblies.get(&key).into_iter().flatten().copied());
                    }
                }
            }
            for id in next {
                if ids.insert(id) {
                    pending.push(id);
                }
            }
        }
        // Fields which were remapped or removed by this edit retain their old dependency.
        for (&holder, entity) in &self.check_before {
            if crate::field::field_dependencies(entity)
                .0
                .iter()
                .any(|h| ids.contains(h))
                || crate::object_tags::tag_dependency(entity).is_some_and(|h| ids.contains(&h))
            {
                ids.insert(holder);
            }
        }
        for entity in self.drawing.doc.entities_in_block(sheet) {
            if crate::object_tags::tag_dependency(entity).is_some_and(|h| ids.contains(&h)) {
                ids.insert(entity.common().handle.value());
            }
        }
        let mut boxes: Vec<_> = ink
            .pieces
            .iter()
            .filter(|p| ids.contains(&p.owner))
            .map(|p| p.bounds)
            .collect();
        for id in ids.clone() {
            let Some(old) = self.check_before.get(&id) else {
                continue;
            };
            // If an old reference's definition changed or disappeared, its old extent is
            // unavailable in the current document. Keep a full check for that uncommon case.
            let block = match old.as_ref() {
                EntityType::Insert(i) => Some(i.block_name.as_str()),
                EntityType::Table(t) => Some(t.block_name.as_str()),
                EntityType::Dimension(d) => Some(d.base().block_name.as_str()),
                _ => None,
            };
            if let Some(name) = block {
                let block = self.drawing.doc.block_records.get(name)?;
                if self
                    .check_before
                    .values()
                    .any(|e| e.common().owner_handle == block.handle)
                {
                    return None;
                }
            }
            let flattened = qiyas_geom::flatten(
                &self.drawing.doc,
                old,
                &Xform::identity(),
                self.measure_tol(),
            );
            let mut points = Vec::new();
            for path in flattened.paths {
                if let Some(text) = path.text {
                    // Use the same glyph bounds as current plotted text. The geometry
                    // fallback's estimated width can miss a former overlap after a move.
                    let laid = self.text.borrow_mut().lay_out_text(
                        &self.drawing.doc,
                        &self.xref_docs(),
                        &text,
                        self.arabic_reversed,
                    )?;
                    points.extend(laid.corners);
                } else {
                    points.extend(path.path.points);
                }
            }
            if !points.is_empty() {
                boxes.push(ink::bounds(&points));
            }
        }
        // Retained regions form reverse topology dependencies, including a failed enclosure
        // whose last valid ring is retained. Empty invalid regions must be reconsidered.
        let sources: Vec<_> = ids
            .iter()
            .flat_map(|id| {
                [
                    self.drawing.doc.get_entity(Handle::new(*id)),
                    self.check_before.get(id).map(|e| e.as_ref()),
                ]
            })
            .flatten()
            .collect();
        if (scope.rules.is_empty() || scope.rules.iter().any(|rule| rule.starts_with("room_")))
            && sources
                .iter()
                .any(|entity| crate::derived::enclosure_source(entity))
        {
            // Ordinary room names have no retained-region link. Follow the connected source
            // component to find candidate labels even deep inside a room after a wall removal.
            // Bounds deliberately overselect; the room rule still traces the exact enclosure.
            let extent = ink::union(ink.pieces.iter().map(|piece| piece.bounds));
            let tolerance = extent.map_or(self.measure_tol(), |b| {
                ((b[1][0] - b[0][0]).max(b[1][1] - b[0][1]) * 1.1e-6)
                    .max(self.measure_tol())
                    .max(1e-6)
            });
            let mut seen = BTreeSet::new();
            for seed in &boxes {
                let mut component = *seed;
                let mut pending = vec![*seed];
                while let Some(bounds) = pending.pop() {
                    for piece in ink.near(ink::expand(bounds, tolerance)) {
                        if piece.annotation
                            || piece.grid
                            || piece.table
                            || !self
                                .drawing
                                .doc
                                .get_entity(Handle::new(piece.owner))
                                .is_some_and(crate::derived::enclosure_source)
                        {
                            continue;
                        }
                        let key = (
                            piece.owner,
                            piece.bounds.map(|point| point.map(f64::to_bits)),
                        );
                        if seen.insert(key) {
                            component = ink::union([component, piece.bounds].into_iter()).unwrap();
                            pending.push(piece.bounds);
                        }
                    }
                }
                ids.extend(
                    ink.near(component)
                        .filter(|piece| piece.text.is_some())
                        .map(|piece| piece.owner),
                );
            }
        }
        for (&owner, regions) in &index.regions {
            for (region, policy) in regions {
                if !sources.iter().any(|e| {
                    e.common().owner_handle.value() == owner
                        && (crate::walls::wall_tag(e).is_some()
                            || policy.accepts(&e.common().layer))
                        && !matches!(
                            e,
                            EntityType::Text(_)
                                | EntityType::MText(_)
                                | EntityType::Table(_)
                                | EntityType::Dimension(_)
                        )
                }) {
                    continue;
                }
                if let Some(entity) = self.drawing.doc.get_entity(Handle::new(*region)) {
                    if let Some(retained) = crate::object_regions::retained_region(entity) {
                        let points: Vec<_> = qiyas_geom::flatten(
                            &self.drawing.doc,
                            entity,
                            &Xform::identity(),
                            self.measure_tol(),
                        )
                        .into_paths()
                        .into_iter()
                        .flat_map(|p| p.points)
                        .collect();
                        if !retained.valid
                            || points.is_empty()
                            || boxes.iter().any(|b| overlaps(*b, ink::bounds(&points)))
                        {
                            ids.insert(*region);
                        }
                    }
                }
            }
        }
        for bounds in &boxes {
            let expanded = [
                [bounds[0][0] - near, bounds[0][1] - near],
                [bounds[1][0] + near, bounds[1][1] + near],
            ];
            ids.extend(
                ink.near(expanded)
                    .filter(|piece| overlaps(expanded, piece.bounds))
                    .map(|piece| piece.owner),
            );
        }
        // A region's field holders may be outside the plan. They are dependencies regardless
        // of distance, and do not cause an expanding spatial flood across the whole sheet.
        let sources: Vec<_> = ids.iter().copied().collect();
        for id in sources {
            ids.extend(index.fields.get(&id).into_iter().flatten().copied());
            ids.extend(index.schedules.get(&id).into_iter().flatten().copied());
        }
        Some(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;
    use serde_json::json;

    #[test]
    fn removed_wall_rechecks_a_distant_plain_room_name_but_not_another_building() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("room-dependencies.dxf"), units);
            let k = 1.0 / s.drawing.units.to_metres();
            let mut walls = Vec::new();
            let mut labels = Vec::new();
            for x in [0.0, 30.0] {
                for (a, b) in [
                    ([x, 0.0], [x + 10.0, 0.0]),
                    ([x + 10.0, 0.0], [x + 10.0, 10.0]),
                    ([x + 10.0, 10.0], [x, 10.0]),
                    ([x, 10.0], [x, 0.0]),
                ] {
                    let result = s
                        .run_input(
                            &json!({"type":"draw","kind":"line",
                        "a":a.map(|v|v*k),"b":b.map(|v|v*k)}),
                            Source::User,
                        )
                        .unwrap();
                    walls.push(json!(result)["handles"][0].as_u64().unwrap());
                }
                let result = s
                    .run_input(
                        &json!({"type":"text","at":[(x+5.0)*k,5.0*k],
                    "height":0.2*k,"value":"Room"}),
                        Source::User,
                    )
                    .unwrap();
                labels.push(json!(result)["handles"][0].as_u64().unwrap());
            }
            s.run_input(
                &json!({"type":"delete_entities","handles":[walls[0]]}),
                Source::User,
            )
            .unwrap();
            let scope = CheckScope {
                handles: vec![walls[0]],
                rules: vec!["room_open".into()],
                ..Default::default()
            };
            let ink = s.sheet_ink("*Model_Space");
            let selected = s
                .check_scope_handles(&scope, "*Model_Space", &ink, 0.2 * k)
                .unwrap();
            assert!(
                selected.contains(&labels[0]),
                "lost room dependency in unit {units}"
            );
            assert!(
                !selected.contains(&labels[1]),
                "unrelated room selected in unit {units}"
            );
        }
    }

    #[test]
    fn computation_follows_previous_and_current_neighbours_in_every_unit() {
        for units in [4, 5, 6, 1] {
            let dir = tempfile::tempdir().unwrap();
            let mut s = Session::new_blank(&dir.path().join("scope.dxf"), units);
            let k = 1.0 / s.drawing.units.to_metres();
            let mut line = |a: [f64; 2], b: [f64; 2]| {
                let result = s
                    .run_input(
                        &json!({"type":"draw", "kind":"line",
                    "a":a.map(|v|v*k), "b":b.map(|v|v*k)}),
                        Source::User,
                    )
                    .unwrap();
                json!(result)["handles"][0].as_u64().unwrap()
            };
            let wall = line([0., 0.], [5., 0.]);
            let old = line([2., 0.05], [2.0005, 0.05]);
            let new = line([22., 0.05], [22.0005, 0.05]);
            let remote = line([100., 100.], [100.0005, 100.]);
            let scope = CheckScope {
                handles: vec![wall],
                rules: vec!["tiny_segment".into()],
                ..Default::default()
            };
            let selected = |s: &Session| {
                let ink = s.sheet_ink("*Model_Space");
                s.check_scope_handles(&scope, "*Model_Space", &ink, 0.2 * k)
                    .unwrap()
            };
            assert_eq!(selected(&s), [wall, old].into());
            s.run_input(&json!({"type":"modify","kind":"move","targets":[{"handle":wall}],"delta":[20.*k,0]}),Source::User).unwrap();
            assert_eq!(selected(&s), [wall, old, new].into(), "{units}");
            s.run_input(
                &json!({"type":"delete_entities","handles":[wall]}),
                Source::User,
            )
            .unwrap();
            assert_eq!(selected(&s), [wall, new].into(), "removal in {units}");
            assert!(!selected(&s).contains(&remote));
            assert!(s.undo());
            assert_eq!(selected(&s), [wall, new].into());
            assert!(s.redo());
            assert_eq!(selected(&s), [wall, new].into());
            // Computation includes neighbours, while the caller's requested findings
            // retain the established response contract.
            assert!(s.check_drawing(&scope).findings.is_empty());
            assert_eq!(
                s.check_drawing(&CheckScope {
                    handles: vec![],
                    ..scope
                })
                .findings
                .len(),
                3
            );
        }
    }
}
