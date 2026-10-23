//! PDF reading through pdfium. These need the pdfium library (`scripts/pdfium.sh`, or
//! `QIYAS_PDFIUM`); without it they are ignored (see build.rs).

use opencadcodec::EntityType;
use qiyas_pdf::fixtures::{
    write_many, write_rotated, write_wall, PAGE_PT, RECT_PT, WALL_AT_PT, WALL_SIZE_PT,
};
use qiyas_pdf::{
    extract_vectors, page_count, page_size, render_page, thumbnails, PdfError, TEXT_LAYER,
};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn the_wall_page_imports_as_a_closed_polyline_and_a_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wall(dir.path());
    assert_eq!(page_count(&path).unwrap(), 1);
    let (w, h) = page_size(&path, 1).unwrap();
    assert!(
        close(w, PAGE_PT.0, 0.01) && close(h, PAGE_PT.1, 0.01),
        "{w} x {h}"
    );

    let entities = extract_vectors(&path, 1, 0.01).unwrap();
    let outlines: Vec<_> = entities
        .iter()
        .filter_map(|e| match e {
            EntityType::LwPolyline(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(outlines.len(), 1, "{entities:#?}");
    let rect = outlines[0];
    assert!(rect.is_closed);
    assert_eq!(rect.vertices.len(), 4);
    assert_eq!(rect.common.layer, "PDF-FF0000");
    let xs: Vec<f64> = rect.vertices.iter().map(|v| v.location.x).collect();
    let ys: Vec<f64> = rect.vertices.iter().map(|v| v.location.y).collect();
    let span = |v: &[f64]| {
        v.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - v.iter().copied().fold(f64::INFINITY, f64::min)
    };
    // 100 x 50 points at 0.01 units per point.
    assert!(close(span(&xs), 1.0, 1e-4) && close(span(&ys), 0.5, 1e-4));
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(close(min_x, RECT_PT[0][0] * 0.01, 1e-4) && close(min_y, RECT_PT[0][1] * 0.01, 1e-4));
    // 1 pt is 0.35 mm on paper.
    assert_eq!(rect.common.line_weight.value(), 35);

    let texts: Vec<_> = entities
        .iter()
        .filter_map(|e| match e {
            EntityType::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 1, "{entities:#?}");
    let wall = texts[0];
    assert_eq!(wall.value, "WALL");
    assert_eq!(wall.common.layer, TEXT_LAYER);
    assert!(close(wall.insertion_point.x, WALL_AT_PT[0] * 0.01, 1e-3));
    assert!(close(wall.insertion_point.y, WALL_AT_PT[1] * 0.01, 1e-3));
    assert!(
        close(wall.height, WALL_SIZE_PT * 0.7 * 0.01, 1e-4),
        "{}",
        wall.height
    );
    assert!(close(wall.rotation, 0.0, 1e-9));
}

#[test]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn a_page_renders_at_its_size_in_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wall(dir.path());
    let mut heard = Vec::new();
    // 144 dpi: two pixels per point.
    let image = render_page(&path, 1, 144.0, |p| heard.push(p)).unwrap();
    assert_eq!(image.dimensions(), (400, 200));
    assert_eq!(heard.first(), Some(&0));
    assert_eq!(heard.last(), Some(&100));
    assert!(heard.windows(2).all(|w| w[0] <= w[1]));
    // White paper, a red outline (image rows run from the top).
    assert_eq!(image.get_pixel(380, 20).0, [255, 255, 255, 255]);
    let [x, y] = [
        RECT_PT[0][0] * 2.0 + 60.0,
        (PAGE_PT.1 - RECT_PT[0][1]) * 2.0,
    ];
    let red = image.get_pixel(x as u32, y as u32 - 1).0;
    assert!(red[0] > 200 && red[1] < 120 && red[2] < 120, "{red:?}");

    let thumbs = thumbnails(&path, &[1], 64).unwrap();
    assert_eq!(thumbs[0].dimensions(), (64, 32));
    assert_eq!(
        thumbnails(&path, &[2], 64).unwrap_err(),
        PdfError::NoSuchPage { page: 2, count: 1 }
    );
}

#[test]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn a_missing_or_broken_file_is_an_error_not_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.pdf");
    assert!(matches!(page_count(&missing), Err(PdfError::Unreadable(_))));
    let junk = dir.path().join("junk.pdf");
    std::fs::write(&junk, b"not a pdf").unwrap();
    assert!(matches!(page_count(&junk), Err(PdfError::Unreadable(_))));
    let path = write_wall(dir.path());
    assert_eq!(
        page_size(&path, 0).unwrap_err(),
        PdfError::NoSuchPage { page: 0, count: 1 }
    );
}

/// Resident memory of this process in bytes (Linux).
#[cfg(target_os = "linux")]
fn resident_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

#[test]
#[cfg(target_os = "linux")]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn page_1999_of_2000_is_read_without_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_many(dir.path(), 2000);
    assert_eq!(page_count(&path).unwrap(), 2000);
    let (w, h) = page_size(&path, 1999).unwrap();
    assert!(close(w, PAGE_PT.0, 0.01) && close(h, PAGE_PT.1, 0.01));
    // Page 1999's line is 1999 % 150 = 49 points long.
    let lines = extract_vectors(&path, 1999, 1.0).unwrap();
    let EntityType::Line(line) = &lines[0] else {
        panic!("{lines:?}")
    };
    assert!(close(line.end.x - line.start.x, 49.0, 1e-3));
    let image = render_page(&path, 1999, 72.0, |_| {}).unwrap();
    assert_eq!(image.dimensions(), (200, 100));
    let rss = resident_bytes();
    assert!(rss < 300 << 20, "{} MB resident", rss >> 20);
}

