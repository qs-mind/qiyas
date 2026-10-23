# Goal B: honest commands and stable handles

Worktree: `.local/wt/goal-b`, branch `goal/b`. No commit or push. No paid model calls.
Implementation and all headless acceptance jobs are complete. Final verification is partial:
workspace Clippy passes, but the full Rust rerun ran out of disk space and the frontend
rerun cannot resolve the provided dependency link. The exact results are recorded below.

The audit showed edits reporting success after automatic updates had undone them. The new
command boundary checks the requested fields after those updates and cancels the whole edit
when a requested value did not survive. A no-op has no changed entities and no undo step.

Changes under review:

- Explicit opening IDs survive schedules and size edits. Size marks are display defaults.
- Doors move along their wall by changing their stored position. Off-wall moves and lone
  opening copies are refused with the supported alternative.
- Generated parts, live schedule structure, and retained room geometry have explicit edit
  rules. Supported formatting changes apply to the owner; unsupported changes name it.
- Layer renames follow stored references, including custom layer-property defaults and
  choices. Wall layer properties apply through recipe data.
- Wall rebuilds keep surviving identities and formatting. Schedules, fields and later batch
  steps follow replacement maps; ambiguous replacements are refused.
- Handles in commands and errors use decimal. Numeric strings and explicit hexadecimal
  strings are accepted, including MCP record, entity and measurement reads. Object record
  keys can be passed directly to object commands.
- Unchanged ordinary table cells keep their display handles. Unchanged take-off, BOQ and
  conflict edits also keep undo and redo history.
- Unknown editing fields are errors. Tests that previously required warnings for ignored
  edit fields have been changed to require refusal and no mutation. Valid-input default
  tests retain their original checks. Script tests now use decimal or explicit `0x` input.

The retained-region metadata test used to move a boundary on its own to force regeneration.
That move was silently reversed. It now asserts a clear refusal, then triggers a normal
drawing refresh for its original metadata checks. The damaged-object metadata test still
checks the named application and exact entity; its expected handle is now decimal.
The wall-system constraint test used to require constraints to be deleted during rebuilding.
It now requires the same constraint before and after rebuilding, undo and redo because its
centreline survives. A recognition fixture supplied an unsupported `color` on `new_layer`;
that ignored field was removed, keeping its existing white layer and recognition assertions.
Four frontend display assertions now expect decimal 42 instead of hexadecimal 2A. The
separate DWG field-code tests still require the hexadecimal encoding.

The new transform test caught −90° and 270° being compared as different hosted orientations.
The comparison now treats those equivalent angles equally. It still checks the final position.
Custom-ID tests also save and reopen both DWG and DXF in mm, cm, m and in.

The original opening-schedule regression (27 openings, four units, DWG and DXF reopening)
also caught a handle being reused for a different fragment after tiny floating-point changes.
Matching now reserves unchanged geometry before assigning handles to changed geometry, with
the same tolerance used by the final-effect check. Linked fields refresh after schedules,
so intermediate stale schedule text does not become a requested edit.
Joining two previously separate wall groups also needs a consistent graphics owner:
the smallest surviving driver handle. Stable handles can put that driver later in the run
list. Generated hatch boundaries use the same complete-edge comparison as closed outlines,
so a different starting corner or loop order does not falsely reject an equivalent shape.

The first real MCP job caught handle strings being rejected by operation validation before
the command parser saw them. Normalization now happens before that validation. The original
failure is saved at `.local/goal-run/failures/handle-before-validation/`.

Finding coverage:

| Findings | Resulting behavior |
| --- | --- |
| C2 | Overridden requested fields cancel the whole edit and name the automatic update; unchanged edits retain undo and redo. |
| C1, X-B1, M1 | Explicit D1/W1 IDs survive schedules, size edits and file reopening. |
| C3, C4 | Door movement updates distance along its host; off-wall movement and lone copies are refused. |
| C5, C6, X-B4 | Generated-part formatting routes to the owner; unsupported edits name the owner. |
| C9, C10, X-B6 | Live schedule structure and retained room vertex edits are refused clearly. |
| X-B3, X-B5, C14 | Layer references rename together, rebuilds retain formatting, and declared role-layer properties apply. |
| C7, X-B2, M3, X-B7 | Surviving wall/opening members keep handles; schedules and batch references follow replacements. |
| C13 | Empty opening edits do not rebuild, including after file reopening. |
| C8, C18, X-B9 | Public handles use decimal; decimal and explicit hexadecimal strings work, including object keys. |
| C12 | Unknown editing fields are errors before mutation. |

Before evidence is in the founder-approved audit under
`/home/kareem/Work/qs-mind/qiyas/.local/audit/` in the founder's checkout:

