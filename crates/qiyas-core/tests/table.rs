//! Tables and fields. A new table is a real Table entity (ACAD_TABLE) on the current layer with
//! its `*T` block, read back by `read_tables` as a schedule; cells are edited, rows and columns
//! inserted and deleted and cells merged one undo step each; the tables of an existing drawing are
//! drawn and picked as themselves. Fields (date, file name, an entity's area or length, a block
//! count) are text whose field code Qiyas keeps (`QIYAS_FIELD` xdata) and evaluates; object fields
//! follow the entity they read in the same step as the edit.

use opencadcodec::entities::{Table, TableColumn, TableRow};
use opencadcodec::{BlockRecord, CadDocument, EntityType, Handle, MText, Vector3};
use qiyas_core::fixtures::{sample_doc, schedule_doc, write_drawing};
use qiyas_core::{
    field_code, Command, FieldKind, ModifySpec, Outcome, Session, Target, TextScope, FIELD_APP,
};

fn open(doc: &CadDocument, name: &str) -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(doc, &dir.path().join(name));
    (dir, Session::open(&path).unwrap())
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn one(outcome: Outcome) -> u64 {
    match outcome {
        Outcome::Added { handles } if handles.len() == 1 => handles[0],
        other => panic!("expected one new entity, got {other:?}"),
    }
}

fn table(s: &Session, h: u64) -> Table {
    match s.drawing.doc.get_entity(Handle::new(h)) {
        Some(EntityType::Table(t)) => t.clone(),
        other => panic!("{h:X} is no table: {other:?}"),
    }
}

fn texts(t: &Table) -> Vec<Vec<String>> {
    t.rows
        .iter()
        .map(|r| r.cells.iter().map(|c| c.text_value().to_string()).collect())
        .collect()
}

fn text_of(s: &Session, h: u64) -> String {
    match s.drawing.doc.get_entity(Handle::new(h)) {
        Some(EntityType::Text(t)) => t.value.clone(),
        Some(EntityType::MText(t)) => t.value.clone(),
        other => panic!("{h:X} is no text: {other:?}"),
    }
}

/// The texts the table's `*T` block draws.
fn block_texts(s: &Session, h: u64) -> Vec<String> {
    let name = table(s, h).block_name;
    assert!(name.starts_with("*T"), "{name}");
    s.drawing
        .doc
        .entities_in_block(&name)
        .filter_map(|e| match e {
            EntityType::MText(m) => Some(m.value.clone()),
            _ => None,
        })
        .collect()
}

fn new_table(s: &mut Session, at: [f64; 2], data: &[&[&str]]) -> u64 {
    let rows = data.len();
    let cols = data[0].len();
    one(s
        .run(Command::Table {
            at,
            rows,
            cols,
            col_width: 4.0,
            row_height: 1.0,
            data: data
                .iter()
                .map(|r| r.iter().map(|c| c.to_string()).collect())
                .collect(),
        })
        .unwrap())
}

/// The sample's closed 4 x 3 rectangle on A-WALL.
fn rectangle(s: &Session) -> u64 {
    s.drawing
        .entities_in("*Model_Space")
        .find_map(|e| match e {
            EntityType::LwPolyline(p) if p.is_closed => Some(p.common.handle.value()),
            _ => None,
        })
        .unwrap()
}

/// The sample's 10 m line.
fn wall_line(s: &Session) -> u64 {
    s.drawing
        .entities_in("*Model_Space")
        .find_map(|e| match e {
            EntityType::Line(l) if (l.end.x - 10.0).abs() < 1e-9 => Some(l.common.handle.value()),
            _ => None,
        })
        .unwrap()
}

fn field_of(s: &Session, h: u64) -> Option<String> {
    let e = s.drawing.doc.get_entity(Handle::new(h))?;
    let record = e.common().extended_data.get_record(FIELD_APP)?;
    Some(
        record
            .values
            .iter()
            .filter_map(|v| match v {
                opencadcodec::xdata::XDataValue::String(s) => Some(s.as_str()),
                _ => None,
            })
            .collect(),
    )
}

