use opencadcodec::entities::{Insert, TextHorizontalAlignment, TextVerticalAlignment};
use opencadcodec::{BlockRecord, CadDocument, EntityType, MText, Text, Vector3};
use qiyas_core::fixtures::{sample_doc, schedule_doc};
use qiyas_render::{
    build_batch_with_text, place_text, Batch, Quad, TextEngine, TextOptions, Visibility, NO_CHAIN,
    PAGE_SIZE, QUAD_IN_BLOCK, SIZE_BUCKETS, TEXT_BOX,
};

const MODEL: &str = "*Model_Space";

fn text_batch(doc: &CadDocument, options: TextOptions) -> (Batch, TextEngine) {
    let mut engine = TextEngine::bundled_only();
    let batch = build_batch_with_text(
        doc,
        MODEL,
        &Visibility::default(),
        1e-3,
        &mut engine,
        options,
    );
    (batch, engine)
}

fn at_zoom(px: f64) -> TextOptions {
    TextOptions {
        arabic_reversed: false,
        px_per_unit: Some(px),
    }
}

/// The quads of the text range whose entity is `entity`.
fn quads_of(batch: &Batch, entity: u64) -> &[Quad] {
    let r = batch
        .text_ranges
        .iter()
        .find(|r| r.entity == entity)
        .unwrap_or_else(|| panic!("no text range for {entity}"));
    &batch.quads[r.first as usize..(r.first + r.count) as usize]
}

/// Absolute corners of `quads`, as (x, y) pairs.
fn corners(batch: &Batch, quads: &[Quad]) -> Vec<[f64; 2]> {
    quads
        .iter()
        .flat_map(|q| {
            q.corners.as_chunks::<2>().0.iter().map(|[x, y]| {
                [
                    f64::from(*x) + batch.origin[0],
                    f64::from(*y) + batch.origin[1],
                ]
            })
        })
        .collect()
}

/// Absolute end points of the segments of the range of `entity` (its box, for a text).
fn box_of(batch: &Batch, entity: u64) -> Vec<[f64; 2]> {
    let r = batch.ranges.iter().find(|r| r.entity == entity).unwrap();
    batch.segments[4 * r.first as usize..4 * (r.first + r.count) as usize]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[x, y]| {
            [
                f64::from(*x) + batch.origin[0],
                f64::from(*y) + batch.origin[1],
            ]
        })
        .collect()
}

fn add(doc: &mut CadDocument, entity: EntityType) -> u64 {
    doc.add_entity(entity).unwrap().value()
}

#[test]
fn sample_title_is_drawn_as_glyphs_and_its_box_is_picked_not_drawn() {
    let doc = sample_doc();
    let (batch, engine) = text_batch(&doc, TextOptions::default());
    let title = batch
        .ranges
        .iter()
        .find(|r| r.flags == TEXT_BOX)
        .expect("the title's box");
    assert_eq!(
        batch.ranges.iter().filter(|r| r.flags == TEXT_BOX).count(),
        1
    );
    // "GROUND FLOOR PLAN": 15 letters, the two spaces draw nothing.
    let quads = quads_of(&batch, title.entity);
    assert_eq!(quads.len(), 15);
    assert_eq!(batch.text_ranges.len(), 1);
    assert_eq!(batch.text_ranges[0].chain, NO_CHAIN);
    assert!(quads.iter().all(|q| q.flags == 0 && q.color == 0xFFFFFF));
    assert!(batch.atlas_version >= 1);
    assert_eq!(batch.atlas_pages, 1);
    assert_eq!(engine.atlas().version(), batch.atlas_version);
    for q in quads {
        let [u0, v0, u1, v1] = q.uv.map(u32::from);
        assert!(u0 < u1 && v0 < v1 && u1 <= PAGE_SIZE && v1 <= PAGE_SIZE);
    }
    // The glyphs sit on the baseline at y = -2, 0.5 tall, from x = 0 rightwards.
    let points = corners(&batch, quads);
    let min_y = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let max_y = points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_x = points.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    assert!((min_y - -2.0).abs() < 0.05, "{min_y}");
    assert!((max_y - -1.5).abs() < 0.1, "{max_y}");
    assert!(min_x > -0.05 && min_x < 0.1, "{min_x}");
    // The box spans the drawn text.
    let max_x = points
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let b = box_of(&batch, title.entity);
    let box_max_x = b.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    assert!((box_max_x - max_x).abs() < 0.1, "{box_max_x} {max_x}");
}

