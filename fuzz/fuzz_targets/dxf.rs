#![no_main]

use std::io::Write;

use libfuzzer_sys::fuzz_target;
use qiyas_core::Drawing;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    let mut file = tempfile::Builder::new()
        .suffix(".dxf")
        .tempfile()
        .expect("temporary DXF");
    file.write_all(data).expect("write DXF input");
    // Drawing::open selects the codec's failsafe DXF reader and restores paper settings.
    let _ = Drawing::open(file.path());
});
