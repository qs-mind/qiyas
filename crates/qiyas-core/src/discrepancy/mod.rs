//! Shared discrepancy engine used by the registry command, desktop and take-off crate.
mod compare;
mod matching;
pub(crate) mod rules;
use crate::{
    knowledge, measurement::Citation, Command, Conflict, ConflictKind, ConflictStatus, CoreError,
    Outcome, Session, Source,
};
pub use compare::{compare, CompareInputs};
pub use rules::{CheckRule, Set, TableSource, Test, Tolerance};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Information,
    #[default]
    Warning,
    Error,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reference {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing: Option<String>,
    pub handle: u64,
    pub sheet: String,
    pub grid: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Query {
    pub severity: Severity,
    pub question_en: String,
    pub question_ar: String,
    pub references: Vec<Reference>,
    pub citations: Vec<Citation>,
    pub rule: Option<String>,
    /// Cloud handles, one per evidence sheet. Labels belong to the existing Comment command.
    pub clouds: Vec<u64>,
}
pub(super) fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}
pub(super) fn finding(
    kind: ConflictKind,
    key: &str,
    reason: String,
    reason_ar: String,
) -> Conflict {
    let hash_key = format!("{}|{key}", kind.name());
    let hash = hash_key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Conflict {
        id: format!("{hash:016x}"),
        kind,
        boq_code: None,
        measurements: vec![],
        schedule: None,
        sheets: vec![],
        expected: None,
        found: None,
        unit: String::new(),
        reason,
        reason_ar,
        status: ConflictStatus::Open,
        note: String::new(),
        created: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("UTC"),
        hash_key,
        query: None,
    }
}

impl Session {
    /// One query per generated item whose blind second reading differs from the first. The
    /// item stays out of the BOQ until the engineer answers it (`resolve_conflict`).
    pub(crate) fn second_reading_conflicts(
        &self,
        report: &crate::measurement::MeasurementReport,
    ) -> Vec<Conflict> {
        report
            .items
            .iter()
            .filter(|i| {
                i.second_reading.as_ref().is_some_and(|s| {
                    matches!(
                        s.status,
                        crate::second_reading::SecondStatus::Differs
                            | crate::second_reading::SecondStatus::Accepted
                    )
                })
            })
            .map(|item| {
                let read = item.second_reading.as_ref().expect("filtered");
                let unit = crate::deliverables::display_unit(&item.unit);
                let reason = format!(
                    "{}: the first reading is {:.2} {unit} and the second, made without it, is {:.2} {unit} (allowed difference {}%)",
                    item.description_en, read.first, read.second, read.tolerance_percent
                );
                let reason_ar = format!(
                    "{}: القراءة الأولى {:.2} {unit} والقراءة الثانية، دون الاطلاع عليها، {:.2} {unit} (الفرق المسموح {}%)",
                    item.description_ar, read.first, read.second, read.tolerance_percent
                );
                let mut c = finding(
                    ConflictKind::QuantityMismatch,
                    &crate::second_reading::conflict_key(&item.code),
                    reason,
                    reason_ar,
                );
                c.boq_code = Some(item.base_code().to_string());
                c.measurements = vec![format!("generated:{}", item.base_code())];
                c.expected = Some(read.first);
                c.found = Some(read.second);
                c.unit = item.unit.clone();
                c
            })
            .collect()
    }

