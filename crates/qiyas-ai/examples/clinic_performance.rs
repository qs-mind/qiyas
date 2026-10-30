// Deterministic clinic tool replay. No provider or credentials are used.
use qiyas_ai::{surface, MeasurementMethod, ToolError, ToolOutput};
#[allow(dead_code)]
#[path = "../src/advice.rs"]
mod advice;
#[path = "../src/churn.rs"]
mod churn;
use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf, time::Instant};

fn seed(dir: &std::path::Path, units: i16) -> Session {
    // A differential run can share the baseline's logical drawing path while writing all
    // evidence to its own output directory. This keeps the context fingerprint comparable.
    let session_dir = std::env::var_os("CLINIC_SESSION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.to_path_buf());
    let mut s = Session::new_blank(&session_dir.join("clinic.dxf"), units);
    s.apply_settings_method(Some(&qiyas_core::measurement::RuleSet::named(
        "POMI (RICS)",
    )))
    .unwrap();
    s
}
fn members(s: &Session) {
    for g in s.wall_groups(None).unwrap() {
        for h in g.handles {
            assert!(
                s.drawing
                    .doc
                    .get_entity(opencadcodec::Handle::new(h))
                    .is_some(),
                "missing wall member {h}"
            );
        }
    }
}
fn rewrite_paths(v: &mut Value, dir: &std::path::Path) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if k == "path" {
                    if let Some(p) = v.as_str() {
                        *v = json!(dir.join(std::path::Path::new(p).file_name().unwrap()));
                    }
                } else {
                    rewrite_paths(v, dir);
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                rewrite_paths(v, dir)
            }
        }
        _ => {}
    }
}

/// The recording predates automatic object layers. Their table records consume handles,
/// but are not recorded entities. Rebase handle fields only, never geometry or revisions.
fn rebase_layer_allocations(value: &mut Value, layers: &[u64]) {
    fn handle(value: &mut Value, layers: &[u64]) {
        if let Some(mut id) = value.as_u64() {
            for &layer in layers {
                if layer <= id {
                    id += 1;
                }
            }
            *value = json!(id);
        } else if let Some(values) = value.as_array_mut() {
            for value in values {
                handle(value, layers);
            }
        } else {
            rebase_layer_allocations(value, layers);
        }
    }
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "handle" | "handles" | "insert" | "inserts" | "centreline"
                ) {
                    handle(value, layers);
                } else {
                    rebase_layer_allocations(value, layers);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                rebase_layer_allocations(value, layers);
            }
        }
        _ => {}
    }
}
/// The one recorded call whose literal handle no longer names what the recording meant: the
/// opening schedule totals each kind (one more table row), which moves every handle made after it.
const HANDLE_SHIFT_CALL: u64 = 83;

