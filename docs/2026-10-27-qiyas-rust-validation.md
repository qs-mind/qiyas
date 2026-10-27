# Rust validation — 27 October 2026
Rust compiles, formatting and Clippy pass, and all six requested release guards pass.
The exact workspace test command fails because the sandbox denies loopback sockets.
A diagnostic `cargo test --workspace --no-fail-fast` completed all targets: **1,777 passed,
50 failed due to socket permission denials, and 20 already ignored**. Nested child-process
summary lines are excluded from those totals. No tests were deleted, skipped, newly ignored,
or loosened; no speed or memory limits were raised.

All Cargo commands ran individually with `CARGO_INCREMENTAL=0` and `nice -n 10`.
The existing four-job configuration was retained. Release checks used
`RUST_TEST_NOCAPTURE=1` to capture their figures. No profiles or dependencies changed.
No root `src/` files or excluded sample files were touched. No windows or browsers were opened.
No staging, commits, or pushes were performed.

## 1. Fixes by area

- **W3/output:** Arabic window-size expectations now match the requested Arabic digits and unit.
  The conflicts workbook test accounts for its review footer, and the print area includes that
  footer. PDF text explicitly assigns empty `ActualText` to separately drawn Arabic dot/mark
  glyphs, which prevents replacement characters from breaking extracted review notices.
- **W5/drafting:** Exact command-name components rank before substring matches, so `hatch`
  finds `draw.hatch` first. Stair cuts and handrails meet their boundaries; soil ticks include
  the ends of the ground line. The registry's general drawing check now passes these symbols
  in mm, cm, metres and inches. The en-suite evidence membership check is Clippy-clean.
  Existing drafting-knowledge, planning, provenance, symbol, rollback and undo tests pass.
- **W6/take-off:** Second-reading query keys distinguish full size-split item codes. Resolving
  one size cannot admit another; changed readings or units do not inherit an old acceptance.
  Counts use zero tolerance. The blind reader redacts supported Arabic/Persian digits and
  integer counts, rejects a quantity changed during its provider call, counts unattempted
  items correctly on failure, and creates queries before an export-triggered pass returns.
  The evidence-export argument schema now accepts its documented `path` and `language`.
  The drawing-pack fixtures pass without behavioral changes to the pack check.
- **Merged test binaries/build:** Shared helpers are declared once through missing `#[path]`
  entries in the existing `tests/all.rs` binaries and imported by their users. The registry
  example harness creates the generated item needed by second-reading examples. The absolute
  RSS PDF guard runs its original operation and 300 MB assertion in a child process, so other
  tests' allocator/PDF caches cannot contaminate it. Async test mutex guards end before await.
  Clippy issues and the handoff's formatting are fixed. The fill guard logs its measured
  values after timing; its assertion is unchanged.

New regressions cover independent size disagreements and changed accepted readings, Arabic
and integer-count redaction, stale reader answers, failed-reader accounting, and query creation
without a later compare. An independent read-only review of W6 supplied the blind-reading
findings; no reviewer ran concurrent builds or edited files.

## 2. Files touched

