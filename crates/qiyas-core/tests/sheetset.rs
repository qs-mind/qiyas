//! Sheet sets: a `.qss` JSON file listing drawings (paths relative to it, inside its folder) and
//! their layouts. Only the `.qss` file itself is ever written.

use qiyas_core::fixtures::write_sample;
use qiyas_core::{sheet_set_entries, sheet_set_entry, SheetSet, SheetSetDrawing};

#[test]
fn a_sheet_set_keeps_relative_paths_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let plans = dir.path().join("plans");
    std::fs::create_dir(&plans).unwrap();
    let a = write_sample(&plans, "dwg");
    let b = write_sample(dir.path(), "dxf");
    let qss = dir.path().join("Villa.qss");

    let set = SheetSet {
        project: None,
        name: "Villa".into(),
        drawings: vec![
            sheet_set_entry(&qss, &a, vec!["Layout1".into()]).unwrap(),
            sheet_set_entry(&qss, &b, Vec::new()).unwrap(),
        ],
    };
    assert_eq!(set.drawings[0].path, "plans/sample.dwg");
    assert_eq!(set.drawings[1].path, "sample.dxf");
    set.save(&qss).unwrap();
    let text = std::fs::read_to_string(&qss).unwrap();
    assert!(text.contains("\"plans/sample.dwg\""), "{text}");

    let again = SheetSet::load(&qss).unwrap();
    assert_eq!(again, set);
    let entries = sheet_set_entries(&qss, &again);
    assert_eq!(entries.len(), 2);
    assert_eq!(std::path::Path::new(&entries[0].full), a.as_path());
    assert!(!entries[0].missing);
    assert_eq!(entries[0].layouts, ["Layout1"]);

    std::fs::remove_file(&b).unwrap();
    assert!(sheet_set_entries(&qss, &again)[1].missing);
}

#[test]
fn a_sheet_set_never_reaches_outside_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let inner = dir.path().join("set");
    std::fs::create_dir(&inner).unwrap();
    let outside = write_sample(dir.path(), "dwg");
    let qss = inner.join("Set.qss");
    // A drawing outside the folder is refused.
    assert!(sheet_set_entry(&qss, &outside, Vec::new()).is_err());
    // So is a path climbing out of it, or an absolute one, in a set saved or read.
    for path in ["../sample.dwg", outside.to_str().unwrap()] {
        let set = SheetSet {
            project: None,
            name: "Set".into(),
            drawings: vec![SheetSetDrawing {
                path: path.into(),
                layouts: Vec::new(),
            }],
        };
        assert!(set.save(&qss).is_err());
        assert!(!qss.exists());
        std::fs::write(
            &qss,
            serde_json::to_string(&serde_json::json!({
                "name": "Set",
                "drawings": [{"path": path, "layouts": []}]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(SheetSet::load(&qss).is_err());
        std::fs::remove_file(&qss).unwrap();
    }
    // Only a .qss is written.
    let set = SheetSet {
        project: None,
        name: "Set".into(),
        drawings: Vec::new(),
    };
    assert!(set.save(&inner.join("Set.json")).is_err());
}
