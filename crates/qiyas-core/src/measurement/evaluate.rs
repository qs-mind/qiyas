use std::collections::BTreeMap;

use opencadcodec::Handle;
use qiyas_param::expr::{Resolver, Value as EValue};

use super::*;
use crate::objects::{ObjectFilter, ObjectRecord};

struct Context<'a> {
    session: &'a Session,
    values: BTreeMap<String, EValue>,
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
    fn add_object(&mut self, object: &ObjectRecord, prefix: &str) {
        for (k, v) in &object.instance.props {
            if let Some(v) = typed(v) {
                self.values.insert(format!("{prefix}{k}"), v);
            }
        }
        for (k, v) in &object.facts {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Number(*v));
            self.values
                .insert(format!("{prefix}geometry.{k}"), EValue::Number(*v));
        }
        for (k, v) in &object.quantities {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Number(*v));
        }
        self.values
            .insert(format!("{prefix}id"), EValue::Text(object.identity.clone()));
        for (k, v) in [
            ("type", object.instance.type_name.as_str()),
            ("trade", object.category.as_str()),
            ("level", object.instance.level.as_deref().unwrap_or("")),
        ] {
            self.values
                .insert(format!("{prefix}{k}"), EValue::Text(v.into()));
        }
    }
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

/// A line for a gross floor area, a circulation share or another figure that is information,
/// not an item to buy or build, by what its own description says.
fn reads_as_information(description_en: &str, description_ar: &str, section: &str) -> bool {
    let text = format!("{description_en} {description_ar} {section}").to_lowercase();
    [
        "gross floor area",
        "gfa",
        "informational",
        "for information",
        "المسطح الإجمالي",
        "إجمالي المسطح",
        "المساحة الإجمالية",
        "معلوماتي",
    ]
    .iter()
    .any(|marker| {
        text.match_indices(marker).any(|(at, _)| {
            // "gfa" inside another word is not the abbreviation.
            *marker != "gfa"
                || (!text[..at].ends_with(|c: char| c.is_alphanumeric())
                    && !text[at + 3..].starts_with(|c: char| c.is_alphanumeric()))
        })
    })
}

/// What tells this object's size apart from another's in the same BOQ line: its mark (as the
/// schedule shows it), else for an opening its width and height. `(code part, description part)`.
fn size_label(s: &Session, object: &ObjectRecord, unit: &str) -> Option<(String, String)> {
    let text = |name: &str| {
        object
            .instance
            .props
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    };
    // Doors and windows only: any other counted object keeps the mapping's own code, which a
    // client BOQ is matched by.
    if !object.category.eq_ignore_ascii_case("opening") || !matches!(unit, "pcs" | "nr" | "count")
    {
        return None;
    }
    if let Some(mark) = text("mark") {
        return Some((mark.clone(), mark));
    }
    if let Some(mark) = s
        .drawing
        .doc
        .get_entity(Handle::new(object.instance.id))
        .and_then(crate::opening_schedule::mark)
    {
        return Some((mark.clone(), mark));
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
        ),
        None => (format!("{}", mm(width)), format!("{} mm", mm(width))),
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
        let mut context = Context {
            session: s,
            values: BTreeMap::new(),
        };
        context.add_object(object, "");
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
            for term in terms {
                lines.push(line(
                    &context,
                    term,
                    unit,
                    method,
                    &citations,
                    vec![source.clone()],
                )
                .map_err(|e| on_object(e, object.instance.id))?);
            }
            for deduction in deductions {
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
                        session: s,
                        values: context.values.clone(),
                    };
                    child_context.add_object(void, "void.");
                    let boundary = child_context.condition(&deduction.boundary.when)?;
                    let predicate = if boundary {
                        &deduction.boundary.deduct_when
                    } else {
                        &deduction.when
                    };
                    let included = child_context.condition(predicate)?;
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
                    "Rule {} on object {} ({}): deductions of {:.3} {unit} exceed its gross quantity of {gross:.3} {unit}, so its quantity is limited to 0. If the rule deducts from geometry.volume, geometry.net_area or another net figure, those already exclude the hosted openings: deduct from geometry.gross_volume or geometry.gross_area instead, or drop the deduction. Otherwise check the openings on this object",
                    rule.id,
                    object.instance.id,
                    object.instance.type_name,
                    gross - raw,
                ));
                lines.push(DimensionLine {
                    label: "Deductions limited to the gross quantity".into(),
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
                session: s,
                values: context.values.clone(),
            };
            mapped_context
                .values
                .insert("unit".into(), EValue::Text(unit.clone()));
            let mut item = GeneratedItem {
                confidence,
                tolerances: rule.tolerance.clone().into_iter().collect(),
                code: template(&mapping.code, &mapped_context)?,
                description_en: template(&mapping.description_en, &mapped_context)?,
                description_ar: template(&mapping.description_ar, &mapped_context)?,
                section: template(&mapping.section, &mapped_context)?,
                trade: template(&mapping.trade, &mapped_context)?,
                unit: template(&mapping.unit, &mapped_context)?,
                quantity,
                mapping_citations: vec![mapping.citation.clone()],
                coverage: rule.coverage.clone(),
                lines,
                informational: mapping.informational
                    || reads_as_information(
                        &mapping.description_en,
                        &mapping.description_ar,
                        &mapping.section,
                    ),
            };
            if item.unit != *unit {
                return Err(invalid(format!(
                    "BOQ mapping {} changes measured unit {unit} to {} without a measurement rule",
                    item.code, item.unit
                )));
            }
            // A BOQ line is one description. Doors and windows of different sizes or marks are
            // different lines unless the mapping's own text already says which size.
            let sized = [&mapping.code, &mapping.description_en, &mapping.description_ar]
                .iter()
                .any(|t| SIZE_PLACEHOLDERS.iter().any(|p| t.contains(p)));
            if !sized && !item.informational {
                if let Some((code, label)) = size_label(s, object, unit) {
                    item.code = format!("{}-{code}", item.code);
                    item.description_en = format!("{} — {label}", item.description_en);
                    item.description_ar = format!("{} — {label}", item.description_ar);
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
