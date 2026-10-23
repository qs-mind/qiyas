//! Numerical reproduction from submitted evidence and drawing-owned rules, never truth.
use std::collections::BTreeMap;

use qiyas_core::{measurement::Citation, Session};
use serde::{Deserialize, Serialize};

use super::model::{objects, unit, Item, Submission};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemTrace {
    pub index: usize,
    pub id: String,
    pub traceable: bool,
    pub reason: String,
}

fn reproduces(actual: f64, expected: f64, count: bool) -> bool {
    actual.is_finite()
        && expected.is_finite()
        && if count || expected == 0.0 {
            actual == expected
        } else {
            (actual - expected).abs() <= expected.abs() * 0.005
        }
}

fn exists(session: &Session, handle: u64) -> Result<(), String> {
    session
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(handle))
        .map(|_| ())
        .ok_or_else(|| format!("missing drawing handle {handle:X}"))
}

fn source_handle(source: &str) -> Result<u64, String> {
    // PDF citations retain the embedded calibrated driver's handle, not an original CAD ID.
    let text = source.rsplit_once(";object=").map_or(source, |(_, h)| h);
    u64::from_str_radix(text.trim_start_matches("0x").trim_start_matches("0X"), 16)
        .map_err(|_| format!("unresolved source object {source}"))
}

fn note_citation(session: &Session, citation: &Citation, required: bool) -> Result<(), String> {
    let text = format!("{} {}", citation.source, citation.clause).to_lowercase();
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric() && c != '.')
        .filter(|s| !s.is_empty())
        .collect();
    let mut mentions_note = false;
    let mut found_handle = false;
    for (i, word) in words.iter().enumerate() {
        if !matches!(*word, "note" | "notes") {
            continue;
        }
        mentions_note = true;
        let tail = &words[i + 1..];
        let tail = if tail.first() == Some(&"handle") {
            &tail[1..]
        } else {
            tail
        };
        for token in tail {
            let token = token.trim_end_matches('.');
            let handle = if let Some(hex) = token.strip_prefix("0x") {
                u64::from_str_radix(hex, 16).ok()
            } else {
                token.parse::<u64>().ok()
            };
            if let Some(handle) = handle {
                exists(session, handle)
                    .map_err(|_| format!("missing cited note handle {handle:X}"))?;
                found_handle = true;
            } else if token != "and" {
                break;
            }
        }
    }
    if mentions_note && !found_handle && required {
        return Err("drawing note citation lacks a note handle".into());
    }
    Ok(())
}

fn geometric(session: &Session, item: &Item) -> Result<f64, String> {
    if item.basis != "geometric" {
        return Err("no saved dimension recipe for cited rule".into());
    }
    let mut total = 0.0;
    let factor = session.drawing.units.to_metres();
    for source in objects(item) {
        let handle = source_handle(&source)?;
        exists(session, handle)?;
        let info = session
            .entity_info(handle, &[])
            .map_err(|e| e.to_string())?;
        total += match item.kind.as_str() {
            "count" => 1.0,
            "length" => info.length.ok_or("source has no measurable length")? * factor,
            "area" => info.area.ok_or("source has no measurable area")? * factor * factor,
            "volume" => {
                info.area.ok_or("source has no measurable area")?
                    * factor
                    * factor
                    * item.height_m.ok_or("volume needs cited height_m")?
            }
            _ => return Err("unsupported geometric kind".into()),
        };
    }
    Ok(total)
}

