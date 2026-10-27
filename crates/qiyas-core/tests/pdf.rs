//! PDF import and PDF underlays. They need the pdfium library (`scripts/pdfium.sh`); without it
//! each test says so and passes without checking anything.

use std::path::Path;

use opencadcodec::entities::Underlay;
use opencadcodec::objects::ObjectType;
use opencadcodec::EntityType;
use qiyas_core::fixtures::write_sample;
use qiyas_core::{AttachMode, CoreError, PictureSource, PreparedPdf, Session};
use qiyas_pdf::fixtures::{write_many, write_wall, PAGE_PT, RECT_PT};
use qiyas_render::Batch;

const MODEL: &str = "*Model_Space";

fn pdfium() -> bool {
    let there = qiyas_pdf::available();
    if !there {
        eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
    }
    there
}

fn underlays(session: &Session) -> Vec<&Underlay> {
    session
        .drawing
        .doc
        .entities_in_block(MODEL)
        .filter_map(|e| match e {
            EntityType::Underlay(u) => Some(u),
            _ => None,
        })
        .collect()
}

/// The absolute corners of image quad `i` of `batch`: top-left, top-right, bottom-right,
/// bottom-left.
fn image_corners(batch: &Batch, i: usize) -> [[f64; 2]; 4] {
    let c = batch.images[i].corners;
    let o = batch.origin;
    [0, 1, 2, 3].map(|k| [f64::from(c[2 * k]) + o[0], f64::from(c[2 * k + 1]) + o[1]])
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

#[test]
fn an_import_adds_the_page_as_entities_in_one_undo_step() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    let before = session.drawing.doc.entities_in_block(MODEL).count();
    let version = session.batch_version();

    let added = session
        .attach_pdf(&pdf, 1, [100.0, 200.0], 0.01, AttachMode::Import)
        .unwrap();
    assert_eq!(added, 2);
    let doc = &session.drawing.doc;
    assert_eq!(doc.entities_in_block(MODEL).count(), before + 2);
    let rect = doc
        .entities_in_block(MODEL)
        .find_map(|e| match e {
            EntityType::LwPolyline(p) if p.common.layer == "PDF-FF0000" => Some(p),
            _ => None,
        })
        .expect("the rectangle");
    let first = rect.vertices[0].location;
    assert!(near(
        [first.x, first.y],
        [100.0 + RECT_PT[0][0] * 0.01, 200.0 + RECT_PT[0][1] * 0.01],
        1e-4
    ));
    assert!(doc.layers.contains("PDF-FF0000"));
    assert!(doc.entities_in_block(MODEL).any(
        |e| matches!(e, EntityType::Text(t) if t.value == "WALL" && t.common.layer == "PDF-TEXT")
    ));
    // The window draws them: a new batch version, and the rectangle's four sides in it.
    assert_ne!(session.batch_version(), version);
    let drawn = |s: &Session| {
        let b = s.batch();
        let o = b.origin;
        b.segments
            .chunks(4)
            .filter(|s| f64::from(s[0]) + o[0] > 99.0 && f64::from(s[1]) + o[1] > 199.0)
            .count()
    };
    assert!(drawn(&session) >= 4);

    assert!(session.undo());
    assert_eq!(session.drawing.doc.entities_in_block(MODEL).count(), before);
    assert_eq!(drawn(&session), 0);
    assert!(session.redo());
    assert_eq!(
        session.drawing.doc.entities_in_block(MODEL).count(),
        before + 2
    );
    assert!(drawn(&session) >= 4);
}

