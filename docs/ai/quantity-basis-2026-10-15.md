# QSM-451: quantity scope and basis

Source decision: QSM-450, adoption-decision revision 1 (410aeeb7-9d0e-42cc-86ca-8f739ff275b3), read in full before changes.

Implementation is local source work. Desktop acceptance remains QSM-452. No release, merge, deployment, live-model accuracy, professional net-room approval or new real-drawing run is claimed.

CountBlock and block summaries use one bounded enumeration visitor. Each top-level INSERT record counts once, including one record for an array. Loaded xref model-space tops are included. Nested occurrences, array cells, complete parent context, cycles, unavailable xrefs, dynamic visibility and clipping are expressly unresolved. Scope and limitations travel in command results, AI tool results (including empty summaries), the take-off view, and Excel. `as_shown` applies user hiding; `in_file` ignores user hiding; both exclude file off/frozen layers and invisible records. The visitor also checks the immediate loaded xref parent's invisible flag. Quantix Python block_counts is untouched.

Measurement has defaulted `area_origin`, `measurement_basis`, and `count_scope` fields. Old JSON and old embedded xdata remain readable as unspecified. New embedded xdata preserves the metadata in a tenth JSON string, leaving legacy slots intact. Existing entities supply existing_boundary; picked shapes supply manual_or_entity. Neither grants net approval. Fields distinguish gross boundary and wall-face data when such professional basis is supplied; the AI gets no approval setter.

`room_areas` retains `net_area_m2` for compatibility, but adds candidate_area_m2, unapproved_net, an unapproved type, and net_accuracy_eligible=false. Its origin is manual_or_entity and its region method is enclosed_drawn_linework: the finder cannot prove professional wall faces. `label_spaces` writes “Candidate … m²”. English and Arabic prompts preserve the limits.

The new qiyas-takeoff accuracy API gives separate absolute, 1% and exact integer verdicts. Zero reference and rounded inputs have an indeterminate relative verdict. Non-finite inputs are invalid. The net-room report excludes every non-area or non-approved-net row from both numerator and eligible denominator. This API is a report boundary, not a new desktop accuracy dashboard. Existing BOQ commercial tolerance policy stays separate.

| Decision case | Evidence within declared scope |
|---|---|
| Child under invisible/off/frozen parent | measure synthetic fixture keeps nested child outside enumeration; explicit limitation. Separate file off/frozen test excludes direct records in both policies. |
| Child layer 0, hidden then visible parent | synthetic child on 0 stays outside the top-level count in both parent states and after hiding A-WALL. No inherited visible-count claim. |
| Nested insertion and 2×3 array | fixture has nested and top-level arrays. Three plain DOOR records + one array record = 4 records, not a visible-cell total. |
| Cycle / missing xref / dynamic visibility | fixture includes a self-cycle and missing xref; count terminates and retains limitations. Dynamic visibility and clipping are always marked unresolved. |
| User hiding | existing query tests plus synthetic test: as_shown changes, in_file stays 4; off/frozen test excludes both. |
| Real symbol reference 26 and 182 | Outside bounded enumeration acceptance. No new real-drawing claim or heavy rerun. Exact-count unit cases reject 30/26 and 186/182. Quantix repair stays QSM-449. |
| Unapproved room / fountain A-ROOM | net report test excludes unspecified, gross, wall-face and unapproved-net areas. Authoring test verifies unapproved results and labels. |
| Same region, ready boundary vs wall face | JSON and embedded metadata round-trip test; Excel readback checks distinct origin and basis. UI test checks both labels. |
| Absolute pass but >1% error | accuracy test: 0.0102 vs 0.01 with 0.001 tolerance passes absolute and fails relative. Integer, zero and rounding policies tested separately. |
| Old measurement without basis | JSON removes all three new fields; loads unspecified. Existing save tests exercise legacy embedded records. Unspecified rows are excluded from net accuracy. |

Verification: targeted Rust tests, TypeScript no-emit check, and TakeoffPanel Vitest. Exact final counts and source SHA-256 values are recorded with the review handoff.

Review scope: the listed implementation/test files only. The checkout contains earlier geometry, corpus, evaluation, ribbon, and locale changes from other work. They are not part of this task. Locale diffs contain the earlier takeoffEmpty message; only the new takeoff.origin/basis/scope keys belong here. No commit was made; the repository requires a full workspace check before commits, while this assignment calls for small focused checks.

