# Clinic engine replay

`clinic-performance.json` preserves all 85 completed tool calls in the supplied
`clinic.log` (SHA-256 `820355a5a5ab06fca2a281aa50bf269fbf9092d982bf95f9432b79db32a99918`).
The log starts on 24 October 2026 at 19:50 and ends at 23:03:18. The later
487-second move and work through 23:58 mentioned in the clinic notes are not
completed calls in this file.

The fixture comes from the Goal R research harness. That harness recovered three
clipped inputs (68, 69 and 72) by matching their call IDs in the clinic conversation.
No provider replies, credentials or settings files are needed for this replay.
The initial metre session and subsequent new millimetre session apply the name-only
Settings method `POMI (RICS)` before any commands. This reproduces the live handles.
Complete revision tokens are rebound to the isolated session; the recorded stale
revision stays stale, and the malformed revision stays malformed. Host tab listing
is emulated, and the desktop Save As dialog is omitted. Export paths stay in the
run directory. These host adaptations are recorded beside each call.

Run `cargo run --offline -p qiyas-ai --example clinic_performance` (with the assigned
worktree's `CARGO_TARGET_DIR`). `CLINIC_LABEL` selects the output folder below
`.local/goal-run/`; use a distinct baseline/after label for each build profile.
Each call saves its result, duration, full entity/quantity/measurement snapshot,
and any PNG. Every error verifies entities, document structure, history and revision
against the before state; every call verifies surviving wall membership. A recorded
refusal of the input's shape alone (`missing_field`/`unknown_field`, such as call 24's
`run` with the command's fields at the top level) may now succeed: Goal A's normalisation
wraps those fields, and the replay notes it instead of failing.

For strict comparison, run the after executable with `CLINIC_SESSION_DIR` pointing
at the baseline output directory and `CLINIC_LABEL=BASELINE_LABEL/AFTER_LABEL`.
The after directory must be a subdirectory of the baseline directory because
exports are restricted to the source drawing folder. The session path is only a
logical drawing name; evidence and exports still go to that separate after directory.
Run `python3 scripts/compare-clinic.py BEFORE AFTER`. It compares floating-point
bits, all saved records and results, and revision context fingerprints. Only random
session UUIDs and isolated evidence paths are rebound. No quantity, revision count,
handle or finding is excluded. Image captions retain their revision fingerprints, and captured PNG bytes are compared.

Exported PDF bytes and all workbook members are also compared. Workbook creation
and modification timestamps alone are normalized; worksheet values, formulas and
styles must remain exact.
