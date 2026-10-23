// A fixed CPU reference independent of the measured engine. Primitive integer sorting is
// unaffected by rstar, geometry or solver code generation. No elapsed-time floor is used.
#[allow(dead_code)]
pub fn reference() -> std::time::Duration {
    use std::hint::black_box;
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let values: Vec<u64> = (0..2_000_000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        })
        .collect();
    let mut work = values.clone();
    let start = std::time::Instant::now();
    for _ in 0..6 {
        work.copy_from_slice(&values);
        black_box(&mut work).sort_unstable();
        black_box(&work);
    }
    start.elapsed()
}

#[allow(dead_code)]
pub fn assert_relative(
    elapsed: std::time::Duration,
    reference: std::time::Duration,
    ratio: f64,
    label: &str,
) {
    let observed = elapsed.as_secs_f64() / reference.as_secs_f64();
    eprintln!("{label}: {elapsed:?}, reference {reference:?}, ratio {observed:.4}, limit {ratio}");
    assert!(
        observed < ratio,
        "{label}: workload/reference ratio {observed:.4} exceeds {ratio}"
    );
}
