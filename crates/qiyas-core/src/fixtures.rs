//! A small synthetic drawing for tests. No customer drawing is ever committed.

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use opencadcodec::entities::{
    AttributeEntity, Dimension, DimensionLinear, Insert, Table, TableColumn, TableRow,
};
use opencadcodec::{
    Arc, BlockRecord, CadDocument, Color, DwgWriter, DxfWriter, EntityType, Layer, Line,
    LwPolyline, MText, Text, Vector2, Vector3,
};

/// Layers A-WALL (red) and A-DOOR (green); a line, a closed rectangle and an arc on A-WALL; three DOOR
/// references on A-DOOR (plain, rotated a quarter turn, scaled 2); a title text. Units are metres.
pub fn sample_doc() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    add_layers(&mut doc);
    let leaf = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 0.9, 0.0, 0.0));
    add_block(&mut doc, "DOOR", leaf);

    let mut rectangle = LwPolyline::new();
    for (x, y) in [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)] {
        rectangle.add_point(Vector2::new(x, y));
    }
    rectangle.is_closed = true;
    let walls = [
        EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 10.0, 0.0, 0.0)),
        EntityType::LwPolyline(rectangle),
        EntityType::Arc(Arc::from_center_radius_angles(
            Vector3::new(20.0, 0.0, 0.0),
            2.0,
            0.0,
            PI,
        )),
    ];
    let doors = [
        Insert::new("DOOR", Vector3::new(1.0, 1.0, 0.0)),
        Insert::new("DOOR", Vector3::new(5.0, 1.0, 0.0)).with_rotation(PI / 2.0),
        Insert::new("DOOR", Vector3::new(9.0, 1.0, 0.0)).with_scale(2.0, 2.0, 2.0),
    ];
    for (layer, entity) in walls
        .into_iter()
        .map(|e| ("A-WALL", e))
        .chain(doors.into_iter().map(|i| ("A-DOOR", EntityType::Insert(i))))
    {
        let mut entity = entity;
        entity.common_mut().layer = layer.to_string();
        doc.add_entity(entity).unwrap();
    }
    doc.add_entity(EntityType::Text(
        Text::with_value("GROUND FLOOR PLAN", Vector3::new(0.0, -2.0, 0.0)).with_height(0.5),
    ))
    .unwrap();
    doc
}

/// The sample's layers, then a door schedule drawn as loose text: the title "DOOR SCHEDULE" at
/// (0, 3.6), header MARK | WIDTH | QTY at y 3.0 and rows D1, D2, D3 below it on a 2.0 column and
/// 0.5 row pitch, text height 0.25. Also a 2x2 table entity (A B / C D) at (20, 0), a linear
/// dimension from (0, -5) to (4, -5), a formatted MText note at (0, -8), and a TAG block (a text
/// "T1" inside) inserted at (30, 10) with attribute MARK = D9. Units are metres.
pub fn schedule_doc() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    add_layers(&mut doc);
    let text = |value: &str, x: f64, y: f64, height: f64| {
        EntityType::Text(Text::with_value(value, Vector3::new(x, y, 0.0)).with_height(height))
    };
    doc.add_entity(text("DOOR SCHEDULE", 0.0, 3.6, 0.35))
        .unwrap();
    let grid = [
        ["MARK", "WIDTH", "QTY"],
        ["D1", "0.9", "2"],
        ["D2", "1.0", "1"],
        ["D3", "1.2", "4"],
    ];
    for (r, row) in grid.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            let entity = text(value, 2.0 * c as f64, 3.0 - 0.5 * r as f64, 0.25);
            doc.add_entity(entity).unwrap();
        }
    }

    let mut table = Table::new(Vector3::new(20.0, 0.0, 0.0), 0, 0);
    table.rows = vec![
        TableRow::from_texts(&["A", "B"]),
        TableRow::from_texts(&["C", "D"]),
    ];
    table.columns = vec![TableColumn::with_width(2.0), TableColumn::with_width(2.0)];
    doc.add_entity(EntityType::Table(table)).unwrap();

    let dimension =
        DimensionLinear::new(Vector3::new(0.0, -5.0, 0.0), Vector3::new(4.0, -5.0, 0.0));
    doc.add_entity(EntityType::Dimension(Dimension::Linear(dimension)))
        .unwrap();

    let note = MText::with_value("{\\C1;FIRE} RATED\\PDOORS", Vector3::new(0.0, -8.0, 0.0))
        .with_height(0.25);
    doc.add_entity(EntityType::MText(note)).unwrap();

    add_block(&mut doc, "TAG", text("T1", 0.0, 0.0, 0.2));
    let mut tag = Insert::new("TAG", Vector3::new(30.0, 10.0, 0.0));
    tag.attributes.push(
        AttributeEntity::new("MARK".to_string(), "D9".to_string())
            .with_position(Vector3::new(30.5, 10.0, 0.0))
            .with_height(0.2),
    );
    doc.add_entity(EntityType::Insert(tag)).unwrap();
    doc
}

/// Arabic room labels on the sample's layers, for drawing Arabic text: a room outline on A-WALL
/// with "غرفة المعيشة" (living room, 0.6 tall) inside it at (1, 2), "مطبخ" (kitchen) at (7, 2)
/// stored backwards (letter by letter in visual order, as old CAD fonts needed it), and the
/// mixed "Door D1 باب" at (1, -1), 0.4 tall, on A-DOOR. Units are metres.
pub fn arabic_text_doc() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    add_layers(&mut doc);
    let mut room = LwPolyline::new();
    for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)] {
        room.add_point(Vector2::new(x, y));
    }
    room.is_closed = true;
    let kitchen_backwards: String = "مطبخ".chars().rev().collect();
    let entities = [
        ("A-WALL", EntityType::LwPolyline(room)),
        (
            "A-WALL",
            EntityType::Text(
                Text::with_value("غرفة المعيشة", Vector3::new(1.0, 2.0, 0.0)).with_height(0.6),
            ),
        ),
        (
            "A-WALL",
            EntityType::Text(
                Text::with_value(kitchen_backwards, Vector3::new(7.0, 2.0, 0.0)).with_height(0.6),
            ),
        ),
        (
            "A-DOOR",
            EntityType::Text(
                Text::with_value("Door D1 باب", Vector3::new(1.0, -1.0, 0.0)).with_height(0.4),
            ),
        ),
    ];
    for (layer, mut entity) in entities {
        entity.common_mut().layer = layer.to_string();
        doc.add_entity(entity).unwrap();
    }
    doc
}

