use super::*;

#[test]
fn arabic_labels_restore_logical_order_and_preserve_latin_numbers() {
    assert_eq!(arabic_logical_text("مالس"), "سلام");
    assert_eq!(arabic_logical_text("WALL 12"), "WALL 12");
    assert_eq!(arabic_logical_text(" WALL 12مالس"), "سلام WALL 12");
    // Arabic-Indic digits keep their order: 34, not 43.
    assert_eq!(arabic_logical_text(" ٣٤ةحاسم"), "مساحة ٣٤");
    assert_eq!(arabic_logical_text(" ١٢٫٥ةحاسم"), "مساحة ١٢٫٥");
}

#[test]
fn missing_override_returns_installation_message_without_loading_ort() {
    std::env::set_var("QIYAS_OCR", "/no/such/qiyas-ocr-resource-folder");
    assert!(!available());
    let error = read(&image::RgbImage::new(1, 1)).unwrap_err();
    assert_eq!(error, OcrError::NotInstalled);
    assert_eq!(
        error.to_string(),
        "Text reading is not installed: run scripts/ocr.sh"
    );
    std::env::remove_var("QIYAS_OCR");
}
