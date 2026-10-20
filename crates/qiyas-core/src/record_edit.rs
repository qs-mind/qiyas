//! Editing records and making entities generically: `Command::SetRecords` sets any property of
//! an entity, layer, text style or a few header variables by JSON pointer into its record's
//! `data` (serialise, set, deserialise: the drawing library checks every type), and
//! `Command::CreateEntities` adds an entity of any type it knows from a default and the fields
//! given. `record_schema` says, for a record type, every path, its value type and whether it can
//! be written (and with which command when not), from the types traced off their serde
//! (`serde_reflection`).
//!
//! An edit runs what the specific commands run after them: the take-off measures the entity
//! again, fields are worked out, a dimension's or table's block is drawn again; constraints of
//! an edited entity go (`param::Reshape::Edited`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use opencadcodec::document::HeaderVariables;
use opencadcodec::tables::{Layer, TextStyle};
use opencadcodec::{Color, EntityType, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serde_reflection::{
    ContainerFormat, Error as TraceError, Format, Named, Registry, Samples, Tracer, TracerConfig,
    VariantFormat,
};

use crate::records::{entity_data, Collection};
use crate::session::variant_name;
use crate::{CoreError, Outcome, Session};

/// One property to set: the value at the JSON pointer `path` in the data of the record `key`
/// (an entity's handle, a layer's or text style's name, "header").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecordUpdate {
    pub key: Value,
    pub path: String,
    pub value: Value,
    /// The value read before: the edit is refused when the record holds another now (null or
    /// left out: no check).
    #[serde(default)]
    pub expected: Option<Value>,
}

/// One entity to make: its type ("Line", "LwPolyline", "Text"…) and the fields to give it,
/// shaped as its record's `data` (missing ones take the type's defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NewEntity {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: Value,
}

/// Whether a path of a record can be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rule {
    Write,
    /// Refused: why, and the command that does it instead.
    Refuse {
        why: &'static str,
        via: Option<&'static str>,
    },
}

const IDENTITY: Rule = Rule::Refuse {
    why: "identity and ownership stay as they are",
    via: None,
};

/// The rule for `path` of an entity of type `kind`.
pub(crate) fn entity_rule(kind: &str, path: &str) -> Rule {
    let under = |prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
    if [
        "/common/handle",
        "/common/owner_handle",
        "/common/reactors",
        "/common/xdictionary_handle",
    ]
    .iter()
    .any(|p| under(p))
    {
        return IDENTITY;
    }
    if under("/common/extended_data") {
        return Rule::Refuse {
            why: "extended data holds Qiyas's own marks (reading order, fields, constraints, comments)",
            via: None,
        };
    }
    match kind {
        "Text" | "MText" if under("/value") => Rule::Refuse {
            why: "text is stored in reading order and marked",
            via: Some("edit_text"),
        },
        "AttributeEntity" if under("/value") => Rule::Refuse {
            why: "attributes are set on their block reference",
            via: Some("edit_attributes"),
        },
        // Its attributes are placed with it: moving, turning or scaling it moves them too.
        "Insert" if !under("/common") => Rule::Refuse {
            why: "a block reference's place, scale and attributes go together",
            via: Some("modify (move, rotate, scale), set_property or edit_attributes"),
        },
        "Dimension" if path.contains("block_name") => Rule::Refuse {
            why: "a dimension's block is its own",
            via: Some("dim_edit"),
        },
        "Table" if !under("/common") => Rule::Refuse {
            why: "a table's cells, rows and columns have their own commands",
            via: Some("edit_cell"),
        },
        _ => Rule::Write,
    }
}

/// The rule for `path` of a layer.
fn layer_rule(path: &str) -> Rule {
    match path {
        "/name" => Rule::Refuse {
            why: "a layer is renamed with its entities and the take-off",
            via: Some("rename_layer"),
        },
        "/handle" => IDENTITY,
        _ => Rule::Write,
    }
}

fn style_rule(path: &str) -> Rule {
    match path {
        "/name" | "/handle" => IDENTITY,
        _ => Rule::Write,
    }
}

/// The header variables `SetRecords` may set (kept with the tables in the undo step).
pub(crate) const HEADER_WRITABLE: [&str; 13] = [
    "/linetype_scale",
    "/point_display_mode",
    "/point_display_size",
    "/text_height",
    "/fill_mode",
    "/mirror_text",
    "/lineweight_display",
    "/linear_unit_format",
    "/linear_unit_precision",
    "/angular_unit_format",
    "/angular_unit_precision",
    "/attribute_visibility",
    "/project_name",
];

fn header_rule(path: &str) -> Rule {
    if HEADER_WRITABLE.contains(&path) {
        return Rule::Write;
    }
    if path == "/insertion_units" {
        return Rule::Refuse {
            why: "the units decide every measurement: they are chosen when the drawing is made",
            via: Some("new_drawing"),
        };
    }
    if path.starts_with("/dim_") {
        return Rule::Refuse {
            why: "dimension variables follow the dimension style",
            via: Some("set_dim_style"),
        };
    }
    if path.starts_with("/current_annotation_scale") || path == "/annotation_scale_value" {
        return Rule::Refuse {
            why: "the annotation scale has its own command",
            via: Some("annotation_scale"),
        };
    }
    Rule::Refuse {
        why: "this header variable is read-only here",
        via: None,
    }
}

/// The command to use instead of `set_records` for a collection that has its own commands.
fn collection_via(collection: Collection) -> Option<&'static str> {
    Some(match collection {
        Collection::DimStyles => "set_dim_style",
        Collection::Blocks => "redefine_block",
        Collection::Layouts => "mview",
        Collection::Groups => "add_to_group",
        Collection::Views => "save_view",
        Collection::LayerStates => "save_layer_state",
        Collection::Xrefs => "(the View tab's external references)",
        Collection::Takeoff => "set_description",
        Collection::Boq => "set_boq_tolerance",
        Collection::Conflicts => "resolve_conflict",
        Collection::Schedules => "(read-only)",
        Collection::Comments => "edit_text",
        Collection::Constraints => "add_constraint",
        Collection::Parameters => "set_parameter",
        Collection::Linetypes => "(read-only)",
        Collection::Entities
        | Collection::Viewports
        | Collection::Layers
        | Collection::TextStyles
        | Collection::Header => return None,
    })
}