The list includes files changed only by the requested workspace formatter.
- [crates/qiyas-ai/src/blind_reader.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/blind_reader.rs)
- [crates/qiyas-ai/src/surface.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/surface.rs)
- [crates/qiyas-ai/src/surface_args.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/surface_args.rs)
- [crates/qiyas-ai/tests/all.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/all.rs)
- [crates/qiyas-ai/tests/drafting_eval.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/drafting_eval.rs)
- [crates/qiyas-ai/tests/goal_a.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/goal_a.rs)
- [crates/qiyas-ai/tests/goal_j.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/goal_j.rs)
- [crates/qiyas-ai/tests/goal_l.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/goal_l.rs)
- [crates/qiyas-ai/tests/surface.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/surface.rs)
- [crates/qiyas-ai/tests/w3.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/tests/w3.rs)
- [crates/qiyas-boq/src/dimensions.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/src/dimensions.rs)
- [crates/qiyas-boq/src/export.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/src/export.rs)
- [crates/qiyas-boq/src/report.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/src/report.rs)
- [crates/qiyas-boq/tests/all.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/all.rs)
- [crates/qiyas-boq/tests/deliverables.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/deliverables.rs)
- [crates/qiyas-boq/tests/dimensions.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/dimensions.rs)
- [crates/qiyas-boq/tests/export.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/export.rs)
- [crates/qiyas-boq/tests/report.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/report.rs)
- [crates/qiyas-boq/tests/w3_workbook.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-boq/tests/w3_workbook.rs)
- [crates/qiyas-core/src/commands.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/commands.rs)
- [crates/qiyas-core/src/deliverables.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/deliverables.rs)
- [crates/qiyas-core/src/digest/measurement_checks.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/digest/measurement_checks.rs)
- [crates/qiyas-core/src/discrepancy/mod.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/discrepancy/mod.rs)
- [crates/qiyas-core/src/draft_symbols.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/draft_symbols.rs)
- [crates/qiyas-core/src/drafting_standards.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/drafting_standards.rs)
- [crates/qiyas-core/src/lib.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/lib.rs)
- [crates/qiyas-core/src/measurement/evaluate.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/measurement/evaluate.rs)
- [crates/qiyas-core/src/pack_check.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/pack_check.rs)
- [crates/qiyas-core/src/quantity_notes.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/quantity_notes.rs)
- [crates/qiyas-core/src/registry.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/registry.rs)
- [crates/qiyas-core/src/second_reading.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/second_reading.rs)
- [crates/qiyas-core/src/sheet_setup.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/sheet_setup.rs)
- [crates/qiyas-core/src/space_planning.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/space_planning.rs)
- [crates/qiyas-core/tests/all.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/all.rs)
- [crates/qiyas-core/tests/check.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/check.rs)
- [crates/qiyas-core/tests/goal_j.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/goal_j.rs)
- [crates/qiyas-core/tests/goal_u.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/goal_u.rs)
- [crates/qiyas-core/tests/image.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/image.rs)
- [crates/qiyas-core/tests/measurement.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/measurement.rs)
- [crates/qiyas-core/tests/measurement_methods.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/measurement_methods.rs)
- [crates/qiyas-core/tests/modify.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/modify.rs)
- [crates/qiyas-core/tests/own_output.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/own_output.rs)
- [crates/qiyas-core/tests/param.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/param.rs)
- [crates/qiyas-core/tests/pdf.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/pdf.rs)
- [crates/qiyas-core/tests/trust.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/trust.rs)
- [crates/qiyas-core/tests/w3_output.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/w3_output.rs)
- [crates/qiyas-core/tests/w5_drafting.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/w5_drafting.rs)
- [crates/qiyas-desktop/src/plot.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-desktop/src/plot.rs)
- [crates/qiyas-plot/src/evidence.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/evidence.rs)
- [crates/qiyas-plot/src/font.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/font.rs)
- [crates/qiyas-plot/tests/plot.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/tests/plot.rs)
- [crates/qiyas-render/tests/fills.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-render/tests/fills.rs)
- [docs/2026-10-27-qiyas-rust-validation.md](/home/kareem/Work/qs-mind/qiyas/docs/2026-10-27-qiyas-rust-validation.md)

## 3. Final requested checks

The commands below were run in this order after the fixes. The workspace failure was retained
and reported; it was not bypassed by changing or skipping tests. Full logs are linked beside
each command. Every final `test result:` line is reproduced, including zero-test binaries
selected by Cargo's filters.

**`nice -n 10 cargo fmt --all`** — PASS — exit 0. [Log](/tmp/qiyas-final-fmt.log)

No formatter diagnostics.

