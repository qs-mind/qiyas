#[test]
fn reads_arabic_and_english_from_image_pixels() {
    let there = qiyas_ocr::available();
    if !there {
        eprintln!("skipped: needs text reading (scripts/ocr.sh)");
        return;
    }
    let there = qiyas_pdf::available();
    if !there {
        eprintln!("skipped: needs pdfium (scripts/pdfium.sh)");
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "qiyas-ocr-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = qiyas_pdf::fixtures::write_bilingual_ocr(&dir);
    let rgba = qiyas_pdf::render_page(&path, 1, 300.0, |_| {}).unwrap();
    let image = image::DynamicImage::ImageRgba8(rgba).into_rgb8();
    let texts = qiyas_ocr::read(&image).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    for word in ["WALL", "سلام"] {
        let found = texts
            .iter()
            .find(|t| t.text.contains(word))
            .unwrap_or_else(|| panic!("missing {word}: {texts:?}"));
        assert!(found.confidence.is_finite() && found.confidence > 0.0 && found.confidence <= 1.0);
        assert!(found.quad.iter().all(|p| p[0] >= 0.0
            && p[1] >= 0.0
            && p[0] <= f64::from(image.width())
            && p[1] <= f64::from(image.height())));
    }
}