| Finding | Before evidence |
| --- | --- |
| Custom IDs overwritten | `.local/audit/flat/transcript.jsonl`, steps 54–81 |
| Batch loses wall handles | `.local/audit/batch-contract/transcript.jsonl`, step 4 |
| Layer rename resurrects old layer | `.local/audit/core-probes/transcript.jsonl`, steps 16–20 |
| Rebuild loses colour and selected schedule | Same transcript, steps 35–44; PNGs 0045 and 0046 |
| Generated edits and cells revert | `.local/audit/core-more-probes/transcript.jsonl`, steps 14–39 |

The end-to-end jobs pass through the rebuilt `target/debug/qiyas-cli mcp` process, using
the audit's stdlib Python client. No provider is involved. Every mutation is followed by
entity, object and layer reads; assertions also check the particular effect and undo history.

All following evidence paths are relative to this worktree:

| Evidence | Result |
| --- | --- |
| `.local/goal-run/verified-{mm,cm,m,in}/transcript.jsonl` | All four units pass, 182 MCP exchanges per drawing. |
| `.local/goal-run/verified-summary.json` | D1 remains on door 286; distance is 2.5 m in every unit. |
| `.local/goal-run/core-probes/transcript.jsonl` | Renamed layer stays renamed; rebuilt red wall and selected schedule survive. |
| `.local/goal-run/core-more-probes/transcript.jsonl` | Object string key works; generated raw edits and live cells/rows are clearly refused. |
| `.local/goal-run/batch-contract/transcript.jsonl` | Both wall-opening `$step` references work; ordinary batch reference control also passes. |
| `.local/goal-run/replayed-summary.json` | All three founder audit replays pass their assertions. |

The four-unit transcripts use the same steps: 22 moves the door from 2.0 m to 2.5 m;
60 adds two openings using the original wall handles; 65 assigns W1; 75 returns empty
change lists and unchanged history for an empty opening edit; 80 applies wall layer
properties; 106 refuses the partially overridden raw edit and keeps its original colour;
138 reads the retained room as 12 m²; 144 checks an unchanged ordinary cell; 149–154 join
two lines, follow the replaced `$step` handle, set the joined line red and measure 2 m.
Steps 165–180 add two separate 10 m walls and join them with a 4 m bridge. Both original
wall handles survive, the result is one 24 m wall group, and the existing door and schedule
remain valid. The final capture shows both horizontal walls and the connecting bridge.

The refusal at step 106 names entity 278, the `invisible` field and “object geometry and
hosted positions” as the update that overrode it. The second requested field (colour) is
rolled back too. The no-op at step 75 reports `added: []`, `changed: []`, `removed: []`.

Saved and visually inspected PNGs:

- `verified-mm/0021-before-move-0.png` and `verified-mm/0159-after-verified-0.png` show
  the door shifted along the wall, the extra windows and retained red formatting.
- `verified-mm/0161-opening-ids-closeup-0.png` shows D1, W1 and the remaining default
  size mark, with a total of three openings. The equivalent inches capture was inspected too.
- `verified-cm/0163-stable-schedule-closeup-0.png` shows “Legacy wall”, count 1.000;
  the earlier audit showed `####` after rebuilding.
- `verified-m/0164-room-boundary-kept-0.png` shows the original rectangular boundary
  after both direct vertex edits were refused.
- `verified-in/0180-joined-wall-groups-0.png` shows the two wall groups joined by a bridge;
  equivalent captures in mm, cm and m were checked too.
- `core-probes/0045-core-final-0.png` retains red wall formatting; `0046-stale-schedule-closeup-0.png`
  shows the valid selected-wall count. `core-more-probes/0044-schedule-closeup-0.png`
  is identical to the valid table capture, including after refused edits.

The PNG paths above are beneath `.local/goal-run/`. There are 27 final PNGs. Companion captures exist for all
four units. The general wall view intentionally uses a wide scale; table close-ups provide
readable text. The Arabic body label renders in the supplied font setup.

All Cargo commands used the required shared target directory, two build jobs, no dev/test
debug information and no incremental compilation. No `cargo clean` was run. Checks used
offline dependencies. Log paths in the table are beneath `.local/goal-run/`.

