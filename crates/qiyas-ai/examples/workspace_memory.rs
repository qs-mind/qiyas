//! Three offline clinic replays in one process. No provider, credentials or cloud calls.
//! Usage: workspace_memory FIXTURE_JSON OUTPUT_DIRECTORY [LAST_CALL]
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
    let mut s = Session::new_blank(&dir.join("clinic.dxf"), units);
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
fn replay(fixture: &str, dir: PathBuf, limit: usize, ready: std::sync::Arc<std::sync::Barrier>) {
    let label = dir.file_name().unwrap().to_string_lossy().to_string();
    fs::create_dir_all(&dir).unwrap();
    let mut output = fs::File::create(dir.join("calls.jsonl")).unwrap();
    let calls: Vec<Value> = serde_json::from_str(fixture).unwrap();
    ready.wait();
    let mut s = seed(&dir, 6);
    let mut guard = churn::Guard::default();
    let mut last_turn = 0;
    for call in calls.iter().take(limit.saturating_add(1)) {
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
        rewrite_paths(&mut args, &dir);
        input["args"] = args.clone();
        eprintln!(
            "START {label} call={i} {tool}.{op} live_ms={}",
            call["live_ms"]
        );
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
        let seconds = start.elapsed().as_secs_f64();
        let (value, error, image_bytes) = match result {
            Ok(ToolOutput::Json(v)) => (Some(v), None, 0),
            Ok(ToolOutput::Image { png, caption }) => {
                let bytes = png.len();
                fs::write(dir.join(format!("{i:03}.png")), png).unwrap();
                (Some(json!({"caption":caption})), None, bytes)
            }
            Err(e) => (None, Some(e.json()), 0),
        };
        let expected_error = !call["error"].is_null();
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
        let row = json!({"index":i,"turn":call["turn"],"request":call["request"],"tool":tool,"op":op,"live_ms":call["live_ms"],"seconds":seconds,"error":error,"expected_error":expected_error,"host_note":host_note,"rollback_equal":rollback_equal,"entities":s.drawing.doc.entities().count(),"result_bytes":value.as_ref().map(|v|v.to_string().len()),"image_bytes":image_bytes});
        writeln!(output, "{row}").unwrap();
        output.flush().unwrap();
        fs::write(
            dir.join(format!("{i:03}-result.json")),
            serde_json::to_vec_pretty(&json!({"result":value,"error":error})).unwrap(),
        )
        .unwrap();
        eprintln!("DONE {label} call={i} seconds={seconds:.6} error={} entities={} rollback={rollback_equal:?}",error.as_ref().map(|e|e.to_string()).unwrap_or_default(),s.drawing.doc.entities().count());
        assert_eq!(
            error.is_some(),
            expected_error,
            "unexpected replay outcome at call {i}"
        );
        assert_ne!(
            rollback_equal,
            Some(false),
            "rollback changed state at call {i}"
        );
        members(&s);
    }
    s.save_as_headless(&dir.join("final.dxf")).unwrap();
    fs::write(
        dir.join("final-groups.json"),
        serde_json::to_vec_pretty(&s.wall_groups(None).unwrap()).unwrap(),
    )
    .unwrap();
}

fn memory_kib(field: &str, path: &str) -> u64 {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix(field)?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(args.len() >= 3, "Provide a fixture and output directory");
    let fixture = fs::read_to_string(&args[1]).unwrap();
    let out = PathBuf::from(&args[2]);
    fs::create_dir_all(&out).unwrap();
    let limit = args.get(3).map_or(84, |value| value.parse().unwrap());
    let available = memory_kib("MemAvailable:", "/proc/meminfo");
    let budget = available.saturating_sub(1024 * 1024);
    let start = Instant::now();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let workers: Vec<_> = (1..=3)
        .map(|index| {
            let fixture = fixture.clone();
            let dir = out.join(format!("drawing-{index}"));
            let ready = barrier.clone();
            std::thread::spawn(move || replay(&fixture, dir, limit, ready))
        })
        .collect();
    barrier.wait();
    let mut peak = 0;
    let mut samples = fs::File::create(out.join("memory.jsonl")).unwrap();
    while workers.iter().any(|worker| !worker.is_finished()) {
        let rss = memory_kib("VmRSS:", "/proc/self/status");
        peak = peak.max(rss);
        writeln!(
            samples,
            "{}",
            json!({"seconds":start.elapsed().as_secs_f64(),"rss_kib":rss})
        )
        .unwrap();
        samples.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    let complete = workers
        .into_iter()
        .map(|worker| worker.join().is_ok())
        .collect::<Vec<_>>();
    peak = peak.max(memory_kib("VmHWM:", "/proc/self/status"));
    let result = json!({"drawings":3,"last_call":limit,"complete":complete,"seconds":start.elapsed().as_secs_f64(),
        "peak_rss_kib":peak,"available_at_start_kib":available,"budget_leaving_1_gib_kib":budget,
        "within_budget":peak <= budget,"scope":"Three sessions and production tool surface; excludes WebKit and provider histories"});
    fs::write(
        out.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("{result}");
    assert!(complete.iter().all(|done| *done));
    assert!(
        peak <= budget,
        "three replays exceed available memory minus 1 GiB"
    );
}