#[test]
fn a_new_3_by_2_table_reads_back_as_a_schedule() {
    let (dir, mut s) = open(&sample_doc(), "plan.dxf");
    s.run(Command::SetCurrent {
        current: qiyas_core::Current {
            layer: "A-DOOR".to_string(),
            ..Default::default()
        },
    })
    .unwrap();
    let h = new_table(
        &mut s,
        [50.0, 0.0],
        &[&["Item", "Qty"], &["Door", "3"], &["Window", "5"]],
    );
    let tb = table(&s, h);
    assert_eq!(tb.common.layer, "A-DOOR", "on the current layer");
    assert_eq!((tb.rows.len(), tb.columns.len()), (3, 2));
    assert!(tb.columns.iter().all(|c| c.width == 4.0));
    assert!(tb.rows.iter().all(|r| r.height == 1.0));
    let style = tb.rows[0].cells[0].style.as_ref().expect("a cell style");
    assert_eq!(
        style.text_style_name,
        s.text_style(),
        "the current text style"
    );
    // Its block draws the grid and the texts.
    assert_eq!(
        block_texts(&s, h),
        ["Item", "Qty", "Door", "3", "Window", "5"]
    );

    let schedule = |s: &Session| {
        s.read_tables(None)
            .into_iter()
            .find(|sc| sc.source != 0 && sc.columns == ["Item", "Qty"])
            .expect("the new table")
    };
    let read = schedule(&s);
    assert_eq!(read.source, h);
    assert_eq!(read.rows, [["Door", "3"], ["Window", "5"]]);

    // One undo step, taking its block with it; redo brings both back.
    assert!(s.undo());
    assert!(s.drawing.doc.get_entity(Handle::new(h)).is_none());
    assert!(s.redo());
    assert_eq!(block_texts(&s, h).len(), 6);

    // Saved and opened again (DXF and DWG), it is still the same schedule.
    for name in ["again.dxf", "again.dwg"] {
        let path = dir.path().join(name);
        s.save_as(&path).unwrap();
        let back = Session::open(&path).unwrap();
        let read = schedule(&back);
        assert_eq!(read.rows, [["Door", "3"], ["Window", "5"]], "{name}");
    }
}

#[test]
fn a_table_needs_cells_and_sizes() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let bad = |rows, cols, w: f64, h: f64, data: Vec<Vec<String>>| Command::Table {
        at: [0.0, 0.0],
        rows,
        cols,
        col_width: w,
        row_height: h,
        data,
    };
    assert!(s.run(bad(0, 2, 1.0, 1.0, vec![])).is_err());
    assert!(s.run(bad(2, 2, -1.0, 1.0, vec![])).is_err());
    assert!(s.run(bad(2, 2, 1.0, -1.0, vec![])).is_err());
    assert!(s
        .run(bad(1, 1, 1.0, 1.0, vec![vec!["a".into(), "b".into()]]))
        .is_err());
    assert!(!s.can_undo());
}

#[test]
fn an_existing_drawings_table_is_drawn_picked_and_described() {
    // The schedule sample's 2 x 2 table at (20, 0): columns 2 wide, rows 0.25 tall, no block.
    let (_dir, s) = open(&schedule_doc(), "plan.dxf");
    let h = s
        .drawing
        .entities_in("*Model_Space")
        .find_map(|e| matches!(e, EntityType::Table(_)).then(|| e.common().handle.value()))
        .unwrap();
    let picked = s
        .pick([23.0, -0.25], 0.05)
        .expect("the line between its rows");
    assert_eq!(picked.top, h);
    assert_eq!(picked.kind.as_deref(), Some("Table"));
    let window = s.pick_window([[19.9, -0.6], [24.1, 0.1]], false);
    assert!(window.iter().any(|p| p.top == h), "{window:?}");

    let info = s.entity_info(h, &[]).unwrap();
    assert_eq!(info.kind, "Table");
    let cells = info.table.expect("its cells");
    assert_eq!(cells.cells, [["A", "B"], ["C", "D"]]);
    assert_eq!(cells.widths, [2.0, 2.0]);
    assert_eq!(cells.heights, [0.25, 0.25]);
    assert_eq!(info.length, None, "a table is not measured by its lines");
    assert_eq!(s.table_cell_at(h, [21.0, -0.4]), Some([1, 0]));
    assert_eq!(s.table_cell_at(h, [25.0, -0.4]), None);
}