#[test]
fn an_underlay_is_drawn_and_survives_a_dxf_save() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    let handle = session
        .attach_pdf(&pdf, 1, [10.0, 20.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let u = underlays(&session);
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].common.handle.value(), handle);
    // Metres: one point is 0.0254 / 72 m, so scale 0.01 m per point is 0.01 * 72 / 0.0254.
    assert!((u[0].x_scale - 0.01 * 72.0 / 0.0254).abs() < 1e-9);

    // Drawn as a picture of the page: 200 x 100 points at 0.01 is 2 x 1 units.
    let batch = session.batch();
    assert_eq!(batch.images.len(), 1);
    assert_eq!(batch.image_table.len(), 1);
    let c = image_corners(&batch, 0);
    let (w, h) = (PAGE_PT.0 * 0.01, PAGE_PT.1 * 0.01);
    assert!(near(c[0], [10.0, 20.0 + h], 1e-4), "{c:?}");
    assert!(near(c[2], [10.0 + w, 20.0], 1e-4), "{c:?}");
    assert!(batch.bounds[1][0] >= 10.0 + w - 1e-4);
    assert_eq!(session.underlay_at([11.0, 20.5]), Some(handle));
    assert_eq!(session.underlay_at([9.0, 20.5]), None);
    // The picture for a zoom of 300 px per unit: 512 px is the power of two above, so the
    // 2-unit page is 1024 px wide, 1024 px over 200/72 inches.
    let image = session
        .underlay_image(batch.image_table[0].id, 300.0)
        .unwrap();
    let PictureSource::Pdf { page, dpi } = image.source else {
        panic!("a PDF page: {image:?}");
    };
    assert_eq!(page, 1);
    assert!((dpi - 1024.0 / (200.0 / 72.0)).abs() < 1e-2, "{dpi}");
    assert_eq!(image.version, batch.image_table[0].version);
    // Zoomed far in, the picture stops at 2048 px.
    let big = session
        .underlay_image(batch.image_table[0].id, 1e6)
        .unwrap();
    let PictureSource::Pdf { dpi, .. } = big.source else {
        panic!("a PDF page: {big:?}");
    };
    assert!((dpi - 2048.0 / (200.0 / 72.0)).abs() < 1e-2, "{dpi}");

    // Hiding its layer hides it.
    session.set_layer_hidden(qiyas_core::UNDERLAY_LAYER, true);
    assert!(session.batch().images.is_empty());
    session.set_layer_hidden(qiyas_core::UNDERLAY_LAYER, false);

    let out = dir.path().join("with-underlay.dxf");
    session.save_as(&out).unwrap();
    let reopened = Session::open(&out).unwrap();
    let u = underlays(&reopened);
    assert_eq!(u.len(), 1);
    let Some(ObjectType::UnderlayDefinition(def)) =
        reopened.drawing.doc.objects.get(&u[0].definition_handle)
    else {
        panic!("the definition is gone");
    };
    // Stored beside the drawing, relative to it.
    assert_eq!(def.file_path, "wall.pdf");
    assert_eq!(def.page_name, "1");
    assert_eq!(reopened.batch().images.len(), 1);
    assert!(near(
        image_corners(&reopened.batch(), 0)[3],
        [10.0, 20.0],
        1e-4
    ));
}

#[test]
fn an_underlay_survives_a_dwg_save() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dwg")).unwrap();
    let pdf = write_wall(dir.path());
    session
        .attach_pdf(&pdf, 1, [10.0, 20.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let out = dir.path().join("with-underlay.dwg");
    session.save_as(&out).unwrap();
    let reopened = Session::open(&out).unwrap();
    let u = underlays(&reopened);
    assert_eq!(u.len(), 1);
    assert!((u[0].x_scale - 0.01 * 72.0 / 0.0254).abs() < 1e-9);
    assert!((u[0].y_scale - 0.01 * 72.0 / 0.0254).abs() < 1e-9);
    assert!(near(
        [u[0].insertion_point.x, u[0].insertion_point.y],
        [10.0, 20.0],
        1e-9
    ));
    let Some(ObjectType::UnderlayDefinition(def)) =
        reopened.drawing.doc.objects.get(&u[0].definition_handle)
    else {
        panic!("the definition is gone");
    };
    assert_eq!(def.file_path, "wall.pdf");
    assert_eq!(def.page_name, "1");
    assert_eq!(reopened.batch().images.len(), 1);
}

#[test]
fn save_as_in_another_folder_keeps_the_underlay_linked() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let jobs = dir.path().join("jobs");
    std::fs::create_dir_all(&jobs).unwrap();
    let mut session = Session::open(&write_sample(&jobs, "dxf")).unwrap();
    let pdf = write_wall(&jobs);
    session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let definition = |s: &Session| {
        let u = underlays(s)[0].definition_handle;
        match s.drawing.doc.objects.get(&u) {
            Some(ObjectType::UnderlayDefinition(def)) => def.file_path.clone(),
            _ => panic!("no definition"),
        }
    };
    assert_eq!(definition(&session), "wall.pdf");

    // Out of the PDF's folder: the link becomes absolute and still finds it.
    let tender = dir.path().join("tender");
    std::fs::create_dir_all(&tender).unwrap();
    let out = jobs.join("../tender/a.dxf");
    session.save_as(&out).unwrap();
    let reopened = Session::open(&out).unwrap();
    let stored = definition(&reopened);
    assert!(Path::new(&stored).is_absolute(), "{stored}");
    assert_eq!(reopened.batch().images.len(), 1);
    assert!(
        reopened.info().problems.is_empty(),
        "{:?}",
        reopened.info().problems
    );

    // Back beside it: relative again.
    let mut reopened = reopened;
    reopened.save_as(&jobs.join("b.dxf")).unwrap();
    assert_eq!(
        definition(&Session::open(&jobs.join("b.dxf")).unwrap()),
        "wall.pdf"
    );
}

#[test]
fn drawing_changes_mark_the_drawing_unsaved_until_it_is_saved() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    assert!(!session.is_dirty());
    // A take-off change is saved in the project file, not the drawing.
    session
        .run(qiyas_core::Command::MeasureDistance {
            points: vec![[0.0, 0.0], [1.0, 0.0]].into(),
        })
        .unwrap();
    assert!(!session.is_dirty());
    session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.01, AttachMode::Import)
        .unwrap();
    assert!(session.is_dirty());
    session.save().unwrap();
    assert!(!session.is_dirty());
    assert!(session.undo());
    assert!(session.is_dirty());
}

