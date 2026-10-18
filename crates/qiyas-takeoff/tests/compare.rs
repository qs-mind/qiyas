use qiyas_core::{
    BlockCount, Boq, BoqItem, Conflict, ConflictKind, ConflictStatus, Kind, Measurement, Schedule,
    Takeoff, Units,
};
use qiyas_takeoff::{compare, CompareInputs};

fn item(code: &str, description: &str, unit: &str, quantity: f64) -> BoqItem {
    BoqItem {
        code: code.to_string(),
        description: description.to_string(),
        unit: unit.to_string(),
        quantity,
        rate: None,
        section: String::new(),
        tolerance: None,
        row: 2,
    }
}

fn boq(items: Vec<BoqItem>) -> Boq {
    Boq::new("boq.xlsx".to_string(), items, 0)
}

/// A measurement in metres on `sheet`, linked to `code` when given.
fn m(kind: Kind, value: f64, description: &str, sheet: &str, code: Option<&str>) -> Measurement {
    let mut m = Measurement::new(
        kind,
        value,
        Units::Metres,
        description.to_string(),
        String::new(),
        sheet.to_string(),
    );
    m.boq_code = code.map(str::to_string);
    m
}

fn takeoff(items: Vec<Measurement>) -> Takeoff {
    Takeoff { items }
}

fn blocks(counts: &[(&str, usize)]) -> Vec<BlockCount> {
    counts
        .iter()
        .map(|(name, count)| BlockCount {
            count_scope: qiyas_core::INSERT_COUNT_SCOPE,
            visibility_policy: "in_file",
            limitations: qiyas_core::INSERT_COUNT_LIMITATIONS,
            block_name: name.to_string(),
            layer: "0".to_string(),
            count: *count,
            attributes: Vec::new(),
        })
        .collect()
}

/// References of `name` tagged with `attributes`.
fn tagged(name: &str, count: usize, attributes: &[&str]) -> BlockCount {
    BlockCount {
        count_scope: qiyas_core::INSERT_COUNT_SCOPE,
        visibility_policy: "in_file",
        limitations: qiyas_core::INSERT_COUNT_LIMITATIONS,
        block_name: name.to_string(),
        layer: "0".to_string(),
        count,
        attributes: attributes.iter().map(|a| a.to_string()).collect(),
    }
}

/// A schedule with a Mark and a Qty column.
fn door_schedule(rows: &[(&str, &str)]) -> Schedule {
    Schedule {
        name: "Door Schedule".to_string(),
        sheet: "Schedules".to_string(),
        source: 0,
        columns: vec!["Mark".into(), "Qty".into()],
        rows: rows
            .iter()
            .map(|(mark, qty)| vec![mark.to_string(), qty.to_string()])
            .collect(),
    }
}

/// The schedule-vs-plan conflicts of `schedule` against `sheets`.
fn against_plans(schedule: &Schedule, sheets: &[(String, Vec<BlockCount>)]) -> Vec<Conflict> {
    let empty = Takeoff::default();
    let inputs = CompareInputs {
        boq: None,
        takeoff: &empty,
        schedules: std::slice::from_ref(schedule),
        blocks: sheets,
        units_label: "metres",
    };
    compare(&inputs, 5.0, &[])
        .into_iter()
        .filter(|c| c.kind == ConflictKind::ScheduleVsPlan)
        .collect()
}

/// The conflicts of a BOQ and a take-off alone.
fn run(boq: &Boq, takeoff: &Takeoff, previous: &[Conflict]) -> Vec<Conflict> {
    let inputs = CompareInputs {
        boq: Some(boq),
        takeoff,
        schedules: &[],
        blocks: &[],
        units_label: "metres",
    };
    compare(&inputs, 5.0, previous)
}

fn of_kind(conflicts: &[Conflict], kind: ConflictKind) -> Vec<&Conflict> {
    conflicts.iter().filter(|c| c.kind == kind).collect()
}