#[test]
fn cells_rows_columns_and_merges_are_one_step_each() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let h = new_table(&mut s, [50.0, 0.0], &[&["a", "b"], &["c", "d"]]);
    let steps = |s: &mut Session, cmd: Command| {
        let before = texts(&table(s, h));
        s.run(cmd).unwrap();
        let after = table(s, h);
        assert!(s.undo());
        assert_eq!(texts(&table(s, h)), before, "undone in one step");
        assert!(s.redo());
        after
    };

    let tb = steps(
        &mut s,
        Command::EditCell {
            target: t(h),
            row: 0,
            col: 1,
            text: "B {1}\\x".into(),
        },
    );
    assert_eq!(
        tb.cell_text(0, 1),
        Some("B \\{1\\}\\\\x"),
        "plain text, escaped"
    );
    assert_eq!(
        s.entity_info(h, &[]).unwrap().table.unwrap().cells[0][1],
        "B {1}\\x"
    );
    assert!(block_texts(&s, h).contains(&"B \\{1\\}\\\\x".to_string()));

    let tb = steps(
        &mut s,
        Command::InsertRow {
            target: t(h),
            at: 1,
        },
    );
    assert_eq!(texts(&tb), [["a", "B \\{1\\}\\\\x"], ["", ""], ["c", "d"]]);
    assert_eq!(tb.rows[1].height, 1.0, "as tall as the row above");
    assert!(
        tb.rows[1].cells[0].style.is_some(),
        "styled as its neighbours"
    );

    let tb = steps(
        &mut s,
        Command::InsertCol {
            target: t(h),
            at: 2,
        },
    );
    assert_eq!(tb.columns.len(), 3);
    assert_eq!(tb.columns[2].width, 4.0);

    let tb = steps(
        &mut s,
        Command::DeleteRow {
            target: t(h),
            row: 1,
        },
    );
    assert_eq!(texts(&tb), [["a", "B \\{1\\}\\\\x", ""], ["c", "d", ""]]);

    let tb = steps(
        &mut s,
        Command::DeleteCol {
            target: t(h),
            col: 2,
        },
    );
    assert_eq!(tb.columns.len(), 2);

    let tb = steps(
        &mut s,
        Command::MergeCells {
            target: t(h),
            first: [0, 0],
            last: [0, 1],
        },
    );
    assert_eq!(tb.rows[0].cells[0].merge_width, 2);
    let info = s.entity_info(h, &[]).unwrap().table.unwrap();
    assert_eq!(info.merges, [[0, 0, 0, 1]]);
    // A merged cell's text is its first cell's; editing a cell inside it edits that one.
    s.run(Command::EditCell {
        target: t(h),
        row: 0,
        col: 1,
        text: "TITLE".into(),
    })
    .unwrap();
    assert_eq!(table(&s, h).cell_text(0, 0), Some("TITLE"));
    // One cell given unmerges the merged cell it is in.
    let tb = steps(
        &mut s,
        Command::MergeCells {
            target: t(h),
            first: [0, 1],
            last: [0, 1],
        },
    );
    assert_eq!(tb.rows[0].cells[0].merge_width, 1);

    // A table keeps one row and one column at least; cells must exist.
    let one_row = new_table(&mut s, [80.0, 0.0], &[&["x"]]);
    assert!(s
        .run(Command::DeleteRow {
            target: t(one_row),
            row: 0
        })
        .is_err());
    assert!(s
        .run(Command::DeleteCol {
            target: t(one_row),
            col: 0
        })
        .is_err());
    assert!(s
        .run(Command::EditCell {
            target: t(one_row),
            row: 3,
            col: 0,
            text: "y".into()
        })
        .is_err());
    // Only tables.
    let line = wall_line(&s);
    assert!(s
        .run(Command::EditCell {
            target: t(line),
            row: 0,
            col: 0,
            text: "y".into()
        })
        .is_err());
}