/// Writes the Arabic text drawing as `arabic.<ext>` in `dir`.
pub fn write_arabic(dir: &Path, ext: &str) -> PathBuf {
    write_doc(&arabic_text_doc(), &dir.join(format!("arabic.{ext}")))
}

/// Layers A-WALL (red) and A-DOOR (green).
fn add_layers(doc: &mut CadDocument) {
    for (name, index) in [("A-WALL", 1), ("A-DOOR", 3)] {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = Color::Index(index);
        doc.layers.add(layer).unwrap();
    }
}

/// A block named `name` holding one entity.
fn add_block(doc: &mut CadDocument, name: &str, entity: EntityType) {
    let mut block = BlockRecord::new(name);
    block.handle = doc.allocate_handle();
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    let mut entity = entity;
    entity.common_mut().owner_handle = owner;
    doc.add_entity(entity).unwrap();
}

/// Adds to `doc` an xref named `name` stored as `path` (a block record flagged as an xref, with
/// no content) and a reference to it in model space at `at`, turned `rotation` radians, on layer
/// `X-REF`. Returns the reference's handle.
pub fn add_xref(doc: &mut CadDocument, name: &str, path: &str, at: [f64; 2], rotation: f64) -> u64 {
    let mut block = BlockRecord::new(name);
    block.handle = doc.allocate_handle();
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    block.flags.is_xref = true;
    block.xref_path = path.to_string();
    doc.block_records.add(block).unwrap();
    let mut insert = Insert::new(name, Vector3::new(at[0], at[1], 0.0)).with_rotation(rotation);
    insert.common.layer = "X-REF".to_string();
    doc.add_entity(EntityType::Insert(insert)).unwrap().value()
}

/// A host drawing in `dir`: `host.dxf`, with the sample's layers, a line of its own on A-WALL
/// from (0, -10) to (5, -10), and the xref `plan-arch` stored as `plan-arch.dxf`, referenced at
/// (100, 0) turned a quarter turn; and the xref itself, `plan-arch.dxf`, the sample drawing.
/// Returns the host's path. Units are metres.
pub fn write_xref_pair(dir: &Path) -> PathBuf {
    let mut host = CadDocument::new();
    host.header.insertion_units = 6;
    add_layers(&mut host);
    let mut own = EntityType::Line(Line::from_coords(0.0, -10.0, 0.0, 5.0, -10.0, 0.0));
    own.common_mut().layer = "A-WALL".to_string();
    host.add_entity(own).unwrap();
    add_xref(
        &mut host,
        "plan-arch",
        "plan-arch.dxf",
        [100.0, 0.0],
        PI / 2.0,
    );
    write_doc(&sample_doc(), &dir.join("plan-arch.dxf"));
    write_doc(&host, &dir.join("host.dxf"))
}

/// Writes `doc` to `path`, DXF or DWG by extension.
pub fn write_drawing(doc: &CadDocument, path: &Path) -> PathBuf {
    write_doc(doc, path)
}

/// Writes the schedule drawing as `schedule.<ext>` in `dir`.
pub fn write_schedule(dir: &Path, ext: &str) -> PathBuf {
    write_doc(&schedule_doc(), &dir.join(format!("schedule.{ext}")))
}

/// Writes the sample as `sample.dxf` or `sample.dwg` in `dir`.
pub fn write_sample(dir: &Path, ext: &str) -> PathBuf {
    write_doc(&sample_doc(), &dir.join(format!("sample.{ext}")))
}

/// Writes `doc` to `path`, DXF or DWG by extension.
fn write_doc(doc: &CadDocument, path: &Path) -> PathBuf {
    let dxf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
    if dxf {
        DxfWriter::new(doc).write_to_file(path).unwrap();
    } else {
        DwgWriter::write_to_file(path, doc).unwrap();
    }
    path.to_path_buf()
}

/// Writes `dir/name`, a 200 x 100 pixel PNG: red on the left half, blue on the right.
pub fn write_png(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let image = ::image::RgbaImage::from_fn(200, 100, |x, _| {
        if x < 100 {
            ::image::Rgba([255, 0, 0, 255])
        } else {
            ::image::Rgba([0, 0, 255, 255])
        }
    });
    image.save(&path).expect("the PNG is written");
    path
}

thread_local! {
    static PANIC_AFTER_EDIT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Test-only fault injection on this thread, after the nth document mutation and before
/// derivation. The hook clears itself before unwinding and cannot be enabled by wire input.
pub fn panic_on_document_edit(n: usize) {
    assert!(n > 0);
    PANIC_AFTER_EDIT.set(Some(n));
}

pub(crate) fn panic_after_document_edit() {
    match PANIC_AFTER_EDIT.get() {
        Some(1) => {
            PANIC_AFTER_EDIT.set(None);
            panic!("injected document edit panic");
        }
        Some(n) => PANIC_AFTER_EDIT.set(Some(n - 1)),
        None => {}
    }
}

/// Nested timers enabled explicitly by offline performance fixtures.
pub mod profile;