/// Three links across two sheets: 40 + 35 + 24 = 99 m2.
fn slab_takeoff() -> Takeoff {
    takeoff(vec![
        m(Kind::Area, 40.0, "Slab bay 1", "GF Plan", Some("A.1")),
        m(Kind::Area, 35.0, "Slab bay 2", "GF Plan", Some("A.1")),
        m(Kind::Area, 24.0, "Slab bay 3", "FF Plan", Some("A.1")),
    ])
}

#[test]
fn quantity_mismatch_uses_the_sum_of_links() {
    let takeoff = slab_takeoff();
    // 99 against 100 is within 5 %.
    let within = run(&boq(vec![item("A.1", "Slab", "m2", 100.0)]), &takeoff, &[]);
    assert!(within.is_empty(), "{within:#?}");

    let outside = run(&boq(vec![item("A.1", "Slab", "m2", 120.0)]), &takeoff, &[]);
    assert_eq!(outside.len(), 1, "{outside:#?}");
    let c = &outside[0];
    assert_eq!(c.kind, ConflictKind::QuantityMismatch);
    assert_eq!(c.boq_code.as_deref(), Some("A.1"));
    assert_eq!(c.expected, Some(120.0));
    assert!((c.found.unwrap() - 99.0).abs() < 1e-9);
    assert_eq!(c.measurements.len(), 3);
    assert_eq!(c.sheets, ["FF Plan", "GF Plan"]);
    assert_eq!(c.unit, "m2");
    assert_eq!(
        c.reason,
        "BOQ says 120.000 m², drawings measure 99.000 m² (−17.5 %)"
    );
    assert_eq!(
        c.reason_ar,
        "جدول الكميات يذكر 120.000 م²، والمقاس من الرسومات 99.000 م² (−17.5 %)"
    );
    assert_eq!(c.status, ConflictStatus::Open);
    assert_eq!(c.id.len(), 16);

    // The item's own tolerance wins over the default.
    let mut loose = item("A.1", "Slab", "m2", 120.0);
    loose.tolerance = Some(20.0);
    assert!(run(&boq(vec![loose]), &takeoff, &[]).is_empty());
}

#[test]
fn linked_items_never_report_not_in_drawings() {
    let takeoff = takeoff(vec![m(Kind::Area, 50.0, "Area", "GF Plan", Some("A.1"))]);
    let bill = boq(vec![
        // Linked, though its description matches nothing measured.
        item("A.1", "Ceramic floor tiles", "m2", 50.0),
        item("A.2", "Waterproofing membrane", "m2", 30.0),
    ]);
    let conflicts = run(&bill, &takeoff, &[]);
    let missing = of_kind(&conflicts, ConflictKind::NotInDrawings);
    assert_eq!(missing.len(), 1, "{conflicts:#?}");
    assert_eq!(missing[0].boq_code.as_deref(), Some("A.2"));
    assert_eq!(missing[0].expected, Some(30.0));
    assert_eq!(missing[0].found, None);
}

#[test]
fn missing_from_boq_groups_by_description() {
    let takeoff = takeoff(vec![
        m(Kind::Length, 10.0, "Wall 1", "GF Plan", None),
        m(Kind::Length, 6.0, "wall-2", "FF Plan", None),
        m(Kind::Length, 4.0, " WALL ", "GF Plan", None),
        m(Kind::Length, 12.0, "Kerb stone", "Site", None),
        // Described by a BOQ item: not missing.
        m(Kind::Area, 80.0, "Concrete slab GF", "GF Plan", None),
    ]);
    let bill = boq(vec![item("A.1", "Reinforced concrete slab", "m2", 80.0)]);
    let conflicts = run(&bill, &takeoff, &[]);
    let missing = of_kind(&conflicts, ConflictKind::MissingFromBoq);
    assert_eq!(missing.len(), 2, "{conflicts:#?}");
    let walls = missing.iter().find(|c| c.measurements.len() == 3).unwrap();
    assert_eq!(walls.found, Some(20.0));
    assert_eq!(walls.unit, "m");
    assert_eq!(walls.sheets, ["FF Plan", "GF Plan"]);
    assert_eq!(
        walls.reason,
        "3 measurements \"Wall 1\" (20.000 m) match no BOQ item"
    );
    let kerb = missing.iter().find(|c| c.measurements.len() == 1).unwrap();
    assert_eq!(kerb.found, Some(12.0));
    assert_eq!(
        kerb.reason,
        "1 measurement \"Kerb stone\" (12.000 m) matches no BOQ item"
    );
    assert!(of_kind(&conflicts, ConflictKind::NotInDrawings).is_empty());
}

