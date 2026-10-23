//! Element definitions are drawing data. This module only understands geometry, properties
//! and expressions; trade names and measurement conventions never select an algorithm.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use qiyas_param::expr::{Expr, Resolver, Value as ExprValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::{CoreError, DrawSpec, Session, Target};

pub const OBJECT_APP: &str = "QIYAS_OBJECT";
pub const PART_APP: &str = "QIYAS_OBJECT_PART";
pub(crate) const SCHEDULE_APP: &str = "QIYAS_OBJECT_SCHEDULE";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Property {
    Number {
        unit: UnitKind,
        default: f64,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
    },
    Text {
        default: String,
    },
    Choice {
        default: String,
        choices: Vec<String>,
    },
    Bool {
        default: bool,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Scalar,
    Length,
    Area,
    Volume,
    Angle,
    Count,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeometryKind {
    Point,
    Path,
    Region,
    Band,
    Hosted,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Vertical {
    pub level: Option<String>,
    pub base: Option<String>,
    pub height: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PlanRecipe {
    pub outline: bool,
    /// Band width expression, in metres. Converted to drawing units before offsetting.
    pub band_faces: Option<String>,
    /// Text expression selecting centre, left or right of the driver path.
    pub band_justification: Option<String>,
    /// Reuse the original path as the centreline instead of drawing a second path.
    pub centreline_driver: bool,
    /// Optional layer per generated role; instance overrides are applied last.
    pub role_layers: BTreeMap<String, String>,
    pub hatch: bool,
    pub centreline: bool,
    /// DrawSpec JSON in local drawing coordinates. Strings beginning '=' are expressions.
    pub symbols: Vec<Value>,
    /// Store local symbol entities in an INSERT driver's block.
    pub symbols_in_driver: bool,
    pub label_template: Option<String>,
    /// Text height expression in metres; omitted uses the drawing's annotation scale.
    pub label_height: Option<String>,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JoinRule {
    #[default]
    Separate,
    Union,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HostCut {
    pub width: String,
    pub at: String,
    #[serde(default)]
    pub offset_along: Option<String>,
    #[serde(default)]
    pub offset_normal: Option<String>,
    #[serde(default)]
    pub mirror: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObjectType {
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub properties: BTreeMap<String, Property>,
    /// Read-only property expressions, including values supplied by a host's geometry.
    #[serde(default)]
    pub property_bindings: BTreeMap<String, String>,
    pub geometry: GeometryKind,
    #[serde(default)]
    pub vertical: Vertical,
    #[serde(default)]
    pub plan: PlanRecipe,
    #[serde(default)]
    pub quantities: BTreeMap<String, String>,
    #[serde(default)]
    pub join: JoinRule,
    #[serde(default)]
    pub host_cut: Option<HostCut>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObjectPlacement {
    #[serde(rename = "type")]
    pub type_name: String,
    /// Existing driver, or points for a new driver. Exactly one is required.
    #[serde(default)]
    pub driver: Option<u64>,
    #[serde(default)]
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub region: Option<crate::object_regions::RegionSource>,
    #[serde(default)]
    pub props: BTreeMap<String, Value>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub host: Option<u64>,
    /// Explicit assembly identity permits connected members with different vertical spans.
    #[serde(default)]
    pub assembly: Option<String>,
    #[serde(default)]
    pub layers: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInstance {
    pub id: u64,
    #[serde(rename = "type")]
    pub type_name: String,
    pub props: BTreeMap<String, Value>,
    pub level: Option<String>,
    #[serde(default)]
    pub host: Option<u64>,
    /// Explicit assembly identity permits connected members with different vertical spans.
    #[serde(default)]
    pub assembly: Option<String>,
    #[serde(default)]
    pub layers: BTreeMap<String, String>,
    /// Import marker in transitional files; conversion clears it before shared derivation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ObjectPart {
    pub owner: u64,
    pub index: usize,
    #[serde(default)]
    pub role: String,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ObjectFilter {
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub category: Option<String>,
    pub level: Option<String>,
    pub sheet: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ObjectSchedule {
    pub at: [f64; 2],
    #[serde(default)]
    pub filter: ObjectFilter,
    /// Property, quantity or geometry fact names; id/type/level/category are also available.
    pub columns: Vec<String>,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub headers: Vec<String>,
    #[serde(default)]
    pub templates: BTreeMap<String, String>,
    #[serde(default)]
    pub decimals: BTreeMap<String, usize>,
    #[serde(default)]
    pub ids: Option<Vec<u64>>,
    #[serde(default)]
    pub totals: BTreeMap<String, String>,
    #[serde(default)]
    pub default_layer: Option<String>,
    #[serde(default)]
    pub preserve_overrides: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct ObjectRecord {
    /// Stable across carried drawing revisions; id remains the local driver handle.
    pub identity: String,
    #[serde(flatten)]
    pub instance: ObjectInstance,
    pub category: String,
    pub sheet: String,
    pub facts: BTreeMap<String, f64>,
    pub quantities: BTreeMap<String, f64>,
    pub valid: bool,
}
fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}
fn expression(text: &str) -> Result<Expr, CoreError> {
    Expr::parse(text).map_err(|e| invalid(e.to_string()))
}
fn typed(value: &Value) -> Option<ExprValue> {
    match value {
        Value::Number(n) => n.as_f64().map(ExprValue::Number),
        Value::String(s) => Some(ExprValue::Text(s.clone())),
        Value::Bool(b) => Some(ExprValue::Bool(*b)),
        _ => None,
    }
}
fn expr_json(value: &ExprValue) -> Value {
    match value {
        ExprValue::Number(n) => json!(n),
        ExprValue::Text(s) => json!(s),
        ExprValue::Bool(b) => json!(b),
    }
}
pub(crate) fn instance(entity: &EntityType) -> Option<ObjectInstance> {
    crate::xdata::read_json(entity.common(), OBJECT_APP)
}
impl Property {
    fn default_value(&self) -> Value {
        match self {
            Self::Number { default, .. } => json!(default),
            Self::Text { default } | Self::Choice { default, .. } => json!(default),
            Self::Bool { default } => json!(default),
        }
    }
    fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Number { min, max, .. } => value.as_f64().is_some_and(|n| {
                n.is_finite() && min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m)
            }),
            Self::Text { .. } => value.is_string(),
            Self::Choice { choices, .. } => value
                .as_str()
                .is_some_and(|s| choices.iter().any(|c| c == s)),
            Self::Bool { .. } => value.is_boolean(),
        }
    }
}
impl ObjectType {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.name.trim().is_empty() || self.category.trim().is_empty() {
            return Err(invalid("Object type name and category are required"));
        }
        for (name, prop) in &self.properties {
            if !qiyas_param::expr::is_name(name)
                || name.starts_with("host.")
                || (reserved(name) && !matches!(name.as_str(), "height" | "base"))
            {
                return Err(invalid(format!(
                    "Invalid or reserved object property {name}"
                )));
            }
            if !prop.accepts(&prop.default_value()) {
                return Err(invalid(format!("Invalid default or limits for {name}")));
            }
        }
        for (name, text) in &self.property_bindings {
            if !self.properties.contains_key(name) {
                return Err(invalid(format!(
                    "Property binding names undeclared property {name}"
                )));
            }
            for referenced in expression(text)?.names() {
                if !self.properties.contains_key(&referenced) && !reserved(&referenced) {
                    return Err(invalid(format!(
                        "Unknown property binding name {referenced}"
                    )));
                }
            }
        }
        let mut pending: BTreeMap<_, _> = self.property_bindings.iter().collect();
        while !pending.is_empty() {
            let ready: Vec<_> = pending
                .iter()
                .filter_map(|(name, text)| {
                    expression(text)
                        .ok()
                        .filter(|e| e.names().iter().all(|n| !pending.contains_key(n)))
                        .map(|_| (*name).clone())
                })
                .collect();
            if ready.is_empty() {
                return Err(invalid("Circular property bindings"));
            }
            for name in ready {
                pending.remove(&name);
            }
        }
        for (name, text) in &self.quantities {
            if !qiyas_param::expr::is_name(name)
                || name.starts_with("host.")
                || self.properties.contains_key(name)
            {
                return Err(invalid(format!(
                    "Invalid or ambiguous quantity name {name}"
                )));
            }
            expression(text)?;
        }
        for text in [
            &self.vertical.base,
            &self.vertical.height,
            &self.plan.band_faces,
            &self.plan.band_justification,
            &self.plan.label_height,
        ]
        .into_iter()
        .flatten()
        {
            expression(text)?;
        }
        if matches!(self.join, JoinRule::Union) && self.plan.band_faces.is_none() {
            return Err(invalid("Union joins require band_faces"));
        }
        if self.geometry == GeometryKind::Band && self.plan.band_faces.is_none() {
            return Err(invalid("A band requires a band_faces width expression"));
        }
        if self.geometry == GeometryKind::Hosted && self.host_cut.is_none() {
            return Err(invalid(
                "Hosted geometry requires host_cut to define its station on the host",
            ));
        }
        if let Some(cut) = &self.host_cut {
            for text in [&cut.width, &cut.at].into_iter().chain(
                [&cut.offset_along, &cut.offset_normal, &cut.mirror]
                    .into_iter()
                    .flatten(),
            ) {
                expression(text)?;
            }
            if self.geometry != GeometryKind::Hosted {
                return Err(invalid("A host cut requires hosted geometry"));
            }
        }
        // Cycles and missing names are rejected even before any instances exist.
        let known: BTreeSet<_> = self
            .properties
            .keys()
            .cloned()
            .chain(self.quantities.keys().cloned())
            .collect();
        for text in self.quantities.values().chain(
            [
                &self.vertical.base,
                &self.vertical.height,
                &self.plan.band_faces,
                &self.plan.band_justification,
                &self.plan.label_height,
            ]
            .into_iter()
            .flatten(),
        ) {
            for name in expression(text)?.names() {
                if !known.contains(&name) && !reserved(&name) {
                    return Err(invalid(format!("Unknown object expression name {name}")));
                }
            }
        }
        if let Some(cut) = &self.host_cut {
            for text in [&cut.width, &cut.at].into_iter().chain(
                [&cut.offset_along, &cut.offset_normal, &cut.mirror]
                    .into_iter()
                    .flatten(),
            ) {
                for name in expression(text)?.names() {
                    if !self.properties.contains_key(&name) && !reserved(&name) {
                        return Err(invalid(format!("Unknown host-cut expression name {name}")));
                    }
                }
            }
        }
        for text in [
            &self.vertical.base,
            &self.vertical.height,
            &self.plan.band_faces,
            &self.plan.band_justification,
        ]
        .into_iter()
        .flatten()
        {
            for name in expression(text)?.names() {
                if self.quantities.contains_key(&name) {
                    return Err(invalid(format!(
                        "Geometry expressions cannot depend on quantity {name}"
                    )));
                }
            }
        }
        if self.plan.symbols.len() > 10000 {
            return Err(invalid("Too many symbol entities"));
        }
        for symbol in &self.plan.symbols {
            let mut checked = symbol.clone();
            validate_symbol(&mut checked, &known)?;
            serde_json::from_value::<DrawSpec>(checked)
                .map_err(|e| invalid(format!("Invalid symbol recipe: {e}")))?;
        }
        let mut done = BTreeSet::new();
        for name in self.quantities.keys() {
            self.check_cycle(name, &mut BTreeSet::new(), &mut done)?;
        }
        Ok(())
    }
    fn check_cycle(
        &self,
        name: &str,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), CoreError> {
        if done.contains(name) {
            return Ok(());
        }
        if !active.insert(name.into()) {
            return Err(invalid(format!("Circular object quantity {name}")));
        }
        for used in expression(&self.quantities[name])?.names() {
            if self.quantities.contains_key(&used) {
                self.check_cycle(&used, active, done)?;
            }
        }
        active.remove(name);
        done.insert(name.into());
        Ok(())
    }
    pub(crate) fn properties(
        &self,
        overrides: &BTreeMap<String, Value>,
    ) -> Result<BTreeMap<String, Value>, CoreError> {
        let mut result: BTreeMap<_, _> = self
            .properties
            .iter()
            .map(|(k, p)| (k.clone(), p.default_value()))
            .collect();
        for (name, value) in overrides {
            let p = self
                .properties
                .get(name)
                .ok_or_else(|| invalid(format!("Unknown property {name} on {}", self.name)))?;
            if !p.accepts(value) {
                return Err(invalid(format!(
                    "Invalid value or limits for property {name}"
                )));
            }
            result.insert(name.clone(), value.clone());
        }
        Ok(result)
    }
}
fn host_fact_available(name: &str) -> bool {
    let fact = name.strip_prefix("geometry.").unwrap_or(name);
    qiyas_param::expr::is_name(name)
        && !matches!(
            fact,
            "area" | "gross_area" | "net_area" | "volume" | "gross_volume"
        )
}
fn reserved(name: &str) -> bool {
    matches!(
        name,
        "length"
            | "area"
            | "perimeter"
            | "count"
            | "base"
            | "height"
            | "level_elevation"
            | "unit"
            | "x"
            | "y"
            | "gross_area"
            | "net_area"
            | "volume"
            | "gross_volume"
            | "rotation"
    ) || name.strip_prefix("host.").is_some_and(host_fact_available)
        || name.strip_prefix("geometry.").is_some_and(|n| {
            matches!(
                n,
                "length"
                    | "area"
                    | "perimeter"
                    | "count"
                    | "base"
                    | "height"
                    | "level_elevation"
                    | "unit"
                    | "x"
                    | "y"
                    | "gross_area"
                    | "net_area"
                    | "volume"
                    | "gross_volume"
                    | "rotation"
            )
        })
}
struct Context<'a> {
    session: &'a Session,
    values: BTreeMap<String, ExprValue>,
}
impl Resolver for Context<'_> {
    fn resolve(&self, name: &str) -> Option<ExprValue> {
        self.values.get(name).cloned()
    }
    fn lookup(&self, table: &str, key: &ExprValue, column: &str) -> Option<ExprValue> {
        let entry = knowledge::get(&self.session.drawing.doc, Kind::Tables, table).ok()??;
        let key = match key {
            ExprValue::Text(s) => s.clone(),
            ExprValue::Number(n) => n.to_string(),
            ExprValue::Bool(b) => b.to_string(),
        };
        typed(entry.data.get(&key)?.get(column)?)
    }
}
impl Context<'_> {
    fn number(&self, text: &str) -> Result<f64, CoreError> {
        expression(text)?
            .eval_with(self)
            .and_then(|v| v.as_number())
            .map_err(|e| invalid(format!("{text}: {e}")))
    }
}
impl Session {
    pub fn object_types(&self) -> Result<Vec<ObjectType>, CoreError> {
        knowledge::list(&self.drawing.doc, Kind::Types)?
            .into_iter()
            .map(|e| {
                serde_json::from_value(e.data)
                    .map_err(|e| invalid(format!("Invalid object type: {e}")))
            })
            .collect()
    }
    pub(crate) fn object_type(&self, name: &str) -> Result<ObjectType, CoreError> {
        let entry = knowledge::get(&self.drawing.doc, Kind::Types, name)?
            .ok_or_else(|| invalid(format!("There is no object type {name}")))?;
        serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string()))
    }
    pub(crate) fn define_object_type(
        &mut self,
        definition: ObjectType,
        redefine: bool,
    ) -> Result<(), CoreError> {
        definition.validate()?;
        let exists = knowledge::get(&self.drawing.doc, Kind::Types, &definition.name)?.is_some();
        if exists != redefine {
            return Err(invalid(if exists {
                "This type exists; use redefine_object_type"
            } else {
                "This type does not exist; use define_object_type"
            }));
        }
        knowledge::put(
            &mut self.drawing.doc,
            Kind::Types,
            &definition.name,
            &json!(definition),
        )
    }
    pub(crate) fn delete_object_type(&mut self, name: &str) -> Result<(), CoreError> {
        if self
            .recognition_rules()?
            .iter()
            .any(|rule| rule.object_type.eq_ignore_ascii_case(name))
        {
            return Err(invalid(
                "Cannot delete a type referenced by a recognition rule",
            ));
        }
        if self
            .drawing
            .doc
            .entities()
            .filter_map(instance)
            .any(|i| i.type_name.eq_ignore_ascii_case(name))
        {
            return Err(invalid("Cannot delete an object type with instances"));
        }
        if !knowledge::remove(&mut self.drawing.doc, Kind::Types, name)? {
            return Err(invalid("Object type does not exist"));
        }
        Ok(())
    }
    pub(crate) fn object_app(&mut self, name: &str) -> Result<u64, CoreError> {
        if !self.drawing.doc.app_ids.contains(name) {
            let mut app = opencadcodec::AppId::new(name);
            app.handle = self.drawing.doc.allocate_handle();
            self.drawing.doc.app_ids.add(app).map_err(invalid)?;
        }
        Ok(self
            .drawing
            .doc
            .app_ids
            .get(name)
            .expect("registered")
            .handle
            .value())
    }
    pub(crate) fn stamp_object(
        &mut self,
        h: u64,
        object: &ObjectInstance,
    ) -> Result<(), CoreError> {
        let app = self.object_app(OBJECT_APP)?;
        let e = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(h))
            .ok_or_else(|| invalid("Object driver is missing"))?;
        crate::xdata::write_json(e.common_mut(), OBJECT_APP, app, object)
    }
    pub(crate) fn place_objects(
        &mut self,
        placements: Vec<ObjectPlacement>,
    ) -> Result<Vec<u64>, CoreError> {
        if placements.is_empty() {
            return Err(invalid("Place at least one object"));
        }
        let mut out = Vec::new();
        for p in placements {
            let definition = self.object_type(&p.type_name)?;
            let props = definition.properties(&p.props)?;
            if p.region.is_some() && definition.geometry != GeometryKind::Region {
                return Err(invalid("A retained region requires region geometry"));
            }
            if p.driver.is_some() == !p.points.is_empty() {
                return Err(invalid("Give driver or points, not both"));
            }
            let h = if let Some(h) = p.driver {
                self.sheet_targets(&[Target {
                    handle: h,
                    chain: vec![],
                }])?;
                if self
                    .drawing
                    .doc
                    .get_entity(Handle::new(h))
                    .and_then(instance)
                    .is_some()
                {
                    return Err(invalid("Driver already has an object"));
                }
                h
            } else {
                let spec = if matches!(
                    definition.geometry,
                    GeometryKind::Point | GeometryKind::Hosted
                ) {
                    if p.points.len() != 1 {
                        return Err(invalid("Point and hosted objects require one point"));
                    }
                    json!({"kind":"point", "p":p.points[0]})
                } else {
                    json!({"kind":"polyline", "points":p.points, "closed":definition.geometry == GeometryKind::Region})
                };
                let spec: DrawSpec =
                    serde_json::from_value(spec).map_err(|e| invalid(e.to_string()))?;
                self.draw(spec)?[0]
            };
            let object = ObjectInstance {
                id: h,
                type_name: definition.name,
                props,
                level: p.level.or(definition.vertical.level),
                host: p.host,
                assembly: p.assembly,
                layers: p.layers,
                compatibility: None,
            };
            self.stamp_object(h, &object)?;
            if let Some(source) = p.region {
                self.bind_object_region(h, source)?;
            }
            out.push(h);
        }
        Ok(out)
    }
    pub(crate) fn set_object_props(
        &mut self,
        targets: &[u64],
        props: &BTreeMap<String, Value>,
        level: Option<String>,
    ) -> Result<(), CoreError> {
        if targets.is_empty() {
            return Err(invalid("Choose at least one object"));
        }
        self.sheet_targets(
            &targets
                .iter()
                .map(|&handle| Target {
                    handle,
                    chain: vec![],
                })
                .collect::<Vec<_>>(),
        )?;
        for &h in targets {
            let mut object = self
                .drawing
                .doc
                .get_entity(Handle::new(h))
                .and_then(instance)
                .ok_or_else(|| invalid("Choose an object driver"))?;
            let definition = self.object_type(&object.type_name)?;
            if let Some(name) = props
                .keys()
                .find(|name| definition.property_bindings.contains_key(*name))
            {
                return Err(invalid(format!(
                    "Property {name} is bound to an expression; edit its source"
                )));
            }
            object.props.extend(props.clone());
            object.props = self
                .object_type(&object.type_name)?
                .properties(&object.props)?;
            if level.is_some() {
                object.level = level.clone();
            }
            self.stamp_object(h, &object)?;
        }
        Ok(())
    }
    fn object_context<'a>(
        &'a self,
        object: &ObjectInstance,
        definition: &ObjectType,
    ) -> Result<Context<'a>, CoreError> {
        self.object_context_depth(object, definition, 0)
    }
    fn object_context_depth<'a>(
        &'a self,
        object: &ObjectInstance,
        definition: &ObjectType,
        depth: usize,
    ) -> Result<Context<'a>, CoreError> {
        if depth > 16 {
            return Err(invalid("Object host chain is cyclic or too deep"));
        }

        let driver = self
            .drawing
            .doc
            .get_entity(Handle::new(object.id))
            .ok_or_else(|| invalid("Missing object driver"))?;
        let paths = qiyas_geom::flatten(
            &self.drawing.doc,
            driver,
            &qiyas_geom::Xform::identity(),
            self.measure_tol(),
        )
        .into_paths();
        let factor = self.drawing.units.to_metres();
        let mut length = 0.0;
        let mut area = 0.0;
        for path in &paths {
            length += path.length() * factor;
            if path.closed {
                area += path.area() * factor * factor;
            }
        }
        if let Some(region_area) = crate::object_regions::retained_region_area(driver) {
            area = region_area * factor * factor;
        }
        let pos = driver_position(driver);
        let mut values: BTreeMap<String, ExprValue> = definition
            .properties(&object.props)?
            .iter()
            .filter_map(|(k, v)| typed(v).map(|v| (k.clone(), v)))
            .collect();
        for (k, v) in [
            ("length", length),
            ("perimeter", length),
            ("area", area),
            ("gross_area", area),
            ("net_area", area),
            ("count", 1.0),
            ("unit", factor),
            ("x", pos[0] * factor),
            ("y", pos[1] * factor),
            ("rotation", driver_rotation(driver).to_degrees()),
        ] {
            values.insert(k.into(), ExprValue::Number(v));
        }
        let elevation = match &object.level {
            Some(level) => knowledge::get(&self.drawing.doc, Kind::Levels, level)?
                .ok_or_else(|| invalid(format!("Unknown level {level}")))?
                .data
                .get("elevation")
                .and_then(Value::as_f64)
                .ok_or_else(|| invalid("Level requires a numeric elevation in metres"))?,
            None => 0.0,
        };
        values.insert("level_elevation".into(), ExprValue::Number(elevation));
        for (k, v) in values.clone() {
            if reserved(&k) {
                values.insert(format!("geometry.{k}"), v);
            }
        }
        if let Some(host) = object.host {
            let host = self
                .drawing
                .doc
                .get_entity(Handle::new(host))
                .and_then(instance)
                .ok_or_else(|| invalid("Missing typed host"))?;
            let host_definition = self.object_type(&host.type_name)?;
            let host_context = self.object_context_depth(&host, &host_definition, depth + 1)?;
            for (name, value) in &host_context.values {
                if host_fact_available(name) {
                    values.insert(format!("host.{name}"), value.clone());
                }
            }
            if let Some(width) = &host_definition.plan.band_faces {
                values.insert(
                    "host.band_width".into(),
                    ExprValue::Number(host_context.number(width)?),
                );
            }
        }
        let mut context = Context {
            session: self,
            values,
        };
        let mut pending = definition.property_bindings.clone();
        while !pending.is_empty() {
            let ready: Vec<_> = pending
                .iter()
                .filter_map(|(name, text)| {
                    expression(text)
                        .ok()
                        .filter(|e| e.names().iter().all(|n| !pending.contains_key(n)))
                        .map(|_| name.clone())
                })
                .collect();
            if ready.is_empty() {
                return Err(invalid("Circular property bindings"));
            }
            for name in ready {
                let text = pending.remove(&name).expect("listed binding");
                let value = expression(&text)?
                    .eval_with(&context)
                    .map_err(|e| invalid(e.to_string()))?;
                if !definition.properties[&name].accepts(&expr_json(&value)) {
                    return Err(invalid(format!("Invalid bound property {name}")));
                }
                context.values.insert(name, value);
            }
        }
        for (key, expr) in [
            ("base", &definition.vertical.base),
            ("height", &definition.vertical.height),
        ] {
            let n = expr
                .as_deref()
                .map(|e| context.number(e))
                .transpose()?
                .unwrap_or_else(|| {
                    context
                        .values
                        .get(key)
                        .and_then(|v| v.as_number().ok())
                        .unwrap_or(0.0)
                });
            context.values.insert(key.into(), ExprValue::Number(n));
            context
                .values
                .insert(format!("geometry.{key}"), ExprValue::Number(n));
        }
        Ok(context)
    }
    /// Evaluate a local symbol recipe before its insert exists. Local point facts are zero;
    /// physical dimensions and other inputs come from the type's validated properties.
    pub(crate) fn symbol_recipe_for_properties(
        &self,
        definition: &ObjectType,
        props: &BTreeMap<String, Value>,
    ) -> Result<Vec<DrawSpec>, CoreError> {
        let mut values: BTreeMap<_, _> = definition
            .properties(props)?
            .iter()
            .filter_map(|(name, value)| typed(value).map(|value| (name.clone(), value)))
            .collect();
        for name in [
            "length",
            "perimeter",
            "area",
            "gross_area",
            "net_area",
            "volume",
            "x",
            "y",
            "rotation",
            "level_elevation",
        ] {
            values.insert(name.into(), ExprValue::Number(0.0));
            values.insert(format!("geometry.{name}"), ExprValue::Number(0.0));
        }
        values.insert("count".into(), ExprValue::Number(1.0));
        values.insert("geometry.count".into(), ExprValue::Number(1.0));
        values.insert(
            "unit".into(),
            ExprValue::Number(self.drawing.units.to_metres()),
        );
        values.insert(
            "geometry.unit".into(),
            ExprValue::Number(self.drawing.units.to_metres()),
        );
        let mut context = Context {
            session: self,
            values,
        };
        for (name, expression) in [
            ("base", &definition.vertical.base),
            ("height", &definition.vertical.height),
        ] {
            let value = expression
                .as_deref()
                .map(|text| context.number(text))
                .transpose()?
                .unwrap_or(0.0);
            context.values.insert(name.into(), ExprValue::Number(value));
            context
                .values
                .insert(format!("geometry.{name}"), ExprValue::Number(value));
        }
        self.quantity_values(definition, &mut context)?;
        evaluated_symbol_specs(&definition.plan.symbols, &context)
    }

    fn quantity_values(
        &self,
        definition: &ObjectType,
        context: &mut Context<'_>,
    ) -> Result<BTreeMap<String, f64>, CoreError> {
        let mut pending = definition.quantities.clone();
        let mut result = BTreeMap::new();
        while !pending.is_empty() {
            let ready: Vec<_> = pending
                .iter()
                .filter_map(|(name, text)| {
                    let e = expression(text).ok()?;
                    e.names()
                        .iter()
                        .all(|n| !pending.contains_key(n))
                        .then_some(name.clone())
                })
                .collect();
            if ready.is_empty() {
                return Err(invalid("Circular or invalid object quantities"));
            }
            for name in ready {
                let text = pending.remove(&name).expect("pending");
                let n = context.number(&text)?;
                context.values.insert(name.clone(), ExprValue::Number(n));
                result.insert(name, n);
            }
        }
        Ok(result)
    }
    pub fn objects(&self, filter: &ObjectFilter) -> Result<Vec<ObjectRecord>, CoreError> {
        self.validate_object_metadata()?;
        let mut out = Vec::new();
        for entity in self.drawing.doc.entities() {
            let Some(mut object) = instance(entity) else {
                continue;
            };
            let definition = self.object_type(&object.type_name)?;
            let sheet = self
                .drawing
                .doc
                .block_records
                .iter()
                .find(|b| b.handle == entity.common().owner_handle)
                .map(|b| b.name.clone())
                .unwrap_or_default();
            if filter
                .type_name
                .as_ref()
                .is_some_and(|n| !n.eq_ignore_ascii_case(&definition.name))
                || filter
                    .category
                    .as_ref()
                    .is_some_and(|n| !n.eq_ignore_ascii_case(&definition.category))
                || filter.level.as_ref().is_some_and(|n| {
                    object
                        .level
                        .as_ref()
                        .is_none_or(|level| !level.eq_ignore_ascii_case(n))
                })
                || filter
                    .sheet
                    .as_ref()
                    .is_some_and(|n| !n.eq_ignore_ascii_case(&sheet))
            {
                continue;
            }
            let mut context = self.object_context(&object, &definition)?;
            self.band_facts(&object, &definition, &mut context)?;
            for name in definition.property_bindings.keys() {
                if let Some(value) = context.values.get(name) {
                    object.props.insert(name.clone(), expr_json(value));
                }
            }
            let facts = context
                .values
                .iter()
                .filter_map(|(k, v)| {
                    k.strip_prefix("geometry.")
                        .and_then(|name| v.as_number().ok().map(|v| (name.to_owned(), v)))
                })
                .collect();
            let valid = crate::object_regions::retained_region(entity).is_none_or(|r| r.valid);
            let quantities = if valid {
                self.quantity_values(&definition, &mut context)?
            } else {
                BTreeMap::new()
            };
            out.push(ObjectRecord {
                identity: self.revision_identity(object.id),
                instance: object,
                category: definition.category,
                sheet,
                facts: if valid { facts } else { BTreeMap::new() },
                quantities,
                valid,
            });
        }
        out.sort_by_key(|o| o.instance.id);
        Ok(out)
    }
}
fn driver_position(entity: &EntityType) -> [f64; 2] {
    match entity {
        EntityType::Point(p) => [p.location.x, p.location.y],
        EntityType::Insert(i) => [i.insert_point.x, i.insert_point.y],
        EntityType::LwPolyline(p) => p
            .vertices
            .first()
            .map(|v| [v.location.x, v.location.y])
            .unwrap_or([0.0; 2]),
        EntityType::Line(l) => [l.start.x, l.start.y],
        _ => [0.0; 2],
    }
}