fn refuse(rule: Rule, what: &str, path: &str) -> Result<(), CoreError> {
    match rule {
        Rule::Write => Ok(()),
        Rule::Refuse { why, via } => Err(invalid(match via {
            Some(via) => format!("{what} {path} can't be set here ({why}): use {via}"),
            None => format!("{what} {path} can't be set ({why})"),
        })),
    }
}

/// Sets each update's value in `data`, checking `expected`.
fn apply(data: &mut Value, updates: &[RecordUpdate], what: &str) -> Result<(), CoreError> {
    for u in updates {
        if !u.path.starts_with('/') {
            return Err(invalid(format!(
                "{what}: the path {} must be a JSON pointer such as /common/layer",
                u.path
            )));
        }
        let slot = data.pointer_mut(&u.path).ok_or_else(|| {
            invalid(format!(
                "{what} has no {} (record_schema lists its paths)",
                u.path
            ))
        })?;
        if let Some(expected) = &u.expected {
            if slot != expected {
                return Err(invalid(format!(
                    "{what} {} changed since it was read: it is {} now",
                    u.path, slot
                )));
            }
        }
        *slot = u.value.clone();
    }
    Ok(())
}

/// Reads `wire` back as a `T`, naming the field that does not fit.
fn read_back<T: serde::de::DeserializeOwned>(wire: Value, what: &str) -> Result<T, CoreError> {
    serde_path_to_error::deserialize(wire)
        .map_err(|e| invalid(format!("{what}: {} at {}", e.inner(), e.path())))
}

/// The updates grouped by record key, in the order first given.
fn by_key(updates: Vec<RecordUpdate>) -> Vec<(Value, Vec<RecordUpdate>)> {
    let mut groups: Vec<(Value, Vec<RecordUpdate>)> = Vec::new();
    for u in updates {
        match groups.iter_mut().find(|(k, _)| *k == u.key) {
            Some((_, list)) => list.push(u),
            None => groups.push((u.key.clone(), vec![u])),
        }
    }
    groups
}

impl Session {
    /// Checks every record replacement against the target's shape without changing the
    /// drawing. Kept copies also make expected values follow earlier updates to the same key.
    pub(crate) fn validate_record_inputs(
        &self,
        input: &Value,
    ) -> crate::validation::ValidationReport {
        use crate::validation::{InputIssue, ValidationReport};
        let mut report = ValidationReport::default();
        if input["type"] != "set_records" {
            return report;
        }
        let Ok(collection) = serde_json::from_value::<Collection>(input["collection"].clone())
        else {
            return report;
        };
        if collection_via(collection).is_some() {
            return report;
        }
        let Some(updates) = input["updates"].as_array() else {
            return report;
        };
        let mut staged = BTreeMap::<String, (String, Value, Value, Value)>::new();
        for (index, update) in updates.iter().enumerate() {
            let (Some(key), Some(path), Some(value)) = (
                update.get("key"),
                update["path"].as_str(),
                update.get("value"),
            ) else {
                continue;
            };
            // Batch references are checked statically first, then this check runs on their
            // resolved values before the step executes.
            if has_step_reference(update) {
                continue;
            }
            let prefix = format!("/updates/{index}");
            let mut error = |code: &str, field: &str, message: String| {
                report.valid = false;
                report.errors.push(InputIssue {
                    code: code.into(),
                    pointer: format!("{prefix}/{field}"),
                    message,
                });
            };
            let cache_key = key
                .as_str()
                .map(opencadcodec::tables::normalize_name)
                .unwrap_or_else(|| key.to_string());
            if !staged.contains_key(&cache_key) {
                match self.record_input_data(collection, key) {
                    Ok((kind, data)) => match record_input_schema(&kind) {
                        Ok(schema) => {
                            staged.insert(cache_key.clone(), (kind, data.clone(), schema, data));
                        }
                        Err(why) => {
                            error("invalid_value", "key", why.to_string());
                            continue;
                        }
                    },
                    Err((code, why)) => {
                        error(code, "key", why);
                        continue;
                    }
                }
            }
            let (kind, data, schema, baseline) = staged.get_mut(&cache_key).expect("loaded above");
            let rule = match collection {
                Collection::Entities | Collection::Viewports => entity_rule(kind, path),
                Collection::Layers => layer_rule(path),
                Collection::TextStyles => style_rule(path),
                Collection::Header => header_rule(path),
                _ => unreachable!("only writable collections"),
            };
            if let Err(why) = refuse(rule, kind, path) {
                error("refused", "path", why.to_string());
                continue;
            }
            let Some(previous) = data.pointer(path).filter(|_| path.starts_with('/')) else {
                error(
                    "not_found",
                    "path",
                    format!("{kind} has no {path} (record_schema lists its paths)"),
                );
                continue;
            };
            if update
                .get("expected")
                .filter(|value| !value.is_null())
                .is_some_and(|expected| expected != previous)
            {
                error(
                    "stale",
                    "expected",
                    format!("{kind} {path} changed since it was read: it is {previous} now"),
                );
            }
            if let Some(value_schema) = input_schema_at(schema, data, path) {
                let mut checked = crate::validation::validate_schema(&value_schema, value);
                let codec_error = if checked.errors.is_empty() {
                    None
                } else {
                    let mut probe = baseline.clone();
                    if let Some(slot) = probe.pointer_mut(path) {
                        *slot = value.clone();
                        record_input_error(kind, probe)
                    } else {
                        None
                    }
                };
                for issue in checked.errors.iter_mut().chain(&mut checked.warnings) {
                    let field = format!("{path}{}", issue.pointer)
                        .split('/')
                        .skip(1)
                        .map(|part| part.replace("~1", "/").replace("~0", "~"))
                        .collect::<Vec<_>>()
                        .join(".");
                    issue.message = format!("{kind}.{field}: {}", issue.message);
                    issue.pointer = format!("{prefix}/value{}", issue.pointer);
                }
                if let (Some(issue), Some(message)) = (checked.errors.first_mut(), codec_error) {
                    issue.message.push_str(&format!("; {message}"));
                }
                report.append(checked);
            }
            if let Some(original) = baseline.pointer(path) {
                missing_replacement_fields(
                    kind,
                    baseline,
                    original,
                    value,
                    path,
                    &format!("{prefix}/value"),
                    &mut report,
                );
            }
            *data.pointer_mut(path).expect("read above") = value.clone();
        }
        report
    }