#[test]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn a_turned_page_imports_and_renders_as_it_is_shown() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_rotated(dir.path());
    // 200 x 100 turned a quarter: shown 100 wide, 200 high.
    let (w, h) = page_size(&path, 1).unwrap();
    assert!(close(w, 100.0, 0.01) && close(h, 200.0, 0.01), "{w} x {h}");
    // (x, y) on the page shows at (y, 200 - x): the line runs down from (10, 190) to (10, 140).
    let entities = extract_vectors(&path, 1, 1.0).unwrap();
    let EntityType::Line(line) = &entities[0] else {
        panic!("{entities:?}")
    };
    assert!(
        close(line.start.x, 10.0, 1e-3) && close(line.start.y, 190.0, 1e-3),
        "{line:?}"
    );
    assert!(
        close(line.end.x, 10.0, 1e-3) && close(line.end.y, 140.0, 1e-3),
        "{line:?}"
    );
    assert_eq!(line.common.layer, "PDF-FF0000");
    // Rendered the same way: rows from the top, so the line is in rows 10 to 60 at x 9 to 10.
    let image = render_page(&path, 1, 72.0, |_| {}).unwrap();
    assert_eq!(image.dimensions(), (100, 200));
    let red = |x: u32, y: u32| {
        let p = image.get_pixel(x, y).0;
        p[0] > 200 && p[1] < 150 && p[2] < 150
    };
    assert!(red(9, 35) || red(10, 35));
    assert!(!red(35, 10) && !red(35, 190), "not drawn unturned");
}

#[test]
#[cfg_attr(not(pdfium), ignore = "needs pdfium: run scripts/pdfium.sh")]
fn plain_text_retains_page_numbers_and_rejects_invalid_pages() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_wall(dir.path());
    let pages = qiyas_pdf::page_text(&path, &[1]).unwrap();
    assert_eq!(pages[0].0, 1);
    assert!(pages[0].1.contains("WALL"));
    assert!(matches!(
        qiyas_pdf::page_text(&path, &[0]),
        Err(PdfError::NoSuchPage { .. })
    ));
    assert!(matches!(
        qiyas_pdf::page_text(&path, &[2]),
        Err(PdfError::NoSuchPage { .. })
    ));
    assert!(qiyas_pdf::page_text(&path, &[]).is_err());
    let jobs: Vec<_> = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || qiyas_pdf::page_text(&path, &[1]).unwrap())
        })
        .collect();
    for job in jobs {
        assert_eq!(job.join().unwrap(), pages);
    }
}
