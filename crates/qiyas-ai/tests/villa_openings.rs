//! The founder's logged villa inputs, replayed without a provider or credentials.
use opencadcodec::Handle;
use qiyas_ai::{surface, ToolOutput};
use qiyas_core::{Session, Source};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[path = "../../../tests/support/performance.rs"]
mod performance;

#[path = "support/villa.rs"]
mod villa;

fn snapshot(s: &mut Session) -> Value {
    let entities: BTreeMap<_, _> = s
        .drawing
        .doc
        .entities()
        .map(|e| (e.common().handle.value(), json!(e)))
        .collect();
    json!({"entities":entities, "structure":s.drawing.doc.snapshot_structure(),
        "groups":s.wall_groups(None).unwrap(), "history":s.history_steps(),
        "revision":s.document_revision(), "takeoff":s.takeoff})
}

fn members_exist(s: &Session) {
    for group in s.wall_groups(None).unwrap() {
        for handle in group.handles {
            assert!(
                s.drawing.doc.get_entity(Handle::new(handle)).is_some(),
                "missing wall member {handle}"
            );
        }
        for run in group.runs {
            assert!(s
                .drawing
                .doc
                .get_entity(Handle::new(run.centreline))
                .is_some());
            for opening in run.openings {
                assert!(s
                    .drawing
                    .doc
                    .get_entity(Handle::new(opening.insert))
                    .is_some());
            }
        }
    }
}

fn count(s: &Session) -> usize {
    s.wall_groups(None)
        .unwrap()
        .iter()
        .flat_map(|g| &g.runs)
        .map(|r| r.openings.len())
        .sum()
}

fn scale(value: &mut Value, factor: f64) {
    fn coordinates(value: &mut Value, factor: f64) {
        if let Some(n) = value.as_f64() {
            *value = json!(n / factor);
        } else if let Some(values) = value.as_array_mut() {
            for v in values {
                coordinates(v, factor);
            }
        }
    }
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if ["a", "b", "points", "min", "max", "at", "width", "thickness"]
                    .contains(&key.as_str())
                {
                    coordinates(value, factor);
                } else {
                    scale(value, factor);
                }
            }
        }
        Value::Array(values) => {
            for v in values {
                scale(v, factor);
            }
        }
        _ => {}
    }
}

fn replay(units: i16, factor: f64) -> Duration {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("villa.dxf"), units);
    s.apply_settings_method(Some(&qiyas_core::measurement::RuleSet::named(
        "POMI (RICS)",
    )))
    .unwrap();
    let calls = villa::calls();
    let mut maximum = Duration::ZERO;
    let mut refused = 0;
    for (i, mut call) in calls.into_iter().enumerate() {
        scale(&mut call["input"]["args"], factor);
        villa::bind_openings(
            &mut call["input"],
            &json!(s.wall_groups(None).unwrap()),
            factor,
        );
        let surface::Route::Session { tool, op, args } =
            surface::route(call["tool"].as_str().unwrap(), &call["input"]).unwrap()
        else {
            panic!("session input")
        };
        let openings = args["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["type"] == "add_wall_openings")
            .map(|c| c["openings"].as_array().unwrap().len())
            .sum::<usize>();
        let before = snapshot(&mut s);
        let before_count = count(&s);
        let start = Instant::now();
        let result = surface::run_session(&mut s, &tool, &op, &args, Source::Ai);
        let elapsed = start.elapsed();
        eprintln!(
            "villa units={units} call={i} openings={openings} seconds={:.6} result={}",
            elapsed.as_secs_f64(),
            result
                .as_ref()
                .map(|_| "ok".to_string())
                .unwrap_or_else(|e| e.to_string())
        );
        if openings > 0 {
            maximum = maximum.max(elapsed);
        }
        match result {
            Ok(ToolOutput::Json(_)) => {
                assert_eq!(count(&s), before_count + openings, "call {i}");
                let groups = s.wall_groups(None).unwrap();
                for command in args["commands"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["type"] == "add_wall_openings")
                {
                    let host = command["centreline"].as_u64().unwrap();
                    let run = groups
                        .iter()
                        .flat_map(|g| &g.runs)
                        .find(|r| r.centreline == host)
                        .expect("requested host survives");
                    for requested in command["openings"].as_array().unwrap() {
                        let actual = run
                            .openings
                            .iter()
                            .find(|o| {
                                (o.opening.at - requested["at"].as_f64().unwrap()).abs()
                                    < 1e-7 / factor
                            })
                            .expect("requested opening position");
                        assert!(
                            (actual.opening.width - requested["width"].as_f64().unwrap()).abs()
                                < 1e-7 / factor
                        );
                        assert_eq!(json!(actual.opening.kind), requested["kind"]);
                        for field in ["sill", "head"] {
                            if !requested[field].is_null() {
                                assert_eq!(json!(actual.opening)[field], requested[field]);
                            }
                        }
                    }
                }
            }
            Ok(_) => panic!("expected drawing result"),
            Err(error) => {
                let message = error.to_string();
                assert!(
                    i > 5,
                    "the original opening phases fit: call {i}: {message}"
                );
                assert!(
                    openings > 0
                        && message.contains("fit")
                        && (message.contains("overlap")
                            || message.contains("junction")
                            || message.contains("does not fit")),
                    "call {i}: {message}"
                );
                assert_eq!(
                    snapshot(&mut s),
                    before,
                    "call {i} must roll back every record and membership"
                );
                refused += 1;
            }
        }
        members_exist(&s);
    }
    assert_eq!(
        count(&s),
        44,
        "all 44 distinct requested openings must be present"
    );
    assert!(
        refused > 0,
        "logged retries must refuse overlapping openings"
    );
    maximum
}

