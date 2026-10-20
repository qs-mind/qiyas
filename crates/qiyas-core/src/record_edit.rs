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
use std::sync::{Arc, OnceLock};

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
        if entities.is_empty() {
            return Err(invalid("Give at least one entity"));
        }
        let block = self.sheet_block(sheet.as_deref());
        let owner = self
            .drawing
            .doc
            .block_records
            .get(&block)
            .map(|b| b.handle)
            .ok_or_else(|| CoreError::NoSuchSheet(block.clone()))?;
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
