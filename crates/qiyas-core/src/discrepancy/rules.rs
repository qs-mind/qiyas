//! Cited data selects element sets and table columns; the engine supplies only joins and arithmetic.
use super::{finding, invalid, Conflict, ConflictKind, Query, Reference, Severity};
use crate::{
    knowledge,
    measurement::Citation,
    objects::{ObjectFilter, ObjectRecord},
    CoreError, Session,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckRule {
    pub citation: Citation,
    #[serde(default)]
    pub severity: Severity,
    pub question_en: String,
    pub question_ar: String,
    pub test: Test,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Set {
    #[serde(default)]
    pub filter: ObjectFilter,
    /// Exact property values. The engine does not interpret trade names.
    #[serde(default)]
    pub properties: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TableSource {
    Drawing {
        handle: u64,
        sheet: String,
    },
    /// A supplied document's extracted rows in knowledge tables. Data must include rows and citation.
    Document {
        name: String,
    },
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    #[serde(default)]
    pub percent: f64,
    #[serde(default)]
    pub absolute: f64,
}
impl Tolerance {
    pub fn validate(&self) -> Result<(), CoreError> {
        if [self.percent, self.absolute]
            .iter()
            .any(|n| !n.is_finite() || *n < 0.)
        {
            return Err(invalid("Check tolerances must be finite and nonnegative"));
        }
        Ok(())
    }
    pub fn differs(&self, a: f64, b: f64) -> bool {
        (a - b).abs() > self.absolute.max(a.abs() * self.percent / 100.) + 1e-9
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Test {
    /// Compare each table row with the selected instances sharing its key.
    Schedule {
        table: TableSource,
        key_column: usize,
        value_column: usize,
        elements: Set,
        property: String,
        /// Empty means count; otherwise sum a property, quantity or geometry fact.
        #[serde(default)]
        measure: Option<String>,
        /// Compare a size on each matching instance instead of summing the set.
        #[serde(default)]
        each: bool,
        /// Conversion from table units into the physical units of the instances.
        #[serde(default = "one")]
        factor: f64,
        #[serde(default)]
        tolerance: Tolerance,
        #[serde(default)]
        unit: String,
    },
    /// For each left element, join its host or a property to right element ids/properties.
    Relation {
        left: Set,
        right: Set,
        /// "host" or a left property name.
        join: String,
        /// Empty means right element id; otherwise a right property name.
        #[serde(default)]
        right_property: Option<String>,
        minimum: usize,
        #[serde(default)]
        maximum: Option<usize>,
    },
    /// Every selected element's property must appear in the supplied table column.
    Document {
        table: TableSource,
        key_column: usize,
        elements: Set,
        property: String,
    },
    /// A general quantity relation between two element sets: left sum = right sum * factor.
    QuantityRelation {
        left: Set,
        right: Set,
        left_measure: String,
        right_measure: String,
        factor: f64,
        #[serde(default)]
        tolerance: Tolerance,
        #[serde(default)]
        unit: String,
    },
}
fn one() -> f64 {
    1.
}
impl CheckRule {
    pub fn validate(&self) -> Result<(), CoreError> {
        for s in [
            &self.citation.source,
            &self.citation.clause,
            &self.question_en,
            &self.question_ar,
        ] {
            if s.trim().is_empty() {
                return Err(invalid(
                    "Checks need a source, clause and questions in both languages",
                ));
            }
        }
        match &self.test {
            Test::Schedule {
                factor,
                tolerance,
                property,
                ..
            } => {
                tolerance.validate()?;
                if !factor.is_finite() || *factor <= 0. || property.is_empty() {
                    return Err(invalid("Invalid schedule factor or key property"));
                }
            }
            Test::QuantityRelation {
                factor, tolerance, ..
            } => {
                tolerance.validate()?;
                if !factor.is_finite() {
                    return Err(invalid("Invalid relation factor"));
                }
            }
            Test::Relation {
                join,
                minimum,
                maximum,
                ..
            } => {
                if join.is_empty() || maximum.is_some_and(|m| m < *minimum) {
                    return Err(invalid("Invalid relation bounds or join"));
                }
            }
            Test::Document { property, .. } => {
                if property.is_empty() {
                    return Err(invalid("Document check needs a key property"));
                }
            }
        }
        Ok(())
    }
}
fn key(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn property(o: &ObjectRecord, name: &str) -> Option<String> {
    match name {
        "type" => Some(o.instance.type_name.clone()),
        "id" => Some(o.instance.id.to_string()),
        "host" => o.instance.host.map(|h| h.to_string()),
        _ => o.instance.props.get(name).map(key),
    }
}
fn number(o: &ObjectRecord, name: &str) -> Result<f64, CoreError> {
    let n = if name == "count" {
        Some(1.)
    } else {
        o.quantities
            .get(name)
            .or_else(|| o.facts.get(name))
            .copied()
            .or_else(|| o.instance.props.get(name).and_then(Value::as_f64))
    };
    n.filter(|n| n.is_finite()).ok_or_else(|| {
        invalid(format!(
            "Missing numeric property {name} on {:X}",
            o.instance.id
        ))
    })
}
fn elements(s: &Session, set: &Set) -> Result<Vec<ObjectRecord>, CoreError> {
    let rows = s.objects(&set.filter)?;
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|o| {
            set.properties
                .iter()
                .all(|(k, v)| o.instance.props.get(k) == Some(v))
        })
        .collect();
    let evidence = s.recognition_evidence()?;
    if rows.iter().any(|o| {
        evidence.iter().any(|e| {
            e.object == Some(o.instance.id) && e.state != crate::recognition::ReviewState::Accepted
        })
    }) {
        return Err(invalid("A check selected unresolved recognition evidence"));
    }
    if rows.iter().any(|o| !o.valid) {
        return Err(invalid(
            "A check selected invalid elements; repair their evidence before checking",
        ));
    }
    Ok(rows)
}
fn reference(o: &ObjectRecord) -> Reference {
    Reference {
        handle: o.instance.id,
        sheet: o.sheet.clone(),
        grid: o
            .instance
            .props
            .get("grid")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}
type ReadTable = (Vec<Vec<String>>, Vec<Reference>, Citation);
fn table(s: &Session, source: &TableSource) -> Result<ReadTable, CoreError> {
    match source {
        TableSource::Drawing { handle, sheet } => {
            let t = s
                .read_tables(Some(sheet))
                .into_iter()
                .find(|t| t.source == *handle)
                .ok_or_else(|| invalid("Check table not found on its cited sheet"))?;
            Ok((
                t.rows,
                vec![Reference {
                    handle: *handle,
                    sheet: sheet.clone(),
                    grid: None,
                }],
                Citation {
                    source: format!("Drawing {sheet} handle {handle:X}"),
                    clause: "Table rows".into(),
                },
            ))
        }
        TableSource::Document { name } => {
            #[derive(Deserialize)]
            struct Data {
                rows: Vec<Vec<String>>,
                citation: Citation,
            }
            let entry = knowledge::get(&s.drawing.doc, knowledge::Kind::Tables, name)?
                .ok_or_else(|| invalid(format!("Missing supplied table {name}")))?;
            let data: Data =
                serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string()))?;
            if data.citation.source.trim().is_empty() || data.citation.clause.trim().is_empty() {
                return Err(invalid("Document table needs its source and clause/page"));
            }
            Ok((data.rows, vec![], data.citation))
        }
    }
}
fn cell(row: &[String], column: usize) -> Result<&str, CoreError> {
    row.get(column)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("Check table has a missing mapped cell"))
}

pub fn evaluate(s: &Session) -> Result<Vec<Conflict>, CoreError> {
    let mut out = vec![];
    for entry in knowledge::list(&s.drawing.doc, knowledge::Kind::Checks)? {
        let rule: CheckRule =
            serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string()))?;
        rule.validate()?;
        let mut emit = |kind,
                        subject: String,
                        expected,
                        found,
                        unit: String,
                        refs: Vec<Reference>,
                        citation: Option<Citation>| {
            let mut f = finding(
                kind,
                &format!("check:{}:{subject}", entry.name),
                format!("{}: {subject}", entry.name),
                format!("{}: {subject}", entry.name),
            );
            f.expected = expected;
            f.found = found;
            f.unit = unit;
            f.sheets = refs.iter().map(|r| r.sheet.clone()).collect();
            f.sheets.sort();
            f.sheets.dedup();
            f.query = Some(Query {
                severity: rule.severity,
                question_en: rule.question_en.clone(),
                question_ar: rule.question_ar.clone(),
                references: refs,
                citations: std::iter::once(rule.citation.clone())
                    .chain(citation)
                    .collect(),
                rule: Some(entry.name.clone()),
                ..Default::default()
            });
            out.push(f);
        };
        match rule.test {
            Test::Schedule {
                table: source,
                key_column,
                value_column,
                elements: set,
                property: prop,
                measure,
                each,
                factor,
                tolerance,
                unit,
            } => {
                let (rows, refs, citation) = table(s, &source)?;
                let objects = elements(s, &set)?;
                for (i, row) in rows.iter().enumerate() {
                    let k = cell(row, key_column)?;
                    let expected = cell(row, value_column)?
                        .parse::<f64>()
                        .map_err(|_| invalid("Schedule value is not a number"))?
                        * factor;
                    if !expected.is_finite() {
                        return Err(invalid("Schedule value is not finite"));
                    }
                    let matched: Vec<_> = objects
                        .iter()
                        .filter(|o| property(o, &prop).as_deref() == Some(k))
                        .collect();
                    if each && !matched.is_empty() {
                        for object in &matched {
                            let found = number(object, measure.as_deref().unwrap_or("count"))?;
                            if tolerance.differs(expected, found) {
                                let mut refs = refs.clone();
                                refs.push(reference(object));
                                emit(
                                    ConflictKind::ScheduleVsPlan,
                                    format!("row {i} ({k}) instance {:X}", object.instance.id),
                                    Some(expected),
                                    Some(found),
                                    unit.clone(),
                                    refs,
                                    Some(citation.clone()),
                                );
                            }
                        }
                        continue;
                    }
                    let found = matched
                        .iter()
                        .map(|o| number(o, measure.as_deref().unwrap_or("count")))
                        .collect::<Result<Vec<_>, _>>()?
                        .iter()
                        .sum::<f64>();
                    if tolerance.differs(expected, found) {
                        let mut refs = refs.clone();
                        refs.extend(matched.iter().map(|o| reference(o)));
                        emit(
                            ConflictKind::ScheduleVsPlan,
                            format!("row {i} ({k})"),
                            Some(expected),
                            Some(found),
                            unit.clone(),
                            refs,
                            Some(citation.clone()),
                        );
                    }
                }
            }
            Test::Relation {
                left,
                right,
                join,
                right_property,
                minimum,
                maximum,
            } => {
                let right = elements(s, &right)?;
                for o in elements(s, &left)? {
                    let k = property(&o, &join);
                    let related: Vec<_> = right
                        .iter()
                        .filter(|r| {
                            k.is_some()
                                && property(r, right_property.as_deref().unwrap_or("id")) == k
                        })
                        .collect();
                    let n = related.len();
                    if n < minimum || maximum.is_some_and(|m| n > m) {
                        let mut refs = vec![reference(&o)];
                        refs.extend(related.iter().map(|r| reference(r)));
                        emit(
                            ConflictKind::RelationMismatch,
                            format!("{:X}", o.instance.id),
                            Some(minimum as f64),
                            Some(n as f64),
                            "pcs".into(),
                            refs,
                            None,
                        );
                    }
                }
            }
            Test::Document {
                table: source,
                key_column,
                elements: set,
                property: prop,
            } => {
                let (rows, refs, citation) = table(s, &source)?;
                let keys = rows
                    .iter()
                    .map(|r| cell(r, key_column))
                    .collect::<Result<Vec<_>, _>>()?;
                for o in elements(s, &set)? {
                    let k = property(&o, &prop)
                        .ok_or_else(|| invalid(format!("Missing check property {prop}")))?;
                    if !keys.contains(&k.as_str()) {
                        let mut refs = refs.clone();
                        refs.push(reference(&o));
                        emit(
                            ConflictKind::DocumentMismatch,
                            format!("{:X} ({k})", o.instance.id),
                            None,
                            None,
                            String::new(),
                            refs,
                            Some(citation.clone()),
                        );
                    }
                }
            }
            Test::QuantityRelation {
                left,
                right,
                left_measure,
                right_measure,
                factor,
                tolerance,
                unit,
            } => {
                let left = elements(s, &left)?;
                let right = elements(s, &right)?;
                let sum = |objects: &[ObjectRecord], name: &str| -> Result<f64, CoreError> {
                    Ok(objects
                        .iter()
                        .map(|o| number(o, name))
                        .collect::<Result<Vec<_>, _>>()?
                        .iter()
                        .sum())
                };
                let expected = sum(&right, &right_measure)? * factor;
                let found = sum(&left, &left_measure)?;
                if tolerance.differs(expected, found) {
                    emit(
                        ConflictKind::RelationMismatch,
                        "quantity relation".into(),
                        Some(expected),
                        Some(found),
                        unit,
                        left.iter().chain(&right).map(reference).collect(),
                        None,
                    );
                }
            }
        }
    }
    Ok(out)
}