#[test]
fn a_page_read_first_is_added_in_one_undo_step() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    let read = PreparedPdf::read(&pdf, 1, 0.01, AttachMode::Underlay).unwrap();
    let handles = session.apply_pdf(read, [5.0, 5.0]).unwrap();
    assert_eq!(handles.len(), 1);
    assert_eq!(underlays(&session).len(), 1);
    assert!(session.undo());
    assert!(underlays(&session).is_empty());
    assert!(PreparedPdf::read(&pdf, 9, 0.01, AttachMode::Import).is_err());
    assert!(PreparedPdf::read(&pdf, 1, -1.0, AttachMode::Import).is_err());
}

#[test]
fn calibrating_scales_the_underlay_about_the_first_point() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    let handle = session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let scale = underlays(&session)[0].x_scale;
    let version = session.batch_version();
    let image_version = session.batch().image_table[0].version;
    // The rectangle's bottom edge on the page is 1 unit long; it is really 4 m.
    let (p1, p2) = ([0.2, 0.2], [1.2, 0.2]);
    session.calibrate(handle, p1, p2, 4.0).unwrap();
    let u = underlays(&session)[0];
    assert!((u.x_scale / scale - 4.0).abs() < 1e-9);
    assert!((u.y_scale / scale - 4.0).abs() < 1e-9);
    // The first point stays put: the corner 0.2 to its left and below is now 0.8 away.
    assert!(near(
        [u.insertion_point.x, u.insertion_point.y],
        [-0.6, -0.6],
        1e-9
    ));
    let batch = session.batch();
    assert_ne!(session.batch_version(), version);
    // A rescaled page needs another resolution.
    assert_ne!(batch.image_table[0].version, image_version);
    assert!(near(image_corners(&batch, 0)[2], [-0.6 + 8.0, -0.6], 1e-4));

    assert!(session.undo());
    assert!((underlays(&session)[0].x_scale - scale).abs() < 1e-12);

    let err = session.calibrate(handle, p1, p1, 4.0).unwrap_err();
    assert!(matches!(err, CoreError::InvalidCommand(_)));
    let line = session
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Line(_)))
        .unwrap()
        .common()
        .handle
        .value();
    assert!(session.calibrate(line, p1, p2, 4.0).is_err());
}

#[test]
fn a_missing_pdf_or_page_is_an_error() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    let missing = session.attach_pdf(
        Path::new("/nowhere/plan.pdf"),
        1,
        [0.0, 0.0],
        1.0,
        AttachMode::Underlay,
    );
    assert!(matches!(missing, Err(CoreError::Pdf(_))));
    let page = session
        .attach_pdf(&pdf, 3, [0.0, 0.0], 1.0, AttachMode::Import)
        .unwrap_err();
    assert_eq!(
        page.to_string(),
        "Page 3 is not in this PDF (it has 1 pages)"
    );
    assert!(session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.0, AttachMode::Underlay)
        .is_err());
    assert!(!session.can_undo());
}

/// Resident memory of this process in bytes.
#[cfg(target_os = "linux")]
fn resident_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

#[test]
#[cfg(target_os = "linux")]
fn page_1999_of_a_2000_page_pdf_attaches_as_an_underlay() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_many(dir.path(), 2000);
    session
        .attach_pdf(&pdf, 1999, [0.0, 0.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let batch = session.batch();
    assert_eq!(batch.images.len(), 1);
    let image = session
        .underlay_image(batch.image_table[0].id, 500.0)
        .unwrap();
    assert!(matches!(
        image.source,
        PictureSource::Pdf { page: 1999, .. }
    ));
    let pixels = qiyas_core::draw_picture(&image, |_| {}).unwrap();
    assert_eq!(pixels.width(), 1024);
    let rss = resident_bytes();
    assert!(rss < 300 << 20, "{} MB resident", rss >> 20);
}

#[test]
fn the_pack_card_calls_a_vector_page_vector_and_reads_its_orientation() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.01, AttachMode::Underlay)
        .unwrap();
    let card = session.pack_check().unwrap();
    let pages = card.line("pdf_pages").expect("a PDF page is on the drawing");
    assert!(pages.passes && pages.found_en.contains("vector"), "{pages:?}");
    // A vector page needs no calibration line; it is stored upright.
    assert!(card.line("calibration").is_none());
    assert!(card.line("orientation").unwrap().passes);
    assert_eq!(qiyas_pdf::page_rotation(&pdf, 1).unwrap(), 0);
}

#[test]
fn the_pack_card_says_when_a_pdf_page_cannot_be_read() {
    if !pdfium() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&write_sample(dir.path(), "dxf")).unwrap();
    let pdf = write_wall(dir.path());
    session
        .attach_pdf(&pdf, 1, [0.0, 0.0], 0.01, AttachMode::Underlay)
        .unwrap();
    std::fs::remove_file(&pdf).unwrap();
    let card = session.pack_check().unwrap();
    let pages = card.line("pdf_pages").unwrap();
    assert!(!pages.passes && pages.found_en.contains("Could not read"), "{pages:?}");
    assert!(!pages.fix_en.is_empty());
}