#[test]
fn arabic_cell_text_is_kept_in_reading_order() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let h = new_table(&mut s, [50.0, 0.0], &[&["غرفة المعيشة", "مطبخ"]]);
    let tb = table(&s, h);
    assert_eq!(tb.cell_text(0, 0), Some("غرفة المعيشة"));
    assert!(qiyas_geom::is_logical_text(&tb.common), "marked as Qiyas's");
    // Drawn from its block, every text in reading order whatever "Arabic text is reversed" says.
    let e = EntityType::Table(tb);
    let out = qiyas_geom::flatten(&s.drawing.doc, &e, &qiyas_geom::Xform::identity(), 1e-3);
    let drawn: Vec<_> = out.paths.iter().filter_map(|p| p.text.as_ref()).collect();
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|t| t.logical));
    assert_eq!(drawn[0].value, "غرفة المعيشة");
}

#[test]
fn find_and_replace_reaches_table_cells() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let h = new_table(&mut s, [50.0, 0.0], &[&["Door", "Fire door"]]);
    match s
        .run(Command::FindText {
            find: "door".into(),
            scope: TextScope::Sheet,
            match_case: false,
            whole_word: true,
        })
        .unwrap()
    {
        Outcome::Found { matches, .. } => assert!(matches.iter().any(|m| m.handle == h)),
        other => panic!("{other:?}"),
    }
    match s
        .run(Command::FindReplace {
            find: "door".into(),
            replace: "gate".into(),
            scope: TextScope::Sheet,
            match_case: false,
            whole_word: true,
        })
        .unwrap()
    {
        Outcome::Replaced { handles, .. } => assert!(handles.contains(&h)),
        other => panic!("{other:?}"),
    }
    assert_eq!(texts(&table(&s, h)), [["gate", "Fire gate"]]);
    assert_eq!(
        block_texts(&s, h),
        ["gate", "Fire gate"],
        "its block rebuilt"
    );
}

#[test]
fn an_area_field_follows_the_rectangle_it_reads() {
    let (dir, mut s) = open(&sample_doc(), "plan.dxf");
    let rect = rectangle(&s);
    let field = FieldKind::Area { target: t(rect) };
    let code = field_code(&field);
    assert_eq!(
        code,
        format!("%<\\AcObjProp Object(%<\\_ObjId {rect:X}>%).Area \\f \"%lu2%pr3\">%")
    );
    let h = one(s
        .run(Command::Field {
            at: [0.0, 5.0],
            height: 0.25,
            field,
        })
        .unwrap());
    assert_eq!(text_of(&s, h), "12.000");
    assert_eq!(field_of(&s, h).as_deref(), Some(code.as_str()));
    // The code is kept in the file (DXF and DWG).
    for name in ["field.dxf", "field.dwg"] {
        let path = dir.path().join(name);
        s.save_as(&path).unwrap();
        let back = Session::open(&path).unwrap();
        assert_eq!(field_of(&back, h).as_deref(), Some(code.as_str()), "{name}");
    }

    // Scaled, the rectangle's area field follows in the same step.
    s.run(Command::Modify(ModifySpec::Scale {
        targets: vec![t(rect)],
        base: [0.0, 0.0],
        factor: 2.0,
        copy: false,
    }))
    .unwrap();
    assert_eq!(text_of(&s, h), "48.000");
    assert!(s.undo());
    assert_eq!(text_of(&s, h), "12.000", "undone with the scale");
    assert!(s.redo());
    assert_eq!(text_of(&s, h), "48.000");

    // A length field; erased, the entity it read leaves "####".
    let line = wall_line(&s);
    let len = one(s
        .run(Command::Field {
            at: [0.0, 6.0],
            height: 0.25,
            field: FieldKind::Length { target: t(line) },
        })
        .unwrap());
    assert_eq!(text_of(&s, len), "10.000");
    s.run(Command::DeleteEntities {
        targets: vec![t(line)],
    })
    .unwrap();
    assert_eq!(text_of(&s, len), "####");
    // Nothing to update is no step.
    let undoable = s.can_undo();
    s.run(Command::UpdateFields).unwrap();
    assert_eq!(s.can_undo(), undoable);
}

