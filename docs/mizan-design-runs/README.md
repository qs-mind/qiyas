# Mizan design runs: what went wrong and how it was fixed

From 2 to 3 November 2026, Mizan was given the same brief six times: design a small single-storey office,
in full detail, on named sheets with title blocks. Every run was studied call by call, the causes of
each failure were fixed in Qiyas, and the next run checked the fixes. This page records what was wrong,
what changed, the methods used and the results.

## The result

Same brief, same model (gpt-6-astra on the ChatGPT plan), a fresh drawing in metres each time.

| Run | Ended | AI requests | Tool calls | Failed calls | Time | Own blocks | Block inserts | Sheets | PDFs |
|---|---|---|---|---|---|---|---|---|---|
| 1 | Asked its questions | 2 / 80 | 2 | 0 | 1 min | 0 | 0 | 0 | 0 |
| 2 | Finished | 78 / 80 | 214 | 19 (8.9%) | 27 min | 14 | 62 | 3 | 4 |
| 3 | Cut off by the AI service | 36 / 80 | 74 | 6 (8.1%) | 12 min | 13 | 47 | 0 finished | 0 |
| 4 | Used every request | 80 / 80 | 155 | 10 (6.5%) | 33 min | 16 | 69 | 4 | 4 |
| 5 | Used every request | 80 / 80 | 148 | 4 (2.7%) | 25 min | 18 | 77 | 3 | 3 |
| 6 | Finished | 100 / 100 | 287 | 5 (1.7%) | 31 min | 18 | 53 | 3 | 3 + a take-off workbook |

Run 1 stopped correctly: Mizan asked about the missing brief facts (team size, rooms) instead of
inventing them, and the answers were added to the brief for every later run.

Run 6 is the first run that finished the whole job on its own. It drew a coloured 1:50 plan with six
equipped desks, a six-seat meeting room, a fitted kitchenette, an accessible-style WC with its turning
circle, four doors with swings, eight windows, room areas, dimensions and schedules; a schedules sheet;
and kitchen, workstation and wall-section details at 1:25, each sheet with a title block and a scale bar.
It also wrote a draft take-off workbook with 31 quantity rows.

Before any of this work, the first test session in the app sent an average of about 115,000 input
tokens per request (267,000 at the peak) and drew about 200 loose lines with no reusable components.
Runs 2 to 6 averaged 42,000 to 48,000.

## Why Mizan and Qiyas failed

1. **Too many instructions and too much fixed content.** Mizan carried long prompts, shipped symbol
   and furniture packs, drafting-standard files, a code register and an "expert frame" in its plan.
   It spent its effort following them rather than designing, and every request was large and slow.
2. **Loose lines instead of components.** A desk or a door was a pile of separate lines, drawn again
   wherever it appeared. Detail stayed poor and every change was expensive.
3. **Qiyas refused inputs whose meaning was plain.** `name` for `block_name`, a decision without
   `set_by`, a loop variable named `x`, a range without a step, a text edited through `set_records`.
   Every refusal cost a whole AI request, and many errors said neither where the mistake was nor what
   to send instead, so Mizan guessed again.
4. **A cut-off answer ended the run.** When the AI service stopped an answer part way, the job
   stopped with it (run 3, at request 36).
5. **Sheets came out badly.** The drawing sat on a third of the paper and notes overlapped.
6. **Bugs.** Arabic text broke the JSON Qiyas saves inside drawings (the code page turned characters
   into `\U+XXXX`); undoing a batch did not restore the current layer; a run that used its whole
   request allowance was reported as a provider failure.

## What changed

### Less instruction, no fixed content

- The prompts (`crates/qiyas-ai/src/prompt_en.txt`, `prompt_ar.txt`) are short: draw first, build
  components as blocks, check each part after drawing it.
- The shipped packs and data files were removed: `interiors-pack.json`, `drafting-standards.json`,
  `code-register.json`, the built-in drafting symbols and the 5% comparison tolerance. Without a
  tolerance from the method, the job or the engineer, comparisons are exact.
- The expert frame was taken out of the plan panel.
- Ruling: Qiyas ships no ready-made libraries, symbol sets or design data. Mizan draws each component
  at real size and in full detail as a block in the drawing, and reuses or improves it there. Nothing
  is kept on the computer to make Mizan work better, so a fresh install anywhere works the same.
- `drawing_kit.txt` gives the complete forms of the common commands within a fixed byte budget, so
  Mizan copies a working shape instead of discovering it one schema at a time.

### Components and recipes

- Mizan defines each component once (`define_block`) and inserts it where needed. Wall openings use
  Mizan's own door or window block as their symbol.
- `generate` (`crates/qiyas-core/src/generate.rs`) runs a data-only geometry recipe: loops, variables
  and expressions expand into ordinary commands in one undo step. One recipe draws a row of desks
  with chairs, a column grid, stair treads or seats along an arc. Recipes run inside batches.

### Qiyas accepts what Mizan means

The input layer (`crates/qiyas-core/src/input_aliases.rs`) turns natural forms into the exact command
before checking it, and every value it fills in comes back in `defaults_used`, so nothing is silent.
Accepted now:

