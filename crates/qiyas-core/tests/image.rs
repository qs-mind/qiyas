//! Raster images: attached, drawn, calibrated, saved and reopened, cropped, and their pictures.

use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::{ImageEncoder, Rgb, RgbImage};
use opencadcodec::entities::RasterImage;
use opencadcodec::objects::ObjectType;
use opencadcodec::{EntityType, Handle, Vector2};
use qiyas_core::fixtures::{write_png, write_sample};
use qiyas_core::{Command, ImageDisplay, Outcome, PictureSource, Session, IMAGE_LAYER};
use qiyas_render::{Batch, IMAGE_ADJUSTED, TEXT_BOX};

const MODEL: &str = "*Model_Space";

fn images(session: &Session) -> Vec<&RasterImage> {
    session
        .drawing
        .doc
        .entities_in_block(MODEL)
        .filter_map(|e| match e {
            EntityType::RasterImage(i) => Some(i),
            _ => None,
        })
        .collect()
}

fn definition_file(session: &Session, image: &RasterImage) -> String {
    match image
        .definition_handle
        .and_then(|h| session.drawing.doc.objects.get(&h))
    {
        Some(ObjectType::ImageDefinition(def)) => def.file_name.clone(),
        _ => panic!("no definition"),
    }
}

/// The absolute corners of image quad `i` of `batch`: top-left, top-right, bottom-right,
/// bottom-left.
fn image_corners(batch: &Batch, i: usize) -> [[f64; 2]; 4] {
    let c = batch.images[i].corners;
    let o = batch.origin;
    [0, 1, 2, 3].map(|k| [f64::from(c[2 * k]) + o[0], f64::from(c[2 * k + 1]) + o[1]])
}

fn assert_corners(got: [[f64; 2]; 4], want: [[f64; 2]; 4]) {
    for (g, w) in got.iter().zip(&want) {
        assert!(
            (g[0] - w[0]).abs() < 1e-4 && (g[1] - w[1]).abs() < 1e-4,
            "{got:?} != {want:?}"
        );
    }
}

/// A session on the sample drawing (metres) in `dir` with `pic.png` attached at (5, 5), 20 wide.
fn attached(dir: &Path, ext: &str) -> (Session, u64) {
    let mut session = Session::open(&write_sample(dir, ext)).unwrap();
    let png = write_png(dir, "pic.png");
    let handle = session.attach_image(&png, [5.0, 5.0], 20.0).unwrap();
    (session, handle)
}

#[test]
fn an_attached_png_is_an_autocad_image_drawn_as_one_quad() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, handle) = attached(dir.path(), "dxf");
    assert!(session.is_dirty());
    let found = images(&session);
    assert_eq!(found.len(), 1);
    let image = found[0];
    assert_eq!(image.common.handle.value(), handle);
    assert_eq!(image.common.layer, IMAGE_LAYER);
    assert_eq!((image.size.x, image.size.y), (200.0, 100.0));
    assert!((image.u_vector.x - 0.1).abs() < 1e-12 && image.u_vector.y == 0.0);
    assert!((image.v_vector.y - 0.1).abs() < 1e-12 && image.v_vector.x == 0.0);
    assert_eq!((image.brightness, image.contrast, image.fade), (50, 50, 0));
    assert_eq!(definition_file(&session, image), "pic.png");
    // The reactor links the definition back to the image, as AutoCAD keeps it.
    let reactor = image.definition_reactor_handle.unwrap();
    match session.drawing.doc.objects.get(&reactor) {
        Some(ObjectType::ImageDefinitionReactor(r)) => {
            assert_eq!(r.owner, Handle::new(handle));
        }
        other => panic!("no reactor: {other:?}"),
    }

    let batch = session.batch();
    assert_eq!(batch.images.len(), 1);
    assert_corners(
        image_corners(&batch, 0),
        [[5.0, 15.0], [25.0, 15.0], [25.0, 5.0], [5.0, 5.0]],
    );
    assert_eq!(batch.images[0].flags, qiyas_render::image_flags(50, 50, 0));
    assert!(batch.images[0].flags & IMAGE_ADJUSTED != 0);
    // Picked by its outline, which is never drawn.
    let picked = session.pick([15.0, 5.01], 0.05).unwrap();
    assert_eq!((picked.top, picked.entity), (handle, handle));
    let range = batch.ranges.iter().find(|r| r.entity == handle).unwrap();
    assert_eq!((range.count, range.flags), (4, TEXT_BOX));
    assert_eq!(session.underlay_at([10.0, 10.0]), Some(handle));

    // One undo step takes it away.
    assert!(session.undo());
    assert!(images(&session).is_empty());
    assert!(session.batch().images.is_empty());
    assert!(session.redo());
    assert_eq!(session.batch().images.len(), 1);
}