#[test]
fn date_file_name_and_count_fields() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let today = {
        let d = time::OffsetDateTime::now_utc().date();
        format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
    };
    assert_eq!(
        field_code(&FieldKind::Date),
        "%<\\AcVar Date \\f \"yyyy-MM-dd\">%"
    );
    let date = one(s
        .run(Command::Field {
            at: [0.0, 5.0],
            height: 0.25,
            field: FieldKind::Date,
        })
        .unwrap());
    assert_eq!(text_of(&s, date), today);
    let name = one(s
        .run(Command::Field {
            at: [0.0, 6.0],
            height: 0.25,
            field: FieldKind::FileName,
        })
        .unwrap());
    assert_eq!(text_of(&s, name), "plan.dxf");
    let count = one(s
        .run(Command::Field {
            at: [0.0, 7.0],
            height: 0.25,
            field: FieldKind::Count {
                block: "DOOR".into(),
            },
        })
        .unwrap());
    assert_eq!(text_of(&s, count), "3");

    // A text typed with field codes in it is a field too; so is an MText.
    let typed = one(s
        .run(Command::MText {
            at: [0.0, 8.0],
            width: 0.0,
            height: 0.25,
            value: format!(
                "Doors: {}",
                field_code(&FieldKind::Count {
                    block: "DOOR".into()
                })
            ),
            style: None,
        })
        .unwrap());
    assert_eq!(text_of(&s, typed), "Doors: 3");

    // A door erased: UpdateFields counts again, one step.
    let door = s
        .drawing
        .entities_in("*Model_Space")
        .find_map(|e| match e {
            EntityType::Insert(i) if i.block_name == "DOOR" => Some(i.common.handle.value()),
            _ => None,
        })
        .unwrap();
    s.run(Command::DeleteEntities {
        targets: vec![t(door)],
    })
    .unwrap();
    assert_eq!(text_of(&s, count), "3", "a count is updated on demand");
    s.run(Command::UpdateFields).unwrap();
    assert_eq!(text_of(&s, count), "2");
    assert_eq!(text_of(&s, typed), "Doors: 2");
    assert!(s.undo());
    assert_eq!(text_of(&s, count), "3");
}

#[test]
fn fields_in_table_cells_follow_too() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let rect = rectangle(&s);
    let h = new_table(&mut s, [50.0, 0.0], &[&["Room", "Area"], &["Hall", ""]]);
    let code = field_code(&FieldKind::Area { target: t(rect) });
    s.run(Command::EditCell {
        target: t(h),
        row: 1,
        col: 1,
        text: format!("{code} m2"),
    })
    .unwrap();
    assert_eq!(table(&s, h).cell_text(1, 1), Some("12.000 m2"));
    let info = s.entity_info(h, &[]).unwrap().table.unwrap();
    assert_eq!(info.fields, [[1, 1]]);
    // A row inserted above moves the field with its cell.
    s.run(Command::InsertRow {
        target: t(h),
        at: 0,
    })
    .unwrap();
    s.run(Command::Modify(ModifySpec::Scale {
        targets: vec![t(rect)],
        base: [0.0, 0.0],
        factor: 2.0,
        copy: false,
    }))
    .unwrap();
    assert_eq!(table(&s, h).cell_text(2, 1), Some("48.000 m2"));
    assert!(
        block_texts(&s, h).contains(&"48.000 m2".to_string()),
        "block rebuilt"
    );
    assert!(s.undo());
    assert_eq!(table(&s, h).cell_text(2, 1), Some("12.000 m2"));
    // Typed over, the cell is plain text again.
    s.run(Command::EditCell {
        target: t(h),
        row: 2,
        col: 1,
        text: "n/a".into(),
    })
    .unwrap();
    assert!(s
        .entity_info(h, &[])
        .unwrap()
        .table
        .unwrap()
        .fields
        .is_empty());
}