fn dimension(
    session: &Session,
    item: &Item,
    fresh: &Result<Submission, String>,
) -> Result<f64, String> {
    let fresh = fresh.as_ref().map_err(Clone::clone)?;
    let sources = objects(item);
    let candidates: Vec<_> = fresh
        .items
        .iter()
        .filter(|q| {
            q.aggregate_of.is_empty()
                && (q.id == item.id || (q.rule == item.rule && objects(q) == sources))
        })
        .collect();
    let [q] = candidates.as_slice() else {
        return Err("missing or ambiguous saved dimension recipe".into());
    };
    if q.rule != item.rule
        || objects(q) != sources
        || q.kind != item.kind
        || unit(&q.unit) != unit(&item.unit)
        || q.basis != item.basis
        || q.citation != item.citation
    {
        return Err("dimension rule, sources, citation or unit differs from saved drawing".into());
    }
    // A rule may name its note in its basis, its main citation or a dimension citation.
    let citations = q
        .citation
        .iter()
        .chain(q.dimension_lines.iter().flat_map(|l| &l.citations))
        .map(|c| format!("{} {}", c.source, c.clause))
        .collect::<Vec<_>>()
        .join(" ");
    note_citation(
        session,
        &Citation {
            source: item.basis.clone(),
            clause: citations,
        },
        item.basis != "geometric",
    )?;
    if item.dimension_lines.len() != q.dimension_lines.len() || q.dimension_lines.is_empty() {
        return Err("missing or incomplete dimension lines".into());
    }
    for (saved, derived) in item.dimension_lines.iter().zip(&q.dimension_lines) {
        if saved.sources != derived.sources
            || saved.source_expressions != derived.source_expressions
            || saved.operation != derived.operation
            || saved.included != derived.included
            || saved.unit != derived.unit
            || saved.citations != derived.citations
            || saved.method != derived.method
            || !reproduces(saved.value, derived.value, item.kind == "count")
            || !reproduces(saved.timesing, derived.timesing, item.kind == "count")
            || saved.factors.len() != derived.factors.len()
            || !saved
                .factors
                .iter()
                .zip(&derived.factors)
                .all(|(a, b)| reproduces(*a, *b, item.kind == "count"))
        {
            return Err("dimension line differs from re-evaluated rule on geometry".into());
        }
        for source in &saved.sources {
            exists(session, source.handle)?;
            for evidence in &source.evidence {
                if let Some(handle) = evidence.handle {
                    exists(session, handle)?;
                }
            }
        }
    }
    Ok(q.quantity)
}

