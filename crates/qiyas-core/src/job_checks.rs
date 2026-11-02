//! Numeric facts compared only with cited limits stored in this drawing.
use crate::{knowledge, measurement::Citation, CoreError, Session};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) struct SpaceClassifications(BTreeMap<(String, String), Option<String>>);
impl SpaceClassifications {
    pub(crate) fn get(&self, floor: &str, name: &str) -> Option<String> {
        self.0.get(&(floor.into(), name.into())).cloned().flatten()
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Fact {
    pub target: String,
    pub floor: String,
    pub name: String,
    pub classification: Option<String>,
    pub metric: String,
    pub unit: String,
    pub measured: f64,
    pub handle: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Bound {
    Value(f64),
    Decision { decision: String },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Limit {
    kind: String,
    target: String,
    metric: String,
    unit: String,
    #[serde(default)]
    floor: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    classification: Option<String>,
    #[serde(default)]
    minimum: Option<Bound>,
    #[serde(default)]
    maximum: Option<Bound>,
    #[serde(default)]
    citation: Option<Citation>,
}
fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}
impl Limit {
    fn decode(value: &Value) -> Result<Self, CoreError> {
        let rule: Self =
            serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
        if rule.kind != "measurement_limit"
            || rule.target.trim().is_empty()
            || rule.metric.trim().is_empty()
            || (rule.minimum.is_none() && rule.maximum.is_none())
        {
            return Err(invalid(
                "A measurement limit needs a target, metric and minimum or maximum",
            ));
        }
        for bound in [&rule.minimum, &rule.maximum].into_iter().flatten() {
            match bound {
                Bound::Value(n)
                    if !n.is_finite()
                        || rule.citation.as_ref().is_none_or(|c| {
                            c.source.trim().is_empty() || c.clause.trim().is_empty()
                        }) =>
                {
                    return Err(invalid(
                        "Numeric limits need a finite value and a source and clause",
                    ))
                }
                Bound::Decision { decision } if decision.trim().is_empty() => {
                    return Err(invalid("A limit needs a decision name"))
                }
                _ => {}
            }
        }
        Ok(rule)
    }
    fn matches(&self, fact: &Fact) -> bool {
        self.target == fact.target
            && self.metric == fact.metric
            && self.floor.as_ref().is_none_or(|v| v == &fact.floor)
            && self.name.as_ref().is_none_or(|v| v == &fact.name)
            && self
                .classification
                .as_ref()
                .is_none_or(|v| Some(v) == fact.classification.as_ref())
    }
}
pub(crate) fn validate(value: &Value) -> Result<(), CoreError> {
    if value["kind"] == "measurement_limit" {
        Limit::decode(value)?;
    }
    Ok(())
}
impl Session {
    fn resolve_bound(&self, bound: &Bound, rule: &Limit) -> Result<(f64, Value), CoreError> {
        match bound {
            Bound::Value(n) => Ok((*n, json!(rule.citation))),
            Bound::Decision { decision } => {
                let d = self
                    .decision(decision)?
                    .ok_or_else(|| invalid(format!("Missing decision: {decision}")))?;
                if d.status == crate::decisions::Status::Asked
                    || d.citation.trim().is_empty()
                    || d.unit != rule.unit
                {
                    return Err(invalid(format!(
                        "Decision {decision} needs a value, citation and unit {}",
                        rule.unit
                    )));
                }
                let n = d
                    .value
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or_else(|| invalid(format!("Decision {decision} is not numeric")))?;
                Ok((n, json!(d)))
            }
        }
    }
    pub(crate) fn check_facts(&self, facts: &[Fact]) -> Result<Vec<Value>, CoreError> {
        let rules = knowledge::list(&self.drawing.doc, knowledge::Kind::Rules)?;
        let rules: Vec<_> = rules
            .into_iter()
            .filter(|r| r.data["kind"] == "measurement_limit")
            .map(|r| Limit::decode(&r.data).map(|limit| (r.name, limit)))
            .collect::<Result<_, _>>()?;
        let mut rows = Vec::new();
        let mut matched = std::collections::BTreeSet::new();
        for fact in facts {
            let mut found = false;
            for (name, rule) in &rules {
                if !rule.matches(fact) {
                    continue;
                }
                found = true;
                matched.insert(name.clone());
                let mut row = json!(fact);
                row["rule"] = json!(name);
                let resolved: Result<_, CoreError> = (|| {
                    if rule.unit != fact.unit {
                        return Err(invalid(format!(
                            "Limit unit {} differs from measured unit {}",
                            rule.unit, fact.unit
                        )));
                    }
                    let lo = rule
                        .minimum
                        .as_ref()
                        .map(|v| self.resolve_bound(v, rule))
                        .transpose()?;
                    let hi = rule
                        .maximum
                        .as_ref()
                        .map(|v| self.resolve_bound(v, rule))
                        .transpose()?;
                    if lo
                        .as_ref()
                        .zip(hi.as_ref())
                        .is_some_and(|(lo, hi)| lo.0 > hi.0)
                    {
                        return Err(invalid("Minimum exceeds maximum"));
                    }
                    Ok((lo, hi))
                })();
                match resolved {
                    Ok((lo, hi)) => {
                        let passed = lo.as_ref().is_none_or(|v| fact.measured >= v.0)
                            && hi.as_ref().is_none_or(|v| fact.measured <= v.0);
                        row["passed"] = json!(passed);
                        row["status"] = json!(if passed { "pass" } else { "fail" });
                        row["minimum"] = json!(lo.as_ref().map(|v| v.0));
                        row["maximum"] = json!(hi.as_ref().map(|v| v.0));
                        row["citations"] =
                            json!(lo.iter().chain(hi.iter()).map(|v| &v.1).collect::<Vec<_>>());
                        row["message"] = json!(if passed {
                            "Within the supplied limits"
                        } else {
                            "Outside the supplied limits"
                        });
                    }
                    Err(e) => {
                        row["passed"] = Value::Null;
                        row["status"] = json!("unavailable");
                        row["message"] = json!(e.to_string());
                    }
                }
                rows.push(row);
            }
            if !found {
                let mut row = json!(fact);
                row["passed"] = Value::Null;
                row["message"] = json!("No limit was given");
                row["status"] = json!("no_limit");
                rows.push(row);
            }
        }
        for (name, rule) in &rules {
            if !matched.contains(name) {
                rows.push(json!({"rule":name,"target":rule.target,"metric":rule.metric,"measured":null,"passed":null,"status":"unavailable","message":"No matching measured fact was available"}));
            }
        }
        Ok(rows)
    }
    pub(crate) fn space_classifications(&self) -> Result<SpaceClassifications, CoreError> {
        let mut classifications = BTreeMap::new();
        for rule in knowledge::list(&self.drawing.doc, knowledge::Kind::Rules)? {
            if rule.data["kind"] != "space_classification" {
                continue;
            }
            let (Some(floor), Some(name)) =
                (rule.data["floor"].as_str(), rule.data["name"].as_str())
            else {
                continue;
            };
            classifications
                .entry((floor.into(), name.into()))
                .or_insert_with(|| rule.data["classification"].as_str().map(str::to_owned));
        }
        Ok(SpaceClassifications(classifications))
    }
    fn room_clear_width(&self, handle: u64) -> Option<f64> {
        let entity = self
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(handle))?;
        let k = self.drawing.units.to_metres();
        let mut rings: Vec<_> = qiyas_geom::flatten(
            &self.drawing.doc,
            entity,
            &qiyas_geom::Xform::identity(),
            self.measure_tol(),
        )
        .into_paths()
        .into_iter()
        .filter(|p| p.closed)
        .map(|p| {
            p.points
                .iter()
                .map(|p| [p[0] * k, p[1] * k])
                .collect::<Vec<_>>()
        })
        .collect();
        if let Some(region) = crate::object_regions::retained_region(entity) {
            for (i, hole) in region.holes.iter().enumerate() {
                let poly =
                    opencadcodec::EntityType::LwPolyline(crate::object_regions::region_polyline(
                        hole,
                        region.hole_bulges.get(i).map_or(&[], Vec::as_slice),
                    ));
                rings.extend(
                    qiyas_geom::flatten(
                        &self.drawing.doc,
                        &poly,
                        &qiyas_geom::Xform::identity(),
                        self.measure_tol(),
                    )
                    .into_paths()
                    .into_iter()
                    .filter(|p| p.closed)
                    .map(|p| {
                        p.points
                            .iter()
                            .map(|p| [p[0] * k, p[1] * k])
                            .collect::<Vec<_>>()
                    }),
                );
            }
        }
        crate::space_planning::minimum_clear_width(&rings, self.measure_tol() * k)
    }

    pub(crate) fn drawing_measurements(
        &self,
        sheet: &str,
        handles: &[u64],
    ) -> Result<Vec<Value>, CoreError> {
        let mut facts = vec![Fact {
            target: "drawing".into(),
            floor: sheet.into(),
            name: sheet.into(),
            classification: None,
            metric: "entity_count".into(),
            unit: "pcs".into(),
            handle: None,
            measured: self
                .drawing
                .doc
                .entities_in_block(sheet)
                .filter(|e| handles.is_empty() || handles.contains(&e.common().handle.value()))
                .count() as f64,
        }];
        let objects = self.object_records_for(
            &crate::objects::ObjectFilter {
                sheet: Some(sheet.into()),
                ..Default::default()
            },
            (!handles.is_empty()).then_some(handles),
            false,
        )?;
        let classifications = self.space_classifications()?;
        for object in objects {
            if !handles.is_empty() && !handles.contains(&object.instance.id) {
                continue;
            }
            let props = &object.instance.props;
            let target = if object.category == "opening" {
                props
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("opening")
            } else {
                &object.category
            };
            let name = props
                .get("name")
                .or_else(|| props.get("mark"))
                .and_then(Value::as_str)
                .unwrap_or(&object.instance.type_name);
            let geometric_width = if object.category == "room" && object.valid {
                self.room_clear_width(object.instance.id)
            } else {
                None
            };
            let floor = object.instance.level.as_deref().unwrap_or(sheet);
            let mut classification = props
                .get("room_kind")
                .or_else(|| props.get("space_kind"))
                .or_else(|| props.get("kind").filter(|v| v.as_str() != Some("room")))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if classification.is_none() && object.category == "room" {
                classification = classifications.get(floor, name);
            }
            for (metric, unit, value) in [
                ("area_m2", "m2", object.facts.get("area").copied()),
                (
                    "width_m",
                    "m",
                    geometric_width.or_else(|| {
                        props
                            .get("width_m")
                            .or_else(|| props.get("width"))
                            .and_then(Value::as_f64)
                    }),
                ),
                (
                    "riser_m",
                    "m",
                    props
                        .get("riser_m")
                        .or_else(|| props.get("riser"))
                        .and_then(Value::as_f64),
                ),
                (
                    "going_m",
                    "m",
                    props
                        .get("going_m")
                        .or_else(|| props.get("going"))
                        .and_then(Value::as_f64),
                ),
                (
                    "headroom_m",
                    "m",
                    props.get("headroom_m").and_then(Value::as_f64),
                ),
                (
                    "passage_beside_m",
                    "m",
                    props.get("passage_beside_m").and_then(Value::as_f64),
                ),
                (
                    "door_distance_m",
                    "m",
                    props.get("door_distance_m").and_then(Value::as_f64),
                ),
                (
                    "steps_per_flight",
                    "pcs",
                    props.get("steps_per_flight").and_then(Value::as_f64),
                ),
                ("count", "pcs", Some(1.0)),
            ] {
                if let Some(measured) = value {
                    facts.push(Fact {
                        target: target.into(),
                        floor: floor.into(),
                        name: name.into(),
                        classification: classification.clone(),
                        metric: metric.into(),
                        unit: unit.into(),
                        measured,
                        handle: Some(object.instance.id),
                    });
                }
            }
        }
        self.check_facts(&facts)
    }
}
