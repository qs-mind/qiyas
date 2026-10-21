# Qiyas for Quantity Surveyors: the autonomous senior QS

## The goal (north star)

> Give Qiyas a tender package (drawings in DWG or PDF, specs, and the client's BOQ or none).
> Within an hour it returns a complete, auditable take-off and BOQ, measured to the standard
> you work to.
> - Every quantity traces to the drawing.
> - Every assumption is flagged.
> - Every discrepancy is raised as a query.
>
> A senior QS signs it after a review, not a re-measure. When revision B arrives, Qiyas
> returns the variation quantities in minutes.

Qiyas does **quantities only**, never rates. It works on **anyone's drawings**: messy
consultant DWGs, PDFs and scans, not just drawings Qiyas made. The villa was one example.

**How success is measured** (on a benchmark of real projects, each with a senior QS's own
take-off as ground truth):

| Measure | Target |
|---|---|
| Accuracy per BOQ item against the senior QS | within ±2% on 95% of items |
| Coverage: BOQ items measured automatically | over 90% |
| Time: tender take-off | hours, not days |
| Traceability: quantities with a geometry and rule reference | 100% |
| Discrepancies found that a senior QS would raise | over 90% |
| Revision to variation statement | minutes |

## What a senior QS actually does (the jobs Qiyas must do)

1. **Take-off:** measure every element from plans, sections, elevations and schedules, across
   architecture, structure, MEP and external works.
2. **Measure to a standard:** units, deduction thresholds and what each item includes, per
   POMI, NRM2, SMM7, CESMM4 or local practice in KSA, UAE and Egypt.
3. **Write the BOQ:** items with standard descriptions in English and Arabic, units and
   quantities, grouped by trade and section.
4. **Check the consultant's BOQ** against the drawings: missing, over- or under-measured, and
   wrong-unit items.
5. **Cross-check the documents:** schedule vs plan, architecture vs structure vs MEP, spec vs
   drawing, revision vs revision.
6. **Raise queries (RFIs)** on everything unclear, clouded on the drawing.
7. **Post-contract:** variations from revisions, re-measurement, progress quantities from site
   mark-ups, final account quantities.
8. **Deliver:** dimension sheets (timesing, squaring, deductions), a BOQ in the client's
   format, a query list and a variation statement, in Excel and PDF.

## Principles (standing rules)

- **No hard-coded knowledge:**
  - Qiyas has general engines that compute and verify exactly.
  - Mizan writes the knowledge as data, inside the drawing: element types, recognition
    rules, measurement rules, BOQ mappings, checks and report layouts.
- **Mizan decides and writes; Qiyas computes.** Mizan never types a quantity.
- **Every number has a source:** the drawing (handle, sheet, grid), the user's document
  (page, clause) or "assumed", which is flagged.
- **Quantities only.** Any language (English and Arabic), any units, any drawing source.
- Judged on a **portfolio of real project types**: towers, offices and fit-out, schools,
  clinics, hotels, warehouses, MEP-heavy buildings, external works and roads. Never tuned to
  one.

## The engines (built once, general)

| Engine | What it does | What Mizan writes as data |
|---|---|---|
| **E0 Foundation** | knowledge store inside the DWG (named-object dictionary XRecords); a re-derive hook run after every edit; an upgraded expression engine; validation | — |
| **E1 Recognition** ("read any drawing") | turns plain geometry into typed elements: layers, blocks, hatches, closed regions, text, legends and schedules, plus Mizan's vision on pictures; PDF vector and raster drawings; xrefs; a confidence per element | recognition rules ("blocks named D-* on A-DOOR are doors, width from the attribute") |
| **E2 Element types** | typed elements with properties, levels and heights, quantities as expressions; drawn and edited as normal; DWG/DXF round trip and undo | type definitions (wall, slab, column, door, duct, kerb…) |
| **E3 Measurement rules** | applies a standard consistently: units, deductions ("openings over 0.5 m² deducted"), what an item includes, rounding | measurement rules from the standard document the user supplies |
| **E4 Dimension sheets** | traceable formulas (timesing, squaring, adds and deducts) with a reference per line; evaluated exactly | the dimension lines per item |
| **E5 BOQ engine** | builds items from mappings (description template, unit, section, trade); reads and writes any client BOQ format; bilingual | BOQ mappings and description templates |
| **E6 Discrepancy engine** | compares everything with everything: BOQ ↔ drawings, schedule ↔ plan, discipline ↔ discipline, spec ↔ drawing; produces findings and RFIs with clouds | check rules with citations |
| **E7 Documents** | reads PDF, Excel and Word text (specs, BOQs, preambles, standards, drawing registers) as read-only input | value tables extracted with their source |
| **E8 Revisions** | compares revisions of a drawing; carries the take-off and recognition over; quantity deltas per item; clouds the changes | — |
| **E9 Projects** | many drawings as one project (the sheet set `.qss`); quantities and checks across drawings; overnight batch runs | — |
| **E10 Deliverables** | report templates producing dimension sheets, BOQ, query list and variation statement, in Excel and PDF, bilingual | report layouts |
| **E11 Trust** | confidence and a review queue; approval modes (per job, per phase, autonomous); saved conversations; "how did you get this number"; your view separate from Mizan's sheet | — |

## Phases (in order of value to a QS)

### Now
- **Phase 13:** fix the one failing plot test (viewport frames now on non-plotting `A-VIEW`:
  the test gets its own layer), run the full check, commit.
- **Phase 14:** saved conversations. The brief is ready. Add the **approval modes** setting
  (per job default, per phase, autonomous), enforced in the agent around the `plan`
  operation; a failed check always pauses.

### Phase 15: The QS benchmark (built first, so every later phase is scored)

Built from the research (`.local/research/reports/QS benchmark sources and methods.md`).

**Store:** `.local/benchmark/` in the repo, git-ignored (development data; never in `~/.qiyas`).
- `sources/`: raw downloads, each with its licence and a provenance log entry.
- `cases/<id>/`: the drawing (DWG, DXF or PDF), `truth.json` (each item's quantity, unit,
  kind and source objects) and its variants.
- `holdout/`: sealed; dev tests never read it.

**Sources (open licences only):**
- buildingSMART Medical-Dental Clinic and Duplex Apartment IFC (CC BY 4.0): plans per storey
  exported to DXF, with truth computed from the model geometry.
- ResPlan and Swiss Dwellings (CC BY 4.0): a few hundred plans rendered to DXF in metres,
  with truth from their geometry.
- **Qiyas's own generator:** varied projects drawn through Qiyas commands, writing exact truth
  as they draw: an office fit-out, a warehouse, an apartment floor, MEP runs, external works
  and a non-building part. Deterministic seeds.
- Restricted tender packs (UNDP, US road departments, Kenya) are logged as candidates, never
  downloaded until Kareem has permission.

**Variants:**
- **Messy copies:** exploded blocks, wrong and merged layers, overlaps and gaps, text drawn as
  lines, odd units, missing schedules.
- PDF exports, vector and scanned.
- **Seeded errors:** missing, wrong-quantity and wrong-unit BOQ items; schedule/plan
  mismatches; a revision B with known changes.

**Harness:** `qiyas-cli bench`.
- Scores a submitted take-off or BOQ against the truth:
  - counts exact;
  - geometry within 0.5%;
  - rule-based quantities within 2–5%;
  - judgement items within 10%;
  - ±25% also reported, for comparison with TakeoffBench.
- Also measures coverage and traceability (it re-measures the cited handles), precision and
  recall on seeded errors, false alarms on clean cases, time and repeatability.
- Produces a report, plus a baseline run of today's Qiyas.
- Live Mizan runs on the benchmark, each with Kareem's OK.

**Tooling:** benchmark converters may use a Python venv under `.local/benchmark/.venv`
(ifcopenshell, shapely, ezdxf). They are never shipped and never committed. Repo code
(generator and harness) uses only licence-approved Rust crates.

**Done when:**
- The store holds at least 50 dev cases across five or more project types, plus a sealed
  holdout.
- `qiyas-cli bench run` produces a scored report for today's Qiyas.
- The report says what the next phases must improve.

### Phase 16: Foundation and element types (E0, E2)

**Builds:**
- The knowledge store in the DWG (named-object dictionary `QIYAS_KNOWLEDGE`, XRecords holding
  JSON, undo through the objects snapshot, round-trip tests).
- One re-derive hook in `Session::record_document_change`, replacing the hard-coded
  rooms/schedules `if`s.
- The expression engine upgrade (qiyas-param `expr.rs`: powers, comparisons, logic, `if`,
  `min`, `max`, `round`, lookups).
- Element types as data, with instances on driver entities (`QIYAS_OBJECT`) and generated
  graphics.
- Walls, rooms and openings migrated onto it, with parity tests.

**Done when:** Mizan defines element types for five unrelated trades in tests (blockwork
wall, slab, column, duct, kerb). Each is placed and measured, with no new code.

### Phase 17: Read any drawing (E1, E7 basics)

**Builds:**
- Recognition rules as data: matching on layer, block, attribute, hatch, closed region, text
  and colour, with property extraction (width from an attribute, thickness from parallel
  lines, height from a section or a note).
- Legend and schedule reading: the drawing's own door schedule becomes element types.
- PDF drawings:
  - vector take-off through the existing `extract_vectors`, with a calibrated scale;
  - raster or scanned pages through Mizan's vision on pictures, then Qiyas's geometry to snap
    and measure.
- A confidence per recognised element, and a **review queue** in the window: accept, fix or
  reject. Each correction becomes a rule.
- PDF text reading for Mizan (`qiyas_read document`), with PDF attachments allowed in the
  composer.

**Done when:** on messy consultant drawings from the portfolio (an office, a warehouse, an
MEP layout, a PDF-only set), recognition finds at least 95% of the elements a senior QS
counts, and the rest are listed in review.

### Phase 18: Measure like a senior QS (E3, E4, E5)

**Builds:**
- Measurement rules written from the standard the user supplies. No standard is built in.
- Dimension sheets per item: traceable lines (`3 / 4.20 × 2.80 = 35.28 m²`, then the opening
  deduction), each line pointing to its handle, sheet and grid.
- BOQ mappings: descriptions from templates in English and Arabic, units, sections and
  trades.
- The generated BOQ follows every edit. Nothing is typed.

**Done when:** for each portfolio project, the generated BOQ matches the senior QS's within
±2% on 95% of items, and every line traces back to the drawing.

### Phase 19: Find every discrepancy (E6, E7)

**Builds:**
- Comparison of the client's BOQ against the drawings, using the existing qiyas-takeoff
  compare, extended to items mapped from elements.
- Checks as data: schedule vs plan, discipline vs discipline (an MEP penetration with no
  structural opening), spec vs drawing (a door type the spec doesn't define).
- A query list (RFIs) with clouds and references, in English and Arabic.

**Done when:** on seeded projects with planted errors (missing items, wrong quantities,
plan/schedule mismatches), at least 90% are found, with few false alarms.

### Phase 20: Revisions and variations (E8)

**Builds:**
- Comparing revision A with revision B: matched, added, removed and changed elements and
  geometry.
- Recognition rules and the take-off carried over to B.
- Quantity deltas per BOQ item, a variation quantity statement, and changes clouded on B.

**Done when:** a revised drawing set gives a correct variation statement in minutes,
verified against a hand-measured variation.

### Phase 21: Project scale (E9)

**Builds:**
- A project spanning many drawings (sheet set `.qss`): the BOQ aggregated across drawings and
  disciplines.
- Cross-drawing checks.
- A drawing register.
- Overnight batch runs through the existing `qiyas-cli batch`, with a morning report.

**Open question for Kareem:** knowledge lives inside each drawing, so where does project-level
data (the combined BOQ, cross-drawing checks) live? The proposal is the sheet set file
`.qss`.

### Phase 22: Deliverables (E10)

Report layouts as data: dimension sheets, the BOQ in the client's own Excel format (mapped
from their template), the query list and the variation statement. Excel and PDF, bilingual,
with Thmanyah for Arabic where licensed.

### Phase 23: Trust and the benchmark (E11)

**Builds:**
- "How did you get this number?" on any quantity: the dimension lines, the elements and the
  rule.
- Your view separate from Mizan's working sheet: browse freely during a run, replacing Phase
  13's refusal.
- The **QS benchmark:** real anonymised projects with senior QS take-offs as ground truth,
  scored automatically on the six measures above after every phase.
- The Claude vs Gemini comparison and choosing the model per task, each paid run with
  Kareem's OK.

### Later (design side)

Generated sections and elevations, formula sheets for engineering calculations, and design
authoring. Kept on the roadmap, behind the QS work.

## How the work is done

- One phase at a time, delegated to Codex (GPT-6 Astra, ultra) with a brief from this plan.
- Claude reviews and runs `scripts/check.sh` on the laptop (CARGO_BUILD_JOBS=2; clean stale
  test binaries when the disk is low), then commits. Ask before any push.
- A benchmark run after each phase from 16 on. Live model runs each need Kareem's OK.
- Docs: `docs/2026-10-21-qiyas-qs-roadmap.md` (this plan for the team) plus one doc per
  phase. CLAUDE.md updated each phase.

## Kareem's answers (21 Oct)

`~/.qiyas` is the **production** data home (users running Qiyas). Development data never goes there: research and benchmark data live in the repo's git-ignored `.local/` (`.local/research`, `.local/benchmark`), and debug builds use `.local/qiyas-home` as their Qiyas home.


1. **No real projects.** Claude runs deep research for public, legally usable benchmark
   material:
   - government tender packages with drawings and BOQ quantities;
   - open CAD and floor-plan datasets with labelled elements;
   - sample building models.

   From it Claude builds the QS benchmark:
   - each project's ground truth is checked by hand;
   - the files are stored outside the public repo, with their licences recorded;
   - only openly licensed material goes anywhere shared.
2. **The method of measurement is set in Mizan's Settings.**
   - A "Method of measurement" choice: POMI, NRM2, SMM7, CESMM4, local practice, or a
     custom one.
   - Its rule set is data that Mizan writes from the standard's document (attached in
     Settings) or from research notes. Copyrighted standards are never copied as text; their
     rules are written in Qiyas's own words, with clause references.
   - Rule sets are stored in `~/.qiyas/methods/<name>.json`.
   - The method chosen in Settings applies to every job. A project can override it in its
     `.qss`.
   - Every quantity Mizan produces names the method and the clause it followed.
3. **Project data lives in `~/.qiyas` as `.qss` files.**
   - Format: `~/.qiyas/projects/<project>.qss`, a sheet set extended with the project's
     drawings (absolute paths allowed here), its combined BOQ, its cross-drawing checks and
     its method override.
   - Today `sheetset.rs` refuses paths outside the `.qss` folder. That rule stays for sheet
     sets saved beside drawings, and is relaxed for projects in `~/.qiyas`.
   - Knowledge about a single drawing stays inside that drawing.
   - **Where:** `~/.qiyas` is home-level, next to the app's config folder (`settings.json`,
     logs, conversations), which is unchanged.

## Changes to the phases from these answers
- **Now:** add the Method of measurement setting (with the approval modes) to Phase 14's
  Settings work. Its rule engine arrives in Phase 18.
- **Phase 18:** measurement rules come from the method chosen in Settings (or the project's
  override). A test proves that switching methods changes the deduction and unit rules and
  re-derives the quantities.
- **Phase 21:** projects in `~/.qiyas/projects/*.qss`, listed in a Projects panel. Overnight
  batch runs work per project.
- **Phase 15:** the benchmark is built first from the researched open sources plus Qiyas's own generator.