#[test]
fn calibrating_an_image_makes_its_distances_true() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, handle) = attached(dir.path(), "dxf");
    session
        .calibrate(handle, [5.0, 5.0], [25.0, 5.0], 10.0)
        .unwrap();
    let image = images(&session)[0];
    assert!(
        (image.u_vector.x - 0.05).abs() < 1e-12,
        "{:?}",
        image.u_vector
    );
    assert!(
        (image.v_vector.y - 0.05).abs() < 1e-12,
        "{:?}",
        image.v_vector
    );
    let batch = session.batch();
    assert_corners(
        image_corners(&batch, 0),
        [[5.0, 10.0], [15.0, 10.0], [15.0, 5.0], [5.0, 5.0]],
    );
    // The image's bottom edge, end to end, now measures 10 m.
    let outcome = session
        .run(Command::MeasureDistance {
            points: vec![[5.0, 5.0], [15.0, 5.0]].into(),
        })
        .unwrap();
    let Outcome::Measured { measurement } = outcome else {
        panic!("not measured: {outcome:?}");
    };
    assert_eq!(measurement.unit, "m");
    assert!(
        (measurement.value - 10.0).abs() <= 10.0 * 0.005,
        "{}",
        measurement.value
    );
    assert_eq!(format!("{:.3}", measurement.value), "10.000");
}

#[test]
fn an_image_survives_dxf_and_dwg_saves() {
    for ext in ["dxf", "dwg"] {
        let dir = tempfile::tempdir().unwrap();
        let (mut session, _) = attached(dir.path(), ext);
        let out = dir.path().join(format!("with-image.{ext}"));
        session.save_as(&out).unwrap();
        let reopened = Session::open(&out).unwrap();
        let found = images(&reopened);
        assert_eq!(found.len(), 1, "{ext}");
        let image = found[0];
        assert_eq!(definition_file(&reopened, image), "pic.png", "{ext}");
        // The definition sits in ACAD_IMAGE_DICT, where AutoCAD looks for it.
        let doc = &reopened.drawing.doc;
        let Some(ObjectType::Dictionary(root)) =
            doc.objects.get(&doc.header.named_objects_dict_handle)
        else {
            panic!("{ext}: no named-object dictionary");
        };
        let Some(ObjectType::Dictionary(dict)) = root
            .get("ACAD_IMAGE_DICT")
            .and_then(|h| doc.objects.get(&h))
        else {
            panic!("{ext}: no ACAD_IMAGE_DICT");
        };
        assert_eq!(dict.get("pic"), image.definition_handle, "{ext}");
        assert!((image.u_vector.x - 0.1).abs() < 1e-9, "{ext}");
        assert_eq!((image.size.x, image.size.y), (200.0, 100.0), "{ext}");
        assert!(
            reopened.info().problems.is_empty(),
            "{ext}: {:?}",
            reopened.info().problems
        );
        let batch = reopened.batch();
        assert_eq!(batch.images.len(), 1, "{ext}");
        assert_corners(
            image_corners(&batch, 0),
            [[5.0, 15.0], [25.0, 15.0], [25.0, 5.0], [5.0, 5.0]],
        );
        let picture = reopened.picture(batch.image_table[0].id, 5.0).unwrap();
        assert_eq!((picture.width(), picture.height()), (160, 80), "{ext}");
    }
}

#[test]
fn a_missing_image_file_is_a_problem_and_draws_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, _) = attached(dir.path(), "dxf");
    let out = dir.path().join("lost.dxf");
    session.save_as(&out).unwrap();
    std::fs::remove_file(dir.path().join("pic.png")).unwrap();
    let reopened = Session::open(&out).unwrap();
    assert_eq!(images(&reopened).len(), 1);
    assert!(reopened.batch().images.is_empty());
    assert!(
        reopened
            .info()
            .problems
            .contains(&"Image pic.png not found".to_string()),
        "{:?}",
        reopened.info().problems
    );

    // Found again in an `images` folder beside the drawing.
    std::fs::create_dir_all(dir.path().join("images")).unwrap();
    write_png(&dir.path().join("images"), "pic.png");
    let reopened = Session::open(&out).unwrap();
    assert_eq!(reopened.batch().images.len(), 1);
    assert!(reopened.info().problems.is_empty());
}

