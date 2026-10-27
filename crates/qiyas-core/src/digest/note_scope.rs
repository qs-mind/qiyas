//! Explicit note references and their measurement scope, without classifying drawing words.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use crate::measurement::{MeasurementReport, Rule};
use crate::recognition::{Candidate, ReviewState};
use serde_json::{json, Value};

static NUMBERS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\b(?:0[xX][0-9a-fA-F]+|[0-9]+(?:\.[0-9]+)?)\b").unwrap());

fn handle(text: &str) -> Option<u64> {
    if text.starts_with("0x") || text.starts_with("0X") {
        u64::from_str_radix(&text[2..], 16).ok()
    } else {
        text.parse().ok()
    }
}

/// The number is the tail of a code such as a sheet number `A-101` or a mark `D-12`, not a
/// reference to an element.
fn part_of_a_code(text: &str, m: &regex::Match<'_>) -> bool {
    let mut before = text[..m.start()].chars().rev();
    before.next() == Some('-') && before.next().is_some_and(char::is_alphanumeric)
}

pub(super) fn named_handles(text: &str, handles: &BTreeSet<u64>) -> BTreeSet<u64> {
    NUMBERS
        .find_iter(text)
        .filter(|m| !part_of_a_code(text, m))
        .filter_map(|m| handle(m.as_str()))
        .filter(|h| handles.contains(h))
        .collect()
}

fn cites(text: &str, note: u64, full_note: &str) -> bool {
    (!full_note.is_empty() && text.contains(full_note))
        || NUMBERS
            .find_iter(text)
            .filter(|m| !part_of_a_code(text, m))
            .any(|m| handle(m.as_str()) == Some(note))
}

fn citations(rule: &Rule) -> String {
    std::iter::once(&rule.citation)
        .chain(rule.terms.iter().map(|t| &t.citation))
        .chain(
            rule.bands.iter().flat_map(|b| {
                std::iter::once(&b.citation).chain(b.terms.iter().map(|t| &t.citation))
            }),
        )
        .map(|c| format!("{} {}", c.source, c.clause))
        .collect::<Vec<_>>()
        .join("\n")
}

fn exclusions(report: &str) -> BTreeSet<(u64, u64)> {
    // An explicit format keeps a passing mention of a note from excusing missing scope.
    static EXCLUSION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?im)^\s*(?:-\s*)?exclude\s+(0x[0-9a-f]+|[0-9]+)\s*:\s*(.+?)\s*\(note\s+(0x[0-9a-f]+|[0-9]+)\)\s*$",
        ).unwrap()
    });
    EXCLUSION
        .captures_iter(report)
        .filter_map(|c| {
            c[2].chars().any(char::is_alphabetic).then_some(())?;
            Some((handle(&c[1])?, handle(&c[3])?))
        })
        .collect()
}

fn uses_factor(rule: &Rule, text: &str, named: &BTreeSet<u64>) -> bool {
    NUMBERS.find_iter(text).any(|m| {
        if handle(m.as_str()).is_some_and(|h| named.contains(&h)) {
            return false;
        }
        let Ok(value) = m.as_str().parse::<f64>() else {
            return false;
        };
        let tail = text[m.end()..].trim_start();
        let factors = if tail.starts_with('%') {
            vec![value / 100.0, 1.0 + value / 100.0, 1.0 - value / 100.0]
        } else {
            vec![value]
        };
        factors.into_iter().any(|factor| {
            factor != 0.0
                && factor != 1.0
                && (super::measurement_checks::terms_scaled(&rule.terms, factor)
                    || rule.terms.iter().any(|t| {
                        super::measurement_checks::terms_scaled(std::slice::from_ref(t), factor)
                    })
                    || rule
                        .bands
                        .iter()
                        .any(|b| super::measurement_checks::terms_scaled(&b.terms, factor)))
        })
    })
}