#[test]
fn markups_and_zero_items_never_count_as_missing() {
    let mut markup = m(Kind::Item, 0.0, "Check this wall", "GF Plan", None);
    markup.unit = "markup".to_string();
    let mut zero = m(Kind::Item, 0.0, "Bags of cement", "GF Plan", None);
    zero.unit = "bags".to_string();
    let takeoff = takeoff(vec![markup, zero]);
    let bill = boq(vec![item("A.1", "Plaster", "m2", 10.0)]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert!(of_kind(&conflicts, ConflictKind::MissingFromBoq).is_empty());
    // A markup that names a BOQ item does not count as measuring it.
    let bill = boq(vec![item("A.1", "Wall plaster", "m2", 10.0)]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert_eq!(of_kind(&conflicts, ConflictKind::NotInDrawings).len(), 1);
}

#[test]
fn arabic_descriptions_match_by_tokens() {
    let takeoff = takeoff(vec![m(
        Kind::Volume,
        12.0,
        "خرسانة الأعمدة",
        "GF Plan",
        None,
    )]);
    let bill = boq(vec![item("C.1", "خرسانة مسلحة للأعمدة", "m3", 12.0)]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert!(conflicts.is_empty(), "{conflicts:#?}");
}

#[test]
fn schedule_vs_plan_detects_count_difference() {
    let schedule = Schedule {
        name: "Door Schedule".to_string(),
        sheet: "Schedules".to_string(),
        source: 0,
        columns: vec!["Mark".into(), "Type".into(), "Qty".into()],
        rows: vec![
            vec!["D1".into(), "Single".into(), "4".into()],
            vec!["D2".into(), "Double".into(), "2 nos".into()],
            // Names no block: not compared.
            vec!["D7".into(), "Sliding".into(), "1".into()],
        ],
    };
    let sheets = vec![
        ("GF Plan".to_string(), blocks(&[("DOOR-D1", 2)])),
        ("FF Plan".to_string(), blocks(&[("D1", 1), ("D2", 2)])),
        // The legend on the schedule sheet is not a plan.
        ("Schedules".to_string(), blocks(&[("D1", 1), ("D2", 1)])),
    ];
    let empty = takeoff(vec![]);
    let inputs = CompareInputs {
        boq: None,
        takeoff: &empty,
        schedules: std::slice::from_ref(&schedule),
        blocks: &sheets,
        units_label: "metres",
    };
    let conflicts = compare(&inputs, 5.0, &[]);
    assert_eq!(conflicts.len(), 1, "{conflicts:#?}");
    let c = &conflicts[0];
    assert_eq!(c.kind, ConflictKind::ScheduleVsPlan);
    assert_eq!(c.schedule.as_deref(), Some("Door Schedule"));
    assert_eq!(c.expected, Some(4.0));
    assert_eq!(c.found, Some(3.0));
    assert_eq!(c.unit, "pcs");
    assert_eq!(c.sheets, ["FF Plan", "GF Plan", "Schedules"]);
    assert_eq!(
        c.reason,
        "Schedule \"Door Schedule\" lists 4 of D1; the plans show 3 (GF Plan 2, FF Plan 1)"
    );

    // A table without a count column is not a schedule to compare.
    let mut plain = schedule.clone();
    plain.columns = vec!["Mark".into(), "Type".into(), "Width".into()];
    let inputs = CompareInputs {
        schedules: std::slice::from_ref(&plain),
        ..inputs
    };
    assert!(compare(&inputs, 5.0, &[]).is_empty());
}

#[test]
fn drawing_vs_drawing_compares_sheets_of_one_floor() {
    let sheets = vec![
        (
            "GF Architecture".to_string(),
            blocks(&[("CHAIR", 10), ("*U12", 3), ("DESK", 4)]),
        ),
        (
            "GF Furniture".to_string(),
            blocks(&[("CHAIR", 8), ("*U12", 5), ("DESK", 4)]),
        ),
        // Another floor: not compared.
        ("FF Furniture".to_string(), blocks(&[("CHAIR", 2)])),
    ];
    let empty = takeoff(vec![]);
    let inputs = CompareInputs {
        boq: None,
        takeoff: &empty,
        schedules: &[],
        blocks: &sheets,
        units_label: "metres",
    };
    let conflicts = compare(&inputs, 5.0, &[]);
    assert_eq!(conflicts.len(), 1, "{conflicts:#?}");
    let c = &conflicts[0];
    assert_eq!(c.kind, ConflictKind::DrawingVsDrawing);
    assert_eq!(c.sheets, ["GF Architecture", "GF Furniture"]);
    assert_eq!((c.expected, c.found), (Some(10.0), Some(8.0)));
    assert_eq!(
        c.reason,
        "CHAIR is counted 10 on GF Architecture and 8 on GF Furniture"
    );
}

#[test]
fn unit_doubt_for_length_vs_area() {
    // "ls" is not exactly the item's "LS".
    let mut lump = m(Kind::Item, 1.0, "Preliminaries", "GF Plan", Some("B.1"));
    lump.unit = "ls".to_string();
    let takeoff = takeoff(vec![
        m(Kind::Length, 12.0, "Skirting", "GF Plan", Some("A.1")),
        m(Kind::Area, 30.0, "Tiles", "GF Plan", Some("A.1")),
        lump,
    ]);
    let bill = boq(vec![
        item("A.1", "Floor tiles", "m2", 100.0),
        item("B.1", "Preliminaries", "LS", 1.0),
    ]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert!(
        of_kind(&conflicts, ConflictKind::QuantityMismatch).is_empty(),
        "units that do not match are never summed: {conflicts:#?}"
    );
    let doubts = of_kind(&conflicts, ConflictKind::UnitDoubt);
    assert_eq!(doubts.len(), 2, "{conflicts:#?}");
    let tiles = doubts
        .iter()
        .find(|c| c.boq_code.as_deref() == Some("A.1"))
        .unwrap();
    assert_eq!(tiles.measurements, [takeoff.items[0].id.clone()]);
    assert_eq!(
        tiles.reason,
        "BOQ item A.1 is in m², but 1 linked measurement is in another unit: 12.000 m"
    );
    // An item in a unit of its own, with links not exactly in that unit, is a doubt too.
    assert!(doubts.iter().any(|c| c.boq_code.as_deref() == Some("B.1")));

    // Links exactly in the item's own unit: no doubt.
    let mut exact = takeoff.clone();
    exact.items[2].unit = "LS".to_string();
    let conflicts = run(&bill, &exact, &[]);
    assert!(!conflicts
        .iter()
        .any(|c| c.boq_code.as_deref() == Some("B.1")));
}

#[test]
fn linked_measurements_are_evidence_only_for_their_item() {
    let takeoff = takeoff(vec![m(
        Kind::Area,
        40.0,
        "Internal plaster to walls",
        "GF Plan",
        Some("A.1"),
    )]);
    let bill = boq(vec![
        item("A.1", "Internal plaster to walls", "m2", 40.0),
        item("A.2", "External plaster to walls", "m2", 60.0),
    ]);
    let conflicts = run(&bill, &takeoff, &[]);
    let missing = of_kind(&conflicts, ConflictKind::NotInDrawings);
    assert_eq!(missing.len(), 1, "{conflicts:#?}");
    assert_eq!(missing[0].boq_code.as_deref(), Some("A.2"));
}

#[test]
fn descriptions_need_more_than_one_shared_word() {
    let takeoff = takeoff(vec![
        m(Kind::Volume, 8.0, "Concrete columns", "GF Plan", None),
        m(Kind::Area, 50.0, "Screed 50mm", "GF Plan", None),
    ]);
    let bill = boq(vec![
        item("A.1", "Reinforced concrete slabs", "m3", 20.0),
        item("A.2", "Plaster 20mm thick", "m2", 50.0),
    ]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert_eq!(
        of_kind(&conflicts, ConflictKind::MissingFromBoq).len(),
        2,
        "{conflicts:#?}"
    );
    assert_eq!(of_kind(&conflicts, ConflictKind::NotInDrawings).len(), 2);
}

#[test]
fn items_sharing_a_code_keep_separate_conflicts() {
    let takeoff = slab_takeoff();
    let mut second = item("A.1", "Slab", "m2", 150.0);
    second.row = 5;
    let bill = boq(vec![item("A.1", "Slab", "m2", 120.0), second]);
    let conflicts = run(&bill, &takeoff, &[]);
    assert_eq!(conflicts.len(), 2, "{conflicts:#?}");
    assert_ne!(conflicts[0].id, conflicts[1].id);
    assert!(conflicts[1].hash_key.ends_with("|row 5"));
}

#[test]
fn marks_match_block_names_and_attribute_values() {
    let schedule = door_schedule(&[("D1", "5"), ("A", "1")]);
    let sheets = vec![
        (
            "GF Plan".to_string(),
            vec![
                tagged("D1", 1, &[]),
                tagged("DOOR-D1", 1, &[]),
                // Tagged D1 by attribute.
                tagged("DOOR", 2, &["D1"]),
                tagged("DOOR", 4, &["D2"]),
                // Name and attribute both say D1: counted once.
                tagged("D1", 1, &["D1"]),
                // A one-letter mark matches whole names only.
                tagged("A-DOOR", 3, &[]),
            ],
        ),
        ("Site".to_string(), vec![tagged("A", 1, &[])]),
    ];
    let conflicts = against_plans(&schedule, &sheets);
    // D1: 1 + 1 + 2 + 1 = 5, as listed; A: 1, as listed.
    assert!(conflicts.is_empty(), "{conflicts:#?}");
}

#[test]
fn nos_and_no_headers_are_count_columns() {
    let plans = [("GF Plan".to_string(), blocks(&[("D1", 3)]))];
    for columns in [["MARK", "NOS"], ["TYPE", "No."]] {
        let mut schedule = door_schedule(&[("D1", "4")]);
        schedule.columns = columns.iter().map(|c| c.to_string()).collect();
        let conflicts = against_plans(&schedule, &plans);
        assert_eq!(conflicts.len(), 1, "{columns:?}");
        assert_eq!(
            (conflicts[0].expected, conflicts[0].found),
            (Some(4.0), Some(3.0))
        );
    }
}

#[test]
fn sheets_of_one_floor_count_once() {
    let schedule = |n: &str| door_schedule(&[("D1", n)]);
    let sheet = |name: &str, n: usize| (name.to_string(), blocks(&[("D1", n)]));
    let one_floor = [sheet("GF Plan", 12), sheet("GF Furniture", 12)];
    assert!(against_plans(&schedule("12"), &one_floor).is_empty());
    let two_floors = [sheet("GF Plan", 12), sheet("FF Plan", 12)];
    assert!(against_plans(&schedule("24"), &two_floors).is_empty());
    let short = [sheet("GF Plan", 12), sheet("FF Plan", 10)];
    let conflicts = against_plans(&schedule("24"), &short);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].found, Some(22.0));
}

#[test]
fn ids_are_stable_across_runs() {
    let takeoff = slab_takeoff();
    let bill = boq(vec![
        item("A.1", "Slab", "m2", 120.0),
        item("A.2", "Waterproofing", "m2", 30.0),
    ]);
    let first = run(&bill, &takeoff, &[]);
    let second = run(&bill, &takeoff, &[]);
    assert_eq!(first.len(), 2);
    let ids = |cs: &[Conflict]| cs.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&first), ids(&second));
    assert_ne!(first[0].id, first[1].id);
    assert!(first[0].hash_key.starts_with("quantity_mismatch|A.1|"));
}

#[test]
fn resolved_conflicts_keep_their_status_after_recompare() {
    let takeoff = slab_takeoff();
    let bill = boq(vec![item("A.1", "Slab", "m2", 120.0)]);
    let mut first = run(&bill, &takeoff, &[]);
    first[0].status = ConflictStatus::Resolved;
    first[0].note = "Bay 4 is on the roof sheet".to_string();
    first[0].created = "2026-10-01T00:00:00Z".to_string();

    let again = run(&bill, &takeoff, &first);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].id, first[0].id);
    assert_eq!(again[0].status, ConflictStatus::Resolved);
    assert_eq!(again[0].note, "Bay 4 is on the roof sheet");
    assert_eq!(again[0].created, "2026-10-01T00:00:00Z");

    // Another set of links is another finding: open again.
    let mut more = slab_takeoff();
    more.items.truncate(2);
    let changed = run(&bill, &more, &first);
    assert_eq!(changed.len(), 1);
    assert_ne!(changed[0].id, first[0].id);
    assert_eq!(changed[0].status, ConflictStatus::Open);
}

