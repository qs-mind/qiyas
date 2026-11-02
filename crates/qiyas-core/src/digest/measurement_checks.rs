//! Structural checks of measured counts and cited percentage recipes.
use crate::measurement::{MeasurementReport, Operation, Rule, Term};
use qiyas_param::expr::{Expr, Op};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn constant(text: &str) -> Option<f64> {
    Expr::parse(text).ok()?.eval(&|_| None).ok()
}

fn multiplied_by(expr: &Expr, factor: f64) -> bool {
    if expr.eval(&|_| None).is_ok_and(|v| close(v, factor)) {
        return true;
    }
    match expr {
        Expr::Binary(Op::Mul, a, b) => multiplied_by(a, factor) || multiplied_by(b, factor),
        Expr::Binary(Op::Div, a, b) => {
            multiplied_by(a, factor) || b.eval(&|_| None).is_ok_and(|v| close(1.0 / v, factor))
        }
        _ => false,
    }
}

pub(super) fn terms_scaled(terms: &[Term], factor: f64) -> bool {
    if terms.is_empty() {
        return false;
    }
    // A factor can be a separate operand, timesing, or part of an arithmetic expression.
    if terms.iter().all(|t| {
        t.factors
            .iter()
            .chain([&t.timesing])
            .any(|f| Expr::parse(f).is_ok_and(|e| multiplied_by(&e, factor)))
    }) {
        return true;
    }
    // A base plus a proportional term is the same multiplication, even when the base
    // includes another dimensional factor. Compare recipes, never measured quantities.
    if terms.len() == 2 {
        for (base, extra) in [(&terms[0], &terms[1]), (&terms[1], &terms[0])] {
            if base.operation != Operation::Add {
                continue;
            }
            let recipe = |term: &Term| {
                let mut coefficient = constant(&term.timesing)?;
                let mut variables = Vec::new();
                for f in &term.factors {
                    if let Some(value) = constant(f) {
                        coefficient *= value;
                    } else {
                        variables.push(f.trim().to_owned());
                    }
                }
                variables.sort();
                Some((coefficient, variables))
            };
            if let (Some((a, av)), Some((b, bv))) = (recipe(base), recipe(extra)) {
                let sign = if extra.operation == Operation::Add {
                    1.0
                } else {
                    -1.0
                };
                if !av.is_empty() && av == bv && a != 0.0 && close(1.0 + sign * b / a, factor) {
                    return true;
                }
            }
        }
    }
    false
}

fn rule_scaled(rule: &Rule, factor: f64) -> bool {
    terms_scaled(&rule.terms, factor) && rule.bands.iter().all(|b| terms_scaled(&b.terms, factor))
}