#[test]
fn a_rectangular_clip_crops_the_quad_and_the_picture() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, handle) = attached(dir.path(), "dxf");
    // The left half of the image, from 10 px below its top to 60 px: pixel corners at -0.5.
    match session.drawing.doc.get_entity_mut(Handle::new(handle)) {
        Some(EntityType::RasterImage(image)) => {
            image.set_clip_rect(Vector2::new(-0.5, 9.5), Vector2::new(99.5, 59.5))
        }
        _ => panic!("no image"),
    }
    // Saved and opened again, so the clip goes through the DXF too.
    let out = dir.path().join("clipped.dxf");
    session.save_as(&out).unwrap();
    let reopened = Session::open(&out).unwrap();
    let batch = reopened.batch();
    assert_eq!(batch.images.len(), 1);
    // 0.1 units a pixel: x 5..15, y from 15 - 1 = 14 down to 15 - 6 = 9.
    assert_corners(
        image_corners(&batch, 0),
        [[5.0, 14.0], [15.0, 14.0], [15.0, 9.0], [5.0, 9.0]],
    );
    let image = reopened
        .underlay_image(batch.image_table[0].id, 1000.0)
        .unwrap();
    assert_eq!(
        image.source,
        PictureSource::Raster {
            crop: Some([0.0, 0.1, 0.5, 0.6]),
            long_px: 2048
        }
    );
    // The picture is the red half only, at its own 100 x 50 pixels.
    let picture = qiyas_core::draw_picture(&image, |_| {}).unwrap();
    assert_eq!((picture.width(), picture.height()), (100, 50));
    assert!(picture.pixels().all(|p| p.0 == [255, 0, 0, 255]));
}

#[test]
fn an_image_picture_is_drawn_at_the_zoom_bucket() {
    let dir = tempfile::tempdir().unwrap();
    let (session, _) = attached(dir.path(), "dxf");
    let batch = session.batch();
    let id = batch.image_table[0].id;
    // 20 units at 5 px per unit: rounded up to 8, so 160 px wide.
    let picture = session.picture(id, 5.0).unwrap();
    assert_eq!((picture.width(), picture.height()), (160, 80));
    assert_eq!(picture.as_raw().len(), 160 * 80 * 4);
    assert_eq!(picture.get_pixel(10, 40).0, [255, 0, 0, 255]);
    assert_eq!(picture.get_pixel(150, 40).0, [0, 0, 255, 255]);
    // Zoomed far in: never more than the file's own 200 px.
    let picture = session.picture(id, 300.0).unwrap();
    assert_eq!((picture.width(), picture.height()), (200, 100));
    // Far out: at least 64 px on the long side.
    let picture = session.picture(id, 0.01).unwrap();
    assert_eq!((picture.width(), picture.height()), (64, 32));
    // The resolution tells the sizes apart for the window's cache.
    let a = session.underlay_image(id, 5.0).unwrap();
    let b = session.underlay_image(id, 9.0).unwrap();
    assert_ne!(a.resolution(), b.resolution());
    assert_eq!(a.version, b.version);
}

#[test]
fn brightness_contrast_and_fade_are_one_undo_step_each() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, handle) = attached(dir.path(), "dxf");
    let version = session.batch_version();
    let display = ImageDisplay {
        brightness: 70,
        contrast: 40,
        fade: 30,
    };
    session.set_image_display(handle, display).unwrap();
    assert_eq!(session.image_display(handle), Some(display));
    assert_ne!(session.batch_version(), version);
    let flags = session.batch().images[0].flags;
    assert_eq!(flags, IMAGE_ADJUSTED | 70 << 8 | 40 << 16 | 30 << 24);
    assert!(session.undo());
    assert_eq!(
        session.image_display(handle),
        Some(ImageDisplay {
            brightness: 50,
            contrast: 50,
            fade: 0
        })
    );
    // Out of range, or not an image: refused, nothing to undo.
    let too_bright = ImageDisplay {
        brightness: 101,
        ..display
    };
    assert!(session.set_image_display(handle, too_bright).is_err());
    assert_eq!(session.image_display(1), None);
    assert!(session.set_image_display(1, display).is_err());
}