pub fn submission(submission: &Submission, session: Option<&Session>) -> Vec<ItemTrace> {
    let fresh = session
        .ok_or_else(|| "drawing unavailable to remeasure".to_string())
        .and_then(|s| {
            s.remeasure_report()
                .map_err(|e| e.to_string())
                .and_then(|r| {
                    super::submission::from_report(
                        &r,
                        &s.recognition_evidence().map_err(|e| e.to_string())?,
                        &s.drawing,
                    )
                })
        });
    let aggregates: BTreeMap<_, _> = (0..submission.items.len())
        .filter_map(|i| {
            super::submission::aggregate_components(&submission.items, i).map(|c| (i, c))
        })
        .collect();
    let mut results: Vec<_> = submission
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let result = (|| {
                let session = session.ok_or("drawing unavailable to remeasure")?;
                if item.source_objects.is_empty() {
                    return Err("no cited handles".into());
                }
                for source in &item.source_objects {
                    exists(session, source_handle(source)?)?;
                }
                let expected_unit = match item.kind.as_str() {
                    "count" => "pcs",
                    "length" => "m",
                    "area" => "m2",
                    "volume" => "m3",
                    _ => "",
                };
                if unit(&item.unit) != expected_unit {
                    return Err("quantity kind and unit differ".into());
                }
                let value = if item.rule.is_some() || !item.dimension_lines.is_empty() {
                    dimension(session, item, &fresh)?
                } else {
                    geometric(session, item)?
                };
                if !reproduces(item.quantity, value, item.kind == "count") {
                    return Err(
                        "re-evaluated geometry/rule does not reproduce submitted quantity".into(),
                    );
                }
                Ok(())
            })();
            ItemTrace {
                index,
                id: item.id.clone(),
                traceable: result.is_ok(),
                reason: result
                    .err()
                    .unwrap_or_else(|| "reproduced from saved drawing geometry and rule".into()),
            }
        })
        .collect();
    for (i, item) in submission
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.aggregate_of.is_empty())
    {
        let result = aggregates
            .get(&i)
            .ok_or_else(|| "incomplete or inconsistent aggregate components".to_string())
            .and_then(|parts| {
                parts
                    .iter()
                    .find(|p| !results[**p].traceable)
                    .map_or(Ok(()), |p| {
                        Err(format!(
                            "component {}: {}",
                            submission.items[*p].id, results[*p].reason
                        ))
                    })
            });
        results[i].traceable = result.is_ok();
        results[i].reason = result.err().unwrap_or_else(|| {
            format!(
                "all {} dimension components reproduced",
                item.aggregate_of.len()
            )
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use qiyas_core::{Outcome, Source};
    use serde_json::json;

    fn fixture() -> (tempfile::TempDir, Session, Submission, u64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawing.dxf");
        let mut s = Session::new_blank(&path, 6);
        let Outcome::Added { handles } = s
            .run_input(
                &json!({"type":"text","at":[0,8],"height":0.2,"value":"Surface factor 2.4 m"}),
                Source::User,
            )
            .unwrap()
            .outcome
        else {
            panic!("note")
        };
        let note = handles[0];
        let citation =
            json!({"source":format!("drawing note {note}"),"clause":"Surface factor 2.4 m"});
        s.run_input(&json!({"type":"batch","commands":[
            {"type":"draw","kind":"rectangle","a":[0,0],"b":[3,2]},
            {"type":"draw","kind":"rectangle","a":[5,0],"b":[7,2]},
            {"type":"define_object_type","definition":{"name":"Observed","geometry":"region","category":"test"}},
            {"type":"recognition","kind":"define","rule":{"name":"observed","object_type":"Observed","match":{"entity_type":"LwPolyline","closed_region":true},"confidence":0.99}},
            {"type":"recognition","kind":"apply"},
            {"type":"measurement","kind":"set_method","scope":"project","method":{"name":"Own method","title":"Own method","based_on":"drawing","source_note":"Cited surface factor","rules":[
                {"id":"surface","element_type":"Observed","kind":"area","basis":"rule:my-surface","unit":"m2","terms":[{"label":"Surface","operation":"add","factors":["geometry.length","2.4"],"citation":citation}],"citation":citation},
                {"id":"count","element_type":"Observed","kind":"count","basis":"geometric","unit":"pcs","terms":[{"label":"Count","operation":"add","factors":["geometry.count"],"citation":citation}],"citation":citation}
            ]}},
            {"type":"measurement","kind":"set_mappings","mappings":[
                {"rule":"surface","code":"TOTAL","description_en":"Surface","description_ar":"Surface","section":"Test","trade":"","unit":"m2","citation":citation}
            ]}
        ]}), Source::User).unwrap();
        let submitted = super::super::submission::from_session(&s).unwrap();
        s.save_as(&path).unwrap();
        let reopened = Session::open_embedded(&path).unwrap();
        (dir, reopened, submitted, note)
    }

    #[test]
    fn saved_rule_and_aggregate_reproduce_without_truth_rules() {
        let (_dir, s, submitted, _) = fixture();
        let traces = submission(&submitted, Some(&s));
        assert!(traces.iter().all(|t| t.traceable), "{traces:?}");
        let truth = super::super::model::Truth {
            schema_version: 1,
            id: "test".into(),
            project_type: "test".into(),
            drawing: "drawing.dxf".into(),
            items: vec![submitted.items[0].clone()],
            rules: Default::default(),
            seeded_errors: vec![],
        };
        let score = super::super::score::score(&truth, &submitted, Some(&s));
        assert_eq!(score.rates["traceability_all"], 1.0);
        assert_eq!(score.rates["traceability_submitted"], 1.0);
        assert!(score.metrics.extras > 0);
    }

    #[test]
    fn tampered_quantity_and_aggregate_do_not_trace() {
        let (_dir, s, mut submitted, _) = fixture();
        let i = submitted
            .items
            .iter()
            .position(|i| i.rule.as_deref() == Some("surface"))
            .unwrap();
        submitted.items[i].quantity *= 1.02;
        let traces = submission(&submitted, Some(&s));
        assert!(!traces[i].traceable);
        assert!(!traces.iter().find(|t| t.id == "TOTAL").unwrap().traceable);
        let count = submitted
            .items
            .iter_mut()
            .find(|i| i.kind == "count")
            .unwrap();
        count.quantity += 0.000001;
        let id = count.id.clone();
        assert!(
            !submission(&submitted, Some(&s))
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .traceable
        );
    }

    #[test]
    fn missing_geometry_or_note_handles_do_not_trace() {
        let (_dir, mut s, submitted, note) = fixture();
        // Delete directly so the cached report is deliberately left intact.
        s.drawing
            .doc
            .remove_entity_arc(opencadcodec::Handle::new(note));
        let traces = submission(&submitted, Some(&s));
        assert!(traces.iter().all(|t| !t.traceable), "{traces:?}");
        assert!(traces[0].reason.contains("note handle"));
        let mut broken = submitted.clone();
        broken.items[0].source_objects = vec!["FFFFFFFF".into()];
        let traces = submission(&broken, Some(&s));
        assert!(!traces[0].traceable);
        assert!(traces[0].reason.contains("missing drawing handle"));
    }

    #[test]
    fn stale_cached_report_cannot_prove_changed_geometry() {
        let (_dir, mut s, submitted, _) = fixture();
        let handle = source_handle(&submitted.items[0].source_objects[0]).unwrap();
        let entity = s
            .drawing
            .doc
            .get_entity_mut(opencadcodec::Handle::new(handle))
            .unwrap();
        let opencadcodec::entities::EntityType::LwPolyline(polyline) = entity else {
            panic!("region")
        };
        polyline.vertices[1].location.x += 4.0;
        let traces = submission(&submitted, Some(&s));
        assert!(!traces[0].traceable, "{traces:?}");
    }

    #[test]
    fn rule_note_needs_a_handle_but_can_name_it_in_the_basis() {
        let (_dir, s, _, note) = fixture();
        let mut citation = Citation {
            source: "rule:surface".into(),
            clause: "drawing material note".into(),
        };
        assert!(note_citation(&s, &citation, true).is_err());
        citation.source = format!("rule:note {note} surface");
        assert!(note_citation(&s, &citation, true).is_ok());
    }

    #[test]
    fn aggregate_counts_require_exact_reproduction() {
        let (_dir, s, mut submitted, _) = fixture();
        let counts: Vec<_> = submitted
            .items
            .iter()
            .filter(|i| i.kind == "count")
            .cloned()
            .collect();
        let mut total = counts[0].clone();
        total.id = "COUNT-TOTAL".into();
        total.aggregate_of = counts.iter().map(|i| i.id.clone()).collect();
        total.source_objects = counts
            .iter()
            .flat_map(|i| i.source_objects.clone())
            .collect();
        total.dimension_lines = counts
            .iter()
            .flat_map(|i| i.dimension_lines.clone())
            .collect();
        total.quantity = counts.iter().map(|i| i.quantity).sum();
        submitted.items.push(total);
        assert!(submission(&submitted, Some(&s)).last().unwrap().traceable);
        submitted.items.last_mut().unwrap().quantity += 1e-12;
        assert!(!submission(&submitted, Some(&s)).last().unwrap().traceable);
    }

    #[test]
    fn embedded_open_refuses_linked_drawings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drawing.dxf");
        let mut session = Session::new_blank(&path, 6);
        let doc = &mut session.drawing.doc;
        let mut block = opencadcodec::BlockRecord::new("external");
        block.handle = doc.allocate_handle();
        block.block_entity_handle = doc.allocate_handle();
        block.block_end_handle = doc.allocate_handle();
        block.flags.is_xref = true;
        block.xref_path = "must-not-open.dxf".into();
        doc.block_records.add(block).unwrap();
        doc.add_entity(opencadcodec::EntityType::Insert(
            opencadcodec::entities::Insert::new(
                "external",
                opencadcodec::Vector3::new(0.0, 0.0, 0.0),
            ),
        ))
        .unwrap();
        session.save_as(&path).unwrap();
        assert!(Session::open_embedded(&path).is_err());
    }
}