/// The driver's local X axis. Expressions use degrees; CAD transforms keep radians.
fn driver_rotation(entity: &EntityType) -> f64 {
    match entity {
        EntityType::Point(p) => p.x_axis_angle,
        EntityType::Insert(i) => i.rotation,
        EntityType::Line(l) => (l.end.y - l.start.y).atan2(l.end.x - l.start.x),
        EntityType::LwPolyline(p) => p
            .vertices
            .windows(2)
            .find_map(|pair| {
                let a = &pair[0];
                let b = &pair[1];
                let dx = b.location.x - a.location.x;
                let dy = b.location.y - a.location.y;
                (dx != 0.0 || dy != 0.0).then(|| dy.atan2(dx) - 2.0 * a.bulge.atan())
            })
            .unwrap_or(0.0),
        _ => 0.0,
    }
    .rem_euclid(std::f64::consts::TAU)
}

impl Session {
    /// A union is local to a type, sheet, level and vertical span. Driver order also fixes
    /// which object owns the graphics and receives the overlap deduction in its quantities.
    pub(crate) fn band_members(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
    ) -> Result<Vec<ObjectInstance>, CoreError> {
        if !matches!(definition.join, JoinRule::Union) {
            return Ok(vec![object.clone()]);
        }
        let driver = self
            .drawing
            .doc
            .get_entity(Handle::new(object.id))
            .ok_or_else(|| invalid("Missing driver"))?;
        let owner = driver.common().owner_handle;
        let context = self.object_context(object, definition)?;
        let span = [context.number("base")?, context.number("height")?];
        let mut members = Vec::new();
        for entity in self.drawing.doc.entities() {
            if entity.common().owner_handle != owner {
                continue;
            }
            let Some(mut member) = instance(entity) else {
                continue;
            };
            if !member.type_name.eq_ignore_ascii_case(&definition.name)
                || member.compatibility.is_some()
                || member.assembly != object.assembly
            {
                continue;
            }
            let same_level = match (&object.level, &member.level) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            };
            if !same_level {
                continue;
            }
            // A copy's xdata still carries the old id until the refresh reaches that driver.
            member.id = entity.common().handle.value();
            let context = self.object_context(&member, definition)?;
            if object.assembly.is_some()
                || [context.number("base")?, context.number("height")?] == span
            {
                members.push(member);
            }
        }
        members.sort_by_key(|member| member.id);
        Ok(members)
    }
    fn band_member(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
    ) -> Result<crate::object_bands::BandMember, CoreError> {
        let context = self.object_context(object, definition)?;
        let factor = self.drawing.units.to_metres();
        let width = context.number(
            definition
                .plan
                .band_faces
                .as_deref()
                .ok_or_else(|| invalid("A band requires a width expression"))?,
        )?;
        let justification = if let Some(text) = &definition.plan.band_justification {
            let value = expression(text)?
                .eval_with(&context)
                .map_err(|e| invalid(e.to_string()))?;
            match value
                .as_text()
                .map_err(|e| invalid(e.to_string()))?
                .to_ascii_lowercase()
                .as_str()
            {
                "centre" | "center" => qiyas_geom::walls::Justification::Centre,
                "left" => qiyas_geom::walls::Justification::Left,
                "right" => qiyas_geom::walls::Justification::Right,
                _ => return Err(invalid("Band justification must be centre, left or right")),
            }
        } else {
            qiyas_geom::walls::Justification::Centre
        };
        let entity = self
            .drawing
            .doc
            .get_entity(Handle::new(object.id))
            .ok_or_else(|| invalid("Missing band driver"))?;
        let paths = qiyas_geom::flatten(
            &self.drawing.doc,
            entity,
            &qiyas_geom::Xform::identity(),
            self.measure_tol(),
        )
        .into_paths();
        if paths.len() != 1 || width <= 0.0 || !factor.is_finite() {
            return Err(invalid("A band requires one path and a positive width"));
        }
        Ok(crate::object_bands::BandMember {
            id: object.id,
            points: paths[0].points.clone(),
            closed: paths[0].closed,
            width_m: width,
            base_m: context.number("base")? + context.number("level_elevation")?,
            height_m: context.number("height")?,
            justification,
        })
    }
    fn band_cut(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
    ) -> Result<crate::object_bands::BandCut, CoreError> {
        let cut = definition
            .host_cut
            .as_ref()
            .ok_or_else(|| invalid("Missing host cut"))?;
        let context = self.object_context(object, definition)?;
        Ok(crate::object_bands::BandCut {
            host: object.host.ok_or_else(|| invalid("Missing band host"))?,
            at_m: context.number(&cut.at)?,
            width_m: context.number(&cut.width)?,
            base_m: context.number("base")? + context.number("level_elevation")?,
            height_m: context.number("height")?,
        })
    }
    fn band_geometry(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
        through: Option<u64>,
    ) -> Result<Option<crate::object_bands::BandDerived>, CoreError> {
        if definition.plan.band_faces.is_none() {
            return Ok(None);
        }
        let members = self
            .band_members(object, definition)?
            .iter()
            .filter(|member| through.is_none_or(|last| member.id <= last))
            .map(|member| self.band_member(member, definition))
            .collect::<Result<Vec<_>, _>>()?;
        if members.is_empty() {
            return Ok(None);
        }
        let ids: BTreeSet<_> = members.iter().map(|m| m.id).collect();
        let cuts = self
            .drawing
            .doc
            .entities()
            .filter_map(instance)
            .filter(|o| o.host.is_some_and(|host| ids.contains(&host)))
            .map(|o| self.band_cut(&o, &self.object_type(&o.type_name)?))
            .collect::<Result<Vec<_>, CoreError>>()?;
        crate::object_bands::derive_band(
            &members,
            &cuts,
            self.drawing.units.to_metres(),
            self.measure_tol(),
        )
        .map(Some)
    }
    fn band_facts(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
        context: &mut Context<'_>,
    ) -> Result<(), CoreError> {
        if let Some(band) = self.band_geometry(object, definition, Some(object.id))? {
            let previous = if matches!(definition.join, JoinRule::Union) {
                self.band_geometry(object, definition, object.id.checked_sub(1))?
            } else {
                None
            };
            let square = self.drawing.units.to_metres().powi(2);
            let gross = (band.geometry.gross_area
                - previous.as_ref().map_or(0.0, |b| b.geometry.gross_area))
                * square;
            let net = (band.geometry.net_area
                - previous.as_ref().map_or(0.0, |b| b.geometry.net_area))
                * square;
            context
                .values
                .insert("gross_area".into(), ExprValue::Number(gross));
            context
                .values
                .insert("net_area".into(), ExprValue::Number(net));
            context.values.insert("area".into(), ExprValue::Number(net));
            let gross_volume =
                band.gross_volume - previous.as_ref().map_or(0.0, |b| b.gross_volume);
            context
                .values
                .insert("gross_volume".into(), ExprValue::Number(gross_volume));
            context.values.insert(
                "geometry.gross_volume".into(),
                ExprValue::Number(gross_volume),
            );
            let volume = band.net_volume - previous.as_ref().map_or(0.0, |b| b.net_volume);
            context
                .values
                .insert("volume".into(), ExprValue::Number(volume));
            context
                .values
                .insert("geometry.volume".into(), ExprValue::Number(volume));
            context
                .values
                .insert("geometry.gross_area".into(), ExprValue::Number(gross));
            context
                .values
                .insert("geometry.net_area".into(), ExprValue::Number(net));
            context
                .values
                .insert("geometry.area".into(), ExprValue::Number(net));
        }
        Ok(())
    }
    fn object_graphics(
        &self,
        object: &ObjectInstance,
        definition: &ObjectType,
    ) -> Result<Vec<(String, EntityType)>, CoreError> {
        let driver = self
            .drawing
            .doc
            .get_entity(Handle::new(object.id))
            .ok_or_else(|| invalid("Missing driver"))?;
        if crate::object_regions::retained_region(driver).is_some_and(|region| !region.valid) {
            return Ok(Vec::new());
        }
        let paths = qiyas_geom::flatten(
            &self.drawing.doc,
            driver,
            &qiyas_geom::Xform::identity(),
            self.measure_tol(),
        )
        .into_paths();
        match definition.geometry {
            GeometryKind::Region
                if paths.len() != 1 || !paths[0].closed || paths[0].area() <= 0.0 =>
            {
                return Err(invalid(
                    "Region object requires a closed driver with positive area",
                ))
            }
            GeometryKind::Path | GeometryKind::Band
                if paths.len() != 1 || paths[0].length() <= 0.0 =>
            {
                return Err(invalid("Path object requires a nonempty path driver"))
            }
            GeometryKind::Point | GeometryKind::Hosted
                if !matches!(driver, EntityType::Point(_) | EntityType::Insert(_)) =>
            {
                return Err(invalid("Point object requires a point or insert driver"))
            }
            _ => {}
        }
        if definition.plan.symbols_in_driver && !matches!(driver, EntityType::Insert(_)) {
            return Err(invalid("symbols_in_driver requires an INSERT driver"));
        }
        if definition.geometry == GeometryKind::Hosted {
            let host = object
                .host
                .and_then(|h| self.drawing.doc.get_entity(Handle::new(h)))
                .and_then(instance)
                .ok_or_else(|| invalid("Hosted object requires an existing object host"))?;
            if host.id == object.id {
                return Err(invalid("An object cannot host itself"));
            }
            let host_entity = self
                .drawing
                .doc
                .get_entity(Handle::new(host.id))
                .expect("host");
            if host_entity.common().owner_handle != driver.common().owner_handle {
                return Err(invalid("Host and object must be on the same sheet"));
            }
        }
        let mut context = self.object_context(object, definition)?;
        self.band_facts(object, definition, &mut context)?;
        self.quantity_values(definition, &mut context)?;
        let band = self.band_geometry(object, definition, None)?;
        let draws_union = !matches!(definition.join, JoinRule::Union)
            || self
                .band_members(object, definition)?
                .first()
                .is_some_and(|member| member.id == object.id);
        let mut specs = Vec::new();
        if let Some(band) = band {
            if draws_union {
                let members = self
                    .band_members(object, definition)?
                    .iter()
                    .map(|m| self.band_member(m, definition))
                    .collect::<Result<Vec<_>, _>>()?;
                specs.extend(crate::object_bands::band_graphics(
                    &members,
                    &band,
                    &definition.plan,
                    false,
                ));
            }
        } else {
            let regions: Vec<_> = if let Some(region) = crate::object_regions::region_graphics(
                &self.drawing.doc,
                driver,
                self.measure_tol(),
            ) {
                vec![region]
            } else {
                paths
                    .iter()
                    .filter(|p| p.closed)
                    .map(|p| qiyas_geom::walls::Region {
                        outer: p.points.clone(),
                        holes: vec![],
                    })
                    .collect()
            };
            if definition.plan.outline {
                for region in &regions {
                    for ring in std::iter::once(&region.outer).chain(region.holes.iter()) {
                        specs.push((
                            "outline".to_owned(),
                            DrawSpec::Polyline {
                                points: ring.clone().into(),
                                closed: true,
                                bulges: vec![],
                            },
                        ));
                    }
                }
                if regions.is_empty() {
                    for path in &paths {
                        specs.push((
                            "outline".to_owned(),
                            DrawSpec::Polyline {
                                points: path.points.clone().into(),
                                closed: path.closed,
                                bulges: vec![],
                            },
                        ));
                    }
                }
            }
            if definition.plan.hatch {
                for region in &regions {
                    specs.push((
                        "hatch".to_owned(),
                        DrawSpec::Hatch {
                            points: region.outer.clone().into(),
                            holes: region.holes.iter().cloned().map(Into::into).collect(),
                            boundary_handles: vec![],
                            bulges: vec![],
                            pattern: crate::HatchPattern::Solid,
                        },
                    ));
                }
            }
        }
        if definition.plan.centreline && !definition.plan.centreline_driver {
            for path in &paths {
                specs.push((
                    "centreline".to_owned(),
                    DrawSpec::Polyline {
                        points: path.points.clone().into(),
                        closed: path.closed,
                        bulges: vec![],
                    },
                ));
            }
        }
        let mut result = Vec::new();
        for (role, spec) in specs {
            result.extend(
                self.draw_entities(spec)?
                    .into_iter()
                    .map(|e| (role.clone(), e)),
            );
        }
        let pos = driver_position(driver);
        for (index, spec) in evaluated_symbol_specs(
            if definition.plan.symbols_in_driver {
                &[]
            } else {
                &definition.plan.symbols
            },
            &context,
        )?
        .into_iter()
        .enumerate()
        {
            // Property dimensions are physical sizes. Driver scaling moves the insertion;
            // size changes are explicit property edits, independent of this local rotation.
            let placing = crate::modify::Placing::plain(
                opencadcodec::types::Transform::from_rotation(
                    opencadcodec::Vector3::UNIT_Z,
                    driver_rotation(driver),
                )
                .then(&opencadcodec::types::Transform::from_translation(
                    opencadcodec::Vector3::new(pos[0], pos[1], 0.0),
                )),
            );
            for mut e in self.draw_entities(spec)? {
                if matches!(driver, EntityType::Point(point) if point.normal.z < 0.0) {
                    // Reflect before rotation, correcting curved entities and keeping text readable.
                    crate::modify::place(
                        &mut e,
                        &crate::modify::Placing::mirror(
                            opencadcodec::types::Transform::from_mirror_y(),
                            0.0,
                        ),
                    );
                }
                crate::modify::place(&mut e, &placing);
                result.push((format!("symbol:{index}"), e));
            }
        }
        if let Some(template) = &definition.plan.label_template {
            let mut label = template
                .replace("{type}", &definition.name)
                .replace("{id}", &object.id.to_string());
            for (key, value) in &context.values {
                let text = match value {
                    ExprValue::Number(n) => format!("{n:.3}"),
                    ExprValue::Text(s) => s.clone(),
                    ExprValue::Bool(b) => b.to_string(),
                };
                label = label.replace(&format!("{{{key}}}"), &text);
            }
            let height = definition
                .plan
                .label_height
                .as_deref()
                .map(|e| {
                    context
                        .number(e)
                        .map(|n| n / self.drawing.units.to_metres())
                })
                .transpose()?
                .unwrap_or(self.default_text_height());
            if height <= 0.0 {
                return Err(invalid("Label height must be positive"));
            }
            let mut text = opencadcodec::Text::new();
            text.insertion_point = opencadcodec::Vector3::new(pos[0], pos[1], 0.0);
            text.height = height;
            text.value = label;
            result.push(("label".into(), EntityType::Text(text)));
        }
        for (role, e) in &mut result {
            let source = driver.common();
            let common = e.common_mut();
            common.layer = object
                .layers
                .get(role)
                .or_else(|| definition.plan.role_layers.get(role))
                .cloned()
                .unwrap_or_else(|| source.layer.clone());
            common.owner_handle = source.owner_handle;
            common.color = source.color;
            common.color_name = source.color_name.clone();
            common.color_book_handle = source.color_book_handle;
            common.line_weight = source.line_weight;
            common.linetype = source.linetype.clone();
            common.linetype_handle = source.linetype_handle;
            common.linetype_scale = source.linetype_scale;
            common.transparency = source.transparency;
            common.invisible = source.invisible && !definition.plan.centreline_driver;
            common.material_flags = source.material_flags;
            common.material_handle = source.material_handle;
            common.plotstyle_flags = source.plotstyle_flags;
            common.plotstyle_handle = source.plotstyle_handle;
            common.full_visual_style_handle = source.full_visual_style_handle;
            common.face_visual_style_handle = source.face_visual_style_handle;
            common.edge_visual_style_handle = source.edge_visual_style_handle;
        }
        Ok(result)
    }
    fn refresh_hosted_positions(&mut self) -> Result<(), CoreError> {
        let objects: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(instance)
            .filter(|o| o.host.is_some() && o.compatibility.is_none())
            .collect();
        for object in objects {
            let definition = self.object_type(&object.type_name)?;
            let Some(cut) = &definition.host_cut else {
                continue;
            };
            let host = object
                .host
                .and_then(|h| self.drawing.doc.get_entity(Handle::new(h)))
                .and_then(instance)
                .ok_or_else(|| invalid("Remove hosted objects before deleting their host"))?;
            let host_definition = self.object_type(&host.type_name)?;
            let member = self.band_member(&host, &host_definition)?;
            let evaluated = self.band_cut(&object, &definition)?;
            let built = crate::object_bands::derive_band(
                &[member],
                &[evaluated],
                self.drawing.units.to_metres(),
                self.measure_tol(),
            )?;
            let placed = built
                .geometry
                .openings
                .first()
                .ok_or_else(|| invalid("Missing hosted position"))?;
            let context = self.object_context(&object, &definition)?;
            let factor = self.drawing.units.to_metres();
            let along = cut
                .offset_along
                .as_deref()
                .map(|e| context.number(e))
                .transpose()?
                .unwrap_or(0.0)
                / factor;
            let normal = cut
                .offset_normal
                .as_deref()
                .map(|e| context.number(e))
                .transpose()?
                .unwrap_or(0.0)
                / factor;
            let mirror = cut
                .mirror
                .as_deref()
                .map(|e| {
                    expression(e)?
                        .eval_with(&context)
                        .and_then(|v| v.as_bool())
                        .map_err(|e| invalid(e.to_string()))
                })
                .transpose()?;
            let [dx, dy] = placed.dir;
            let at = [
                placed.centre[0] + dx * along - dy * normal,
                placed.centre[1] + dy * along + dx * normal,
            ];
            let angle = dy.atan2(dx).rem_euclid(std::f64::consts::TAU);
            let entity = self
                .drawing
                .doc
                .get_entity_mut(Handle::new(object.id))
                .ok_or_else(|| invalid("Missing hosted driver"))?;
            match entity {
                EntityType::Point(point) => {
                    point.location.x = at[0];
                    point.location.y = at[1];
                    point.x_axis_angle = angle;
                    if let Some(mirror) = mirror {
                        point.normal.z = if mirror { -1.0 } else { 1.0 };
                    }
                }
                EntityType::Insert(insert) => {
                    insert.insert_point.x = at[0];
                    insert.insert_point.y = at[1];
                    insert.rotation = angle;
                    if definition.plan.symbols_in_driver {
                        // The evaluated block already carries the physical dimensions.
                        insert.set_x_scale(insert.x_scale().signum());
                        insert.set_y_scale(insert.y_scale().signum());
                        insert.set_z_scale(1.0);
                    }
                    if let Some(mirror) = mirror {
                        // The hosted frame expression owns reflection after ordinary copy
                        // or mirror commands have transformed the native INSERT scales.
                        insert.set_y_scale(insert.y_scale().abs());
                        insert.set_x_scale(if mirror {
                            -insert.x_scale().abs()
                        } else {
                            insert.x_scale().abs()
                        });
                    }
                }
                _ => return Err(invalid("Hosted cut drivers must be points or inserts")),
            }
            entity.common_mut().raw_record = None;
            entity.common_mut().graphic_data = None;
        }
        Ok(())
    }
    fn object_part_region(&self, entity: &EntityType) -> Option<qiyas_geom::walls::Region> {
        if !matches!(entity, EntityType::Hatch(_)) {
            return None;
        }
        let mut paths = qiyas_geom::flatten(
            &self.drawing.doc,
            entity,
            &qiyas_geom::Xform::identity(),
            self.measure_tol(),
        )
        .into_paths()
        .into_iter();
        let outer = paths.next()?.points;
        Some(qiyas_geom::walls::Region {
            outer,
            holes: paths.map(|p| p.points).collect(),
        })
    }
    /// Replaces matching parts under their existing handles, so measurements and fields retain
    /// their sources. Removed parts follow the same remapping path as a wall rebuild.
    pub(crate) fn refresh_objects(&mut self) -> Result<(), CoreError> {
        self.validate_object_metadata()?;
        if !self.drawing.doc.app_ids.contains(OBJECT_APP) {
            return Ok(());
        }
        let copies: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                instance(e)
                    .filter(|o| o.id != e.common().handle.value())
                    .map(|o| (e.common().handle.value(), o))
            })
            .collect();
        let mut copy_map: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for (h, o) in &copies {
            copy_map.entry(o.id).or_default().push(*h);
        }
        for values in copy_map.values_mut() {
            values.sort_unstable();
        }
        let mut assembly_copies: BTreeMap<(String, u64, usize), u64> = BTreeMap::new();
        for (h, object) in &copies {
            if let Some(assembly) = &object.assembly {
                let ordinal = copy_map[&object.id]
                    .iter()
                    .position(|v| v == h)
                    .expect("copy listed");
                let sheet = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(*h))
                    .expect("copy exists")
                    .common()
                    .owner_handle
                    .value();
                assembly_copies
                    .entry((assembly.clone(), sheet, ordinal))
                    .and_modify(|anchor| *anchor = (*anchor).min(*h))
                    .or_insert(*h);
            }
        }
        for (h, mut object) in copies {
            let ordinal = copy_map[&object.id]
                .iter()
                .position(|v| *v == h)
                .expect("copy listed");
            if let Some(host) = object
                .host
                .and_then(|host| copy_map.get(&host))
                .and_then(|copies| copies.get(ordinal))
            {
                object.host = Some(*host);
            }
            if let Some(assembly) = &object.assembly {
                let sheet = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(h))
                    .expect("copy exists")
                    .common()
                    .owner_handle
                    .value();
                let anchor = assembly_copies[&(assembly.clone(), sheet, ordinal)];
                object.assembly = Some(format!("copy:{anchor:X}"));
            }
            object.id = h;
            self.stamp_object(h, &object)?;
        }
        self.refresh_hosted_positions()?;
        let drivers: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| instance(e).map(|i| (e.common().handle.value(), i)))
            .collect();
        let ids: BTreeSet<_> = drivers.iter().map(|(h, _)| *h).collect();
        let stale: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                crate::xdata::read_json::<ObjectPart>(e.common(), PART_APP)
                    .filter(|p| !ids.contains(&p.owner))
                    .map(|_| e.common().handle)
            })
            .collect();
        for h in &stale {
            self.drawing.doc.remove_entity_arc(*h);
        }
        self.follow_edit(&[], &stale, &[], &[]);
        let app = self.object_app(PART_APP)?;
        for (handle, mut object) in drivers {
            // Ordinary copy commands copy xdata too: a new driver gets its own identity.
            if object.id != handle {
                object.id = handle;
                self.stamp_object(handle, &object)?;
            }
            let definition = self.object_type(&object.type_name)?;
            object.props = definition.properties(&object.props)?;
            let context = self.object_context(&object, &definition)?;
            for name in definition.property_bindings.keys() {
                if let Some(value) = context.values.get(name) {
                    object.props.insert(name.clone(), expr_json(value));
                }
            }
            self.stamp_object(handle, &object)?;
            for layer in definition
                .plan
                .role_layers
                .values()
                .chain(object.layers.values())
            {
                if layer.trim().is_empty() || layer.contains('|') {
                    return Err(invalid("Object role layers must name local drawing layers"));
                }
                if !self.drawing.doc.layers.contains(layer) {
                    self.new_layer(layer)?;
                }
            }
            if definition.plan.centreline_driver {
                let driver = self
                    .drawing
                    .doc
                    .get_entity_mut(Handle::new(handle))
                    .ok_or_else(|| invalid("Missing centreline driver"))?;
                driver.common_mut().invisible = !definition.plan.centreline;
            }
            let wanted = self.object_graphics(&object, &definition)?;
            let mut old: Vec<_> = self
                .drawing
                .doc
                .entities()
                .filter_map(|entity| {
                    crate::xdata::read_json::<ObjectPart>(entity.common(), PART_APP)
                        .filter(|part| part.owner == handle)
                        .map(|part| (part, entity.clone()))
                })
                .collect();
            old.sort_by_key(|(part, entity)| (part.index, entity.common().handle.value()));
            let same_role =
                |part: &ObjectPart, before: &EntityType, role: &str, after: &EntityType| {
                    (part.role == role || part.role.is_empty())
                        && std::mem::discriminant(before) == std::mem::discriminant(after)
                };
            let old_regions: Vec<_> = old
                .iter()
                .map(|(_, e)| self.object_part_region(e))
                .collect();
            let new_regions: Vec<_> = wanted
                .iter()
                .map(|(_, e)| self.object_part_region(e))
                .collect();
            let tolerance = self.measure_tol();
            let overlaps = |before: usize, after: usize| {
                old_regions[before]
                    .as_ref()
                    .zip(new_regions[after].as_ref())
                    .is_some_and(|(a, b)| qiyas_geom::walls::regions_overlap(a, b, tolerance))
            };
            let mut used = BTreeSet::new();
            let mut changed = Vec::new();
            let mut live = Vec::new();
            for (index, (role, mut entity)) in wanted.iter().cloned().enumerate() {
                let candidate = old
                    .iter()
                    .enumerate()
                    .find(|(i, (part, before))| {
                        !used.contains(i)
                            && same_role(part, before, &role, &entity)
                            && (role != "hatch" || overlaps(*i, index))
                    })
                    .or_else(|| {
                        // A move can leave no geometric overlap. The same number of matching
                        // parts still preserves their identities by order within the role.
                        let old_count = old
                            .iter()
                            .filter(|(part, e)| same_role(part, e, &role, &entity))
                            .count();
                        let new_count = wanted
                            .iter()
                            .filter(|(r, e)| {
                                *r == role
                                    && std::mem::discriminant(e) == std::mem::discriminant(&entity)
                            })
                            .count();
                        (old_count == new_count)
                            .then(|| {
                                old.iter().enumerate().find(|(i, (part, before))| {
                                    !used.contains(i) && same_role(part, before, &role, &entity)
                                })
                            })
                            .flatten()
                    })
                    .map(|(i, (_, before))| (i, before.common().handle));
                if let Some((i, _)) = candidate {
                    // Regenerating geometry must retain other applications' metadata on a
                    // surviving part. The shared writer replaces only our ownership record.
                    entity.common_mut().extended_data = old[i].1.common().extended_data.clone();
                }
                crate::xdata::write_json(
                    entity.common_mut(),
                    PART_APP,
                    app,
                    &ObjectPart {
                        owner: handle,
                        index,
                        role,
                    },
                )?;
                if let Some((i, h)) = candidate {
                    used.insert(i);
                    entity.common_mut().handle = h;
                    if self.drawing.doc.get_entity(h) != Some(&entity) {
                        *self.drawing.doc.get_entity_mut(h).expect("part exists") = entity;
                        changed.push(h);
                    }
                    live.push(h.value());
                } else {
                    live.push(
                        self.drawing
                            .doc
                            .add_entity(entity)
                            .map_err(|e| invalid(e.to_string()))?
                            .value(),
                    );
                }
            }
            let mut mapping = BTreeMap::new();
            for (i, (part, before)) in old.iter().enumerate() {
                let old_handle = before.common().handle.value();
                let matching: Vec<_> = wanted
                    .iter()
                    .enumerate()
                    .filter(|(_, (role, after))| same_role(part, before, role, after))
                    .collect();
                let replacements: Vec<_> = if matches!(before, EntityType::Hatch(_)) {
                    let overlapping: Vec<_> = matching
                        .iter()
                        .filter(|(j, _)| overlaps(i, *j))
                        .map(|(j, _)| live[*j])
                        .collect();
                    if overlapping.is_empty() && live.contains(&old_handle) {
                        vec![old_handle]
                    } else {
                        overlapping
                    }
                } else if live.contains(&old_handle) {
                    let old_count = old
                        .iter()
                        .filter(|(p, e)| same_role(p, e, &part.role, before))
                        .count();
                    if part.role == "outline" && old_count != matching.len() {
                        matching.iter().map(|(j, _)| live[*j]).collect()
                    } else {
                        vec![old_handle]
                    }
                } else if part.role == "outline" {
                    matching.iter().map(|(j, _)| live[*j]).collect()
                } else {
                    Vec::new()
                };
                if replacements != [old_handle] {
                    mapping.insert(old_handle, replacements);
                }
            }
            let removed: Vec<_> = old
                .iter()
                .enumerate()
                .filter(|(i, _)| !used.contains(i))
                .map(|(_, (_, e))| e.common().handle)
                .collect();
            for h in &removed {
                self.drawing.doc.remove_entity_arc(*h);
            }
            let pieces: Vec<_> = mapping
                .iter()
                .map(|(h, replacements)| (Handle::new(*h), replacements.clone()))
                .collect();
            self.follow_edit(&changed, &removed, &pieces, &[]);
            self.remap_wall_fields(&mapping)?;
        }
        self.refresh_insert_symbol_recipes()
    }
}
fn validate_symbol(value: &mut Value, known: &BTreeSet<String>) -> Result<(), CoreError> {
    match value {
        Value::String(s) if s.starts_with('=') => {
            for name in expression(&s[1..])?.names() {
                if !known.contains(&name) && !reserved(&name) {
                    return Err(invalid(format!("Unknown symbol expression name {name}")));
                }
            }
            *value = json!(1.0);
        }
        Value::Array(a) => {
            for v in a {
                validate_symbol(v, known)?;
            }
        }
        Value::Object(m) => {
            for v in m.values_mut() {
                validate_symbol(v, known)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn evaluated_symbol_specs(
    symbols: &[Value],
    context: &Context<'_>,
) -> Result<Vec<DrawSpec>, CoreError> {
    symbols
        .iter()
        .map(|symbol| {
            let mut value = symbol.clone();
            evaluate_symbol(&mut value, context)?;
            serde_json::from_value(value)
                .map_err(|e| invalid(format!("Invalid symbol recipe: {e}")))
        })
        .collect()
}

fn evaluate_symbol(value: &mut Value, context: &Context<'_>) -> Result<(), CoreError> {
    match value {
        Value::String(s) if s.starts_with('=') => *value = json!(context.number(&s[1..])?),
        Value::Array(a) => {
            for v in a {
                evaluate_symbol(v, context)?;
            }
        }
        Value::Object(m) => {
            for v in m.values_mut() {
                evaluate_symbol(v, context)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn record_value(record: &ObjectRecord, name: &str) -> Option<Value> {
    match name {
        "id" => Some(json!(record.instance.id)),
        "type" => Some(json!(record.instance.type_name)),
        "category" => Some(json!(record.category)),
        "level" => Some(json!(record.instance.level)),
        _ => record
            .quantities
            .get(name)
            .or_else(|| record.facts.get(name))
            .map(|n| json!(n))
            .or_else(|| record.instance.props.get(name).cloned()),
    }
}
pub(crate) fn display(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        _ => value.to_string(),
    }
}