#[test]
fn right_aligned_text_ends_at_its_alignment_point() {
    let mut doc = CadDocument::new();
    let mut text = Text::with_value("WALL", Vector3::new(0.0, 0.0, 0.0)).with_height(1.0);
    text.horizontal_alignment = TextHorizontalAlignment::Right;
    text.alignment_point = Some(Vector3::new(10.0, 5.0, 0.0));
    let right = add(&mut doc, EntityType::Text(text));
    let mut centred = Text::with_value("WALL", Vector3::new(0.0, 0.0, 0.0)).with_height(1.0);
    centred.horizontal_alignment = TextHorizontalAlignment::Center;
    centred.vertical_alignment = TextVerticalAlignment::Middle;
    centred.alignment_point = Some(Vector3::new(20.0, 5.0, 0.0));
    let centre = add(&mut doc, EntityType::Text(centred));
    let (batch, _) = text_batch(&doc, at_zoom(40.0));

    let b = box_of(&batch, right);
    let box_max_x = b.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    assert!((box_max_x - 10.0).abs() < 1e-4, "{box_max_x}");
    let points = corners(&batch, quads_of(&batch, right));
    let ink_max_x = points
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max);
    // The last glyph's ink stops at its side bearing, just short of the point.
    assert!(ink_max_x <= 10.0 + 0.05 && ink_max_x > 9.8, "{ink_max_x}");
    let min_y = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    assert!((min_y - 5.0).abs() < 0.05, "baseline at the point: {min_y}");

    // Centred and middle: the box is centred on its point both ways.
    let b = box_of(&batch, centre);
    let (lo, hi) = b
        .iter()
        .fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        });
    assert!(((lo[0] + hi[0]) / 2.0 - 20.0).abs() < 1e-4);
    assert!(((lo[1] + hi[1]) / 2.0 - 5.0).abs() < 1e-4);
}

#[test]
fn text_inside_a_rotated_or_mirrored_block_turns_with_it() {
    let mut doc = CadDocument::new();
    let mut block = BlockRecord::new("LABEL");
    block.handle = doc.allocate_handle();
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    let owner = block.handle;
    doc.block_records.add(block).unwrap();
    let mut label = EntityType::Text(Text::with_value("AB", Vector3::new(0.0, 0.0, 0.0)));
    label.common_mut().owner_handle = owner;
    let inner = add(&mut doc, label);
    let turned = add(
        &mut doc,
        EntityType::Insert(
            Insert::new("LABEL", Vector3::new(10.0, 0.0, 0.0))
                .with_rotation(std::f64::consts::FRAC_PI_2),
        ),
    );
    let mirrored = add(
        &mut doc,
        EntityType::Insert(
            Insert::new("LABEL", Vector3::new(30.0, 0.0, 0.0)).with_scale(-1.0, 1.0, 1.0),
        ),
    );
    let (batch, _) = text_batch(&doc, at_zoom(40.0));
    assert_eq!(batch.text_ranges.len(), 2);
    for r in &batch.text_ranges {
        assert_eq!(r.entity, inner);
        assert_ne!(r.chain, NO_CHAIN);
    }
    let of = |top: u64| {
        let r = batch.text_ranges.iter().find(|r| r.top == top).unwrap();
        &batch.quads[r.first as usize..(r.first + r.count) as usize]
    };
    // Turned a quarter: the glyph's top edge (top-left to top-right) runs up the page.
    let q = of(turned)[0];
    assert_eq!(q.flags & QUAD_IN_BLOCK, QUAD_IN_BLOCK);
    let [x0, y0, x1, y1] = [q.corners[0], q.corners[1], q.corners[2], q.corners[3]];
    assert!((x1 - x0).abs() < 1e-4 && y1 > y0);
    // Mirrored: the top edge runs right to left, and the text sits left of the insert.
    let quads = of(mirrored);
    let q = quads[0];
    assert!(q.corners[2] < q.corners[0]);
    let points = corners(&batch, quads);
    // The quads reach a padding texel past the ink.
    assert!(points.iter().all(|p| p[0] <= 30.0 + 0.05));
}

#[test]
fn attributes_and_mtext_draw_their_plain_text() {
    let doc = schedule_doc();
    let (batch, _) = text_batch(&doc, at_zoom(40.0));
    let mtext = doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::MText(_)))
        .unwrap()
        .common()
        .handle
        .value();
    // "{\C1;FIRE} RATED\PDOORS": the codes are gone, two lines, 14 letters.
    let quads = quads_of(&batch, mtext);
    assert_eq!(quads.len(), 14);
    let points = corners(&batch, quads);
    let max_y = points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    // Top-left attachment at y = -8: the first line hangs below it, the second further down.
    assert!(max_y <= -8.0 + 0.05, "{max_y}");
    assert!(min_y < -8.0 - 0.25 * 2.0, "{min_y}");
    let insert = doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle
        .value();
    // The TAG insert draws the attribute D9 (picked as the insert) and the block's text T1.
    assert!(batch
        .text_ranges
        .iter()
        .any(|r| r.top == insert && r.entity == insert && r.chain == NO_CHAIN && r.count == 2));
    assert!(batch
        .text_ranges
        .iter()
        .any(|r| r.top == insert && r.entity != insert && r.chain != NO_CHAIN));
}

