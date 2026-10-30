//! Track replacement of this caller's work, independently of failed-edit retries.
use std::collections::{BTreeMap, BTreeSet};

use qiyas_core::{Session, Source};
use serde_json::{json, Value};

use crate::{surface, MeasurementMethod, ToolError, ToolOutput};

type Bounds = [[f64; 2]; 2];
#[derive(Clone)]
struct Shape {
    family: String,
    sheet: String,
    bounds: Bounds,
}
#[derive(Default)]
struct Phase {
    own: BTreeMap<u64, (Shape, usize)>,
    removed: Vec<(Shape, usize)>,
    added_total: usize,
    removed_total: usize,
}
#[derive(Default)]
pub(crate) struct Guard {
    epoch: Option<u64>,
    phase: String,
    phases: BTreeMap<String, Phase>,
}

fn snapshot(s: &Session) -> BTreeSet<u64> {
    s.drawing
        .sheets()
        .iter()
        .flat_map(|sheet| s.drawing.entities_in(&sheet.block))
        .map(|e| e.common().handle.value())
        .collect()
}
fn shapes(s: &Session, handles: Vec<u64>) -> BTreeMap<u64, Shape> {
    let mut result = BTreeMap::new();
    if handles.is_empty() {
        return result;
    }
    let mut query = qiyas_core::records::RecordQuery {
        handles,
        sheet: s.automation_sheet.clone(),
        limit: Some(200),
        ..Default::default()
    };
    while let Ok(page) = s.records(&query) {
        for row in page.records {
            if !row["xref"].is_null() {
                continue;
            }
            let Some(handle) = qiyas_core::registry::handle_number(&row["key"]) else {
                continue;
            };
            let Ok(bounds) = serde_json::from_value(row["bounds"].clone()) else {
                continue;
            };
            let family = match row["type"].as_str().unwrap_or_default() {
                "Text" | "MText" => "text",
                "Dimension" => "dimension",
                // Wall systems and hand-drawn outlines are the same visible geometry.
                "Line" | "LwPolyline" | "Polyline2D" | "Hatch" => "geometry",
                other => other,
            }
            .to_string();
            result.insert(
                handle,
                Shape {
                    family,
                    sheet: row["sheet"].as_str().unwrap_or_default().into(),
                    bounds,
                },
            );
        }
        let Some(next) = page.next_offset else { break };
        query.offset = next;
    }
    result
}
fn similar(a: &Shape, b: &Shape, tolerance: f64) -> bool {
    a.family == b.family
        && a.sheet == b.sheet
        && (0..2).all(|i| {
            a.bounds[0][i] <= b.bounds[1][i] + tolerance
                && b.bounds[0][i] <= a.bounds[1][i] + tolerance
        })
}
fn deletion_handles(value: &Value, out: &mut BTreeSet<u64>) {
    let mut value = match value {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        v => v.clone(),
    };
    qiyas_core::registry::canonicalize(&mut value);
    if value["type"] == "delete_entities" {
        for target in value["targets"].as_array().into_iter().flatten() {
            if let Some(h) = qiyas_core::registry::handle_number(target)
                .or_else(|| qiyas_core::registry::handle_number(&target["handle"]))
            {
                out.insert(h);
            }
        }
    }
    for key in ["command", "commands"] {
        if let Some(values) = value[key].as_array() {
            for v in values {
                deletion_handles(v, out);
            }
        } else if !value[key].is_null() {
            deletion_handles(&value[key], out);
        }
    }
}
impl Guard {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn set_phase(&mut self, id: &str) {
        self.phase = id.into();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &mut self,
        session: &mut Session,
        tool: &str,
        op: &str,
        args: &Value,
        source: Source,
        method: &MeasurementMethod,
    ) -> Result<ToolOutput, ToolError> {
        if self.epoch != Some(session.epoch()) {
            let phase = std::mem::take(&mut self.phase);
            self.reset();
            self.phase = phase;
            self.epoch = Some(session.epoch());
        }
        if tool != surface::EXECUTE || matches!(op, "plan" | "view" | "select") {
            return surface::run_session_with_method(session, tool, op, args, source, method);
        }
        let before = snapshot(session);
        let phase = self.phases.entry(self.phase.clone()).or_default();
        let typed = if op == "type" {
            match qiyas_core::cmdline::Machine::default()
                .step(args["line"].as_str().unwrap_or_default())
            {
                Ok(qiyas_core::cmdline::Step::Command(c)) => json!(c),
                _ => Value::Null,
            }
        } else {
            Value::Null
        };
        let undo = op == "undo_own"
            || (op == "type"
                && matches!(
                    args["line"]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .to_uppercase()
                        .as_str(),
                    "UNDO" | "U"
                ));
        let mut removing = BTreeSet::new();
        if undo {
            removing.extend(session.undo_added_handles(source));
        }
        deletion_handles(args, &mut removing);
        deletion_handles(&typed, &mut removing);
        let protected: Vec<_> = removing
            .iter()
            .filter_map(|h| {
                phase
                    .own
                    .get(h)
                    .filter(|(_, n)| *n > 0)
                    .map(|(s, _)| (*h, s.family.as_str()))
            })
            .collect();
        let advice = if !protected.is_empty() {
            Some(
                json!({"code":"repeated_rebuild","message":"This work was already rebuilt in this phase. / تمت إعادة إنشاء هذا العمل في هذه المرحلة بالفعل.","handles":protected.iter().map(|(h,_)| h).collect::<Vec<_>>()}),
            )
        } else {
            None
        };
        let mut output =
            match surface::run_session_with_method(session, tool, op, args, source, method) {
                Ok(output) => output,
                Err(error) => {
                    return if let Some(advice) = advice {
                        crate::advice::result(Err(error), advice)
                    } else {
                        Err(error)
                    }
                }
            };
        let after = snapshot(session);
        let removed: Vec<_> = before.difference(&after).copied().collect();
        let added: Vec<_> = after.difference(&before).copied().collect();
        // Only explicit removal counts as a rebuild. An edit may replace implementation
        // entities while retaining the requested object; that is an honest correction.
        if undo || !removing.is_empty() {
            for h in &removed {
                if let Some((shape, generation)) = phase.own.remove(h) {
                    phase.removed.push((shape, generation));
                }
            }
        }
        let current = shapes(
            session,
            added.iter().chain(phase.own.keys()).copied().collect(),
        );
        let tolerance = 0.05 / session.drawing.units.to_metres();
        let mut footprints = BTreeMap::<String, Bounds>::new();
        for h in &added {
            let Some(shape) = current.get(h) else {
                continue;
            };
            footprints
                .entry(shape.family.clone())
                .and_modify(|bounds| {
                    bounds[0] = std::array::from_fn(|i| bounds[0][i].min(shape.bounds[0][i]));
                    bounds[1] = std::array::from_fn(|i| bounds[1][i].max(shape.bounds[1][i]));
                })
                .or_insert(shape.bounds);
        }
        for h in &added {
            let Some(mut shape) = current.get(h).cloned() else {
                continue;
            };
            shape.bounds = footprints[&shape.family];
            let generation = phase
                .removed
                .iter()
                .filter(|(old, _)| similar(old, &shape, tolerance))
                .map(|(_, n)| n + 1)
                .max()
                .unwrap_or(0);
            phase.own.insert(*h, (shape, generation));
        }
        for (h, (shape, _)) in &mut phase.own {
            if !added.contains(h) {
                if let Some(current) = current.get(h) {
                    *shape = current.clone();
                }
            }
        }
        phase.added_total += added.len();
        phase.removed_total += removed.len();
        if let ToolOutput::Json(value) = &mut output {
            value["phase_churn"] = json!({"added":phase.added_total,"removed":phase.removed_total,"total":phase.added_total+phase.removed_total});
            if let Some(mut advice) = advice {
                advice["churn"] = json!({"added":added.len(),"removed":removed.len(),"total":added.len()+removed.len()});
                crate::advice::append(value, advice);
            }
            value["churn"] = json!({"added":added.len(),"removed":removed.len(),"total":added.len()+removed.len()});
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn run(g: &mut Guard, s: &mut Session, c: Value) -> Result<ToolOutput, ToolError> {
        g.run(
            s,
            surface::EXECUTE,
            "run",
            &json!({"command":c}),
            Source::Ai,
            &MeasurementMethod::default(),
        )
    }
    fn text_at(x: f64, k: f64) -> Value {
        json!({"type":"text","at":[x*k,0],"height":0.2*k,"value":"Name"})
    }
    fn handle(out: ToolOutput) -> u64 {
        if let ToolOutput::Json(v) = out {
            v["added"][0].as_u64().unwrap()
        } else {
            panic!()
        }
    }
    #[test]
    fn own_rebuilds_are_scoped_by_phase_and_overlapping_bounds() {
        let dir = tempfile::tempdir().unwrap();
        for (units, k) in [(4, 1000.0), (5, 100.0), (6, 1.0), (1, 1.0 / 0.0254)] {
            let mut s = Session::new_blank(&dir.path().join("phase.dxf"), units);
            let mut g = Guard::default();
            g.set_phase("walls");
            let old = handle(run(&mut g, &mut s, text_at(0.0, k)).unwrap());
            run(
                &mut g,
                &mut s,
                json!({"type":"delete_entities","targets":[old]}),
            )
            .unwrap();
            let rebuilt = handle(run(&mut g, &mut s, text_at(0.0, k)).unwrap());
            run(
                &mut g,
                &mut s,
                json!({"type":"edit_text","target":rebuilt,"value":"Corrected"}),
            )
            .unwrap();
            let revision = s.document_revision();
            let ToolOutput::Json(output) = run(
                &mut g,
                &mut s,
                json!({"type":"delete_entities","targets":[rebuilt]}),
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(output["advice"][0]["code"], "repeated_rebuild");
            assert_eq!(output["churn"]["removed"], 1);
            assert!(s.document_revision() > revision);
            assert!(s
                .drawing
                .doc
                .get_entity(opencadcodec::Handle::new(rebuilt))
                .is_none());
            let rebuilt = handle(run(&mut g, &mut s, text_at(0.0, k)).unwrap());
            let other = handle(run(&mut g, &mut s, text_at(50.0, k)).unwrap());
            run(
                &mut g,
                &mut s,
                json!({"type":"delete_entities","targets":[other]}),
            )
            .unwrap();
            // Switching away and back cannot erase the phase's replacement history.
            g.set_phase("other");
            g.set_phase("walls");
            let ToolOutput::Json(output) = run(
                &mut g,
                &mut s,
                json!({"type":"delete_entities","targets":[rebuilt]}),
            )
            .unwrap() else {
                panic!()
            };
            assert_eq!(output["advice"][0]["code"], "repeated_rebuild");
            g.reset();
            let rebuilt = handle(run(&mut g, &mut s, text_at(0.0, k)).unwrap());
            run(
                &mut g,
                &mut s,
                json!({"type":"delete_entities","targets":[rebuilt]}),
            )
            .unwrap();
        }
    }
}
