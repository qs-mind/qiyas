//! Writes a large synthetic floor-plan drawing for timing the engine and the window.
//!
//! `cargo run --release -p qiyas-core --example bench_gen -- <out.dwg|out.dxf> [rooms]`
//!
//! Each room is a closed 24-vertex wall polyline, 8 lines, 4 arcs, 3 circles, two texts and a
//! door block reference (the block holds 12 lines), on four layers, laid out on a grid in metres.
//! 2,000 rooms (the default) give about 40,000 entities and about 1.3 million segments once the
//! block references are flattened. Deterministic: the same arguments write the same drawing.
//! No customer drawing is involved.

use std::f64::consts::PI;
use std::path::PathBuf;

use opencadcodec::entities::Insert;
use opencadcodec::{
    Arc, BlockRecord, CadDocument, Circle, Color, DwgWriter, DxfWriter, EntityType, Layer, Line,
    LwPolyline, Text, Vector2, Vector3,
};
use qiyas_core::Session;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(
        args.next()
            .expect("usage: bench_gen <out.dwg|out.dxf> [rooms]"),
    );
    let rooms: usize = args
        .next()
        .map_or(2000, |n| n.parse().expect("rooms is a number"));

    let mut doc = CadDocument::new();
    doc.header.insertion_units = 6;
    for (name, index) in [("A-WALL", 1), ("A-DOOR", 3), ("A-FURN", 5), ("A-ANNO", 7)] {
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        layer.color = Color::Index(index);
        doc.layers.add(layer).unwrap();
    }
    let mut block = BlockRecord::new("DOOR");
    block.handle = doc.allocate_handle();
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    for i in 0..12 {
        let y = i as f64 * 0.075;
        let mut leaf = EntityType::Line(Line::from_coords(0.0, y, 0.0, 0.9, y, 0.0));
        leaf.common_mut().owner_handle = owner;
        doc.add_entity(leaf).unwrap();
    }

    let columns = (rooms as f64).sqrt().ceil() as usize;
    for r in 0..rooms {
        let (ox, oy) = ((r % columns) as f64 * 12.0, (r / columns) as f64 * 9.0);
        let mut put = |layer: &str, entity: EntityType| {
            let mut entity = entity;
            entity.common_mut().layer = layer.to_string();
            doc.add_entity(entity).unwrap();
        };
        let mut wall = LwPolyline::new();
        for k in 0..24 {
            let a = k as f64 * 2.0 * PI / 24.0;
            wall.add_point(Vector2::new(
                ox + 5.0 + 4.5 * a.cos(),
                oy + 4.0 + 3.5 * a.sin(),
            ));
        }
        wall.is_closed = true;
        put("A-WALL", EntityType::LwPolyline(wall));
        for k in 0..8 {
            let x = ox + k as f64 * 1.2;
            put(
                "A-FURN",
                EntityType::Line(Line::from_coords(x, oy, 0.0, x, oy + 8.0, 0.0)),
            );
        }
        for k in 0..4 {
            let centre = Vector3::new(ox + 2.0 + 2.0 * k as f64, oy + 4.0, 0.0);
            put(
                "A-FURN",
                EntityType::Arc(Arc::from_center_radius_angles(centre, 0.9, 0.0, PI)),
            );
        }
        for k in 0..3 {
            let centre = Vector3::new(ox + 3.0 + 2.0 * k as f64, oy + 6.0, 0.0);
            put(
                "A-FURN",
                EntityType::Circle(Circle::from_center_radius(centre, 0.4)),
            );
        }
        let label = format!("ROOM {r}");
        put(
            "A-ANNO",
            EntityType::Text(
                Text::with_value(&label, Vector3::new(ox + 3.0, oy + 4.0, 0.0)).with_height(0.3),
            ),
        );
        put(
            "A-ANNO",
            EntityType::Text(
                Text::with_value("12.50 m2", Vector3::new(ox + 3.0, oy + 3.4, 0.0))
                    .with_height(0.2),
            ),
        );
        put(
            "A-DOOR",
            EntityType::Insert(Insert::new("DOOR", Vector3::new(ox + 1.0, oy + 0.5, 0.0))),
        );
    }

    let dxf = out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dxf"));
    if dxf {
        DxfWriter::new(&doc).write_to_file(&out).unwrap();
    } else {
        DwgWriter::write_to_file(&out, &doc).unwrap();
    }
    // Read it back as the window would, so the file is known to open.
    let session = Session::open(&out).expect("the written drawing opens");
    println!(
        "{}: {rooms} rooms, {} segments, {} bytes",
        out.display(),
        session.batch().segments.len() / 4,
        std::fs::metadata(&out).unwrap().len()
    );
}
