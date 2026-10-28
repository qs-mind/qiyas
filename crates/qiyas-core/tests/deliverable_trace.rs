use crate::measurement_fixture as fixture;
use qiyas_core::{
    deliverables::{self, Kind, Language, TableRow},
    projects::{aggregate, ProjectData},
    Session, SheetSet,
};
use serde_json::json;

#[test]
fn drawing_reports_keep_item_and_dimension_identity_through_grouping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.dxf");
    let mut session = Session::new_blank(&path, 6);
    fixture::fixture(&mut session);
    fixture::apply(&mut session, fixture::method("trace", false));
    let measured = session.measurement_report().unwrap();
    for language in [Language::En, Language::Ar] {
        for kind in [Kind::Boq, Kind::Dimensions] {
            let report = deliverables::drawing(&session, kind, language, None).unwrap();
            assert_eq!(report.drawing.as_deref(), path.to_str());
            let rows = report.table().unwrap();
            assert!(rows
                .iter()
                .filter(|row| row.heading)
                .all(|row| row.target.is_none()));
            assert!(report.status_row().target.is_none());
            for row in rows.iter().filter(|row| !row.heading) {
                let target = row.target.as_ref().unwrap();
                let item = measured
                    .items
                    .iter()
                    .find(|item| target.item == format!("generated:{}", item.code))
                    .unwrap();
                if kind == Kind::Boq {
                    assert!(target.line.is_none());
                } else {
                    let id = target.line.as_deref().unwrap();
                    assert!(!id.is_empty());
                    assert!(item.lines.iter().any(|line| line.id == id));
                }
            }
        }
    }
}

#[test]
fn project_indices_precede_filtering_and_survive_duplicate_codes() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new_blank(&dir.path().join("source.dxf"), 6);
    fixture::fixture(&mut session);
    fixture::apply(&mut session, fixture::method("trace", false));
    let mut boq = aggregate(&[("source.dxf".into(), session.measurement_report().unwrap())]);
    let item = boq[0].clone();
    boq[0].item.informational = true;
    boq.push(item.clone());
    boq.push(item);
    let set = SheetSet {
        name: "Project".into(),
        drawings: vec![],
        project: Some(ProjectData {
            boq,
            ..Default::default()
        }),
    };
    let report = deliverables::project(&set, Kind::Boq, Language::En, None).unwrap();
    assert!(report.drawing.is_none());
    let targets: Vec<_> = report
        .table()
        .unwrap()
        .into_iter()
        .filter_map(|row| row.target)
        .collect();
    assert_eq!(
        targets
            .iter()
            .map(|target| target.item.as_str())
            .collect::<Vec<_>>(),
        ["project:1", "project:2"]
    );
    assert!(targets.iter().all(|target| target.line.is_none()));
    let dimensions = deliverables::project(&set, Kind::Dimensions, Language::Ar, None).unwrap();
    for row in &dimensions.rows {
        assert!(row["target"]["item"]
            .as_str()
            .unwrap()
            .starts_with("project:"));
        assert!(!row["target"]["line"].as_str().unwrap().is_empty());
    }
}

#[test]
fn old_table_rows_deserialize_without_a_target() {
    let row: TableRow = serde_json::from_value(json!({"cells":["old"],"heading":false})).unwrap();
    assert!(row.target.is_none());
    assert!(serde_json::to_value(row).unwrap().get("target").is_none());
}
