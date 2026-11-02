#![no_main]

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use qiyas_core::cmdline::Machine;

fuzz_target!(|data: &[u8]| {
    if data.len() > 16 * 1024 {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut machine = Machine::default();
    if let Ok(point) = Unstructured::new(data).arbitrary::<[f64; 2]>() {
        machine.set_last(Some(point));
    }
    for line in text.split('\n').take(128) {
        let _ = machine.step(line);
        let _ = machine.take_input();
        let _ = machine.last();
    }
    machine.cancel();
    assert!(!machine.is_active());
    assert!(machine.take_input().is_none());
});