#[test]
fn arabic_reasons_say_qita_for_pieces() {
    let takeoff = takeoff(vec![m(Kind::Count, 5.0, "Windows", "GF Plan", Some("B.2"))]);
    let conflicts = run(
        &boq(vec![item("B.2", "Windows", "pcs", 4.0)]),
        &takeoff,
        &[],
    );
    assert_eq!(conflicts.len(), 1, "{conflicts:#?}");
    assert_eq!(
        conflicts[0].reason,
        "BOQ says 4 pcs, drawings measure 5 pcs (+25.0 %)"
    );
    assert_eq!(
        conflicts[0].reason_ar,
        "جدول الكميات يذكر 4 قطعة، والمقاس من الرسومات 5 قطعة (+25.0 %)"
    );
}

#[test]
fn default_named_measurements_are_never_missing_from_boq() {
    let mut auto = m(Kind::Area, 22.09, "LwPolyline on A-ROOM", "GF Plan", None);
    auto.auto_description = true;
    let bill = boq(vec![item("A.4", "Floor screed, bedroom", "m2", 30.0)]);
    let conflicts = run(&bill, &takeoff(vec![auto.clone()]), &[]);
    assert!(
        of_kind(&conflicts, ConflictKind::MissingFromBoq).is_empty(),
        "{conflicts:#?}"
    );

    // Named by someone, it is work the BOQ may lack.
    let mut named = auto;
    named.auto_description = false;
    let conflicts = run(&bill, &takeoff(vec![named]), &[]);
    assert_eq!(of_kind(&conflicts, ConflictKind::MissingFromBoq).len(), 1);
}

#[test]
fn plural_descriptions_match_singular_ones() {
    let mut doors = m(Kind::Count, 3.0, "DOOR count", "GF Plan", None);
    doors.auto_description = true;
    let bill = boq(vec![item("B.1", "Doors 900 mm", "pcs", 3.0)]);
    let conflicts = run(&bill, &takeoff(vec![doors]), &[]);
    assert!(conflicts.is_empty(), "{conflicts:#?}");
}
