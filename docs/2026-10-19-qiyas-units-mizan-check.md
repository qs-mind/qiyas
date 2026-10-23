# Check: new drawings, Mizan's file and edit tools, drawing correct in every unit (2026-10-19)

## Why

Kareem talked to Mizan with no drawing open: it could not make or open one, and Qiyas itself had
no New. Then, testing a new drawing in metres, a 1 m circle drew as a 10-gon, and Mizan made up
an excuse ("VIEWRES"). A later session (`agent.log`, the villa `Untitled-1.dwg`) showed
dimensions metres tall, labels drawn twice over, layer 0 hidden to cover mistakes, and table text
running out of its rows. His ruling: Qiyas works at AutoCAD level, and an issue like this never
happens again.

## Root causes

| What was seen | Cause |
|---|---|
| A 1 m circle as a 10-gon in metres | The screen's curve tolerance `BATCH_TOL = 0.05` was in drawing units: 0.05 mm in millimetres, 5 cm in metres. Only millimetre drawings had been tested. |
| Dimensions 18 m tall, extension lines 125 m, giant arrowheads | The drawing's dimension style was the codec's stock one (text 0.18, offset 0.625, extension 1.25, no unit); the annotation scale 1:100 multiplied it. |
| Labels drawn twice, layer 0 hidden, dimensions faked with lines and text | Mizan's picture (view_image) drew line segments only: no text, no fills. It could not see its labels, had no tool to change them, and worked round the giant dimensions. |
| Table text over the next row | Rows kept their height when a cell's text wrapped. |
| "VIEWRES" | The prompt did not forbid explaining what no tool result shows. |

## What changed

- New drawing: `Session::new_blank` (metric template at 1:100), IPC `new_drawing`, New on the
  quick-access bar, Ctrl+N, NEW/QNEW; Save goes through Save As until the first one; an untitled
  drawing's crash autosave is offered at launch (`untitled_autosaves`).
- Mizan and MCP: `list_drawings`, `new_drawing`, `open_drawing`, `switch_drawing`,
  `save_drawing`, `close_drawing` through `DrawingHost`; `modify`, `edit`, `erase`, `set_text`,
  `undo_last`. The user keeps every choice that can lose work or pick a file.
- Units: `batch_tol` (0.05 mm), `measure_tol` (1 µm), zoom regen (`zoom_tol`, a quarter pixel),
  16 segments a circle at least; block INSERT scaled by block units; AUTOCONSTRAIN, grid,
  multiline, AI hatch, table text, image width and text height defaults in real units; a stock
  dimension style sized on open (`size_stock_dim_styles`); edit tolerances that grow with survey
  coordinates.
- Mizan sees: `picture_batch` + `render_png_with_text` draw text glyphs and solid fills.
- Prompt: no made-up explanations, no LaTeX (and `plainMath` in the panel), fix what you drew
  instead of drawing it again, never hide layers to hide a mistake, dimensions only with
  add_dimension. Fixed text stays under 800 words.
- Tables: rows grow to hold wrapped text (`fit_rows`).
- Window: New chat button, model name gone from the panel, origin (UCS) icon with UCSICON and a
  View toggle.

## Evidence

- `crates/qiyas-core/tests/units.rs`: one plan in mm, cm, m and inches draws the same segments,
  measures the same areas, picks, snaps, selects by window and hatches by click the same; a block
  from a millimetre library comes in 0.9 m wide; AUTOCONSTRAIN does not weld ends 4 cm apart in
  metres; the grid is a metre; a stock-style drawing's dimension is 18 m before and 0.25 m after
  opening again; JOIN works at a 3.3e9 mm coordinate.
- `crates/qiyas-core/tests/blank.rs`: new drawings, circles as round in every unit, zoom regen,
  readable dimensions in m, cm and mm.
- `crates/qiyas-ai/tests/tools.rs` `view_image_shows_text_and_solid_fills`; `authoring.rs` for
  the edit tools, `set_text` and the AI hatch scale; `agent.rs` for a new drawing made and drawn
  on in one turn.
- The villa drawing, opened again: dimensions 0.25 m, no giant arrows or lines
  (`cargo run -p qiyas-plot --example plot_file`, `cargo run -p qiyas-ai --example inspect`).
- DWG corpus (`docs/dwg-corpus-results-2026-10-19.json`): all nine files pass; 22,672 segments
  (11,767 before: circles under the tolerance now get 16 segments, not as few as 4); first
  picture 13 to 19 ms a file. A floor of 64 was tried first and made the corpus 6.6 times
  heavier, so it was lowered: zoom regen rounds what is zoomed into.
- `scripts/check.sh` steps, all passing: fmt, clippy, `cargo test --workspace`, the three
  release speed guards, typecheck, vitest (98 files, 745 tests). Run one at a time on one or two
  cores (the laptop overheats and runs out of memory otherwise).

## Not covered

- Mizan's behaviour with the real model (Gemini 3.8 Flash) is not tested here: the tools, the
  picture and the rules are; whether the model uses them well needs Kareem's live testing.
- Duplicate labels already in the villa drawing stay until someone erases them.
- Rows grow; columns do not resize to their content.
