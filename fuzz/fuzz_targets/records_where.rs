#![no_main]

use libfuzzer_sys::fuzz_target;
use qiyas_core::records::{Collection, RecordQuery, Where};
use qiyas_core::{Session, Source};
use serde_json::json;

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    let Ok(conditions) = serde_json::from_slice::<Vec<Where>>(data) else {
        return;
    };
    let mut session = Session::new_blank(std::path::Path::new("records.dxf"), 6);
    session
        .run_input(
            &json!({"type":"draw","kind":"line","a":[0,0],"b":[5,0]}),
            Source::User,
        )
        .expect("fixed line fixture");
    for collection in [Collection::Entities, Collection::Layers, Collection::Header] {
        let query = RecordQuery {
            collection,
            conditions: conditions.clone(),
            limit: Some(20),
            ..RecordQuery::default()
        };
        let _ = session.records(&query);
    }
});