    fn record_input_data(
        &self,
        collection: Collection,
        key: &Value,
    ) -> Result<(String, Value), (&'static str, String)> {
        let missing = || {
            (
                "not_found",
                format!("The drawing has no {collection:?} record {key}"),
            )
        };
        let bad_key = || {
            (
                "invalid_value",
                format!("Invalid {collection:?} record key {key}"),
            )
        };
        let data = match collection {
            Collection::Entities | Collection::Viewports => {
                let handle = key.as_u64().ok_or_else(bad_key)?;
                let entity = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(handle))
                    .ok_or_else(missing)?;
                return Ok((variant_name(entity), entity_data(entity)));
            }
            Collection::Layers => {
                let name = key.as_str().ok_or_else(bad_key)?;
                let value = self.drawing.doc.layers.get(name).ok_or_else(missing)?;
                ("Layer", serde_json::to_value(value))
            }
            Collection::TextStyles => {
                let name = key.as_str().ok_or_else(bad_key)?;
                let value = self.drawing.doc.text_styles.get(name).ok_or_else(missing)?;
                ("TextStyle", serde_json::to_value(value))
            }
            Collection::Header if key == "header" => (
                "HeaderVariables",
                serde_json::to_value(&self.drawing.doc.header),
            ),
            _ => return Err(bad_key()),
        };
        data.1
            .map(|value| (data.0.into(), value))
            .map_err(|why| ("invalid_value", why.to_string()))
    }

    /// `Command::SetRecords`.
    pub(crate) fn set_records(
        &mut self,
        collection: Collection,
        updates: Vec<RecordUpdate>,
    ) -> Result<Outcome, CoreError> {
        if updates.is_empty() {
            return Err(invalid("Give at least one update"));
        }
        if let Some(via) = collection_via(collection) {
            return Err(invalid(format!(
                "{collection:?} records are changed with {via}, not set_records"
            )));
        }
        match collection {
            Collection::Entities | Collection::Viewports => self.set_entity_records(updates),
            Collection::Layers => self.set_layer_records(updates),
            Collection::TextStyles => self.set_style_records(updates),
            Collection::Header => self.set_header_records(updates),
            _ => unreachable!("collection_via covers the rest"),
        }
    }

    fn set_entity_records(&mut self, updates: Vec<RecordUpdate>) -> Result<Outcome, CoreError> {
        let sheets: BTreeSet<Handle> = self
            .drawing
            .sheets()
            .into_iter()
            .filter_map(|s| {
                self.drawing
                    .doc
                    .block_records
                    .get(&s.block)
                    .map(|b| b.handle)
            })
            .collect();
        let mut changed = Vec::new();
        let mut rebuild_dims = Vec::new();
        let mut rebuild_tables = Vec::new();
        for (key, list) in by_key(updates) {
            let h = key
                .as_u64()
                .ok_or_else(|| invalid(format!("An entity's key is its handle, not {key}")))?;
            let handle = Handle::new(h);
            let old = self
                .drawing
                .doc
                .get_entity(handle)
                .ok_or(CoreError::NoSuchEntity(h))?
                .clone();
            let kind = variant_name(&old);
            let what = format!("{kind} {h:X}");
            if !sheets.contains(&old.common().owner_handle) {
                return Err(invalid(format!(
                    "{what} is inside a block: edit the block in place first (edit_block_in_place)"
                )));
            }
            if self.is_locked(handle) {
                return Err(invalid(format!("{what} is on a locked layer")));
            }
            for u in &list {
                refuse(entity_rule(&kind, &u.path), &what, &u.path)?;
            }
            let mut data = entity_data(&old);
            apply(&mut data, &list, &what)?;
            let new = entity_from_record(&kind, data, &old, &what)?;
            self.check_entity_refs(&new, &what)?;
            if new.common().handle != handle
                || new.common().owner_handle != old.common().owner_handle
            {
                return Err(invalid(format!("{what}: its identity must stay")));
            }
            match &new {
                EntityType::Dimension(_) => rebuild_dims.push(handle),
                EntityType::Table(_) => rebuild_tables.push(handle),
                _ => {}
            }
            self.drawing.doc.replace_entity_arc(handle, Arc::new(new));
            changed.push(handle);
        }
        for h in rebuild_dims {
            self.rebuild_dimension(h)?;
        }
        for h in rebuild_tables {
            self.rebuild_table(h)?;
        }
        let unlinked = self.follow_takeoff(&changed, &[]);
        self.follow_fields()?;
        Ok(if unlinked == 0 {
            Outcome::Changed
        } else {
            Outcome::TakeoffFollowed {
                remeasured: 0,
                unlinked,
            }
        })
    }

    /// The layer, linetype and text style an entity names are the drawing's.
    fn check_entity_refs(&self, e: &EntityType, what: &str) -> Result<(), CoreError> {
        let doc = &self.drawing.doc;
        let c = e.common();
        if !doc.layers.contains(&c.layer) {
            return Err(invalid(format!(
                "{what}: the drawing has no layer {} (new_layer makes one)",
                c.layer
            )));
        }
        let lt = c.linetype.trim();
        if !(lt.is_empty()
            || lt.eq_ignore_ascii_case("ByLayer")
            || lt.eq_ignore_ascii_case("ByBlock")
            || doc.line_types.contains(lt))
        {
            return Err(invalid(format!("{what}: the drawing has no linetype {lt}")));
        }
        let style = match e {
            EntityType::Text(t) => Some(&t.style),
            EntityType::MText(t) => Some(&t.style),
            _ => None,
        };
        if let Some(style) = style.filter(|s| !s.is_empty()) {
            if !doc.text_styles.contains(style) {
                return Err(invalid(format!(
                    "{what}: the drawing has no text style {style}"
                )));
            }
        }
        Ok(())
    }

    fn set_layer_records(&mut self, updates: Vec<RecordUpdate>) -> Result<Outcome, CoreError> {
        for (key, list) in by_key(updates) {
            let name = key
                .as_str()
                .ok_or_else(|| invalid(format!("A layer's key is its name, not {key}")))?;
            let what = format!("Layer {name}");
            let old = self
                .drawing
                .doc
                .layers
                .get(name)
                .ok_or_else(|| invalid(format!("The drawing has no layer {name}")))?
                .clone();
            if old.name.contains('|') {
                return Err(invalid(format!(
                    "{what} belongs to an xref: it is read-only"
                )));
            }
            for u in &list {
                refuse(layer_rule(&u.path), &what, &u.path)?;
            }
            let mut data = serde_json::to_value(&old).map_err(|e| invalid(e.to_string()))?;
            apply(&mut data, &list, &what)?;
            let new: Layer = read_back(data, &what)?;
            if new.flags.frozen && old.name.eq_ignore_ascii_case(&self.current.layer) {
                return Err(invalid(format!(
                    "{what} is the current layer: it can't be frozen"
                )));
            }
            if !(new.line_type.is_empty() || self.drawing.doc.line_types.contains(&new.line_type)) {
                return Err(invalid(format!(
                    "{what}: the drawing has no linetype {}",
                    new.line_type
                )));
            }
            *self.drawing.doc.layers.get_mut(name).expect("read above") = new;
        }
        Ok(Outcome::Changed)
    }

    fn set_style_records(&mut self, updates: Vec<RecordUpdate>) -> Result<Outcome, CoreError> {
        for (key, list) in by_key(updates) {
            let name = key
                .as_str()
                .ok_or_else(|| invalid(format!("A text style's key is its name, not {key}")))?;
            let what = format!("Text style {name}");
            let old = self
                .drawing
                .doc
                .text_styles
                .get(name)
                .ok_or_else(|| invalid(format!("The drawing has no text style {name}")))?
                .clone();
            for u in &list {
                refuse(style_rule(&u.path), &what, &u.path)?;
            }
            let mut data = serde_json::to_value(&old).map_err(|e| invalid(e.to_string()))?;
            apply(&mut data, &list, &what)?;
            let new: TextStyle = read_back(data, &what)?;
            *self
                .drawing
                .doc
                .text_styles
                .get_mut(name)
                .expect("read above") = new;
        }
        Ok(Outcome::Changed)
    }

    fn set_header_records(&mut self, updates: Vec<RecordUpdate>) -> Result<Outcome, CoreError> {
        for u in &updates {
            if u.key != json!("header") {
                return Err(invalid("The header's key is \"header\""));
            }
            refuse(header_rule(&u.path), "Header", &u.path)?;
        }
        let mut data =
            serde_json::to_value(&self.drawing.doc.header).map_err(|e| invalid(e.to_string()))?;
        apply(&mut data, &updates, "Header")?;
        let new: HeaderVariables = read_back(data, "Header")?;
        // Only the variables that may be set come over (the undo step keeps exactly those).
        let h = &mut self.drawing.doc.header;
        h.linetype_scale = new.linetype_scale;
        h.point_display_mode = new.point_display_mode;
        h.point_display_size = new.point_display_size;
        h.text_height = new.text_height;
        h.fill_mode = new.fill_mode;
        h.mirror_text = new.mirror_text;
        h.lineweight_display = new.lineweight_display;
        h.linear_unit_format = new.linear_unit_format;
        h.linear_unit_precision = new.linear_unit_precision;
        h.angular_unit_format = new.angular_unit_format;
        h.angular_unit_precision = new.angular_unit_precision;
        h.attribute_visibility = new.attribute_visibility;
        h.project_name = new.project_name.clone();
        Ok(Outcome::Changed)
    }

    /// `Command::CreateEntities`: the entities' handles.
    pub(crate) fn create_entities(
        &mut self,
        entities: Vec<NewEntity>,
        sheet: Option<String>,
    ) -> Result<Vec<u64>, CoreError> {
        let block = self.sheet_block(sheet.as_deref());
        self.create_entities_in_block(entities, &block)
    }

    /// Creates the supplied record entities directly in `block`, preserving the same defaults
    /// and validation whether the owner is a sheet or a named block definition.
    pub(crate) fn create_entities_in_block(
        &mut self,
        entities: Vec<NewEntity>,
        block: &str,
    ) -> Result<Vec<u64>, CoreError> {
        if entities.is_empty() {
            return Err(invalid("Give at least one entity"));
        }
        let owner = self
            .drawing
            .doc
            .block_records
            .get(block)
            .map(|b| b.handle)
            .ok_or_else(|| CoreError::NoSuchSheet(block.to_string()))?;
        let mut handles = Vec::with_capacity(entities.len());
        for (index, wanted) in entities.into_iter().enumerate() {
            let what = format!("Entity {index} ({})", wanted.kind);
            let base = default_entity(&wanted.kind)?;
            let kind = variant_name(&base);
            let mut data = entity_data(&base);
            {
                // The current state first; what the caller gives wins.
                let c = &self.current;
                let common = &mut data["common"];
                common["layer"] = json!(c.layer);
                common["color"] = serde_json::to_value(
                    crate::draw::parse_color(&c.color).unwrap_or(Color::ByLayer),
                )
                .unwrap_or_default();
                common["linetype"] = json!(if c.linetype.eq_ignore_ascii_case("ByLayer") {
                    String::new()
                } else {
                    c.linetype.clone()
                });
                common["line_weight"] =
                    serde_json::to_value(opencadcodec::LineWeight::from_value(c.lineweight))
                        .unwrap_or_default();
            }
            if let Some(given) = wanted.data.get("common").and_then(Value::as_object) {
                for key in given.keys() {
                    let path = format!("/common/{key}");
                    refuse(entity_rule(&kind, &path), &what, &path)?;
                }
            }
            merge(&mut data, &wanted.data);
            let mut entity: EntityType = read_back(json!({ kind.clone(): data }), &what)?;
            let layer = entity.common().layer.trim().to_string();
            if layer.contains('|') {
                return Err(invalid(format!(
                    "{what}: a layer of an xref can't take entities"
                )));
            }
            if !self.drawing.doc.layers.contains(&layer) {
                self.ensure_layer(&layer, Color::Index(7));
            }
            self.check_entity_refs(&entity, &what)?;
            if matches!(entity, EntityType::Text(_) | EntityType::MText(_)) {
                self.register_text_app();
                crate::text::mark_logical(entity.common_mut());
            }
            if matches!(entity, EntityType::Wipeout(_)) {
                self.drawing.doc.classes.update_defaults();
            }
            let common = entity.common_mut();
            common.handle = Handle::NULL;
            common.owner_handle = owner;
            let h = self
                .drawing
                .doc
                .add_entity(entity)
                .map_err(|e| invalid(format!("{what}: {e}")))?;
            handles.push(h.value());
        }
        Ok(handles)
    }
}