#[test]
fn logged_villa_openings_are_atomic_and_keep_their_members() {
    replay(6, 1.0);
}

#[test]
#[ignore = "full four-unit villa replay; smaller four-unit rollback tests run normally"]
fn logged_villa_in_every_unit() {
    for (units, factor) in [(4, 0.001), (5, 0.01), (6, 1.0), (1, 0.0254)] {
        replay(units, factor);
    }
}

#[test]
#[ignore = "release speed guard: cargo test --release -p qiyas-ai --test villa_openings villa_speed -- --ignored --nocapture"]
#[allow(clippy::assertions_on_constants)]
fn villa_speed() {
    // This ignored test must fail clearly if explicitly run in a debug build.
    assert!(!cfg!(debug_assertions), "run the speed guard in release");
    let reference = performance::reference();
    let maximum = replay(6, 1.0);
    performance::assert_relative(maximum, reference, 2.0, "largest villa opening batch");
}

#[test]
fn a_panicking_tool_rolls_back_and_releases_the_session() {
    use std::sync::{Arc, Mutex};
    let dir = tempfile::tempdir().unwrap();
    let shared = Arc::new(Mutex::new(Session::new_blank(
        &dir.path().join("panic.dxf"),
        6,
    )));
    let before = snapshot(&mut shared.lock().unwrap());
    let worker = shared.clone();
    let error = std::thread::spawn(move || {
        let mut s = worker.lock().unwrap();
        qiyas_core::fixtures::panic_on_document_edit(2);
        surface::run_session(
            &mut s,
            surface::EXECUTE,
            "batch",
            &json!({"commands":[
                {"type":"draw","kind":"line","a":[0,0],"b":[1,0]},
                {"type":"draw","kind":"line","a":[0,1],"b":[1,1]}
            ]}),
            Source::Ai,
        )
        .unwrap_err()
    })
    .join()
    .expect("the tool must return an error, not unwind its worker");
    assert!(error.to_string().contains("rolled back"), "{error}");
    let mut s = shared
        .try_lock()
        .expect("session released without poisoning");
    assert_eq!(snapshot(&mut s), before);
    surface::run_session(
        &mut s,
        surface::EXECUTE,
        "run",
        &json!({"command":
            {"type":"draw","kind":"line","a":[0,0],"b":[1,0]}
        }),
        Source::Ai,
    )
    .unwrap();
    assert_eq!(s.drawing.doc.entities().count(), 1);
    assert!(s.undo_if(Source::Ai));
    assert_eq!(s.drawing.doc.entities().count(), 0);
    assert!(s.redo());
    assert_eq!(s.drawing.doc.entities().count(), 1);
}
