use super::*;

#[test]
fn ocr_page_shape_and_bilingual_reading_order() {
    let text = |value: &str, x, y, confidence| qiyas_pdf::PageText {
        text: value.into(),
        confidence,
        bbox: [[x, y], [x + 20.0, y + 10.0]],
    };
    let page = ocr_page_value(
        3,
        &[
            text("World", 50.0, 10.0, 0.9),
            text("العالم", 10.0, 40.0, 0.8),
            text("مرحبا", 50.0, 40.0, 0.7),
            text("Hello", 10.0, 10.0, 0.95),
        ],
    );
    assert_eq!(
        page,
        json!({"page":3,"text":"مرحبا العالم\nHello World",
        "ocr":true,"confidence":0.84,"needs_vision":false})
    );
    assert_eq!(ocr_page_value(1, &[])["confidence"], 0.0);
    assert_eq!(ocr_page_value(1, &[])["needs_vision"], true);
}