#[test]
fn wire_forms() {
    let cmd: Command = serde_json::from_str(
        r#"{"type":"table","at":[0,0],"rows":2,"cols":3,"col_width":4,"row_height":1}"#,
    )
    .unwrap();
    assert!(matches!(
        cmd,
        Command::Table {
            rows: 2,
            cols: 3,
            ..
        }
    ));
    let cmd: Command = serde_json::from_str(
        r#"{"type":"edit_cell","target":{"handle":42},"row":1,"col":0,"text":"x"}"#,
    )
    .unwrap();
    assert!(matches!(cmd, Command::EditCell { row: 1, .. }));
    for wire in [
        r#"{"type":"insert_row","target":{"handle":42},"at":1}"#,
        r#"{"type":"insert_col","target":{"handle":42},"at":1}"#,
        r#"{"type":"delete_row","target":{"handle":42},"row":1}"#,
        r#"{"type":"delete_col","target":{"handle":42},"col":1}"#,
        r#"{"type":"merge_cells","target":{"handle":42},"first":[0,0],"last":[1,1]}"#,
        r#"{"type":"update_fields"}"#,
        r#"{"type":"field","at":[0,0],"height":0.25,"field":{"kind":"date"}}"#,
        r#"{"type":"field","at":[0,0],"height":0.25,"field":{"kind":"file_name"}}"#,
        r#"{"type":"field","at":[0,0],"height":0.25,"field":{"kind":"count","block":"DOOR"}}"#,
        r#"{"type":"field","at":[0,0],"height":0.25,"field":{"kind":"area","target":{"handle":42}}}"#,
        r#"{"type":"field","at":[0,0],"height":0.25,"field":{"kind":"length","target":{"handle":42}}}"#,
    ] {
        serde_json::from_str::<Command>(wire).unwrap_or_else(|e| panic!("{wire}: {e}"));
    }
}

/// The texts of the block a table refers to, found as a DWG reader leaves it: by its record
/// handle when it has no block name.
fn record_texts(s: &Session, h: u64) -> (Handle, Vec<String>) {
    let tb = table(s, h);
    let doc = &s.drawing.doc;
    let record = match tb.block_record_handle {
        Some(r) => doc.block_records.iter().find(|b| b.handle == r).unwrap(),
        None => doc.block_records.get(&tb.block_name).unwrap(),
    };
    let texts = doc
        .entities_in_block(&record.name)
        .filter_map(|e| match e {
            EntityType::MText(m) => Some(m.value.clone()),
            _ => None,
        })
        .collect();
    (record.handle, texts)
}

#[test]
fn a_copied_dwg_table_gets_a_block_of_its_own() {
    // An AutoCAD-like table: its *T1 block holds what it shows.
    let mut doc = sample_doc();
    let mut record = BlockRecord::new("*T1");
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    record.flags.anonymous = true;
    let owner = record.handle;
    doc.block_records.add(record).unwrap();
    let mut text = EntityType::MText(MText::with_value("A", Vector3::new(1.0, -0.5, 0.0)));
    text.common_mut().owner_handle = owner;
    doc.add_entity(text).unwrap();
    let mut tb = Table::new(Vector3::new(50.0, 0.0, 0.0), 0, 0);
    tb.rows = vec![TableRow::from_texts(&["A"])];
    tb.columns = vec![TableColumn::with_width(2.0)];
    tb.block_name = "*T1".to_string();
    tb.block_record_handle = Some(owner);
    let original = doc.add_entity(EntityType::Table(tb)).unwrap().value();
    let (_dir, mut s) = open(&doc, "plan.dwg");
    let (record, texts) = record_texts(&s, original);
    assert_eq!(texts, ["A"]);

    let copy = one(s
        .run(Command::Modify(ModifySpec::Copy {
            targets: vec![t(original)],
            delta: [0.0, -10.0],
            count: 1,
        }))
        .unwrap());
    let (copied, _) = record_texts(&s, copy);
    assert_ne!(copied, record, "the copy has a block of its own");
    s.run(Command::EditCell {
        target: t(copy),
        row: 0,
        col: 0,
        text: "Z".into(),
    })
    .unwrap();
    assert_eq!(record_texts(&s, original), (record, vec!["A".to_string()]));
    assert_eq!(record_texts(&s, copy).1, ["Z"]);

    // Two tables sharing one block (as an older copy left them): editing one gives it a new block.
    let mut shared = table(&s, original);
    shared.common.handle = Handle::NULL;
    shared.insertion_point = Vector3::new(80.0, 0.0, 0.0);
    let twin = s
        .drawing
        .doc
        .add_entity(EntityType::Table(shared))
        .unwrap()
        .value();
    s.run(Command::EditCell {
        target: t(twin),
        row: 0,
        col: 0,
        text: "Y".into(),
    })
    .unwrap();
    assert_eq!(record_texts(&s, original), (record, vec!["A".to_string()]));
    let (own, texts) = record_texts(&s, twin);
    assert_ne!(own, record);
    assert_eq!(texts, ["Y"]);
}

