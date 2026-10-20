use qiyas_core::{Command, CoreError, Session};

/// Opens the sample drawing (DXF) in a session. The temp dir must outlive the test body.
fn sample_session() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_sample(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    (dir, session)
}

#[test]
fn session_reports_info_and_switches_sheet() {
    let (_dir, mut session) = sample_session();
    let info = session.info();
    assert!(info.path.ends_with("sample.dxf"), "{}", info.path);
    assert_eq!(info.units, "metres");
    assert_eq!(info.sheets[0].block, "*Model_Space");
    let layers: Vec<_> = info.layers.iter().map(|l| l.name.as_str()).collect();
    assert!(
        layers.contains(&"A-WALL") && layers.contains(&"A-DOOR"),
        "{layers:?}"
    );
    assert_eq!(session.sheet, "*Model_Space");

    session.set_sheet("*Model_Space").unwrap();
    assert!(
        matches!(session.set_sheet("NOPE"), Err(CoreError::NoSuchSheet(name)) if name == "NOPE")
    );
    assert_eq!(session.sheet, "*Model_Space");
}

#[test]
fn hiding_a_layer_changes_the_batch() {
    let (_dir, mut session) = sample_session();
    assert_eq!(session.batch().ranges.len(), 7);

    session.set_layer_hidden("A-WALL", true);
    assert_eq!(session.batch().ranges.len(), 4);
    assert_eq!(session.hidden_layers(), ["A-WALL"]);
    // The rectangle is gone, so nothing is picked on its edge.
    assert_eq!(session.pick([4.0, 1.5], 0.05), None);

    // Layer names match case-insensitively, as in DXF.
    session.set_layer_hidden("a-wall", false);
    assert_eq!(session.batch().ranges.len(), 7);
    assert!(session.hidden_layers().is_empty());
}

#[test]
fn entity_info_for_rectangle() {
    let (_dir, session) = sample_session();
    // (2, 0) lies on both the line and the rectangle; the rectangle's right edge is its own.
    let picked = session
        .pick([4.0, 1.5], 0.05)
        .expect("rectangle edge is picked");
    // On the sheet itself: no chain, and the entity is the top-level one.
    assert!(picked.chain.is_empty());
    assert_eq!(picked.top, picked.entity);
    let handle = picked.entity;
    let info = session.entity_info(handle, &[]).unwrap();
    assert_eq!(info.handle, handle);
    assert_eq!(info.inside_block, None);
    assert_eq!(info.kind, "LwPolyline");
    assert_eq!(info.layer, "A-WALL");
    assert_eq!(info.area, Some(12.0));
    assert_eq!(info.length, Some(14.0));
    assert_eq!(info.block_name, None);
    assert_eq!(info.text, None);

    assert!(matches!(
        session.entity_info(0xDEAD_BEEF, &[]),
        Err(CoreError::NoSuchEntity(0xDEAD_BEEF))
    ));
}

#[test]
fn batch_version_ignores_the_take_off_and_follows_the_drawing_view() {
    let (_dir, mut session) = sample_session();
    let v0 = session.batch_version();
    assert_eq!(session.batch_version(), v0);
    // The window holds it as a JavaScript number, which is exact up to 2^53.
    assert!(v0 < 1 << 53);

    // A measurement and its undo and redo do not change what is drawn.
    session
        .run(Command::MeasureDistance {
            points: vec![[0.0, 0.0], [3.0, 4.0]].into(),
        })
        .unwrap();
    assert_eq!(session.batch_version(), v0);
    assert!(session.undo());
    assert_eq!(session.batch_version(), v0);
    assert!(session.redo());
    assert_eq!(session.batch_version(), v0);

    // Hiding a layer does, and showing it again gives the first version back.
    session.set_layer_hidden("A-WALL", true);
    let hidden = session.batch_version();
    assert_ne!(hidden, v0);
    session.set_layer_hidden("a-wall", false);
    assert_eq!(session.batch_version(), v0);

    // So does another sheet.
    let paper = session
        .info()
        .sheets
        .into_iter()
        .find(|s| !s.is_model)
        .expect("the sample has a paper layout");
    session.set_sheet(&paper.block).unwrap();
    assert_ne!(session.batch_version(), v0);
    assert_ne!(session.batch_version(), hidden);
    session.set_sheet("*Model_Space").unwrap();
    assert_eq!(session.batch_version(), v0);
}

#[test]
fn saving_changes_the_batch_version_and_another_session_never_shares_it() {
    let (dir, mut session) = sample_session();
    let v0 = session.batch_version();
    // The save embeds the (empty) take-off, but still counts as a change to the drawing.
    session.save().unwrap();
    assert_ne!(session.batch_version(), v0);

    let again = Session::open(&dir.path().join("sample.dxf")).unwrap();
    let other = Session::open(&dir.path().join("sample.dxf")).unwrap();
    assert_ne!(again.batch_version(), other.batch_version());
}

