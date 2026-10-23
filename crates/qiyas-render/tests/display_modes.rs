//! Line patterns and point symbols are visible in the shared screen/plot batch.
use opencadcodec::entities::{Insert, Point};
use opencadcodec::tables::{LineType, LineTypeElement};
use opencadcodec::{CadDocument, EntityType, Line, Vector3};
use qiyas_render::{build_batch, Visibility};
fn doc(mode: i16, size: f64) -> CadDocument {
    let mut doc = CadDocument::new();
    doc.header.point_display_mode = mode;
    doc.header.point_display_size = size;
    doc.add_entity(EntityType::Line(Line::from_coords(
        0., 0., 0., 100., 0., 0.,
    )))
    .unwrap();
    doc.add_entity(EntityType::Point(Point::at(Vector3::new(50., 10., 0.))))
        .unwrap();
    doc
}
#[test]
fn point_modes_and_negative_percent_size_are_respected() {
    for (mode, count) in [
        (0, 1),
        (1, 0),
        (2, 2),
        (3, 2),
        (4, 1),
        (32, 33),
        (64, 5),
        (98, 38),
    ] {
        let d = doc(mode, -2.0);
        let b = build_batch(&d, "*Model_Space", &Visibility::default(), 0.001);
        assert_eq!(b.segments.len() / 4, 1 + count, "PDMODE {mode}");
        if mode == 2 {
            let s = &b.segments[4..8];
            assert!(((s[2] - s[0]) as f64 - 2.0).abs() < 1e-6);
        }
    }
}
#[test]
fn dash_phase_and_layer_entity_global_scales_are_preserved() {
    for k in [1000.0, 100.0, 1.0, 1.0 / 0.0254] {
        let mut d = CadDocument::new();
        let mut lt = LineType::new("CUSTOM");
        lt.elements = vec![
            LineTypeElement::dash(0.5 * k),
            LineTypeElement::space(0.5 * k),
        ];
        lt.pattern_length = k;
        d.line_types.add(lt).unwrap();
        d.layers.get_mut("0").unwrap().line_type = "CUSTOM".into();
        let mut line = Line::from_coords(0., 0., 0., 4. * k, 0., 0.);
        line.common.linetype_scale = 2.0;
        let h = d.add_entity(EntityType::Line(line)).unwrap();
        let b = build_batch(&d, "*Model_Space", &Visibility::default(), 0.001 * k);
        assert_eq!(b.segments.len() / 4, 2);
        assert!((f64::from(b.segments[2] - b.segments[0]) - k).abs() < k * 1e-6);
        let raw = qiyas_geom::flatten(
            &d,
            d.get_entity(h).unwrap(),
            &qiyas_geom::Xform::identity(),
            0.001 * k,
        );
        assert!((raw.into_paths()[0].length() - 4. * k).abs() < k * 1e-6);
        d.header.linetype_scale = 2.0;
        assert_eq!(
            build_batch(&d, "*Model_Space", &Visibility::default(), 0.001 * k)
                .segments
                .len()
                / 4,
            1
        );
    }
}
#[test]
fn byblock_patterns_follow_the_insert() {
    let mut d = CadDocument::new();
    let mut lt = LineType::new("DASH");
    lt.elements = vec![LineTypeElement::dash(1.), LineTypeElement::space(1.)];
    lt.pattern_length = 2.;
    d.line_types.add(lt).unwrap();
    let mut block = opencadcodec::BlockRecord::new("Symbol");
    block.handle = d.allocate_handle();
    let owner = block.handle;
    d.block_records.add(block).unwrap();
    let mut line = Line::from_coords(0., 0., 0., 10., 0., 0.);
    line.common.owner_handle = owner;
    line.common.linetype = "ByBlock".into();
    d.add_entity(EntityType::Line(line)).unwrap();
    let mut insert = Insert::new("Symbol", Vector3::new(0., 0., 0.));
    insert.common.linetype = "DASH".into();
    d.add_entity(EntityType::Insert(insert)).unwrap();
    assert_eq!(
        build_batch(&d, "*Model_Space", &Visibility::default(), 0.001)
            .segments
            .len()
            / 4,
        5
    );
}