pub fn replay(output_dir: Option<&std::path::Path>) -> Vec<std::time::Duration> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let label = std::env::var("CLINIC_LABEL").unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into()
    });
    let dir = output_dir
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::var_os("CLINIC_OUTPUT_DIR").map(PathBuf::from))
        .unwrap_or_else(|| root.join(".local/goal-run").join(&label));
    fs::create_dir_all(&dir).unwrap();
    let mut output = fs::File::create(dir.join("calls.jsonl")).unwrap();
    let mut timings = Vec::new();
    let calls: Vec<Value> =
        serde_json::from_str(include_str!("../tests/fixtures/clinic-performance.json")).unwrap();
    let limit: usize = std::env::var("CLINIC_END")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let loaded = std::env::var("CLINIC_LOAD").ok();
    let mut s = loaded
        .as_ref()
        .map(|p| Session::open_standalone(std::path::Path::new(p)).unwrap())
        .unwrap_or_else(|| seed(&dir, 6));
    let mut handle_shifts: Vec<u64> = Vec::new();
    let mut guard = churn::Guard::default();
    let mut last_turn = 0;
    for call in calls.iter().take(if loaded.is_some() {
        0
    } else {
        limit.saturating_add(1)
    }) {
        let i = call["index"].as_u64().unwrap();
        if call["turn"].as_u64().unwrap() != last_turn {
            guard.reset();
            last_turn = call["turn"].as_u64().unwrap();
        }
        let tool = call["tool"].as_str().unwrap();
        let mut input = call["input"].clone();
        let op = input["op"]
            .as_str()
            .or(input["source"].as_str())
            .unwrap()
            .to_string();
        let mut args = if let Some(a) = input["args"].as_str() {
            serde_json::from_str::<Value>(a).unwrap()
        } else {
            input["args"].clone()
        };
        // Session UUID/path/context are intentionally new; rebase complete revision tokens.
        // Keep the logged stale refusal and malformed token refusal, without disabling checks.
        let expected = input
            .get("expected_revision")
            .filter(|v| !v.is_null())
            .or_else(|| args.get("expected_revision"))
            .cloned();
        if let Some(old) = expected {
            let mut fresh = json!(s.revision());
            if old.get("sheet").is_none() {
                fresh.as_object_mut().unwrap().remove("sheet");
            }
            if call["error"]
                .as_str()
                .is_some_and(|e| e.contains("\"code\":\"stale_read\""))
            {
                fresh["document_revision"] = json!(u64::MAX);
            }
            args["expected_revision"] = fresh;
            input.as_object_mut().unwrap().remove("expected_revision");
        }
        rewrite_paths(&mut args, &fs::canonicalize(&dir).unwrap());
        // These object layers are new since this recording; it contains no authored Q-* layers.
        let mut layer_handles: Vec<_> = s
            .drawing
            .doc
            .layers
            .iter()
            .filter(|layer| {
                matches!(
                    layer.name.as_str(),
                    "Q-LEGACY-WALL" | "Q-LEGACY-DOOR" | "Q-LEGACY-WINDOW" | "Q-LEGACY-ROOM"
                )
            })
            .map(|layer| layer.handle.value())
            .collect();
        layer_handles.sort_unstable();
        rebase_layer_allocations(&mut args, &layer_handles);
        input["args"] = args.clone();
        eprintln!(
            "START {label} call={i} {tool}.{op} live_ms={}",
            call["live_ms"]
        );
        // Small labels now shrink or become linked notes; the recorded failed batch must
        // succeed. Keep every other recorded refusal and rollback assertion unchanged.
        let label_fit_refusal = call["error"]
            .as_str()
            .and_then(|e| serde_json::from_str::<Value>(e).ok())
            .is_some_and(|e| {
                e["command"] == "label_spaces"
                    && e["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("The room is too small for"))
            });
        // Undo keeps the handle counter moving on, so the refused state is restored whole.
        let refused_doc = label_fit_refusal.then(|| s.drawing.doc.clone());
        // The untouched snapshot is for rollback evidence, outside the timed interval.
        let before_structure = s.drawing.doc.snapshot_structure();
        let before_entities: BTreeMap<_, _> = s
            .drawing
            .doc
            .entities()
            .map(|e| (e.common().handle.value(), json!(e)))
            .collect();
        let before_history = json!(s.history_steps());
        let before_revision = s.document_revision();
        let profiling = std::env::var_os("CLINIC_PROFILE").is_some();
        if profiling {
            qiyas_core::fixtures::profile::begin();
        }
        let profile_span = qiyas_core::fixtures::profile::span("clinic.call");
        let start = Instant::now();
        let (result, host_note) = match op.as_str() {
            "new_drawing" => {
                s = seed(&dir, 4);
                (
                    Ok(ToolOutput::Json(json!({"new_drawing":"mm"}))),
                    Some("host emulated: blank millimetre session plus Settings method"),
                )
            }
            "drawings" => (
                Ok(ToolOutput::Json(json!({"drawings":s.info()}))),
                Some("host tab list emulated"),
            ),
            "save_drawing" => (
                Ok(ToolOutput::Json(
                    json!({"skipped":"desktop Save As dialog"}),
                )),
                Some("dialog omitted; no drawing mutation"),
            ),
            _ => (
                surface::route(tool, &input)
                    .map_err(ToolError::BadInput)
                    .and_then(|r| match r {
                        surface::Route::Session { tool, op, args } => guard.run(
                            &mut s,
                            &tool,
                            &op,
                            &args,
                            Source::Ai,
                            &MeasurementMethod::default(),
                        ),
                        _ => panic!("unhandled host route {i} {op}"),
                    }),
                None,
            ),
        };
        if i == 4 {
            guard.set_phase("setup");
        }
        let elapsed = start.elapsed();
        timings.push(elapsed);
        let seconds = elapsed.as_secs_f64();
        drop(profile_span);
        let profile = if profiling {
            json!(qiyas_core::fixtures::profile::finish())
        } else {
            json!(null)
        };
        let (value, error, image_bytes) = match result {
            Ok(ToolOutput::Json(v)) => (Some(v), None, 0),
            Ok(ToolOutput::Image { png, caption }) => {
                let bytes = png.len();
                fs::write(dir.join(format!("{i:03}.png")), png).unwrap();
                (Some(json!({"caption":caption})), None, bytes)
            }
            Err(e) => (None, Some(e.json()), 0),
        };
        let expected_error = !call["error"].is_null() && !label_fit_refusal;
        if label_fit_refusal && error.is_none() {
            // The recording continued from the refused state: take the step back so the later
            // recorded handles still name the same entities.
            assert!(s.undo(), "the label step is undoable at call {i}");
            s.drawing.doc = refused_doc.expect("saved before the label step");
        }
        let rollback_equal = if error.is_some() {
            Some(
                before_structure == s.drawing.doc.snapshot_structure()
                    && before_revision == s.document_revision()
                    && before_history == json!(s.history_steps())
                    && before_entities
                        == s.drawing
                            .doc
                            .entities()
                            .map(|e| (e.common().handle.value(), json!(e)))
                            .collect(),
            )
        } else {
            None
        };
        let row = json!({"index":i,"turn":call["turn"],"request":call["request"],"tool":tool,"op":op,"live_ms":call["live_ms"],"seconds":seconds,"error":error,"expected_error":expected_error,"host_note":host_note,"rollback_equal":rollback_equal,"entities":s.drawing.doc.entities().count(),"result_bytes":value.as_ref().map(|v|v.to_string().len()),"image_bytes":image_bytes,"profile":profile});
        writeln!(output, "{row}").unwrap();
        output.flush().unwrap();
        fs::write(
            dir.join(format!("{i:03}-result.json")),
            serde_json::to_vec_pretty(&json!({"result":value,"error":error})).unwrap(),
        )
        .unwrap();
        eprintln!("DONE {label} call={i} seconds={seconds:.6} error={} entities={} rollback={rollback_equal:?}",error.as_ref().map(|e|e.to_string()).unwrap_or_default(),s.drawing.doc.entities().count());
        // A recorded refusal of the input's shape alone (fields the registry did not know at
        // the time) may now be normalised and accepted: Goal A wraps top-level command fields.
        let shape_refusal = call["error"]
            .as_str()
            .and_then(|e| serde_json::from_str::<Value>(e).ok())
            .is_some_and(|e| {
                e["errors"].as_array().is_some_and(|errs| {
                    !errs.is_empty()
                        && errs.iter().all(|x| {
                            matches!(x["code"].as_str(), Some("missing_field" | "unknown_field"))
                        })
                })
            });
        // W1: an export of quantities whose check failed is now refused unless named DRAFT.
        let export_gate = error
            .as_ref()
            .is_some_and(|e| e["code"] == "quantity_check_failed");
        // W3: the opening schedule totals each kind (one more row, so more table cells), which
        // moves every handle made after it. A recorded literal handle that now names something
        // else is refused as not what the recording meant; the batch is put back whole (the
        // rollback check below still holds). Regenerate the fixture's handles to exercise it.
        let handle_shift = i == HANDLE_SHIFT_CALL
            && !expected_error
            && error.as_ref().is_some_and(|e| {
                e["message"].as_str().is_some_and(|m| {
                    m.contains("is not a wall opening") || m.contains("No such entity")
                })
            });
        if handle_shift {
            handle_shifts.push(i);
            eprintln!(
                "NOTE call {i}: a recorded handle now names another entity (W3 per-kind totals moved the handles after the first opening schedule)"
            );
        } else if error.is_none() && expected_error && shape_refusal {
            eprintln!(
                "NOTE call {i}: the recorded input-shape refusal is now normalised and accepted"
            );
        } else if export_gate && !expected_error {
            eprintln!("NOTE call {i}: the export is now refused while the quantity check fails");
        } else {
            assert_eq!(
                error.is_some(),
                expected_error,
                "unexpected replay outcome at call {i}"
            );
        }
        assert_ne!(
            rollback_equal,
            Some(false),
            "rollback changed state at call {i}"
        );
        members(&s);
        let entities: BTreeMap<_, _> = s
            .drawing
            .doc
            .entities()
            .map(|e| (e.common().handle.value(), json!(e)))
            .collect();
        let snapshot = json!({"entities":entities,"structure":s.drawing.doc.snapshot_structure(),"objects":s.objects(&Default::default()).unwrap(),"measurement":s.measurement_report().unwrap(),"takeoff":s.takeoff});
        fs::write(
            dir.join(format!("{i:03}-state.json")),
            serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
    }
    // The tolerance fires at its one call and nowhere else, whole replays only.
    if loaded.is_none() && limit >= HANDLE_SHIFT_CALL as usize {
        assert_eq!(
            handle_shifts,
            [HANDLE_SHIFT_CALL],
            "the handle tolerance must fire exactly once, at call {HANDLE_SHIFT_CALL}"
        );
    }
    if std::env::var("CLINIC_SAVE_FINAL").is_ok() {
        s.save_as_headless(&dir.join("final-research.dxf")).unwrap();
    }
    fs::write(
        dir.join("final-groups.json"),
        serde_json::to_vec_pretty(&s.wall_groups(None).unwrap()).unwrap(),
    )
    .unwrap();
    timings
}

#[allow(dead_code)]
fn main() {
    replay(None);
}