#[test]
fn batch_bytes_are_sent_only_when_the_version_differs() {
    let (_dir, mut session) = sample_session();
    let v0 = session.batch_version();
    let full = session.batch().to_bytes();
    assert_eq!(session.batch_bytes_if(None, None), Some(full.clone()));
    assert_eq!(session.batch_bytes_if(Some(v0 ^ 1), None), Some(full));
    assert_eq!(session.batch_bytes_if(Some(v0), None), None);

    // A layer click changes the version, so the old one gets the new batch.
    session.set_layer_hidden("A-WALL", true);
    let bytes = session
        .batch_bytes_if(Some(v0), None)
        .expect("batch changed");
    assert_eq!(bytes, session.batch().to_bytes());
    assert_eq!(
        session.batch_bytes_if(Some(session.batch_version()), None),
        None
    );
}

#[test]
fn the_arabic_setting_redraws_text_is_saved_and_is_no_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_arabic(dir.path(), "dxf");
    let mut session = Session::open(&path).unwrap();
    assert!(!session.arabic_reversed);
    assert!(!session.info().arabic_reversed);
    let v0 = session.batch_version();
    let before = session.batch();
    assert_eq!(before.text_ranges.len(), 3);
    assert!(!before.quads.is_empty());

    session
        .run(Command::SetArabicReversed { on: true })
        .unwrap();
    assert!(session.arabic_reversed);
    assert!(!session.can_undo());
    assert_ne!(session.batch_version(), v0);
    // Same glyph count, other glyphs for the Arabic texts.
    let after = session.batch();
    assert_eq!(after.quads.len(), before.quads.len());
    assert_ne!(after.quads, before.quads);

    session.save_project().unwrap();
    let again = Session::open(&path).unwrap();
    assert!(again.arabic_reversed);
    assert!(again.info().arabic_reversed);
}

#[test]
fn glyphs_are_drawn_again_for_another_zoom_and_atlas_pages_follow() {
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_arabic(dir.path(), "dxf");
    let session = Session::open(&path).unwrap();
    let first = session.batch();
    assert!(first.px_per_unit > 0.0);
    assert!(first.atlas_version >= 1);
    // Every page is sent the first time; none once the window holds this version.
    let all = session.atlas_bytes(None);
    let pages = u32::from_le_bytes(all[8..12].try_into().unwrap());
    assert_eq!(pages, first.atlas_pages);
    assert_eq!(session.atlas_bytes(Some(first.atlas_version)).len(), 12);

    let v = session.batch_version();
    let bytes = session
        .batch_bytes_if(None, Some(first.px_per_unit * 16.0))
        .unwrap();
    // The zoom is not part of the version: the window decides when to ask.
    assert_eq!(session.batch_version(), v);
    let zoomed = session.batch();
    assert_eq!(bytes, zoomed.to_bytes());
    assert_eq!(zoomed.px_per_unit, first.px_per_unit * 16.0);
    assert_eq!(zoomed.segments, first.segments);
    assert!(zoomed.atlas_version > first.atlas_version);
    assert!(session.atlas_bytes(Some(first.atlas_version)).len() > 12);
    // Without a zoom the batch keeps the one it has.
    session.batch_bytes_if(None, None).unwrap();
    assert_eq!(session.batch().px_per_unit, zoomed.px_per_unit);
}

