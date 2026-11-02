#![no_main]

use libfuzzer_sys::fuzz_target;
use qiyas_plot::PlotStyle;

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    if let Ok(style) = PlotStyle::parse(data) {
        let _ = style.apply([255, 0, 0], None);
        for entry in style.styles.iter().take(256) {
            let _ = style.apply([0, 0, 0], Some(&entry.name));
        }
        let _ = PlotStyle::parse(&style.to_bytes());
    }
});
