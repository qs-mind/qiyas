use calamine::{open_workbook, Data, Reader, Xlsx};
use qiyas_core::{Kind, Measurement, Takeoff, Units};

fn item(kind: Kind, value: f64, description: &str) -> Measurement {
    Measurement::new(
        kind,
        value,
        Units::Metres,
        description.to_string(),
        "A-WALL".to_string(),
        "*Model_Space".to_string(),
    )
}

#[test]
fn export_is_readable_by_calamine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("villa-takeoff.xlsx");
    let mut takeoff = Takeoff::default();
    let mut boundary = item(Kind::Area, 12.0, "Slab");
    boundary.area_origin = qiyas_core::AreaOrigin::ExistingBoundary;
    boundary.measurement_basis = qiyas_core::MeasurementBasis::Gross;
    takeoff.add(boundary);
    let mut wall = item(Kind::Area, 12.0, "Wall face");
    wall.area_origin = qiyas_core::AreaOrigin::WallFace;
    wall.measurement_basis = qiyas_core::MeasurementBasis::WallFace;
    // Verified separately below, without changing the existing totals fixture.
    let wall_path = dir.path().join("wall-face.xlsx");
    qiyas_boq::export_takeoff(&wall_path, &[wall], &[], "Wall face").unwrap();
    let mut wall_book: Xlsx<_> = open_workbook(&wall_path).unwrap();
    let wall_sheet = wall_book.worksheet_range("Take-off").unwrap();
    assert_eq!(
        wall_sheet.get((1, 8)),
        Some(&Data::String("wall_face".into()))
    );
    assert_eq!(
        wall_sheet.get((1, 9)),
        Some(&Data::String("wall_face".into()))
    );
    takeoff.add(item(Kind::Length, 7.0, "Wall"));
    takeoff.add(item(Kind::Count, 3.0, "Doors"));

    qiyas_boq::export_takeoff(
        &path,
        &takeoff.items,
        &takeoff.totals(),
        "villa.dwg take-off",
    )
    .unwrap();

    let mut book: Xlsx<_> = open_workbook(&path).unwrap();
    let sheet = book.worksheet_range("Take-off").unwrap();
    let header: Vec<_> = (0..8)
        .map(|c| sheet.get((0, c)).unwrap().to_string())
        .collect();
    assert_eq!(
        header,
        [
            "Description",
            "Kind",
            "Quantity",
            "Unit",
            "Layer",
            "Sheet",
            "Note",
            "Id"
        ]
    );
    assert_eq!(
        sheet.get((1, 8)),
        Some(&Data::String("existing_boundary".into()))
    );
    assert_eq!(sheet.get((1, 9)), Some(&Data::String("gross".into())));
    assert_eq!(
        sheet.get((3, 10)),
        Some(&Data::String("unspecified".into()))
    );
    assert_eq!(sheet.get((1, 0)), Some(&Data::String("Slab".into())));
    assert_eq!(sheet.get((1, 1)), Some(&Data::String("area".into())));
    assert_eq!(sheet.get((1, 2)), Some(&Data::Float(12.0)));
    assert_eq!(
        sheet.get((1, 7)),
        Some(&Data::String(takeoff.items[0].id.clone()))
    );
    assert_eq!(sheet.get((2, 2)), Some(&Data::Float(7.0)));
    assert_eq!(sheet.get((3, 1)), Some(&Data::String("count".into())));
    assert_eq!(sheet.get((3, 2)), Some(&Data::Float(3.0)));
    // A blank row, then the totals block.
    assert!(sheet.get((4, 0)).is_none_or(|d| *d == Data::Empty));
    assert_eq!(sheet.get((5, 0)), Some(&Data::String("Kind".into())));
    assert_eq!(sheet.get((6, 4)), Some(&Data::Float(12.0)));
    assert_eq!(sheet.get((8, 4)), Some(&Data::Float(3.0)));
    // Every output says it is not approved.
    assert!((0..sheet.height()).any(|r| {
        sheet
            .get((r, 0))
            .is_some_and(|d| d.to_string() == "Status: For review")
    }));

    let totals = book.worksheet_range("Totals").unwrap();
    let header: Vec<_> = (0..5)
        .map(|c| totals.get((0, c)).unwrap().to_string())
        .collect();
    assert_eq!(header, ["Kind", "Unit", "Item", "Count", "Sum"]);
    assert_eq!(totals.get((1, 0)), Some(&Data::String("area".into())));
    assert_eq!(totals.get((1, 2)), Some(&Data::String("Slab".into())));
    assert_eq!(totals.get((1, 3)), Some(&Data::Float(1.0)));
    assert_eq!(totals.get((2, 4)), Some(&Data::Float(7.0)));
    assert_eq!(totals.get((3, 0)), Some(&Data::String("count".into())));
    assert_eq!(totals.get((3, 4)), Some(&Data::Float(3.0)));
}

