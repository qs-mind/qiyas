//! Item quantities and applied inputs, paged with the other take-off findings.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::EntityType;
use qiyas_param::expr::Expr;
use serde_json::{json, Value};

use crate::decisions::{Decision, DecisionSource, Status};
use crate::knowledge::{self, Kind};
use crate::measurement::{MeasurementReport, NamedSource, Operation, SourceKind};
use crate::{CoreError, Session};

struct Evidence {
    handles: BTreeMap<u64, &'static str>,
    tables: Vec<String>,
    decisions: Vec<Decision>,
}

fn names(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
        text[..at].chars().next_back().is_none_or(|c| !word(c))
            && text[at + name.len()..]
                .chars()
                .next()
                .is_none_or(|c| !word(c))
    })
}

impl Evidence {
    fn read(session: &Session) -> Result<Self, CoreError> {
        let handles = session
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                matches!(
                    e,
                    EntityType::Text(_) | EntityType::MText(_) | EntityType::Table(_)
                )
                .then_some((e.common().handle.value(), "drawing"))
            })
            .collect();
        Ok(Self {
            handles,
            tables: knowledge::list(&session.drawing.doc, Kind::Tables)?
                .into_iter()
                .filter(|t| {
                    t.data.get("citation").is_some_and(|c| {
                        c.as_str().is_some_and(|s| !s.trim().is_empty())
                            || c.get("source")
                                .and_then(Value::as_str)
                                .is_some_and(|s| !s.trim().is_empty())
                    })
                })
                .map(|t| t.name)
                .collect(),
            decisions: session.decisions()?,
        })
    }

    fn rule(&self, text: &str) -> &'static str {
        // Match explicit references, not coincidental numbers in a clause or percentage.
        let lower = text.to_lowercase();
        let words: Vec<_> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        for pair in words.windows(2) {
            if ["note", "handle", "table", "ملاحظة", "جدول", "عنصر"].contains(&pair[0])
            {
                let handle = pair[1]
                    .strip_prefix("0x")
                    .map_or_else(|| pair[1].parse::<u64>(), |h| u64::from_str_radix(h, 16));
                if handle.is_ok_and(|h| self.handles.contains_key(&h)) {
                    return "drawing";
                }
            }
        }
        if self.tables.iter().any(|name| names(text, name)) {
            return "document";
        }
        for decision in &self.decisions {
            if decision.status == Status::Answered && names(text, &decision.name) {
                return match decision.source {
                    DecisionSource::Engineer => "engineer",
                    DecisionSource::Drawing => "drawing",
                    DecisionSource::Document | DecisionSource::CodeOrStandard => "document",
                    DecisionSource::Default => "qiyas_default",
                    DecisionSource::MizanDesign | DecisionSource::AssumptionAllowed => "mizan",
                };
            }
        }
        "mizan"
    }

    fn source(&self, source: &NamedSource) -> Option<&'static str> {
        Some(match source.source.kind {
            SourceKind::Measured
            | SourceKind::DrawingNote
            | SourceKind::DrawingAttribute
            | SourceKind::DrawingTable
            | SourceKind::DrawingBlock => "drawing",
            SourceKind::Document => "document",
            SourceKind::Engineer => "engineer",
            SourceKind::TypeDefault => "qiyas_default",
            SourceKind::Rule => {
                if source
                    .source
                    .handle
                    .is_some_and(|h| self.handles.contains_key(&h))
                {
                    "drawing"
                } else {
                    self.rule(&source.source.text)
                }
            }
            SourceKind::Mizan | SourceKind::Unknown => "mizan",
            SourceKind::Derived => return None,
            SourceKind::Ocr => {
                if source.source.handle.is_some() {
                    "drawing"
                } else {
                    "document"
                }
            }
        })
    }

    fn decided_by(&self, sources: &[NamedSource]) -> &'static str {
        // A supplied input cannot conceal another input that was assumed.
        sources
            .iter()
            .filter_map(|s| self.source(s))
            .max_by_key(|who| match *who {
                "mizan" => 4,
                "qiyas_default" => 3,
                "engineer" => 2,
                "document" => 1,
                _ => 0,
            })
            .unwrap_or("mizan")
    }
}

fn summary_row(kind: &str, mut row: Value) -> Value {
    for value in row.as_object_mut().unwrap().values_mut() {
        if let Some(text) = value.as_str() {
            *value = json!(super::short(text));
        }
    }
    json!({"section":"measurement_summary", "kind":kind, "row":row})
}

pub(super) fn rows(session: &Session, report: &MeasurementReport) -> Result<Vec<Value>, CoreError> {
    let evidence = Evidence::read(session)?;
    let mut rows = Vec::new();
    let rules: BTreeMap<_, _> = report
        .quantities
        .iter()
        .flat_map(|q| {
            q.lines
                .iter()
                .map(move |l| (l.id.as_str(), q.rule.as_str()))
        })
        .collect();
    for item in &report.items {
        rows.push(summary_row(
            "measured",
            json!({"item":item.code,"quantity":item.quantity,"unit":item.unit}),
        ));
        let mut seen = BTreeSet::new();
        let mut assumed = BTreeSet::new();
        for line in item.lines.iter().filter(|l| l.included) {
            let rule = rules.get(line.id.as_str()).copied().unwrap_or("");
            if line.operation == Operation::Deduct && !line.id.ends_with("/rounding") {
                rows.push(summary_row("deductions", json!({"item":item.code,
                    "from":line.sources.first().map(|s|s.handle),
                    "deducted":line.sources.get(1).map_or_else(|| json!(super::short(&line.label)), |s|json!(s.handle)),
                    "quantity":line.value.abs(),"unit":line.unit,"rule":rule})));
            }
            for operand in &line.operands {
                let mut factors = vec![(
                    operand.label.clone(),
                    operand.value,
                    operand.sources.clone(),
                )];
                // Show an embedded allowance's value as well as the resulting dimension.
                for source in operand
                    .sources
                    .iter()
                    .filter(|s| s.source.kind == SourceKind::Rule)
                {
                    if let Ok(value) = Expr::parse(&source.name).and_then(|e| e.eval(&|_| None)) {
                        if value != operand.value {
                            factors.push((
                                format!("{}: {}", operand.label, source.name),
                                value,
                                vec![source.clone()],
                            ));
                        }
                    }
                }
                for (name, value, sources) in factors {
                    let who = evidence.decided_by(&sources);
                    let source = sources
                        .iter()
                        .map(|s| {
                            let handle = s
                                .source
                                .handle
                                .map(|h| format!(" handle {h}"))
                                .unwrap_or_default();
                            format!("{}: {:?}{handle} {}", s.name, s.source.kind, s.source.text)
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    let key = json!([name, value, source, who]).to_string();
                    let factor = json!({"item":item.code,"name":super::short(&name),"value":value,
                        "source":super::short(&source),"decided_by":who});
                    if seen.insert(key) {
                        rows.push(summary_row("factors", factor));
                    }
                    if value != 1.0
                        && who == "mizan"
                        && sources.iter().any(|s| s.source.kind == SourceKind::Rule)
                        && assumed.insert(json!([rule, name, value, source]).to_string())
                    {
                        rows.push(
                            json!({"section":"assumed_values","item":super::short(&item.code),"rule":super::short(rule),
                            "name":super::short(&name),"value":value,"source":super::short(&source),
                            "message":"Rule value has no supplied source; Mizan decided it.",
                            "message_ar":"قيمة القاعدة بلا مصدر مقدم؛ قررها ميزان."}),
                        );
                    }
                }
            }
        }
    }
    Ok(rows)
}
