use std::collections::BTreeMap;

use opencadcodec::Handle;
use qiyas_param::expr::{Resolver, Value as EValue};

use super::*;
use crate::objects::{ObjectFilter, ObjectRecord};

struct Context<'a> {
    session: &'a Session,
    values: BTreeMap<String, EValue>,
    /// Where each named value came from, for the working an engineer reads.
    sources: BTreeMap<String, PropSource>,
    /// Display units of named values ("m", "m2"...), where known.
    units: BTreeMap<String, String>,
    dependencies: BTreeMap<String, Vec<NamedSource>>,
}
impl<'a> Context<'a> {
    fn new(session: &'a Session) -> Self {
        Self {
            session,
            values: BTreeMap::new(),
            sources: BTreeMap::new(),
            units: BTreeMap::new(),
            dependencies: BTreeMap::new(),
        }
    }
}
/// The unit a geometry fact is in, by the names the engine gives them.
fn fact_unit(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if n.ends_with("volume") {
        "m3"
    } else if n.ends_with("area") {
        "m2"
    } else if n == "count" {
        "nr"
    } else if n.ends_with("length") || n.ends_with("perimeter") {
        "m"
    } else {
        ""
    }
}
fn kind_unit(kind: crate::objects::UnitKind) -> &'static str {
    use crate::objects::UnitKind;
    match kind {
        UnitKind::Length => "m",
        UnitKind::Area => "m2",
        UnitKind::Volume => "m3",
        UnitKind::Angle => "deg",
        UnitKind::Count => "nr",
        UnitKind::Scalar => "",
    }
}
fn typed(value: &Value) -> Option<EValue> {
    match value {
        Value::String(s) => Some(EValue::Text(s.clone())),
        Value::Number(n) => n.as_f64().map(EValue::Number),
        Value::Bool(b) => Some(EValue::Bool(*b)),
        _ => None,
    }
}
impl Resolver for Context<'_> {
    fn resolve(&self, name: &str) -> Option<EValue> {
        self.values.get(name).cloned()
    }
    fn lookup(&self, table: &str, key: &EValue, column: &str) -> Option<EValue> {
        let key = match key {
            EValue::Text(s) => s.clone(),
            EValue::Number(n) => n.to_string(),
            EValue::Bool(b) => b.to_string(),
        };
        let data = knowledge::get(&self.session.drawing.doc, Kind::Tables, table)
            .ok()??
            .data;
        typed(data.get(&key)?.get(column)?)
    }
}
impl Context<'_> {
    fn value(&self, expression: &str) -> Result<EValue, CoreError> {
        self.session
            .evaluation_expression(expression)
            .map_err(|e| invalid(format!("Measurement expression {expression}: {e}")))?
            .eval_with(self)
            .map_err(|e| invalid(format!("Measurement expression {expression}: {e}")))
    }
    fn number(&self, expression: &str) -> Result<f64, CoreError> {
        self.value(expression)?
            .as_number()
            .map_err(|e| invalid(e.to_string()))
    }
    fn condition(&self, expression: &str) -> Result<bool, CoreError> {
        self.value(expression)?
            .as_bool()
            .map_err(|e| invalid(e.to_string()))
    }
    fn named_sources(&self, name: &str, seen: &mut Vec<String>) -> Vec<NamedSource> {
        if seen.iter().any(|n| n == name) {
            return Vec::new();
        }
        seen.push(name.into());
        let mut out = self.dependencies.get(name).cloned().unwrap_or_default();
        if let Some(source) = self.sources.get(name) {
            out.push(NamedSource {
                name: name.into(),
                source: source.clone(),
            });
            if source.kind == SourceKind::Derived {
                if let Ok(expression) = self.session.evaluation_expression(&source.text) {
                    for dependency in expression.names() {
                        let dependency = if name.starts_with("void.") {
                            format!("void.{dependency}")
                        } else if name.starts_with("host.") {
                            format!("host.{dependency}")
                        } else {
                            dependency
                        };
                        out.extend(self.named_sources(&dependency, seen));
                    }
                }
            }
        }
        out
    }

    fn expression_sources(&self, expression: &str) -> Vec<NamedSource> {
        self.session
            .evaluation_expression(expression)
            .map(|expression| {
                expression
                    .names()
                    .iter()
                    .flat_map(|name| self.named_sources(name, &mut Vec::new()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn add_object(&mut self, object: &ObjectRecord, prefix: &str) {
        let definition = self.session.object_type(&object.instance.type_name).ok();
        for (k, v) in &object.instance.props {
            if let Some(v) = typed(v) {
                self.values.insert(format!("{prefix}{k}"), v);
            }
            let bound = definition.as_ref().and_then(|d| d.property_bindings.get(k));
            let source = match (object.instance.sources.get(k), bound) {
                (_, Some(binding)) => PropSource {
                    decision: None,
                    kind: SourceKind::Derived,
                    handle: None,
                    text: binding.clone(),
                },
                (Some(source), None) => source.clone(),
                (None, None) => PropSource {
                    decision: None,
                    kind: SourceKind::Unknown,
                    handle: Some(object.instance.id),
                    ..Default::default()
                },
            };
            let mut source = source;
            source.handle.get_or_insert(object.instance.id);
            self.sources.insert(format!("{prefix}{k}"), source);
            if let Some(crate::objects::Property::Number { unit, .. }) =
                definition.as_ref().and_then(|d| d.properties.get(k))
            {
                self.units
                    .insert(format!("{prefix}{k}"), kind_unit(*unit).into());
            }
        }
        // A measured figure names what it was measured with, so a volume of a joined wall reads as
        // its length, plan area and the element's own heights and thicknesses, not a bare number.
        let mut basis = object
            .facts
            .iter()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "length" | "area" | "perimeter" | "volume" | "count"
                )
            })
            .map(|(k, v)| format!("{k} {} {}", round3(*v), fact_unit(k)))
            .collect::<Vec<_>>();
        for (k, v) in &object.instance.props {
            if let (Some(n), Some(unit)) = (v.as_f64(), self.units.get(&format!("{prefix}{k}"))) {
                basis.push(format!("{k} {} {unit}", round3(n)));
            }
        }
        let measured = PropSource {
            decision: None,
            kind: SourceKind::Measured,
            handle: Some(object.instance.id),
            text: basis.join(", "),
        };
        for fact in object.facts.keys() {
            let mut expressions = Vec::new();
            if let Some(d) = &definition {
                if fact.contains("volume") || fact.contains("face_area") || fact == "height" {
                    expressions.extend(d.vertical.height.iter());
                    expressions.extend(d.vertical.base.iter());
                }
                if fact.contains("area")
                    || fact.contains("volume")
                    || fact.contains("face")
                    || fact == "band_width"
                {
                    expressions.extend(d.plan.band_faces.iter());
                }
            }
            let mut dependencies = Vec::new();
            for expression in expressions {
                if let Ok(expr) = self.session.evaluation_expression(expression) {
                    for name in expr.names() {
                        dependencies.extend(
                            self.named_sources(&format!("{prefix}{name}"), &mut Vec::new()),
                        );
                    }
                }
            }
            for name in [
                format!("{prefix}{fact}"),
                format!("{prefix}geometry.{fact}"),
            ] {
                self.dependencies.insert(name, dependencies.clone());
            }
        }
        for (k, v) in &object.facts {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Number(*v));
            self.values
                .insert(format!("{prefix}geometry.{k}"), EValue::Number(*v));
            for name in [format!("{prefix}{k}"), format!("{prefix}geometry.{k}")] {
                self.sources.insert(name.clone(), measured.clone());
                self.units.insert(name, fact_unit(k).into());
            }
        }
        for (k, v) in &object.quantities {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Number(*v));
            let recipe = definition
                .as_ref()
                .and_then(|d| d.quantities.get(k))
                .cloned()
                .unwrap_or_default();
            self.sources.insert(
                format!("{prefix}{k}"),
                PropSource {
                    decision: None,
                    kind: SourceKind::Derived,
                    handle: Some(object.instance.id),
                    text: recipe,
                },
            );
            self.units
                .insert(format!("{prefix}{k}"), fact_unit(k).into());
        }
        self.values
            .insert(format!("{prefix}id"), EValue::Text(object.identity.clone()));
        let type_en = crate::objects::public_type_name(&object.instance.type_name, false);
        let type_ar = crate::objects::public_type_name(&object.instance.type_name, true);
        for (k, v) in [
            ("type", type_en.as_str()),
            ("type_ar", type_ar.as_str()),
            ("trade", object.category.as_str()),
            ("level", object.instance.level.as_deref().unwrap_or("")),
        ] {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Text(v.into()));
        }
    }
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}
fn reference(
    s: &Session,
    object: &ObjectRecord,
    evidence: &[crate::recognition::Candidate],
) -> DrawingReference {
    let mut sources = evidence
        .iter()
        .filter(|c| c.object == Some(object.instance.id))
        .flat_map(|c| c.evidence.clone())
        .collect::<Vec<_>>();
    if let Some(entity) = s.drawing.doc.get_entity(Handle::new(object.instance.id)) {
        if let Some(source) = crate::xdata::read_json::<crate::recognition::Evidence>(
            entity.common(),
            crate::recognition::SOURCE_APP,
        ) {
            sources.push(source);
        }
    }
    DrawingReference {
        handle: object.instance.id,
        sheet: object.sheet.clone(),
        grid: object
            .instance
            .props
            .get("grid")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_owned),
        evidence: sources,
    }
}
/// An error about one object says which.
fn on_object(error: CoreError, id: u64) -> CoreError {
    match error {
        CoreError::InvalidCommand(message) => invalid(format!("Object {id}: {message}")),
        other => other,
    }
}
/// The named values of a term, each with its source, in the order they multiply.
fn operands(context: &Context<'_>, term: &Term) -> Result<Vec<Operand>, CoreError> {
    let mut result = Vec::new();
    let timesing = (term.timesing.trim() != "1").then(|| ("Times".to_string(), &term.timesing));
    let factors = term.factors.iter().enumerate().map(|(i, expression)| {
        let label = term
            .factor_labels
            .get(i)
            .filter(|l| !l.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| expression.clone());
        (label, expression)
    });
    for (label, expression) in timesing.into_iter().chain(factors) {
        let value = context.number(expression)?;
        let names = context
            .session
            .evaluation_expression(expression)
            .map(|e| e.names())
            .unwrap_or_default();
        // A value looked up in a table the drawing knows (a schedule or document table).
        let tables: Vec<NamedSource> = expression
            .match_indices("lookup(")
            .filter_map(|(at, _)| {
                let rest = expression[at + 7..].trim_start();
                let rest = rest.strip_prefix('"')?;
                let name = &rest[..rest.find('"')?];
                let citation = knowledge::get(&context.session.drawing.doc, Kind::Tables, name)
                    .ok()
                    .flatten()
                    .and_then(|k| k.data.get("citation").cloned())
                    .map(|c| match c {
                        Value::String(s) => s,
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                Some(NamedSource {
                    name: name.to_string(),
                    source: PropSource {
                        decision: None,
                        kind: SourceKind::DrawingTable,
                        handle: None,
                        text: citation,
                    },
                })
            })
            .collect();
        let mut sources: Vec<NamedSource> = if !tables.is_empty() {
            tables
                .into_iter()
                .chain(names.iter().filter_map(|name| {
                    context.sources.get(name).map(|source| NamedSource {
                        name: name.clone(),
                        source: source.clone(),
                    })
                }))
                .collect()
        } else if names.is_empty() {
            // A number written in the rule: the rule's citation is its source.
            vec![NamedSource {
                name: expression.clone(),
                source: PropSource {
                    decision: None,
                    kind: SourceKind::Rule,
                    handle: None,
                    text: format!("{} {}", term.citation.source, term.citation.clause)
                        .trim()
                        .to_string(),
                },
            }]
        } else {
            names
                .iter()
                .map(|name| NamedSource {
                    name: name.clone(),
                    source: context.sources.get(name).cloned().unwrap_or_default(),
                })
                .collect()
        };
        for name in &names {
            for dependency in context.named_sources(name, &mut Vec::new()) {
                if !sources.contains(&dependency) {
                    sources.push(dependency);
                }
            }
        }
        let unit = match names.as_slice() {
            [one] if expression.trim() == one => {
                context.units.get(one).cloned().unwrap_or_default()
            }
            _ => String::new(),
        };
        result.push(Operand {
            label,
            expression: expression.clone(),
            value,
            unit,
            sources,
        });
    }
    Ok(result)
}
fn line(
    context: &Context<'_>,
    term: &Term,
    unit: &str,
    method: &RuleSet,
    citations: &[Citation],
    sources: Vec<DrawingReference>,
) -> Result<DimensionLine, CoreError> {
    let timesing = context.number(&term.timesing)?;
    let factors = term
        .factors
        .iter()
        .map(|s| context.number(s))
        .collect::<Result<Vec<_>, _>>()?;
    if timesing < 0.0 || factors.iter().any(|v| *v < 0.0) {
        let negative: Vec<String> = std::iter::once((&term.timesing, timesing))
            .chain(term.factors.iter().zip(factors.iter().copied()))
            .filter(|(_, value)| *value < 0.0)
            .map(|(expression, value)| format!("`{expression}` = {value}"))
            .collect();
        return Err(invalid(format!(
            "Dimension operands must be nonnegative; use a deduct operation to subtract. Term \"{}\" has {}. A negative value usually means one property is smaller than another (a head below its sill): check the object's properties, or write max(expression, 0)",
            term.label,
            negative.join(" and ")
        )));
    }
    let magnitude = std::iter::once(timesing)
        .chain(factors.iter().copied())
        .map(|v| format!("({v})"))
        .collect::<Vec<_>>()
        .join(" * ");
    let expression = format!(
        "{}{magnitude}",
        if term.operation == Operation::Deduct {
            "-"
        } else {
            ""
        }
    );
    let value = context.number(&expression)?;
    let mut refs = citations.to_vec();
    refs.push(term.citation.clone());
    Ok(DimensionLine {
        id: String::new(),
        operands: operands(context, term)?,
        label: term.label.clone(),
        operation: term.operation,
        timesing,
        factors,
        source_expressions: std::iter::once(term.timesing.clone())
            .chain(term.factors.clone())
            .collect(),
        expression,
        value,
        unit: unit.into(),
        method: super::canonical_method_name(&method.name),
        citations: refs,
        sources,
        included: true,
        decision: None,
    })
}
fn total(context: &Context<'_>, lines: &[DimensionLine]) -> Result<f64, CoreError> {
    let expression = lines
        .iter()
        .filter(|l| l.included)
        .map(|l| format!("({})", l.value))
        .collect::<Vec<_>>()
        .join(" + ");
    if expression.is_empty() {
        Ok(0.0)
    } else {
        context.number(&expression)
    }
}
fn template(template: &str, context: &Context<'_>) -> Result<String, CoreError> {
    let mut result = String::new();
    let mut rest = template;
    while let Some((before, after)) = rest.split_once('{') {
        result.push_str(before);
        let (name, tail) = after
            .split_once('}')
            .ok_or_else(|| invalid("Unclosed BOQ template placeholder"))?;
        let value = context
            .resolve(name)
            .ok_or_else(|| invalid(format!("Unknown BOQ template field {name}")))?;
        result.push_str(&match value {
            EValue::Text(s) => s,
            EValue::Number(n) => n.to_string(),
            EValue::Bool(b) => b.to_string(),
        });
        rest = tail;
    }
    result.push_str(rest);
    Ok(result)
}

/// Placeholders that already make a BOQ line per size or mark.
const SIZE_PLACEHOLDERS: [&str; 7] = [
    "{mark}", "{size}", "{width}", "{height}", "{head}", "{sill}", "{depth}",
];

/// What tells a door or window's size apart from another's in the same BOQ line, always from
/// its width and height (never from a mark, which depends on whether a schedule or a tag was made
/// first): `(code part, English description part, Arabic description part)`.
fn size_label(object: &ObjectRecord, unit: &str) -> Option<(String, String, String)> {
    // Doors and windows only: any other counted object keeps the mapping's own code, which a
    // client BOQ is matched by.
    if !object.category.eq_ignore_ascii_case("opening") || !matches!(unit, "pcs" | "nr" | "count") {
        return None;
    }
    let number = |name: &str| object.instance.props.get(name).and_then(Value::as_f64);
    let mm = |metres: f64| (metres * 1000.0).round() as i64;
    let width = number("width").filter(|w| *w > 0.0)?;
    let height = number("head")
        .map(|head| head - number("sill").unwrap_or(0.0))
        .filter(|h| *h > 0.0);
    Some(match height {
        Some(h) => (
            format!("{}x{}", mm(width), mm(h)),
            format!("{} × {} mm", mm(width), mm(h)),
            crate::numbers::arabic_numerals(&format!("{} × {} مم", mm(width), mm(h))),
        ),
        None => (
            format!("{}", mm(width)),
            format!("{} mm", mm(width)),
            crate::numbers::arabic_numerals(&format!("{} مم", mm(width))),
        ),
    })
}

pub(super) fn derive(s: &Session) -> Result<MeasurementReport, CoreError> {
    #[cfg(any(test, feature = "fixtures"))]
    let _profile = crate::fixtures::profile::span("measurement.evaluate.derive");
    let _read = s.evaluation_read()?;
    let selection = s.measurement_selection()?;
    let mut report = MeasurementReport {
        selection: selection.clone(),
        ..Default::default()
    };
    let Some(method) = selection.effective() else {
        report.warnings.push("No measurement method selected; existing geometric quantities have no standard applied".into());
        return Ok(report);
    };
    method.validate()?;
    let mappings: Vec<Mapping> = read(s, "mappings")?;
    // A name-only method has no numeric expressions to evaluate. Keep source/validity
    // warnings, but do not solve every assembly's quantities merely to report missing rules.
    let exclusions = s.measurement_exclusions()?;
    let objects: Vec<_> = s
        .object_records(&ObjectFilter::default(), !method.rules.is_empty())?
        .into_iter()
        .filter(|o| !exclusions.entity(s, o.instance.id))
        .collect();
    let evidence = s.recognition_evidence()?;
    type ItemKey = (String, String, String, String, String, String);
    let mut items = BTreeMap::<ItemKey, GeneratedItem>::new();
    for object in &objects {
        if !object.valid
            || evidence.iter().any(|c| {
                c.object == Some(object.instance.id)
                    && c.state != crate::recognition::ReviewState::Accepted
            })
        {
            report.warnings.push(format!(
                "Object {} is invalid or awaiting recognition review; no quantity generated",
                object.instance.id
            ));
            continue;
        }
        if objects.iter().any(|v| {
            v.instance.host == Some(object.instance.id)
                && (!v.valid
                    || evidence.iter().any(|c| {
                        c.object == Some(v.instance.id)
                            && c.state != crate::recognition::ReviewState::Accepted
                    }))
        }) {
            report.warnings.push(format!(
                "Object {} has an unresolved void; its quantity is withheld",
                object.instance.id
            ));
            continue;
        }
        let mut context = Context::new(s);
        if let Some(host) = objects
            .iter()
            .find(|o| Some(o.instance.id) == object.instance.host)
        {
            context.add_object(host, "host.");
        }
        context.add_object(object, "");
        let mut cut_sources = Vec::new();
        for child in objects
            .iter()
            .filter(|v| v.instance.host == Some(object.instance.id))
        {
            let Ok(definition) = s.object_type(&child.instance.type_name) else {
                continue;
            };
            let Some(cut) = definition.host_cut.as_ref().filter(|cut| cut.cuts_host) else {
                continue;
            };
            let mut child_context = Context::new(s);
            child_context.add_object(object, "host.");
            child_context.add_object(child, "");
            for expression in std::iter::once(&cut.width)
                .chain(definition.vertical.height.iter())
                .chain(definition.vertical.base.iter())
            {
                if let Ok(expr) = s.evaluation_expression(expression) {
                    for name in expr.names() {
                        cut_sources.extend(child_context.named_sources(&name, &mut Vec::new()));
                    }
                }
            }
        }
        for (name, dependencies) in &mut context.dependencies {
            if !name.contains("gross") && (name.contains("volume") || name.contains("area")) {
                dependencies.extend(cut_sources.clone());
            }
        }
        let source = reference(s, object, &evidence);
        let mut measured = false;
        for rule in &method.rules {
            if rule
                .element_type
                .as_ref()
                .is_some_and(|t| !t.eq_ignore_ascii_case(&object.instance.type_name))
                || rule
                    .trade
                    .as_ref()
                    .is_some_and(|t| !t.eq_ignore_ascii_case(&object.category))
            {
                continue;
            }
            measured = true;
            let mut decision_sources: Vec<_> = rule
                .bands
                .iter()
                .flat_map(|band| context.expression_sources(&band.when))
                .collect();
            let bands = rule
                .bands
                .iter()
                .map(|b| context.condition(&b.when).map(|yes| (b, yes)))
                .collect::<Result<Vec<_>, _>>()?;
            let bands = bands
                .into_iter()
                .filter(|(_, yes)| *yes)
                .map(|(b, _)| b)
                .collect::<Vec<_>>();
            if bands.len() > 1 {
                return Err(invalid(format!(
                    "Overlapping unit bands in {} for {}",
                    rule.id, object.instance.id
                )));
            }
            let (unit, terms, deductions) = bands
                .first()
                .map_or((&rule.unit, &rule.terms, &rule.deductions), |band| {
                    (&band.unit, &band.terms, &band.deductions)
                });
            let mut citations = vec![rule.citation.clone()];
            if let Some(band) = bands.first() {
                citations.push(band.citation.clone());
            }
            let mut lines = Vec::new();
            let line_id = |part: &str| format!("{}/{}/{part}", object.identity, rule.id);
            for (index, term) in terms.iter().enumerate() {
                let mut entry = line(
                    &context,
                    term,
                    unit,
                    method,
                    &citations,
                    vec![source.clone()],
                )
                .map_err(|e| on_object(e, object.instance.id))?;
                entry.id = line_id(&format!("t{index}"));
                lines.push(entry);
            }
            for (index, deduction) in deductions.iter().enumerate() {
                for void in objects.iter().filter(|v| {
                    v.instance.host == Some(object.instance.id)
                        && v.instance
                            .type_name
                            .eq_ignore_ascii_case(&deduction.element_type)
                }) {
                    if !void.valid {
                        return Err(invalid(format!(
                            "Void {} has invalid geometry",
                            void.instance.id
                        )));
                    }
                    let mut child_context = Context {
                        values: context.values.clone(),
                        sources: context.sources.clone(),
                        units: context.units.clone(),
                        dependencies: context.dependencies.clone(),
                        ..Context::new(s)
                    };
                    child_context.add_object(void, "void.");
                    let boundary = child_context.condition(&deduction.boundary.when)?;
                    let predicate = if boundary {
                        &deduction.boundary.deduct_when
                    } else {
                        &deduction.when
                    };
                    let included = child_context.condition(predicate)?;
                    decision_sources
                        .extend(child_context.expression_sources(&deduction.boundary.when));
                    decision_sources.extend(child_context.expression_sources(predicate));
                    let mut refs = citations.clone();
                    refs.push(deduction.boundary.citation.clone());
                    let mut entry = line(
                        &child_context,
                        &deduction.term,
                        unit,
                        method,
                        &refs,
                        vec![source.clone(), reference(s, void, &evidence)],
                    )
                    .map_err(|e| on_object(e, object.instance.id))?;
                    entry.id = line_id(&format!("d{index}/{}", void.identity));
                    entry.included = included;
                    entry.decision = Some(format!(
                        "{} void; {predicate} = {included}",
                        if boundary { "boundary" } else { "interior" }
                    ));
                    lines.push(entry);
                }
            }
            let raw = total(&context, &lines)?;
            let limited = raw < 0.0;
            if limited {
                // Never a negative quantity, and never a refused method: the quantity is limited
                // to nothing, the working says so, and the report names what to look at.
                let gross: f64 = lines
                    .iter()
                    .filter(|l| l.included && l.value > 0.0)
                    .map(|l| l.value)
                    .sum();
                report.warnings.push(format!(
                    "Rule {} on object {} ({}): deductions of {:.3} {unit} exceed its gross quantity of {gross:.3} {unit}, so its quantity is limited to 0. If the rule deducts from geometry.volume, geometry.net_area, geometry.internal_face_area, geometry.external_face_area or another net figure, those already exclude the hosted openings: deduct from geometry.gross_volume, geometry.gross_area or the corresponding geometry.gross_internal_face_area / geometry.gross_external_face_area instead, or drop the deduction. Otherwise check the openings on this object",
                    rule.id,
                    object.instance.id,
                    object.instance.type_name,
                    gross - raw,
                ));
                lines.push(DimensionLine {
                    id: line_id("limited"),
                    operands: vec![],
                    label: LIMITED_LINE.into(),
                    operation: Operation::Add,
                    timesing: 1.0,
                    factors: vec![-raw],
                    source_expressions: vec![format!("{}", -raw)],
                    expression: format!("({})", -raw),
                    value: -raw,
                    unit: unit.clone(),
                    method: super::canonical_method_name(&method.name),
                    citations: vec![rule.citation.clone()],
                    sources: vec![source.clone()],
                    included: true,
                    decision: Some(format!(
                        "deductions {:.3} exceed gross {gross:.3}: limited to 0",
                        gross - raw
                    )),
                });
            }
            if let Some(rounding) = &rule.rounding {
                let scale = 10_f64.powi(rounding.decimals as i32);
                let call = match rounding.mode {
                    RoundMode::Nearest => "round",
                    RoundMode::Up => "ceil",
                    RoundMode::Down => "floor",
                };
                let expression = format!("{call}(({raw}) * {scale}) / {scale} - ({raw})");
                let adjustment = context.number(&expression)?;
                lines.push(DimensionLine {
                    id: line_id("rounding"),
                    operands: vec![],
                    label: "Rounding adjustment".into(),
                    operation: if adjustment < 0.0 {
                        Operation::Deduct
                    } else {
                        Operation::Add
                    },
                    timesing: 1.0,
                    factors: vec![adjustment.abs()],
                    source_expressions: vec![expression.clone()],
                    expression,
                    value: adjustment,
                    unit: unit.clone(),
                    method: super::canonical_method_name(&method.name),
                    citations: vec![rounding.citation.clone()],
                    sources: vec![source.clone()],
                    included: true,
                    decision: None,
                });
            }
            // Never a negative quantity (a limited one is exactly nothing, not rounding dust).
            let quantity = if limited {
                0.0
            } else {
                total(&context, &lines)?.max(0.0)
            };
            let confidence =
                lines
                    .iter()
                    .flat_map(|l| &l.sources)
                    .fold(rule.confidence, |value, source| {
                        let recognition = evidence
                            .iter()
                            .filter(|c| c.object == Some(source.handle))
                            .map(|c| c.confidence)
                            .reduce(f64::min)
                            .unwrap_or(crate::trust::unscored());
                        value.min(recognition)
                    });
            let assumed_decision = decision_sources.iter().any(|source| {
                matches!(
                    source.source.kind,
                    SourceKind::TypeDefault | SourceKind::Mizan | SourceKind::Unknown
                )
            });
            if let Some(operand) = lines.iter_mut().flat_map(|line| &mut line.operands).next() {
                for source in decision_sources {
                    if !operand.sources.contains(&source) {
                        operand.sources.push(source);
                    }
                }
            }
            report.quantities.push(MeasuredQuantity {
                confidence,
                object: object.instance.id,
                rule: rule.id.clone(),
                method: super::canonical_method_name(&method.name),
                citation: rule.citation.clone(),
                unit: unit.clone(),
                quantity,
                coverage: rule.coverage.clone(),
                lines: lines.clone(),
            });
            let Some(mapping) = mappings.iter().find(|m| m.rule == rule.id) else {
                report
                    .warnings
                    .push(format!("No BOQ mapping for rule {}", rule.id));
                continue;
            };
            let mut mapped_context = Context {
                values: context.values.clone(),
                ..Context::new(s)
            };
            mapped_context
                .values
                .insert("unit".into(), EValue::Text(unit.clone()));
            let mut item = GeneratedItem {
                assumed: assumed_decision
                    || lines
                        .iter()
                        .filter(|l| l.included)
                        .flat_map(|l| &l.operands)
                        .flat_map(|o| &o.sources)
                        .any(|s| {
                            matches!(
                                s.source.kind,
                                SourceKind::TypeDefault | SourceKind::Mizan | SourceKind::Unknown
                            )
                        }),
                confidence,
                tolerances: rule.tolerance.clone().into_iter().collect(),
                code: template(&mapping.code, &mapped_context)?,
                description_en: template(&mapping.description_en, &mapped_context)?,
                description_ar: template(
                    &mapping.description_ar.replace("{type}", "{type_ar}"),
                    &mapped_context,
                )?,
                section: template(&mapping.section, &mapped_context)?,
                trade: template(&mapping.trade, &mapped_context)?,
                unit: template(&mapping.unit, &mapped_context)?,
                quantity,
                mapping_citations: vec![mapping.citation.clone()],
                coverage: rule.coverage.clone(),
                lines,
                informational: mapping.informational,
                limited,
                base_code: String::new(),
                base_description_en: String::new(),
                base_description_ar: String::new(),
                second_reading: None,
            };
            for kind in ["wall", "door", "window", "room"] {
                let id = format!("Legacy {kind}");
                item.description_en = item
                    .description_en
                    .replace(&id, &crate::objects::public_type_name(&id, false));
                item.description_ar = item
                    .description_ar
                    .replace(&id, &crate::objects::public_type_name(&id, true));
            }
            if item.unit != *unit {
                return Err(invalid(format!(
                    "BOQ mapping {} changes measured unit {unit} to {} without a measurement rule",
                    item.code, item.unit
                )));
            }
            // Mapping fields determine which physical quantities are grouped together.
            // Preserve defaults behind explicitly supplied size/grouping placeholders too.
            let mut grouping_sources = Vec::new();
            for text in [
                &mapping.code,
                &mapping.description_en,
                &mapping.description_ar,
                &mapping.section,
                &mapping.trade,
            ] {
                let mut rest = text.as_str();
                while let Some((_, after)) = rest.split_once('{') {
                    let Some((name, tail)) = after.split_once('}') else {
                        break;
                    };
                    grouping_sources.extend(context.named_sources(name, &mut Vec::new()));
                    rest = tail;
                }
            }
            item.assumed |= grouping_sources.iter().any(|s| {
                matches!(
                    s.source.kind,
                    SourceKind::TypeDefault | SourceKind::Mizan | SourceKind::Unknown
                )
            });
            if let Some(operand) = item
                .lines
                .iter_mut()
                .find(|l| l.included)
                .and_then(|l| l.operands.first_mut())
            {
                for dependency in grouping_sources {
                    if !operand.sources.contains(&dependency) {
                        operand.sources.push(dependency);
                    }
                }
            }
            // A BOQ line is one description. Doors and windows of different sizes or marks are
            // different lines unless the mapping's own text already says which size.
            let sized = [
                &mapping.code,
                &mapping.description_en,
                &mapping.description_ar,
            ]
            .iter()
            .any(|t| SIZE_PLACEHOLDERS.iter().any(|p| t.contains(p)));
            if !sized && !item.informational {
                if let Some((code, en, ar)) = size_label(object, unit) {
                    // Group counts by size using the same source trail as their physical operands.
                    let dependencies: Vec<_> = ["width", "head", "sill"]
                        .iter()
                        .flat_map(|name| context.named_sources(name, &mut Vec::new()))
                        .collect();
                    item.assumed |= dependencies.iter().any(|s| {
                        matches!(
                            s.source.kind,
                            SourceKind::TypeDefault | SourceKind::Mizan | SourceKind::Unknown
                        )
                    });
                    if let Some(operand) = item
                        .lines
                        .iter_mut()
                        .find(|l| l.included)
                        .and_then(|l| l.operands.first_mut())
                    {
                        for dependency in dependencies {
                            if !operand.sources.contains(&dependency) {
                                operand.sources.push(dependency);
                            }
                        }
                    }
                    item.base_code = item.code.clone();
                    item.base_description_en = item.description_en.clone();
                    item.base_description_ar = item.description_ar.clone();
                    item.code = format!("{}-{code}", item.code);
                    item.description_en = format!("{} - {en}", item.description_en);
                    item.description_ar = format!("{} - {ar}", item.description_ar);
                }
            }
            let key = (
                item.code.clone(),
                item.description_en.clone(),
                item.description_ar.clone(),
                item.section.clone(),
                item.trade.clone(),
                item.unit.clone(),
            );
            if let Some(existing) = items.get_mut(&key) {
                for t in item.tolerances {
                    if !existing.tolerances.contains(&t) {
                        existing.tolerances.push(t);
                    }
                }
                existing.confidence = existing.confidence.min(item.confidence);
                existing.limited |= item.limited;
                existing.assumed |= item.assumed;
                existing.lines.extend(item.lines);
                existing.quantity = total(&context, &existing.lines)?.max(0.0);
                for c in item.mapping_citations {
                    if !existing.mapping_citations.contains(&c) {
                        existing.mapping_citations.push(c);
                    }
                }
                for c in item.coverage {
                    if !existing.coverage.contains(&c) {
                        existing.coverage.push(c);
                    }
                }
            } else {
                items.insert(key, item);
            }
        }
        if measured && source.grid.is_none() {
            report.warnings.push(format!(
                "Object {} on {} has no assigned grid reference",
                object.instance.id, object.sheet
            ));
        }
        if !measured {
            report.warnings.push(format!(
                "No measurement rule covers object {} ({})",
                object.instance.id, object.instance.type_name
            ));
        }
    }
    // One code, several descriptions: keep every line, number the codes, tell the author.
    let mut by_code = BTreeMap::<String, usize>::new();
    for item in items.values() {
        *by_code.entry(item.code.clone()).or_default() += 1;
    }
    let mut seen = BTreeMap::<String, usize>::new();
    for item in items.values_mut() {
        if by_code[&item.code] > 1 {
            let n = seen.entry(item.code.clone()).or_default();
            *n += 1;
            if *n > 1 {
                item.code = format!("{}-{n}", item.code);
            }
        }
    }
    for (code, n) in by_code.iter().filter(|(_, n)| **n > 1) {
        report.warnings.push(format!("BOQ code {code} was given to {n} different descriptions or units; the later ones were numbered {code}-2…. Put the property that differs in the mapping's code or description template"));
    }
    for item in items.values().filter(|i| i.informational) {
        report.warnings.push(format!("BOQ line {} is informational, so it is left out of the BOQ (it stays in the dimension sheets)", item.code));
    }
    report.items = items.into_values().collect();
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
    report.warnings.sort();
    report.warnings.dedup();
    Ok(report)
}