fn has_step_reference(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("$step") || object.values().any(has_step_reference)
        }
        Value::Array(values) => values.iter().any(has_step_reference),
        _ => false,
    }
}

/// Serde reflection describes types but does not retain serde(default). Probe one omission at
/// a time against the original valid record, so defaults and optional fields stay optional.
fn missing_replacement_fields(
    kind: &str,
    baseline: &Value,
    original: &Value,
    replacement: &Value,
    record_path: &str,
    result_path: &str,
    report: &mut crate::validation::ValidationReport,
) {
    match (original, replacement) {
        (Value::Object(old), Value::Object(new)) => {
            for (field, old_value) in old {
                let escaped = field.replace('~', "~0").replace('/', "~1");
                let next_record = format!("{record_path}/{escaped}");
                let next_result = format!("{result_path}/{escaped}");
                if let Some(value) = new.get(field) {
                    missing_replacement_fields(
                        kind,
                        baseline,
                        old_value,
                        value,
                        &next_record,
                        &next_result,
                        report,
                    );
                } else {
                    let mut probe = baseline.clone();
                    let Some(object) = probe
                        .pointer_mut(record_path)
                        .and_then(Value::as_object_mut)
                    else {
                        continue;
                    };
                    object.remove(field);
                    if let Some(message) = required_record_field(kind, probe) {
                        report.valid = false;
                        report.errors.push(crate::validation::InputIssue {
                            code: "missing_field".into(),
                            pointer: next_result,
                            message,
                        });
                    }
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            for (index, (old_value, value)) in old.iter().zip(new).enumerate() {
                missing_replacement_fields(
                    kind,
                    baseline,
                    old_value,
                    value,
                    &format!("{record_path}/{index}"),
                    &format!("{result_path}/{index}"),
                    report,
                );
            }
        }
        _ => {}
    }
}

fn required_record_field(kind: &str, data: Value) -> Option<String> {
    record_input_error(kind, data).filter(|message| message.starts_with("missing field "))
}

fn record_input_error(kind: &str, data: Value) -> Option<String> {
    fn diagnostic<T: serde::de::DeserializeOwned>(wire: Value) -> Option<String> {
        let error = serde_path_to_error::deserialize::<_, T>(wire).err()?;
        Some(format!("{} at {}", error.inner(), error.path()))
    }
    match kind {
        "Layer" => diagnostic::<Layer>(data),
        "TextStyle" => diagnostic::<TextStyle>(data),
        "HeaderVariables" => diagnostic::<HeaderVariables>(data),
        _ => diagnostic::<EntityType>(json!({kind:data})),
    }
}

/// The schema of an existing field, following array indexes and the current enum variant.
fn input_schema_at(schema: &Value, data: &Value, path: &str) -> Option<Value> {
    fn descend(schema: &Value, data: &Value, parts: &[String]) -> Option<Value> {
        let Some((first, rest)) = parts.split_first() else {
            return Some(schema.clone());
        };
        if let Some(variants) = schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
        {
            return variants
                .iter()
                .filter(|variant| crate::validation::validate_schema(variant, data).valid)
                .find_map(|variant| descend(variant, data, parts));
        }
        if let Some(index) = first.parse::<usize>().ok().filter(|_| data.is_array()) {
            let sub = schema
                .get("prefixItems")
                .and_then(|items| items.get(index))
                .or_else(|| schema.get("items"))?;
            return descend(sub, data.get(index)?, rest);
        }
        let sub = schema
            .get("properties")
            .and_then(|properties| properties.get(first))
            .or_else(|| {
                schema
                    .get("additionalProperties")
                    .filter(|value| value.is_object())
            })?;
        descend(sub, data.get(first)?, rest)
    }
    let parts: Vec<String> = path
        .strip_prefix('/')?
        .split('/')
        .map(|part| part.replace("~1", "/").replace("~0", "~"))
        .collect();
    descend(schema, data, &parts)
}

/// The entity of type `kind` that the record `data` describes, with the storage-only data serde
/// leaves out (DWG handles, flags) taken from `old`, the entity it was read from, and from its
/// attributes for a block reference.
pub fn entity_from_record(
    kind: &str,
    data: Value,
    old: &EntityType,
    what: &str,
) -> Result<EntityType, CoreError> {
    let mut new: EntityType = read_back(json!({ kind: data }), what)?;
    new.preserve_storage_data_from(old);
    if let (EntityType::Insert(new), EntityType::Insert(old)) = (&mut new, old) {
        for attribute in &mut new.attributes {
            let Some(before) = old
                .attributes
                .iter()
                .find(|a| a.common.handle == attribute.common.handle)
            else {
                continue;
            };
            let mut wrapped = EntityType::AttributeEntity(attribute.clone());
            wrapped.preserve_storage_data_from(&EntityType::AttributeEntity(before.clone()));
            if let EntityType::AttributeEntity(kept) = wrapped {
                *attribute = kept;
            }
        }
    }
    Ok(new)
}

/// `given` merged over `base`: objects key by key, anything else replaced.
fn merge(base: &mut Value, given: &Value) {
    match (base, given) {
        (Value::Object(b), Value::Object(g)) => {
            for (k, v) in g {
                match b.get_mut(k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, given) if !given.is_null() => *slot = given.clone(),
        _ => {}
    }
}

/// A new entity of the type named `kind` (any case) as the drawing library makes it.
fn default_entity(kind: &str) -> Result<EntityType, CoreError> {
    use opencadcodec::entities as e;
    let k = kind.trim().to_ascii_lowercase();
    let via = |cmd: &str| {
        Err(invalid(format!(
            "A {kind} is made with {cmd}, which builds what it needs"
        )))
    };
    Ok(match k.as_str() {
        "point" => EntityType::Point(e::Point::default()),
        "line" => EntityType::Line(e::Line::default()),
        "circle" => EntityType::Circle(e::Circle::default()),
        "arc" => EntityType::Arc(e::Arc::default()),
        "ellipse" => EntityType::Ellipse(e::Ellipse::default()),
        "lwpolyline" => EntityType::LwPolyline(e::LwPolyline::default()),
        "polyline2d" => EntityType::Polyline2D(e::Polyline2D::default()),
        "polyline3d" => EntityType::Polyline3D(e::Polyline3D::default()),
        "text" => EntityType::Text(e::Text::default()),
        "mtext" => EntityType::MText(e::MText::default()),
        "spline" => EntityType::Spline(e::Spline::default()),
        "ray" => EntityType::Ray(e::Ray::default()),
        "xline" => EntityType::XLine(e::XLine::default()),
        "hatch" => EntityType::Hatch(e::Hatch::default()),
        "leader" => EntityType::Leader(e::Leader::default()),
        "mline" => EntityType::MLine(e::MLine::default()),
        "tolerance" => EntityType::Tolerance(e::Tolerance::default()),
        "wipeout" => EntityType::Wipeout(e::Wipeout::default()),
        "dimension" => return via("dimension"),
        "table" => return via("table"),
        "viewport" => return via("mview"),
        "insert" => return via("insert_block"),
        "rasterimage" => return via("attach_image"),
        "underlay" => return via("attach_pdf_underlay"),
        "attributedefinition" => return via("att_def"),
        "attributeentity" => return via("edit_attributes"),
        _ => {
            return Err(invalid(format!(
                "Entities of type {kind} can't be made here: record_schema lists the types"
            )))
        }
    })
}

/// Creation and validation share the same refusals and command suggestions.
pub(crate) fn validate_entity_creation(kind: &str) -> Result<(), CoreError> {
    default_entity(kind).map(|_| ())
}

/// The entity types `create_entities` makes.
pub const CREATABLE: [&str; 18] = [
    "Point",
    "Line",
    "Circle",
    "Arc",
    "Ellipse",
    "LwPolyline",
    "Polyline2D",
    "Polyline3D",
    "Text",
    "MText",
    "Spline",
    "Ray",
    "XLine",
    "Hatch",
    "Leader",
    "MLine",
    "Tolerance",
    "Wipeout",
];

// ---- record_schema ----

/// Every type the records hold, traced once off their serde and kept as JSON (the tracer's
/// registry is not thread-safe); read back for each call.
fn traced() -> Registry {
    static TRACED: OnceLock<String> = OnceLock::new();
    let text = TRACED.get_or_init(|| {
        let mut names = BTreeSet::new();
        let registry = loop {
            match trace_with(&names) {
                Ok(registry) => break registry,
                Err(missing) => {
                    let before = names.len();
                    names.extend(missing);
                    if names.len() == before {
                        // No progress: give what was traced.
                        break trace_unchecked(&names);
                    }
                }
            }
        };
        serde_json::to_string(&registry).unwrap_or_default()
    });
    serde_json::from_str(text).unwrap_or_default()
}

fn trace_passes(tracer: &mut Tracer, names: &BTreeSet<String>) {
    let samples = Samples::new();
    for _ in 0..10_000 {
        let _ = tracer.trace_type_once::<EntityType>(&samples);
        let _ = tracer.trace_type_once::<Layer>(&samples);
        let _ = tracer.trace_type_once::<TextStyle>(&samples);
        let _ = tracer.trace_type_once::<HeaderVariables>(&samples);
        let mut progress = tracer.check_incomplete_enum("EntityType").is_some();
        for n in names {
            progress |= tracer.check_incomplete_enum(n).is_some();
        }
        if !progress {
            break;
        }
    }
}

fn trace_with(names: &BTreeSet<String>) -> Result<Registry, Vec<String>> {
    let mut tracer = Tracer::new(TracerConfig::default());
    trace_passes(&mut tracer, names);
    match tracer.registry() {
        Ok(registry) => Ok(registry),
        Err(TraceError::MissingVariants(list)) => Err(list),
        Err(_) => Err(Vec::new()),
    }
}

fn trace_unchecked(names: &BTreeSet<String>) -> Registry {
    let mut tracer = Tracer::new(TracerConfig::default());
    trace_passes(&mut tracer, names);
    tracer.registry_unchecked()
}

/// How deep `record_schema` lists nested fields.
const SCHEMA_DEPTH: usize = 4;

/// `Command::RecordSchema`: the paths of the record type `name` ("Line", "LwPolyline", "Layer",
/// "TextStyle", "header", or a collection such as "layers"), each with its value type and whether
/// it can be set, and the shapes of the named types they use.
pub fn record_schema(name: &str) -> Result<Value, CoreError> {
    let registry = &traced();
    let n = name.trim();
    let (type_name, rule): (String, &dyn Fn(&str) -> Rule) = match n.to_ascii_lowercase().as_str() {
        "layer" | "layers" => ("Layer".into(), &layer_rule),
        "textstyle" | "text_style" | "text_styles" => ("TextStyle".into(), &style_rule),
        "header" | "headervariables" => ("HeaderVariables".into(), &header_rule),
        _ => {
            let found = registry
                .keys()
                .find(|k| k.eq_ignore_ascii_case(n))
                .cloned()
                .filter(|k| entity_type_names(registry).contains(k))
                .ok_or_else(|| {
                    invalid(format!(
                        "No record type {n}: entity types are {}; also layer, text_style, header",
                        entity_type_names(registry).join(", ")
                    ))
                })?;
            let kind = found.clone();
            return Ok(describe(registry, &found, &move |p: &str| {
                entity_rule(&kind, p)
            }));
        }
    };
    Ok(describe(registry, &type_name, rule))
}

/// The JSON shape accepted inside a record's data. Struct fields are optional because entity
/// creation merges them over the entity defaults; any supplied value still has its exact type.
/// This uses the same serde trace as record_schema, including nested fields and enum payloads.
pub fn record_input_schema(name: &str) -> Result<Value, CoreError> {
    static SCHEMAS: OnceLock<Mutex<BTreeMap<String, Value>>> = OnceLock::new();
    let mut schemas = SCHEMAS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let key = name.trim().to_ascii_lowercase();
    if let Some(schema) = schemas.get(&key) {
        return Ok(schema.clone());
    }
    let registry = traced();
    let canonical = match name.trim().to_ascii_lowercase().as_str() {
        "layer" | "layers" => "Layer".to_string(),
        "textstyle" | "text_style" | "text_styles" => "TextStyle".to_string(),
        "header" | "headervariables" => "HeaderVariables".to_string(),
        _ => entity_type_names(&registry)
            .into_iter()
            .find(|kind| kind.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| invalid(format!("No record type {name}")))?,
    };
    let container = entity_struct(&registry, &canonical).unwrap_or(&canonical);
    let format = registry
        .get(container)
        .ok_or_else(|| invalid(format!("No record type {name}")))?;
    let schema = input_container(&registry, format, &mut BTreeSet::new());
    schemas.insert(key, schema.clone());
    Ok(schema)
}

fn input_fields(
    registry: &Registry,
    fields: &[Named<Format>],
    active: &mut BTreeSet<String>,
) -> Value {
    let properties: serde_json::Map<String, Value> = fields
        .iter()
        .map(|field| {
            (
                field.name.clone(),
                input_format(registry, &field.value, active),
            )
        })
        .collect();
    json!({"type":"object", "properties":properties, "additionalProperties":false})
}

fn input_tuple(registry: &Registry, fields: &[Format], active: &mut BTreeSet<String>) -> Value {
    let items: Vec<Value> = fields
        .iter()
        .map(|field| input_format(registry, field, active))
        .collect();
    json!({"type":"array", "prefixItems":items, "minItems":fields.len(), "maxItems":fields.len()})
}

fn input_container(
    registry: &Registry,
    container: &ContainerFormat,
    active: &mut BTreeSet<String>,
) -> Value {
    match container {
        ContainerFormat::UnitStruct => json!({"type":"null"}),
        ContainerFormat::NewTypeStruct(field) => input_format(registry, field, active),
        ContainerFormat::TupleStruct(fields) => input_tuple(registry, fields, active),
        ContainerFormat::Struct(fields) => input_fields(registry, fields, active),
        ContainerFormat::Enum(variants) => {
            let choices: Vec<Value> = variants
                .values()
                .map(|variant| {
                    let payload = match &variant.value {
                        VariantFormat::Unit => {
                            return json!({"type":"string", "const":variant.name})
                        }
                        VariantFormat::NewType(field) => input_format(registry, field, active),
                        VariantFormat::Tuple(fields) => input_tuple(registry, fields, active),
                        VariantFormat::Struct(fields) => input_fields(registry, fields, active),
                        VariantFormat::Variable(_) => json!({}),
                    };
                    json!({
                        "type":"object", "properties":{&variant.name:payload},
                        "required":[variant.name], "additionalProperties":false
                    })
                })
                .collect();
            json!({"oneOf":choices})
        }
    }
}

fn input_format(registry: &Registry, format: &Format, active: &mut BTreeSet<String>) -> Value {
    match format {
        Format::Unit => json!({"type":"null"}),
        Format::Bool => json!({"type":"boolean"}),
        Format::I8 => json!({"type":"integer","minimum":i8::MIN,"maximum":i8::MAX}),
        Format::I16 => json!({"type":"integer","minimum":i16::MIN,"maximum":i16::MAX}),
        Format::I32 => json!({"type":"integer","minimum":i32::MIN,"maximum":i32::MAX}),
        Format::I64 => json!({"type":"integer","minimum":i64::MIN,"maximum":i64::MAX}),
        Format::I128 => {
            json!({"type":"integer","minimum":i128::MIN as f64,"maximum":i128::MAX as f64})
        }
        Format::U8 => json!({"type":"integer","minimum":0,"maximum":u8::MAX}),
        Format::U16 => json!({"type":"integer","minimum":0,"maximum":u16::MAX}),
        Format::U32 => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
        Format::U64 => json!({"type":"integer","minimum":0,"maximum":u64::MAX}),
        Format::U128 => json!({"type":"integer","minimum":0,"maximum":u128::MAX as f64}),
        Format::F32 => json!({"type":"number","minimum":f32::MIN,"maximum":f32::MAX}),
        Format::F64 => json!({"type":"number"}),
        Format::Char => json!({"type":"string","minLength":1,"maxLength":1}),
        Format::Str => json!({"type":"string"}),
        Format::Bytes => {
            json!({"type":"array","items":{"type":"integer","minimum":0,"maximum":255}})
        }
        Format::Option(field) => {
            json!({"anyOf":[input_format(registry, field, active), {"type":"null"}]})
        }
        Format::Seq(field) => json!({"type":"array","items":input_format(registry, field, active)}),
        Format::Map { value, .. } => {
            json!({"type":"object","additionalProperties":input_format(registry, value, active)})
        }
        Format::Tuple(fields) => input_tuple(registry, fields, active),
        Format::TupleArray { content, size } => json!({
            "type":"array", "items":input_format(registry, content, active),
            "minItems":size, "maxItems":size
        }),
        Format::TypeName(name) => {
            // A recursive type has no finite inline schema; the next serde read checks it.
            if !active.insert(name.clone()) {
                return json!({});
            }
            let schema = registry
                .get(name)
                .map(|container| input_container(registry, container, active))
                .unwrap_or_else(|| json!({}));
            active.remove(name);
            schema
        }
        Format::Variable(_) => json!({}),
    }
}

/// The entity types: `EntityType`'s variants.
fn entity_type_names(registry: &Registry) -> Vec<String> {
    match registry.get("EntityType") {
        Some(ContainerFormat::Enum(variants)) => variants
            .values()
            .map(|v: &Named<VariantFormat>| v.name.clone())
            .collect(),
        _ => Vec::new(),
    }
}

fn describe(registry: &Registry, type_name: &str, rule: &dyn Fn(&str) -> Rule) -> Value {
    // An entity's record is its variant's struct (`EntityType::Line(Line)`).
    let container = entity_struct(registry, type_name)
        .unwrap_or(type_name)
        .to_string();
    let mut paths = Vec::new();
    let mut types = BTreeMap::new();
    if let Some(format) = registry.get(&container) {
        walk_container(registry, format, "", 0, &mut paths, &mut types, rule);
    }
    let mut out = json!({
        "type": type_name,
        "paths": paths,
        "types": types,
        "notes": [
            "Lengths and coordinates are drawing units; angles (start_angle, rotation…) are radians here, while Qiyas's commands take degrees.",
            "Set a value with set_records {collection, updates: [{key, path, value}]}; paths are JSON pointers into the record's data.",
        ],
    });
    if entity_type_names(registry).iter().any(|n| n == type_name) {
        out["creatable"] = json!(CREATABLE.contains(&type_name));
    }
    out
}

/// The struct an entity variant holds (`Line` for `Line`, `PolygonMeshEntity` for
/// `PolygonMesh`).
fn entity_struct<'a>(registry: &'a Registry, variant: &str) -> Option<&'a str> {
    match registry.get("EntityType")? {
        ContainerFormat::Enum(variants) => variants.values().find_map(|v| {
            (v.name == variant).then_some(())?;
            match &v.value {
                VariantFormat::NewType(f) => match f.as_ref() {
                    Format::TypeName(name) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            }
        }),
        _ => None,
    }
}

fn walk_container(
    registry: &Registry,
    format: &ContainerFormat,
    prefix: &str,
    depth: usize,
    paths: &mut Vec<Value>,
    types: &mut BTreeMap<String, String>,
    rule: &dyn Fn(&str) -> Rule,
) {
    if let ContainerFormat::Struct(fields) = format {
        for field in fields {
            let path = format!("{prefix}/{}", field.name);
            walk_field(registry, &field.value, &path, depth, paths, types, rule);
        }
    }
}

fn walk_field(
    registry: &Registry,
    format: &Format,
    path: &str,
    depth: usize,
    paths: &mut Vec<Value>,
    types: &mut BTreeMap<String, String>,
    rule: &dyn Fn(&str) -> Rule,
) {
    let mut entry = json!({ "path": path, "type": shape_of(registry, format, types) });
    match rule(path) {
        Rule::Write => {}
        Rule::Refuse { why, via } => {
            entry["write"] = json!(false);
            entry["why"] = json!(why);
            if let Some(via) = via {
                entry["via"] = json!(via);
            }
        }
    }
    paths.push(entry);
    // Into named structs (not enums), to a depth.
    if depth + 1 >= SCHEMA_DEPTH {
        return;
    }
    let inner = match format {
        Format::TypeName(name) => Some(name.as_str()),
        Format::Option(f) => match f.as_ref() {
            Format::TypeName(name) => Some(name.as_str()),
            _ => None,
        },
        _ => None,
    };
    if let Some(ContainerFormat::Struct(_)) = inner.and_then(|n| registry.get(n)) {
        let c = registry.get(inner.expect("checked")).expect("checked");
        walk_container(registry, c, path, depth + 1, paths, types, rule);
    }
}

/// A short type for a format: "number", "string", "[Vector3]", "Color"; named types other than
/// plain structs go into `types` with their shape.
fn shape_of(registry: &Registry, format: &Format, types: &mut BTreeMap<String, String>) -> String {
    match format {
        Format::Unit => "null".into(),
        Format::Bool => "bool".into(),
        Format::I8 | Format::I16 | Format::I32 | Format::I64 | Format::I128 => "integer".into(),
        Format::U8 | Format::U16 | Format::U32 | Format::U64 | Format::U128 => {
            "integer (0 or more)".into()
        }
        Format::F32 | Format::F64 => "number".into(),
        Format::Char | Format::Str => "string".into(),
        Format::Bytes => "bytes".into(),
        Format::Option(f) => format!("{} | null", shape_of(registry, f, types)),
        Format::Seq(f) => format!("[{}]", shape_of(registry, f, types)),
        Format::Map { key, value } => format!(
            "{{{}: {}}}",
            shape_of(registry, key, types),
            shape_of(registry, value, types)
        ),
        Format::Tuple(fs) => format!(
            "[{}]",
            fs.iter()
                .map(|f| shape_of(registry, f, types))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Format::TupleArray { content, size } => {
            format!("[{}; {size}]", shape_of(registry, content, types))
        }
        Format::TypeName(name) => {
            if !types.contains_key(name) {
                // Mark first: types can refer to themselves.
                types.insert(name.clone(), String::new());
                let shape = registry
                    .get(name)
                    .map(|c| container_shape(registry, c, types))
                    .unwrap_or_default();
                types.insert(name.clone(), shape);
            }
            name.clone()
        }
        Format::Variable(_) => "any".into(),
    }
}

fn container_shape(
    registry: &Registry,
    c: &ContainerFormat,
    types: &mut BTreeMap<String, String>,
) -> String {
    match c {
        ContainerFormat::UnitStruct => "null".into(),
        ContainerFormat::NewTypeStruct(f) => shape_of(registry, f, types),
        ContainerFormat::TupleStruct(fs) => format!(
            "[{}]",
            fs.iter()
                .map(|f| shape_of(registry, f, types))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ContainerFormat::Struct(fields) => format!(
            "{{{}}}",
            fields
                .iter()
                .map(|f| format!("{}: {}", f.name, shape_of(registry, &f.value, types)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ContainerFormat::Enum(variants) => variants
            .values()
            .map(|v| match &v.value {
                VariantFormat::Unit => format!("\"{}\"", v.name),
                VariantFormat::NewType(f) => {
                    format!("{{\"{}\": {}}}", v.name, shape_of(registry, f, types))
                }
                VariantFormat::Tuple(fs) => format!(
                    "{{\"{}\": [{}]}}",
                    v.name,
                    fs.iter()
                        .map(|f| shape_of(registry, f, types))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                VariantFormat::Struct(fields) => format!(
                    "{{\"{}\": {{{}}}}}",
                    v.name,
                    fields
                        .iter()
                        .map(|f| format!("{}: {}", f.name, shape_of(registry, &f.value, types)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                VariantFormat::Variable(_) => v.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}
