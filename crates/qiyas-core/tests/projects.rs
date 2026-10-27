use qiyas_core::{
    projects::{self, ProjectCommand},
    Command, Outcome, Session, Source,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
mod fixture {
    include!("support/measurement_fixture.rs");
}
fn isolated(name: &str) -> Option<PathBuf> {
    if std::env::var("QIYAS_PROJECT_CHILD").ok().as_deref() == Some(name) {
        return Some(PathBuf::from(std::env::var_os("QIYAS_HOME").unwrap()));
    }
    let home = tempfile::tempdir().unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &test_path(name), "--nocapture"])
        .env("QIYAS_PROJECT_CHILD", name)
        .env("QIYAS_HOME", home.path())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&result.stdout);
    assert!(result.status.success(), "{out}");
    // A wrong name runs no test and still succeeds; the child must have run this one.
    assert!(out.contains("1 passed"), "the child ran no test:\n{out}");
    None
}

/// The test's name as the test binary knows it: its module path without the binary's name
/// (all the tests run in one binary, see tests/all.rs).
fn test_path(name: &str) -> String {
    match module_path!().split_once("::") {
        Some((_, module)) => format!("{module}::{name}"),
        None => name.to_owned(),
    }
}
fn command(session: &mut Session, value: Value) -> Value {
    let Outcome::Project { result } = session.run_input(&value, Source::Ai).unwrap().outcome else {
        panic!()
    };
    result
}
fn create(session: &mut Session) -> PathBuf {
    PathBuf::from(
        command(
            session,
            json!({"type":"project","kind":"create","name":"Test"}),
        )["path"]
            .as_str()
            .unwrap(),
    )
}
fn drawing(home: &Path, name: &str) -> PathBuf {
    let path = home.join(name);
    let mut s = Session::new_blank(&path, 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("first", false));
    for (i, text) in ["NO: A-1", "TITLE: Floor", "REV: B", "DATE: 2026-10-22"]
        .iter()
        .enumerate()
    {
        fixture::run(
            &mut s,
            json!({"type":"text","at":[0,20+i as i32],"value":text,"height":0.2}),
        );
    }
    s.save_as_headless(&path).unwrap();
    path
}
#[test]
fn projects_aggregate_trace_override_register_checks_and_undo() {
    let Some(home) = isolated("projects_aggregate_trace_override_register_checks_and_undo") else {
        return;
    };
    let mut s = Session::new_blank(&home.join("workspace.dxf"), 6);
    let qss = create(&mut s);
    let mut paths = vec![];
    for name in ["a.dxf", "b.dxf", "c.dxf"] {
        let path = drawing(&home, name);
        command(
            &mut s,
            json!({"type":"project","kind":"add_drawing","path":qss,"drawing":path}),
        );
        paths.push(path);
    }
    command(
        &mut s,
        json!({"type":"project","kind":"set_method","path":qss,"method":fixture::method("linear",true)}),
    );
    command(
        &mut s,
        json!({"type":"project","kind":"set_register_rules","path":qss,"rules":[
        {"field":"number","text_pattern":"^NO: (.+)$"},{"field":"title","text_pattern":"^TITLE: (.+)$"},{"field":"revision","text_pattern":"^REV: (.+)$"},{"field":"date","text_pattern":"^DATE: (.+)$"}]}),
    );
    let mut a = Session::open_standalone(&paths[0]).unwrap();
    fixture::run(
        &mut a,
        json!({"type":"set_knowledge","kind":"tables","name":"schedule","data":{"rows":[["Surface","4"]],"citation":{"source":"Test schedule","clause":"row 1"}}}),
    );
    a.save_as_headless(&paths[0]).unwrap();
    command(
        &mut s,
        json!({"type":"project","kind":"set_checks","path":qss,"checks":{
            "schedule":{"citation":{"source":"Test","clause":"S"},"question_en":"Confirm schedule?","question_ar":"تأكيد الجدول؟","test":{"kind":"schedule","table":{"kind":"document","drawing":paths[0],"name":"schedule"},"key_column":0,"value_column":1,"elements":{"filter":{"type":"Surface"}},"property":"type"}},
            "sizes":{"citation":{"source":"Test","clause":"each"},"question_en":"Confirm size?","question_ar":"تأكيد المقاس؟","test":{"kind":"schedule","table":{"kind":"document","drawing":paths[0],"name":"schedule"},"key_column":0,"value_column":1,"elements":{"filter":{"type":"Surface"}},"property":"type","measure":"copies","each":true}},
        "duplicates":{"citation":{"source":"Test","clause":"D"},"question_en":"Confirm identity?","question_ar":"تأكيد الهوية؟","test":{"kind":"relation","left":{"filter":{"type":"Surface"}},"right":{"filter":{"type":"Surface"}},"join":"grid","right_property":"grid","minimum":1,"maximum":1}}
        }}),
    );
    let before = std::fs::read(&qss).unwrap();
    let n = s.history_steps().0.len();
    let result = command(&mut s, json!({"type":"project","kind":"run","path":qss}));
    assert_eq!(s.history_steps().0.len(), n + 1);
    let set: qiyas_core::SheetSet = serde_json::from_value(result["set"].clone()).unwrap();
    let p = set.project.unwrap();
    assert!(p.checked, "{:?}", p.warnings);
    assert_eq!(p.boq.len(), 1);
    assert_eq!(p.boq[0].item.unit, "m");
    assert!((p.boq[0].item.quantity - 33.).abs() < 1e-8);
    assert_eq!(p.boq[0].contributions.len(), 3);
    let explanation =
        qiyas_core::trust::explain_project(qss.to_str().unwrap(), "project:0").unwrap();
    assert_eq!(explanation["drawings"].as_array().unwrap().len(), 3);
    assert_eq!(explanation["confidence"], 0.5);
    assert_eq!(p.confidence_summary.low, 1);
    assert!(p.boq[0]
        .contributions
        .iter()
        .all(|c| !c.explanation["measurement_rules"]
            .as_array()
            .unwrap()
            .is_empty()));
    assert!(p.boq[0]
        .contributions
        .iter()
        .all(|c| !c.lines.is_empty() && c.lines.iter().all(|l| l.method == "linear")));
    assert_eq!(p.register.len(), 3);
    assert!(p
        .register
        .iter()
        .all(|r| r.flags.is_empty() && r.revision.as_deref() == Some("B")));
    assert_eq!(p.queries.len(), 7);
    assert_eq!(
        p.queries
            .iter()
            .map(|q| &q.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        7
    );
    let schedule = p
        .queries
        .iter()
        .find(|q| q.query.as_ref().unwrap().rule.as_deref() == Some("schedule"))
        .unwrap();
    assert_eq!(schedule.expected, Some(4.));
    assert_eq!(schedule.found, Some(3.));
    assert_eq!(
        schedule
            .query
            .as_ref()
            .unwrap()
            .references
            .iter()
            .filter_map(|r| r.drawing.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert!(s.undo());
    assert_eq!(std::fs::read(&qss).unwrap(), before);
    assert!(s.redo());
    assert_eq!(projects::load(&qss).unwrap().project.unwrap(), p);
    // The project method never rewrites a drawing's independent knowledge.
    assert_eq!(
        Session::open_standalone(&paths[0])
            .unwrap()
            .measurement_report()
            .unwrap()
            .items[0]
            .unit,
        "m2"
    );
    assert!(s
        .run(Command::Project(Box::new(ProjectCommand::AddDrawing {
            path: qss.to_string_lossy().into(),
            drawing: paths[0].to_string_lossy().into(),
            layouts: vec![]
        })))
        .is_err());
}
#[test]
fn project_paths_missing_metadata_and_failed_drawings_remain_explicit() {
    let Some(home) = isolated("project_paths_missing_metadata_and_failed_drawings_remain_explicit")
    else {
        return;
    };
    let mut s = Session::new_blank(&home.join("workspace.dxf"), 6);
    let qss = create(&mut s);
    let a = drawing(&home, "a.dxf");
    command(
        &mut s,
        json!({"type":"project","kind":"add_drawing","path":qss,"drawing":a}),
    );
    assert!(s
        .run_input(
            &json!({"type":"project","kind":"create","name":"../escape"}),
            Source::User
        )
        .is_err());
    assert!(s
        .run_input(
            &json!({"type":"project","kind":"open","path":home.join("Test.qss")}),
            Source::User
        )
        .is_err());
    let before = std::fs::read(&qss).unwrap();
    command(&mut s, json!({"type":"project","kind":"run","path":qss}));
    let p = projects::load(&qss).unwrap().project.unwrap();
    assert!(p.register.iter().all(|r| r.flags.len() == 4));
    std::fs::write(&qss, &before).unwrap();
    assert!(!s.undo(), "Undo must refuse an external edit");
    std::fs::remove_file(&a).unwrap();
    command(&mut s, json!({"type":"project","kind":"run","path":qss}));
    let p = projects::load(&qss).unwrap().project.unwrap();
    assert!(!p.checked);
    assert_eq!(p.drawings[0].status, "error");
    assert!(p.boq.is_empty());
    command(
        &mut s,
        json!({"type":"project","kind":"remove_drawing","path":qss,"drawing":a}),
    );
    assert!(projects::load(&qss).unwrap().drawings.is_empty());
    assert!(s.undo());
    assert_eq!(projects::load(&qss).unwrap().drawings.len(), 1);
    let mut outside = projects::load(&qss).unwrap();
    outside.drawings[0].path = a.to_string_lossy().into();
    assert!(outside.save(&home.join("outside.qss")).is_err());
    #[cfg(unix)]
    {
        let link = home.join("projects/link.qss");
        std::os::unix::fs::symlink(home.join("outside.qss"), &link).unwrap();
        assert!(!qiyas_core::data_home::is_project_path(&link));
    }
}
#[test]
fn aggregation_separates_units_sections_trades_and_uses_description_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("a.dxf"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("first", false));
    let mut a = s.measurement_report().unwrap();
    a.items[0].code.clear();
    a.items[0].description_en = "  Same   description ".into();
    let mut b = a.clone();
    b.items[0].description_en = "same description".into();
    assert_eq!(
        projects::aggregate(&[("a".into(), a.clone()), ("b".into(), b.clone())]).len(),
        1
    );
    for field in ["unit", "section", "trade"] {
        let mut item = json!(b.items[0]);
        item[field] = json!("different");
        b.items[0] = serde_json::from_value(item).unwrap();
        assert_eq!(
            projects::aggregate(&[("a".into(), a.clone()), ("b".into(), b.clone())]).len(),
            2
        );
        b = a.clone();
    }
}
