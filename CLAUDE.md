# Qiyas

Qiyas (قياس) is a desktop measurement CAD for quantity surveyors, part of QS Mind. It opens DWG and DXF drawings, lets the QS measure and edit, and uses AI to build and check BOQs against the drawing. Public repo (`qs-mind/qiyas`, Apache-2.0, `LICENSE`). Stack: Tauri 2, React, TypeScript, Vite, Rust.

## Layout

```
qiyas/
  crates/
    qiyas-core      the document: opencadcodec CadDocument in memory, layers, blocks,
                    layouts, xrefs, selection, snaps, units, undo and redo, command bus, draws
    qiyas-geom      2D geometry on top of opencadkernel: offset, trim, fillet, area,
                    length, closed regions, intersections
    qiyas-render    turns the document into GPU buffers: lines, fills, text glyphs,
                    hatches, images; spatial index; dirty tracking per handle
    qiyas-param     parametric constraints: points, radii, curves and their constraints, the
                    damped Gauss-Newton solver (nalgebra), the expression parser
    qiyas-takeoff   the comparison engine: take-off and drawings against the BOQ
                    (conflicts); the BOQ, schedule and conflict types live in qiyas-core
    qiyas-boq       Excel in and out (calamine to read, rust_xlsxwriter to write)
    qiyas-pdf       PDF import as vectors and text, PDF underlay pages
    qiyas-plot      plot a sheet to PDF, plot styles (CTB/STB), print through the OS
    qiyas-ai        the 57 AI tools over the command bus, the agent loop, the Claude and
                    OpenAI-compatible clients, the MCP server
    qiyas-mcp       the stdio bridge Claude Desktop and Claude Code start (bundled beside the app)
    qiyas-cli       the command line without a window: convert, run a script, take off, batch
    qiyas-desktop   the Tauri app: IPC, file dialogs, settings, keychain, assistant, MCP switch
  src/              React + TypeScript + Vite: viewport, ribbon, command line, panels; `src/ui/` the UI kit
  scripts/check.sh  cargo fmt, clippy, test, a release speed test, tsc, vitest, in one run
```

All the crates above exist; `qiyas-pdf` reads PDFs (import, underlays), `qiyas-plot` writes them.

Pinned libraries (git revs in the workspace `Cargo.toml`): opencadcodec `a35f43e` (DWG/DXF) and
opencadkernel `99b6b48` (`geom2d` for snaps and edits, `offset` for polyline offsets: it brings
cavalier_contours, MIT OR Apache-2.0). Both MPL-2.0. Licence texts of code we ship
live in `LICENSES/` (opencadcodec, opencadkernel, pdfium, nalgebra's Apache-2.0, foldhash's Zlib,
clipboard-win's and error-code's BSL-1.0); add one there for every new non-MIT library. Font licences sit beside the fonts in `crates/qiyas-render/fonts/`.

Samples: `samples/test-plan.dwg` (written by `cargo run -p qiyas-core --example write_test_plan`) and `samples/test-boq.xlsx` (written by `cargo run -p qiyas-boq --example write_test_boq`); the exact quantities and the two conflicts the BOQ must give are in `samples/test-plan.md`. `cargo run -p qiyas-ai --example phase2_walkthrough` measures, imports, links and compares them through the AI tools, no model needed.

Frontend layout: `src/main.tsx` renders `src/app/App.tsx`, the composition root (`src/App.tsx`
re-exports it). It calls the hooks in `src/app/` in this order, which keeps their effects in the
order they always ran: `useShell`, `useOptions`, `useFileTabs`, `useAutosave`, `useDrafting`,
`useTakeoffState`, `useSheets`, `useLayers`, `useToolChoice`, `useAttach`, `useBlocks`,
`useAnnotate`, `useParametric`, `useCommands`, `useTool`, `useAssistantWiring`, `useDrawings`, `useTakeoff`,
`usePlot`, `useCommandLine`, `useSelection`, `useOverlay`. It hands `apply`, `onCommand` and
the tool's `cancel` to hooks made before them through refs filled right after they exist, and
renders `Header`, `Dialogs` and `Panels`. Dialogs and panels themselves are in `src/shell/`, the canvas
in `src/viewport/`, the tools in `src/tools/`; `src/app/App.test.tsx` checks the wiring. The
three late refs are `measureRef` (`useSheets` cancels the tool when the sheet changes), `applyRef`
(`useBlocks`, `useAnnotate`) and `onCommandRef` (the tool palette in `useBlocks`); they are called
with `?.`, so a missing fill still type-checks, and `App.test.tsx` has a test for each.

Mizan's waiting (the "Working…" line, a running tool card, the busy assistant tab) is the Qiyas mark
itself, `src/ai/MizanMark.tsx`: the brand drawing, its mesh drawn line by line round the centre, then
wiped and drawn again (CSS, still and drawn under reduced motion). Waiting is shown by `src/shell/DotLoader.tsx` (clean-room, our own design: do not copy code from any
dot-matrix loader library, whose licence forbids it): `<DotLoader pattern="scan|spiral|pulse|chase"
size="sm|md" label? />`, 3x3 (`sm`, one 12 px status-bar line) or 5x5 (`md`), pure CSS in `theme.css`
(`.dot-loader`, one keyframe, a per-dot `--p` sets the delay; no timer), `--accent` dots, grid
follows the text direction, still under `prefers-reduced-motion`. Without `label` it is `aria-hidden`
and the text beside it (kept) speaks; with one it is a named `img`, never a live region. The status bar
gets `waiting` from `useCommandLine` (opening, comparing); no `.spinner` is left. A wait that reports a
percentage is a bar instead (see the UI kit below).