| Check | Actual result and evidence |
| --- | --- |
| `cargo fmt --all` | Passed. Later Rust edits also passed `rustfmt --check`; `fmt-final-check.exit` is 0. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Passed on the final source; `clippy-final.log`. An earlier new-test allocation warning was corrected. |
| `cargo build --offline -p qiyas-cli` | Passed on the final source; `cli-build-final.log`, executable hash in `cli-sha256.txt`. |
| Four-unit headless acceptance | Passed, 728 MCP exchanges total; `verified-summary.json`, `verify-goal-b-final.log`. |
| Three founder audit replays | Passed; `replayed-summary.json`, `core-probes-final.log`, `core-more-probes-final.log`, `batch-contract-final.log`, `replay-assertions-final.log`. |
| Full `qiyas-core` tests | Initial run: 777 passed, 3 failed; `core-test.log`, totals by target in `initial-test-results.json`. The three failures were addressed: recognition subsequently passed 9/9, and the final wall suite passed 6/6. A complete final rerun could not finish linking because the disk was full. |
| New command regression suite | Final compiled executable passed 15/15; `honest_commands-binary-final.log`. This includes save/reopen, no-op history, four-unit movement, custom IDs, stable handles and atomic rollback. |
| Wall and recognition regressions | Wall suite 6/6 in `wall_systems-binary-final.log`; recognition 9/9 in `core-regressions-rerun.log`. The earlier wall failure in that latter log was fixed afterwards and verified by the final wall suite and four-unit MCP jobs. |
| Full `qiyas-ai` tests | Initial run: 183 passed, 36 failed, 1 ignored; `ai-test.log`. Of the failures, 35 could not open local ports in this sandbox. The other was a new measurement fixture assuming mm although its sample is in metres; the fixture now uses drawing units. Its corrected executable has not been verified. |
| Final combined core/AI test rerun | Exit 101 before tests: LLVM reported “No space left on device”; `core-ai-final.log`. This is not a passing full-suite result. |
| Required AI `--lib --test surface --test agent` rerun | Exit 101 before tests: the surface test linker terminated with signal 7 while the root filesystem had about 5 MB free; `ai-required-final.log`. The old surface executable passed 56/57 and still contains the old fixture; `surface-binary-final.log`. |
| `node_modules/.bin/tsc --noEmit` | Initial run passed (`tsc.exit` is 0). Final rerun could not start: “Too many levels of symbolic links”; `tsc-final.log`. |
| `node_modules/.bin/vitest run --maxWorkers=2` | Initial run: 840 passed, 5 failed across 114 files; `vitest.log`. Three failures expected old hexadecimal labels and are corrected in source. Two untouched Ribbon tests timed out at 5 seconds. Final rerun could not start because of the dependency link; `vitest-final.log`. |
| `git diff --check` | Passed. |

The core total above counts each target's final summary, excluding two nested child-process
summaries that otherwise double-count passing project tests. The AI ignored test is
`two_private_drawing_reference_answers`, which requires private drawings and
`QIYAS_REAL_EVAL_REFERENCES`.

The 35 port-blocked tests are listed individually in `.local/goal-run/port-blocked-tests.txt`:
one model-discovery unit test, 25 agent tests and nine HTTP MCP tests. They failed with
`PermissionDenied` opening local listeners; no provider calls were made. Stdio MCP acceptance
is unaffected and passed. The two unresolved frontend timeouts are
`Ribbon.test.tsx` / “the View tab says whether the drawing's Arabic text is reversed” and
`RibbonUi.test.tsx` / “the pointer resting on a button shows a tooltip: name, what it does,
its alias; leaving hides it”. Their tests and timeouts were not loosened.

Final command exit codes are in `.local/goal-run/final-check-results.json`. The older
`check-results.json` describes the first run, including failures subsequently fixed.

Files changed are listed exactly in `.local/goal-run/changed-files.txt`. Changes are confined
to `qiyas-core` command/derivation/identity code, its two legacy recipe data files and tests;
`qiyas-ai/src/surface.rs` and its surface tests; seven frontend handle-label files and four
existing display tests;
`CLAUDE.md`; and this report. The new core
modules are `net_effect.rs`, `handle_refs.rs` and `layer_refs.rs`; the new integration test is
`honest_commands.rs`. The smaller changes to audit, drawing, recognition and measurement
messages replace public hexadecimal handle formatting with decimal.

No new dependencies, paid calls, commits or pushes. No provider, prompt or `check.rs`
changes. Finishing C8 required a narrow exception to the frontend ownership boundary:
`GeneratedBoq.tsx`, `ConflictsPanel.tsx`, `Explain.tsx`, `ConstraintBar.tsx`,
`FieldDialog.tsx`, `RecognitionPanel.tsx` and `PropertiesPanel.tsx` only change displayed
handle numbers to decimal. The DWG field code remains hexadecimal. These small substitutions
should be retained when merging Goal F's UI changes. The provided `node_modules` and Thmanyah
symlinks were left in place.

To finish validation, free space in the shared build filesystem and provide an intact
installed `node_modules` directory. The founder's current `node_modules` path points to
itself; this worktree's original link points there. I did not change that founder path or
install packages. Then rerun the full core/AI tests, the required AI subset, TypeScript and
Vitest; rerun the listed local-port tests in an environment that allows listeners.
No product decision is needed for these remaining checks.

Unsupported generated edits remain deliberate refusals, as allowed by Goal B. An explicit
ID in an old file that exactly equals an old automatic size mark cannot be distinguished
without provenance; setting that ID again records it explicitly for future edits.
