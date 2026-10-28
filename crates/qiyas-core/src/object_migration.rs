//! Converts historical metadata to ordinary typed instances. Bundled definitions are data;
//! the shared object engine owns subsequent geometry, quantities and property bindings.

use std::collections::BTreeMap;

use opencadcodec::{EntityType, Handle};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::objects::{instance, ObjectInstance, ObjectPart, ObjectType, PART_APP};
use crate::walls::{explicit_runs, wall_tag};
use crate::{CoreError, DrawSpec, NewEntity, Session};

const SYMBOL_APP: &str = "QIYAS_OBJECT_SYMBOL_RECIPE";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct SymbolRecipe {
    base: String,
    specs: Value,
    #[serde(default)]
    adopted: bool,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

fn definitions() -> Result<BTreeMap<String, ObjectType>, CoreError> {
    serde_json::from_str(include_str!("../data/legacy-object-types.json"))
        .map_err(|e| invalid(format!("Legacy object definitions: {e}")))
}

impl Session {
    pub(crate) fn ensure_legacy_type(&mut self, kind: &str) -> Result<ObjectType, CoreError> {
        let definitions = definitions()?;
        let definition = definitions
            .get(kind)
            .ok_or_else(|| invalid("Unknown legacy element kind"))?;
        if let Some(entry) = knowledge::get(&self.drawing.doc, Kind::Types, &definition.name)? {
            return serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string()));
        }
        knowledge::put(
            &mut self.drawing.doc,
            Kind::Types,
            &definition.name,
            &json!(definition),
        )?;
        Ok(definition.clone())
    }

    /// Reuse only blocks with the same evaluated local recipe. A changed recipe receives a
    /// separate block so references with different property values retain their own symbols.
    pub(crate) fn legacy_symbol_block(
        &mut self,
        base: &str,
        specs: Vec<DrawSpec>,
    ) -> Result<String, CoreError> {
        let recipe = SymbolRecipe {
            base: base.into(),
            specs: json!(specs),
            adopted: false,
        };
        let mut matching: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let EntityType::Insert(insert) = entity else {
                    return None;
                };
                let cached: SymbolRecipe = crate::xdata::read_json(&insert.common, SYMBOL_APP)?;
                (cached == recipe && self.drawing.doc.block_records.contains(&insert.block_name))
                    .then(|| insert.block_name.clone())
            })
            .collect();
        matching.sort();
        if let Some(name) = matching.into_iter().next() {
            return Ok(name);
        }
        let name = if !self.drawing.doc.block_records.contains(base) {
            base.to_owned()
        } else {
            (1..)
                .map(|i| format!("{base}_Q{i}"))
                .find(|name| !self.drawing.doc.block_records.contains(name))
                .expect("available block name")
        };
        let mut entities = Vec::new();
        for spec in specs {
            for entity in self.draw_entities(spec)? {
                let mut data = crate::records::entity_data(&entity);
                data["common"] = json!({"layer":"0"});
                entities.push(NewEntity {
                    kind: crate::session::variant_name(&entity),
                    data,
                });
            }
        }
        let current = std::mem::take(&mut self.current);
        let created = if entities.is_empty() {
            // An empty symbol recipe is intentional; an INSERT may reference an empty block.
            let doc = &mut self.drawing.doc;
            let mut record = opencadcodec::BlockRecord::new(&name);
            record.handle = doc.allocate_handle();
            record.block_entity_handle = doc.allocate_handle();
            record.block_end_handle = doc.allocate_handle();
            record.units = doc.header.insertion_units;
            doc.block_records.add(record).map(|_| ()).map_err(invalid)
        } else {
            self.define_block_entities(&name, [0.0, 0.0], entities)
                .map(|_| ())
        };
        self.current = current;
        created?;
        Ok(name)
    }

    pub(crate) fn stamp_legacy_symbol_recipe(
        &mut self,
        handle: u64,
        base: &str,
        specs: &[DrawSpec],
    ) -> Result<(), CoreError> {
        self.stamp_symbol_recipe(
            handle,
            &SymbolRecipe {
                base: base.into(),
                specs: json!(specs),
                adopted: false,
            },
        )
    }

    fn stamp_symbol_recipe(&mut self, handle: u64, recipe: &SymbolRecipe) -> Result<(), CoreError> {
        let app = self.object_app(SYMBOL_APP)?;
        let entity = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(handle))
            .ok_or(CoreError::NoSuchEntity(handle))?;
        crate::xdata::write_json(entity.common_mut(), SYMBOL_APP, app, recipe)
    }

    /// Existing unmarked symbols are adopted on open, preserving hand-edited block geometry.
    /// Later recipe edits replace the referenced block while retaining each INSERT's identity
    /// and placement, including its swing reflection and any linked schedule/field sources.
    pub(crate) fn refresh_insert_symbol_recipes(&mut self) -> Result<(), CoreError> {
        let openings: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let EntityType::Insert(insert) = entity else {
                    return None;
                };
                let object = instance(entity)?;
                let cached = crate::xdata::read_json::<SymbolRecipe>(&insert.common, SYMBOL_APP);
                Some((
                    insert.common.handle.value(),
                    insert.block_name.clone(),
                    object,
                    cached,
                ))
            })
            .collect();
        let mut changed = Vec::new();
        for (handle, block, object, cached) in openings {
            let definition = self.object_type(&object.type_name)?;
            if !definition.plan.symbols_in_driver {
                continue;
            }
            let specs = self.symbol_recipe_for_properties(&definition, &object.props)?;
            let base = cached
                .as_ref()
                .map_or(block.as_str(), |cache| cache.base.as_str());
            let recipe = SymbolRecipe {
                base: base.into(),
                specs: json!(specs),
                adopted: cached.is_none(),
            };
            if cached
                .as_ref()
                .is_some_and(|old| old.base == recipe.base && old.specs == recipe.specs)
            {
                continue;
            }
            if cached.is_some() {
                let name = self.legacy_symbol_block(base, specs.clone())?;
                let Some(EntityType::Insert(insert)) =
                    self.drawing.doc.get_entity_mut(Handle::new(handle))
                else {
                    unreachable!("listed insert")
                };
                insert.block_name = name;
                insert.common.graphic_data = None;
                changed.push(Handle::new(handle));
            }
            self.stamp_symbol_recipe(handle, &recipe)?;
        }
        self.follow_edit(&changed, &[], &[], &[]);
        Ok(())
    }

    /// Adopt existing drivers and parts without replacing them on open. Once imported, the
    /// ordinary object engine owns their properties and all subsequent regeneration.
    pub(crate) fn migrate_legacy_objects(&mut self) -> Result<(), CoreError> {
        self.migrate_legacy_rooms()?;
        if !self.drawing.doc.app_ids.contains(crate::walls::WALL_APP) {
            return Ok(());
        }
        let members: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                wall_tag(e).map(|t| (e.common().handle.value(), e.common().owner_handle, t))
            })
            .collect();
        if members.is_empty() {
            return Ok(());
        }
        let definitions = definitions()?;
        // Older stored recipes predate layer-property bindings. Upgrade only that missing
        // contract, keeping the drawing's supplied geometry and quantity recipes intact.
        for template in definitions.values() {
            if let Some(entry) = knowledge::get(&self.drawing.doc, Kind::Types, &template.name)? {
                if entry.data["plan"].get("layer_properties").is_none() {
                    let mut data = entry.data;
                    data["plan"]["layer_properties"] = json!(template.plan.layer_properties);
                    data["plan"]["host_layer_property"] = json!(template.plan.host_layer_property);
                    knowledge::put(&mut self.drawing.doc, Kind::Types, &template.name, &data)?;
                    let drivers: Vec<_> = self
                        .drawing
                        .doc
                        .entities()
                        .filter_map(instance)
                        .filter(|o| o.type_name == template.name)
                        .collect();
                    for mut object in drivers {
                        for (role, property) in &template.plan.layer_properties {
                            if let Some(layer) = object.layers.get(role) {
                                object.props.insert(property.clone(), json!(layer));
                            }
                        }
                        self.stamp_object(object.id, &object)?;
                    }
                }
            }
        }
        let runs: BTreeMap<_, _> = members
            .iter()
            .filter(|(_, _, tag)| tag.role == "run")
            .map(|(h, sheet, tag)| ((sheet.value(), tag.group, tag.index), *h))
            .collect();
        // Stable handles can put an older run after a newer run when groups are joined.
        // The object engine gives shared graphics to the smallest surviving driver handle.
        let mut owners = BTreeMap::<(u64, u64), u64>::new();
        for (&(sheet, group, _), &handle) in &runs {
            owners
                .entry((sheet, group))
                .and_modify(|owner| *owner = (*owner).min(handle))
                .or_insert(handle);
        }
        let factor = self.drawing.units.to_metres();
        let mut converted = Vec::new();
        for (h, sheet, mut tag) in members.iter().cloned() {
            explicit_runs(&mut tag.spec);
            let (kind, props, host) = match tag.role.as_str() {
                "run" => {
                    let Some(run) = tag.spec.runs.get(tag.index) else {
                        continue;
                    };
                    (
                        "wall".to_owned(),
                        BTreeMap::from([
                            (
                                "width".into(),
                                json!(run.thickness.unwrap_or(tag.spec.thickness) * factor),
                            ),
                            (
                                "wall_height".into(),
                                json!(run.height.unwrap_or(tag.spec.height)),
                            ),
                            (
                                "justification".into(),
                                json!(run.justification.unwrap_or(tag.spec.justification)),
                            ),
                        ]),
                        None,
                    )
                }
                "opening" => {
                    let Some(opening) = tag.spec.openings.get(tag.index) else {
                        continue;
                    };
                    let Some(host) = runs.get(&(sheet.value(), tag.group, opening.run)) else {
                        continue;
                    };
                    let kind =
                        serde_json::to_value(opening.kind).map_err(|e| invalid(e.to_string()))?;
                    let mark = self
                        .drawing
                        .doc
                        .get_entity(Handle::new(h))
                        .and_then(crate::opening_schedule::mark)
                        .unwrap_or_default();
                    (
                        kind.as_str().expect("opening enum string").to_owned(),
                        BTreeMap::from([
                            ("width".into(), json!(opening.width * factor)),
                            (
                                "host_width".into(),
                                json!(
                                    tag.spec.runs[opening.run]
                                        .thickness
                                        .unwrap_or(tag.spec.thickness)
                                        * factor
                                ),
                            ),
                            ("distance".into(), json!(opening.at * factor)),
                            ("sill".into(), json!(opening.sill)),
                            ("head".into(), json!(opening.head)),
                            ("swing".into(), json!(opening.swing)),
                            ("mark".into(), json!(mark)),
                        ]),
                        Some(*host),
                    )
                }
                _ => continue,
            };
            let names = crate::walls::resolved_layers(&tag.spec.layers);
            let mut props = props;
            if kind == "wall" {
                for (role, layer) in ["outline", "hatch", "centreline", "doors", "windows"]
                    .into_iter()
                    .zip(&names)
                {
                    props.insert(format!("{role}_layer"), json!(layer));
                }
            }
            let layers = BTreeMap::from([
                ("outline".into(), names[0].clone()),
                ("hatch".into(), names[1].clone()),
                ("centreline".into(), names[2].clone()),
            ]);
            converted.push((
                h,
                kind,
                props,
                host,
                format!("{}:{}", sheet.value(), tag.group),
                layers,
            ));
        }
        for (h, kind, props, host, assembly, layers) in converted {
            let definition = &definitions[&kind];
            if knowledge::get(&self.drawing.doc, Kind::Types, &definition.name)?.is_none() {
                knowledge::put(
                    &mut self.drawing.doc,
                    Kind::Types,
                    &definition.name,
                    &json!(definition),
                )?;
            }
            let mut object = self
                .drawing
                .doc
                .get_entity(Handle::new(h))
                .and_then(instance)
                .unwrap_or(ObjectInstance {
                    id: h,
                    type_name: definition.name.clone(),
                    props: BTreeMap::new(),
                    level: None,
                    host,
                    assembly: None,
                    layers: BTreeMap::new(),
                    compatibility: Some(kind.clone()),
                    sources: BTreeMap::new(),
                });
            // Explicitly retyped objects belong to their supplied definition.
            if object.compatibility.is_none() {
                continue;
            }
            object.id = h;
            object.host = host;
            object.assembly = Some(assembly);
            object.layers = layers;
            object.compatibility = None;
            let mut props = props;
            // Earlier Phase 16 drawings may already contain the original compatibility type.
            // Do not add a new property that their stored definition does not declare.
            if !self
                .object_type(&object.type_name)?
                .properties
                .contains_key("host_width")
            {
                props.remove("host_width");
            }
            object.props.extend(props);
            self.stamp_object(h, &object)?;
        }
        let app = self.object_app(PART_APP)?;
        for (h, sheet, tag) in members {
            if matches!(tag.role.as_str(), "run" | "opening") {
                continue;
            }
            let Some(owner) = owners.get(&(sheet.value(), tag.group)) else {
                continue;
            };
            // Once converted, the object engine owns each part's role and index. Replacing
            // them with legacy per-role indices would change untouched wall groups whenever
            // another group is edited. New parts and changed owners still need migration.
            if self
                .drawing
                .doc
                .get_entity(Handle::new(h))
                .is_some_and(|entity| {
                    crate::xdata::read_json::<ObjectPart>(entity.common(), PART_APP)
                        .is_some_and(|part| part.owner == *owner)
                })
            {
                continue;
            }
            let entity = self
                .drawing
                .doc
                .get_entity_mut(Handle::new(h))
                .expect("listed member");
            crate::xdata::write_json(
                entity.common_mut(),
                PART_APP,
                app,
                &ObjectPart {
                    owner: *owner,
                    index: tag.index,
                    role: tag.role.clone(),
                },
            )?;
        }
        Ok(())
    }
}
