//! Drawing-owned decisions and explicit property references to their values.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::Handle;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::objects::{Property, UnitKind};
use crate::{CoreError, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    Engineer,
    Drawing,
    Document,
    MizanDesign,
    CodeOrStandard,
    AssumptionAllowed,
    Default,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Setter {
    Engineer,
    #[serde(alias = "Mizan")]
    Mizan,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Asked,
    Answered,
    Provisional,
}

/// One explicit value or unanswered question. Text and citations are retained verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub name: String,
    /// Null only for an unanswered question; otherwise a number, string or boolean.
    pub value: Value,
    /// Physical unit, or empty for a nonnumeric value or dimensionless number.
    pub unit: String,
    pub reason: String,
    pub source: DecisionSource,
    pub citation: String,
    pub set_by: Setter,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

/// A single entry or an atomic set of entries.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Entries {
    One(Box<Decision>),
    Many(Vec<Decision>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionCommand {
    /// Save one or many decisions and update explicitly linked properties and fields atomically.
    /// حفظ قرار أو عدة قرارات وتحديث الخصائص والحقول المرتبطة في خطوة واحدة.
    #[schemars(example = json!({"kind":"set","entries":{"name":"height","value":3,"unit":"m","reason":"Engineer brief / موجز المهندس","source":"engineer","citation":"Brief / الموجز","set_by":"engineer","status":"answered"}}))]
    Set { entries: Entries },
    /// Read the decision register, optionally by name, with pagination.
    /// قراءة سجل القرارات مع التصفية بالاسم وتقسيم النتائج إلى صفحات.
    #[schemars(example = json!({"kind":"list","offset":0,"limit":20}))]
    List {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        offset: usize,
        #[serde(default = "default_limit")]
        limit: usize,
    },
}
fn default_limit() -> usize {
    20
}
fn invalid(en: &str, ar: &str) -> CoreError {
    CoreError::InvalidCommand(format!("{en} / {ar}"))
}
impl Decision {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.name.is_empty()
            || self.name.trim() != self.name
            || self.name.len() > 255
            || self.name.chars().any(|c| c.is_control() || c == '"')
            || self.name.contains("%<")
            || self.name.contains(">%")
        {
            return Err(invalid("Invalid decision name", "اسم القرار غير صالح"));
        }
        if self.reason.trim().is_empty()
            || self.value.is_array()
            || self.value.is_object()
            || (self.status != Status::Asked && self.value.is_null())
            || (self.status == Status::Asked
                && (!self.value.is_null()
                    || self.question.as_deref().is_none_or(|s| s.trim().is_empty())))
        {
            return Err(invalid(
                "Give a reason and a scalar value, or an unanswered question with null value",
                "أدخل سبباً وقيمة مفردة أو سؤالاً بلا إجابة وقيمته فارغة",
            ));
        }
        Ok(())
    }
}

fn decode(entry: knowledge::Entry) -> Result<Decision, CoreError> {
    let decision: Decision = serde_json::from_value(entry.data)
        .map_err(|e| invalid(&format!("Invalid decision: {e}"), "سجل القرار غير صالح"))?;
    decision.validate()?;
    if !decision.name.eq_ignore_ascii_case(&entry.name) {
        return Err(invalid(
            "Decision name differs from its record",
            "اسم القرار مختلف عن اسم السجل",
        ));
    }
    Ok(decision)
}

impl Session {
    pub fn decisions(&self) -> Result<Vec<Decision>, CoreError> {
        knowledge::list(&self.drawing.doc, Kind::Decisions)?
            .into_iter()
            .map(decode)
            .collect()
    }

    pub(crate) fn decision(&self, name: &str) -> Result<Option<Decision>, CoreError> {
        knowledge::get(&self.drawing.doc, Kind::Decisions, name)?
            .map(decode)
            .transpose()
    }

    pub(crate) fn run_decision(&mut self, command: DecisionCommand) -> Result<Value, CoreError> {
        match command {
            DecisionCommand::List {
                name,
                offset,
                limit,
            } => {
                let rows = self
                    .decisions()?
                    .into_iter()
                    .filter(|d| name.as_ref().is_none_or(|n| d.name.eq_ignore_ascii_case(n)))
                    .map(|d| json!(d))
                    .collect();
                Ok(crate::outside_check::page(json!({}), rows, offset, limit))
            }
            DecisionCommand::Set { entries } => {
                let entries = match entries {
                    Entries::One(d) => vec![*d],
                    Entries::Many(ds) => ds,
                };
                if entries.is_empty() {
                    return Err(invalid(
                        "Give at least one decision",
                        "أدخل قراراً واحداً على الأقل",
                    ));
                }
                let mut names = BTreeSet::new();
                for d in &entries {
                    d.validate()?;
                    if !names.insert(d.name.to_lowercase()) {
                        return Err(invalid("Duplicate decision name", "اسم قرار مكرر"));
                    }
                }
                // The command recorder rolls back knowledge and all affected entities together.
                for d in &entries {
                    knowledge::put(&mut self.drawing.doc, Kind::Decisions, &d.name, &json!(d))?;
                }
                let objects: Vec<_> = self
                    .drawing
                    .doc
                    .entities()
                    .filter_map(crate::objects::instance)
                    .collect();
                for object in objects {
                    if !object.sources.values().any(|s| {
                        s.decision
                            .as_ref()
                            .is_some_and(|name| names.contains(&name.to_lowercase()))
                    }) {
                        continue;
                    }
                    let mut props = BTreeMap::new();
                    let definition = self.object_type(&object.type_name)?;
                    for (property, source) in &object.sources {
                        let Some(name) = &source.decision else {
                            continue;
                        };
                        if names.contains(&name.to_lowercase()) {
                            let value = self.decision_property_value(
                                name,
                                definition.properties.get(property),
                            )?;
                            props.insert(property.clone(), value);
                        }
                    }
                    if props.is_empty() {
                        continue;
                    }
                    let entity = self
                        .drawing
                        .doc
                        .get_entity(Handle::new(object.id))
                        .ok_or(CoreError::NoSuchEntity(object.id))?;
                    let owner = entity.common().owner_handle;
                    let sheet = self
                        .drawing
                        .sheets()
                        .into_iter()
                        .find(|s| {
                            self.drawing
                                .doc
                                .block_records
                                .get(&s.block)
                                .is_some_and(|b| b.handle == owner)
                        })
                        .ok_or_else(|| {
                            invalid(
                                "Linked property is not owned by a sheet",
                                "الخاصية المرتبطة لا تتبع لوحة",
                            )
                        })?;
                    if self.is_locked(Handle::new(object.id)) {
                        return Err(invalid(
                            "A linked property is locked",
                            "إحدى الخصائص المرتبطة مقفلة",
                        ));
                    }
                    let previous = std::mem::replace(&mut self.sheet, sheet.block);
                    let result = self.set_object_props(
                        &[object.id],
                        &props,
                        None,
                        &object.sources,
                        crate::measurement::PropSource::default(),
                    );
                    self.sheet = previous;
                    result?;
                }
                self.update_fields(false)?;
                Ok(json!({"changed":entries.len()}))
            }
        }
    }

    pub(crate) fn decision_property_value(
        &self,
        name: &str,
        property: Option<&Property>,
    ) -> Result<Value, CoreError> {
        let d = self
            .decision(name)?
            .ok_or_else(|| invalid("Decision does not exist", "القرار غير موجود"))?;
        if d.value.is_null() {
            return Err(invalid(
                "An unanswered decision has no property value",
                "القرار الذي لم تتم الإجابة عنه ليس له قيمة للخاصية",
            ));
        }
        let property = property.ok_or_else(|| {
            invalid(
                "Decision reference names an undeclared property",
                "مرجع القرار يشير إلى خاصية غير معرّفة",
            )
        })?;
        if let Property::Number { unit, .. } = property {
            let expected = match unit {
                UnitKind::Length => "m",
                UnitKind::Area => "m2",
                UnitKind::Volume => "m3",
                UnitKind::Angle => "deg",
                UnitKind::Count => "nr",
                UnitKind::Scalar => "",
            };
            let (dimension, scale) = unit_scale(&d.unit);
            if dimension != expected {
                return Err(invalid(
                    "Decision and property units differ",
                    "وحدات القرار والخاصية مختلفة",
                ));
            }
            return d.value.as_f64().map(|n| json!(n * scale)).ok_or_else(|| {
                invalid(
                    "Numeric property needs a numeric decision",
                    "الخاصية الرقمية تحتاج إلى قرار رقمي",
                )
            });
        }
        Ok(d.value)
    }

    pub(crate) fn validate_decision_sources(
        &self,
        props: &BTreeMap<String, Value>,
        sources: &BTreeMap<String, crate::measurement::PropSource>,
        definition: &crate::objects::ObjectType,
    ) -> Result<(), CoreError> {
        for (name, source) in sources {
            if let Some(decision) = &source.decision {
                let value =
                    self.decision_property_value(decision, definition.properties.get(name))?;
                let matches = props.get(name).is_some_and(|p| {
                    p == &value
                        || p.as_f64()
                            .zip(value.as_f64())
                            .is_some_and(|(a, b)| same_number(a, b))
                });
                if !matches {
                    return Err(invalid(
                        "Property must equal its cited decision in the property's units",
                        "يجب أن تساوي الخاصية القرار المشار إليه بوحدات الخاصية",
                    ));
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn same_number(a: f64, b: f64) -> bool {
    (a - b).abs() <= f64::EPSILON * 64.0 * a.abs().max(b.abs()).max(1.0)
}
/// Unit aliases and conversions only, never design knowledge.
pub(crate) fn unit_scale(unit: &str) -> (String, f64) {
    let unit = unit
        .trim()
        .to_lowercase()
        .replace('²', "2")
        .replace('³', "3")
        .replace('^', "");
    let (name, factor) = match unit.as_str() {
        "m" | "م" | "metre" | "meter" | "metres" | "meters" | "متر" | "أمتار" => {
            ("m", 1.0)
        }
        "mm" | "مم" | "millimetres" | "millimeters" => ("m", 0.001),
        "cm" | "سم" | "centimetres" | "centimeters" => ("m", 0.01),
        "ft" | "feet" | "قدم" => ("m", 0.3048),
        "in" | "inch" => ("m", 0.0254),
        "m2" | "م2" | "sqm" | "sq m" | "square metres" | "square meters" | "متر مربع" => {
            ("m2", 1.0)
        }
        "mm2" | "مم2" => ("m2", 1e-6),
        "cm2" | "سم2" => ("m2", 1e-4),
        "ft2" => ("m2", 0.09290304),
        "m3" | "م3" | "cum" | "cu m" | "cubic metres" | "cubic meters" | "متر مكعب" => {
            ("m3", 1.0)
        }
        "mm3" | "مم3" => ("m3", 1e-9),
        "cm3" | "سم3" => ("m3", 1e-6),
        "pcs" | "pc" | "nr" | "no" | "nos" | "عدد" | "قطعة" | "قطع" => ("nr", 1.0),
        "deg" | "°" | "درجة" => ("deg", 1.0),
        "l" | "litre" | "liter" | "لتر" => ("l", 1.0),
        "s" | "sec" | "ثانية" => ("s", 1.0),
        "h" | "hr" | "ساعة" => ("h", 1.0),
        "n" => ("n", 1.0),
        "kn" => ("n", 1000.0),
        "pa" => ("pa", 1.0),
        "kpa" => ("pa", 1000.0),
        "mpa" => ("pa", 1e6),
        "%" => ("%", 1.0),
        "kg" | "كجم" => ("kg", 1.0),
        "t" | "طن" => ("kg", 1000.0),
        _ => return (unit, 1.0),
    };
    (name.into(), factor)
}