#[test]
fn bad_attachments_are_refused_without_an_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let png = write_png(dir.path(), "pic.png");
    assert!(session.attach_image(&png, [0.0, 0.0], 0.0).is_err());
    assert!(session
        .attach_image(&dir.path().join("none.png"), [0.0, 0.0], 1.0)
        .is_err());
    let junk = dir.path().join("junk.png");
    std::fs::write(&junk, b"not an image").unwrap();
    let error = session.attach_image(&junk, [0.0, 0.0], 1.0).unwrap_err();
    assert!(
        error.to_string().starts_with("This image can't be read"),
        "{error}"
    );
    assert!(!session.can_undo());
    assert!(!session.is_dirty());
}

#[test]
fn a_photo_stands_the_way_its_exif_orientation_says() {
    let dir = tempfile::tempdir().unwrap();
    // 40 x 20 pixels, red on the left, blue on the right, marked "turn 90 degrees clockwise".
    let pixels = RgbImage::from_fn(40, 20, |x, _| {
        if x < 20 {
            Rgb([255, 0, 0])
        } else {
            Rgb([0, 0, 255])
        }
    });
    let path = dir.path().join("photo.jpg");
    let mut encoder = JpegEncoder::new_with_quality(std::fs::File::create(&path).unwrap(), 95);
    // A little-endian TIFF block with one IFD entry: Orientation (0x0112), SHORT, 1, value 6.
    let exif: Vec<u8> = [
        &b"II*\0"[..],
        &8u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &0x0112u16.to_le_bytes(),
        &3u16.to_le_bytes(),
        &1u32.to_le_bytes(),
        &6u16.to_le_bytes(),
        &[0, 0],
        &0u32.to_le_bytes(),
    ]
    .concat();
    encoder.set_exif_metadata(exif).unwrap();
    encoder
        .write_image(pixels.as_raw(), 40, 20, image::ExtendedColorType::Rgb8)
        .unwrap();

    assert_eq!(qiyas_core::image_size(&path).unwrap(), (20, 40));
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    session.attach_image(&path, [0.0, 0.0], 2.0).unwrap();
    let image = images(&session)[0];
    assert_eq!((image.size.x, image.size.y), (20.0, 40.0));
    let batch = session.batch();
    let picture = session.picture(batch.image_table[0].id, 100.0).unwrap();
    assert_eq!((picture.width(), picture.height()), (20, 40));
    // Turned clockwise, the red left half is now on top.
    let [r, _, b, _] = picture.get_pixel(10, 5).0;
    assert!(r > 200 && b < 60, "top {:?}", picture.get_pixel(10, 5));
    let [r, _, b, _] = picture.get_pixel(10, 35).0;
    assert!(b > 200 && r < 60, "bottom {:?}", picture.get_pixel(10, 35));
}

#[test]
fn the_pack_card_flags_a_scan_until_it_is_calibrated_and_again_after_an_undo() {
    let dir = tempfile::tempdir().unwrap();
    let (mut session, handle) = attached(dir.path(), "dxf");
    let card = session.pack_check().unwrap();
    let calibration = card
        .line("calibration")
        .expect("an attached image is a scan");
    assert!(!calibration.passes && !card.ready);
    assert!(calibration.found_en.contains("pic.png"), "{calibration:?}");
    assert!(calibration.fix_en.to_lowercase().contains("calibrate"));
    assert!(calibration.fix_ar.contains("اضبط"));

    session
        .calibrate(handle, [5.0, 5.0], [15.0, 5.0], 10.0)
        .unwrap();
    assert!(
        session
            .pack_check()
            .unwrap()
            .line("calibration")
            .unwrap()
            .passes
    );
    // Saving and reading it again keeps the note: it is drawing knowledge.
    let path = dir.path().join("saved.dxf");
    session.save_as(&path).unwrap();
    let reopened = Session::open(&path).unwrap();
    assert!(
        reopened
            .pack_check()
            .unwrap()
            .line("calibration")
            .unwrap()
            .passes
    );

    assert!(session.undo());
    assert!(
        !session
            .pack_check()
            .unwrap()
            .line("calibration")
            .unwrap()
            .passes
    );
}
