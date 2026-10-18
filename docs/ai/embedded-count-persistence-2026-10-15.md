# Embedded count persistence — QSM-461

The drawing now stores the count note and source in its existing JSON xdata metadata.
A count without locations uses one invisible Point with `record_only: true`.
The reader restores its row but adds no count location. Repeated saves replace the carrier.
The count scope and numeric value stay unchanged. This change does not add recursive counting.
Legacy 7-, 8-, 9- and 10-field records retain their existing defaults.

Validation: `cargo test -p qiyas-core --test save`: 22 passed, 0 failed.
The new file tests save and reopen DWG and DXF twice, with the project sidecar removed each time.
They check zero and nonzero rows, Arabic note text, source, scope, carrier visibility and point count.
The legacy test checks all four old record lengths.
Desktop testing and Excel export remain for the same tester after code review.
Dependencies were not changed. Other working tree changes belong to existing work.
The existing area-origin, measurement-basis and count-scope changes in embed.rs predate this fix.

Review snapshot (SHA-256):
- `crates/qiyas-core/src/embed.rs`: `a2c43d300079fb08a7526afc1e35c6fe7dfefdf4a3dfd4b5702ce96671327a0d`
- `crates/qiyas-core/tests/save.rs`: `cc6dbdd946a151c8a9a4813eeaeaa32916c0c13c09f8528e5bc608fbc285f02f`