#[test]
fn entity_info_reports_colour_linetype_weight_and_attributes() {
    use opencadcodec::entities::AttributeEntity;
    use opencadcodec::{Color, EntityType, LineWeight};

    // The sample with the rectangle in true colour, dashed and 0.35 mm; the arc blue (ACI 5); the
    // first DOOR reference cyan (ACI 4) with a MARK attribute, and the door leaf ByBlock.
    let mut doc = qiyas_core::fixtures::sample_doc();
    let handles: Vec<(u64, &'static str)> = doc
        .entities_in_block("*Model_Space")
        .map(|e| {
            let kind = match e {
                EntityType::Line(_) => "line",
                EntityType::LwPolyline(_) => "rectangle",
                EntityType::Arc(_) => "arc",
                EntityType::Insert(_) => "door",
                EntityType::Text(_) => "text",
                _ => "other",
            };
            (e.common().handle.value(), kind)
        })
        .collect();
    let find = |kind: &str| handles.iter().find(|(_, k)| *k == kind).unwrap().0;
    let (line, rectangle, arc, door, text) = (
        find("line"),
        find("rectangle"),
        find("arc"),
        find("door"),
        find("text"),
    );
    fn get(doc: &mut opencadcodec::CadDocument, h: u64) -> &mut EntityType {
        doc.get_entity_mut(opencadcodec::Handle::new(h)).unwrap()
    }
    let common = get(&mut doc, rectangle).common_mut();
    common.color = Color::Rgb {
        r: 0x12,
        g: 0x34,
        b: 0x56,
    };
    common.linetype = "DASHED".to_string();
    common.line_weight = LineWeight::Value(35);
    get(&mut doc, arc).common_mut().color = Color::Index(5);
    let EntityType::Insert(insert) = get(&mut doc, door) else {
        unreachable!()
    };
    insert.common.color = Color::Index(4);
    let at = insert.insert_point;
    insert.attributes.push(
        AttributeEntity::new("MARK".to_string(), "D1".to_string())
            .with_position(at)
            .with_height(0.2),
    );
    let leaf = doc.block_records.get("DOOR").unwrap().entity_handles[0].value();
    get(&mut doc, leaf).common_mut().color = Color::ByBlock;
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_drawing(&doc, &dir.path().join("props.dxf"));
    let session = Session::open(&path).unwrap();

    // ByLayer takes the layer's colour (A-WALL is red).
    let info = session.entity_info(line, &[]).unwrap();
    assert_eq!(info.color, "ByLayer");
    assert_eq!(info.color_rgb, [255, 0, 0]);
    assert_eq!(info.linetype, "ByLayer");
    assert_eq!(info.lineweight, -1);
    assert_eq!(info.radius, None);
    assert!(info.attributes.is_empty());

    let info = session.entity_info(rectangle, &[]).unwrap();
    assert_eq!(info.color, "#123456");
    assert_eq!(info.color_rgb, [0x12, 0x34, 0x56]);
    assert_eq!(info.linetype, "DASHED");
    assert_eq!(info.lineweight, 35);

    let info = session.entity_info(arc, &[]).unwrap();
    assert_eq!(info.color, "ACI 5");
    assert_eq!(info.color_rgb, [0, 0, 255]);
    assert_eq!(info.radius, Some(2.0));

    let info = session.entity_info(door, &[]).unwrap();
    assert_eq!(info.color, "ACI 4");
    assert_eq!(info.block_name.as_deref(), Some("DOOR"));
    assert_eq!(info.attributes, [("MARK".to_string(), "D1".to_string())]);

    // ByBlock inside the reference takes the reference's colour.
    let info = session.entity_info(leaf, &[door]).unwrap();
    assert_eq!(info.color, "ByBlock");
    assert_eq!(info.color_rgb, [0, 255, 255]);

    let info = session.entity_info(text, &[]).unwrap();
    assert_eq!(info.height, Some(0.5));
    assert!(info.style.is_some());
}

/// Adds a block `name` holding `entity` and returns the entity's handle.
fn add_block(
    doc: &mut opencadcodec::CadDocument,
    name: &str,
    entity: opencadcodec::EntityType,
) -> u64 {
    let mut block = opencadcodec::BlockRecord::new(name);
    block.handle = doc.allocate_handle();
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    let mut entity = entity;
    entity.common_mut().owner_handle = owner;
    doc.add_entity(entity).unwrap().value()
}

#[test]
fn entity_info_resolves_by_block_colours_as_drawn() {
    use opencadcodec::entities::Insert;
    use opencadcodec::{Color, EntityType, Line, Vector3};

    let line = |color: Color| {
        let mut l = Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        l.common.layer = "A-WALL".to_string();
        l.common.color = color;
        EntityType::Line(l)
    };
    let reference = |block: &str, layer: &str, color: Color| {
        let mut i = Insert::new(block, Vector3::new(0.0, 0.0, 0.0));
        i.common.layer = layer.to_string();
        i.common.color = color;
        EntityType::Insert(i)
    };
    let mut doc = qiyas_core::fixtures::sample_doc();
    // A ByBlock line on A-WALL (red) in LEAF; LEAF referenced ByBlock on A-WALL inside FRAME.
    let leaf_line = add_block(&mut doc, "LEAF", line(Color::ByBlock));
    let inner = add_block(
        &mut doc,
        "FRAME",
        reference("LEAF", "A-WALL", Color::ByBlock),
    );
    // On the sheet: LEAF on A-DOOR (green) ByLayer; FRAME on A-DOOR ByLayer; FRAME in blue.
    let one = doc
        .add_entity(reference("LEAF", "A-DOOR", Color::ByLayer))
        .unwrap()
        .value();
    let two = doc
        .add_entity(reference("FRAME", "A-DOOR", Color::ByLayer))
        .unwrap()
        .value();
    let blue = doc
        .add_entity(reference("FRAME", "A-DOOR", Color::Index(5)))
        .unwrap()
        .value();
    let dir = tempfile::tempdir().unwrap();
    let path = qiyas_core::fixtures::write_drawing(&doc, &dir.path().join("byblock.dxf"));
    let session = Session::open(&path).unwrap();

    // ByBlock in a ByLayer reference takes the reference's layer colour, not its own layer's.
    let info = session.entity_info(leaf_line, &[one]).unwrap();
    assert_eq!(info.color, "ByBlock");
    assert_eq!(info.layer, "A-WALL");
    assert_eq!(info.color_rgb, [0, 255, 0]);

    // Two levels: ByBlock in a ByBlock reference in a ByLayer one on A-DOOR: green.
    let info = session.entity_info(leaf_line, &[two, inner]).unwrap();
    assert_eq!(info.color_rgb, [0, 255, 0]);
    // The same with the outer reference blue.
    let info = session.entity_info(leaf_line, &[blue, inner]).unwrap();
    assert_eq!(info.color_rgb, [0, 0, 255]);
}