#[test]
fn the_totals_sheet_adds_like_with_like_and_never_floor_tiles_to_a_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("like.xlsx");
    let mut takeoff = Takeoff::default();
    // Floor tiles measured in two pieces, and a ceiling: all m².
    takeoff.add(item(Kind::Area, 40.0, "Floor tiles"));
    takeoff.add(item(Kind::Area, 60.0, "Floor tiles"));
    takeoff.add(item(Kind::Area, 100.0, "Ceiling"));
    qiyas_boq::export_takeoff(&path, &takeoff.items, &takeoff.totals(), "like").unwrap();
    let mut book: Xlsx<_> = open_workbook(&path).unwrap();
    let totals = book.worksheet_range("Totals").unwrap();
    // One row per item: the two floor pieces add to 100 (count 2); the ceiling is its own row.
    assert_eq!(totals.get((1, 2)), Some(&Data::String("Floor tiles".into())));
    assert_eq!(totals.get((1, 3)), Some(&Data::Float(2.0)));
    assert_eq!(totals.get((1, 4)), Some(&Data::Float(100.0)));
    assert_eq!(totals.get((2, 2)), Some(&Data::String("Ceiling".into())));
    assert_eq!(totals.get((2, 4)), Some(&Data::Float(100.0)));
    // No row adds the two items together (200 m²).
    for r in 0..totals.height() {
        assert_ne!(totals.get((r, 4)), Some(&Data::Float(200.0)), "row {r}");
    }
}

#[test]
fn export_table_writes_one_data_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doors.xlsx");
    let columns = ["Handle", "Block", "MARK", "Length (m)"].map(String::from);
    let long = "x".repeat(100);
    let rows = vec![
        ["2A", "DOOR", "D1", "2.313"].map(String::from).to_vec(),
        vec!["43".into(), "DOOR".into(), long.clone(), "-0.5".into()],
        ["inf", "1,200", "NaN", ""].map(String::from).to_vec(),
        ["007", "1e3", "+5", " 2 "].map(String::from).to_vec(),
        ["0", "1.", ".5", "-"].map(String::from).to_vec(),
    ];
    qiyas_boq::export_table(&path, &columns, &rows).unwrap();

    let mut book: Xlsx<_> = open_workbook(&path).unwrap();
    assert_eq!(book.sheet_names(), ["Data"]);
    let sheet = book.worksheet_range("Data").unwrap();
    let header: Vec<String> = (0..4)
        .map(|c| sheet.get((0, c)).unwrap().to_string())
        .collect();
    assert_eq!(header, columns);
    assert_eq!(sheet.get((1, 2)), Some(&Data::String("D1".into())));
    assert_eq!(sheet.get((2, 2)), Some(&Data::String(long)));
    // Plain decimals are written as numbers; everything else as text.
    let text = |s: &str| Some(Data::String(s.into()));
    assert_eq!(sheet.get((1, 3)), Some(&Data::Float(2.313)));
    assert_eq!(sheet.get((2, 3)), Some(&Data::Float(-0.5)));
    assert_eq!(sheet.get((2, 0)), Some(&Data::Float(43.0)));
    assert_eq!(sheet.get((5, 0)), Some(&Data::Float(0.0)));
    for (row, col, s) in [
        (1, 0, "2A"),
        (3, 0, "inf"),
        (3, 1, "1,200"),
        (3, 2, "NaN"),
        (4, 0, "007"),
        (4, 1, "1e3"),
        (4, 2, "+5"),
        (4, 3, " 2 "),
        (5, 1, "1."),
        (5, 2, ".5"),
        (5, 3, "-"),
    ] {
        assert_eq!(sheet.get((row, col)).cloned(), text(s), "{s:?}");
    }
    assert_eq!(sheet.height(), 6);
}
