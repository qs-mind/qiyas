//! Measurement methods are user data. This engine supplies arithmetic, provenance and grouping.
mod evaluate;
mod methods;
pub use methods::load_method_at;

use std::collections::BTreeSet;

use qiyas_param::expr::Expr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::knowledge::{self, Kind};
use crate::{CoreError, Session};

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}
fn one() -> String {
    "1".into()
}
fn version_one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    /// Document, page or research source supplied by the user or Mizan.
    pub source: String,
    pub clause: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageNote {
    pub text: String,
    pub citation: Citation,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Add,
    Deduct,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub label: String,
    pub operation: Operation,
    #[serde(default = "one")]
    pub timesing: String,
    /// Expressions in physical units. Multiplication is evaluated by qiyas-param.
    pub factors: Vec<String>,
    pub citation: Citation,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundaryRule {
    /// Expression determining whether this hosted void reaches its host's boundary.
    /// Names prefixed void. read the child; host dimensions are unprefixed.
    pub when: String,
    /// Separate deduction predicate for boundary voids, supplied by the method.
    pub deduct_when: String,
    pub citation: Citation,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Deduction {
    pub element_type: String,
    /// Interior opening/void threshold predicate, including equality policy.
    pub when: String,
    pub boundary: BoundaryRule,
    pub term: Term,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitBand {
    /// Width/depth (or other property) predicate. Overlapping bands are refused.
    pub when: String,
    pub unit: String,
    /// Complete dimension recipe for this unit, including unit-appropriate deductions.
    pub terms: Vec<Term>,
    #[serde(default)]
    pub deductions: Vec<Deduction>,
    pub citation: Citation,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoundMode {
    Nearest,
    Up,
    Down,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rounding {
    pub decimals: u32,
    pub mode: RoundMode,
    pub citation: Citation,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Confidence in the cited interpretation, from 0 to 1. Unscored legacy rules use 0.5.
    #[serde(default = "crate::trust::unscored")]
    pub confidence: f64,
    /// Cited by this rule: allowed quantity discrepancy in percent and/or physical units.
    #[serde(default)]
    pub tolerance: Option<crate::discrepancy::Tolerance>,
    pub id: String,
    /// Output quantity kind: count, length, area or volume. Omitted legacy rules use the unit.
    #[serde(default)]
    pub kind: Option<String>,
    /// geometric, or rule:<cited note name>. Declare the interpretation, not the BOQ code.
    #[serde(default)]
    pub basis: Option<String>,
    /// Exact type and/or category selection. No trade names are built into the engine.
    #[serde(default)]
    pub element_type: Option<String>,
    #[serde(default)]
    pub trade: Option<String>,
    pub unit: String,
    pub terms: Vec<Term>,
    #[serde(default)]
    pub deductions: Vec<Deduction>,
    #[serde(default)]
    pub bands: Vec<UnitBand>,
    #[serde(default)]
    pub rounding: Option<Rounding>,
    #[serde(default)]
    pub coverage: Vec<CoverageNote>,
    pub citation: Citation,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    #[serde(default = "version_one")]
    pub version: u32,
    pub name: String,
    pub title: String,
    pub based_on: String,
    pub source_note: String,
    pub rules: Vec<Rule>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub rule: String,
    /// Templates substitute {type}, {level}, {unit} or object property/quantity names.
    pub code: String,
    pub description_en: String,
    pub description_ar: String,
    pub section: String,
    pub trade: String,
    /// Must equal the measured unit, or use {unit} to follow method bands.
    pub unit: String,
    pub citation: Citation,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MethodSelection {
    pub settings: Option<RuleSet>,
    pub project: Option<RuleSet>,
}
impl MethodSelection {
    pub fn effective(&self) -> Option<&RuleSet> {
        self.project.as_ref().or(self.settings.as_ref())
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MethodScope {
    Settings,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MeasurementCommand {
    /// Apply a validated method snapshot. Null clears this scope; project takes precedence.
    /// Quantities, dimension sheets and BOQ re-derive in the same undo step.
    #[schemars(example = json!({"kind":"set_method","scope":"project","method":{"version":1,"name":"geometry","title":"Geometric quantities","based_on":"drawing","source_note":"No standard selected; visible source geometry only","rules":[{"id":"length","kind":"length","basis":"geometric","element_type":"Run","unit":"m","terms":[{"label":"Measured length","operation":"add","factors":["geometry.length"],"citation":{"source":"drawing","clause":"source path"}}],"citation":{"source":"drawing","clause":"source path"}}]}}))]
    #[schemars(extend("x-takeoff-step" = "4 apply selected method"))]
    #[schemars(example = json!({"kind":"set_method","scope":"project","method":{"version":1,"name":"observed","title":"Geometry","based_on":"drawing","source_note":"Visible geometry","rules":[{"id":"count","kind":"count","basis":"geometric","element_type":"Observed","unit":"pcs","terms":[{"label":"Count","operation":"add","factors":["geometry.count"],"citation":{"source":"drawing","clause":"insert"}}],"citation":{"source":"drawing","clause":"insert"}}]}}))]
    #[schemars(extend("x-takeoff-kit" = {"order":50,"example":1,"batch":40}))]
    SetMethod {
        scope: MethodScope,
        method: Option<Box<RuleSet>>,
    },
    /// Replace BOQ mappings. Descriptions and grouping are data; quantities are always derived.
    #[schemars(example = json!({"kind":"set_mappings","mappings":[{"rule":"length","code":"L-{id}","description_en":"Measured {type}","description_ar":"Measured {type}","section":"Drawing quantities","trade":"","unit":"m","citation":{"source":"drawing","clause":"source path"}}]}))]
    #[schemars(extend("x-takeoff-step" = "5 map recognised elements to BOQ"))]
    #[schemars(example = json!({"kind":"set_mappings","mappings":[{"rule":"count","code":"C","description_en":"Observed inserts","description_ar":"Observed inserts","section":"Drawing","trade":"","unit":"pcs","citation":{"source":"drawing","clause":"insert"}}]}))]
    #[schemars(extend("x-takeoff-kit" = {"order":60,"example":1,"batch":50}))]
    SetMappings { mappings: Vec<Mapping> },
    /// Read the applied methods, quantities, generated BOQ and dimension sheets.
    #[schemars(example = json!({"kind":"report"}))]
    #[schemars(extend("x-takeoff-step" = "6 check generated BOQ"))]
    #[schemars(extend("x-takeoff-kit" = {"order":90,"example":0,"batch":60}))]
    Report,
}
impl MeasurementCommand {
    pub fn is_read(&self) -> bool {
        matches!(self, Self::Report)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingReference {
    pub handle: u64,
    pub sheet: String,
    /// None means that the drawing has no assigned grid; it is explicitly reported for review.
    pub grid: Option<String>,
    pub evidence: Vec<crate::recognition::Evidence>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DimensionLine {
    pub label: String,
    pub operation: Operation,
    pub timesing: f64,
    pub factors: Vec<f64>,
    pub source_expressions: Vec<String>,
    /// Fully resolved arithmetic, independently evaluable without a drawing or lookup table.
    pub expression: String,
    pub value: f64,
    pub unit: String,
    pub method: String,
    pub citations: Vec<Citation>,
    pub sources: Vec<DrawingReference>,
    pub included: bool,
    pub decision: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasuredQuantity {
    #[serde(default = "crate::trust::unscored")]
    pub confidence: f64,
    pub object: u64,
    pub rule: String,
    pub method: String,
    pub citation: Citation,
    pub unit: String,
    pub quantity: f64,
    pub coverage: Vec<CoverageNote>,
    pub lines: Vec<DimensionLine>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedItem {
    #[serde(default = "crate::trust::unscored")]
    pub confidence: f64,
    #[serde(default)]
    pub tolerances: Vec<crate::discrepancy::Tolerance>,
    pub code: String,
    pub description_en: String,
    pub description_ar: String,
    pub section: String,
    pub trade: String,
    pub unit: String,
    pub quantity: f64,
    pub mapping_citations: Vec<Citation>,
    pub coverage: Vec<CoverageNote>,
    pub lines: Vec<DimensionLine>,
}
impl GeneratedItem {
    pub fn methods(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.method.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn rule_citations(&self) -> Vec<Citation> {
        let mut citations = Vec::new();
        for citation in self.lines.iter().flat_map(|line| &line.citations) {
            if !citations.contains(citation) {
                citations.push(citation.clone());
            }
        }
        citations
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MeasurementReport {
    #[serde(default)]
    pub confidence_summary: crate::trust::ConfidenceSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<crate::revisions::VariationReport>,
    pub selection: MethodSelection,
    pub quantities: Vec<MeasuredQuantity>,
    pub items: Vec<GeneratedItem>,
    pub warnings: Vec<String>,
}

fn text(value: &str) -> Result<(), CoreError> {
    if value.trim().is_empty() {
        return Err(invalid(
            "Measurement names, templates and citations must be nonempty",
        ));
    }
    Ok(())
}
fn citation(c: &Citation) -> Result<(), CoreError> {
    text(&c.source)?;
    text(&c.clause)
}
fn expression(s: &str) -> Result<(), CoreError> {
    Expr::parse(s)
        .map(|_| ())
        .map_err(|e| invalid(e.to_string()))
}
fn terms(values: &[Term]) -> Result<(), CoreError> {
    if values.is_empty() {
        return Err(invalid("A measurement recipe needs dimension terms"));
    }
    for term in values {
        text(&term.label)?;
        citation(&term.citation)?;
        expression(&term.timesing)?;
        if term.factors.is_empty() {
            return Err(invalid("A dimension line needs factors"));
        }
        for factor in &term.factors {
            expression(factor)?;
        }
    }
    Ok(())
}
fn deductions(values: &[Deduction]) -> Result<(), CoreError> {
    for d in values {
        text(&d.element_type)?;
        expression(&d.when)?;
        expression(&d.boundary.when)?;
        expression(&d.boundary.deduct_when)?;
        citation(&d.boundary.citation)?;
        terms(std::slice::from_ref(&d.term))?;
        if d.term.operation != Operation::Deduct {
            return Err(invalid("A void term must deduct"));
        }
    }
    Ok(())
}
impl RuleSet {
    /// A name selected by the QS. Mizan supplies the rules later; no standard is bundled.
    pub fn named(name: &str) -> Self {
        Self {
            version: 1,
            name: name.into(),
            title: name.into(),
            based_on: name.into(),
            source_note: String::new(),
            rules: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        if self.version != 1 {
            return Err(invalid(
                "Unsupported measurement rule set version; supported versions: 1",
            ));
        }
        text(&self.name)?;
        text(&self.title)?;
        let mut ids = BTreeSet::new();
        for r in &self.rules {
            if !r.confidence.is_finite() || !(0.0..=1.0).contains(&r.confidence) {
                return Err(invalid("Measurement confidence must be in 0..1"));
            }
            if let Some(t) = &r.tolerance {
                t.validate()?;
            }
            text(&r.id)?;
            text(&r.unit)?;
            if r.kind
                .as_deref()
                .is_some_and(|k| !matches!(k, "count" | "length" | "area" | "volume"))
            {
                return Err(invalid(
                    "Measurement kind must be count, length, area or volume",
                ));
            }
            if r.basis.as_deref().is_some_and(|b| {
                b != "geometric"
                    && !b
                        .strip_prefix("rule:")
                        .is_some_and(|n| !n.trim().is_empty())
            }) {
                return Err(invalid(
                    "Measurement basis must be geometric or rule:<cited note name>",
                ));
            }
            citation(&r.citation)?;
            if !ids.insert(&r.id) {
                return Err(invalid("Duplicate measurement rule id"));
            }
            if r.element_type.is_none() && r.trade.is_none() {
                return Err(invalid("A measurement rule needs a type or trade"));
            }
            for selector in [&r.element_type, &r.trade].into_iter().flatten() {
                text(selector)?;
            }
            terms(&r.terms)?;
            deductions(&r.deductions)?;
            for band in &r.bands {
                expression(&band.when)?;
                text(&band.unit)?;
                citation(&band.citation)?;
                terms(&band.terms)?;
                deductions(&band.deductions)?;
            }
            if let Some(round) = &r.rounding {
                if round.decimals > 12 {
                    return Err(invalid("Rounding precision must be at most 12 decimals"));
                }
                citation(&round.citation)?;
            }
            for note in &r.coverage {
                text(&note.text)?;
                citation(&note.citation)?;
            }
        }
        Ok(())
    }
}
impl Session {
    pub fn measurement_selection(&self) -> Result<MethodSelection, CoreError> {
        read(self, "selection")
    }
    pub fn measurement_report(&self) -> Result<MeasurementReport, CoreError> {
        let mut report: MeasurementReport = read(self, "result")?;
        report.variation = self.variation()?;
        let unavailable: Vec<_> = self
            .takeoff
            .items
            .iter()
            .filter(|m| !m.available())
            .collect();
        if !unavailable.is_empty() {
            report.warnings.push(format!(
                "{} unavailable quantities excluded from totals",
                unavailable.len()
            ));
            report.warnings.extend(
                unavailable
                    .iter()
                    .map(|m| format!("{}: {}", m.description, m.note)),
            );
        }
        report.items.sort_by(|a, b| {
            a.confidence
                .total_cmp(&b.confidence)
                .then(a.code.cmp(&b.code))
        });
        report.quantities.sort_by(|a, b| {
            a.confidence
                .total_cmp(&b.confidence)
                .then(a.object.cmp(&b.object))
        });
        report.confidence_summary =
            crate::trust::ConfidenceSummary::of(report.items.iter().map(|i| i.confidence));
        Ok(report)
    }

    /// Re-evaluates the saved method against current geometry without trusting cached results.
    pub fn remeasure_report(&self) -> Result<MeasurementReport, CoreError> {
        evaluate::derive(self)
    }
    pub(crate) fn run_measurement(
        &mut self,
        command: MeasurementCommand,
    ) -> Result<Value, CoreError> {
        match command {
            MeasurementCommand::Report => {
                return serde_json::to_value(self.measurement_report()?)
                    .map_err(|e| invalid(e.to_string()))
            }
            MeasurementCommand::SetMethod { scope, method } => {
                if let Some(method) = &method {
                    method.validate()?;
                }
                let mut selection = self.measurement_selection()?;
                let slot = match scope {
                    MethodScope::Settings => &mut selection.settings,
                    MethodScope::Project => &mut selection.project,
                };
                *slot = method.map(|m| *m);
                put(self, "selection", &selection)?;
            }
            MeasurementCommand::SetMappings { mappings } => {
                let mut rules = BTreeSet::new();
                for m in &mappings {
                    for s in [
                        &m.rule,
                        &m.code,
                        &m.description_en,
                        &m.description_ar,
                        &m.unit,
                    ] {
                        text(s)?;
                    }
                    citation(&m.citation)?;
                    if !rules.insert(&m.rule) {
                        return Err(invalid("Use one BOQ mapping per measurement rule"));
                    }
                }
                put(self, "mappings", &mappings)?;
            }
        }
        Ok(json!({"changed":true}))
    }
    pub(crate) fn refresh_measurement(&mut self) -> Result<(), CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("measurement.refresh_measurement");
        if knowledge::get(&self.drawing.doc, Kind::Measurement, "selection")?.is_none() {
            return Ok(());
        }
        let report = evaluate::derive(self)?;
        put(self, "result", &report)
    }
}
fn read<T: serde::de::DeserializeOwned + Default>(s: &Session, name: &str) -> Result<T, CoreError> {
    knowledge::get(&s.drawing.doc, Kind::Measurement, name)?
        .map(|e| serde_json::from_value(e.data).map_err(|e| invalid(e.to_string())))
        .unwrap_or_else(|| Ok(T::default()))
}
fn put(s: &mut Session, name: &str, value: &impl Serialize) -> Result<(), CoreError> {
    let value = serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?;
    if knowledge::get(&s.drawing.doc, Kind::Measurement, name)?.is_none_or(|e| e.data != value) {
        knowledge::put(&mut s.drawing.doc, Kind::Measurement, name, &value)?;
    }
    Ok(())
}

impl MeasurementReport {
    /// Adapter for the existing comparison engine. It does not edit the manual take-off.
    pub fn comparison_measurements(&self, units: crate::Units) -> Vec<crate::Measurement> {
        self.items
            .iter()
            .map(|item| {
                let mut m = crate::Measurement::new(
                    crate::Kind::Item,
                    item.quantity,
                    units,
                    item.description_en.clone(),
                    String::new(),
                    String::new(),
                );
                m.confidence = item.confidence;
                m.id = format!("generated:{}", item.code);
                m.created.clear();
                m.unit = item.unit.clone();
                m.shown = item.quantity;
                m.boq_code = Some(item.code.clone());
                m.source = "derived".into();
                m.handles = item
                    .lines
                    .iter()
                    .flat_map(|l| l.sources.iter().map(|s| s.handle))
                    .collect();
                m.handles.sort_unstable();
                m.handles.dedup();
                m.sheet = item
                    .lines
                    .first()
                    .and_then(|l| l.sources.first())
                    .map(|s| s.sheet.clone())
                    .unwrap_or_default();
                m.note = item
                    .lines
                    .iter()
                    .map(|l| {
                        format!(
                            "{}: {}",
                            l.method,
                            l.citations
                                .iter()
                                .map(|c| c.clause.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join("; ");
                m
            })
            .collect()
    }
}