- records filters with `field`, `block` or `name` for `block_name`, and a single `{name: value}` filter;
- a text search `pattern`, a case-insensitive regular expression;
- a decision with `action` for `kind`, a lone entry beside `kind` without `entries`, no `set_by`
  (Mizan's), and no `unit` for a value that is not a number;
- `generate` loops named `x`, `y` or `angle` unless a loop runs along a path, and ranges without a step
  (one, towards the end);
- `mview` with paper corners and a model window together (the window's middle is the centre and, with
  no scale, the largest scale that shows it); an `mview` aimed at a sheet's overall viewport changes
  its one model viewport, or makes one;
- an edit whose targets are all on another sheet runs on that sheet without switching the view;
- `set_records` of a plain text's `/value` runs as `edit_text` in the same undo step;
- `dim_edit` with `text_position`; tables with row data; name-only `set_layer`; and the earlier
  shorthand listed in `CLAUDE.md` under "Input shorthand".

When Qiyas must refuse, the error now does the work for Mizan:

- broken JSON is answered with the text where it broke (`…[[0,0]],⟨here⟩[9,9]…`);
- a wrong field names the field, the allowed ones and a filled example;
- a sheet that already exists names its viewports and the exact `mview` to change it;
- writing into Qiyas's own marks points to `set_knowledge` for Mizan's own data;
- a miscopied artifact id that is part of exactly one saved id still finds it.

Tool results share one envelope (`crates/qiyas-ai/src/result_envelope.rs`); a read of a missing part
of saved evidence is a soft `found: false` with the nearest paths, not an error. Replaced element
handles are followed across commands (`handle_refs.rs`), and one sheet lookup (`Drawing::resolve_sheet`)
serves commands, reads, captures and plotting.

### Runs keep going

- A cut-off answer is kept, retried and continued (`crates/qiyas-ai/src/stream_recovery.rs`) instead
  of ending the job.
- AI provider failures are handled the same way for every provider, and picture support is detected
  for each one.
- A run that uses its request allowance reports `stopped: request limit reached (N of N requests used)`.

### Sheets

- `make_sheet` and `arrange` fit the drawing at the largest standard scale that fits the paper, with
  zones for plans, notes, schedules and the title block. Some paper always stays free: standard
  scales come in steps (1:50 to 1:25 doubles the size), so a drawing cannot always fill A1.
- The drawing check finds text over text and text over lines; overlapping notes are separated with
  leaders in one undo step.

### Bugs fixed

- Saved JSON is ASCII-only (`xdata::ascii_json`), with a literal `\U+` protected, so Arabic survives
  DWG 2000 and 2004 code pages; old files still read.
- A quadratic scan in room labelling, introduced during this work, was caught by the check before
  merging (100 times slower on large drawings) and removed.
- Undoing a batch restores the current layer: a group's own view change is undone before its steps.
- The `opening_schedule` `height` alias no longer declares a default of its own.

## Methods

### The test

`qiyas-cli design-run` runs the production Mizan agent headless on a new drawing and writes
everything needed to judge the run:

```sh
cargo run -p qiyas-cli -- design-run --brief office-brief.txt --out runs/office-6 --max-requests 100
cargo run -p qiyas-cli -- design-compare runs/office-5 runs/office-6
```

Each run folder holds `report.md` and `report.json` (status, requests, token use, every tool call and
failed call, entities, blocks and inserts, sheets, the final answer), `steps.jsonl` (every request and
result), `agent.log`, `drawing.dwg`, one PNG per sheet and the PDFs and workbooks Mizan exported.
Starting a design run starts real AI requests; it is never part of CI or the tests.

### Reading a run

After each run every failed call was pulled from `steps.jsonl` with its exact input and error. Each
failure was put in one of three groups:

1. Mizan meant one clear thing: Qiyas now accepts it and reports what it filled in.
2. The meaning was not clear: Qiyas still refuses, and the error says exactly what to send.
3. A Qiyas bug: fixed.

Each fix got a test built from the run's own input, reduced to what it needs:
`crates/qiyas-core/tests/office_run_four.rs`, `office_run_five.rs`, and the cases in
`tests/validation.rs`, `crates/qiyas-ai/tests/surface.rs` and `src/artifact.rs`.

### Building and testing

- All the code for a round is written first (in parallel worktrees when several agents share the
  work), then merged, built once and tested once, and every failure is fixed in one batch, then one
  final check. There is no build-and-test loop after each small change.
- The check uses the whole machine: every core for the build, the window checks beside the Rust
  tests, then the release speed tests. It runs formatting, clippy, generated window types, the
  workspace tests, the PDF tests, four speed guards, the TypeScript check and vitest. On a 16-core
  machine it takes about 10 minutes.
- Fuzz and mutation testing run one worker per core (`fuzz/README.md`).

## Still open

Each run still shows a few new slips, because Mizan writes free-form input. Run 6 had five, and it
recovered from each one and finished:

- a `set_layer` with a DASHED linetype the drawing does not have;
- a batch step reference that pointed at a later step;
- polyline vertex paths written as `/data/vertices/...` instead of `/vertices/...`;
- another write into Qiyas's own viewport marks;
- marking BOQ items before any were generated.

## Commits

| Commit | Change |
|---|---|
| `84e54ec` | Expert frame out of the plan panel |
| `d78b82d`, `a627850` | Rulings: no static content; components live in the drawing |
| `a7c00eb` | Leaner Mizan: drawing first, components as blocks |
| `2766180` | Shipped design data, built-in symbols and the 5% tolerance removed |
| `6b47bff` | Computed geometry recipes (`generate`) |
| `a1b965d` | `qiyas-cli design-run` and `design-compare` |
| `d1bb384` | Commands accept Mizan's natural inputs and fill safe defaults |
| `02a661f` | One result envelope; forgiving evidence reads |
| `b6c020d` | Recipes run inside batches; tables scale and rotate |
| `c29bbe3` | Replaced element ids are followed; one sheet resolver |
| `3169777` | Sheets fill their paper; overlapping notes separated |
| `4eca902` | Cut-off answers kept, retried and continued |
| `6b46426` | Nested batches, record data paths, skipped unknown check rules |
| `ba9eb8f` | Run 4's ten refused inputs, the request-cap status, batch undo |
| `bf60d38` | Run 5's four refused inputs; build and fuzz with the whole machine |
