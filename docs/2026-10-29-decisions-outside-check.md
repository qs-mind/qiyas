# Decisions and independent outside check

1. `decision.set` stores one entry or an atomic array in drawing knowledge kind `decisions`.
   Each entry retains name, scalar value, unit, reason, source, citation, setter, status and
   optional question. An unanswered question has a null value, never an invented number.
   `decision.list` is a bounded, paged read. Both commands are registry-discoverable; entries
   travel with DWG/DXF knowledge and undo/redo. Generic `set_knowledge` cannot bypass the
   decision schema or linked updates.

   Element `PropSource.decision` names the entry. Placement and property edits validate the
   value against the decision, converting compatible physical units. Updating a decision
   updates its referenced properties, derived geometry, quantities and fields in one undo
   step. Ownership and locked layers still apply. Old provenance records default to no link.

2. `%<\AcVar QiyasDecision "name">%` and `field` kind `decision` print the current value and
   unit through the existing text/table field path. Missing or unanswered entries show
   `####`. Arabic notes use Arabic digits; a following unit or a unit-bearing table heading
   avoids duplicate units.

3. `outside_check` is a read command, reachable as `qiyas_read` op `run` without AI changes.
   Raw counts read sheet-owned CAD entities directly, including hidden layers. References
   group by literal block name and layer; areas and lengths group by layer. Insert arrays
   count their instances. Nested block definitions are not counted as extra sheet entities.
   Stored generated BOQ quantities are compared in compatible units by their source handles.
   Cross-group dimension lines that cannot be assigned are reported, never proportioned.
   Missing source handles are reported separately. Signed deduction lines keep their sign.

   The outline is an explicit boundary, otherwise the outer boundaries of the uncut wall
   union, otherwise the largest closed polyline/hatch. Geometry checks report partial outside
   elements, unattached wall endpoints, enclosed boundaries with no identified opening,
   and overlap areas. Wall/room/opening drawing metadata identifies geometric roles without
   consulting object types, recognition rules, mappings or building classifications. Untagged
   geometry can supply `wall_handles`, `space_handles` and `opening_handles`. Wall-union
   holes also supply enclosures not already represented by retained boundaries; other closed
   boundaries are a fallback when neither is available.
   Missing geometry or roles is reported as `not_checked`, never an implicit pass.

   Optional `answer` text supports English and Arabic digits, physical unit conversions and
   printed numeric precision. Recognized physical units and custom units present in the
   records are matched against decisions, raw measurements, takeoff and generated quantities.
   A free-text number has an answer byte range and empty drawing handles when unsupported.
   Provisional/unanswered dependencies and Mizan design reasons are review entries, not errors.

   Results have bilingual messages, complete handles, global/per-sheet totals and paging.
   The records budget is 12 KB, with the existing single-oversized-record exception to keep
   paging lossless. No design thresholds, country/type rules, acceptance gates or quantity
   mutations are introduced. Analytic lengths/areas are preferred; geometric containment and
   overlap use flattened curves with a disclosed chord error of 0.00001 m. Untagged drawings
   need explicit roles to establish which geometry represents walls or openings. Compound
   units absent from the known physical aliases and the stored records are not interpreted.

4. `takeoff_check` retains its fields and adds `outside_check` totals and a command reference.
   Existing `passed` and export policy are unchanged; the new findings remain factual evidence.

## Scope and tests

Named files: new `src/decisions.rs`, `src/outside_check.rs`; `commands.rs`, `registry.rs`,
`lib.rs`, `field.rs`; new `tests/decisions.rs`, `tests/outside_check.rs`, registered in
`tests/all.rs` (all under `crates/qiyas-core`).

Required supporting edits: `knowledge.rs` registers the lazily created kind;
`measurement.rs` adds backward-compatible provenance; `objects.rs` validates links;
`measurement/evaluate.rs`, `recognition.rs`, `trust.rs` and `walls.rs` initialize the new
optional field in existing constructors; `digest.rs` adds the summary. Existing concurrent
edits in those files were retained. No AI files or prohibited room/planning files were edited.

New regression coverage: DWG/DXF round trips, field updates and undo/redo, unanswered
questions, atomic validation, unit conversion, contradictory citations, live BOQ operand
provenance, locked-property rollback, English/Arabic fields, registry effects/examples,
lossless paging, 28-door recount after BOQ omission, per-sheet grouping, missing BOQ handles,
nonallocatable groups and signed deductions, outline containment, wall union, dangling ends,
opening contact, overlap area, Arabic answer quantities and decision review, takeoff summary.
No existing tests were changed or deleted except adding the modules to `tests/all.rs`.

No build, formatter, test, or automated check was run, per the founder's instruction.
Compilation and runtime verification remain for the single coordinated check after all code
work. No dependencies, commits, staging, windows or browser sessions were added.
