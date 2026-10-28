use qiyas_core::{Session, Source};
use serde_json::{json, Value};

#[test]
fn recognition_keeps_ocr_evidence_caps_confidence_and_prefers_supplied_text() {
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
    for supplied in [false, true] {
        let mut session = Session::new_blank(&dir.path().join("scan.dxf"), 6);
        session
            .run_input(
                &json!({"type":"define_object_type","definition":{
                    "name":"Space","category":"test","geometry":"region"
                }}),
                Source::User,
            )
            .unwrap();
        let texts = if supplied {
            json!([{"text":"Verified label","at":[32,82]}])
        } else {
            json!([])
        };
        let result = session.run_input(&json!({"type":"recognition","kind":"vision","spec":{
            "path":path,"page":1,
            "calibration":{"a":[20,20],"b":[120,20],"distance_m":1},
            "snap_radius_m":0.001,"texts":texts,
            "hints":[{"object_type":"Space","points":[[10,10],[180,10],[180,95],[10,95]],"confidence":0.99}]
        }}), Source::User).unwrap();
        let result = serde_json::to_value(result).unwrap();
        let candidate = &result["result"]["candidates"][0];
        let evidence: Vec<&Value> = candidate["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "ocr")
            .collect();
        let handle = candidate["handle"].as_u64().unwrap();
        let label = session.drawing.element_metadata(handle).unwrap().label;
        if supplied {
            assert_eq!(label.as_deref(), Some("Verified label"));
            assert!(evidence.is_empty());
        } else {
            assert!(label.unwrap().contains("WALL"));
            assert!(!evidence.is_empty());
            for text in evidence {
                assert!(
                    candidate["confidence"].as_f64().unwrap()
                        <= text["confidence"].as_f64().unwrap()
                );
                assert_eq!(text["page"], 1);
                assert!(text["bbox"].is_array());
            }
            assert_eq!(candidate["state"], "pending");
        }
    }
}
