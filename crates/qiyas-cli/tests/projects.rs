use calamine::Reader;
use qiyas_core::{projects::ProjectData, Session, Source};
use serde_json::json;
mod fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../qiyas-core/tests/support/measurement_fixture.rs"
    ));
}
#[test]
fn project_cli_exports_a_traceable_workbook_and_morning_report_in_an_isolated_home() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join("projects")).unwrap();
    let mut paths = vec![];
    for name in ["a.dxf", "b.dxf"] {
        let path = dir.path().join(name);
        let mut s = Session::new_blank(&path, 6);
        fixture::fixture(&mut s);
        fixture::apply(&mut s, fixture::method("test", false));
        s.save_as_headless(&path).unwrap();
        paths.push(path);
    }
    let path = home.join("projects/CLI.qss");
    std::fs::write(&path,json!({"name":"CLI","drawings":paths.iter().map(|p|json!({"path":p})).collect::<Vec<_>>(),"project":ProjectData::default()}).to_string()).unwrap();
    let out = dir.path().join("out");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_qiyas-cli"))
        .args(["project", "run"])
        .arg(&path)
        .arg("--out")
        .arg(&out)
        .env("QIYAS_HOME", &home)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let set = qiyas_boq::project::import(&out.join("project.xlsx")).unwrap();
    let project = set.project.unwrap();
    assert_eq!(project.boq.len(), 1);
    assert_eq!(project.boq[0].contributions.len(), 2);
    let mut book = calamine::open_workbook_auto(out.join("project.xlsx")).unwrap();
    for name in ["Generated BOQ", "Dimension sheets", "Queries", "Register"] {
        assert!(book.worksheet_range(name).is_ok(), "{name}");
    }
    let dimensions = format!("{:?}", book.worksheet_range("Dimension sheets").unwrap());
    assert!(dimensions.contains("a.dxf") && dimensions.contains("b.dxf"));
    assert!(std::fs::read_to_string(out.join("morning-report.md"))
        .unwrap()
        .contains("2 drawings"));
    std::fs::remove_file(&paths[1]).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_qiyas-cli"))
        .args(["project", "run"])
        .arg(&path)
        .arg("--out")
        .arg(&out)
        .env("QIYAS_HOME", &home)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(std::fs::read_to_string(out.join("morning-report.md"))
        .unwrap()
        .contains("1 failed"));
}
#[test]
fn project_prediction_writes_without_any_readable_truth() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("truth.json")).unwrap();
    let mut s = Session::new_blank(&dir.path().join("drawing.dxf"), 6);
    fixture::fixture(&mut s);
    fixture::apply(&mut s, fixture::method("test", false));
    let report = s.measurement_report().unwrap();
    let items = qiyas_core::projects::aggregate(&[("drawing.dxf".into(), report)]);
    let set = qiyas_core::SheetSet {
        name: "Truth inaccessible".into(),
        drawings: vec![],
        project: Some(ProjectData {
            boq: items,
            ..Default::default()
        }),
    };
    qiyas_boq::project::export(&dir.path().join("project.xlsx"), &set).unwrap();
    assert_eq!(
        qiyas_boq::project::import(&dir.path().join("project.xlsx")).unwrap(),
        set
    );
    s.run_input(&json!({"type":"measurement","kind":"report"}), Source::User)
        .unwrap();
}