The UI kit (`src/ui/` and the parts it feeds): MIT headless libraries, styled with plain CSS from the
brand tokens in `theme.css` (no Tailwind, no shadcn, no `motion`; CSS transitions only, none under
`prefers-reduced-motion`). `LanguageProvider` hands Base UI the direction (`DirectionProvider`), so
arrows and popups mirror in Arabic. `src/test/setup.ts` (vitest `setupFiles`) stubs `ResizeObserver`,
`matchMedia` and `scrollIntoView` for jsdom.
- `@base-ui/react`: Tabs (the ribbon's and the side panel's: one `Tabs.Indicator`, moved by a CSS
  transform, is the chosen tab's face; `SideTabs.tsx` never wraps and folds the last tabs into a "⋯"
  menu when the panel is too narrow, like the ribbon's "More"); Tooltip (`shell/Tooltip.tsx`: the
  `Tip` wrapper keeps its API, the delay is 400 ms, the next card shows at once within 300 ms, and a
  click's focus shows none); Menu (`shell/RibbonFlyout.tsx`: a group's "⋯" and the tabs' "More", where
  the "More" tab is a `Tabs.Tab` that is also the `Menu.Trigger`); Context Menu (`viewport/CanvasMenu.tsx`:
  Move, Copy, Erase, Properties, Zoom extents on a right-click of the drawing); Popover (the status
  bar's list of recovered problems); Dialog (`ui/ModalDialog.tsx`, behind the Settings, Plot, Purge,
  Audit and Recovery dialogs: Escape closes unless `dismissible` is false, a click outside does not,
  `open` false hides the Plot dialog while a window is clicked); Toggle Group and Radio Group
  (`ui/Segmented.tsx`: the status bar's OSNAP / ORTHO / POLAR / GRID, the PDF attach mode and the
  conflicts' status filter); Progress (`ui/Progress.tsx`). Base UI moves focus and closes popups a
  tick later, so tests wait (`act`, or fake timers advanced) before they look.
- `@tanstack/react-table` (v9: `tableFeatures`, `useTable`) and `@tanstack/react-virtual`: `ui/DataGrid.tsx`
  is the take-off, BOQ and conflicts tables: sorting (click, again, again; numbers largest first, text
  in natural order, nothing last), resizing (drag the edge, arrows on it, double click to reset; the
  sizes are kept per grid in `localStorage` `qiyas.grid.<name>`), a sticky header, virtualised rows (a
  box with no layout keeps its initial size), sections (`section`, rows sorted inside them), the one
  `grow` column takes the room left over, every cell's full text is its tooltip, the arrows, Home, End
  and Enter work on rows, and the row buttons (`actions`) show on hover or focus. Row heights are
  fixed (27 px; the conflicts' reason wraps to two lines at 40). `--row-hover` and `--row-current`
  are solid colours, because the row buttons sit on them.
- `@headless-tree/react`: the layers panel is a tree (`LayersPanel.tsx`, `layerGroups.ts`): a drawing
  with 12 layers or more is grouped by discipline prefix (`A-`, `S-`, `E-`) and by xref (`PLAN|…`), a
  prefix or xref with one layer stays at the top. The library's movement keys (arrows, Home, End) are
  off while the focus is in a layer's properties (`isEnabled`), so a drop-down keeps its arrows; a
  click selects, Enter or a double click makes a layer current, the folded groups are kept in
  `qiyas.layers.collapsed`. Eye (and the current layer's pencil) stay on the row; isolate, make
  current and properties show over the end of the name on hover or focus.
- `cmdk`: the command line's suggestions (`CommandLine.tsx`). The chosen row is the line's own
  (`active`), not cmdk's: nothing is chosen until the arrows choose, so Enter runs what was typed; a
  hidden first row takes cmdk's start. A command shows its shortest alias as a `<kbd>`; an alias whose
  command does not itself start with what was typed is its own row; headings only when both show.
- `sonner`: notices (`ui/notices.tsx`, the one `<Notices>` in `App.tsx`): `setNotice` is a toast, one at
  a time (a new one takes the last one's place; `null`, or the next command, takes it away), `waiting`
  keeps it with the dots until replaced; `setError` is a toast that stays until it is closed or `null`
  (the next success) clears it; both are on `useShell`. At the bottom end (bottom left in Arabic), above
  the status bar. The status bar says status only. `notifyUndo` is a toast with a counting-down Undo
  button (the Purge dialog offers it, calling the app's `history(undo)`, once it closes: the modal
  dialog would make the toast inert); any other command (`run`) takes it away.
- Mizan's thread (`ai/useStickToBottom.ts`): follows the newest message while the reader is at the
  bottom; scrolled up it leaves them and offers "Jump to latest". There is no propose-and-approve flow
  for BOQ changes (the tools apply and each card can be undone), so there are no suggestion cards.
- The side panels' widths and folds are our own handle (`app/panelRails.tsx`: drag, double click, arrows;
  kept on this computer), not `react-resizable-panels`.
- Dependencies (all MIT; `tslib`, 0BSD, comes with them, text in `LICENSES/tslib-0BSD.txt`):
  `@base-ui/react`, `@tanstack/react-table`, `@tanstack/react-virtual` (3.14.13: the newest that passes
  pnpm's release-age gate), `@headless-tree/core` and `/react`, `cmdk`, `sonner`, and
  `react-markdown` with `remark-gfm` (Mizan's answers as Markdown, `ai/Markdown.tsx`; licences in
  `LICENSES/THIRD-PARTY.md`).

## Global constraints

- Desktop only, no web build. Local check is `scripts/check.sh`. CI on GitHub
  (`.github/workflows/ci.yml`, on every pull request and push to `main`): `check` runs
  `scripts/pdfium.sh` and `scripts/check.sh` on Ubuntu (required to merge into `main`;
  branch protection, admins may push directly); `build-smoke` compiles the app in release with
  the sidecar (`pnpm tauri build --no-bundle`) on Ubuntu, Windows and macOS; `dependency-review`
  is advisory on pull requests. Branch protection requires the status `check`: keep that job
  id. Pull requests cancel their older runs; runs on `main` always finish. Only `main` saves the
  Rust cache (pull requests read it; release runs never save). Every action is pinned to a
  commit SHA with its release in a comment, every checkout has `persist-credentials: false`, and
  Dependabot (`.github/dependabot.yml`) bumps cargo, npm and the actions weekly. Releases:
  `.github/workflows/release.yml` (see Updates and installers).
- Window title and product name: `Qiyas`. Arabic name: `قياس`. Bundle identifier: `com.qsmind.qiyas`.
- This repo is its own git repo. Commit inside it. Pushing `main` and pushing a `v*` tag are the founder's acts or his explicit ask; CI builds the draft release, and only a human clicks Publish.
- Only MPL-2.0, Apache-2.0, MIT, BSD, ISC, OFL, Zlib or BSL-1.0 dependencies (ISC: `ring`,
  `rustls-webpki`, `untrusted`; 0BSD, a BSD: `tslib`; Zlib: `foldhash`; BSL-1.0: `clipboard-win`, `error-code` on
  Windows, through `arboard`, the system clipboard, MIT OR Apache-2.0, with its Wayland
  data-control feature). No code copied from Open CAD Studio (GPL).
- Thmanyah font files are never committed. `scripts/fonts.sh` copies them from `~/Downloads/Thmanyah-Font-Family/thmanyah typeface` into the git-ignored `src/fonts/thmanyah/`. Instrument Sans (OFL) is committed. The viewport draws drawing text with Noto Sans and Noto Sans Arabic (OFL, unhinted Regular, with their licences in `crates/qiyas-render/fonts/`), behind the drawing's own TrueType font when the computer has it.
- English text says "Qiyas" and "QS Mind"; Arabic text says "قياس" and "كيو إس مايند". Arabic uses Thmanyah Sans; headings use `font-feature-settings: "swsh"`.
- Measured values are stored in drawing units and displayed in metres, square metres and pieces. Unit factor comes from the drawing header (`insertion_units`): 1 = inches, 2 = feet, 4 = mm, 5 = cm, 6 = m, 0 or unknown = treat as mm and show a warning in the status bar.
- Every mutating command is undoable. Undo and redo stacks live in `qiyas-core`.
- Brand colours: Ink `#0E0F12`, Stone `#F2EEE7`, Paper `#F9F8F4`, Slate `#6E6A64`, Qiyas face anodised titanium `#C9A6E0` to `#6E4A9E`.

## Working here

- Use pnpm, not npm.
- Run `scripts/check.sh` before every commit.
- `qiyas-render` builds with one codegen unit (`Cargo.toml`): split, the pick index builds about
  4 times slower. `cargo test -p qiyas-render --release pick_index_one_million` guards it.
- Bound-xref layer names (`XREF$0$A-WALL`) are shown as `A-WALL` (`short_layer_name`); the
  full name stays in `layer`.
- Pushing `main` and pushing a `v*` tag are the founder's acts or his explicit ask; CI builds the draft release, and only a human clicks Publish.
- Run the app: `pnpm tauri dev --config crates/qiyas-desktop/tauri.conf.json`.
- The `.qiyas` project file is saved next to the drawing after every command.
- Save keeps a `.bak` of the original drawing (e.g. `villa.dwg.bak`) before writing over it. The
  file leaves out anonymous `*Dn`/`*Tn` blocks nothing uses any more and erased group members
  (`purge_for_save`); the drawing in memory keeps them, so an undo after the save finds them.
- Autosave writes `<file>.sv$` and `<file>.sv$.qiyas` (`plan.dwg.sv$`) beside a drawing with
  unsaved changes (see Clipboard, viewports, UCS, sheet sets and recovery); a save, or closing it
  with Don't Save, deletes them.

## Undo

- `qiyas-core/src/undo.rs`: one step per command, at most 200, each with its `Source`
  (`undo_if` lets the AI undo only its own). Take-off, BOQ and conflict steps keep that part whole.
- A drawing command (`Part::Document` in `Command::changes`) runs inside
  `Session::record_document_change`: opencadcodec's entity change recorder keeps the before image
  of each entity it adds, removes or edits (`DocChange`), never a copy of the whole drawing.
  Commands that change layers, views, block records or underlay and image definitions also keep a
  `TablesSnapshot`; divide and measure along keep the take-off too. A failed command is put back.
- Undo removes added entities, restores removed ones under their own handles (a removal leaves
  the handle in its block's list, so `restore_entity_arc` puts it back in place) and swaps edited
  ones back. Every drawing step, undo and redo moves `document_revision` on (batch version, dirty
  flag, tabs).
- `Command::DeleteEntities { targets }`: each `Target { handle, chain }` as picked (`Picked`). Only
  the drawing's own entities at the top of the sheet on screen (empty chain, owned by the sheet);
  a block's inside or an xref's (non-empty chain) or another sheet's are refused, and then nothing
  is deleted.

## Drawing on screen

- Picking reaches inside blocks: the first click picks the line inside the block, a slow second
  click the whole block, a double click or Enter measures it. Measurements keep the block chain.
- Drawing text is drawn as real glyphs (TEXT, MTEXT, attributes) with Noto Sans and Noto Sans
  Arabic, or the drawing's own TrueType font when the computer has it; SHX falls back to Noto.
  Arabic is shaped and joined. View tab, "Arabic text is reversed" fixes drawings whose Arabic
  was stored backwards (saved in the project file).
- Snaps (end, mid, centre, node, intersection, nearest, perpendicular), ortho, polar and the dot
  grid: status bar toggles, F3 / F8 / F10 / F7, settings under the gear. Saved per drawing in the
  project file.
- The UCS icon (`src/viewport/ucsIcon.ts`, an SVG over the canvas moved on every frame without a
  React render): an L of X and Y arrows on the UCS origin (0,0 without a UCS), turned with its x
  axis, a small square marking the point; when the whole icon would not fit on screen it parks
  bottom left (AutoCAD's UCSICON ON, ORigin). Drawn in the canvas's ink or paper at 70%, never
  mirrored in Arabic. UCSICON ON / OFF (bare flips it) and View, Display, "Origin icon"; kept on
  this computer in `localStorage` `qiyas.ucsIcon`, on by default; the split view follows.
- Xrefs are read-only: loaded from the stored path, beside the drawing, `xrefs/` or `x-refs/`
  (depth 4); View tab, External references to reload or locate one (kept in the project file).
- Command line (Ctrl+9 shows or hides it): AutoCAD-style words and aliases (DI, AA, COUNT, DIV,
  ME, ID, Z E/W/IN/OUT, LAYOFF, U…), typed points `x,y`, `@dx,dy`, `@d<angle` in drawing units,
  Arabic digits accepted. Extra aliases: `aliases` in `settings.json`, edited in ALIASEDIT. Typing
  a command lists the matches in a cmdk list above the line (see the UI kit).
- Properties palette under Layers: what the Select tool picked, in AutoCAD's folding sections
  (`PropertiesPanel.tsx`, `propertiesGeometry.ts`; folded ones kept in `qiyas.properties.folded`):
  General (handle, type, layer, colour as drawn, linetype, weight, sheet, block path, xref),
  Geometry (read-only rows from `EntityInfo`, in metres, m² and degrees with the drafting
  precision; a click copies the number without its unit; the values that were editable stay
  fields: radius, closed, width, text, height, rotation, scale, viewport scale and centre) and
  Attributes. Per kind: line start, end, delta, length, angle; polyline vertices, closed,
  length or perimeter, area, width; arc centre, radius, start, end and total angle, length,
  chord; circle centre, radius, diameter, circumference, area; ellipse centre, length, area;
  spline length; hatch pattern, area; text and MText insertion, height, rotation, contents,
  style; block reference name, insertion, scale, rotation; dimension measured, text, height,
  style. Two or more picked (the select tool's whole selection set, `Measure.picks`, each read
  with `entityInfo`: `useSelection.pickedMany`) show a Selection section (count, kinds, total
  length and area, summed in drawing units; dimensions and block references are left out of the
  sums, but a hatch and its boundary both count) and the General fields they share ("Varies"
  otherwise), read-only. A drawing's own entity is edited there (see Layers, groups and
  properties).
- Units (Kareem's ruling: AutoCAD level in every unit; `qiyas-core/tests/units.rs` runs one plan in
  mm, cm, m and inches and must stay green). Nothing is a bare length in drawing units:
  - The screen draws curves within 0.05 mm whatever the units (`Session::batch_tol`,
    `batch_tol_for`); a metre drawing once drew a 1 m circle as a 10-gon. Every circle gets 16
    segments at least (`qiyas-geom` `MAX_STEP`; 64 made the DWG corpus 6.6 times heavier, and
    the zoom regen below already rounds what is zoomed into), and an arc of exactly whole steps
    gains none from rounding (a hatch traced round a circle would leave slivers). The DWG corpus
    gives 22,672 segments (`docs/dwg-corpus.md`).
  - Zoomed in past that, the sheet on screen draws its curves again within a quarter pixel
    (`ZOOM_TOL_PX`, `zoom_tol`, `Session::screen_tol`; part of `batch_version`), and lets them go
    zoomed out: the batch carries `curve_tol` and `base_tol` (atlas section, 40 bytes) and the
    viewport's `checkGlyphs` asks through the glyph zoom hint (`curvesNeedRedraw`), as AutoCAD
    regenerates.
  - Measuring flattens within 1 µm (`measure_tol`, `measure_tol_of`).
  - A new drawing takes AutoCAD's metric template in its units at 1:100 (`metric_template`): dimension
    text 2.5 mm on paper (0.25 m in metres), the grid a metre (`Drafting::grid_spacing` from the
    units when there is no project file), MLINE 20 mm, AUTOCONSTRAIN welding within 0.05 mm
    (`distance_tolerance` left out), Mizan's ANSI hatches at the Hatch tool's 0.025 / metres per
    unit, a table cell without a height at half its row, an image on an empty sheet 10 m wide.
  - INSERT scales a block by its block units (or its source drawing's INSUNITS) to the drawing's
    (`insert_unit_factor`), as AutoCAD; xrefs are not scaled again (AutoCAD bakes that into the
    reference's scale on attach). The last text height used is kept per drawing (`useAnnotate`).
  - On open, a dimension style nobody sized (the codec's stock Standard: text 0.18, offset 0.625,
    extension 1.25; AutoCAD's imperial Standard differs) takes the metric template's sizes in a
    drawing not in inches or feet, and Qiyas's dimensions in it are drawn again
    (`size_stock_dim_styles`): at 1:100 in metres it gave 18 m text and 125 m extension lines.
  - Tables: when Qiyas draws a table again (`rebuild_table`), each row grows to hold its cells'
    text as the text engine wraps it (`qiyas_geom::fit_rows`, `mtext_height`); rows never shrink.
  - Survey coordinates: the edit tolerance (`modify2` `TOL`, 1e-6) and the same-point test
    (`qiyas-geom` `SAME_POINT`) grow with the coordinates' size past a million (`tol_near`,
    1e-12 of it; `SAME_POINT_REL` 1e-13), as float steps there exceed them: JOIN once refused two
    walls meeting at a 3.3e9 mm UTM northing.
- The batch also carries fill triangles (section 12, `u32` colour each, bit 24 =
  `FILL_BACKGROUND`) and their ranges (13), only when there are fills; GL draws fills under the
  lines, then wipeouts in the background colour over all lines (they hide line work under them,
  even lines drawn later: no draw order yet), then the wipeout frames. A fill keeps its rings in
  its own coordinates with the reference's transform (`FlatFill::xf`), so a fill inside a block
  is triangulated once per build and mapped through every reference; a fill of more than 2,000
  corners draws its outline only (the kernel's ear clipping slows down with the square of them;
  `cargo test -p qiyas-render --release --test fills fill_in_a_block` guards it). Solid hatches, 2D solids
  and traces fill; gradient hatches fill with their first colour; pattern hatches draw pattern
  lines clipped to the boundary (range flag `PATTERN`, never part of a hatch boundary, left out
  when denser than 20,000 lines); rays and xlines run across the sheet without growing its
  bounds; multilines draw their element lines but measure along their centre line (the drawn
  points); points draw as a cross (PDSIZE, or 1% of the sheet). Splines flatten from control
  points and knots (the curve through the fit points when there are none). Measuring sees
  outlines only (`FlattenOutcome::into_paths`).

## Drawing

- `Command::Draw(DrawSpec)` (`qiyas-core/src/draw.rs`): line, polyline (bulges), rectangle,
  polygon, circle, arc (centre or three points), ellipse, spline (fit points plus the cubic's
  control points), point, donut, ray, xline, revcloud, wipeout, hatch (closed entities or a
  ring; solid, ANSI31, ANSI37 or a built-in pattern: LINE, NET, DASH, BRICK), 2D solid, trace,
  multiline, sketch. One undo step on the sheet on screen; `Outcome::Added` lists the handles.
  Wire form `{"type":"draw","kind":"line",...}`, angles in degrees. Mizan's `draw` (line,
  polyline, rectangle, circle, arc, hatch, revcloud).
- `Command::Batch { commands }` runs `Draw`, `Text`, `MText`, `InsertBlock`, `Table`, `EditCell`,
  `NewLayer` and `SetCurrent` in order as ONE undo step (tables snapshot, so the current layer
  comes back too) and as all or nothing; `Outcome::Added` lists every entity. Anything else is
  refused. The AI's drawing tools use it.
- `Session::current` (layer, colour, linetype, lineweight; layer "0" and ByLayer by default)
  styles new entities. `Command::SetCurrent` is not an undo step; saved in the project file;
  layer and linetype as the drawing spells them, lineweights from AutoCAD's standard set; a
  layer gone from the drawing falls back to "0". Layers panel: the pen button makes a layer
  current (not an xref's).
- Home tab, Draw: Line (C close, U undo the last line), Polyline (A arc, L line, C close, U
  undo; Esc keeps what is drawn), Rectangle, Circle (2P, 3P, typed radius), Arc (three points,
  or C centre first), Ellipse, Spline, Point, Hatch, Revision cloud, Wipeout.
- Hatch: click inside. `Session::boundary_at` runs `arrangement::bounded_faces` over the entities
  drawn in a box about 50 screen pixels across, flattened again at the measuring tolerance (1 µm in any unit, `Session::measure_tol`), growing
  the box 4 times until the region found fits inside it; regions inside it become islands. An
  edge along a drawn circle or arc comes back as that arc (`Boundary::Ring::bulges`, passed on
  as `DrawSpec::Hatch::bulges`). An entity inside an arrayed reference keeps its drawn
  segments (`locate` places one cell). More than 50,000 drawn segments in the box, text boxes
  and pattern lines not counted, or 4 times as many flattened again, is `TooDense` (the prompt
  says zoom in), never a ring from part of them. A hatch
  measures even-odd (islands taken away), each path exactly when it is a bulged polyline; a path
  that crosses or touches itself counts as its simple loops (`Path::loops`), so a click region's
  island against its wall is taken away too. An island's path keeps the polyline flag (the DWG
  writer writes a polyline edge whole only under it). Type a pattern
  (SOLID, ANSI31, ANSI37, LINE, NET, DASH, BRICK), S scale, A angle; the default scale is
  0.025 / metres per unit (25 in millimetres).
- Command line: L, PL, REC, C, A, EL, SPL, PO, H, REVCLOUD, WIPEOUT; typed whole: POLYGON (POL)
  sides x,y radius [I|C], DONUT (DO) inner outer x,y, RAY / XLINE (XL) x,y x,y, SOLID (SO) 3 or
  4 points, TRACE width points, MLINE (ML) points, SKETCH points.

## Modify

- `Command::Modify(ModifySpec)` (`qiyas-core/src/modify.rs`): move, copy (A: several in a row),
  rotate and scale (C: a copy), mirror (keep; texts and attributes still read forwards, as
  MIRRTEXT 0), stretch (line and polyline vertices in the window move; other entities move whole
  when wholly inside or when their centre or insertion point is), align (one or two pairs, no
  scaling), rectangular and polar arrays (polar `angle` is between items), explode (block
  references one level, as drawn: base point and turned array cells; layer 0 and ByBlock take
  the reference's; visible attributes become texts holding their values; polylines and
  multilines; anything else is left as it is, `Outcome::Exploded { left_alone }` says how many).
  One undo step. Wire form `{"type":"modify","kind":"move","targets":[...],...}`, angles in
  degrees. Erase is `DeleteEntities`. Mizan's `modify`, `edit` and `erase` run them
  (`qiyas-ai/src/execute/edit.rs`), and `undo_last` takes back only its own newest step.
- Targets as for delete: the drawing's own entities at the top of the sheet, all or nothing.
  Changes in place keep handles; copies get new ones (attributes too; vertex, face and sequence
  end handles are nulled for the writers to give) and drop reactors, extension dictionaries and
  hatch associativity. Dimensions and tables only move and copy: a dimension's `*D` block is in
  world coordinates and moves with it (its own insertion point stays); a table refers to its
  `*T` block, which stays; a copy gets its own block under a new anonymous name. The other tools
  refuse them; the window leaves them out of a selection for those tools and says how many.
  Images and PDF underlays move but are not copied.
- The take-off follows edits in the same undo step (`Session::follow_takeoff`): rows measuring
  a moved, turned, scaled, mirrored, stretched or aligned entity are measured again; a row
  measuring an erased or exploded entity and others too is measured again without it (a count
  counts one less); one measuring only erased ones keeps its value but drops its link.
  `Outcome::TakeoffFollowed` (erase, or a placing tool that left a row unmeasurable; `Changed`
  otherwise) and `Exploded` say how many rows were measured again and unlinked; the window shows
  it. Erasing an image or underlay cleans its definition's link.
- `Session::pick_window(bounds, crossing)` (IPC `pick_window`): a window takes entities drawn
  wholly inside, a crossing those with a segment touching it (clipped exactly); always the entity
  at the top of the sheet (a block or xref reference whole), with its kind. The select tool
  keeps a selection set: a click replaces it (PICKADD), Shift+click adds, a left drag draws a box
  (left to right a window, solid; right to left a crossing, dashed), Shift+drag adds, Esc clears;
  after an undo or redo it keeps only what is still drawn. A modify tool starts on it (or on the select
  tool's pick, whole), else asks for one (clicks or a box, then Enter); stretch always asks for
  its crossing window. Home tab, Modify group; command line M, CO/CP, RO, SC, MI, S, AL, AR (R or
  P), X, E. Grips come later. `Session::pick_polygon(points, crossing)` (IPC `pick_polygon`) does
  the same for a polygon (concave too, either way round; an entity wholly inside has both ends and
  the middle of every segment inside and crosses no edge); Mizan's area tool uses it.
- `Command::Edit(EditSpec)` (`qiyas-core/src/modify2.rs`, wire `{"type":"edit","kind":"trim",...}`):
  trim, extend, break, join, lengthen, reverse, offset, fillet, chamfer, blend, pedit, overkill,
  flatten; same targets, one undo step. The take-off follows (`follow_edit`): rows on a cut
  entity measure every piece, rows on joined ones the joined polyline (the outcome says how many
  rows measure it), and a row that can't be measured any more is unlinked and counted (`unlinked`),
  keeping its value. Lines, arcs, circles and flat
  polylines are reshaped; anything drawn can cut or bound. Trim and break keep the handle on the
  longest piece; join keeps the first's. Offset keeps exact arcs (kernel `offset_polyline`, sharp
  straight corners). Fillet and chamfer: two lines or two straight sides of one polyline.
  `Outcome::Edited { added, removed, left_alone }`. Home tab, Modify group, after the modify tools; command line TR, EX
  (Shift+click swaps), BR, J, LEN, REVERSE, O, F, CHA, BLEND, PE, OVERKILL, FLATTEN.

## Text

- `qiyas-core/src/text.rs`: `Command::Text` (one line; `rotation` in degrees) and `MText` (wire
  `mtext`; plain text, `\n` becomes `\P`, `\`, `{`, `}` escaped; box top left and width) go on the
  current layer and state with a text style (`None`: the drawing's TEXTSTYLE, else "Standard");
  `EditText { target, value, tag }` changes a text, an MText (as plain text: its formatting goes)
  or one attribute (`tag`, may be cleared) of a block reference; chains refused. One undo step
  each.
- New and edited text is stored in reading order and marked with xdata app `QIYAS_TEXT`
  (`qiyas_geom::LOGICAL_TEXT_APP`, never `QIYAS`: `embed_measurements` deletes those);
  `FlatText::logical` makes the renderer ignore "Arabic text is reversed" for it.
- `FindReplace { find, replace, scope: sheet | drawing, match_case, whole_word }`: texts, MText
  plain runs (codes such as `\C1;` and braces are never matched), attribute values (a multiline
  one's embedded MText, every line) and dimension overrides (`<>` kept) on the sheet or every
  sheet, not inside block definitions; one undo step, `Outcome::Replaced { count, handles,
  skipped }` (none when nothing changes). Entities on a locked layer, or outside a block edited
  in place (`Session::is_locked`), are left as they are and counted in `skipped`. A TEXT the result would leave blank or break into two
  lines, or a single-line attribute it would break, is left as it is and counted in `skipped`. `FindText` lists the
  matches (`Outcome::Found`), no step. `DrawingInfo` has `text_styles` and `text_style`;
  `EntityInfo::plain_text` is an MText's text without codes.
- Home tab, Annotation group; command line TEXT (DT): click, type in place, Enter (a click elsewhere
  places it too; Esc drops it), H height, R rotation; MTEXT (MT, T): two corners, then the MText
  dialog; TEXTEDIT (ED, DDEDIT) or a double click on a text with Select: the edit dialog (an
  unchanged value sends nothing); FIND: the find and replace dialog (Find next zooms to each
  entity, switching sheet: the sheet's first fit goes to the match, `fitMatch`; Replace all
  highlights what changed and says how many were skipped). New text height starts at
  2.5 mm in drawing units, then the last one used.

## Dimensions

- Drawn as themselves: a Dimension draws its `*D` block (or, without one, the geometry
  `qiyas_geom::dimension_entities` generates from its points and style); leaders draw their line
  and arrowhead, tolerances their frame (flatten's `owner`: every path, even an arrowhead block
  inside, is the entity, chain as for any entity). Picking, windows, `entity_info` (its text) and
  `MeasureEntity` (its measured distance, a length row; angles have none) see the dimension.
- `Command::Dimension(DimSpec)` (`qiyas-core/src/dimension.rs`, wire `{"type":"dimension",
  "kind":"linear",...}`): linear (no rotation: horizontal or vertical by where the line point is),
  aligned, angular (3 points + arc point), radius, diameter, arc length, ordinate, baseline,
  continue (on the base's own line; baseline a DIMDLI step further out each), quick (lines, arcs,
  circles); an ordinate may read 0. Each is a real Dimension on the current layer in the
  current style (DIMSTYLE) with a `*Dn` block: Lines, Solid arrowheads, one Text (marked
  `QIYAS_TEXT`), sized DIMASZ/DIMTXT/DIMEXO/DIMEXE/DIMGAP x DIMSCALE, numbers by DIMDEC, DIMZIN,
  DIMLFAC, DIMRND, DIMDSEP, DIMPOST (decimal only).
- `DimEdit` (text override with `<>`, flip, move text) and find and replace rebuild the block in
  the same step (`rebuild_dimension`). `DimBreak` and `DimJogLine` change the block's lines only:
  a later rebuild drops them. `DimSpace` moves parallel linear dimensions to equal spacing.
  Dimensions are not re-measured by `follow_edit`; a moved dimension's take-off row follows.
- `Leader`/`MLeader` (a Leader + MText; Qiyas writes no MULTILEADER), `Tolerance` (frame in
  AutoCAD codes). `SetDimStyle` (create from current, common fields, make current; header DIM vars
  follow; an edit rebuilds, in the same step, the sheets' dimensions of that style whose block
  Qiyas generated: every part of such a block carries `QIYAS_TEXT`; AutoCAD's blocks stay as they
  are). `AnnotationScale` sets CANNOSCALE ("1:50", header and the variable dictionary); new
  dimensions, leaders and tolerances keep the scale's factor as their own DIMSCALE (`DSTYLE`
  override; it replaces the style's, which applies at 1:1), new text 2.5 mm x scale. No
  annotative scale lists. All undoable (header bits are in `TablesSnapshot`). DIMTAD 4 puts the
  text below the line; `entity_info` shows `%%c`/`%%d` as their signs.
- Units after the number: a dimension Qiyas makes (Annotate tools, command line, Mizan, any
  `DimSpec`) shows the drawing's unit: " m", " cm", " mm", `"` (inches), `'` (feet) from
  INSUNITS, unknown as mm (`Units`). The AutoCAD way: a DIMPOST override on the dimension itself
  (`ACAD` `DSTYLE` group 3, `<> m`; `qiyas_geom::dimpost`/`set_dimpost`, which `DimLook::named`
  reads, so AutoCAD's own overrides count too), only when its style's DIMPOST is empty (or the
  codec's plain `<>`), DIMLFAC is 1 and it is no angle; existing AutoCAD dimensions and styles that
  have a suffix are never touched. A dimension on a layout gets none (paper units); one given another
  style (properties, match) takes or drops the suffix as that style says. `DimStyleFields::show_units` ("Show units" in the style panel,
  `SetDimStyle`, default on, older wire commands without it read as on) is kept in the drawing's
  variable `QIYAS_DIMUNITS_<style>` ("0" once off; undoable with the tables); turning it
  off or on rebuilds the style's Qiyas-made dimensions with the suffix taken off or put on (a
  suffix that is not one of ours stays), another style edit rebuilds them as they are. DimEdit and
  find and replace rebuild from the dimension's own override, so the suffix stays. A DWG read
  keeps the xdata bytes it read and writes them over an edited record: `apply_unit_suffix` drops
  the `ACAD` blob (`raw_dwg_eed`) of the dimension it changes. Default text height is the style's
  DIMTXT x DIMSCALE (the codec's stock 0.18, whatever the units, when the drawing has no style of
  its own): readable on a plan in metres only after `set_annotation_scale`; Mizan's tool says so.
- Annotate tab (Dimensions, Leaders, Dimension style…); command line DLI, DAL, DAN, DRA, DDI, DAR,
  DOR, DBA, DCO, QDIM, DED (T text, F flip, M move), DIMBREAK, DIMSPACE, DIMJOGLINE, LE, MLD, TOL,
  D (DIMSTYLE). Mizan draws them too: `add_dimension` (every `DimSpec` kind, handles for the
  picked entities) and `set_annotation_scale`.

## Comment clouds

- `qiyas-core/src/comment.rs`: `Command::Comment { points, text, arc_len }` draws a revision cloud
  round the polygon and an MText label beside its top right corner (2.5 mm at the annotation
  scale, at least an 80th of the sheet), both on layer `COMMENTS` (red, made when missing,
  ByLayer), one undo step. Both carry `QIYAS_COMMENT` xdata: the cloud `1`, an id and the text as
  typed; the label `2` and the same id (a string, so a copy finds its own label).
  `ListComments` (`Outcome::Comments`) lists every sheet's clouds with the label's text (edited
  like any MText), else the cloud's.
- Window: COMMENT (`commentCloud` in `dimTools.ts`: click the corners, Enter, type the comment);
  the side panel's Comments tab (`useComments`, `CommentsPanel`) lists them and shows one on a
  click. No ribbon button yet (the ribbon layout is being redone). Mizan: `add_comment`,
  `list_comments`.

## Parametric

- `crates/qiyas-param` knows no drawing: a `Sketch` of points (x, y unknowns), circle radii,
  curves (line, circle, arc: centre, start, end kept on one circle) and constraints (coincident,
  fixed, horizontal/vertical of a line or two points, parallel, perpendicular, collinear,
  concentric, tangent on the side it is on now, equal, symmetric; distance along an axis or
  straight, angle, radius). `solve(sketch, pinned)`: Levenberg-Marquardt with minimum-norm steps
  (`dx = -Jᵀ(JJᵀ+λI)⁻¹r`, nalgebra Cholesky, Jacobian by central differences per constraint),
  split into groups joined by shared unknowns, on a copy moved to its middle and scaled to one
  unit across (site grids); what no constraint holds stays put, exactly; a conflict fails with
  the sketch untouched. `cargo test -p qiyas-param --release --test solver_speed`
  (in `scripts/check.sh`): 200 connected constraints in about 4 ms, guard 50 ms. `expr.rs`: own
  parser for `d1 = 2*d2 + 1.5` (numbers with Arabic digits, `+ - * /`, brackets, names, `pi`,
  sin/cos/tan in degrees, sqrt, abs); `evaluate_all` refuses unknown names and circles,
  `evaluate_each` works each out on its own (a broken one spoils only its users), `normalize`
  stores expressions in ASCII (older DWG code pages have no Arabic digits).
- `qiyas-core/src/param.rs`: constraints hold parts (`Ref { handle, part: point | centre |
  edge }`) of lines, lightweight polylines (straight segments only), arcs and circles on the
  sheet, flat in plan; others and chains refused. Stored as JSON strings after an `Integer16(1)`
  in `QIYAS-PARAM` xdata (registered like `QIYAS_TEXT`; through DWG and DXF), a geometric one
  on the held entity that holds the fewest, a dimensional one on its own Dimension; a stored
  constraint names its holder, so a copy's stale xdata is never read. Commands (one undo step
  each, `Outcome::Constrained { ids }`): `AddConstraint { kind, targets, points }` (each pick's
  point picks the nearest vertex or edge; the first entity picked stays when it can),
  `AutoConstrain { targets, settings }` (kernel `constraint_inference`, Equal off by default:
  the sample rectangle gives 4 coincident + 2 horizontal + 2 vertical; pair relations looked for
  among off-axis lines only, as the kernel's own filter is quadratic; more than 2,000 constraints
  refused, counted before the kernel runs), `AddDimConstraint {
  kind: linear|aligned|angular|radius|diameter, targets, points, line_point, expression }` (a
  real Dimension through `add_dimensions`, text `[d1=<>]`, `[fx: d1=<>]` for a formula: the
  brackets are the lock mark; names d/ang/rad/dia + n; lengths in drawing units, angles in
  degrees), `SetParameter { name, expression }` (changed values re-solve, each keeping its
  anchor: a distance's first point, an angle's first line, a centre), `DeleteConstraint { id }`,
  `DeleteConstraintsOn { targets }`; `ListConstraints`, `ListParameters` read.
  `DrawingInfo::has_constraints`.
- Every drawing command keeps them (`Session::run_as`: `param_snapshot` before,
  `follow_param` after, in the same step; `param_extras` adds the tables and take-off to the
  step when needed): an entity erased, reshaped (vertex or edge count, kind, sheet), changed by
  any `Edit` (trim, break, reverse, extend... keep the handle while its vertices become other
  points) or a mirrored arc loses its constraints (`Outcome::Edited::constraints_removed`), a
  dimensional one its Dimension, and parameters that used it keep their values as numbers; an
  entity placed (move, rotate, scale, stretch, align, a mirrored line) is held where it went
  while its constraints are solved again (a locked entity that would move, or a conflict such as
  a fixed point moved, refuses the command). The take-off and fields follow what the solver
  moved. A parameter whose expression can't be worked out (a name gone, text another program
  garbled) is left out of solves and listed with its `error`; the manager shows it and sets or
  deletes it.
- Window: Parametric tab (Geometric: Coincident…Equal, Auto constrain; Dimensional: Linear,
  Aligned, Angular, Radius, Diameter; Manage: Parameters manager…, Delete constraints, Show
  constraints), `src/tools/constraintTools.ts` through `useTool` (its `param` slot), command line
  GEOMCONSTRAINT/GC <C|COL|CON|F|PAR|P|H|V|T|SM|S|E>, DIMCONSTRAINT/DC <L|AL|AN|R|D>,
  AUTOCONSTRAIN, DELCONSTRAINT, PARAMETERS/PAR. `src/app/useParametric.ts` (after
  `useAnnotate`) reads the lists after every redraw, only for a drawing with constraints or once
  a parametric tool or the manager was used. `ConstraintBar` is docked over the drawing's start
  corner (a glyph per constraint grouped by entity; pointing highlights its entities through the
  AI highlight, a click deletes it, × hides the bar until Show constraints); it does not sit
  beside each entity. `ParametersManager`: name, expression, value; Enter or leaving a changed
  expression sets it; refusals show in the dialog. Not AI tools yet.

## Tables and fields

- Tables are drawn as themselves (`qiyas_geom::table_entities`, flatten's Table arm): the `*T`
  block when it has something in it (in table coordinates: insertion point, horizontal direction,
  rows going down), else grid lines (none inside a merged cell) and each cell's text as an MText
  generated from the rows, columns and cells. Picking and windows give the table; `entity_info`
  has `table` (cells as plain text, widths, heights, merges, field cells) and no length or area;
  `table_cell_at` (IPC too) is the cell under a point.
- `qiyas-core/src/table.rs`: `Command::Table { at, rows, cols, col_width, row_height, data }` (current
  layer, current text style, text half the row height, middle centre), `EditCell` (plain text,
  stored escaped; a merged cell's first), `InsertRow/InsertCol { at }`, `DeleteRow/DeleteCol`
  (one kept), `MergeCells { first, last }` (one cell unmerges). One undo step each; each draws the
  `*T` block again (`rebuild_table`; a block another table shares gets a fresh `*Tn` instead).
  A table's block is found by name, else by `block_record_handle` (the DWG reader sets only the
  handle; `qiyas_geom::table_block`), also when copying. Find and replace reaches cells. Qiyas's
  tables are marked `QIYAS_TEXT` (reading order).
- `qiyas-core/src/field.rs`: fields are AutoCAD field codes in the text (`%<\AcVar Date ...>%`,
  `%<\AcVar Filename>%`, `%<\AcObjProp Object(%<\_ObjId HEX>%).Area|Length ...>%`, Qiyas's own
  `%<\AcVar QiyasCount "BLOCK">%`). The visible text is the value; the code (the whole text as
  typed) is kept in `QIYAS_FIELD` xdata (on a table: row, column, code per cell). `Command::Field
  { at, height, field }` adds a one-line text; codes typed into Text, MText or a cell count too.
  `UpdateFields` works them all out again; area and length fields follow every modify, edit and
  erase in the same step (`follow_fields`). A field is a derived value: one on a locked layer, or
  outside a block edited in place, is worked out too (as AutoCAD); the lock stops only the user's
  own changes (find and replace, attribute sync, edits). Metres and square metres (`in_metres`, as the
  take-off), 3 decimals; UTC date; `####` when the entity is gone. Not worked out on open. Find
  and replace leaves field texts and field cells as they are and counts them in `skipped`.
- Annotate tab, Tables group: Table (click, then rows, columns and sizes), Edit table (or a double
  click on a table: `TableEditor`, a grid in the drawing's column order, Shift+click to merge),
  Field (click, then `FieldDialog`; area and length pick their entity; also from the table
  editor into a cell), Update fields. Command line TABLE (TB), TABLEDIT, FIELD, UPDATEFIELD.

## Blocks

- `qiyas-core/src/blocks.rs`: `CreateBlock { name, base, targets, convert_to_insert }` (sheet
  entities leave for a new block, relative to `base`; names AutoCAD refuses, or the drawing's own,
  are refused; with convert a reference takes their place; rows on them are unlinked and counted,
  `Outcome::Edited`), `InsertBlock { name, at, scale, rotation, attribute_values, from_path }`
  (current layer; attributes from the block's ATTDEFs, constants excepted; `from_path` copies a
  missing block with its layers, linetypes, styles and nested blocks from another drawing, read
  only), `WriteBlock { name, path }` (a new .dwg in the drawing's folder, never the drawing
  itself; the Save dialog's path goes through IPC `write_block`, keeping a `.bak` of a file it
  replaces, written through a temp file), `AttDef` (current layer; drawn on a sheet as its tag; inside a block
  only constants draw), `EditAttributes`, `EditAttDef`, `SyncAttributes` (ATTSYNC, values kept; a locked reference is left and counted,
  `Outcome::Updated`),
  `RedefineBlock { name, from_path }`. One undo step each; WBLOCK is none. Constant ATTDEFs are
  never prompted (`AttDefInfo::constant`).
- Edit in place (`EditBlockInPlace { target }`, a sheet reference, not arrayed, even scale, no
  dimensions or tables inside): the block's entities are copied onto the sheet through the
  reference (no step), the reference hidden, the rest faded (`blocks::fade`), locked
  (`sheet_targets`) and left out of `pick` and `pick_window`; new entities on that sheet join the
  block. Commands are steps as usual, but undo stops at the edit's start (`History` floor, which
  follows the 200-step cap); only CreateBlock, WriteBlock, another edit in place, redefining the
  block (or one holding it) and inserting it into itself wait; Save waits too. Closing is by snapshot, never by undo:
  `SaveBlockEdits` makes the sheet's new entities the block's contents (a changed copy replaces
  its entity under the same handle, an unchanged one leaves it, gone ones go, a dimension's `*D`
  block is taken back too), `DiscardBlockEdits` drops them; everything else done meanwhile stays.
  Neither is an undo step, and nothing before it can be undone after it: Save clears the whole
  history, Discard too unless every step made while editing was undone (`History::close_edit`); a
  failed Save puts back what it wrote and leaves the edit open. `Outcome::BlockEditClosed` says how
  many steps went, and how many take-off rows lost their link. `DrawingInfo::block_edit` names the block.
- Insert tab, Block group: Create block (B: pick, base point, name dialog), Insert… and Write
  block… (`BlockPalette`: thumbnails from `block_thumbnail` through `render_png`), Edit in place
  (REFEDIT/BEDIT; the bar saves or discards; REFCLOSE saves), Define attribute (ATT), Edit
  attributes (ATTEDIT, or a double click on an attribute: `AttributesDialog`), Attribute manager
  (BATTMAN), Design Center (another drawing's blocks with their attribute definitions: insert,
  prompting for them, or redefine one this drawing has),
  Tool palette (`tool_palette` in `settings.json`, blocks and command lines; kept apart from the
  Settings dialog like `recents`). Command line B, I, W, REFEDIT, BEDIT, REFCLOSE, ATT, ATTEDIT,
  BATTMAN.

## Layers, groups and properties

- `qiyas-core/src/layers.rs`: `SetLayer { name, prop: on | frozen | locked | color | linetype |
  lineweight | plot, value }` (JSON value; a layer colour is "ACI n" or "#rrggbb"; the current
  layer can't be frozen), `NewLayer`, `DeleteLayer` (never 0, Defpoints, the current layer or one
  with entities, inside blocks too), `RenameLayer` (codec `rename_layer`: entities, layer states;
  take-off rows' `layer`, the current and hidden layers follow; a name another layer has is
  refused). Off and frozen layers are left out of the batch, counts and measures as before
  (`layer_shown`). Locked layers: `sheet_targets` refuses their entities for every command that
  changes what it is given (modify, edit, erase, set and match properties, text, dimension and
  table edits, blocks); `sheet_targets_any` lets them through where they are only read or not
  changed (cutters and boundaries, what a dimension or field measures, groups, a viewport's
  freezes, a match's source). Field values on a locked layer still follow what they read
  (`follow_fields`, `UpdateFields`): the lock blocks user-initiated changes only (find and
  replace, attribute sync, edits). A document step that moves `Session::current`, the hidden layers
  or the layer states' hidden layers keeps them as they were (`undo::ViewState`) and undo and
  redo put them back. Layer states are AutoCAD's own (`ACAD_LAYERSTATES` XRecords, codec
  `capture_layer_state`): `SaveLayerState` (Qiyas's current layer as the state's; the layer
  table's extension dictionary is made outside the step so an undo never leaves a dangling
  pointer), `RestoreLayerState` (the header's current layer is put back), `RenameLayerState`,
  `DeleteLayerState`; the hidden layers at save time live in the project file (`qiyas_hidden`)
  and come back on restore; the current layer stays thawed. `TranslateLayers { from_path,
  mappings: [{ from, to, color }] }`: renames, or merges into an existing `to` (entities move,
  viewports' freezes follow, the old layer goes), then the standard's layer properties (another
  drawing, read-only; IPC `drawing_layers`) and the mapping's colour. `SetViewportFrozen {
  viewport, layer, frozen }` edits a paper-space `Viewport.frozen_layers` (the Layers panel's VP
  freeze column, VPLAYER). `DrawingInfo` has `linetypes`, `layer_states`, `groups`,
  `viewports` (the sheet on screen's, not number 1). The window draws each viewport's model space
  without its frozen layers (see Clipboard, viewports, UCS, sheet sets and recovery).
- `qiyas-core/src/groups.rs`: `CreateGroup { name, targets }`, `AddToGroup`, `RemoveFromGroup`,
  `DeleteGroup` (AutoCAD's explode of a group: the entities stay), `Ungroup { targets }` (every
  group holding one): `GROUP` objects in `ACAD_GROUP`, members carry the group as a reactor; sheet
  entities only. `GroupInfo` lists each member's kind. Select: a click on a grouped entity selects the group (Home, Layers, "Group
  selection" is PICKSTYLE; Ctrl picks one alone); boxes pick entities.
- `qiyas-core/src/props.rs`: `SetProperty { target, prop, value }` (layer, color, linetype,
  lineweight, text, text_height, text_rotation and insert_rotation in degrees, text_style, radius,
  closed, global_width, insert_scale, dim_style; sheet entities only) and `MatchProperties {
  source, targets, props }` (the source may be inside a block or an xref; an xref's layer is
  never given to an entity). Insert scale and rotation scale and turn the reference about its
  insertion point (`modify::place`), so its attributes follow; the scale is the size of its x
  scale, a mirrored reference stays mirrored and `EntityInfo::scale` is negative for one. A dimension's text
  height is a `DSTYLE` DIMTXT override (`qiyas_geom::set_dimtxt`, before DIMSCALE) and its block
  is drawn again; a table's sets every cell; a change of shape runs `follow_edit`; rows on an
  entity take its new layer. `EntityInfo` has `rotation`, `closed`, `global_width`, `scale`, and
  a dimension's text `height`; for the palette's Geometry section (all optional, world
  coordinates and drawing units, an entity inside blocks as its references place it): `start`
  and `end` (line, arc), `centre` (circle, arc, ellipse), `start_angle` and `end_angle` (arc,
  degrees 0 to 360; ends that meet are a full turn), `vertex_count` (polylines), `position` (the
  insertion point of a text, MText or block reference) and `pattern` (a hatch; SOLID for a solid
  fill). Tests: `qiyas-core/tests/entity_geometry.rs`.
- Window: the Layers panel (a tree; see the UI kit) opens a layer's properties under its row (on, frozen, locked, plot,
  colour, linetype, lineweight, rename, delete), adds layers, and opens Layer states… (LAYSTATE,
  LAS) and Translate layers… (LAYTRANS, `LayerTranslator`); the properties palette edits the
  picked host entity in place (xref and block content stay read-only) and, with nothing picked,
  what new entities are drawn with. Home tab, Layers group; command line LAYER (LA: the panel),
  LAYSTATE (LAS), LAYTRANS, VPLAYER (the VP freeze column), GROUP (G: pick, Enter, name),
  UNGROUP, MATCHPROP (MA: source, then targets, Enter), PROPERTIES (PR, CH: the palette).

## Clipboard, viewports, UCS, sheet sets and recovery

- Clipboard (`qiyas-core/src/clipboard.rs`): `CopyToClipboard { targets, base }` (base null: the
  lower left of what they draw) makes a small DXF drawing of the sheet entities (not viewports, xref
  references or pictures) with the layers,
  linetypes, styles and blocks they use, marked `CLIPBOARD_MARK` (a `999` comment), coordinates
  from the base; not an undo step, `Outcome::Clipboard`. `PasteFromClipboard { at, text, as_block,
  height }` reads it through the block `Importer` (new handles; a missing layer comes with its
  colour, a block or style of that name already there is kept), base at `at`, one step; as a
  block (PASTEBLOCK) a new `A$Cnnnnnnnn` block under one reference; plain text becomes a Text
  (MText for several lines) 2.5 mm at the annotation scale. A clipboard drawing over 32 MB,
  plain text over 64 KB or more than 100,000 entities is refused; one in other units (INSUNITS,
  unitless as millimetres) is scaled to the drawing's (as a block, the reference's scale). The
  block `Importer` (paste, WBLOCK, Design Center, redefine) refuses blocks nested more than 32
  deep and drops the source's handles (linetype, colour book, visual styles, material, plot
  style, view representation, attribute definitions; a complex linetype's text style is copied). The desktop `run_command` fills `text`
  from the system clipboard (`arboard`, one `Clipboard` kept in `AppState`: on X11 a dropped one
  takes its copy with it) and puts a copy's text on it. Home tab, Clipboard group;
  Ctrl+C (COPYCLIP), Ctrl+Shift+C (COPYBASE), Ctrl+V (PASTECLIP), Ctrl+Shift+V (PASTEBLOCK); Ctrl+C
  with text selected in a panel copies the text.
- Paper-space viewports (`qiyas-core/src/viewports.rs`): `MView { corner1, corner2 }` (wire
  `mview`; layouts only) adds a `Viewport` on the current layer showing model extents
  (`set_scale`). Its frame draws (flatten's Viewport arm, never number 1); a click inside one with
  nothing of the layout near picks it. `paper_views()` gives each viewport's frame, model centre,
  scale and, when it freezes layers, the spans of the model batch it draws; the window fetches
  model space once (`sheet_batch("*Model_Space")`, `Session::with_sheet` keeps two sheets beside
  the one on screen) and `gl.ts` draws it per viewport with its own camera and a scissor box
  (`src/viewport/inset.ts`): fills, lines, text, under the layout's own drawing (wipeouts there
  hide no lines; pictures and twisted views are not drawn yet). `with_sheet` drops a sheet's copy
  once it is drawn on screen, and all of them on a drawing change. The window keeps model space's
  lines when only its glyphs are drawn again (`refreshSheet`). The palette sets `viewport_scale` and `viewport_centre`.
  View tab, Viewports group; MVIEW (MV).
- Split view: VPORTS (VPORTS 1 / 2, or View, Two viewports) shows `SplitPane` beside the main
  view on the drawing on screen: own camera and sheet, the main batch shared on the same sheet,
  view only this phase (founder's ruling; tools work in the main view), fetched again only when the
  core's version changes. Two drawings side by side wait for a multi-session window (founder's
  ruling).
- UCS (`qiyas-core/src/ucs.rs`): `Ucs { origin, x_axis }` (`UcsObject { target }`: a line's,
  polyline's, arc's, circle's, text's or block reference's), view state (no undo step) in
  `Session::ucs`, `DrawingInfo::ucs` and the project file; saving writes the header (UCSORG,
  UCSXDIR, UCSYDIR) and a `QIYAS` UCS table entry; a drawing without a project file takes its
  header's. The status bar shows "UCS x, y", typed points (`x,y`, `@dx,dy`, `@d<a`), ortho, polar
  and the grid run along it (`src/tools/ucs.ts`). UCS (origin, axis point), UCS OB, UCS W.
- Sheet sets (`qiyas-core/src/sheetset.rs`, `src/shell/SheetSetManager.tsx`): a `.qss` JSON `{ name,
  drawings: [{ path, layouts }] }`, paths relative with `/` and inside its folder (others refused,
  on save and on load); only the `.qss` is written. New from open drawings, open, add, remove,
  open all; Print all waits for plotting (Phase 4). SHEETSET or View, Sheet set….
- Autosave (`qiyas-core/src/autosave.rs`): every `autosave_minutes` (settings.json and the
  Settings dialog, whole minutes 0 to 120, 5 by default, 0 off) `autosave_drawings` writes each tab
  with unsaved changes (not during an edit in place) to `<file>.sv$` (a whole DWG) and
  `<file>.sv$.qiyas`, each to a temp file renamed over. On launch (the recent drawings) and before an open, `RecoveryDialog`
  offers what `offered` (`recoveryQueue.ts`) keeps of `recoverable`'s answers: an autosave newer
  than its drawing (the same second too) or whose drawing is gone, a crash's autosave at any age,
  and before an open only (never at launch, `atLaunch`) a drawing open in another Qiyas and older
  autosaves (`older_copies`). An older autosave alone is not asked about (something else moved the drawing on
  after it: rare), nor a crash that left nothing autosaved (Rust replaces its lock on the next
  open). Recover opens it in a new tab as
  `<name> (recovered).<ext>` with its take-off, unsaved, then Save As (its first save deletes the
  autosave it came from, `Session::recovered_from`); Discard deletes it; Later opens the drawing
  itself (not one open in another Qiyas, `laterOpens`). Questions wait their turn (`recoveryQueue.ts`, e.g. a sheet set's Open all). A save, or
  Don't Save on closing a tab or the window, deletes the drawing's autosave. Besides the timer,
  the app's 1 s watch (`files::start_watch`) autosaves a drawing after every 20 changes to it
  (steps, undos, redos; a save or an autosave starts the count again, `Session::autosave_due`).
- Lock files (`autosave.rs`, `files::Locks`): while a drawing is open the watch keeps
  `<file>.qlock` beside it (JSON `pid`, `host`, `started_at`: when that process started),
  removed on the watch's next beat after it is closed or saved under another name, and when
  Qiyas quits (`RunEvent::Exit`). The watch reads and writes lock files with none of the app's
  locks held, and tries a drawing another Qiyas has again every 10 s. A lock on this computer is
  alive while its pid runs and started when the lock says (Linux `/proc/<pid>/stat`, within 2 s;
  macOS `ps -o etime=`, best effort; Windows the pid alone through `tasklist`, untested here); a
  pid taken over by another process, a zombie, or this pid with another start time is dead. A
  dead lock means Qiyas closed unexpectedly: `recoverable` sets `crashed` and offers the autosave
  even when it is older. `recoverable` answers whenever there is an autosave or a lock other than
  this process's (`autosave` and `saved_at` null for a lock alone): the window decides what to
  show. A running process's lock, or another computer's, is `locked_elsewhere`: never taken over,
  no read-only open, and Discard (`discard_autosave`, `discard_recoverable`) is refused, so that
  Qiyas keeps its autosave. A dead lock is replaced when the drawing is opened again.
- Crash copies: when a crashed drawing is opened again (Later, or any open without Recover) its
  autosave is moved to `<file>.sv$.crash-<yyyymmdd-hhmmss>` (when written, UTC) with its
  `.qiyas`, before anything writes a new `.sv$`; at most 3 per drawing (the oldest go). The
  window asks before every open, so a crash copy was offered once already (as the crash's
  `.sv$`): `recoverable` lists it in `older_copies` (`{ path, saved_at, kind: "crash" }`, newest first), never
  as `autosave` and never as `crashed` (which is a dead lock only). Recover (`recover_autosave`)
  or Discard (`discard_autosave`) with a copy's path opens or deletes that copy; a recovered
  copy's first save deletes it (`autosave_copy_of`).
- Instance autosaves: an autosave (timer, 20 changes, panic flush) never writes over another
  Qiyas's: under a lock this process doesn't own (Open anyway) it goes to `<file>.sv$.<pid>`
  with its `.qiyas`. `recoverable` offers the drawing's `.sv$` (whatever its age) as `autosave`;
  every instance autosave whose process has ended goes in `older_copies` (`kind: "instance"`),
  even beside another Qiyas's `.sv$`; a running process's is never listed, and deleting it is
  refused. A save or Don't Save deletes this process's own instance autosave
  even while the `.sv$` is kept for the other Qiyas. The IPC `discard_autosave`: a copy's path
  deletes that copy; a drawing open here (Don't Save) its own autosave; else (the dialog's
  Discard) the autosave offered, never a crash copy.
  `RecoveryDialog` heads a question with "Qiyas closed unexpectedly last time" when any of its
  drawings crashed with an autosave (those come first, `enqueue`; rows say so only beside newer
  ones), else "A newer autosave exists", "Open in another Qiyas window" or "Older autosaves of
  this drawing"; each row gives the autosave's and the drawing's time in the app's language
  (`localTime(secs, lang)`, in `<bdi>`), says when the autosave is older than the drawing, lists
  its older autosaves under it, each saying whether a crash or an earlier window left it, with
  its own Recover and Discard (by the copy's path; dealing with the main autosave keeps them
  listed, and Discard opens the drawing about to open once nothing is left to ask about it), and warns "This drawing is open in another Qiyas window (PID … on …)" for
  `locked_elsewhere`: a drawing open elsewhere offers Open anyway and Later only (no Discard: it
  would delete that Qiyas's autosave; nor Recover). Recover, Discard and Open anyway act on the
  question they were clicked in (`drop` by its id; a drawing dealt with is no longer what Later
  opens); Later and Escape wait while one works. A Discard Rust refuses (another Qiyas took the
  drawing) is said in the window's language (`refusedBy`). After a Recover its Save As offers the
  recovered drawing's name, and one notice (a toast) says "Recovered from autosave of <time>",
  with "; saved to <path>" when it was saved. A drawing double-clicked while the launch question
  is up with nothing to ask about opens at once, behind it.
- Panic hook (`files::install_panic_hook`): after the usual report, autosaves every drawing with
  unsaved changes on its own thread, at most 5 s, best effort: `try_lock`, so the drawing whose
  command panicked (its lock held) is skipped; a panic during it starts no other.
- RECOVER (Manage tab, Drawing group, Recover…): opens a damaged drawing through the failsafe reader (as every open
  does) and `RecoverReportDialog` lists what it read and dropped by kind (`ReadReport`: DWG object
  types by number, else the reader's code). A truncated DWG loses its file header and does not
  open at all. AUDIT: see Audit, purge and options.

## Audit, purge and options

- `Command::Purge { kinds, dry_run }` (`qiyas-core/src/purge.rs`, wire `{"type":"purge",
  "kinds":["layers","blocks","styles","linetypes","dimstyles","groups"],"dry_run":true}`, no
  kinds = all): a dry run lists, by kind and sorted, what nothing uses (`Outcome::Purged {
  report: PurgeReport }`, no step); the real run takes it out as one undo step (tables, block
  records and their entities, groups) and returns what went; refused while a block is edited in
  place. It grows the save-time `purge_for_save` by the same rules (orphan `*Dn`/`*Tn` blocks,
  gone group members) and adds unused layers (never 0, Defpoints, the current layer, an xref's
  or the take-off's `QIYAS-*` layers every save writes), named blocks no reference uses (nested
  uses count while the outer block stays; never layouts, xrefs, dynamic definitions or other
  anonymous blocks), text styles, linetypes and dimension styles nothing names (never
  Standard, ByLayer, ByBlock, Continuous, the current ones, shape files, a kept style's `$n`
  child) and empty groups. "Uses" is read wide: besides the plain fields, every `Handle(n)` and
  quoted name in the objects', header's, dimension styles', linetypes' and complex entities'
  `Debug` text keeps an entry (`purge::Mentions`; a unit test pins `Handle`'s `Debug` form),
  and so does every quoted string of hex digits (DXF raw pairs keep handles so). A drawing
  holding data the codec does not read (unknown or proxy objects, unknown entities) purges
  layers only: `PurgeReport::limited_reason` says why. A dimension whose block name is `*U0`
  (the DWG reader's name for a null handle) has none; `purge_for_save` uses the same two
  predicates (`purge::anonymous_kind`, `purge::no_block`).
- `Command::Audit { fix }` (`qiyas-core/src/audit.rs`): `Outcome::Audited { report:
  AuditReport }` with `counts` by `AuditKind` and one `AuditItem` per kind and name (count,
  first 100 handles, a plain line, `fixed`): entities whose block is gone, references to
  missing blocks, dimensions and tables whose block is gone, gone group members, entities and
  attributes on missing layers, texts in missing text styles; listed only: empty blocks and
  unused layers. `fix` (one undo step, none when nothing changes) moves missing-layer entities
  to layer 0, missing text styles to Standard and drops gone group members; references to
  missing blocks are listed, never deleted. `fix` is refused while a block is edited in place
  (its working copies are on the sheet); the report alone still runs.
- Options in `settings.json` (`qiyas-desktop/src/settings.rs`, serde defaults, checked by
  `validate`): `display_precision` (2 to 4, 3), `snap_aperture_px` (1 to 50, 10),
  `pick_box_px` (1 to 50, 6), `background_color`, `crosshair_color`, `highlight_color`
  (`#rrggbb`, null = the theme's), `shortcuts` (command to key, `Ctrl+Shift+C`, `F3`, `D`;
  two commands on one key refused, `settings::shortcut_key` spells a key one way), `ribbon`
  (null = `ribbon::default_ribbon`), `aliases` (no two that differ only in case). The ribbon
  (`qiyas-desktop/src/ribbon.rs`, IPC `get_default_ribbon`) is data: tabs, groups, buttons
  `{ kind: tool | action | toggle, command, label }` as `Ribbon.tsx` draws them, in groups that may
  carry `main` (how many buttons show; the rest wait behind the group's "⋯" flyout). Tabs: Measure
  (Measure, Inquiry, Take-off, Dimensions with `main` 5, Markup), Draw (id `home`: Draw, Modify,
  Edit, Annotation, Clipboard, Layers), Annotate (Dimensions with all kinds and the style,
  Leaders, Markup, Tables); those three show on the tab row and the rest (Parametric, Insert, View,
  Manage (Drawing: Audit, Purge, Recover), Output) are under "More" (`SHOWN_TABS`). Group id
  `markup` holds the revision cloud and the comment cloud (COMMENT). `src/shell/ribbon.default.json` is that default, and both
  `tests/options.rs` and `ribbonLayout.test.ts` compare theirs with it. A saved layout may only
  use the default's buttons (moved between groups and tabs as it likes), and one that no longer
  checks out falls back to the default on load. An icon ribbon (`ribbonIcons.tsx`: one family, 24
  grid, 1.5 stroke, `currentColor`, keyed by command, a test fails when a button has none; icons
  that point along the text mirror in Arabic): every button is an icon with its name as
  `aria-label` and a tooltip (`Tooltip.tsx`, Base UI: name, `tip.<command>` description, the command line
  alias from the alias table via `ribbonInfo.ts` `COMMAND_LINE`; "Coming soon" on a disabled
  one; after a short delay, on keyboard focus too, hides on Escape, blur and click). Each group
  is a box of buttons in two rows filled column by column (a group made only of the headline tools
  is one row of primary buttons), its name small and muted under them, a thin rule between groups.
  Sizes are tokens in `theme.css` derived from `--font-base`: standard 30 px (18 px icon), primary
  42 px (24 px icon), scaled a little in narrow and wide windows. As the window narrows a group
  puts more buttons behind its "⋯" flyout (`Ribbon.tsx`, measured: scrollWidth against
  clientWidth), and the panel scrolls sideways only when that is not enough; flyouts
  (`RibbonFlyout.tsx`, a Base UI Menu) are placed on the window, so the scroll never clips them, and take the
  keyboard (Left and Right move between their columns). Views (and Open's recents menu) are drop-downs, still
  our own, placed the same way; Views never goes
  behind a "⋯". New, Open (with its recents arrow), Save, Save as, Undo and Redo are not in the data:
  they are the quick-access bar on the tabs' row after the app name (`QUICK_ACCESS` in
  `ribbonLayout.ts`; logical sides, so Arabic mirrors it), always shown, a `role="toolbar"` with
  one tab stop and arrow/Home/End movement (`useToolbarRoving`). The tabs' row scrolls sideways
  and stops `--prefs-width` short of the header's end (a margin, so no tab lies under the
  preferences overlay; `theme.css`; the preferences are four icon buttons, about 150 px: one
  language switch naming the other language ("ع" / "EN"), one theme switch showing where it
  goes (moon in light, sun in dark), settings (a gear) and about); they take their
  own row under 900 px (move the width with the quick-access bar's and the prefs' width). The tab
  "More" is a `role="tab"` that opens a menu; the tab chosen from it shows in its place (the indicator slides under it). The dialogs (Options, Customize, Aliases) are the window's.
- The window's half (`src/app/useOptions.ts`, read at start and after a dialog saves; Manage tab,
  Drawing group Audit…, Purge… and Recover…, Setup group Options…, Customize…, Aliases…): PURGE (`PU`)
  opens `PurgeDialog` on a dry run of every kind, listed by kind with a tick each (kinds with
  nothing are disabled) and `limited_reason` when set; Purge runs the ticked kinds through
  `runDrawingCommand` (redraw, info, autosave) and lists what went. AUDIT opens `AuditDialog`
  (`fix: false`): counts by kind and one row per item (`line` as its tooltip); Fix (`fix: true`)
  shows only when a fixable item (missing layer or text style, gone group member) is left. Both
  need a drawing; while a block is edited in place the window opens neither and says "save or
  discard the block edit first (REFCLOSE)" (`useOptions` `openPurge`/`openAudit`: the core
  refuses only a real purge, and would audit the edit's working copies). OPTIONS (`OP`):
  `display_precision` is for where no drawing is open (the status bar with none); a drawing's
  drafting precision wins while it is open (the dialog says so). The autosave interval stays the
  Settings dialog's (shown, with a button there). Snap aperture and pick box reach `useTool`
  (its `px`). Background and highlight colours (null = "Theme default") reach both viewports
  (`src/viewport/colours.ts` `palette`: a background of the user's decides by its lightness how
  white and black lines show; wipeouts take it); the crosshair colour is kept but not drawn (no
  crosshair yet; the dialog says so). Shortcuts: a table of the built-in keys (`BUILT_IN_SHORTCUTS` in `src/tools/shortcuts.ts`)
  and the user's; a key cell takes the keys pressed (physical key, `keyOf`), a key another
  command holds is refused (`shortcutClash`); `shortcutFor(e, user)` puts the user's first and a
  command given a key loses its built-in one; a key of the user's calls `preventDefault` (F5 never
  reloads the window). Only tools and the built-in keys' commands can be
  bound (`SHORTCUT_COMMANDS`), not ribbon actions. CUI (`CustomizeUi`): the ribbon as a tree
  (`ribbonLayout.ts` `editTree`: the saved layout, or the window's default, then what the core's
  `get_default_ribbon` has that it leaves out, hidden); show/hide and up/down; Reset saves null.
  `Ribbon.tsx` draws `layout ?? DEFAULT_RIBBON` (the same default as the core's). ALIASEDIT (`AliasEditor`): alias
  to command, upper-cased; refused: two that differ only in case, a command's own name
  (`COMMANDS`), a command that is not one; saved aliases reach the command line (`loadAliases`).

## PDF

- PDFs are read through pdfium (BSD-3), a shared library loaded at run time through
  `pdfium-render`. `scripts/pdfium.sh` downloads the build for this computer from
  bblanchon/pdfium-binaries into `crates/qiyas-desktop/resources/pdfium/` (git-ignored) and its
  licence into `LICENSES/pdfium-BSD-3.txt`. It installs the tested release only (`TESTED_TAG`,
  chromium/8086) and checks the download against `scripts/pdfium.sha256`; a mismatch installs
  nothing. Run it once per machine. Without it the PDF commands
  answer "PDF support is not installed: run scripts/pdfium.sh" and the PDF tests are ignored, so
  `scripts/check.sh` runs them only where the script has been run (no CI checks them).
- `QIYAS_PDFIUM` (the library or its folder) overrides where it is looked for.
- Release: run `scripts/pdfium.sh` before `pnpm bundle`; `tauri.bundle.conf.json` ships the
  folder as the resource `resources/pdfium/`.
- pdfium is not thread-safe: every call runs on one worker thread (`qiyas_pdf`).
- View tab: "Attach PDF…" (underlay or import, scale, page, insert point) and "Calibrate".
  Underlays are kept in the DWG/DXF as AutoCAD PDF underlays on layer `PDF-UNDERLAY`; imports
  go on `PDF-RRGGBB`, `PDF-FILL-RRGGBB` and `PDF-TEXT`. Save As rewrites underlay and image
  links for the new folder. Opening another drawing or closing the window with unsaved drawing
  changes asks "Save changes to the drawing?".

## Images

- View tab: "Attach image…" (png, jpg, tif, bmp, webp; width in drawing units, default a tenth of
  the sheet; insert point typed or clicked), then "Calibrate" as for PDFs. Images are kept as
  AutoCAD images (`RasterImage` + `ImageDefinition` in `ACAD_IMAGE_DICT` + reactor) on layer
  `IMAGE-UNDERLAY`, linked relative to the drawing when inside its folder.
- Images already in a DWG/DXF are drawn when their file is found (as stored, beside the drawing,
  in its `images/` folder); otherwise the problems list says "Image <file> not found".
  Rectangular clips crop; polygonal and inverted clips draw the whole image for now; images
  inside blocks are not drawn.
- Images over 20,000 px a side or 512 MB decoded are refused ("This image is too large").
- Files are decoded once (cut to 4096 px, EXIF orientation applied) with the `image` crate; the
  window gets a picture per zoom (64 to 2048 px) through the same `underlay_image` IPC as PDFs.
- Picking an image (Select tool, click its frame) shows brightness, contrast and fade sliders
  (`set_image_display`, one undo step per release).

## Plot

- `crates/qiyas-plot` (`plot_pdf(session, &PlotJob)`, lopdf, MIT) writes one sheet to a one-page
  PDF as the screen draws it, each batch drawn afresh for the paper
  (`Session::sheet_batch_with_tol`, curves within `PLOT_TOL_MM` = 0.01 mm on paper: a 1 m
  arc at 1:100 gets 18 segments a quarter): a layout draws model space in each viewport first
  (clipped to its frame, at that viewport's scale, without the layers it freezes, matched by
  name), then its own batch: pictures, fills as filled paths, lines, wipeouts in the paper's
  white, their frames, then text.
  `PlotJob { sheet, area: "extents" | {"window": [[x, y], [x, y]]} | "layout", paper: { name,
  w_mm, h_mm }, orientation, scale: "fit" | {"ratio": [num, den]}, centre, style, lineweights,
  output }`. Model space scales by its units (`Units::to_metres`); a layout's units are inches
  when its page setup says so (`plot_paper_units` 0 with a paper size), else millimetres.
  "layout" is the layout's paper from its page setup (the layout's origin at the printable
  area's lower left, inside the margins; rotation 90/270 swaps it), else its limits, else its
  extents; the plot offset is not read. Fit leaves a 5 mm margin; not centred puts the area's
  lower left at the page's.
  `paper_catalog()`: ISO A0 to A4, ANSI A to E, ARCH A to E1, portrait.
- Two steps, so the window never waits on a plot: `plot_snapshot` reads everything under the
  session lock as plain data (`PlotSnapshot`: the batches, what each entity plots with, font
  files, where each picture goes and what to draw for it), then `write_pdf` draws the pictures
  and writes the PDF without the session; the IPC commands run it on a blocking thread.
- Lines are paper millimetres: the plot style's lineweight, else the entity's with `lineweights`
  (ByLayer and ByBlock resolved as inside blocks, Default 0.25 mm), else the thinnest line. White
  plots black. Layers set not to plot are left out, and Defpoints never plots (an xref's
  either). Linetypes plot continuous, as the screen draws them.
- Text is real PDF text: each font used becomes an embedded TrueType subset (`subsetter`, Type0,
  Identity-H) with a ToUnicode map from the shaped text (`LaidText::text`, `Session::font_bytes`).
  The map keeps the first characters seen per glyph, and Noto Sans Arabic draws letters that
  differ only by their dots (ج ح, ب ت ن ي) with one glyph plus a dot mark: a glyph the map gives
  back as another letter (`FontSet::actual_text`) is shown inside a `/Span <</ActualText …>> BDC
  … EMC` holding its own characters, one span per glyph (a whole text's span reads backwards in
  poppler for RTL), so copy and search give the right word (pdftotext checks it in the tests).
  A text with a glyph that doesn't map back to its characters, or a font that can't be embedded
  (CFF outlines, restricted), draws as outlines; so does Arabic stored in visual order that Qiyas
  did not write (`arabic_stored_visually` with "Arabic text is reversed", unless `QIYAS_TEXT`).
  Its shaped text is not its reading text, so a ToUnicode map would be wrong.
- Pictures on any sheet and inside viewports (`Session::plot_pictures`; the window draws none
  there yet) are embedded at 300 dpi on the page, at most 8192 px a side (a raster image never
  beyond its own pixels, decoded at most 4096); each one left out (file gone, PDF support
  missing) is in `PlotOutput::skipped` with why.
- By command a plot goes only in the drawing's folder (`output` a name inside it or a subfolder;
  none: `<drawing>-<sheet>.pdf` beside it), written beside the target then moved over it.
  `plot_pdf_to` writes anywhere: the Save dialog later, and `plot_and_print`'s spool PDF in
  `spool_dir()`: `<temp>/qiyas-plot-<uid>` with mode 0700 on Unix (refused when another user
  owns it or it is a link), `<temp>/qiyas-plot` on Windows. `print_spooled` removes the PDF once
  `lp`/`lpr` have handed CUPS a copy (`hands_off_a_copy`); the PDF viewer's and Windows' copies
  go when a spool PDF is a day old (`SPOOL_KEEP`). Paths come back without Windows' `\\?\` prefix (`plain_path`).
- Plot styles (`ctb.rs`): `PlotStyle::{load, save, parse, to_bytes, apply, default_ctb}` read
  and write CTB and STB files (48-byte header, adler32 of the body, lengths, zlib text with LF
  lines ending in a NUL the length counts, as AutoCAD writes them; CR LF, plain text and a
  description not in UTF-8 are read too), keeping keys Qiyas doesn't know. Test fixtures in
  `crates/qiyas-plot/tests/fixtures/` are written by Qiyas's own writer (`write_fixtures`),
  never vendor files. A CTB's pen is the ACI of the drawn colour (exact, else
  nearest); an STB's style is the layer's plot style name, else Normal. Screen 0 is "no plot";
  screening fades toward white; grayscale greys.
- Printing (`print.rs`) hands the PDF to the OS: Linux `lp -d <printer>` (no printer: the PDF
  viewer through `xdg-open`), macOS `lpr`, Windows the PDF handler's Print or PrintTo verb
  through a fixed `-EncodedCommand` script that reads the file and printer from
  `QIYAS_PRINT_FILE` and `QIYAS_PRINT_PRINTER` (`ProcessStartInfo`): no name or path is ever
  inside a command. PowerShell runs with `-NoProfile -NonInteractive -WindowStyle Hidden` and
  `CREATE_NO_WINDOW` (printing and the printer lists), so no console flashes. Printer names starting with `-` or holding a quote are refused. Tests only
  use `print_dry_run` and `print_plan`.
- IPC (`qiyas-desktop/src/plot.rs`): `paper_catalog`, `plot_pdf`, `plot_pdf_to` (the Plot
  dialog's Save dialog path, anywhere; never from a typed or scripted command), `plot_and_print`,
  `print_pdf`, `list_printers`, `default_printer`, `load_plot_style`, `save_plot_style`.
- Window (`usePlot`, `src/shell/PlotDialog.tsx`, `PlotStyleEditor.tsx`): Output tab Plot… (PLOT,
  PRINT), Quick print (QUICKPRINT: the view on screen, fitted, default printer), Print all layouts
  (PUBLISH: one print job per layout, each with its kept setup) and Plot styles… (STYLESMANAGER:
  edit and save a CTB/STB). The Plot dialog plots the sheet on screen (extents, a window of two
  clicks in the drawing, or the layout), shows a preview drawn from the batch on screen (lines
  only, the area's picture cached while the dialog is open), and plots to PDF through the Save
  dialog (`<drawing>.pdf` offered, `plot_pdf_to`) or prints. Each sheet's page setup, by the
  layout's name (AutoCAD renumbers the `*Paper_Space` blocks), and the drawing's plot style are
  kept in the window's `localStorage` (`qiyas.plot:<drawing path>`): the project file has no place
  for them yet. A kept setup whose paper the catalog lacks is not used; Print all plots a kept
  window as the layout. Quick print and Print all run one at a time and stop before the next plot
  when another drawing comes on screen (`switchGen`).

## Inquiry

- Measure tab: Geometry (click an entity: radius, diameter, centre, angles, lengths, block and text
  facts in an Info card at the top of the right panel), Divide (click, then the number of parts),
  Measure along (click, then the step in metres), ID point (snapped coordinates in metres), and
  Extract data (filter by layers, type, block, text; pick fields and attribute tags; copy as TSV
  or export one "Data" sheet to Excel). Its Take-off group exports the take-off, imports a BOQ,
  compares and exports the conflicts.
- Divide and measure along place point entities on layer `QIYAS-POINT` by true arc length and
  count them in the take-off; points and count are one undo step (a drawing change: Save asks).
- The same five are AI tools: `measure_geometry`, `divide_along`, `measure_along` (step in
  metres), `id_point`, `extract_data` (paged under 40 KB).

## Drawings, tabs, recents and views

- Several drawings open at once, one file tab each above the drawing (title, "•" while the drawing
  has unsaved changes, × or a middle click closes; Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+W). The tab on
  screen is `AppState::session`, so every IPC command, the assistant and MCP act on it; the others
  are parked (`AppState::parked`, without their batch and pick index) and swapped in by
  `activate_tab`. Lock order: `tabs`, `session`,
  `parked`. Switching or closing the tab on screen starts a new assistant conversation, unless
  Mizan did it (`keep_conversation`, see the AI section). Opening a file that is open already
  shows its tab. Closing (or quitting) with unsaved drawing changes asks.
- New drawing (quick-access New, Ctrl+N, NEW / QNEW): `NewDrawingDialog` asks metres,
  centimetres or millimetres (the last choice kept in `localStorage` `qiyas.newUnits`), then IPC
  `new_drawing { units }`: `Session::new_blank` (`CadDocument::new()`, INSUNITS 6, 5 or 4) in a
  new tab at a placeholder `<config>/untitled/Untitled-N.dwg` (`tabs::untitled_path`: the
  smallest N no tab uses and no autosave holds). It is clean until it changes (closing it untouched
  asks nothing) and untitled (`is_untitled`, `DrawingInfo`/`TabInfo` `untitled`): `save()` is
  refused (`UNTITLED`), so the window's Save, and the save of "Save changes?", go through Save As
  (just the name offered); `save_project` writes nothing beside the placeholder; no "no geometry"
  warning; not a recent drawing until its first Save As. Its autosave works as any other's, in
  the placeholder folder; at launch `untitled_autosaves` lists the ones left there (Qiyas closed
  before their first Save As) with the recent drawings for the recovery question, and a recovered
  one stays untitled (`Session::mark_untitled`).
- Recent drawings: `recents` in `settings.json` (10 at most, newest first, kept by the app on every
  open and Save As; the Settings dialog never changes them); the arrow beside Open lists them,
  greyed "(missing)" when the file is gone, then Clear recents.
- Named views: the drawing's `VIEW` table (model space only). View tab, Views: choose one to zoom
  to it, Save current view… (Model sheet only) or × to delete; both are undoable drawing changes.
- A drawing that opens with nothing in model space says "This drawing has no geometry in Model"
  in the status bar.

## AI assistant and settings

- Settings live in `settings.json` in the app's config folder (Linux:
  `~/.config/com.qsmind.qiyas/settings.json`, mode 0600): provider (`anthropic` or
  `openai_compatible`), model (default `claude-opus-5-5`), base URL, effort, MCP on/off, port and
  token, language, theme.
- The API key is never in a file: it lives in the OS keychain (service `qiyas`, account = the
  provider name), is set only in the Settings dialog, and is never sent back to the window.
  Agents never read or type a real key; the founder pastes it himself.
- `QIYAS_AI_MOCK=1` with a debug build (`pnpm tauri dev`) uses a canned provider: it says a
  sentence, measures the first polyline on layer A-WALL and answers; with a selection in the
  message's `[state]` it calls `view_screen` and `select_entities` on it instead. No key, no
  network. Release builds leave it out.
- AI measurements are marked `ai`; the card's Undo only undoes when the last step is the AI's.
- The in-app agent is called Mizan (ميزان): the side tab, the panel and the system prompt use the
  name; code keeps `assistant` / `useAssistant`.
- Mizan asks before guessing (`prompt.rs`): when the request is ambiguous or misses what it needs
  (entities or layer, BOQ item, units, scale, tolerance, placement, overwrite or add) it asks the
  engineer one short question with the options it sees, in the engineer's language, and waits;
  when the request is clear it acts. The question is a plain text answer: the turn ends
  (`StopReason::EndTurn`, `AgentEvent::Done`), the question stays in the history and the next
  message is the answer (`agent.rs` test `a_question_ends_the_turn_cleanly...`). The fixed prompt
  stays under 900 words (a test), English and Arabic saying the same rules.
- Mizan's eyes (`qiyas-ai/src/image.rs`, `marks.rs`, `execute.rs` `picture`): `view_image` draws a
  sheet (`sheet`, by layout or block name; the one on screen by default; it never moves the
  user's view) as the engineer sees it on white: model space inside a layout's viewports
  (`Session::picture_batch_of`, `paper_views_against`, clipped to each frame), pictures and PDF
  underlays under the lines (`plot_pictures` + `draw_picture`, at most 4096 px; one that can't
  be drawn is named in the caption), fills, lines, text as glyphs, the take-off in violet,
  `highlight` handles thick violet. `marks: true` puts orange numbered badges on up to 40
  entities in view (highlighted ones first, then the largest; a block reference whole) and the
  caption lists `#n handle kind layer "text"`. The caption also says how big a pixel is (drawing
  units and mm) and how tall the text is (the smallest tenth and the median of each text's
  tallest glyph, in px; letters read from about 8 px), so Mizan zooms in with `bounds` to read.
- Window tools (`qiyas-ai/src/window.rs`, `DrawingHost::capture_screen`, `select`, `pointed`;
  `qiyas-desktop/src/ai_window.rs`): the core asks the window and waits at most 5 s for
  `ai_window_reply { id, value, error }`, without the session's lock. `view_screen` (`ai:capture
  { id, maxSide }`: the window draws its main view and reads it back in the same task,
  `Renderer.read`, `Viewport.capture`, `viewport/capture.ts`; the core then marks it, what the user
  selected first), `get_selection` (`ai:pointed { id }`: the selection, the marked area, the
  view), `select_entities` (`ai:select { handles, zoom }`: the Select tool's selection set,
  entities drawn at the top of the sheet only; no undo step). Without a window they say so.
  MCP clients reach them too.
- What the user points at (`qiyas-ai/src/context.rs`, `src/ai/pointing.ts`,
  `useAssistantWiring`): the Select tool's selection (or its one pick inside a block, with its
  chain) shows as a chip above the composer ("Selection · 3", its kinds), × leaves it out until
  it changes; the composer's area button starts the `mizanArea` tool (drag a box, or click
  corners and Enter or double click; it then goes back to the tool before and opens Mizan's tab)
  and an "Area · w × h m · n entities" chip; the canvas menu's "Ask Mizan about this" opens the
  tab with the selection. `ai_send { text, context: { selection, region, view, picture }, images }`:
  `Agent::run_turn_with(UserMessage)` adds to `[state]` the view on screen, the selection and the
  region's entities (handle, kind, layer, length and area in metres, a text's words; 50 listed,
  the rest counted by kind; totals without dimensions and block references), and when the
  selection or region changed since the last message (`picture`) a 1000 px picture of it, marked.
  Pictures pasted, dropped or attached (`src/ai/pictures.ts`: png, jpeg, webp, gif, at most 4, at
  most 1600 px a side, 6 MB of base64 each) go before the words as `ContentBlock::Image`
  (Claude: an image block; OpenAI-compatible: an `image_url` data URL) and show as thumbnails
  in the bubble. The history keeps at most 12 pictures or 16 MB of them: past either, the
  oldest give way to a line of text until 8 within 10 MB are left (`trim_pictures`); a
  conversation may make 40 MB of pictures (`IMAGE_BUDGET`).
- The panel's header has New chat (`ai.newChat`, `assistant.clear`): a new conversation, disabled
  while Mizan works or the thread is empty.
- Mizan never explains how Qiyas draws or fails unless a tool result shows it, and writes formulas
  as plain text; a stray TeX formula in an answer reads as text anyway (`ai/plainMath.ts`). It
  fixes what it drew by changing it (`set_text`: words, height in metres, rotation, layer;
  `modify`, `erase`), never by drawing it again, never hides layers to hide a mistake, and
  dimensions only with `add_dimension`.
- view_image draws what the engineer sees: `Session::picture_batch` (the sheet drawn afresh for the
  picture's zoom, nothing cached, so the window's batch and curves stay) and
  `render_png_with_text`: solid fills under the lines, the text as its glyphs from the atlas
  (the text boxes left out). Mizan once saw no text at all and drew labels over labels.
- Looking at a user's drawing from the command line: `cargo run -p qiyas-plot --example plot_file
  -- <drawing> <out.pdf>` (fit on A1; `pdftoppm -png` for a picture) and `cargo run -p qiyas-ai
  --example inspect -- <drawing> [tool[={json}] …]` (read-only tools' JSON; a picture tool writes
  `<drawing>.<tool>.png`).
- Mizan's file tools (`qiyas-ai/src/host.rs`, the first six of the catalogue): `list_drawings`
  (tabs and recents), `new_drawing { units }` (ask when not given), `open_drawing { path }`,
  `switch_drawing { id }`, `save_drawing`, `close_drawing { id? }`. They run through the
  `DrawingHost` trait (`Agent::with_host`, `mcp::serve`'s `host`), outside the session's lock,
  implemented by `qiyas-desktop/src/drawing_host.rs` over the tabs. The assistant chose the
  drawing, so its conversation goes on (`tabs::activate`/`add`/`close` `keep_conversation`) and the
  turn's epoch moves to the drawing now on screen; the answer summarises it (the system prompt's
  is the earlier one's). The window follows `drawing:shown`. The user decides what could lose work
  or pick a new file: a path that is not an open tab, a recent drawing or written in the user's
  message (never for MCP) is confirmed in a native dialog; one with an autosave to decide about is
  refused (`script::open`); an untitled or repaired drawing's save is the window's Save
  (`drawing:save`); closing unsaved changes is the window's question (`drawing:close`, keeping the
  conversation). Without a host they answer `NO_HOST`.
- Measuring a drawn thing is by handle (find_entities / get_entity, then `measure_length` /
  `measure_area`): exact for lines, arcs, circles, LWPOLYLINE and heavy POLYLINE with bulges
  (tested in DXF and DWG). Points are for points the engineer gave. A points measurement whose
  extent matches one drawn entity (within a tenth) still measures, but the answer carries `hint`
  and `candidates` (handle, kind, layer, length, area) so Mizan removes it and measures the
  handle. `add_measurement` refuses a no-geometry item of quantity 0 (notes, analysis) unless
  `empty_item` is true; `add_markup` is unaffected.
- Mizan writes into the drawing (`qiyas-ai/src/execute/author.rs`, 57 tools in all): `add_text`
  (TEXT, or MTEXT for several lines or a width; height in metres, default 2.5 mm x annotation
  scale), `room_areas` and `label_spaces` (the smallest closed region round each room-name text
  or seed via `Session::boundary_at`, like Hatch: `enclosed_area_m2` is what the line work
  encloses, columns that overlap a wall are already out, `net_area_m2` also takes the islands off
  (fountain, shaft; `subtract_islands` false keeps them); bulged edges are exact; texts that look
  like areas are skipped so a second run does not label labels; `label_spaces` writes "NN.NN m²"
  just under each name), `add_table` over `Command::Table` (headings + rows of strings) and
  `edit_table_cell`, `draw` (line, polyline with bulges, rectangle, circle, arc, hatch, revcloud;
  drawing units) and `insert_block`. Each call is one undo step through `Command::Batch`; a layer
  that does not exist is made (white) and the current layer and colour are put back. Verified on
  `samples/test-plan.dwg`: living room 58.29 - 4 x 0.0025 = 58.28 enclosed, 51.211 net of the
  fountain; bedroom 22.085; kitchen 17.385 (the 0.4 m columns overlap the inset corners by
  0.05 x 0.05).
- Agent log: `logs/agent.log` beside `settings.json` (mode 0600, moved to `agent.log.1` past
  5 MB). Each conversation's provider, model and address, then every user message, request,
  answer (text, calls, tokens, time), tool run (input, result) and error with the service's
  whole answer, and for a 4xx the request sent (`provider::request_for_log`: compact JSON,
  pictures as `<image>`, tools as their names, the system prompt as its length, scrubbed of the
  key first, then its LAST 4,000 characters after a `…` when longer); other texts clipped at
  4,000 characters, never the key. The panel's Log button shows
  its newest 256 KB (`ai_log`); an error offers "Show log".
- OpenAI-compatible streams: Gemini (also behind Runware) sends parallel calls whole, all at
  index 0 or with none, so a new id, or a second name, starts a new call (`openai.rs`, `slot`).
  Input that was not valid JSON goes back as `{}`, as for Claude, so one bad call can't make
  every later request fail. An assistant message with calls and no text sends `"content": ""`
  (Gemini's endpoint, Runware in front of it, refuses `null`); a tool message holds only `role`,
  `tool_call_id` and string `content`. A call's streamed `extra_content` (Gemini 3's thought
  signature) is kept by call id in the provider and sent back on that call.

## MCP

Outside AI apps can drive Qiyas through its MCP server (the same tools as the in-app assistant,
the window tools included: `view_screen`, `get_selection`, `select_entities`; their measurements
are marked `mcp`).

- Turn it on in Settings, MCP server. It listens on `127.0.0.1:48123/mcp` (port in Settings) and
  needs `Authorization: Bearer <token>`; a request with an `Origin` other than the server's own
  gets 403, so web pages can't use it; the token is in Settings and in `settings.json` (mode
  0600). The dialog shows whether it is listening.
- Settings gives both setups, with the bridge's path filled in:
  - Claude Code: `claude mcp add qiyas -e QIYAS_MCP_TOKEN=<token> -- <app folder>/qiyas-mcp`
  - Claude Desktop (`claude_desktop_config.json`):
    `{"mcpServers": {"qiyas": {"command": "<app folder>/qiyas-mcp", "env": {"QIYAS_MCP_TOKEN": "<token>"}}}}`
- `qiyas-mcp` speaks MCP on stdio and forwards to the app over HTTP. `QIYAS_MCP_URL` overrides
  the address (default `http://127.0.0.1:48123/mcp`). When the app is not running it answers
  "Qiyas is not running or MCP is disabled in its settings".
- Dev: `cargo build -p qiyas-mcp` puts the bridge in `target/debug`, the folder Settings shows
  under `pnpm tauri dev`.
- Release builds: always `pnpm bundle`, never plain `pnpm tauri build`, so the bridge ships.
  `pnpm bundle` runs both steps below.
- Bundling: `scripts/build-sidecar.sh` builds it in release into
  `crates/qiyas-desktop/binaries/qiyas-mcp-<target triple>` (git-ignored), then
  `pnpm tauri build --config crates/qiyas-desktop/tauri.bundle.conf.json` ships it beside the
  app. The external binary lives in that extra config, not `tauri.conf.json`, so plain builds
  and tests don't need the sidecar built first.

## Scripts and the command line tool

- Command scripts (`qiyas-core/src/script.rs`, `.scr`): one command per line, its points on the
  same line or the lines after it; a line starting with `;` is a comment; blank lines are
  skipped; words in any case. Points as the window's command line: `x,y`, `@dx,dy`, `@d<angle`
  (degrees), drawing units, world coordinates, Arabic digits and `٫`/`،` accepted; the last
  point carries across commands for `@`. Double quotes keep a name with spaces together. The
  whole file is checked before anything runs; an error is `line N: …`, and a run stops at the
  first failing step with its line.

  | Line | Does |
  |---|---|
  | `OPEN <path>` | Opens the drawing (reads its `.qiyas` if beside it). The rest of the line is the path. |
  | `SAVEAS <path> [dwg\|dxf]` | Save As; the format word gives a path without extension its extension (a mismatch is an error). Project file saved beside it. |
  | `SAVE` | Saves over the drawing, keeping `.bak` as the app does. Refused on a drawing the failsafe reader repaired on open (SAVEAS to a new path, or `--allow-repaired-save`). |
  | `QUIT` | Ends the run; later lines are checked, never run. |
  | `DELAY <ms>` | Waits (whole milliseconds; it really sleeps). |
  | `LAYOFF <name…>` / `LAYON <name…>` | Hides / shows layers, one step per name. |
  | `LAYFRZ <name…>` / `LAYTHW <name…>` | Freezes / thaws layers. |
  | `LAYER CURRENT <name>` | Makes the layer current. |
  | `COUNT <block>` | `CountBlock` on the sheet (the counts go in the batch report). |
  | `DI` / `DIST p1 p2` | `MeasureDistance`; exactly 2 points. |
  | `AA` / `AREA p1 p2 p3…` | `MeasureArea`; 3 points or more; a blank line or the end of the file ends them. |
  | `MEASURE <handle hex>` | `MeasureEntity` of the drawing's own entity. |
  | `EXPORT [xlsx path]` | The take-off to Excel; bare, `{name}.{ext}.xlsx`. |
  | `IMPORTBOQ <xlsx path>` | BOQ from Excel, then `SetBoq`. |
  | `COMPARE` | `compare_session`, default tolerance. |
  | `ZOOM E\|EXTENTS\|IN\|OUT` | Nothing headless (`W` is refused). |
  | `ECHO <text>` | Prints the text in the report. |
  | `{"type":…}` | Any `Command` in its wire form, e.g. `{"type":"draw","kind":"line","a":[0,0],"b":[0,5]}`; its `path` and `…_path` fields go through the folder rule below and become absolute. |

  Not in the grammar on purpose: `ME` (measure along) and UCS (points are world). Paths are
  relative to the script's folder (written ones, SAVEAS and EXPORT, to the batch's `--out`
  folder when given) and must stay inside the script's folder, the drawing's folder or the out
  folder. `{name}` in a path is the drawing's file name without extension and `{ext}` its
  extension (a bare `EXPORT` is `{name}.{ext}.xlsx`: one workbook per drawing of a batch, even
  for `plan.dwg` beside `plan.dxf`). A UTF-8 byte order mark at the start is skipped. Only SAVE,
  SAVEAS, EXPORT and a JSON `write_block` write; the `.qiyas` beside a drawing is read on open
  and written only by SAVE and SAVEAS. Saves go through `Session::save_as_headless`: the app's
  `.sv$` autosaves and `.qlock` files are never touched, and a drawing open in a running Qiyas
  (a live lock here, or another computer's: `refuse_if_open_elsewhere`, which reads the lock
  only) is never written ("… is open in Qiyas (process N on host): save or close it there
  first"); a dead lock still saves. `Run::warnings` lists the drawings the
  reader repaired (`<path> opened with N problems`).
  Excel and the comparison come through `script::Host` (`qiyas-cli` gives `qiyas-boq` and
  `qiyas-takeoff`); the window runs the same grammar without it (below).
- Run script (ribbon Manage tab, Run script…, or `SCRIPT` / `SCR`; `qiyas-desktop/src/script.rs`,
  IPC `run_script { path }`): the grammar and folder rule above, run on the drawing on screen
  (none open: the script must OPEN one), each step as the window does it. OPEN opens a tab (or
  shows the one open already) and goes first in recents; a drawing with an autosave (its `.sv$`,
  newer or older than it: the reason says which), a crash, or open in another Qiyas
  (`qiyas_core::recoverable`) is refused ("open it from the ribbon first to decide about its
  autosave"), since a SAVE after it would delete that autosave; older copies alone (crash
  copies, ended instances' autosaves) don't stop it, as a save keeps them. The run keeps to the
  tab on screen when it starts, moved only by OPEN: when another tab is on screen before a line
  (a tab click, a closed tab, a file opened from outside), it stops there ("the drawing on
  screen changed: stopped before this line"). SAVE and
  SAVEAS save as the app does, but SAVE (or SAVEAS onto its own file) is refused for a drawing the
  failsafe reader repaired on open (`report.dropped`; the window's Save asks first, a script
  can't; after a SAVEAS to a new path it may); each drawing command is one
  undo step; QUIT ends the script, never the app; DELAY waits off the async runtime, 10 s at most
  a line (`DELAY_CAP_MS`); ECHO writes its text in the log. While it runs the window says
  "Running script… (line N)" (`script:line` events) with Stop (`stop_script`, checked between
  lines and during a DELAY: "stopped"), and the ribbon (`inert`) and the command line wait. Before OPEN takes another drawing on screen, the take-off of the one the
  script changed is saved beside it (the window saves after every command). Refused while a
  block is edited in place (the window says so before asking for the file). The result is
  `{ ran, lines: [{ line_no, text, ok, message }], changed, took_ms }`: the lines run, the failing
  one last; a script that doesn't read is its bad line alone, `ran` false, and the dialog says
  "Line N could not be read; nothing ran". The messages are
  the CLI's (`describe` is a copy of the core's private one). `ScriptResultDialog` lists the
  lines with a mark each, where it stopped and the time; then the window shows a drawing an OPEN
  put on screen as a tab switch does, or reads the one on screen again (info, hidden layers,
  take-off, batch), and saves the take-off.
- `qiyas-cli` (`crates/qiyas-cli`, clap without colour): no window, and never the settings, the
  keychain, MCP or the AI (it depends on qiyas-core, qiyas-boq and qiyas-takeoff only).
  - `convert <in> <out> [--version 2000|2004|2007|2010|2013|2018]`: DWG or DXF by the output's
    extension, in the input's version unless given: the drawing as read and nothing else (no
    project file, no take-off embedded, the source's project never rewritten); `.bak` when the
    output exists; refused when the output is open in a running Qiyas, as SAVE is. A full Save
    As is `run` with SAVEAS.
  - `run <script.scr> [drawing] [--allow-repaired-save]`: prints `line N: <what it did>` per step.
  - `takeoff <drawing> --count DOOR [--count …] [--export out.xlsx]`: adds the counts to the
    drawing's take-off (read from its `.qiyas`, which is not written) and writes the workbook;
    at least one of the two.
  - `batch <folder|set.qss> --script x.scr [--out <folder>] [--jobs N]`: the folder's own
    `.dwg`/`.dxf` files sorted by name (not subfolders; `.bak` and `.sv$` fall out) or the sheet
    set's drawings in its order; the script is checked once before any drawing is opened, then
    read for every drawing, and two drawings that would write the same file (EXPORT, SAVEAS,
    SAVE, `write_block`) are both refused before anything runs (`error output <path> collides
    with <other>`; paths compared in lower case on every system, as Windows and macOS do); one
    line per drawing in that order, `<path> ok COLUMN=6 DOOR=3`, `<path> warning (N problems
    reading it) COLUMN=0 …` (the reader repaired the drawing; the script ran: check its counts),
    `<path> warning (Project file could not be read …) …` (its `.qiyas` is there but unreadable:
    opened with an empty take-off, the file backed up to `.qiyas.bak`) or `<path> error
    <message>`; then `N drawings: A ok, W with
    warnings, F failed`. Jobs 1 to 64 (default 1), each worker with its own session.
    `--allow-repaired-save` as for `run`.
  - `--quiet` prints nothing on stdout but a batch's summary line; `--json` prints one JSON
    document (batch: `{files: [{path, status, problems, counts, error}], total, ok, warnings,
    failed}`).
  - Exit codes:

    | Code | Means | stderr |
    |---|---|---|
    | 0 | done | nothing |
    | 1 | an error: a bad argument, script line or drawing; a batch with a failed drawing | `qiyas-cli: …` (`F of N drawings failed`) |
    | 2 | done, but a drawing opened with problems the reader repaired, or with a project file that could not be read (warnings only) | `qiyas-cli: … opened with problems: check …` |

    A panic (the reader on a damaged DWG) is caught: `crashed: <message>`, and the batch goes on.
  - Damaged drawings (tested): a DWG cut off in the middle does not open at all ("no usable
    drawing data"); junk bytes in the middle open cleanly, open with problems (a warning), are
    refused, or panic the reader (an error).
  - `cargo run -p qiyas-cli -- batch plans --script count.scr --out out --jobs 2`.

## Updates and installers

- Updates: `tauri-plugin-updater` (`crates/qiyas-desktop/src/updater.rs`). The feed is a static
  `latest.json`, an asset of the newest GitHub release (shape: `crates/qiyas-desktop/updater/latest.sample.json`:
  version, notes, pub_date, a `url` and `signature` per platform key `linux-x86_64-appimage`,
  `linux-x86_64-deb`, `linux-x86_64-rpm`, `windows-x86_64-nsis`, `windows-x86_64-msi`,
  `darwin-aarch64-app`, `darwin-x86_64-app`). Its address is `plugins.updater.endpoints` in `tauri.conf.json` and the
  signing public key `plugins.updater.pubkey`. Both are set (the feed is the `latest.json` asset of the newest GitHub release,
  `https://github.com/qs-mind/qiyas/releases/latest/download/latest.json`); a
  `REPLACE-ME` placeholder in either (or a feed that is not `https://`) makes every check refused (`updates_not_configured`, and the
  About box says "Updates are not set up in this build yet"), never a silent failure.
- About box: Check for updates, then Install and restart (download with its progress, signature
  check, install, restart; refused while a drawing has unsaved changes, before the download and
  again before the install). Closing the box (or Escape) during the download stops it
  (`cancel_update`); during the install the box can't be closed. The feed and the bundle fail
  after 30 s without data (connect or read), the feed request after 60 s whole. One install at a
  time (`update_busy`). A deb or rpm install waits for the password while the window stays
  usable: a drawing changed meanwhile keeps Qiyas running (`installed_restart_later`, "Installed.
  Restart Qiyas when you are ready"), and the update starts with it next time. Release builds also check 15 s after launch when
  `last_update_check` in `settings.json` is a day old or more (or the clock went back); every
  check sets it, and the Settings dialog never changes it. An update found then is offered in a
  native dialog in the window's language (Install and restart, Later). Nothing installs without
  a click.
- Keys: the signing key pair was generated on 2026-10-12 on the founder's authority and lives
  outside the repo: `~/.tauri/qiyas.key` (private, mode 0600), its password in
  `~/.tauri/qiyas.key.password` (0600), the public key in `~/.tauri/qiyas.key.pub` and in
  `pubkey`. `.gitignore` covers `*.key`, `*.key.pub`, `*.pem`, `*.p12`, `*.pfx`, `*.p8`,
  `.tauri/`. A lost private key means installed copies can never update again: keep a backup
  of the two files off this laptop. To make a new pair: `pnpm tauri signer generate -w
  ~/.tauri/qiyas.key` (never paste the password into a chat).
- Release (GitHub Actions, `.github/workflows/release.yml`): bump the version in
  `crates/qiyas-desktop/tauri.conf.json`, `package.json` and `Cargo.toml`, move `CHANGELOG.md`'s
  Unreleased notes into a `## [<version>] - <date>` section (it becomes the release notes),
  commit to `main`, then `git tag v<version> && git push origin v<version>` (the founder's act; a
  tag ruleset lets only the repository admin create, move or delete `v*` tags). `prepare`
  refuses: versions that differ between the three files or from the tag, a tag whose commit is
  not on `origin/main`, a tag whose release is already published, a missing CHANGELOG section,
  and (in `qs-mind/qiyas`) missing signing secrets. Then it builds Linux (ubuntu-22.04:
  AppImage, deb, rpm), Windows (NSIS, MSI) and macOS (macos-latest: aarch64; macos-15-intel:
  x86_64, the Intel image after macos-13 was retired) with `tauri-apps/tauri-action`, signs them
  for the updater and uploads the bundles, their `.sig` files and `latest.json` (NSIS preferred
  for Windows) to a DRAFT release `Qiyas <version>`. The `feed` job checks `latest.json` has all
  seven platform keys, each signed, each url an asset of that draft: tauri-action v1 writes API
  asset urls (`https://api.github.com/repos/qs-mind/qiyas/releases/assets/<id>`), which the
  updater downloads with `Accept: application/octet-stream` (GitHub redirects to the file;
  unauthenticated, 60 requests an hour per IP address). Then a human reviews the draft on
  GitHub and clicks Publish: only a published release is what `releases/latest/download/latest.json`
  serves. Signing uses two secrets of the GitHub environment `release` (deployment policy: tags
  `v*` only, so no branch or pull request can read them), which the founder sets himself (never
  an agent): `gh secret set --env release TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/qiyas.key` and
  `gh secret set --env release TAURI_SIGNING_PRIVATE_KEY_PASSWORD < ~/.tauri/qiyas.key.password`.
  In a fork without them a tag builds unsigned (no `.sig`, no `latest.json`) into a draft marked
  prerelease, which never becomes "latest". To build a tag's draft again run the workflow from
  the tag itself (`gh workflow run release.yml --ref v<version> -f tag=v<version>`); with no tag
  (or on a pull request that changes `release.yml`, `tauri*.conf.json` or `scripts/*.sh`) it is
  a dry run: unsigned bundles as workflow artifacts, no release.
- Release by hand (fallback): `export TAURI_SIGNING_PRIVATE_KEY=~/.tauri/qiyas.key
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/qiyas.key.password)" && pnpm
  bundle:signed` (the password never typed or in shell history) adds `tauri.updater.conf.json`
  (`createUpdaterArtifacts`), so each bundle gets a `.sig` beside it. Then `scripts/release-feed.sh
  ["notes"]` writes `target/release/bundle/latest.json` from the `.sig` files (download address
  `https://github.com/qs-mind/qiyas/releases/download/v<version>/`). Publish a GitHub release
  tagged `v<version>` with the bundles, their `.sig` files and `latest.json` as assets
  (`gh release create v<version> target/release/bundle/{appimage,deb,rpm}/* target/release/bundle/latest.json`);
  the newest release must always carry a `latest.json`. Plain
  `pnpm bundle` needs no key and makes unsigned bundles. The Tauri CLI (`@tauri-apps/cli`
  2.12.1) doesn't write the version into the signature yet, so `requireSignedVersion`
  (downgrade protection: whoever controls the feed host could offer an older signed build as
  newer) stays off; turn it on with the first CLI that writes `version:` into the trusted
  comment.
- Single instance: `tauri-plugin-single-instance`, the first plugin (`launch.rs`). A second
  Qiyas hands its arguments to the running one and quits; existing `.dwg`/`.dxf` paths (relative
  to its folder, or `file://` addresses) are kept, a `.qiyas` gives its drawing (the one it
  names, else the same name with `.dwg`/`.dxf`), anything else is ignored. The window takes them
  (`take_open_requests` after `open:requested`, `src/shell/openRequests.ts`) and opens each
  through `openPath`, so a newer autosave is asked about first, and comes to the front. Files of
  the first launch go the same way; paths are tidied (`.` and `..`, no links followed), and an
  argument that is not text is left out. A debug build uses its own D-Bus name on Linux, so
  `pnpm tauri dev` starts beside an installed Qiyas; on Windows and macOS it hands its arguments
  to an installed Qiyas that runs and quits. On Wayland (Hyprland) the focus request may only
  flash the window. Tested by hand only.
- File associations (`bundle.fileAssociations`): "Qiyas drawing (DWG)" `.dwg` (`image/vnd.dwg`),
  "Qiyas drawing (DXF)" `.dxf` (`image/vnd.dxf`), "Qiyas project" `.qiyas`
  (`application/x-qiyas`, owned; macOS exports `com.qsmind.qiyas.project`). Ruling: Qiyas never
  takes `.dwg`/`.dxf` away from AutoCAD. macOS ranks them `Alternate` and Linux adds Qiyas to
  "Open with"; Windows' NSIS and MSI make every association the default, so
  `tauri.windows.conf.json` (merged on Windows, replacing the list) keeps `.qiyas` only. Linux deb and rpm use `linux/qiyas.desktop` (`Exec` with `%F`,
  which Tauri's own template lacks, so a double click passes the file; the AppImage is built
  from the deb's files; it registers nothing unless appimaged or AppImageLauncher integrates
  it) and install `linux/com.qsmind.qiyas.xml` (the `.qiyas` MIME type).
  macOS sends files as an `Opened` event (`launch::os_opened`). No DWG thumbnails.
- Installers: `pnpm bundle` on each OS builds that OS's own (no cross builds), into
  `target/release/bundle/`. Run `scripts/pdfium.sh` first on each machine.
  - Linux (here): AppImage, deb and rpm.
  - Windows: MSI (WiX) and NSIS `-setup.exe`. Needs the Rust MSVC toolchain, Node and pnpm, and
    Git Bash for the scripts; the Tauri CLI downloads WiX and NSIS itself.
  - macOS: `.app` and dmg for the Mac's own architecture (`--target aarch64-apple-darwin` or
    `x86_64-apple-darwin`, with `scripts/build-sidecar.sh <triple>` first, for the other). Needs
    the Xcode command line tools.
  - Unsigned installers make Gatekeeper (macOS: Apple Developer ID and notarisation) and
    SmartScreen (Windows: a code-signing certificate) warn on first open; buying those is the
    founder's decision.
- Licences: `tauri.bundle.conf.json` ships `LICENSES/` as the resource `licenses/`, with the
  Noto Sans, Noto Sans Arabic and Instrument Sans OFL texts added to it. `LICENSES/README.md`
  says what each text covers and where the MPL-2.0 sources are (opencadcodec, opencadkernel at
  their pinned commits; the crates.io ones). `LICENSES/THIRD-PARTY.md` lists every Rust crate
  (normal dependencies on Linux, Windows and macOS) and the window's JavaScript packages with
  version and licence; `scripts/third-party.sh` writes it (offline, from the lock file and
  `node_modules`): run it after a dependency changes and before a release, and commit it.
- The Thmanyah font files are built into the app by Vite when they are in `src/fonts/thmanyah/`:
  installers built locally embed Thmanyah, while installers built on GitHub use the system's
  Arabic font, because the Thmanyah files are not on the runners. Which to ship (and whether
  the Thmanyah licence allows embedding it) is the founder's decision.

## Where the plans live

In this repo, `docs/` (write new specs, plans, checks and reports here, never in the workspace):

- Spec: `docs/superpowers/specs/2026-10-08-qiyas-design.md`
- Plan (Phase 1a): `docs/superpowers/plans/2026-10-08-qiyas-foundation.md`
- Plan (Phase 2, AI and BOQ): `docs/superpowers/plans/2026-10-08-qiyas-phase2-ai-boq.md`
- Plan (Phase 1b, drawing I/O): `docs/superpowers/plans/2026-10-08-qiyas-phase1b-drawing-io.md`
- Plan (Phase 3, edit and annotate): `docs/superpowers/plans/2026-10-08-qiyas-phase3-edit-annotate.md`
- Plan (Phase 4, output): `docs/superpowers/plans/2026-10-08-qiyas-phase4-output.md`
- Checks: `docs/2026-10-08-qiyas-foundation-check.md`, `docs/2026-10-09-qiyas-phase2-check.md`,
  `docs/2026-10-09-qiyas-phase1b-check.md`, `docs/2026-10-09-qiyas-phase3-check.md`,
  `docs/2026-10-12-qiyas-phase4-check.md`, `docs/2026-10-19-qiyas-units-mizan-check.md`
- Deferred (Phase 3, left for later): `docs/2026-10-09-qiyas-phase3-deferred.md`
- Deferred (Phase 4, left for later): `docs/2026-10-12-qiyas-phase4-deferred.md`
