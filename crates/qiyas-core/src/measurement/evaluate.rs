use std::collections::BTreeMap;

use opencadcodec::Handle;
use qiyas_param::expr::{Expr, Resolver, Value as EValue};

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
        Expr::parse(expression)
            .and_then(|e| e.eval_with(self))
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
        return Err(invalid(
            "Dimension operands must be nonnegative; use a deduct operation",
        ));
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
        method: method.name.clone(),
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

pub(super) fn derive(s: &Session) -> Result<MeasurementReport, CoreError> {
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
    let mut items = BTreeMap::<String, GeneratedItem>::new();
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
                )?);
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
                    )?;
                    entry.included = included;
                    entry.decision = Some(format!(
                        "{} void; {predicate} = {included}",
                        if boundary { "boundary" } else { "interior" }
                    ));
                    lines.push(entry);
                }
            }
            let raw = total(&context, &lines)?;
            if raw < 0.0 {
                return Err(invalid(format!(
                    "Deductions exceed gross quantity for {} on {}",
                    rule.id, object.instance.id
                )));
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
                    method: method.name.clone(),
                    citations: vec![rounding.citation.clone()],
                    sources: vec![source.clone()],
                    included: true,
                    decision: None,
                });
            }
            let quantity = total(&context, &lines)?;
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
                method: method.name.clone(),
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
            let item = GeneratedItem {
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
            };
            if item.unit != *unit {
                return Err(invalid(format!(
                    "BOQ mapping {} changes measured unit {unit} to {} without a measurement rule",
                    item.code, item.unit
                )));
            }
            if let Some(existing) = items.get_mut(&item.code) {
                if existing.unit != item.unit
                    || existing.description_en != item.description_en
                    || existing.description_ar != item.description_ar
                    || existing.section != item.section
                    || existing.trade != item.trade
                {
                    return Err(invalid(format!("BOQ code {} groups incompatible descriptions or units; include the varying property in its template", item.code)));
                }
                for t in item.tolerances {
                    if !existing.tolerances.contains(&t) {
                        existing.tolerances.push(t);
                    }
                }
                existing.confidence = existing.confidence.min(item.confidence);
                existing.lines.extend(item.lines);
                existing.quantity = total(&context, &existing.lines)?;
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
                items.insert(item.code.clone(), item);
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
