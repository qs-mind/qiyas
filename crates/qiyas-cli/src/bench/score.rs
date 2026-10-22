use std::collections::BTreeSet;

use qiyas_core::Session;
use serde::{Deserialize, Serialize};

use super::model::{unit, Finding, Item, Submission, Truth};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub truth_items: usize,
    pub matched: usize,
    pub passed: usize,
    pub within_1_percent: usize,
    pub within_2_percent: usize,
    pub within_5_percent: usize,
    pub within_10_percent: usize,
    pub within_25_percent: usize,
    pub traceable: usize,
    pub extras: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub clean_cases: usize,
    pub clean_false_alarms: usize,
}

impl Metrics {
    pub fn merge(&mut self, other: &Self) {
        self.truth_items += other.truth_items;
        self.matched += other.matched;
        self.passed += other.passed;
        self.within_1_percent += other.within_1_percent;
        self.within_2_percent += other.within_2_percent;
        self.within_5_percent += other.within_5_percent;
        self.within_10_percent += other.within_10_percent;
        self.within_25_percent += other.within_25_percent;
        self.traceable += other.traceable;
        self.extras += other.extras;
        self.true_positives += other.true_positives;
        self.false_positives += other.false_positives;
        self.false_negatives += other.false_negatives;
        self.clean_cases += other.clean_cases;
        self.clean_false_alarms += other.clean_false_alarms;
    }

