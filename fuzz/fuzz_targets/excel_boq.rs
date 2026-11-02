#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    // Preserve the original workbook bytes for corpus files and crash replay.
    let suffix = if data.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        ".xls"
    } else {
        ".xlsx"
    };
    let mut file = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("temporary workbook");
    file.write_all(data).expect("write workbook input");
    let _ = qiyas_boq::import_boq(file.path());
});