    pub fn evaluate_check_rules(&self) -> Result<Vec<Conflict>, CoreError> {
        rules::evaluate(self)
    }
    pub(crate) fn run_discrepancy_check(
        &mut self,
        tolerance: Option<f64>,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        let tolerance = tolerance.unwrap_or(5.);
        if !tolerance.is_finite() || tolerance < 0. {
            return Err(invalid("Tolerance must be finite and nonnegative"));
        }
        let report = self.measurement_report()?;
        let mut takeoff = self.takeoff.clone();
        let generated = report.comparison_measurements(self.drawing.units);
        takeoff
            .items
            .retain(|m| !generated.iter().any(|g| g.boq_code == m.boq_code));
        takeoff.items.extend(generated);
        let mut boq = self.boq.clone();
        // A method's explicit tolerance governs its mapped item, including unit-band items.
        if let Some(boq) = &mut boq {
            for item in &mut boq.items {
                if report
                    .items
                    .iter()
                    .any(|g| g.base_code() == item.code && !g.tolerances.is_empty())
                {
                    item.tolerance = Some(0.);
                }
            }
        }
        let sheets = self.drawing.sheets();
        let schedules: Vec<_> = sheets
            .iter()
            .flat_map(|s| self.read_tables(Some(&s.block)))
            .collect();
        let blocks: Vec<_> = sheets
            .iter()
            .map(|s| (s.name.clone(), self.block_summary_in_file(Some(&s.block))))
            .collect();
        let has_rules = !knowledge::list(&self.drawing.doc, knowledge::Kind::Checks)?.is_empty();
        // Legacy heuristics remain for old manual take-offs. Data-defined jobs use only their cited checks.
        let legacy = report.items.is_empty() && !has_rules;
        let units = self.info().units;
        let mut conflicts = compare(
            &CompareInputs {
                boq: boq.as_ref(),
                takeoff: &takeoff,
                schedules: if legacy { &schedules } else { &[] },
                blocks: if legacy { &blocks } else { &[] },
                units_label: &units,
            },
            tolerance,
            &self.conflicts,
        );
        conflicts.retain(|c| {
            if c.kind != ConflictKind::QuantityMismatch {
                return true;
            }
            let Some(item) = report
                .items
                .iter()
                .find(|g| c.boq_code.as_deref() == Some(g.base_code()))
            else {
                return true;
            };
            item.tolerances.is_empty()
                || item
                    .tolerances
                    .iter()
                    .any(|t| t.differs(c.expected.unwrap_or(0.), c.found.unwrap_or(0.)))
        });
        conflicts.extend(self.evaluate_check_rules()?);
        conflicts.extend(self.second_reading_conflicts(&report));
        for c in &mut conflicts {
            if c.query.is_none() {
                let mut refs = vec![];
                let mut citations = vec![];
                for m in takeoff
                    .items
                    .iter()
                    .filter(|m| c.measurements.contains(&m.id))
                {
                    for &handle in &m.handles {
                        refs.push(Reference {
                            drawing: None,
                            handle,
                            sheet: m.sheet.clone(),
                            grid: None,
                        });
                    }
                }
                for g in report.items.iter().filter(|g| {
                    c.boq_code.as_deref() == Some(g.base_code())
                        || c.measurements
                            .contains(&format!("generated:{}", g.base_code()))
                }) {
                    citations.extend(g.mapping_citations.clone());
                    for l in &g.lines {
                        citations.extend(l.citations.clone());
                        for r in &l.sources {
                            refs.retain(|x| x.handle != r.handle);
                            refs.push(Reference {
                                drawing: None,
                                handle: r.handle,
                                sheet: r.sheet.clone(),
                                grid: r.grid.clone(),
                            });
                        }
                    }
                }
                if let Some(boq) = &boq {
                    citations.push(Citation {
                        source: boq.source.clone(),
                        clause: format!("BOQ item {}", c.boq_code.as_deref().unwrap_or("unlisted")),
                    });
                }
                if refs.is_empty() {
                    for schedule in schedules
                        .iter()
                        .filter(|s| c.schedule.as_ref() == Some(&s.name))
                    {
                        if schedule.source != 0 {
                            refs.push(Reference {
                                drawing: None,
                                handle: schedule.source,
                                sheet: schedule.sheet.clone(),
                                grid: None,
                            });
                        }
                    }
                }
                c.query = Some(Query {
                    question_en: format!(
                        "Please confirm the correct quantity, unit or scope: {}?",
                        c.reason
                    ),
                    question_ar: format!(
                        "يرجى تأكيد الكمية أو الوحدة أو نطاق العمل الصحيح: {}؟",
                        c.reason_ar
                    ),
                    references: refs,
                    citations,
                    ..Default::default()
                });
            }
            if let Some(old) = self.conflicts.iter().find(|old| {
                old.id == c.id
                    && (!c.hash_key.contains("|second_reading|")
                        || (old.expected == c.expected
                            && old.found == c.found
                            && old.unit == c.unit))
            }) {
                c.status = old.status;
                c.note = old.note.clone();
                c.created = old.created.clone();
            }
        }
        self.raise_queries(conflicts, schedules, source)?;
        Ok(Outcome::Changed)
    }
    /// Atomically replace findings and their clouds, preserving status by stable finding id.
    pub fn raise_queries(
        &mut self,
        mut conflicts: Vec<Conflict>,
        schedules: Vec<crate::Schedule>,
        source: Source,
    ) -> Result<(), CoreError> {
        self.transaction(source, "discrepancy_check", |s| {
            let original_sheet = s.sheet.clone();
            let comments = s.list_comments();
            let old_clouds: Vec<_> = s
                .conflicts
                .iter()
                .filter_map(|c| c.query.as_ref())
                .flat_map(|q| q.clouds.iter().copied())
                .collect();
            for comment in comments.iter().filter(|c| old_clouds.contains(&c.handle)) {
                s.set_sheet(&comment.sheet)?;
                let targets = std::iter::once(comment.handle)
                    .chain(comment.label)
                    .map(|handle| crate::Target {
                        handle,
                        chain: vec![],
                    })
                    .collect();
                s.run_as(Command::DeleteEntities { targets }, source)?;
            }
            for c in &mut conflicts {
                let Some(query) = &mut c.query else { continue };
                query.clouds.clear();
                let mut by_sheet = std::collections::BTreeMap::<String, Vec<u64>>::new();
                for r in &query.references {
                    by_sheet.entry(r.sheet.clone()).or_default().push(r.handle);
                }
                // A BOQ-only finding has no geometry. Place its explicitly labelled query on the current sheet.
                if by_sheet.is_empty() {
                    by_sheet.insert(original_sheet.clone(), vec![]);
                    if c.sheets.is_empty() {
                        c.sheets.push(original_sheet.clone());
                    }
                }
                for (sheet, handles) in by_sheet {
                    let block = s
                        .drawing
                        .sheets()
                        .into_iter()
                        .find(|sh| sh.block == sheet || sh.name == sheet)
                        .map(|sh| sh.block)
                        .ok_or_else(|| invalid(format!("Query sheet not found: {sheet}")))?;
                    s.set_sheet(&block)?;
                    let mut points = vec![];
                    for h in handles {
                        let entity = s
                            .drawing
                            .doc
                            .get_entity(opencadcodec::Handle::new(h))
                            .ok_or_else(|| invalid(format!("Query evidence {h} is missing")))?;
                        let candidate = crate::query::Candidate {
                            entity,
                            doc: &s.drawing.doc,
                            layer: std::borrow::Cow::Owned(entity.common().layer.clone()),
                            xf: qiyas_geom::Xform::identity(),
                            chain: vec![],
                            xref: None,
                        };
                        points.extend(s.points_of(&s.xref_docs(), &candidate));
                    }
                    let bounds = if points.is_empty() {
                        [
                            [0., 0.],
                            [s.default_text_height() * 8., s.default_text_height() * 4.],
                        ]
                    } else {
                        crate::query::bounds(&points)
                    };
                    let pad = s.default_text_height().max(1e-6);
                    let [[x0, y0], [x1, y1]] = bounds;
                    let text =
                        format!("RFI {}\n{}\n{}", c.id, query.question_en, query.question_ar);
                    // This text is derived by the check engine, with evidence in the query.
                    // User-supplied AI/MCP comments still require a take-off citation.
                    let handles =
                        s.record_document_change(source, "comment", true, false, |s| {
                            s.add_comment(
                                vec![
                                    [x0 - pad, y0 - pad],
                                    [x1 + pad, y0 - pad],
                                    [x1 + pad, y1 + pad],
                                    [x0 - pad, y1 + pad],
                                ],
                                &text,
                                None,
                            )
                        })?;
                    query.clouds.push(handles[0]);
                }
            }
            s.set_sheet(&original_sheet)?;
            // Qiyas's own write of the computed result: the agent-source refusal in
            // `SetKnowledge` is for hand-typed results, and the group keeps `source`.
            s.run_as(
                Command::SetKnowledge {
                    kind: knowledge::Kind::Queries,
                    name: "result".into(),
                    data: json!(conflicts),
                },
                Source::User,
            )?;
            s.run_as(
                Command::SetConflicts {
                    conflicts,
                    schedules,
                },
                source,
            )?;
            Ok(())
        })
    }
}