#[test]
fn area_and_length_fields_read_in_metres() {
    // The sample in millimetres: its 10-unit line is 0.010 m long.
    let mut doc = sample_doc();
    doc.header.insertion_units = 4;
    let mut square = opencadcodec::LwPolyline::new();
    for (x, y) in [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)] {
        square.add_point(opencadcodec::Vector2::new(x, y));
    }
    square.is_closed = true;
    let square = doc
        .add_entity(EntityType::LwPolyline(square))
        .unwrap()
        .value();
    let (_dir, mut s) = open(&doc, "plan.dxf");
    // A 1000 x 1000 mm square is 1 m2.
    let area = one(s
        .run(Command::Field {
            at: [0.0, 4.0],
            height: 2.5,
            field: FieldKind::Area { target: t(square) },
        })
        .unwrap());
    assert_eq!(text_of(&s, area), "1.000");
    let line = wall_line(&s);
    let h = one(s
        .run(Command::Field {
            at: [0.0, 5.0],
            height: 2.5,
            field: FieldKind::Length { target: t(line) },
        })
        .unwrap());
    assert_eq!(text_of(&s, h), "0.010");
}

#[test]
fn find_and_replace_leaves_fields_alone_and_says_so() {
    let (_dir, mut s) = open(&sample_doc(), "plan.dxf");
    let rect = rectangle(&s);
    let code = field_code(&FieldKind::Area { target: t(rect) });
    let text = one(s
        .run(Command::Text {
            at: [0.0, 5.0],
            height: 0.25,
            rotation: 0.0,
            value: format!("m2 {code}"),
            style: None,
        })
        .unwrap());
    let h = new_table(&mut s, [50.0, 0.0], &[&["m2 note", ""]]);
    s.run(Command::EditCell {
        target: t(h),
        row: 0,
        col: 1,
        text: format!("{code} m2"),
    })
    .unwrap();
    let replace = |s: &mut Session| {
        s.run(Command::FindReplace {
            find: "m2".into(),
            replace: "sqm".into(),
            scope: TextScope::Sheet,
            match_case: true,
            whole_word: false,
        })
        .unwrap()
    };
    match replace(&mut s) {
        Outcome::Replaced {
            count,
            handles,
            skipped,
        } => {
            assert_eq!(count, 1, "the table's plain cell");
            assert_eq!(handles, [h]);
            assert_eq!(skipped, 2, "the field text and the field cell");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(text_of(&s, text), "m2 12.000");
    assert_eq!(texts(&table(&s, h)), [["sqm note", "12.000 m2"]]);
}

/// A cell whose text wraps makes its row taller, as AutoCAD's tables do: a schedule's "Main
/// Entrance Door" in a 2.4 m column once ran over the row under it. Short rows keep their height.
#[test]
fn a_row_grows_to_hold_its_wrapped_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::new_blank(&dir.path().join("t.dwg"), 6);
    let Outcome::Added { handles } = s
        .run(Command::Table {
            at: [0.0, 0.0],
            rows: 2,
            cols: 2,
            col_width: 2.4,
            row_height: 0.5,
            data: vec![
                vec![
                    "D1".into(),
                    "Main Entrance Door, solid oak, 1.20 x 2.20 m".into(),
                ],
                vec!["D2".into(), "Flush".into()],
            ],
        })
        .unwrap()
    else {
        panic!("a table");
    };
    let Some(EntityType::Table(table)) = s.drawing.doc.get_entity(Handle::new(handles[0])) else {
        panic!("a table");
    };
    assert!(
        table.rows[0].height > 0.5 * 2.0,
        "the long text's row: {}",
        table.rows[0].height
    );
    assert!(
        (table.rows[1].height - 0.5).abs() < 1e-9,
        "a short row keeps 0.5: {}",
        table.rows[1].height
    );
}