pub(super) fn findings(report: &MeasurementReport, notes: &[(u64, String)]) -> Vec<Value> {
    let mut errors = Vec::new();
    let method = report.selection.effective();
    if let Some(method) = method {
        for rule in &method.rules {
            let mut net_names = BTreeSet::new();
            for (terms, deductions) in std::iter::once((&rule.terms, &rule.deductions))
                .chain(rule.bands.iter().map(|b| (&b.terms, &b.deductions)))
            {
                if deductions.is_empty() {
                    continue;
                }
                for expression in terms
                    .iter()
                    .flat_map(|t| t.factors.iter().chain([&t.timesing]))
                {
                    if let Ok(expr) = Expr::parse(expression) {
                        net_names.extend(expr.names().into_iter().filter(|name| {
                            matches!(
                                name.as_str(),
                                "geometry.net_area"
                                    | "geometry.volume"
                                    | "geometry.internal_face_area"
                                    | "geometry.external_face_area"
                            )
                        }));
                    }
                }
            }
            for name in net_names {
                errors.push(json!({"section":"measurement_errors","severity":"error",
                    "code":"deduction_from_net","rule":rule.id,"net_name":name,
                    "message":format!("Rule {} deducts hosted openings from {name}, which already excludes them; use the corresponding gross quantity or remove the deduction.", rule.id)}));
            }
        }
    }
    for q in &report.quantities {
        let kind = method
            .and_then(|m| m.rules.iter().find(|r| r.id == q.rule))
            .and_then(|r| r.kind.as_deref());
        if (kind == Some("count") || (kind.is_none() && q.unit == "pcs"))
            && !close(q.quantity, q.quantity.round())
        {
            errors.push(json!({"section":"measurement_errors","severity":"error",
                "code":"fractional_count","handle":q.object,"rule":q.rule,
                "message":"Count quantities must be whole numbers; correct the rule before finishing."}));
        }
    }
    // A deduction beyond the gross quantity limited an object to nothing: the BOQ would carry a
    // zero for work that exists, so it is an error until the openings or the rule are corrected.
    for q in &report.quantities {
        if q.lines
            .iter()
            .any(|l| l.label == crate::measurement::LIMITED_LINE)
        {
            errors.push(json!({"section":"measurement_errors","severity":"error",
                "code":"deductions_exceed_gross","handle":q.object,"rule":q.rule,
                "message":"Deductions exceed the object's gross quantity, so its quantity was limited to 0; correct the rule or the openings before finishing."}));
        }
    }
    let Some(method) = method.filter(|m| m.rules.len() > 1) else {
        return errors;
    };
    let citations: Vec<_> = method
        .rules
        .iter()
        .flat_map(|r| {
            std::iter::once(&r.citation)
                .chain(r.terms.iter().map(|t| &t.citation))
                .chain(r.bands.iter().flat_map(|b| {
                    std::iter::once(&b.citation).chain(b.terms.iter().map(|t| &t.citation))
                }))
        })
        .map(|c| format!("{} {}", c.source, c.clause))
        .collect();
    let mut cited_notes: Vec<String> = notes
        .iter()
        .filter(|(h, text)| {
            !text.is_empty()
                && citations.iter().any(|c| {
                    c.contains(text)
                        || c.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| {
                            w == h.to_string() || w.eq_ignore_ascii_case(&format!("0x{h:X}"))
                        })
                })
        })
        .map(|(_, text)| text.clone())
        .collect();
    // Raster notes can be quoted in citations without a text entity in the drawing.
    cited_notes.extend(citations);
    let percent = regex::Regex::new(r"([0-9]+(?:\.[0-9]+)?)\s*%").unwrap();
    for note in cited_notes {
        let values: Vec<f64> = percent
            .captures_iter(&note)
            .filter_map(|c| c[1].parse::<f64>().ok())
            .collect();
        if values.len() != 1 {
            continue;
        }
        let p = values[0] / 100.0;
        for factor in [1.0 + p, 1.0 - p, p] {
            if close(factor, 1.0) || close(factor, 0.0) {
                continue;
            }
            if method.rules.iter().all(|r| rule_scaled(r, factor)) {
                errors.push(json!({"section":"measurement_errors","severity":"error",
                    "code":"global_allowance","factor":factor,"method":method.name,
                    "message":"allowance applied globally; confirm its scope from the note"}));
                return errors;
            }
        }
    }
    errors
}

pub(super) fn shared_sources(report: &MeasurementReport) -> Vec<Value> {
    let sources: BTreeMap<_, _> = report
        .quantities
        .iter()
        .flat_map(|q| {
            q.lines
                .iter()
                .filter(|l| !l.id.is_empty())
                .map(move |l| (l.id.as_str(), (q.object, q.rule.as_str())))
        })
        .collect();
    let mut groups: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for item in report.items.iter().filter(|i| !i.informational) {
        let code = if item.base_code.is_empty() {
            &item.code
        } else {
            &item.base_code
        };
        for line in item.lines.iter().filter(|l| l.included) {
            if let Some(&(handle, rule)) = sources.get(line.id.as_str()) {
                groups
                    .entry((handle, code, &item.unit))
                    .or_default()
                    .insert(rule);
            }
        }
    }
    let mut questions = Vec::new();
    for ((handle, code, unit), rules) in groups {
        let rules: Vec<_> = rules.into_iter().collect();
        for (i, first) in rules.iter().enumerate() {
            for second in &rules[i + 1..] {
                questions.push(json!({"section":"questions","severity":"question","code":"shared_source",
                    "handle":handle,"rules":[first,second],"boq_code":code,"unit":unit,
                    "message":"Do these rules intentionally measure the same element into this BOQ item? Ask the engineer or record an engineering basis for keeping both."}));
            }
        }
    }
    questions
}

#[cfg(test)]
mod tests {
    use super::*;
    fn report(separate: bool) -> MeasurementReport {
        let citation = json!({"source":"drawing","clause":"note 24: provisional allowance 10%"});
        let mut rules = Vec::new();
        for (id, kind, unit) in [("a", "area", "m2"), ("b", "count", "pcs")] {
            let terms = if separate {
                json!([
                    {"label":"Base","operation":"add","factors":[format!("geometry.{kind}")],"citation":citation},
                    {"label":"Extra","operation":"add","timesing":"10/100","factors":[format!("geometry.{kind}")],"citation":citation}
                ])
            } else {
                json!([{ "label":"Scaled","operation":"add","factors":[format!("geometry.{kind} * (1 + 10/100)")],"citation":citation }])
            };
            rules.push(json!({"id":id,"kind":kind,"unit":unit,"terms":terms,"citation":citation}));
        }
        serde_json::from_value(json!({"selection":{"project":{
            "name":"test","title":"Test","based_on":"drawing","source_note":"note 24","rules":rules
        }},"quantities":[{"object":8,"rule":"b","method":"test","citation":citation,
            "unit":"pcs","quantity":1.1,"coverage":[],"lines":[]}],"items":[],"warnings":[]}))
        .unwrap()
    }

