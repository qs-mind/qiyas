use qiyas_pdf::{fixtures, has_text_layer, ocr_page, page_rotation, page_text};

#[test]
fn scan_text_matches_pdf_text_and_displayed_points_including_rotation() {
    let there = qiyas_pdf::available();
    if !there {
        eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
        return;
    }
    let there = qiyas_ocr::available();
    if !there {
        eprintln!("skipped: needs text reading (scripts/ocr.sh)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = fixtures::write_wall(dir.path());
    assert!(has_text_layer(&source, 1).unwrap());
    assert!(page_text(&source, &[1]).unwrap()[0].1.contains("WALL"));
    for rotated in [false, true] {
        let scan = fixtures::write_ocr_scan(dir.path(), &source, rotated);
        assert_eq!(
            page_rotation(&scan, 1).unwrap(),
            if rotated { 90 } else { 0 }
        );
        assert!(!has_text_layer(&scan, 1).unwrap());
        let texts = ocr_page(&scan, 1).unwrap();
        let wall = texts
            .iter()
            .find(|t| t.text.contains("WALL"))
            .expect("OCR WALL");
        let [x, y] = fixtures::WALL_AT_PT;
        assert!((wall.bbox[0][0] - x).abs() < 8.0, "{wall:?}");
        assert!((wall.bbox[0][1] - y).abs() < 8.0, "{wall:?}");
        assert!(
            wall.bbox[1][0] > x + 20.0 && wall.bbox[1][1] > y,
            "{wall:?}"
        );
    }
}
