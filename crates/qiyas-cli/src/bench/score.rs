use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CalibrationBand {
    pub submitted: usize,
    pub passed: usize,
}

use qiyas_core::Session;
use serde::{Deserialize, Serialize};

use super::model::{objects, unit, Finding, Item, Submission, Truth};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    #[serde(default)]
    pub calibration: BTreeMap<String, CalibrationBand>,
    pub truth_items: usize,
    pub matched: usize,
    pub passed: usize,
    pub within_1_percent: usize,
    pub within_2_percent: usize,
    pub within_5_percent: usize,
    pub within_10_percent: usize,
    pub within_25_percent: usize,
    pub traceable: usize,
    #[serde(default)]
    pub traceable_submitted: Option<usize>,
    pub extras: usize,
    pub true_positives: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub clean_cases: usize,
    pub clean_false_alarms: usize,
}

impl Metrics {
    pub fn calibrate(&mut self, confidence: Option<f64>, passed: bool) {
        let band = confidence
            .map(qiyas_core::trust::band)
            .unwrap_or("unscored");
        let entry = self.calibration.entry(band.into()).or_default();
        entry.submitted += 1;
        entry.passed += usize::from(passed);
    }
    pub fn calibration_rates(&self) -> serde_json::Value {
        let mut result = serde_json::Map::new();
        for name in ["low", "medium", "high", "unscored"] {
            let empty = CalibrationBand::default();
            let b = self.calibration.get(name).unwrap_or(&empty);
            result.insert(name.into(), serde_json::json!({"submitted":b.submitted,"passed":b.passed,"pass_rate":(b.submitted > 0).then(|| b.passed as f64 / b.submitted as f64)}));
        }
        result.into()
    }
    pub fn merge(&mut self, other: &Self) {
        for (name, band) in &other.calibration {
            let entry = self.calibration.entry(name.clone()).or_default();
            entry.submitted += band.submitted;
            entry.passed += band.passed;
        }
        self.truth_items += other.truth_items;
        self.matched += other.matched;
        self.passed += other.passed;
        self.within_1_percent += other.within_1_percent;
        self.within_2_percent += other.within_2_percent;
        self.within_5_percent += other.within_5_percent;
        self.within_10_percent += other.within_10_percent;
        self.within_25_percent += other.within_25_percent;
        self.traceable_submitted = Some(
            self.traceable_submitted.unwrap_or(self.traceable)
                + other.traceable_submitted.unwrap_or(other.traceable),
        );
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
            "traceability_submitted":ratio(self.traceable_submitted.unwrap_or(self.traceable),self.matched+self.extras),
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
    #[serde(default)]
    pub confidence: Option<f64>,
    pub id: String,
    pub tolerance_class: String,
    pub expected: f64,
    pub submitted: Option<f64>,
    pub matched_by: Option<String>,
    #[serde(default)]
    pub submitted_index: Option<usize>,
    pub absolute_percent_error: Option<f64>,
    pub passed: bool,
    #[serde(default)]
    pub traceable: bool,
    #[serde(default)]
    pub trace_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseScore {
    #[serde(default)]
    pub calibration: serde_json::Value,
    pub id: String,
    pub project_type: String,
    pub metrics: Metrics,
    pub rates: serde_json::Value,
    pub items: Vec<ItemScore>,
    pub extra_items: Vec<String>,
    #[serde(default)]
    pub submission_traces: Vec<super::trace::ItemTrace>,
    pub reported_aggregates: Vec<String>,
    pub error_classes: serde_json::Value,
    pub elapsed_ms: f64,
    pub warnings: Vec<String>,
}

fn description(text: &str) -> String {
    let mut words: Vec<_> = text.split_whitespace().collect();
    // A leading item number ("1", "2.", "(3)") is a drawing label's numbering, not its name.
    let numbering = |w: &str| {
        let core = w.trim_matches(|c: char| "().:-".contains(c));
        !core.is_empty() && core.chars().all(|c| c.is_ascii_digit())
    };
    if words.len() > 1 && numbering(words[0]) {
        words.remove(0);
    }
    words.join(" ").to_lowercase()
}

fn match_item(
    gold: &Item,
    truth: &[Item],
    submitted: &[Item],
    used: &BTreeSet<usize>,
    pdf: bool,
) -> Option<(usize, &'static str)> {
    // Matching never consults the proposed quantity. An ambiguous match is left unmatched.
    // Handle matching is tiered on basis: the exact basis first, then the same basis category
    // (the part before ':', such as `rule`), then the same kind, unit and objects whatever the
    // basis. Truth basis names are benchmark-internal, so an agent cannot be expected to spell
    // them. Each tier accepts only a unique candidate; quantities are never consulted.
    for mode in [
        "id",
        "handles",
        "handles_category",
        "handles_any",
        "description_position",
        "pdf_position",
    ] {
        let candidates: Vec<_> = submitted
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                !used.contains(i)
                    && match mode {
                        "id" => !s.id.is_empty() && s.id == gold.id,
                        "handles" | "handles_category" | "handles_any" => {
                            s.kind == gold.kind
                                && !s.source_objects.is_empty()
                                && objects(s) == objects(gold)
                                && match mode {
                                    "handles" => s.basis == gold.basis,
                                    "handles_category" => {
                                        basis_category(&s.basis) == basis_category(&gold.basis)
                                    }
                                    _ => s.unit == gold.unit,
                                }
                        }
                        "pdf_position" => {
                            pdf && s.kind == gold.kind
                                && s.unit == gold.unit
                                && match (s.position, gold.position) {
                                    (Some(a), Some(b)) => {
                                        (a[0] - b[0]).hypot(a[1] - b[1])
                                            <= 0.5_f64.max(
                                                0.03 * gold
                                                    .diagonal_m
                                                    .or(s.diagonal_m)
                                                    .filter(|d| d.is_finite() && *d >= 0.0)
                                                    .unwrap_or(0.0),
                                            )
                                    }
                                    _ => false,
                                }
                        }
                        _ => {
                            !s.description.is_empty()
                                && description(&s.description) == description(&gold.description)
                                && s.kind == gold.kind
                                && match (s.position, gold.position) {
                                    (Some(a), Some(b)) => {
                                        (a[0] - b[0]).hypot(a[1] - b[1])
                                            <= if pdf {
                                                0.25_f64.max(
                                                    0.02 * gold
                                                        .diagonal_m
                                                        .or(s.diagonal_m)
                                                        .filter(|d| d.is_finite() && *d >= 0.0)
                                                        .unwrap_or(0.0),
                                                )
                                            } else {
                                                0.05
                                            }
                                    }
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
            // Several items on the same objects (a base volume and its allowance): pick the one
            // whose own words (id, basis, description) share the most words with the gold item's,
            // when exactly one leads. Never the quantity; a tie stays unmatched.
            if mode.starts_with("handles")
                || (mode == "pdf_position" && same_position(submitted, &candidates))
            {
                if let Some(i) = closest_by_words(gold, submitted, &candidates) {
                    return Some((i, mode));
                }
            }
            return None;
        }
    }
    None
}

fn same_position(submitted: &[Item], candidates: &[usize]) -> bool {
    let Some(first) = submitted[candidates[0]].position else {
        return false;
    };
    candidates.iter().all(|i| {
        submitted[*i]
            .position
            .is_some_and(|p| (p[0] - first[0]).hypot(p[1] - first[1]) <= 1e-6)
    })
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

pub fn score(truth: &Truth, submission: &Submission, session: Option<&Session>) -> CaseScore {
    let traces = super::trace::submission(submission, session);
    let pdf = std::path::Path::new(&truth.drawing)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let mut metrics = Metrics {
        truth_items: truth.items.len(),
        ..Default::default()
    };
    let aggregates: std::collections::BTreeMap<_, _> = (0..submission.items.len())
        .filter_map(|i| {
            super::submission::aggregate_components(&submission.items, i).map(|c| (i, c))
        })
        .collect();
    let mut used = BTreeSet::new();
    let mut items = Vec::new();
    for gold in &truth.items {
        let mut row = ItemScore {
            confidence: None,
            id: gold.id.clone(),
            tolerance_class: gold.tolerance_class.clone(),
            expected: gold.quantity,
            submitted: None,
            matched_by: None,
            submitted_index: None,
            absolute_percent_error: None,
            passed: false,
            traceable: false,
            trace_reason: "missing item".into(),
        };
        // Prefer dimension items. Grouped truth can still match a complete BOQ total, but
        // neither a total nor any of its components can earn a second match.
        let mut excluded = used.clone();
        excluded.extend(aggregates.keys());
        let matched =
            match_item(gold, &truth.items, &submission.items, &excluded, pdf).or_else(|| {
                let mut excluded = used.clone();
                excluded.extend((0..submission.items.len()).filter(|i| {
                    aggregates
                        .get(i)
                        .is_none_or(|parts| parts.iter().any(|p| used.contains(p)))
                }));
                match_item(gold, &truth.items, &submission.items, &excluded, pdf)
            });
        if let Some((i, mode)) = matched {
            used.insert(i);
            if let Some(parts) = aggregates.get(&i) {
                used.extend(parts);
            }
            let s = &submission.items[i];
            metrics.matched += 1;
            row.submitted = Some(s.quantity);
            row.confidence = s.confidence;
            row.matched_by = Some(mode.into());
            row.submitted_index = Some(i);
            let compatible = unit(&s.unit) == unit(&gold.unit) && s.kind == gold.kind;
            row.absolute_percent_error =
                compatible.then(|| ape(s.quantity, gold.quantity)).flatten();
            let mut within = false;
            if let Some(error) = row.absolute_percent_error {
                within = error <= tolerance(gold);
                metrics.within_1_percent += usize::from(error <= 1.0);
                metrics.within_2_percent += usize::from(error <= 2.0);
                metrics.within_5_percent += usize::from(error <= 5.0);
                metrics.within_10_percent += usize::from(error <= 10.0);
                metrics.within_25_percent += usize::from(error <= 25.0);
            }
            row.traceable = traces[i].traceable;
            row.trace_reason = traces[i].reason.clone();
            // Preserve the existing accuracy penalty for declaring a rule quantity geometric.
            // Traceability is independent of that correctness policy.
            let false_geometry = basis_category(&s.basis) == "geometric"
                && basis_category(&gold.basis) != "geometric";
            row.passed = within && !false_geometry;
            metrics.passed += usize::from(row.passed);
            metrics.calibrate(s.confidence, row.passed);
            metrics.traceable += usize::from(row.traceable);
        }
        items.push(row);
    }
    let extra_items = submission
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i) && !aggregates.contains_key(i))
        .map(|(_, i)| {
            if i.id.is_empty() {
                i.description.clone()
            } else {
                i.id.clone()
            }
        })
        .collect::<Vec<_>>();
    metrics.extras = extra_items.len();
    metrics.traceable_submitted = Some(
        metrics.traceable
            + traces
                .iter()
                .filter(|t| {
                    !used.contains(&t.index) && !aggregates.contains_key(&t.index) && t.traceable
                })
                .count(),
    );
    for (i, item) in submission
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.contains(i) && !aggregates.contains_key(i))
    {
        let _ = i;
        metrics.calibrate(item.confidence, false);
    }
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
        calibration: metrics.calibration_rates(),
        id: truth.id.clone(),
        project_type: truth.project_type.clone(),
        rates: metrics.rates(),
        metrics,
        items,
        extra_items,
        submission_traces: traces,
        reported_aggregates: aggregates
            .keys()
            .map(|i| submission.items[*i].id.clone())
            .collect(),
        error_classes: error_classes.into(),
        elapsed_ms: submission.elapsed_ms,
        warnings: submission.warnings.clone(),
    }
}

/// The part of a basis before ':' (`rule:lining-height` -> `rule`).
fn basis_category(basis: &str) -> &str {
    basis.split(':').next().unwrap_or("").trim()
}

/// Words of three letters or more in an item's id, basis and description, without the
/// structural words every submission repeats.
fn words(texts: &[&str]) -> BTreeSet<String> {
    const COMMON: [&str; 6] = ["rule", "dimension", "geometric", "item", "the", "and"];
    texts
        .iter()
        .flat_map(|t| t.split(|c: char| !c.is_alphanumeric()))
        .map(|w| w.to_lowercase())
        .filter(|w| {
            w.len() >= 3 && !w.chars().all(|c| c.is_ascii_digit()) && !COMMON.contains(&w.as_str())
        })
        .collect()
}

/// The single candidate sharing the most words with `gold`, if one leads with at least one.
fn closest_by_words(gold: &Item, submitted: &[Item], candidates: &[usize]) -> Option<usize> {
    let target = words(&[&gold.description, &gold.basis]);
    let mut scored: Vec<(usize, usize)> = candidates
        .iter()
        .map(|&i| {
            let s = &submitted[i];
            let shared = words(&[&s.id, &s.basis, &s.description])
                .intersection(&target)
                .count();
            (shared, i)
        })
        .collect();
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    match scored.as_slice() {
        [(best, i), rest @ ..] if *best > 0 && rest.first().is_none_or(|(next, _)| next < best) => {
            Some(*i)
        }
        _ => None,
    }
}