Desktop check after code review: load old project and embedded-only drawing; inspect unspecified basis; measure an entity and picked area; compare boundary/wall-face metadata; hide/unhide counts; export and read Excel; inspect candidate labels in Arabic and English. Keep unsupported counts and unapproved net outside complete-count/net claims.

Validation result: 179 Rust tests passed in the combined targeted suite. The added empty-summary tool test passed separately (1/1). The final candidate-label metadata check passed separately (1/1). TakeoffPanel passed 6/6; tsc --noEmit passed; git diff --check passed. An initial compile failed on an Insert import and an initial prompt test found 709 words; both were fixed and the affected checks passed.

Source snapshot SHA-256 (whole current file; locale hashes include the earlier empty-state text):

```text
aee0209c7aa454fb1dc8efe1d1b2bc7037318edb2197415e6370a2a8b83c5dd1  crates/qiyas-ai/src/execute.rs
ea6ffa22b4395560ae12e79036c0b016bb8d0b1d3f165153e12a659c834c38f1  crates/qiyas-ai/src/execute/author.rs
a4fcb0157fa22aad4f6a02a7c4848c20e1f55ccdc3071b4d8c1796bcbdabe73c  crates/qiyas-ai/src/prompt.rs
90e2c3efdfda3e90df9fd52ab8725d762ae95c295d1db81de47d6d753ed3f1a9  crates/qiyas-ai/src/tools.rs
41125e82e555fdd90f45ea4244a653cbc80f4f13bd4fcd25c0360f62e9daa210  crates/qiyas-ai/tests/authoring.rs
f849e1d9c8518553ae79223257c62029e6a8c9002b8a0b0ece307a84749e39f4  crates/qiyas-ai/tests/tools.rs
ce44e8a4cac8175d414a691f969534b0ee2f25ebe36ca739d2ac132e6ffaf34e  crates/qiyas-boq/src/export.rs
6af8884b8b101fb56206bbccbb5035371015d9bb1866e9eb608fa34a5627eb2e  crates/qiyas-boq/tests/export.rs
ba63f67e44539b0c98aaa822e23fe79f351246c0d5104e442d8ad23034785f29  crates/qiyas-core/src/commands.rs
b36faa05670c7f94714a28ca2ca8d82daa811622cc8a39564c0e1f47b4ada5fd  crates/qiyas-core/src/embed.rs
667468c82b58b7c30b1ecdbbc5333314868758fa0c8174a957bbd691eaa339d8  crates/qiyas-core/src/lib.rs
1834ec84395548c7c78d14b0366654a236f7d252f171fd2ed08575289a2a259f  crates/qiyas-core/src/query.rs
cc362c02faf9ad7d2d3004e507af36cd2a75121d3a7eb6e62696313b6962a2ce  crates/qiyas-core/src/takeoff.rs
2151e8e3affb6e19f64b6abfa9976ba231cb5ad8e6d45708e040c60e14dc6d5a  crates/qiyas-core/tests/measure.rs
a40ba345030c77ead6e78438259e2b13db07fcdc449816e8423385160aee2759  crates/qiyas-takeoff/src/lib.rs
fb4b809646b2cedd8b4f7b533a433a31a0d11b8af4f410fb9fc63b463f958818  crates/qiyas-takeoff/src/accuracy.rs
0caf68a2aae9eb2abe6eae29ed5d5d7169c5869f02d0d478b68d2c15a9248fc7  crates/qiyas-takeoff/src/matching.rs
c8ddf35c321060f20663e81017410142ec7760d27123fdb40ca337d59cec8740  crates/qiyas-takeoff/tests/compare.rs
e18b7cc90a368da223539abde4f46bbf1e69b65a273025e1df86d2e2ac2e37c5  src/api.ts
0a13e03ba16f2240718a760d04de8c257e0bff404920273a59d81b7bd35d6600  src/i18n/ar.json
6eb7be4a6b3fd3dc2b6e78b43a46241fa3038963f998cd2d6a28de1a0485fbca  src/i18n/en.json
d5b76734a1dad0caad0aa56b0c4cff4f097a67457507517806b23159dcf5c5ce  src/shell/TakeoffPanel.tsx
21f50b047d8f9c9cf193cd4d5234f9c1b7f95be4912da2929adb35e36bb0968e  src/shell/TakeoffPanel.test.tsx
```