pub(super) fn findings(
    report: &MeasurementReport,
    notes: &[(u64, String)],
    handles: &BTreeSet<u64>,
    evidence: &[Candidate],
    end_report: &str,
) -> Vec<Value> {
    let exclusions = exclusions(end_report);
    let sources = |rule: Option<&str>| -> BTreeSet<u64> {
        let quantities: Vec<_> = report
            .quantities
            .iter()
            .filter(|q| rule.is_none_or(|r| r == q.rule))
            .collect();
        let objects: BTreeSet<_> = quantities.iter().map(|q| q.object).collect();
        objects
            .iter()
            .copied()
            .chain(
                quantities
                    .iter()
                    .flat_map(|q| &q.lines)
                    .flat_map(|l| &l.sources)
                    .flat_map(|s| {
                        std::iter::once(s.handle).chain(s.evidence.iter().filter_map(|e| e.handle))
                    }),
            )
            .chain(
                evidence
                    .iter()
                    .filter(|c| {
                        c.state == ReviewState::Accepted
                            && c.object.is_some_and(|h| objects.contains(&h))
                    })
                    .flat_map(|c| {
                        std::iter::once(c.handle).chain(c.evidence.iter().filter_map(|e| e.handle))
                    }),
            )
            .collect()
    };
    let measured = sources(None);
    let rejected: BTreeSet<_> = evidence
        .iter()
        .filter(|c| c.state == ReviewState::Rejected)
        .map(|c| c.handle)
        .collect();
    let mut errors = Vec::new();
    for (note, text) in notes {
        let mut named = named_handles(text, handles);
        named.remove(note);
        for h in &named {
            if (rejected.contains(h) || !measured.contains(h)) && !exclusions.contains(&(*h, *note))
            {
                errors.push(json!({"section":"measurement_errors","severity":"error",
                    "code":"named_handle_omitted","handle":h,"note":note,
                    "message":format!("element {h} named by note {note} was rejected or left out of every rule; measure it or give a reason citing the note in the end report") }));
            }
        }
        if named.is_empty() {
            continue;
        }
        if let Some(method) = report.selection.effective() {
            for rule in &method.rules {
                let elements = sources(Some(&rule.id));
                if !elements.is_empty()
                    && elements.is_disjoint(&named)
                    && cites(&citations(rule), *note, text)
                    && uses_factor(rule, text, &named)
                {
                    errors.push(json!({"section":"measurement_errors","severity":"error",
                        "code":"note_factor_scope","note":note,"rule":rule.id,"handles":elements,
                        "message":format!("factor from note {note} applied to elements the note does not name") }));
                }
            }
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_number_or_mark_is_not_a_reference_to_an_element() {
        let handles: BTreeSet<u64> = [101, 12].into();
        assert!(named_handles("A-101", &handles).is_empty());
        assert!(named_handles("Sheet D-12, drawing A-101", &handles).is_empty());
        assert_eq!(named_handles("see 101 and 12", &handles), [12, 101].into());
        assert_eq!(named_handles("element 101-", &handles), [101].into());
    }

    #[test]
    fn parses_only_complete_numbers_that_are_real_handles() {
        let handles = BTreeSet::from([2, 4, 52, 56, 58, 61, 78]);
        assert_eq!(
            named_handles(
                "Outlines 52,56,58; line 61; schedule 78; factor 2.4; 999 absent; 61 repeated",
                &handles
            ),
            BTreeSet::from([52, 56, 58, 61, 78])
        );
        assert_eq!(
            named_handles(
                "IDs 0x3D and 0X4e; x52 52x 52.56 999999999999999999999999",
                &handles
            ),
            BTreeSet::from([61, 78])
        );
    }

    #[test]
    fn exclusion_requires_a_reason_and_an_explicit_note_citation() {
        assert_eq!(
            exclusions("Exclude 61: duplicate of 52 (note 93)"),
            BTreeSet::from([(61, 93)])
        );
        assert!(exclusions("Handle 61 is unlabelled. Source note 93.").is_empty());
        assert!(exclusions("Exclude 61: (note 93)").is_empty());
        assert!(exclusions("Exclude 61: duplicate of 52").is_empty());
    }

    fn report() -> MeasurementReport {
        let citation = json!({"source":"drawing","clause":"note 93"});
        serde_json::from_value(json!({"selection":{"project":{
            "name":"test","title":"Test","based_on":"drawing","source_note":"note 93",
            "rules":[{"id":"scaled","unit":"m2","terms":[{"label":"Scaled","operation":"add",
                "factors":["geometry.length * 2.4"],"citation":citation}],"citation":citation}]
        }},"quantities":[{"object":52,"rule":"scaled","method":"test","unit":"m2",
            "quantity":18.6,"coverage":[],"lines":[],"citation":citation}],"items":[],"warnings":[]})).unwrap()
    }

    #[test]
    fn omission_and_disjoint_factor_are_errors_even_with_one_rule() {
        let mut report = report();
        let notes = vec![(93, "Element 61; factor 2.4".into())];
        let handles = BTreeSet::from([52, 61, 93]);
        let errors = findings(&report, &notes, &handles, &[], "");
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0]["code"], "named_handle_omitted");
        assert_eq!(errors[0]["handle"], 61);
        assert_eq!(errors[1]["code"], "note_factor_scope");
        assert_eq!(
            errors[1]["message"],
            "factor from note 93 applied to elements the note does not name"
        );
        assert!(errors.iter().all(|e| e["severity"] == "error"));
        // A note-citing exclusion only excuses omission, never a misapplied factor.
        assert_eq!(
            findings(
                &report,
                &notes,
                &handles,
                &[],
                "Exclude 61: duplicate (note 93)"
            )
            .len(),
            1
        );
        assert_eq!(
            findings(
                &report,
                &notes,
                &handles,
                &[],
                "Exclude 61: duplicate (note 94)"
            )
            .len(),
            2
        );
        assert_eq!(
            findings(
                &report,
                &notes,
                &handles,
                &[],
                "Exclude 52: duplicate (note 93)"
            )
            .len(),
            2
        );
        report.quantities[0].object = 61;
        assert!(findings(&report, &notes, &handles, &[], "").is_empty());
    }

    #[test]
    fn uncited_notes_and_unscaled_rules_do_not_get_factor_errors() {
        let notes = vec![(93, "Element 61; factor 2.4".into())];
        let handles = BTreeSet::from([52, 61, 93]);
        let mut report = report();
        let rule = &mut report.selection.project.as_mut().unwrap().rules[0];
        rule.citation.clause = "note 94".into();
        rule.terms[0].citation.clause = "note 94".into();
        assert_eq!(findings(&report, &notes, &handles, &[], "").len(), 1);
        let rule = &mut report.selection.project.as_mut().unwrap().rules[0];
        rule.citation.clause = "note 93".into();
        rule.terms[0].factors = vec!["geometry.length".into()];
        assert_eq!(findings(&report, &notes, &handles, &[], "").len(), 1);
        assert!(findings(&report, &[(93, "Factor 2.4".into())], &handles, &[], "").is_empty());
    }

    #[test]
    fn rejection_needs_a_cited_reason_even_if_an_old_quantity_remains() {
        let mut report = report();
        report.quantities[0].object = 61;
        let notes = vec![(93, "Element 61; factor 2.4".into())];
        let handles = BTreeSet::from([52, 61, 93]);
        let candidate =
            serde_json::from_value(json!({"handle":61,"rule":"observed","confidence":0.5,
            "state":"rejected","placement":{"type":"Observed","driver":61},
            "evidence":[],"issues":["unlabelled"],"object":61}))
            .unwrap();
        let evidence = [candidate];
        let errors = findings(&report, &notes, &handles, &evidence, "");
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["code"], "named_handle_omitted");
        assert!(findings(
            &report,
            &notes,
            &handles,
            &evidence,
            "Exclude 61: duplicate of 52 (note 93)"
        )
        .is_empty());
    }

    #[test]
    fn accepted_source_evidence_counts_as_rule_use() {
        let mut report = report();
        report.quantities[0].object = 101;
        let notes = vec![(93, "Element 61; reference 78; factor 2.4".into())];
        let handles = BTreeSet::from([61, 78, 93, 101]);
        let evidence =
            [
                serde_json::from_value(json!({"handle":61,"rule":"observed","confidence":1.0,
            "state":"accepted","placement":{"type":"Observed","driver":61},
            "evidence":[{"handle":78}],"issues":[],"object":101}))
                .unwrap(),
            ];
        assert!(findings(&report, &notes, &handles, &evidence, "").is_empty());
    }
}