    pub fn rates(&self) -> serde_json::Value {
        let ratio = |n, d| (d != 0).then(|| n as f64 / d as f64);
        serde_json::json!({
            "coverage":ratio(self.matched,self.truth_items),
            "item_precision":ratio(self.matched,self.matched+self.extras),
            "item_f1":ratio(2*self.matched,self.truth_items+self.matched+self.extras),
            "pass_rate_all":ratio(self.passed,self.truth_items),
            "pass_rate_matched":ratio(self.passed,self.matched),
            "p_at_1_percent":ratio(self.within_1_percent,self.matched),
            "p_at_2_percent":ratio(self.within_2_percent,self.matched),
            "p_at_5_percent":ratio(self.within_5_percent,self.matched),
            "p_at_10_percent":ratio(self.within_10_percent,self.matched),
            "p_at_25_percent":ratio(self.within_25_percent,self.matched),
            "traceability_submitted":ratio(self.traceable,self.matched+self.extras),
            "traceability_all":ratio(self.traceable,self.truth_items),
            "error_precision":ratio(self.true_positives,self.true_positives+self.false_positives),
            "error_recall":ratio(self.true_positives,self.true_positives+self.false_negatives),
            "error_f1":ratio(2*self.true_positives,2*self.true_positives+self.false_positives+self.false_negatives),
            "false_alarms_per_clean_case":ratio(self.clean_false_alarms,self.clean_cases)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemScore {
    pub id: String,
    pub tolerance_class: String,
    pub expected: f64,
    pub submitted: Option<f64>,
    pub matched_by: Option<String>,
    pub absolute_percent_error: Option<f64>,
    pub passed: bool,
    pub traceable: bool,
    pub trace_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseScore {
    pub id: String,
    pub project_type: String,
    pub metrics: Metrics,
    pub rates: serde_json::Value,
    pub items: Vec<ItemScore>,
    pub extra_items: Vec<String>,
    pub error_classes: serde_json::Value,
    pub elapsed_ms: f64,
    pub warnings: Vec<String>,
}

fn objects(item: &Item) -> BTreeSet<String> {
    item.source_objects
        .iter()
        .map(|s| {
            u64::from_str_radix(s.trim_start_matches("0x"), 16)
                .map(|n| format!("{n:X}"))
                .unwrap_or_else(|_| s.clone())
        })
        .collect()
}

fn description(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn match_item(
    gold: &Item,
    truth: &[Item],
    submitted: &[Item],
    used: &BTreeSet<usize>,
) -> Option<(usize, &'static str)> {
    // Matching never consults the proposed quantity. An ambiguous match is left unmatched.
    for mode in ["id", "handles", "description_position"] {
        let candidates: Vec<_> = submitted
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                !used.contains(i)
                    && match mode {
                        "id" => !s.id.is_empty() && s.id == gold.id,
                        "handles" => {
                            s.kind == gold.kind
                                && s.basis == gold.basis
                                && !s.source_objects.is_empty()
                                && objects(s) == objects(gold)
                        }
                        _ => {
                            !s.description.is_empty()
                                && description(&s.description) == description(&gold.description)
                                && s.kind == gold.kind
                                && match (s.position, gold.position) {
                                    (Some(a), Some(b)) => (a[0] - b[0]).hypot(a[1] - b[1]) <= 0.05,
                                    (None, _) => {
                                        truth
                                            .iter()
                                            .filter(|t| {
                                                t.kind == gold.kind
                                                    && description(&t.description)
                                                        == description(&gold.description)
                                            })
                                            .count()
                                            == 1
                                    }
                                    _ => false,
                                }
                        }
                    }
            })
            .map(|(i, _)| i)
            .collect();
        if candidates.len() == 1 {
            return Some((candidates[0], mode));
        }
        if candidates.len() > 1 {
            return None;
        }
    }
    None
}

pub fn tolerance(item: &Item) -> f64 {
    match item.tolerance_class.as_str() {
        "count" => 0.0,
        "geometry" => 0.5,
        "rule" => item.tolerance_percent.unwrap_or(2.0),
        "judgement" => 10.0,
        _ => 0.0,
    }
}

fn ape(actual: f64, expected: f64) -> Option<f64> {
    if expected == 0.0 {
        (actual == 0.0).then_some(0.0)
    } else {
        Some((actual - expected).abs() / expected.abs() * 100.0)
    }
}

fn remeasure(
    session: &Session,
    gold: &Item,
    submitted: &Item,
    truth: &Truth,
) -> Result<f64, String> {
    if submitted.source_objects.is_empty() {
        return Err("no cited handles".into());
    }
    if objects(submitted) != objects(gold) {
        return Err("citations do not identify the truth item's objects".into());
    }
    if submitted.basis != gold.basis {
        return Err("missing or different measurement rule reference".into());
    }
    let unique = objects(submitted);
    let mut length = 0.0;
    let mut area = 0.0;
    let mut count = 0;
    let mut has_length = true;
    let mut has_area = true;
    let factor = session.drawing.units.to_metres();
    for source in &unique {
        // Model IDs accompany drawing handles but cannot themselves prove a 2D measurement.
        if source.starts_with("ifc:") || source.starts_with("model:") {
            continue;
        }
        let handle = u64::from_str_radix(source.trim_start_matches("0x"), 16)
            .map_err(|_| format!("unresolved source object {source}"))?;
        let info = session
            .entity_info(handle, &[])
            .map_err(|e| e.to_string())?;
        length += info.length.unwrap_or(0.0) * factor;
        area += info.area.unwrap_or(0.0) * factor * factor;
        has_length &= info.length.is_some();
        has_area &= info.area.is_some();
        count += 1;
    }
    if count == 0 {
        return Err("no drawing handles to remeasure".into());
    }
    if let Some(name) = gold.basis.strip_prefix("rule:") {
        let rule = &truth.rules[name];
        return match rule.operation.as_str() {
            "area_times_height" if has_area => Ok(area * rule.height_m.unwrap_or(0.0)),
            "length_times_factor" if has_length => Ok(length * rule.factor.unwrap_or(0.0)),
            "area_net" if has_area => Ok((area - rule.deduction.unwrap_or(0.0)).max(0.0)),
            _ => Err("rule cannot be reproduced from cited geometry".into()),
        };
    }
    match gold.kind.as_str() {
        "count" => Ok(count as f64),
        "length" if has_length => Ok(length),
        "area" if has_area => Ok(area),
        "volume" if has_area => {
            let h = submitted.height_m.ok_or("volume needs cited height_m")?;
            if gold.height_m != Some(h) {
                return Err("height does not match the declared source".into());
            }
            Ok(area * h)
        }
        _ => Err("cited geometry cannot reproduce this kind".into()),
    }
}

pub fn score(truth: &Truth, submission: &Submission, session: Option<&Session>) -> CaseScore {
    let mut metrics = Metrics {
        truth_items: truth.items.len(),
        ..Default::default()
    };
    let mut used = BTreeSet::new();
    let mut items = Vec::new();
    for gold in &truth.items {
        let mut row = ItemScore {
            id: gold.id.clone(),
            tolerance_class: gold.tolerance_class.clone(),
            expected: gold.quantity,
            submitted: None,
            matched_by: None,
            absolute_percent_error: None,
            passed: false,
            traceable: false,
            trace_reason: "missing item".into(),
        };
        if let Some((i, mode)) = match_item(gold, &truth.items, &submission.items, &used) {
            used.insert(i);
            let s = &submission.items[i];
            metrics.matched += 1;
            row.submitted = Some(s.quantity);
            row.matched_by = Some(mode.into());
            let compatible = unit(&s.unit) == unit(&gold.unit) && s.kind == gold.kind;
            row.absolute_percent_error =
                compatible.then(|| ape(s.quantity, gold.quantity)).flatten();
            if let Some(error) = row.absolute_percent_error {
                row.passed = error <= tolerance(gold) && s.basis == gold.basis;
                metrics.within_1_percent += usize::from(error <= 1.0);
                metrics.within_2_percent += usize::from(error <= 2.0);
                metrics.within_5_percent += usize::from(error <= 5.0);
                metrics.within_10_percent += usize::from(error <= 10.0);
                metrics.within_25_percent += usize::from(error <= 25.0);
            }
            metrics.passed += usize::from(row.passed);
            let measured = session
                .ok_or_else(|| "drawing unavailable to remeasure".to_string())
                .and_then(|session| remeasure(session, gold, s, truth));
            match measured {
                Ok(value) => {
                    // Traceability is a numerical reproduction check, stricter than rule acceptance.
                    row.traceable = compatible
                        && ape(s.quantity, value)
                            .is_some_and(|p| p <= if gold.kind == "count" { 0.0 } else { 0.5 });
                    row.trace_reason = if row.traceable {
                        "reproduced from cited handles and basis"
                    } else {
                        "cited geometry does not reproduce the submitted quantity/unit"
                    }
                    .into();
                }
                Err(e) => row.trace_reason = e,
            }
            metrics.traceable += usize::from(row.traceable);
        }
        items.push(row);
    }
    let extra_items = submission
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i))
        .map(|(_, i)| {
            if i.id.is_empty() {
                i.description.clone()
            } else {
                i.id.clone()
            }
        })
        .collect::<Vec<_>>();
    metrics.extras = extra_items.len();
    let expected: BTreeSet<&Finding> = truth.seeded_errors.iter().collect();
    // A prediction may identify a missing item by its drawing evidence rather than a client code.
    // Resolve unique evidence identities here only; prediction never receives this mapping.
    let resolved: Vec<Finding> = submission
        .findings
        .iter()
        .map(|f| {
            let mut f = f.clone();
            let matches: Vec<_> = truth
                .items
                .iter()
                .filter(|i| super::checks::evidence_id(i) == f.item_id)
                .collect();
            if matches.len() == 1 {
                f.item_id = matches[0].id.clone();
            }
            f
        })
        .collect();
    let found: BTreeSet<&Finding> = resolved.iter().collect();
    metrics.true_positives = found.intersection(&expected).count();
    // Repeated alerts do not earn extra true positives and do count as false alarms.
    metrics.false_positives = submission.findings.len() - metrics.true_positives;
    metrics.false_negatives = expected.difference(&found).count();
    metrics.clean_cases = usize::from(expected.is_empty());
    metrics.clean_false_alarms = if expected.is_empty() {
        submission.findings.len()
    } else {
        0
    };
    let classes: BTreeSet<_> = expected
        .iter()
        .chain(found.iter())
        .map(|f| f.kind.as_str())
        .collect();
    let error_classes: serde_json::Map<String,serde_json::Value> = classes.into_iter().map(|class| {
        let tp = found.intersection(&expected).filter(|f|f.kind==class).count();
        let fp = submission.findings.iter().filter(|f|f.kind==class).count()-tp;
        let fn_count = expected.difference(&found).filter(|f|f.kind==class).count();
        (class.into(),serde_json::json!({"true_positives":tp,"false_positives":fp,"false_negatives":fn_count}))
    }).collect();
    CaseScore {
        id: truth.id.clone(),
        project_type: truth.project_type.clone(),
        rates: metrics.rates(),
        metrics,
        items,
        extra_items,
        error_classes: error_classes.into(),
        elapsed_ms: submission.elapsed_ms,
        warnings: submission.warnings.clone(),
    }
}
