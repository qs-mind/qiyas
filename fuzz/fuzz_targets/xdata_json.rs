#![no_main]

use libfuzzer_sys::fuzz_target;
use opencadcodec::{objects::ObjectType, CadDocument};
use qiyas_core::{knowledge, xdata};
use serde_json::Value;

fuzz_target!(|text: &str| {
    if text.len() > 16 * 1024 {
        return;
    }
    let decoded = xdata::decode_legacy_unicode(text);
    let _ = serde_json::from_str::<Value>(&decoded);
    assert_eq!(xdata::string_chunks(text).concat(), text);

    let mut doc = CadDocument::new();
    knowledge::put(&mut doc, knowledge::Kind::Queries, "fuzz", &Value::Null)
        .expect("fixed knowledge fixture");
    for object in doc.objects.values_mut() {
        if let ObjectType::XRecord(record) = object {
            record.entries.clear();
            for chunk in xdata::string_chunks(text) {
                record.add_string(1, chunk);
            }
        }
    }
    let _ = knowledge::get(&doc, knowledge::Kind::Queries, "fuzz");
});