#[test]
fn another_zoom_draws_from_another_size_and_keeps_the_atlas_pages() {
    let doc = sample_doc();
    let (mut batch, mut engine) = text_batch(&doc, at_zoom(16.0));
    let before = batch.atlas_version;
    // 0.5 tall at 16 px per unit: 8 px glyphs.
    let size = |b: &Batch| {
        let q = b.quads[0];
        f64::from(q.uv[3] - q.uv[1])
    };
    let small = size(&batch);
    place_text(&mut batch, &mut engine, 128.0);
    assert_eq!(batch.px_per_unit, 128.0);
    assert!(size(&batch) > small * 4.0, "{} vs {small}", size(&batch));
    assert_eq!(batch.atlas_version, before + 1);
    // Back to the first zoom: the glyphs are in the atlas already, nothing changes.
    place_text(&mut batch, &mut engine, 16.0);
    assert_eq!(batch.atlas_version, before + 1);
    assert_eq!(size(&batch), small);
    assert_eq!(SIZE_BUCKETS[0], 8);
}

/// Arabic stored backwards, as old CAD fonts needed it, and the same word stored right.
#[test]
fn the_drawing_toggle_reads_all_base_letter_arabic_as_stored_backwards() {
    let mut doc = CadDocument::new();
    let backwards: String = "مطبخ".chars().rev().collect();
    let stored = add(
        &mut doc,
        EntityType::Text(Text::with_value(backwards, Vector3::new(0.0, 0.0, 0.0))),
    );
    let right = add(
        &mut doc,
        EntityType::Text(Text::with_value("مطبخ", Vector3::new(0.0, 5.0, 0.0))),
    );
    let mtext = add(
        &mut doc,
        EntityType::MText(MText::with_value("مطبخ", Vector3::new(0.0, 10.0, 0.0)).with_height(1.0)),
    );
    let uvs = |batch: &Batch, entity: u64| -> Vec<[u16; 4]> {
        quads_of(batch, entity).iter().map(|q| q.uv).collect()
    };
    // One engine, so the same glyph has the same place in the atlas in both batches.
    let mut engine = TextEngine::bundled_only();
    let mut build = |arabic_reversed| {
        let options = TextOptions {
            arabic_reversed,
            px_per_unit: Some(40.0),
        };
        build_batch_with_text(
            &doc,
            MODEL,
            &Visibility::default(),
            1e-3,
            &mut engine,
            options,
        )
    };
    let plain = build(false);
    assert_ne!(uvs(&plain, stored), uvs(&plain, right));
    assert_eq!(uvs(&plain, mtext), uvs(&plain, right));
    let fixed = build(true);
    // With the toggle the drawing holds every Arabic text backwards: the stored-backwards one
    // now draws the word, the others draw it backwards.
    assert_eq!(uvs(&fixed, stored), uvs(&plain, right));
    assert_eq!(uvs(&fixed, right), uvs(&plain, stored));
}

#[test]
fn a_wide_text_draws_from_a_size_that_fits_its_width() {
    let mut doc = CadDocument::new();
    let narrow = add(
        &mut doc,
        EntityType::Text(Text::with_value("H", Vector3::new(0.0, 0.0, 0.0))),
    );
    let mut text = Text::with_value("H", Vector3::new(0.0, 5.0, 0.0));
    text.width_factor = 4.0;
    let wide = add(&mut doc, EntityType::Text(text));
    // 1 unit tall at 10 px per unit: 12 px glyphs; four times as wide wants 48 px.
    let (batch, _) = text_batch(&doc, at_zoom(10.0));
    let rows = |entity| {
        let q = quads_of(&batch, entity)[0];
        q.uv[3] - q.uv[1]
    };
    assert!(
        rows(wide) > rows(narrow) * 3,
        "{} {}",
        rows(wide),
        rows(narrow)
    );
}

#[test]
fn survey_glyphs_keep_local_origins_when_zoom_changes() {
    let mut doc = CadDocument::new();
    add(
        &mut doc,
        EntityType::Line(opencadcodec::Line::from_coords(
            0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
        )),
    );
    add(
        &mut doc,
        EntityType::Text(
            Text::with_value("MEGA SITE", Vector3::new(500_000.0, 3_300_000_000.0, 0.0))
                .with_height(2.0),
        ),
    );
    let (mut batch, mut engine) = text_batch(&doc, at_zoom(8.0));
    let segments = batch.segments.clone();
    let tiles = batch.segment_tiles.clone();
    for zoom in [8.0, 64.0] {
        place_text(&mut batch, &mut engine, zoom);
        assert!(!batch.quads.is_empty());
        assert!(!batch.quad_tiles.is_empty());
        for (i, q) in batch.quads.iter().enumerate() {
            assert!(q.corners.iter().all(|v| v.abs() < 1e7));
            let o = batch.quad_origin(i);
            assert!((o[0] + f64::from(q.corners[0]) - 500_000.0).abs() < 40.0);
            assert!((o[1] + f64::from(q.corners[1]) - 3_300_000_000.0).abs() < 4.0);
        }
        assert_eq!(batch.segments, segments);
        assert_eq!(batch.segment_tiles, tiles);
    }
}