**`nice -n 10 cargo clippy --workspace --all-targets -- -D warnings`** — PASS — exit 0. [Log](/tmp/qiyas-final-clippy.log)

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.65s
```

**`nice -n 10 cargo test --workspace`** — FAIL — exit 101; four loopback socket denials stop this command at the AI library. [Log](/tmp/qiyas-final-workspace.log)

```text
test result: FAILED. 169 passed; 4 failed; 14 ignored; 0 measured; 0 filtered out; finished in 5.26s
```

**`nice -n 10 cargo test -p qiyas-render --release pick_index_one_million`** — PASS — exit 0. [Log](/tmp/qiyas-final-pick.log)

```text
PickIndex::new 1 M segments: 240.753151ms, reference 222.613599ms, ratio 1.0815, limit 1.5
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 13 filtered out; finished in 0.51s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 7 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
```

**`nice -n 10 cargo test -p qiyas-render --release --test fills fill_in_a_block_inserted_500_times`** — PASS — exit 0. [Log](/tmp/qiyas-final-fills.log)

```text
500 fill inserts: build 0.005088 s, 500 triangulations 0.081833 s, ratio 0.0622, limit 0.25
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.09s
```

**`nice -n 10 cargo test -p qiyas-param --release --test solver_speed`** — PASS — exit 0. [Log](/tmp/qiyas-final-solver.log)

```text
200 constraints: 2.069406ms, reference 181.521742ms, ratio 0.0114, limit 0.125
200 constraints re-solve: 1.216326ms, reference 181.521742ms, ratio 0.0067, limit 0.125
first solve 2.069406ms, re-solve 1.216326ms
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.20s
```

**`nice -n 10 cargo test -p qiyas-geom --release walls_speed`** — PASS — exit 0. [Log](/tmp/qiyas-final-walls.log)

```text
500 walls: 12.24765ms, reference 184.007127ms, ratio 0.0666, limit 5
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 29 filtered out; finished in 0.21s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 21 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 11 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.00s
```

**`nice -n 10 cargo test -p qiyas-ai --release --test villa_openings villa_speed -- --ignored --nocapture`** — PASS — exit 0. [Log](/tmp/qiyas-final-villa.log)

```text
largest villa opening batch: 312.48247ms, reference 213.943116ms, ratio 1.4606, limit 2
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 1.08s
```

**`nice -n 10 cargo test -p qiyas-ai --release --test clinic_performance clinic_speed -- --ignored --nocapture`** — PASS — exit 0. [Log](/tmp/qiyas-final-clinic.log)

```text
largest clinic call: 997.188683ms, reference 216.193845ms, ratio 4.6125, limit 8
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 12.51s
```

### Diagnostic workspace coverage

`nice -n 10 cargo test --workspace --no-fail-fast` was an additional diagnostic run to reach
all crates despite the sandbox failures. [Full diagnostic log](/tmp/qiyas-test-diagnostic-2.log).
These are the final summaries per test target; nested child summaries are not counted twice.

```text
qiyas_ai / unit tests
test result: FAILED. 169 passed; 4 failed; 14 ignored; 0 measured; 0 filtered out; finished in 5.31s
qiyas_ai / tests/all.rs
test result: FAILED. 171 passed; 41 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.99s
qiyas_ai / tests/clinic_performance.rs
test result: ok. 2 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.17s
qiyas_ai / tests/villa_openings.rs
test result: ok. 2 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.10s
qiyas_boq / unit tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_boq / tests/all.rs
test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.66s
qiyas_cli / unit tests
test result: ok. 66 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.13s
qiyas_cli / tests/agent_eval.rs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.47s
qiyas_cli / tests/cli.rs
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 33.81s
qiyas_cli / tests/projects.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s
qiyas_core / unit tests
test result: ok. 89 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.19s
qiyas_core / tests/all.rs
test result: ok. 832 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.35s
qiyas_desktop_lib / unit tests
test result: FAILED. 114 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 13.29s
qiyas_desktop / unit tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_desktop / tests/options.rs
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
qiyas_desktop / tests/settings.rs
test result: FAILED. 20 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_geom / unit tests
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
qiyas_geom / tests/dimension.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_geom / tests/ellipse_length.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
qiyas_geom / tests/exact_hatch.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_geom / tests/flatten.rs
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.32s
qiyas_geom / tests/paint.rs
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_geom / tests/table.rs
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_mcp / unit tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_mcp / tests/bridge.rs
test result: FAILED. 1 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
qiyas_param / unit tests
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_param / tests/expr.rs
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_param / tests/residuals.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_param / tests/solver.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s
qiyas_param / tests/solver_speed.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_pdf / unit tests
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_pdf / tests/pdf.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
qiyas_plot / unit tests
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_plot / tests/ctb.rs
test result: ok. 11 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s
qiyas_plot / tests/plot.rs
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.26s
qiyas_plot / tests/print.rs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_render / unit tests
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_render / tests/batch.rs
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.57s
qiyas_render / tests/display_modes.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_render / tests/fills.rs
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_render / tests/text.rs
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_render / tests/visibility.rs
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_takeoff / unit tests
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_takeoff / tests/compare.rs
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
qiyas_takeoff / tests/generated.rs
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
Doc-tests qiyas_ai
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_boq
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_core
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_desktop_lib
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_geom
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_param
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_pdf
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_plot
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_render
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests qiyas_takeoff
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## 4. Existing test expectations changed

1. `qiyas-core/tests/w3_output.rs::the_boq_lists_doors_and_windows_by_size_even_when_the_mapping_says_various`:
   the Arabic window description expects `١٢٠٠ × ١٢٠٠ مم`, replacing the Western-digit English-unit
   suffix. This is the Arabic-output behavior required by W3 and its review commits.
2. `qiyas-boq/tests/report.rs::export_conflicts_is_readable_by_calamine`:
   the worksheet height is 6 rather than 4 because W3 adds a blank separator and the review footer.
   The test now also asserts the exact `Status: For review` cell.

