use qiyas_ai::{surface, ToolOutput};
use serde_json::json;

#[test]
fn scanned_document_returns_marked_ocr_text() {
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
    let source = qiyas_pdf::fixtures::write_wall(dir.path());
    let path = qiyas_pdf::fixtures::write_ocr_scan(dir.path(), &source, false);
    let output =
        surface::run_without_drawing(surface::READ, "document", &json!({"path":path}), None)
            .unwrap()
            .unwrap();
    let ToolOutput::Json(value) = output else {
        panic!("expected document JSON")
    };
    let page = &value["pages"][0];
    assert_eq!(page["page"], 1);
    assert_eq!(page["ocr"], true);
    assert_eq!(page["needs_vision"], false);
    assert!(page["text"].as_str().unwrap().contains("WALL"));
    let confidence = page["confidence"].as_f64().unwrap();
    assert!(confidence > 0.0 && confidence <= 1.0);
    assert!((confidence * 100.0 - (confidence * 100.0).round()).abs() < 1e-9);
}