    #[test]
    fn global_allowance_and_fractional_counts_are_errors_for_both_recipe_forms() {
        for separate in [false, true] {
            let mut report = report(separate);
            let errors = findings(&report, &[]);
            assert_eq!(errors.len(), 2);
            assert_eq!(errors[0]["code"], "fractional_count");
            assert_eq!(errors[1]["code"], "global_allowance");
            assert!(errors.iter().all(|e| e["severity"] == "error"));
            report.quantities[0].quantity = 1.0;
            let rule = &mut report.selection.project.as_mut().unwrap().rules[1];
            rule.terms.truncate(1);
            rule.terms[0].factors = vec!["geometry.count".into()];
            assert!(findings(&report, &[]).is_empty());
        }
    }

    #[test]
    fn unrelated_notes_identity_factors_and_whole_counts_do_not_trigger_errors() {
        let mut report = report(false);
        report.quantities[0].quantity = 2.0;
        for rule in &mut report.selection.project.as_mut().unwrap().rules {
            rule.citation.clause = "source shape".into();
            rule.terms[0].citation.clause = "source shape".into();
        }
        assert!(findings(&report, &[(24, "Allowance 10%".into())]).is_empty());
        for rule in &mut report.selection.project.as_mut().unwrap().rules {
            rule.terms[0].factors = vec!["geometry.count * 1".into()];
            rule.terms[0].citation.clause = "note 24".into();
        }
        assert!(findings(&report, &[(24, "Allowance 10%".into())]).is_empty());
    }

    #[test]
    fn net_deduction_checks_parse_names_in_factors_timesing_and_band_recipes() {
        let mut report = report(false);
        report.quantities.clear();
        report.selection.project.as_mut().unwrap().rules.truncate(1);
        let citation = json!({"source":"drawing","clause":"Tower podium openings"});
        let deduction = json!({"element_type":"Podium opening","when":"true",
            "boundary":{"when":"false","deduct_when":"true","citation":citation},
            "term":{"label":"Opening","operation":"deduct","factors":["void.area"],"citation":citation}});
        for (net, gross) in [
            ("geometry.net_area", "geometry.gross_area"),
            ("geometry.volume", "geometry.gross_volume"),
            (
                "geometry.internal_face_area",
                "geometry.gross_internal_face_area",
            ),
            (
                "geometry.external_face_area",
                "geometry.gross_external_face_area",
            ),
        ] {
            for band in [false, true] {
                for timesing in [false, true] {
                    let rule = &mut report.selection.project.as_mut().unwrap().rules[0];
                    rule.bands.clear();
                    rule.deductions = vec![serde_json::from_value(deduction.clone()).unwrap()];
                    rule.terms[0].timesing = if timesing {
                        format!("max({net}, 0)")
                    } else {
                        "1".into()
                    };
                    rule.terms[0].factors = if timesing {
                        vec!["1".into()]
                    } else {
                        vec![format!("({net} + {net}) / 2")]
                    };
                    if band {
                        rule.bands.push(
                            serde_json::from_value(json!({"when":"true","unit":rule.unit,
                            "terms":rule.terms,"deductions":rule.deductions,"citation":citation}))
                            .unwrap(),
                        );
                        rule.deductions.clear();
                        rule.terms[0].timesing = "1".into();
                        rule.terms[0].factors = vec![gross.into()];
                    }
                    let errors = findings(&report, &[]);
                    assert_eq!(errors.len(), 1, "{net}, band={band}, timesing={timesing}");
                    assert_eq!(errors[0]["code"], "deduction_from_net");
                    assert_eq!(errors[0]["net_name"], net);
                    let rule = &mut report.selection.project.as_mut().unwrap().rules[0];
                    let terms = if band {
                        &mut rule.bands[0].terms
                    } else {
                        &mut rule.terms
                    };
                    terms[0].timesing = "1".into();
                    terms[0].factors = vec![format!("if(true, {gross}, '{net}')")];
                    assert!(findings(&report, &[]).is_empty());
                }
            }
        }
    }
}