Other test edits are formatting, helper imports, Clippy-equivalent expressions, missing fixture
setup, process isolation with the same assertion, timing output, or new regression assertions.
No other existing expected behavior was changed.

## 5. Remaining limits and decisions

- The full workspace cannot pass inside this sandbox. All 50 remaining diagnostic failures
  contain `Operation not permitted` while creating or binding sockets; they require a run in
  an environment that permits loopback sockets. Their implementation behavior remains unverified
  here. The exact requested workspace command stops after the first four, as its log shows.
- The clinic replay keeps the W3 handoff's documented call-83 stale-handle rollback case.
  It verifies rollback but does not restore the heavy successful edit for that recorded batch.
  This pre-existing fixture behavior and its speed limit were not changed.
- No regulatory value or clause was added, and no code-register provenance was guessed.
  No further product decision was needed for these fixes.

The socket-blocked tests are listed below so they can be checked outside the sandbox:
- `chatgpt_plan::responses::capability_regressions::unsupported_reasoning_is_dropped_on_retry_and_on_the_next_turn`
- `chatgpt_plan::responses::acceptance::mock_http_responses_real_agent_edits_and_reads_back`
- `models::tests::an_unreachable_service_falls_back_to_the_builtin_list`
- `claude_code::tests::fake_cli_drives_private_mcp_with_readback_and_undo`
- `agent::billing_is_immediate_and_drawing_errors_never_add_a_takeoff_report`
- `agent::a_new_drawing_made_mid_turn_is_drawn_on_in_the_same_turn_and_save_asks_the_user`
- `agent::api_key_absent_from_logs_and_errors`
- `agent::attached_pictures_go_before_the_words`
- `agent::a_marked_region_lists_what_is_in_it`
- `agent::a_selection_is_listed_and_shown`
- `agent::all_server_failures_retry_with_a_visible_wait_and_identical_request`
- `agent::a_question_ends_the_turn_cleanly_and_the_answer_carries_on`
- `agent::an_answer_cut_off_before_it_said_anything_is_asked_again_with_more_room`
- `agent::an_answer_that_keeps_running_out_stops_at_the_cap`
- `agent::cancel_between_tool_calls_runs_no_more_of_them`
- `agent::file_tools_without_a_host_say_to_use_the_app`
- `agent::cancel_stops_within_one_second`
- `agent::history_is_append_only_and_system_prompt_is_stable`
- `agent::identical_malformed_input_stops_on_sixth_attempt`
- `agent::identical_pictures_keep_the_prefix_and_do_not_resend_images`
- `agent::gemini_parallel_calls_run_apart_and_the_log_tells_the_whole_turn`
- `agent::invalid_output_value_keeps_the_cap_and_does_not_retry`
- `agent::mock_openai_keeps_cache_identity_prefix_and_exact_deepseek_reasoning`
- `agent::invalid_tool_json_is_answered_with_an_error_and_not_run`
- `agent::mid_stream_overload_discards_partial_text_and_keeps_a_retry_log`
- `agent::partial_usage_chunks_retain_prior_counts_through_the_http_adapter`
- `agent::openai_compatible_drops_a_refused_field_and_asks_again`
- `agent::openai_compatible_streams_text_and_tool_calls`
- `agent::refusal_stop_reason_surfaces_category`
- `agent::rejected_request_after_find_entities_is_logged_without_retry`
- `agent::successful_reads_continue_beyond_40_calls`
- `agent::tool_use_round_trip_executes_against_the_session`
- `agent::text_stream_is_forwarded_in_order`
- `agent::view_image_stops_when_the_image_budget_is_used`
- `agent::removals_and_resolutions_report_counts_without_blocking`
- `key_safety::network_tests::model_discovery_never_follows_a_redirect_with_credentials`
- `mcp::a_restart_on_the_same_port_binds_again`
- `mcp::a_wrong_token_is_refused_with_401`
- `mcp::concurrent_counts_both_land`
- `mcp::execute_request_ids_replay_without_another_mutation_or_ui_event`
- `mcp::lists_the_catalogue_and_measures_as_mcp`
- `mcp::other_origins_are_refused_even_with_the_token`
- `mcp::sheet_layer_and_zoom_changes_reach_the_window`
- `mcp::tool_errors_are_error_results`
- `mcp::view_image_is_an_image_block`
- `commands::tests::the_mcp_server_follows_the_settings`
- `credentials_follow_the_saved_address_and_never_a_draft`
- `a_dead_url_answers_not_running`
- `a_wrong_token_says_so`
- `lists_and_calls_through_the_bridge`
