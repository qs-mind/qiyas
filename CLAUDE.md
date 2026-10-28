# Qiyas

Qiyas (قياس) is a desktop measurement CAD for quantity surveyors, part of QS Mind. It opens DWG and DXF drawings, lets the QS measure and edit, and uses AI to build and check BOQs against the drawing. Public repo (`qs-mind/qiyas`, Apache-2.0, `LICENSE`). Stack: Tauri 2, React, TypeScript, Vite, Rust.

The engine is general; the product focuses on construction. Checks, verification and drafting
conventions must work on any drawing. Trade rules belong in optional packs: construction first,
interiors and fit-out next, MEP and structure later. No plane-specific or mechanical-specific
features, and no general CAD marketing. See `docs/2026-10-20-qiyas-scope-decision.md`.

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
    qiyas-ai        three generic AI tools over the command registry, the agent loop, the Claude and
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
in `src/viewport/`, the tools in `src/tools/`; `src/app/App.test.tsx` checks the wiring. It uses condition waits and a file-only 30-second
test timeout because it renders the whole shell; the five-repeat load check uses two workers. The
three late refs are `measureRef` (`useSheets` cancels the tool when the sheet changes), `applyRef`
(`useBlocks`, `useAnnotate`) and `onCommandRef` (the tool palette in `useBlocks`); they are called
with `?.`, so a missing fill still type-checks, and `App.test.tsx` has a test for each.

Mizan's waiting (the "Working…" line and a running tool card) is the Qiyas mark
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
- `@base-ui/react`: Tabs (the ribbon's: one `Tabs.Indicator`, moved by a CSS transform, is the
  chosen tab's face; there is no side tab strip any more, see W4); Tooltip (`shell/Tooltip.tsx`: the
  `Tip` wrapper keeps its API, the delay is 400 ms, the next card shows at once within 300 ms, and a
  click's focus shows none); Menu (`shell/RibbonFlyout.tsx`: a group's "⋯" and the tabs' "More"; the
  Artifacts menu and the artifact head's switch are Menus too); Context Menu (`viewport/CanvasMenu.tsx`:
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
  box with no layout keeps its initial size), sections (`section`, rows sorted inside them), content widths from cached canvas measurement of the real font
  in headers and rendered cells (minimums and a 360 px default cap), and `grow` columns sharing
  the room left over. Long text wraps without a line limit; `measureElement` sets each row's height
  from its content (27 px is only an initial estimate/minimum). Remembered user widths override
  automatic widths, including growing columns; double click clears the override. Narrow panels
  keep values complete and scroll sideways when needed. The arrows, Home, End and Enter work
  on rows, and row buttons (`actions`) have a reserved final column, visible on hover or focus.
  `--row-hover` and `--row-current` are solid colours.
- `@headless-tree/react`: the Layers panel starts as a flat list, regardless of its length.
  "Show folders" is an explicit, remembered choice (`qiyas.layers.folders`, false by default).
  Folders follow name prefixes and external drawings, with a folder icon and a labelled layer
  count; no discipline list is hard-coded. Collapsed folders remain in `qiyas.layers.collapsed`.
  Rows reserve space for actions, do not select text, and allow one selected row. The footer
  wraps and the left column cannot scroll sideways. Arrow keys follow the UI direction: Left
  opens a folder in Arabic, Right opens it in English. Property controls keep their own keys.
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
  kept on this computer), not `react-resizable-panels`. Mizan's column starts at 440 px (672 px while an
  output is open, its own remembered width), 360 and 560 px in a window under 1300 px; the layers
  panel 260 px.
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
- Thmanyah font files are never committed in readable form (founder's ruling, 23 Oct 2026: the whole of Qiyas stays in this one repo, and installers built on GitHub include Thmanyah). Only `assets/fonts/thmanyah-sans.sealed` is committed: the three Sans weights the window uses, AES-256-GCM sealed by `scripts/fonts-seal.mjs`. Its key is the repository secret `THMANYAH_FONTS_KEY` and, on the founder's computer, `~/.config/qs-mind/thmanyah-fonts.key` (keep a backup; to re-seal, run `node scripts/fonts-seal.mjs seal <key file>`). The release workflow unseals it before building. Locally `scripts/fonts.sh` copies them from `~/Downloads/Thmanyah-Font-Family/thmanyah typeface` into the git-ignored `src/fonts/thmanyah/`. Instrument Sans (OFL) is committed. The viewport draws drawing text with Noto Sans and Noto Sans Arabic (OFL, unhinted Regular, with their licences in `crates/qiyas-render/fonts/`), behind the drawing's own TrueType font when the computer has it.
- English text says "Qiyas" and "QS Mind"; Arabic text says "قياس" and "كيو إس مايند". Arabic uses Thmanyah Sans; headings use `font-feature-settings: "swsh"`.
- Measured values are stored in drawing units and displayed in metres, square metres and pieces. Unit factor comes from the drawing header (`insertion_units`): 1 = inches, 2 = feet, 4 = mm, 5 = cm, 6 = m, 0 or unknown = treat as mm and show a warning in the status bar.
- Every mutating command is undoable. Undo and redo stacks live in `qiyas-core`.
- Brand colours: Ink `#0E0F12`, Stone `#F2EEE7`, Paper `#F9F8F4`, Slate `#6E6A64`, Qiyas face anodised titanium `#C9A6E0` to `#6E4A9E`.

## Working here

- Use pnpm, not npm.
- Agent skills live in `.claude/skills/`; every Qiyas job loads the ones its work needs (the map is
  in `.claude/skills/README.md`: speed, Rust, units, tests, UI, Mizan, security, review).
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

## Command registry

- `qiyas-core/src/registry.rs`: every `Command` describes itself from its JSON schema (schemars
  1.2, MIT: `JsonSchema` on `Command` and every type it holds). Each entry: the wire name, category,
  summary and doc (the doc comment), effect (read, view, edit, file, session), the undo part
  (`changes()` on its examples: takeoff, boq, conflicts, drawing, drawing+tables…), destructive,
  batchable, internal, needs (pdf, clipboard), a self-contained schema (definitions inlined,
  number formats left out; `batch`'s commands left open) and examples. A command whose payload is
  a tagged enum lists its kinds, each an entry of its own: `draw.line`, `modify.move`,
  `edit.offset`, `dimension.linear`. `registry()`, `get` (any case, `-` as `_`), `search` (the
  name, doc and the schema's property names, values and descriptions; internal ones left out),
  `index_text` (one line per category, about 2,000 characters, deterministic: the AI's index) and
  `effect_of(&Command)` (a dry-run purge reads, a real one edits).
- A new command needs a doc comment, `#[schemars(extend("x-category" = "…"))]` and an example
  (`#[schemars(example = json!({…}))]`), and each kind an example; `tests/registry.rs` fails
  without them, and checks that every example reads back as its command, every schema is
  self-contained and names its type, and that a command that reads leaves the session as it was.
  Other extensions: `x-effect` ("view", "file", "session" for one that is not an undo step),
  `x-destructive`, `x-batch` false, `x-internal` (hosts only: `set_boq`, `set_conflicts`),
  `x-needs`. Command-line words live on the command or kind variant too:
  `#[schemars(extend("x-words" = ["LINE", "L"]))]`, upper case, the full word first.
  `CommandInfo::words`, `Registry::by_word` and `Registry::words` expose them; the guard
  refuses a word owned by two commands. `x-prompts` overrides argument order or prompt text.
- `validation.rs` checks raw inputs against those schemas before execution, collecting missing
  fields, wrong types, enums, constants and bounds together with JSON pointers. Unknown fields
  on edits are errors; read commands return warnings. The existing serde path error, schema and example remain available.
  `Session::run_input(&Value, Source)` is the JSON execution boundary: it validates the raw
  input, deserializes a plain `Command`, resolves omitted defaults, then executes. Its separate
  `Reported` struct holds `outcome: Outcome`, `defaults_used: [{field, value, unit}]` and
  `warnings`; JSON keeps the ordinary outcome fields with metadata beside them. `Command`
  uses derived serde and both enums can be matched directly. Native Rust callers keep using
  `run` or `run_as`. Raw batch steps pass through `run_input` in order within one transaction,
  so each step's defaults use its actual sheet, scale and geometry. The aggregate report uses
  `/commands/i/…` pointers and the wire batch also includes each step's own metadata.
  The command-line machine exposes its original completed input through `take_input`; script
  steps keep that raw input beside their plain command. Desktop IPC, scripts, typed commands
  and the shared AI/MCP surface all execute wire commands through `run_input`.
- Serde defaults appear in each field's registry schema and description. Conventional lengths
  are resolved using drawing units and the sheet's annotation scale; the result says which
  values were chosen. Text rotation is zero, MText width zero means no wrapping, polygons are
  inscribed, and hatches are solid when omitted. Geometry that defines a shape remains required.
  `input_context_defaults.rs` shares read-only runtime calculations for grid extents, room
  label heights/languages, viewport fitting, clipboard bases and dimensional expressions.
  Per-room defaults carry `{index, name, value}` entries. Revision clouds and comments share
  a default arc length of one thirty-second of their outline; explicit zero is refused.
  Custom hatch rotation/scale default to 0/1; image brightness/contrast/fade to 50/50/0.

## Records

- `qiyas-core/src/records.rs`: every object as a record, `{key, type, …summary, data}`, where
  `data` is the object as opencadcodec serialises it (an entity without its variant wrapper:
  `/common/layer`, `/radius`). `Command::Records { query }` (`Session::records`,
  `Outcome::Records`): collections entities (a sheet, the one on screen by default, or `block`;
  an xref's entities carry `xref` and `chain` and are read-only), layers, linetypes, text and dim
  styles, blocks, layouts, viewports, groups, views, header, layer states, xrefs, take-off, BOQ,
  conflicts, schedules, comments, constraints, parameters; filters handles, names, type, layer,
  sheet, bounds and `where` (eq, ne, in, not_in, lt, le, gt, ge, contains, exists on a summary
  name or a JSON pointer into `data`); `fields` projects; `detail` summary or full. Pages of 20 (200 at most) with a 12 KB budget for records and warnings, excluding the page envelope
  (`MAX_BYTES`, `truncated`; one oversized record can exceed the budget); arrays past 100 items say how many they left out.
- `qiyas-core/src/record_edit.rs`: `Command::SetRecords { collection, updates: [{key, path,
  value, expected}] }`: serialise, set the JSON pointer, read back with `serde_path_to_error` (the
  error names the field), `entity_from_record` puts back what serde skips (and a block
  reference's attributes'); `expected` refuses a record changed since it was read. Entities
  (and viewports) on a sheet's top level, not locked, layers (not an xref's; the current one
  can't freeze), text styles, and the header variables in `HEADER_WRITABLE` (kept in the undo
  step's `HeaderBits`). Refused, each naming its command: identity and ownership, extended data,
  a text's value (edit_text), a block reference's place and attributes (modify, set_property,
  edit_attributes), a dimension's block (dim_edit), a table's cells (edit_cell), a layer's name
  (rename_layer), units (new_drawing), dimension variables (set_dim_style), other collections
  (`collection_via`). One undo step (tables and take-off kept); dimensions and tables are drawn
  again, the take-off and fields follow, an edited entity's constraints go (`Reshape::Edited`).
- `Command::CreateEntities { entities: [{type, data}], sheet }`: the type's default
  (`default_entity`, `CREATABLE`), the current layer, colour, linetype and lineweight, then `data`
  merged over; a layer it names that is missing is made (white); texts are marked as Qiyas's;
  dimensions, tables, viewports, references, pictures and attributes are refused with their
  command. `Outcome::Added`.
- `Command::RecordSchema { name }` (`record_schema`): every path of an entity type, "layer",
  "text_style" or "header" (four levels deep) with its value type, `write: false` with `why` and
  `via` where refused, the shapes of the named types (enums as their variants: `Color` is
  `"ByLayer" | … | {"Index": …} | {"Rgb": {…}}`), whether the type can be created, and that
  angles are radians there. Traced once off the types' serde (`serde-reflection`, MIT OR
  Apache-2.0), explicitly seeding the codec's nested enums before tracing record roots. Dimension
  paths retain their variant, e.g. `/Linear/base/text_middle_point`, matching its record data.
- `record_input_schema` caches those traced types as input schemas too. Creation and block
  definition check all supplied data fields before merging the entity defaults. Record edits
  stage each replacement without mutation, including sequential `expected` values, and collect
  type, unknown-field and path problems. Required fields in a replacement object are checked
  against the codec's actual serde by omitting each field from a valid record; serde defaults
  and optional fields remain optional.

## Undo

- `qiyas-core/src/undo.rs`: one step per command, at most 200, each with its `Source`
  (`undo_if` lets the AI undo only its own) and label (`Command::label`: "draw.line",
  "set_layer"; `Session::history_steps`). Take-off, BOQ and conflict steps keep that part whole;
  one whose command fails half way is put back.
- Transactions (`qiyas-core/src/transaction.rs`): `Session::transaction(source, label, f)` runs
  `f`'s commands as one step: each `run_as` records its own step into an open group
  (`History::begin_group`), which closes into one `Step::Group` (held last first, so undo runs
  them backwards and redo forwards; a group of one is that step). On failure `abort_group` puts
  every step back, last first, and the view state (sheet, hidden layers, current, drafting, UCS,
  isolation, layer states' hidden layers) is restored, with `document_revision` as it was. Not one
  outer recording: the codec's change recorder can't nest. Transactions nest; inside one, commands
  the registry marks `x-batch` false are refused. `Session::last_change()` (`Change { added,
  changed, removed }`): what the last command or transaction did to the entities (drawn and erased
  in one transaction: in none).
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
  Picking and drawing checks place approximate text bounds with the same alignment, MTEXT
  attachment and paragraph spacing as the drawing, including rotated text and attributes;
  wrapping uses estimated character widths.
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
  a command lists the matches in a cmdk list above the line (see the UI kit). IPC `command_words`
  supplies the core's words, aliases and summaries; the window adds its own tools and actions.
  Its existing parser keeps click-driven tools and their options. Unknown words go through
  `cmdline_step { line }` to `qiyas-core/src/cmdline::Machine`, with the current UCS and last
  point. A completed command uses the window's usual apply path; an incomplete one shows its
  next prompt. The pure machine reads points, numbers, names and options from the registry
  schema, with `x-prompts` when a command needs a different order or prompt. A new core word
  requires no window parser change.
- Pastes into text fields (Ctrl+V, Ctrl+Shift+V, Shift+Insert) go through `src/shell/safePaste.ts`
  and IPC `clipboard_read_text` / `clipboard_read_image` (arboard), never the web view's own
  clipboard read: WebKitGTK 2.54 on Wayland hung and then crashed the app there. The field gets a
  paste event (Mizan's composer takes pictures from it), then the text typed in as one edit. The
  web view's right-click Paste is not covered. Keys outside text fields stay PASTECLIP.
- Native file dialogs all go through `src/fileDialogs.ts`. Open and Save start in the folder
  of the drawing on screen (never an untitled placeholder), then the last chosen folder in
  `localStorage`, then the user's Documents folder from Tauri. Suggested filenames are kept.
  This applies to drawings, PDFs, images, scripts, exports, blocks, libraries, sheet sets and
  plot styles. A Vitest guard prevents direct dialog-plugin imports elsewhere.
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
  points); points obey PDMODE (dot, hidden, plus, cross, tick, circle and square combinations) and PDSIZE (drawing size, or a percentage of sheet size). Splines flatten from control
  points and knots (the curve through the fit points when there are none). Measuring sees
  outlines only (`FlattenOutcome::into_paths`).

## Data-defined elements (Phase 16)

- `knowledge.rs`: versioned JSON XRecords in `NOD/QIYAS_KNOWLEDGE`, with subdictionaries
  types, levels, tables, rules, sheets, mappings, views and measurement. Objects and the NOD
  pointer participate in undo; stale DWG raw payloads are cleared on write. Knowledge and
  metadata JSON is written ASCII-only (`xdata::ascii_json`, `\uXXXX` escapes): DWG 2000 and
  2004 keep text in a code page and turn other characters into `\U+XXXX`, which broke the JSON
  on reopen (the clinic run lost its method, mappings and results that way); reads decode
  those escapes from older files (`decode_legacy_unicode`), tested in every DWG version and
  DXF with Arabic. `set_knowledge` from Mizan or MCP refuses the computed entries
  `revisions/{result,baseline,identities}` and `queries/result`: they come from
  `carry_knowledge`/`variation_report` and `discrepancy_check`. Qiyas's own writes of those
  (`discrepancy_check`, `resolve_conflict`) run the inner `SetKnowledge` as `Source::User`
  inside the agent's transaction, which keeps the agent's source for undo. `xdata.rs` shares
  UTF-8 chunking and application-specific raw EED invalidation among walls, rooms and fields.
- `objects.rs`: `define_object_type`, `redefine_object_type`, `delete_object_type`,
  `place_objects`, `set_object_props`, `object_schedule` and `set_knowledge`. Type properties,
  geometry recipes and quantity expressions are data. Physical property values use metres,
  m², m³, degrees or counts; driver coordinates use drawing units. `unit` converts metres to
  drawing coordinates in symbol recipes. Records expose `object_types`, `objects` and
  `knowledge_<kind>`. Generated graphics carry `QIYAS_OBJECT_PART` owner/role links; drivers
  carry `QIYAS_OBJECT`. Generic part move/copy/erase resolves to the driver.
  Point symbols retain rotation and reflection through DWG/DXF. Scaling a point driver moves
  its insertion; explicit physical dimensions and their quantities change through properties.
  Union bands share graphics within the same type, sheet and level; automatic groups also
  share a vertical span, while an explicit assembly supports mixed heights. The shared band
  service integrates volume by vertical span and applies hosted cuts. Deleting an owner transfers
  shared graphics to a survivor so quantity and field references follow. Justification, role
  layers and hosted offsets/reflection are recipe data. Property bindings can read `host.*`.
  The existing geometry kernel refuses overlapping parallel runs in a union group; failed
  placements roll back atomically.
- `derived.rs`: one ordered derivation list inside `record_document_change` (objects, views,
  schedules, BOQ rows, fields). Phase 18 fills the BOQ slot; views remain reserved. Object
  schedules are CAD field tables that follow edits and grouping in the same undo step.
- Expressions now include powers, comparisons, logic, typed strings/booleans, lazy `if`,
  `min/max/round/ceil/floor` and resolver lookups; the numeric constraint API is preserved.
  Arabic digits inside strings remain literal. `geometry.*` is separate from named quantities.
- Legacy wall, room and opening definitions are stored in `data/legacy-object-types.json`.
  The converter runs on open and edits, retaining visible geometry and old quantity behavior.
  Door/window symbols use the stored recipes; redefining them repoints their existing inserts
  to regenerated symbol blocks, retaining placement, schedules and references.
  Wall commands translate old arguments into ordinary typed drivers and shared band rendering;
  legacy tags remain for old API/field compatibility. Rooms use the shared seeded-region source
  (`QIYAS_OBJECT_REGION`), with holes and validity; invalid regions expose no usable quantity.
  Old room/opening schedule commands select JSON recipes for the shared schedule renderer.
  Headers, templates, decimal precision, totals, explicit IDs and default layers are data;
  manual room overrides remain detectable until the source quantity changes.
- Details and verification: `docs/2026-10-21-qiyas-phase16.md`. No new dependencies.

## Drawing recognition and documents (Phase 17)

- `qiyas-core/src/recognition.rs` and `recognition/`: the `recognition` command family
  defines/lists/tests/applies drawing-owned rules in knowledge kind `recognition`. Selectors
  cover globs/regex, layers, blocks, attributes, hatches, closed regions, text and colour.
  Extraction reads attributes, note/section captures, parallel-line distances and geometry.
  Accepted candidates become ordinary Phase 16 instances. Missing evidence, rule conflicts
  and low confidence stay in persistent review metadata (`QIYAS_RECOGNITION`). Each mutation
  is one undo step. Corrections can save scoped or explicitly generalised rules as data.
- Drawing tables, text grids and attribute blocks become type/rule definitions through an
  explicit column mapping. Calibrated PDF imports retain `QIYAS_SOURCE` path/page evidence;
  vision hints snap to CAD geometry or raster ink, with raster interpretations requiring review.
  Recognition uses types with empty plan recipes to retain the source graphics. It currently
  operates on the drawing's own top-level entities; xref content remains read-only.
- The take-off report (Outputs menu, "Take-off report"; `RecognitionPanel`) accepts, fixes or rejects candidates in English and Arabic.
  `qiyas_read document {path,pages?}` reads plain PDF text through the pdfium worker without
  changing a drawing. `qiyas_capture` source `document` renders a PDF page with a coordinate
  caption. The composer accepts PDFs up to 20 MiB under the Qiyas data home's `attachments/`;
  debug attachments and all development recipes/reports stay in `.local/`.
- `qiyas-cli bench recognize --case ID --plan FILE --output FILE` computes submissions from
  recognition recipes. `bench run` uses case-local `recognition.json` where available. Rules
  and quantity mappings are data; prediction code never reads gold quantities. Scoring rules
  are unchanged, and PDF citations do not claim to reproduce original CAD handles.
- The Phase 17 dev run improved the original 956-case cohort from 65.03% to 70.97%
  coverage with 100% matched accuracy within 1%; the current 1,099-case set reaches 71.59%.
  All 120 PDF variants produce scored submissions, but each currently covers selected
  measurements. Independent 95% QS recall and live vision validation remain outstanding.
- Workflow, schema, limits and verification: `docs/2026-10-22-qiyas-phase17.md`.

## Measurement methods and generated BOQ (Phase 18)

- `measurement.rs` / `measurement/evaluate.rs`: user-supplied, cited rules for types/trades,
  timesing and factors, hosted opening/void thresholds, boundary exceptions, unit bands,
  per-object rounding and coverage notes. No standard or trade thresholds ship with Qiyas.
  `measurement.set_method` snapshots Settings/project scopes in drawing knowledge; project
  takes precedence. `measurement.set_mappings` supplies bilingual description/code templates,
  sections, trades and units. `measurement.report` reads quantities, BOQ and dimension sheets.
- The derivation hook refreshes the generated result with every drawing edit in its undo step.
  Lines retain expressions, operands, inclusion decisions, method/clause/source and handle/sheet/
  assigned grid. Missing grids and uncovered objects are reported. Invalid or unreviewed objects
  withhold quantities. All data round-trips with the DWG/DXF. No manually typed generated quantity.
- Settings selects method names: POMI, NRM2, SMM7, CESMM4 (never with the publishing body's name:
  `measurement::canonical_method_name` takes the body out, bracketed or as a bare word, as the window's `plainMethod`; settings load migrates a saved
  "POMI (RICS)" to "POMI" and writes it back; the two names are one method when matched
  (`same_method_name`: an old drawing keeps its rules), and the report, exports and the name told
  to the model show the bare one), local practice in
  Egypt/KSA/UAE, Other (free text), or None (geometric quantities). Labels are English/Arabic;
  saved `measurement_method` is the name string (null for None). Optional `methods/*.json`
  files remain extra name choices. Files are matched by their declared name, not filename.
  Missing files produce a name-only selection with no bundled rules. Invalid matching files
  and duplicate names still fail validation. Settings applies changes transactionally to tabs.
  Opening a tab preserves drawing-authored rules for the same name; project override wins. Project `.qss` runs apply their method override to every member before deriving quantities. The BOQ panel exposes the
  generated items and dimension source links; the take-off export adds bilingual BOQ, dimension
  and review worksheets. Comparison adapts generated quantities without replacing the imported BOQ.
- `recognition.pdf_text` calibrates vectors from a label regex and a ticked scale bar. General
  label/legend rules live as data; ambiguous calibration and scans stay in review.
- `bench run --mode prepared|autonomy`: prepared case recipes versus shared rules only. Autonomy
  never opens case recipes. Both reports show item coverage, precision, F1 and extras per case
  and overall, plus PDF totals, in `baseline-<mode>.json/.md`. Rules/runs stay in `.local/`.
- Algorithm performance guards use `tests/support/performance.rs`, a fixed integer sort workload.
  The million-segment guard also checks that its ratio budget would reject a 4x slowdown.
- Schema, verification, benchmark comparison and limits: `docs/2026-10-22-qiyas-phase18.md`.

## Mizan autonomous take-off (22 October 2026; before Phases 19–23)

- Founder ruling: Mizan works from a request to the completed take-off alone. Human review is
  optional. The headline is **MIZAN ALONE** through live `qiyas-cli eval`; prepared and shared-rule
  engine benchmarks are separate diagnostics, never claims about autonomous model performance.
- `qiyas_read digest {offset?}`: all-sheet units/scale, sheets, layer/kind counts, block inserts
  and attribute tags, spatial text/table evidence, closed-area statistics, hatches, knowledge and
  take-off counts, selected method and a suggested picture. Pages are at most 6,144 JSON bytes;
  `next_offset` continues them. Text previews are bounded; records retain the full source.
  Text clusters are geometric hints, not semantic assertions about a legend or a room.
- `commands {names:[...]}` fetches several schemas in one call. The registry index generates its
  take-off workflow from command/kind `x-takeoff-step` schema metadata. The three tools remain.
- `takeoff_workflow.txt` and its Arabic counterpart guide digest/picture for quantity jobs, batched type/rule authoring, recognition, Settings
  method (project override wins), mappings, self-check and repairs by handle. No case names,
  trade lists, standards text, thresholds or rates are supplied. A named method without rules
  tells Mizan to write rules from its knowledge in its own words, with clause references, through
  `measurement.set_method` at the selected scope. It must apply the method's units, deductions,
  conventions and descriptions and disclose uncertain rules/references in its end report.
  It must not copy standard text or invent clauses. None uses cited geometric quantities.
  Agent opening state and shared MCP state/digest/kit use drawing knowledge first. A pending
  named kit batch has an empty rule set to fill; a loaded kit omits method replacement and
  instructs reuse. No geometric example is relabelled as a standard rule.
- `qiyas_read takeoff_check {offset?,expectations?}` reports per-layer and per-block explanation,
  closed-region coverage, unexplained handles, unmapped tables, unmatched schedule rules,
  uncertainties and verify results. It reads only. These are evidence shares, not truth coverage.
  Unexplained annotations can keep the check incomplete; Mizan must disclose that at the budget
  limit. Multi-sheet recognition evidence is used by measurement derivation and submissions.
- Autonomous is the default; saved per-job/per-phase choices still apply. Autonomous runs report
  introduced drawing errors at completion without an approval pause. The Review panel is a
  take-off report with optional corrections, including accepted low-confidence interpretations.
- Eval submits accepted measured dimension items and their generated BOQ totals. Raw
  manual rows/candidates are excluded. An object with a recognition candidate must be accepted;
  an object Mizan placed itself (a design drawn from scratch) has no candidate and counts.
  Engine recipes likewise require accepted elements;
  legacy `include_baseline` no longer appends raw geometry. Shared development mappings live in
  `.local/benchmark/general/`, never in the product or live/scripted Mizan input. PDF label areas
  become typed recognised enclosures before their explicit mapping is submitted.
- `eval --cases ID ID ...` or `--set live-dev --total-budget-usd N` writes one combined report.
  Each case retains `agent.log` (every step, no rotation), `steps.jsonl` (requests, trimmed tool
  results, usage, reservations and cost), BOQ/dimensions, drawing and self-check. Reservations
  cover the next request only: UTF-8 input bytes plus protocol/image allowance and output cap;
  actual usage settles the ledger. Unknown usage retains the reservation and stops the case.
  Unused case allocations carry forward; the total and daily caps both apply. Selected Settings
  method names and optional files are applied to the isolated evaluation drawing. No case recipes are supplied.
- Frozen sets live in `.local/benchmark/sets/`. `live-dev` selects the first lexical clean case
  in apartment, office, warehouse, MEP and external, plus the first lexical PDF case. `quick`
  round-robins project-type × variant strata, SHA-256 case-ID order within each, up to 120 cases.
  `bench run --set quick --mode prepared|autonomy` forces one repetition. Existing full reports
  provide the before scores for the exact quick cohort. The full 1,099 × 3 remains for milestones.
- `eval_workflow_tests.rs` runs the real agent with a conversation-only scripted provider on two
  newly generated drawings, writes its own rules and mappings, resolves pending candidates by
  handle, and scores positive coverage. The provider has no Session/filesystem/truth access.
  This tests orchestration; live model quality still requires the reviewer's paid evaluation.
- Verified quick cohort (120 cases, one repetition): prepared coverage unchanged at 30.17%,
  precision 19.02% → 69.73%; shared-rule autonomy coverage unchanged at 19.50%, precision
  14.43% → 98.76%. Before scores use exactly these cases from the retained full reports. The
  scripted office/apartment runs reached 37.5%/44.44% coverage and 100% item precision in ten
  requests each. These results do not replace a live MIZAN ALONE score.
- No dependencies added. Validation and measured results: `docs/2026-10-22-mizan-autonomous-takeoff.md`.

### First live-run corrections (22 October 2026)

- MIZAN ALONE remains the priority. No paid eval is part of implementation. The first six-case
  run exposed expensive repeated history, invalid authored definitions and a PDF-only capture
  failure; its 22% coverage is the live baseline, not a target achieved by engine diagnostics.
- `qiyas-ai/src/history.rs` keeps the newest twenty tool results whole and summarizes older
  results once, retaining handles, ids, names, decisions and error codes. Identical successful
  reads keep their newest full result rather than duplicate evidence; changed values stay separate. The first digest and the latest successful command/schema lookups remain whole.
  Pictures follow their separate retention limit. The system prompt stays unchanged; tool ids, arguments and signatures stay paired.
  The older message-count compactor remains the final guard for long conversations.
- Records default to summaries without null/empty fields, with a 12 KB record-and-warning budget (page envelope excluded; a single oversized record is allowed); digest previews and capture captions
  are compact. PDF digest entries expose real source paths/pages and document capture arguments.
  Picture framing includes PDF/raster bounds, even without vector geometry. Relative document
  paths resolve beside the active drawing. Scans use calibrated `recognition.vision` hints and
  retained review metadata, or an explicit failure/missing-evidence explanation.
- Batch knowledge validation stages definitions in order on a private document, shared across
  types, schedule-derived types/rules, methods, mappings, checks and every `set_knowledge` kind.
  Execution still runs sequentially in one undo transaction. A genuinely missing type remains
  an error, including in rejection rules. Use `geometry.count`/`geometry.length` for quantity
  expressions, or distinct aliases; `count: "count"` is circular.
- Prompts execute batches directly and repair returned errors. Registry method/mapping examples
  contain complete recipes and citations. Recognition and mapping should be published together,
  then extended, so partial work remains measurable if a cap interrupts the run.
- The provider exposes current budget notices to the agent. At 80% of requests or cost used,
  or when fewer than two conservative reservations fit, it tells Mizan to stop exploring in a quantity job,
  finish mappings and write the end report. Hard stops report retained quantities locally.
- Eval always writes `end-report.json` and logs it in both logs, retaining the actual answer,
  measurement report, submission count and self-check. `done` with zero submitted items becomes
  a failed investigation with capture/calibration/recognition/mapping diagnostics. The existing
  save-and-score path also runs after request/cost caps; tests exercise both caps with nonempty
  generated submissions. Prediction and providers never receive benchmark truth.
- Offline replay evidence and exact validation outcomes are recorded in
  `docs/2026-10-22-mizan-live-fixes.md`; `.local/mizan-live-fixes/` holds raw check logs and
  per-request token estimates. The 21-request finishing replay peaked at 18,188 estimated input
  tokens and cost $0.231; request 21 fell from 58,841 to 17,086 tokens. Both cap replays retained
  and scored nonempty BOQs. These are scripted accounting comparisons, not live model scores.

### Discovery waste corrections (22 October 2026)

- Start take-off with `digest`, then `takeoff_kit`. The registry index names this first.
  Command/kind `x-takeoff-kit` metadata selects complete registry examples and orders a
  connected type → recognition → apply → method → mappings → report batch. Complete examples
  carry their required fields; enum choices remain explicit, with full contracts in error repairs. The drawing
  digest supplies paired source layer/block names without classifying their meaning.
- The kit has a 6,144-byte guard and deterministic contracts. Copy and adapt its batch;
  never validate empty command stubs or discover these commands one schema at a time.
  Missing-field validation and run errors include required fields and a filled registry
  example, including nested batch steps. Self-check and the final uncertainty report remain
  required; selected methods must be preserved.
- Normal and budgeted requests share a bounded busy/rate-limit retry policy: 5, 15, 45 and
  55 seconds (at most two minutes), with Retry-After seconds or HTTP dates as a lower bound.
  A server delay beyond the remaining window fails without retrying early. Transient rejections do not
  consume the request allowance or accrue unknown usage; transport/stream failures retain
  conservative accounting. No paid eval belongs to this implementation check.
- Offline before/after evidence and check results: `docs/2026-10-22-mizan-discovery.md`.

### First subscription-agent run corrections (22 October 2026)

- Eval and headless MCP submit accepted per-object `measurement.quantities` as dimension
  items, retaining the source handle in hexadecimal, rule, citation and dimension lines.
  Generated BOQ totals remain alongside them with `aggregate_of` component IDs. Bench
  recognition recipes retain the same per-object/grouped representation.
- Measurement rules expose `kind` (count/length/area/volume) and `basis` (geometric or
  `rule:<cited note name>`) in the kit and workflow. Explicit basis is preserved exactly.
  Legacy saved recipes without basis use geometric only for direct geometry or explicit
  area × numeric height; other transformations retain `rule:<own rule ID>` with a warning.
  This is a conservative migration, never a guess at a benchmark note name. Saved quantities
  are never changed; simple volume recipes retain their cited height factor for traceability.
- The reviewer’s scorer uses exact-basis, same-category and same-kind/unit handle tiers,
  never quantity to choose a match; ambiguity stays unmatched. A submitted geometric claim
  must independently reproduce. Complete aggregate totals are reported separately, not counted again as extras. Component IDs, source union,
  units, basis, total quantity and dimension-line multiset must agree. A matched aggregate
  consumes its components; partial/repeated/malformed totals get no exemption. Benchmark
  discrepancy prediction shares this component validation and compares at client granularity,
  so dimension rows and their BOQ total cannot create duplicate quantity or missing-item alarms.
- Digest note text is paged without losing its tail, with the three closest geometry handles,
  layers and bounding-box distances on the same sheet. `named_handles` lists explicit numeric
  references that resolve to drawing entities (decimal or `0x` hexadecimal). Proximity is not applicability.
  Schedule columns include wide tables and detected text grids. Self-check flags dimensional
  conflicts between a cited note's explicit numeric unit and a rule's geometry/factors,
  including percentage scope when the note states a unit. Fractional count quantities and a
  cited percentage applied across every rule are self-check errors, including base-plus-percentage
  recipes. The kit/workflow require fixing these before completion; factors apply only to the
  items named in their note. Named handles rejected or omitted from every rule are errors, as
  are cited factors applied to disjoint handles. `takeoff_check.end_report` accepts a draft final
  answer with `Exclude <handle>: <reason> (note <handle>)` lines; finish checks the retained answer
  with the same rules. Only a matching note-citing exclusion excuses omission. Overlapping note
  scopes and other unitless scope still need judgement.
- `bench rescore-agent --report <combined report.json> --output <new directory>` rebuilds
  retains saved submissions and re-evaluates embedded measurement rules on the saved drawing.
  Traceability never uses truth quantities or rule names; counts reproduce exactly and other
  quantities within 0.5%. All cited handles must exist; rule-derived drawing-note citations
  need note handles. Complete aggregates require every component to trace. Per-submission
  reasons include extras; submitted and all-truth traceability use separate numerators.
  Development scoring then opens truth only for correctness and matching. Holdout replay
  retains saved correctness/correspondence and appends a `rescore` event without opening
  sealed inputs. New reports save submission indices. Legacy mixed outcomes recover the
  scored set only from a unique partition of saved extras and aggregate memberships; missing
  per-truth-row links remain undisclosed, with exact failure identities in `submission_traces`.
  No quantity chooses a correspondence. Originals and contamination exclusions are retained; missing saved
  submissions remain empty. This command never starts an agent or provider.
- Submissions and saved replay use drawing-owned element metadata: centroid/insertion point
  converted to metres (PDF drivers already contain calibration), and verbatim element labels
  with rule text as fallback. PDF regions use the nearest enclosed text; equally near labels
  remain ambiguous. Vision/OCR spatial texts are retained on the source driver. BOQ totals use the unweighted
  centre of their distinct source elements; PDF evidence never claims original CAD handles.
- Open paths use their half-length point; regions use area centroids and blocks use insertion
  points. `bench regenerate-positions` updates only generated development truth position arrays,
  including variants, from source geometry. PDF truth uses calibrated page coordinates. Original
  quantity literals and all other truth bytes stay intact; hashes are appended to
  `sources/qiyas-generator/position-provenance.jsonl`. Holdout contents are never opened.
- PDF `description_position` matching uses max(0.25 m, 2% of the element's geometry-derived
  diagonal); CAD retains 0.05 m. Quantity values never select matches and ambiguity remains
  unmatched. A subsequent PDF-only `pdf_position` tier requires kind/unit and distance within
  max(0.5 m, 3% of the geometry diagonal). Only candidates at the same position can use the
  existing word tie-break; different nearby elements remain ambiguous. Position metadata and
  diagonals come from drawing geometry, not quantities.
- Raster vision refines supplied tick anchors using sub-pixel ink coverage, selects the longest
  verified span and cross-checks supplied dimension strings/endpoints. Its result and evidence
  report residual uncertainty; above 0.5%, candidate confidence is capped at 0.5. Supply
  `calibration_spans`, `dimensions` and verbatim spatial `texts` from vision/OCR; PDF text is
  collected automatically. Missing ticks or independent dimensions are reported explicitly.
  `bench calibrate-scans --output FILE` audits all generated scans against their geometry only
  after production calibration, without agents or changes to cases.
- `bench regenerate-scans` refreshes only existing generated scanned-PDF variants using their
  saved seeds, adds a 4% margin and reframes prepared vision recipes. Other variants and saved
  agent artefacts remain untouched. No model calls are involved.
- Keep Codex's `-c mcp_servers.qiyas.default_tools_approval_mode="approve"` and its argument
  test: read-only agent execution otherwise refuses the configured MCP tools.
- Verification and saved-run comparison: `docs/2026-10-22-agent-eval-fixes.md`.

## Discrepancies and queries (Phase 19)

- The registry command `discrepancy_check` (legacy alias `check`) compares the imported client BOQ with mapped element quantities
  and manual take-off, evaluates drawing knowledge kind `checks`, and raises queries and Comment
  clouds as one transaction. `qiyas-takeoff::compare` and `compare_session` retain their public
  API; the shared comparison implementation now lives in `qiyas-core/src/discrepancy/` so the
  generic command registry, Mizan, desktop and CLI use the same engine.
- `define_check {name,rule}` and `set_knowledge {kind:"checks",name,data}` validate cited rules.
  Rules carry severity and English/Arabic questions. General operations are schedule row joins
  against element counts or summed/per-instance numeric properties, host/property joins, quantity relations between
  sets, and membership in a drawing or supplied document table. Column mappings, element
  selectors, tolerances, units and conversion factors are data. Extracted document tables live
  under knowledge `tables` as `{rows,citation}`; missing evidence is an error, never a zero/pass.
- BOQ comparison finds mapped codes absent from the client list, unmeasured client scope,
  wrong quantities/units (including measurement unit bands) and duplicate codes. A method rule
  may supply `tolerance:{percent,absolute}`; generated items retain their applicable tolerances
  and citations. Existing manual comparison defaults remain for older drawings.
- Findings carry `query` metadata: severity, source citations, bilingual question, evidence
  handles/sheets/grids, rule identity and cloud handles. Stable finding ids retain statuses and
  notes on repeated checks. The drawing's `queries/result` XRecord stores the list; standalone
  DWG/DXF reopen restores it without a sidecar. Resolution and comparison are each one undo
  step. Rechecking replaces owned clouds without accumulating duplicates. Findings describe
  the last check; run `discrepancy_check` again after repairs.
- The conflicts panel shows query questions, severity and evidence; Compare redraws the clouds.
  Take-off exports from all hosts add `Queries`, with readable bilingual columns and lossless
  records. `qiyas_boq::queries::import_queries` reads them back. No rates are exported here.
- Mizan's registry workflow gains `discrepancy_check` after BOQ generation; it authors cited rules, runs
  comparison and reports uncertainties without requiring a human step. Scripted workflow tests
  exercise this command through the production generic tool surface.
- Benchmark prediction now reads client inputs and shared check interpretations from `.local/`,
  submits findings, and leaves evidence-to-truth identity matching to the scorer. Shared data
  declares which interpretations establish complete scope for missing-item checks. Neither
  prediction nor Mizan reads truth. Engine prepared/autonomy modes remain diagnostics; no live
  provider evaluation was authorized or run. Results and limits: `docs/2026-10-22-qiyas-phase19.md`.
- Verified quick results (120 identical cases, one run per mode): seeded-error recall 0/36 →
  36/36 (100%) in both modes; finding precision 80%, nine false alarms over 111 clean cases
  (0.081/case), all on overlap variants. BOQ coverage/item precision remain 30.17%/69.73%
  prepared and 19.50%/98.76% autonomy. DWG/DXF and Excel query round trips passed. Two socket
  tests still require a reviewer outside the sandbox; all other requested checks passed.

## Revisions and variations (Phase 20)

- `revisions.rs` compares the current B with A using stable handles, geometry, layers,
  blocks and attributes, including changed block definitions and renumbered exports.
  `compare_revisions {other_path,tolerance?}` reads per-sheet changes and bounds.
  Tolerance is in drawing units; different-unit inputs must be converted first.
- `carry_knowledge {from_path,tolerance?}` carries types, recognition, measurement methods,
  mappings and checks, re-extracts B on all sheets and retains A's measured baseline in
  drawing knowledge. Revision identities survive handle changes; local object ids stay
  driver handles. BOQ `{id}` templates use the stable identity.
- `variation_report {layer?}` stores quantity-only A/B/delta rows with element/handle traces
  and replaces its Comment clouds in one undo step. Its cloud layer is data, retained in
  knowledge `revisions/cloud_layer` (default VARIATION). Carry is also one undo step.
  Take-off exports include bilingual Variation and lossless chunked records; the drawing
  retains the report and clouds through DWG/DXF. If any retained room enclosure is broken,
  variation quantities are unavailable with a count and handles, including cached reports; an
  unknown quantity must never become a zero/removal delta. No rates or domain rules were added.
- The three commands have command-line words and generic-tool access. The take-off kit
  contains revision examples within its existing byte budget; the workflow index describes
  open B → carry A → re-take-off → variation/clouds → export.
- `qiyas-cli variation A B --export x.xlsx [--drawing-output B-clouded.dxf]` runs the same
  offline flow. Quick benchmark runs also score existing revision-B variants, with
  per-item recall/precision, delta accuracy and per-case time. Prediction never reads truth.
  Engine diagnostics remain distinct from live MIZAN ALONE performance.
  `bench revisions --set quick --mode prepared|autonomy` runs only the revision cohort.
  Final quick revision scores: 9/9 items in both modes, 100% recall/precision/delta
  accuracy, 0.419–0.640 seconds per case including workbook/drawing persistence.
- Implementation details, validation and measured results: `docs/2026-10-22-qiyas-phase20.md`.

## Projects (Phase 21)

- `qiyas-core/src/projects.rs` extends `.qss` with optional project data: method override,
  generated BOQ with per-drawing dimension contributions, cited cross-drawing checks, RFIs,
  register extraction rules/results and per-drawing status. No rates or drawing-specific
  interpretation is stored at project level. The data home is shared by desktop and CLI;
  debug uses `.local/qiyas-home`, release `~/.qiyas`, and `QIYAS_HOME` overrides both.
- Registry `project.create/open/add_drawing/remove_drawing/list`, `set_method`, `set_checks`,
  `set_register_rules` and `run` work through the generic tools. Projects live directly in
  `qiyas_dir()/projects/*.qss` and may reference absolute drawing paths. Ordinary sheet sets
  keep relative paths within their folder, including existing symlink targets. Each project
  mutation is one file undo step; undo refuses an intervening external edit. Project commands
  cannot be nested in drawing batches.
- Aggregation groups generated items by code (description key when empty), section, trade and
  unit. Contributions retain drawing path, quantity and complete dimension lines. Project runs
  apply the override to each isolated drawing session before recognition and measurement,
  reusing core script parsing/execution. Original drawing files are not overwritten by a run;
  Mizan saves its interpretations into each drawing before running the project.
- Phase 19 checks share one evaluator for drawings and projects. Sets may name `drawings`;
  a table names `drawing` as well as its sheet/handle or extracted document table. References
  retain drawing paths so repeated CAD handles are unambiguous. Check failures are explicit
  warnings; unreadable drawings prevent a claim of completed cross-drawing checks.
- Register rules select a field (number, title, revision, date) and an attribute tag or text
  regex capture, optionally scoped by layer/block/sheet. No title-block tag vocabulary ships.
  Missing and conflicting values are flagged, with retained source handles.
- `qiyas-cli project run <project.qss> [--out folder]` writes `project.xlsx` and
  `morning-report.md` (default `<project>.output/`). Workbook sheets include Generated BOQ,
  Dimension sheets, Queries, Register, Drawings and a chunked lossless project record.
  Failed drawings retain partial output and exit 1; incomplete checks exit 2.
- The Projects side panel is gone (W4: a project is a folder you open); the commands
  (`project.*`, `qiyas-cli project run`) and the project workspace stay. Mizan's host uses that
  workspace and may open members of its selected project without repeatedly asking for file approval.
- The take-off workflow includes the project path. The kit explicitly says it is complete;
  discovery for commands covered by it returns their short kit entry. The 6,144-byte cap stays.
- `bench projects --set quick --mode prepared|autonomy` and the full quick runs score projects
  built from existing clinic storeys, duplex storeys and apartment floors. Clean controls and
  planted schedule/identity discrepancies exercise cross-drawing recall. Prediction writes
  artifacts before scoring reads truth. These are engine diagnostics, not MIZAN ALONE.
  `bench projects --rescore <report-folder>` only rescores saved project predictions. The
  scorer preserves each input recipe's declared measurement basis before opening truth.
- No new crates or UI libraries. Validation, scores and limits: `docs/2026-10-22-qiyas-phase21.md`.

## Deliverables (Phase 22)

- `qiyas-core/src/deliverables.rs`: report layouts as drawing knowledge kind `reports` or
  project `.qss` data (`project.set_reports`, one undo step). Neutral bilingual defaults ship
  as JSON data. Sections, columns, headings, grouping, totals and page setup are layout data;
  totals separate units. Dimension lines retain timesing, signed deductions and references.
  Printed dimension sheets (Excel and PDF, `deliverables::printed_lines`) leave out lines that
  add nothing (`1 × 0 = 0`, a rule evaluated on an object it does not apply to; the clinic
  working-out was 18,000 lines, 707 of them zero); an item whose lines all give nothing keeps
  its first line. The measurement report itself keeps every line.
- Registry `deliverable.export {kind, format, layout?, path, language?, project?}` supports
  dimensions, boq, queries, variation and register, in Excel/PDF, English/Arabic. It is in the
  bounded take-off kit. Core prepares the report; shared `qiyas-boq::deliverables` completes it
  in desktop, AI/MCP and CLI/script hosts. Paths and client templates stay in the source folder.
- PDF uses the existing plot text shaping/font embedding/ActualText machinery, with pagination
  and wrapping. Thmanyah is requested through the existing font loader; no font files are added.
  Visible Excel/PDF cells format counts as integers and quantities to the method's rounding, else
  two decimals for m² and m and three for other units (W3). References show sheet, element kind/mark and short
  handles; drawing cells show filenames. Full precision, paths and structured provenance remain
  in the lossless report records. Excel/PDF include lossless report records. Client-template exports preserve the original
  workbook's package and change only mapped quantity cells, leaving rates and amounts untouched.
- `qiyas-cli export <drawing|project.qss> --kind boq --format excel --path boq.xlsx`
  accepts `--language en|ar` and `--layout name`. `project run` adds `--deliverables all`
  (or comma-separated kinds), `--formats excel,pdf`, `--language` and `--layout`.
- Details, mapping example and validation: `docs/2026-10-22-qiyas-phase22.md`.

## Trust (Phase 23)

- `qiyas_read explain {item, project?}` and the window's “How did you get this number?” action
  share `Session::explain`. Item selectors: `generated:<code>`, `boq:<code>`, `takeoff:<id>`,
  `dimension:<decimal object handle>:<rule id>`. Imported rows with duplicate or blank codes use
  `boq-row:<zero-based row index>`. For projects supply `project` (the `.qss` path)
  and the item code or `project:<zero-based row index>` for duplicate codes. Reads never mutate.
- Explanations include dimension arithmetic, elements, handles/sheets, recognition candidates and
  rules with source evidence, measurement rules/citations and confidence. Imported client figures
  and manual rows explicitly disclose missing authored rules. English/Arabic window actions show
  the evidence, with drawing and sheet selection for project contributions. Project runs snapshot
  explanations alongside quantities, so later drawing edits cannot silently change their provenance.
- Measurement rules may supply `confidence` in [0,1]. Generated quantities use the minimum of
  rule confidence and every contributing element's recognition confidence, including deduction
  decisions. Grouped BOQ and project lines take the minimum of their components. Unscored rules,
  authored elements without recognition and legacy/manual quantities use 0.5, never assumed 100%.
  This is conservative evidence confidence, not a statistically proven probability.
- Shared bands: low <0.6, medium [0.6,0.9), high >=0.9. Reports list lowest confidence first and
  include counts by band. The numbers stay in the engine and its reports; the window never shows a
  confidence percentage or band and Mizan no longer appends a confidence report (W4, founder's
  ruling). Confidence never adds a review or approval gate.
- Benchmark submissions retain confidence before truth is opened. Each case and the overall
  JSON/Markdown report compare confidence with correctness using the existing strict tolerances.
  Extras fail; valid aggregate duplicates do not count twice; missing predictions have no confidence.
  Empty bands report N/A. Prepared/shared-rule diagnostics remain distinct from live MIZAN ALONE.
- No new dependencies, trade rules, rates or paid model runs. Phase report:
  `docs/2026-10-22-qiyas-phase23.md`.

## Drawing

- `Command::Draw(DrawSpec)` (`qiyas-core/src/draw.rs`): line, polyline (bulges), rectangle,
  polygon, circle, arc (centre or three points), ellipse, spline (fit points plus the cubic's
  control points), point, donut, ray, xline, revcloud, wipeout, hatch (closed entities or a
  ring; solid, ANSI31, ANSI37 or a built-in pattern: LINE, NET, DASH, BRICK), 2D solid, trace,
  multiline, sketch. One undo step on the sheet on screen; `Outcome::Added` lists the handles.
  Wire form `{"type":"draw","kind":"line",...}`, angles in degrees. Mizan's `draw` (line,
  polyline, rectangle, circle, arc, hatch, revcloud).
- Path and outline inputs use the core's transparent `PointRun<CLOSE>` type. Its schema states
  the policy: a returned start point closes boundaries and is removed; an open path keeps the
  vertex, like AutoCAD's plain polyline. An explicitly closed polyline removes the duplicate.
  The test uses `SAME_POINT` converted from millimetres and `tol_near` for survey coordinates.
  Walls, hatch rings and islands, area measurement and clouds share this rule with picking;
  open polylines, splines, leaders, traces, multilines and sketches preserve their paths.
- `Command::Batch { commands, layer?, color? }` runs any commands in order as ONE undo step, all
  or nothing (a transaction, see Undo): each through `run_as` with its usual follow-ups. Refused:
  what the registry marks `x-batch` false (editing a block in place, writing a block, the
  clipboard copy, another batch). With `layer` they draw on that layer (made when missing), with
  `color` in that colour, and the current state is put back after; undo puts back a current
  state the commands changed. A failure is `CoreError::Step { index, label, error }` ("Batch step
  2 (draw.line): …", from 0). `Outcome::Batch { outcomes, added }`. The AI's drawing tools use
  it.
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

## Drafting conventions

- `qiyas-core/src/conventions.rs`: `CenterMark { targets, extension? }` draws crosses through
  circle and arc centres, extending past the radius. `CenterLine { a, b }` draws the middle
  of two parallel lines over their shared length plus an extension, or their acute angle
  bisector. `SymmetryAxis { p1, p2 }` draws an axis between two points. Handles name entities
  on the current sheet. These use layer `CENTER` and the `CENTER` linetype.
- `Grid { x: [{label, at}], y: [{label, at}], extent?, bubble? }` draws axes with labelled
  bubbles at both ends on `GRID`. `extent` is `{min: [x, y], max: [x, y]}`; omitted, the current
  sheet and axes determine the extent. `bubble` is the radius. Explicit sizes and coordinates use drawing
  units. Default extension is 2.5 mm on paper, bubble radius 3.5 mm, text height 2.5 mm, all
  converted using the drawing's units and annotation scale. Long labels shrink to fit their bubble.
  Grid lines extend one bubble radius beyond the extent before the bubble, keeping bubbles
  on perpendicular axes from colliding at a corner.
- Each command is one undo step including missing layers and linetypes. Setting a layer's
  linetype to `CENTER` or `HIDDEN` also adds that standard linetype when absent, in the same
  undo step. Existing definitions are kept. The command registry exposes these to Mizan
  through its existing tools.

## Walls

- `qiyas-core/src/walls.rs`: `Command::Walls(WallSpec)` creates an editable wall system:
  joined outlines, one solid hatch per region, centrelines and reusable door/window inserts.
  The entire command, including joined existing systems, layers, blocks and xdata, is one undo
  step. Every caller uses the same core rules, including Mizan, MCP, scripts and the command line.
- `WallSpec { runs: [{points, closed, thickness?, height?, justification?}], thickness,
  justification, openings, height, layers, join }`: each run inherits omitted thickness,
  height and justification from the spec. Plan points, thickness, opening `at` (distance to
  its centre along the run) and width use drawing units. Height, sill and head use metres
  above floor. Justification is `centre`, `left` or `right` of the directed run. Height
  defaults to 3 m. Curves use straight polyline approximations.
- `join` defaults to true. A new run meeting or crossing existing centrelines within the
  drawing's tolerance joins their groups when their wall layers match. The original runs and
  openings and the new runs are rebuilt as one system; outer walls and later partitions have
  the same geometry and quantities as one command containing all runs. `join: false` creates
  a separate system. Layers are `outline`, `hatch`, `centreline`, `doors`, `windows`, defaulting
  to `A-WALL`, `A-WALL-PATT`, `A-WALL-CNTR`, `A-DOOR`, `A-GLAZ`.
- Wall members carry `QIYAS_WALL` xdata containing the group's definition, its
  member roles, runs and openings. The drawing is authoritative: DWG/DXF saves, copies and
  undo retain the editable definition. The centreline's first two real values remain its
  thickness in drawing units and height in metres. Measure hatches for net wall plan area.
- `add_wall_openings {centreline, openings: [{at, width, kind, swing?, sill?, head?}]}` adds
  doors/windows to the run named by its centreline handle. `remove_wall_openings {inserts}`
  closes the cuts named by opening insert handles. `edit_wall_opening {insert, at?, width?,
  kind?, swing?, sill?, head?, mark?}` moves or resizes an opening or changes its properties.
  Geometry changes rebuild the group in one undo step; an empty edit does not rebuild. `delete_entities` refuses wall opening inserts and
  names `remove_wall_openings`, so erasing a symbol cannot leave an unexplained hole.
- `wall_groups {width?}` is a read command, reachable with `qiyas_read run`. It reports each
  group on the sheet, its layers, and runs with centreline handle, length, thickness, height,
  justification and opening insert handles/properties. With `width`, each run includes
  `free_ranges` of opening-centre `at` values. Both ends are accepted by the same geometry
  checks used by the mutating commands. Surviving runs and openings retain their handles.
- Openings must fit a straight segment clear of corners, junctions and other openings.
  Refusals name the junction and offer the fitting ranges (or say none fit). A door defaults
  to left swing, sill 0 and head `min(2.1, run height)`; a window defaults to sill 0.9 m and
  the same head convention. Explicit contradictions are refused. Openings cannot wrap across
  a closed run's start. Symbols reuse block definitions named by millimetre size, with their
  geometry converted to drawing units.
- Rebuild outcomes retain `type: "walls"`, handles by role, centreline length (m), gross/net
  area (m²), volume (m³), and `handle_map` from old handles to their replacements. Take-off
  measurements and fields follow replaced hatches/centrelines; constraints on reshaped
  entities are dropped. Undo restores definitions, geometry and these dependent measurements.
  For unequal heights, volume integrates the wall union over each height interval and subtracts
  opening voids. Hatch replacement maps follow positive area overlap, so unrelated regions do
  not gain each other's measurements.
- `qiyas-geom/src/walls.rs` normalises around a local origin and thickness before booleans,
  preserving survey-coordinate accuracy. Each strip uses its run's width and justification.
  Mixed widths join at L, T and X junctions, with overlaps counted once; a partition butts
  into an outer wall's face. The public `where_it_fits` uses the widths of every meeting wall.
  Tests cover hand-calculated mixed 250/150 mm areas in mm, cm, m and inches, staged versus
  one-command construction, editing and persistence. The release `walls_speed` test guards
  500 walls relative to a fixed CPU reference workload.

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
  Overkill's omitted tolerance is 0.000001 mm converted to the drawing's units; an explicit
  tolerance remains in drawing units, including zero.
  `Outcome::Edited { added, removed, left_alone }`. Home tab, Modify group, after the modify tools; command line TR, EX
  (Shift+click swaps), BR, J, LEN, REVERSE, O, F, CHA, BLEND, PE, OVERKILL, FLATTEN.

## Text

- `qiyas-core/src/text.rs`: `Command::Text` (`rotation` in degrees) and `MText` (wire
  `mtext`; plain text, line breaks become `\P`, `\`, `{`, `}` escaped; box top left and width) go on the
  current layer and state with a text style (`None`: the drawing's TEXTSTYLE, else "Standard");
  `EditText { target, value, tag }` changes a text, an MText (as plain text: its formatting goes)
  or one attribute (`tag`, may be cleared) of a block reference; chains refused. One undo step
  each.
- `Text`, `MText`, `AttDef` and `Field` accept an omitted or zero `height`: 2.5 mm on paper at
  the annotation scale, converted to drawing units. Negative heights are refused. `Text`,
  `MText` and `EditText` read a literal backslash-n as a line break. A `Text` with a line break
  becomes an MText, with top left `[at.x, at.y + height]` and width 0.
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
  the clouds stay on the drawing (the Comments tab went in W4). No ribbon button yet. Mizan:
  `add_comment`, `list_comments`.

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
  (in `scripts/check.sh`): 200 connected constraints, guarded relative to a fixed CPU reference. `expr.rs`: own
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
- `Table` sizes can be omitted or zero. The row height is twice the default text height
  (2.5 mm on paper at the annotation scale). The column width fits the longest cell text,
  allowing about 0.7 text heights per character and one row height of padding.
- `qiyas-core/src/field.rs`: fields are AutoCAD field codes in the text (`%<\AcVar Date ...>%`,
  `%<\AcVar Filename>%`, `%<\AcObjProp Object(%<\_ObjId HEX>%).Area|Length ...>%`, Qiyas's own
  `%<\AcVar QiyasCount "BLOCK">%`). The visible text is the value; the code (the whole text as
  typed) is kept in `QIYAS_FIELD` xdata (on a table: row, column, code per cell). `Command::Field
  { at, height, field }` adds a one-line text; codes typed into Text, MText or a cell count too.
  `UpdateFields` works them all out again; area and length fields follow every modify, edit and
  erase in the same step (`follow_fields`). A field is a derived value: one on a locked layer, or
  outside a block edited in place, is worked out too (as AutoCAD); the lock stops only the user's
  own changes (find and replace, attribute sync, edits). Metres and square metres (`in_metres`, as the
  take-off), shown to 2 decimals with their unit unless the text after the field gives one
  (`17.81 m²`, Arabic `١٧٫٨١ م²`; a table cell whose heading gives the unit has none; W3);
  UTC date; `####` when the entity is gone. Not worked out on open. Find
  and replace leaves field texts and field cells as they are and counts them in `skipped`.
- Annotate tab, Tables group: Table (click, then rows, columns and sizes), Edit table (or a double
  click on a table: `TableEditor`, a grid in the drawing's column order; Shift+click or
  Shift+Arrow selects a range to merge). A failed cell write keeps its draft and clears the
  sent marker so Enter retries it unchanged; an in-flight duplicate is still suppressed.
  Row, column and merge edits wait for pending saves; a failed save cancels that queued structural
  edit and leaves the draft intact.
  Field (click, then `FieldDialog`; area and length pick their entity; also from the table
  editor into a cell), Update fields. Command line TABLE (TB), TABLEDIT, FIELD, UPDATEFIELD.

## Blocks

- `DefineBlock { name, base, entities: [{type, data}] }` creates a named definition directly
  from the same record shapes and validation as `CreateEntities`. Coordinates and `base` are
  drawing units; the block keeps the drawing's units. `Session::define_block` runs the command
  and returns its entity handles (`Outcome::Added`); the internal `define_block_entities`
  helper joins a command already being recorded, such as Walls. Names already taken (any case),
  invalid names, empty definitions and supplied ownership are refused. Its entities and new
  layers are one undo step. Place it with `InsertBlock`.
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
  or the layer states' hidden layers keeps only what it changed (`undo::ViewDelta`: current before
  and after, layers it hid and showed, each state's list before and after), and undo and redo put
  back only that: layers hidden or shown since stay as they are, and a renamed hidden layer is
  hidden again under its old name only while it is still hidden under the new one (undoing a
  rename after Show all once hid the whole old set again). Isolating is the core's
  (`Command::IsolateLayer` / `UnisolateLayers`, LAYISO / LAYUNISO, `Session::isolated`, not undo
  steps): isolating the isolated layer again, LAYUNISO or Show all put back only what isolating
  hid (a layer the user showed or hid meanwhile stays so); a rename follows it; restoring a layer
  state ends it. `DrawingInfo::isolated_layer`; the Layers panel says "Isolated: X" with
  Un-isolate, and the row's isolate button is pressed. Layer states are AutoCAD's own (`ACAD_LAYERSTATES` XRecords, codec
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
  LAS) and Match layer names (LAYTRANS, `LayerTranslator`); the properties palette edits the
  picked host entity in place (xref and block content stay read-only) and, with nothing picked,
  what new entities are drawn with. Home tab, Layers group; command line LAYER (LA: the panel),
  LAYSTATE (LAS), LAYTRANS, VPLAYER (the VP freeze column), GROUP (G: pick, Enter, name),
  UNGROUP, MATCHPROP (MA: source, then targets, Enter), PROPERTIES (PR, CH: the palette).

## Layouts

- Fresh drawings size their default paper layouts in `Drawing::blank`: ISO A3 landscape
  (420 x 297 mm) for metric templates, ANSI B landscape (431.8 x 279.4 mm) for inches/feet.
  This is initialization, with no undo step. Existing drawings' unsized layouts stay unsized
  on open; `SetLayoutPaper { name, paper, orientation }` (`SETLAYOUTPAPER`) explicitly sizes
  them or changes their paper in one undo step, preserving the selected sheet and entities.
  Sized-layout errors name both `new_layout` and `set_layout_paper`. Input validation lists
  layout selection, paper sizing and whether frames fit the paper in `not_checked`; a schema
  success alone does not certify those drawing-dependent conditions.
- `qiyas-core/src/layouts.rs` owns `Paper { name, w_mm, h_mm }` and `paper_catalog()` (ISO A0
  to A4, ANSI A to E, ARCH A to E1, portrait), re-exported by `qiyas-plot` to avoid a dependency
  cycle. `NewLayout { name, paper, orientation }` creates a sized paper layout through the
  codec's `add_layout`; `paper` is a catalog name such as `"ISO A1"`, and orientation is
  `portrait` (default) or `landscape`. It leaves the sheet on screen selected. A taken name
  (any case), invalid name or collision with the backing paper-space block is refused.
  `RenameLayout { from, to }` changes the layout and dictionary entry while keeping its block.
- `DrawingInfo.paper` is the current layout's `{name, w_mm, h_mm}` after rotation, or null on
  Model or an unsized layout. `Session::paper_unit()` gives one paper millimetre in sheet
  coordinates. Layout text defaults to 2.5 paper millimetres; dimensions convert the drawing's
  style sizes to paper millimetres with no model annotation scale.
- `MView { corner1, corner2, scale?, centre? }` can set its physical paper:model ratio with a
  string such as `"1:100"` (model units are accounted for), or a numeric custom scale in
  paper millimetres per model unit. `centre` is the model point shown at the viewport's middle.
  Without them it fits and centres the model extents.
  Its alternative form `{ model_bounds: [[x1,y1],[x2,y2]], scale: "1:100",
  paper_centre: [x,y] }` computes the paper frame from the model window and drawing units.
  The two forms are validated together; model bounds use drawing units and paper centre uses
  paper millimetres. Either form accepts `target: {handle}` to resize the same viewport,
  preserving its handle, frozen layers and other settings. A new viewport made on current
  layer 0 goes on non-plotting `A-VIEW`, creating that layer in the same undo step; another
  chosen current layer is respected. Tests cover mm, cm, m and inches at 1:50 and 1:100.
  `x-prompts.required` retains the two corner questions in the typed `MVIEW` form while the
  JSON command also accepts model bounds; explicit prompt requirements ignore schema defaults.
- `TitleBlock { fields, margin_mm?, binding_mm?, layer? }` draws Qiyas's frame and title grid at the
  bottom right of the layout on screen, in paper millimetres. Defaults: margin 10 mm, binding
  20 mm; the left margin is the larger of those. `fields` maps names to values, including
  `title`, `project`, `drawing_no`, `scale`, `date`, `drawn_by`, `sheet` and extra names. Only
  supplied values become text, with literal escaping; missing values stay empty, including the
  date and scale. `Outcome::TitleBlock { handles, empty_fields }` explicitly lists every
  standard or supplied field left empty. The default layer is `A-ANNO-TTLB`; `layer` can
  override it. Missing layers are made in the same undo step and the current layer is kept.
  The title grid shares the frame's outside edges without drawing them twice,
  including on small paper where it fills the frame's width. It requires a sized layout.
  NewLayout, SetLayoutPaper, RenameLayout, MView and TitleBlock
  each make one undo step, keeping their table changes too.

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
- Paper-space viewports (`qiyas-core/src/viewports.rs`): `MView { corner1, corner2, scale?,
  centre? }` (wire `mview`; layouts only; see Layouts) adds a `Viewport` on the current layer,
  fitting model extents when no scale is given. Its frame draws (flatten's Viewport arm, never
  number 1); a click inside one with
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
  `{ kind: tool | action | toggle | command, command, label }` as `Ribbon.tsx` draws them, in groups that may
  carry `main` (how many buttons show; the rest wait behind the group's "⋯" flyout). Tabs: Measure
  (Measure, Inquiry, Take-off, Dimensions with `main` 5, Markup), Draw (id `home`: Draw, Modify,
  Edit, Annotation, Clipboard, Layers), Annotate (Dimensions with all kinds and the style,
  Leaders, Markup, Tables); those three show on the tab row and the rest (Parametric, Insert, View,
  Manage (Drawing: Audit, Purge, Recover), Output) are under "More" (`SHOWN_TABS`). Group id
  `markup` holds the revision cloud and the comment cloud (COMMENT). `src/shell/ribbon.default.json` is that default, and both
  `tests/options.rs` and `ribbonLayout.test.ts` compare theirs with it. A saved layout may use
  the default's buttons and the registry's command words (moved between groups and tabs as it likes), and one that no longer
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
  That IPC catalog includes a Command line tab grouped by registry category, initially hidden.
  Its `kind: "command"` buttons start the core machine's prompts on the command line. Adding
  words to a core variant makes it available here without editing window code. The registry's
  ribbon guard checks every shipped button against core names, words or actual window handlers.
  `Ribbon.tsx` draws `layout ?? DEFAULT_RIBBON` (the same default as the core's). ALIASEDIT (`AliasEditor`): alias
  to command, upper-cased; refused: two that differ only in case, a command's own name
  (the window and registry catalog), a command that is not one; saved aliases reach the command line (`loadAliases`).

## PDF

- PDFs are read through pdfium (BSD-3), a shared library loaded at run time through
  `pdfium-render`. `scripts/pdfium.sh` downloads the build for this computer from
  bblanchon/pdfium-binaries into `crates/qiyas-desktop/resources/pdfium/` (git-ignored) and its
  licence into `LICENSES/pdfium-BSD-3.txt`. It installs the tested release only (`TESTED_TAG`,
  chromium/8086) and checks the download against `scripts/pdfium.sha256`; a mismatch installs
  nothing. Run it once per machine. Without it the PDF commands
  answer "PDF support is not installed: run scripts/pdfium.sh" and the PDF tests are ignored, so
  `scripts/check.sh` runs them only where the script has been run (no CI checks them).
- `QIYAS_PDFIUM` (the library or its folder) overrides where it is looked for. Without it:
  beside the program (`resources/pdfium/`, the installed app), then the repository's
  `crates/qiyas-desktop/resources/pdfium/` (debug and release binaries run from `target/`).
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
  either). Simple dash, gap and dot linetypes plot as they appear on screen; patterns resolve ByLayer and ByBlock and apply entity and global linetype scales. Complex linetype text/shapes and viewport-specific PSLTSCALE remain unsupported.
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
- By command a plot goes only in the export folder (`Session::export_folder`: the drawing's folder,
  or Documents for an untitled drawing; `output` a name inside it or a subfolder;
  none: `<drawing>-<sheet>.pdf` there), written beside the target then moved over it.
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

- `Command::Rooms { query }` (`qiyas-core/src/rooms.rs`) reads rooms round name texts or seed
  points, using a room-specific enclosure policy independent of Hatch and layer visibility. `enclosed_area_m2` is the drawn area;
  `net_area_m2` and `candidate_area_m2` take islands off unless `subtract_islands` is false.
  Room boundaries exclude text/mtext/attributes, dimensions, leaders, tables, points, hatches,
  wipeouts, rays and xlines, every part carrying QIYAS_GRID provenance, opening object symbols,
  and QIYAS measurement overlays (saved derived geometry cannot become its own enclosure).
  Qiyas walls supply joined faces without opening cuts: a doorway closes at the internal wall face.
  `boundary_layers` and `exclude_layers` on room queries, seed measurements, region sources and
  recognition selectors select foreign enclosure geometry explicitly. Retained regions persist
  that policy. Other drawn geometry is eligible when no inclusion list is given. Hatch is unchanged.
  Grid axes and letters now carry provenance as well as bubbles; grid letters never seed rooms.
  Older grids recover axis and letter provenance from stamped bubbles when opened.
  These are unapproved candidate areas. `LabelSpaces { query, layer, height, decimals,
  language }` writes "Area 19.31 m²" or "المساحة ١٩٫٣١ م²" (Arabic digits and decimal separator).
  Labels prefer the space below the name or near the seed, then search inside the room and wrap
  at the same height when necessary. Default height is 70% of the name height, at least 2.5 mm
  on paper; an explicit `height` is in metres. Values and units stay together when wrapping.
  The report says `placement: inside|wrapped`. A room too small for readable text returns a
  clear error and rolls back, so the caller can choose a larger scale or an external linked note.
  Both return `Outcome::Rooms`; labelling is one transaction named `label_spaces`.
  Labels now contain live `QiyasRoomArea` fields. `LabelSpaces` retains each closed outer ring
  as a polyline on non-plotting `A-ROOM-BOUNDARY`, with its seed, name, exact bulges and islands
  in `QIYAS_ROOM` xdata. The report includes `boundary_handle`. This is a durable room object
  through DWG/DXF; approval remains in metadata and the take-off report, never on the plan.
  Old label wording remains recognisable; saved live fields display the new wording on refresh. Room schedule headers are "Area (m²)" or, with
  `language: "ar"`, "المساحة (م²)".
  Changed generated boundaries invalidate the codec's whole `raw_record` and graphics cache;
  changed room metadata also invalidates only its own application's raw DWG EED blob, preserving
  other applications' data. Tests save, reopen, edit, save and reopen again, checking the retained
  area before any command can refresh it.
  Repeated labels reuse the room; conflicting boundary or island policies are refused. Every drawing edit
  retraces rooms from their seeds, excluding retained boundaries from the search, then updates
  labels, schedules and linked take-off inside the same undo step. A broken enclosure shows
  `####`; its take-off has `quantity_valid: false` and null `value`/`shown`, is listed as
  unavailable, and is excluded from totals, comparisons and exports. Reopening refreshes region
  quantities; undo restores validity with the source geometry. Invalid embedded measurements use
  invisible records retaining validity and the room binding, including without a project sidecar.
  Reports count and list unavailable rows.
- `RoomSchedule { at, source_sheet?, labels?, decimals?, language? }` (`ROOMSCHEDULE`) builds a drawing
  table from retained rooms, with per-room area fields and a total field. Source defaults to
  Model; the table goes on the current sheet, using `A-ANNO-TABL` when the current layer is 0.
  `MeasureRoom { label? , point?, boundary_layers?, exclude_layers? }` (`MEASUREROOM`) accepts exactly one live label/boundary
  target or a seed. It measures the same net boundary in square metres, retaining a newly found
  seed region when needed; all quantities remain unapproved candidates. Both commands are one
  undo step. Construction rule `room_area_mismatch` compares linked labels, schedule cells and
  take-off rows with their canonical room, including missing or opened boundaries.
  The drafting replay still compares every room geometry and quantity before/after Grid;
  it removes only `boundary_handle` from that comparison because LabelSpaces between the reads
  now retains room objects, and separately checks their closed polylines and boundary layer.
  Its layer assertions now check label texts and retained boundaries separately, and title-block
  entities on `A-ANNO-TTLB` while the explicitly chosen viewport layer stays `A-ANNO-TEXT`.
- `AddMeasurement` accepts empty zero-quantity Items with `unmeasured_item` advice for AI/MCP
  unless `empty_item` was explicit. Counts of other entity types give `count_non_block` advice;
  external entities without a retainable count location still fail with working alternatives. `AddMarkup { kind: cloud | note, points, text }` adds a take-off
  markup of quantity 0, with at least three points for a cloud or one for a note.
- Measure tab: Geometry (click an entity: radius, diameter, centre, angles, lengths, block and text
  facts in an Info card at the top of the right panel), Divide (click, then the number of parts),
  Measure along (click, then the step in metres), ID point (snapped coordinates in metres), and
  Extract data (filter by layers, type, block, text; pick fields and attribute tags; copy as TSV
  or export one "Data" sheet to Excel). Extraction reads every 5,000-row page, reports loaded
  row counts, and refuses partial results after a failed page. Its Take-off group exports the take-off, imports a BOQ,
  compares and exports the conflicts.
- Divide and measure along place point entities on layer `QIYAS-POINT` by true arc length and
  count them in the take-off; points and count are one undo step (a drawing change: Save asks).
- The same five are AI tools: `measure_geometry`, `divide_along`, `measure_along` (step in
  metres), `id_point`, `extract_data` (paged under 40 KB).

## Drawing check

- `qiyas-core/src/check.rs`: `Command::CheckDrawing { scope }` (`Session::check_drawing`)
  checks the current sheet, or `scope.sheet`. Optional `handles` and `rules` keep only the
  findings requested. `scope.packs` selects `general` and/or `construction`. Empty selects
  general, plus construction when the drawing has retained rooms, take-off rows or enclosure
  geometry. Explicit rule requests run independently of inference; an explicit `["general"]`
  keeps construction rules off. Room candidates never depend on English layer fragments.
  Unknown rule or pack names are validation errors; the schemas list every allowed name.
  It reads without changing the drawing or undo history.
- Rules find short or overshooting ends, duplicates, overlaps, tiny segments, open
  rooms, room-size text that differs, text collisions and poor paper height, layer 0, explicit
  colours, dimension overrides that differ, literal backslash-n, far-off entities and lost
  take-off links. `room_open`, `room_size_text_differs` and `unlinked_takeoff` belong to
  construction; the others belong to general. General's informational `missing_center_mark`
  flags circles and full arcs without a centre mark or centre line through their centre.
  Automatic `missing_axis` detection is omitted: arbitrary outlines do not reliably identify
  their intended axis. Use an explicit symmetry expectation instead.
- `duplicate` and `overlap` compare only entities on the same layer (case-insensitive layer
  names), as AutoCAD OVERKILL does by default: layer is a property that must match. Coincident
  geometry on different layers is intentional, such as grid axes on wall centrelines, a hatch
  boundary on its outline layer, or a viewport frame on a title line.
- `layer_zero` is a warning for entities drawn on a sheet (Model or a layout); inside a block
  definition layer 0 is right (contents take the reference's layer) and is not flagged. The
  own-output guard draws on a layer of its own. `tiny_segment` measures
  actual curve pieces, not display tessellation chords; `overlap` compares actual straight
  CAD edges. A dangling end must continue towards the nearby edge, so a parallel offset
  does not count as a gap. Text checks use the same alignment and attachment as rendering.
  Definition scopes follow their owning layout for model or paper units. Dimension extension
  endpoints are exempt from overshoot only when their location and extension match the style;
  changing an end still exposes a real overshoot. Text/line collision uses oriented text bounds.
- `tests/own_output.rs` runs registry drawing examples in mm, cm, m and inches, with declared
  prerequisites for operations on existing geometry, and checks their additions with the
  general pack, including additions inside block definitions. Every drawing-edit example must
  execute successfully; there is no allowlist of refused examples. Harder cases cover closed
  wall loops, junctions, openings near ends and corners, and small curves. No error or warning
  is accepted; informational notes are allowed.
- `Outcome::Checked` gives each finding's rule, pack, severity, handles, place and message,
  counts by rule and a short summary. At most 200 findings are listed; every text and room
  candidate is inspected. `checked_rules`, `complete` and `not_checked` disclose coverage.
  Nested geometry limits, unsupported multileaders/viewports and dense room tracing make the
  report explicitly incomplete. `check_drawing_all` also removes the findings display limit.

## Verify

- `qiyas-core/src/verify.rs`: `Command::Verify { expectations }` is read-only and returns
  `Outcome::Verified { results }`, one pass/fail result per expectation, with its measured
  value and tolerance. Invalid targets fail their own expectation with an explanation.
  Mizan uses `qiyas_read` with `op: "run"` and `args.command.type: "verify"` before saying done.
- Each expectation has a `kind`, `targets: [{handle}]`, optional picking `points`, `value`,
  `tolerance` and `in_metres`. Targets and pick points use the same part selection as
  `AddConstraint`; distance can also use two literal points. Kinds cover length, distance,
  angle, radius, area, closed, parallel, perpendicular, tangent, concentric, collinear,
  horizontal, vertical, coincident, equal, symmetric, inside_bounds, no_self_intersection
  and count. Count can filter by `layer`, `entity_type` or `block`; bounds are
  `[[min_x, min_y], [max_x, max_y]]` in drawing coordinates.
- Lengths use drawing units, or metres with `in_metres`; areas use the corresponding square
  units and angles use degrees. Defaults are 0.5 mm, 0.1 degree, (0.5 mm)² for areas and zero
  for counts and intersections. Geometric
  relations reuse qiyas-param's constraint kinds and residuals without solving or moving
  the drawing. Whole-outline symmetry accepts a flat closed straight polyline with 3–1024
  vertices and an axis line; picked point symmetry uses two points and an axis line.
  Self-intersection checks flatten paths and refuse more than 4096 vertices per path.
- Relationship results measure deviation from zero: angle or distance error, the largest
  open-end gap for `closed`, distance outside for `inside_bounds`, or the number of paths
  crossing or doubling back for `no_self_intersection`. Bounds use analytic extrema and
  refuse shapes without exact bounds rather than reporting an approximate pass.

## Drawings, tabs, recents and views

- Several drawings open at once, one file tab each above the drawing (title, "•" while the drawing
  has unsaved changes, × or a middle click closes; Ctrl+Tab / Ctrl+Shift+Tab / Ctrl+W). Each tab
  owns a stable drawing workspace and Mizan. Activating a tab changes the visible workspace;
  it does not move sessions or cancel turns. IPC accepts an optional workspace id, resolving the
  visible one once when omitted. Bound agents and MCP connections retain their drawing.
  Lock order: copy and release registry references, then AI, session and published metadata.
  Never take the registry or AI lock while holding a session lock. Opening an already open file
  shows its tab. Closing a busy drawing asks to stop Mizan; unsaved changes ask separately.
- New drawing (quick-access New, Ctrl+N, NEW / QNEW): `NewDrawingDialog` asks metres,
  centimetres or millimetres (the last choice kept in `localStorage` `qiyas.newUnits`), then IPC
  `new_drawing { units }` first reuses an empty untitled tab with matching units; otherwise
  `Session::new_blank` (`CadDocument::new()`, INSUNITS 6, 5 or 4) creates a new tab at a placeholder `<config>/untitled/Untitled-N.dwg` (`tabs::untitled_path`: the
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

## Automation contracts and sheet ownership (2026-10-21)

- Item 2: `qiyas-ai/src/surface_args.rs` describes every operation's arguments and calls core
  `validate_schema` before routing, including window and host operations. Failures carry JSON
  pointers, the expected schema and an example. Private surface contracts refuse unknown
  properties through that validator before serde; registry-backed operations retain the core's
  warnings and command-relative paths. Read shortcuts include their warnings in the result.
  Query contracts match each runner and preserve nullable options. Run with several commands directs callers to
  batch. Command payloads still use registry validation. Capabilities expose these contracts.
  Record schemas now include layouts, block records, dimension styles, linetypes, views,
  viewports, measurements, schedules, BOQ and conflicts. Other collections explicitly name
  supported schemas and direct callers to `records` with `detail: "full"`.
- Item 6: state includes the computer's current local ISO date in `local_date`. Both prompt
  languages require title-block/project metadata from the user or drawing, leaving and listing
  empty fields. Neither the date nor a drawing number is silently chosen for a title block.
- Item 9: both prompt languages require schedules generated from drawing objects; quantities
  must come from measurements or tool results, never typed schedule quantities.
- Item 10: capture captions distinguish model drawing units from layout paper millimetres,
  including pixels' physical size. State reports `sheet_units`, physical `paper`, and layout
  annotation height in paper units while retaining model `metres_per_unit` for model geometry.
- Item 12 (superseded by Phase 23): window sheet selections, including commands and scripts,
  remain available during a turn. Desktop sessions retain an automation working sheet. Generic
  AI/MCP session tools enter it under the session lock and restore the window sheet on success,
  error or panic. Typed commands, batches, reads and picture captures share this boundary.
  Sheet removal falls back to Model. Automatic UI events never change the user's sheet or zoom;
  reveals animate only on the sheet currently visible. User history Undo/Redo and external MCP
  drawing replacements retain turn protection; switching drawing tabs leaves their turns running.

## AI assistant and settings

- Goal S adds `claude_code` and `chatgpt_plan` Settings profiles alongside the unchanged
  API-key profiles. Claude Code uses the user's installed executable on PATH or their selected
  absolute path; an empty model lets Claude Code choose. Sign-in stays in the user's terminal:
  run `claude`, then `/login`. Settings checks are explicit, not polled. Runtime help must
  document strict MCP, permission mode and settings/tool isolation, or the turn is refused.
- Claude Code owns one `-p` process per user message. It gets only Qiyas's three MCP tools,
  no built-in tools, no inherited provider credentials/endpoint overrides, and an empty
  `<Qiyas data home>/claude-work` directory. User/project settings and hooks are excluded.
  Qiyas never reads Claude credentials, runs an SDK/ACP adapter, or scripts a login. Cancel
  sends SIGINT, then SIGTERM after five seconds; a process permit stays held until reaping.
  This cancellation implementation is Unix-only; other platforms refuse this provider.
  It currently requires Work autonomously: per-job/per-phase approvals are refused before
  starting a turn, because Qiyas cannot yet enforce those pauses inside the CLI's tool loop.
- The private MCP connection attributes changes to Mizan, preserving Undo, and uses the
  original user message for named-file authority. Claude stream events become ordinary
  activity cards and translated status/errors. Authentication, limits, billing and account
  holds stop immediately. Qiyas never retries this provider, including its usual context
  and output-limit retries. `eval-agent --agent claude` is refused even with `--agent-cmd`.
  Benchmarks must use ChatGPT, Codex or an API key, never this Claude provider.
- ChatGPT Settings enables **Continue with ChatGPT** (Arabic: **المتابعة باستخدام ChatGPT**),
  an account picker, Sign out, a model list and the ChatGPT per-app limits link. The exact
  SIWC wire specification was supplied by the reviewer on 23 October 2026; do not substitute
  backend-api endpoints or API-key billing for plan usage. OAuth uses PKCE/state/nonce,
  `http://127.0.0.1:<port>/auth/callback`, first registration with `dynamic_agent_client`
  and `agent_name_hint=Qiyas`, then the callback's issued `oaiapp_...` client ID.
- The OS keychain entry `qiyas` / `chatgpt-plan` contains the entire ChatGPT vault: stable
  `urn:uuid:...` host ID, separate account registrations, active account and tokens. Sign-out
  removes only the active account's tokens after attempting discovery-based revocation;
  the host ID and issued client IDs survive. OAuth tokens, subjects and issued client IDs never reach files,
  Settings, conversation state, logs or the frontend. UI account IDs are random local IDs.
- ChatGPT validates ID-token signature against discovery JWKS, issuer, issued-client audience,
  expiry and nonce. Missing `chatgpt.tokens.use.direct` keeps sign-in but blocks inference and
  model discovery. Refresh is serialized, rotates tokens/scopes atomically and retains saved
  credentials on temporary failures. Blocking keychain work owns the account lock even if its
  waiter is cancelled; a started rotation finishes and saves before sign-out can revoke it.
  Queued requests are cancelled before they can use another account. Terminal refresh failure
  clears only tokens and starts one OAuth recovery using the saved registration; a mismatched account is refused.
- ChatGPT lists only models with `visibility=list`, displaying `display_name` and sending
  `slug`. Account changes clear the model list and cancel requests. Responses use `store:false`
  and `stream:true`; only `response.completed` succeeds. Incomplete/broken streams stop.
  SIWC errors use their own English/Arabic classification: limits pause requests until the
  user explicitly resumes, temporary failures use bounded backoff, and unsupported features
  remove the named optional field before retrying a changed body, with a visible notice.
  An admission 401 never starts an OAuth loop or erases credentials. Sign-out cancels requests,
  retries transient revocation failures, clears local tokens and reports unconfirmed revocation.
  This provider is only for Mizan's own requests, never a general proxy or MCP token relay.
- ChatGPT mock HTTP/loopback tests are explicitly marked as requiring a socket-capable runner.
  Run the named ignored `chatgpt_plan::` tests there; no real sign-in or service calls belong
  in automated checks. The original Claude fake-process and credential sentinel checks remain.
- The public documentation was verified by the reviewer on 23 October 2026 and supplied in
  the task; no further fetch was attempted after the founder's instruction. See
  `docs/2026-10-23-qiyas-subscriptions.md` for policy and release gates, and
  `docs/2026-10-23-qiyas-goal-s.md` for actual tests and remaining work.

- Agent session access always runs on a blocking worker, including turn-opening state, drawing
  paths/epochs and drawing checks. Snapshot/checkpoint/resume use cached metadata returned by
  those workers. The first mutation's error baseline is captured under the same session lock,
  after cancellation/epoch validation and before execution; phase/end checks validate again
  after checking. Never hold a `std::sync::MutexGuard` across `.await`: both AI and desktop
  packages deny Clippy's `await_holding_lock` for every target. Agent integration tests and
  desktop AI turn tests have named 60-second timeouts.

- Conversations are app-wide, outside the shared `.qiyas` drawing project: one private JSON
  file per chat in `<app config>/conversations/`, with a small `index.json`. Mizan saves after
  each turn and before resetting for New chat or a failed turn. Other drawings and Settings
  defaults do not reset existing chats.
  The store keeps at most 100 chats, removing the least recently updated first. History defaults
  to the drawing on screen; All drawings, individual delete and Delete all are available there.
  An empty panel offers the latest saved chat for the current drawing.
- Resume restores the visible thread and the compacted model conversation, the exact saved
  system prompt and provider memory (including Gemini call signatures). Changing provider or
  model discards old thinking, raw provider blocks and provider memory. The first resumed turn
  tells Mizan to read the drawing before acting. Old tool cards cannot undo earlier sessions.
  Pictures and thumbnails become `[picture removed]` on disk; keys are never included.
- Settings include `autonomy` (`autonomous`, the default; `per_job`; `per_phase`) and
  `measurement_method` (a selected standard/local/custom name, or null for geometric quantities). Multi-phase
  plans wait for approval per the selected mode; Approve sends `Approved, continue.`, while
  Change focuses the composer. In autonomous mode, unfixed errors are reported without an approval pause.
- `qiyas-core/src/data_home.rs::qiyas_dir()` (re-exported by desktop methods) is the Qiyas data home: release builds use
  `~/.qiyas`, debug builds (`pnpm tauri dev`) use the repository's git-ignored
  `.local/qiyas-home`, and `QIYAS_HOME` overrides either. It creates `methods/` and `projects/`
  on first use. Settings offers named methods and local `methods/*.json` as extra choices; no measurement rules ship with Qiyas.
  Version 1 validates `{name, title, based_on, source_note, rules: []}` and the cited rule
  contents through the Phase 18 engine (an omitted version means 1). Mizan's live state reports the name and whether a nonempty rule set was loaded;
  without rules, quantities are geometric and answers say so.

Mizan's `reveal` results draw the changed entities in order in the highlight overlay over the
previous batch, then show the new batch (`viewport/reveal.ts`, `useSheets`, `useAssistantWiring`).
Steps queue; redraws wait, and zoom cannot expose a pending batch. Reduced motion, another
sheet, empty or very large steps show at once. A user command, undo or drawing/sheet switch
cancels the queue and shows the current drawing. Mutation refreshes also sync layers and layouts.

- `qiyas-ai/src/surface.rs` is the catalogue shared by Mizan and MCP. It offers exactly three
  tools: `qiyas_read { op, args }`, `qiyas_execute { op, args }` and
  `qiyas_capture { source, args }`. `args` is a JSON object written as a string (an object is
  accepted too). Their schemas are strict and closed for Claude and Gemini; every property is
  required, with no open objects or references. The old tool names in `execute.rs`, `window.rs`
  and `host.rs` are private runners, never offered to a model.
- Read operations: state, capabilities, commands (no args: the registry index; `name`: schema
  and examples; `search`: matching commands), validate, records, record_schema, query, measure (numbers
  only), run (read-only commands), check, history, drawings, selection and rooms. Execute
  operations: plan (conversation progress), ask (Mizan's questions, see W1), run (any core command), type (a typed command line), batch, set_records, create_entities, undo_own, select,
  view, new_drawing, open_drawing, switch_drawing, save_drawing, close_drawing, import_boq,
  compare, export_takeoff, export_conflicts and plot. Capture's source is screen or picture.
  `qiyas_execute {op: "plot", args: {sheet?, paper?, orientation?, scale?, path?}}` plots one
  sheet to a PDF in the drawing's folder through `qiyas-plot` (qiyas-ai depends on it): a
  layout plots its own page setup at 1:1 (`Area::Layout`), Model fits ISO A1 landscape;
  `paper` is a catalog name, `scale` is `fit` or `num:den`. Before it, Mizan had no way to
  print the A1 sheets it drew.
- `qiyas_execute {op: "type", args: {"line": "PLINE 0,0 10,0 C"}}` uses the same registry
  machine as scripts and the command line, then the same folder rules, undo attribution,
  drawing check and reveal as `run`. Coordinates use the session's UCS, with relative points
  following earlier points in that line. Each call supplies a whole command: incomplete
  input returns the next prompt without running anything; resend the complete line with its
  answers. `commands` includes words and aliases, and capabilities describe this operation.
- `qiyas_execute {op: "plan", args: {phases: [{id, title, status}]}}` reports a complete
  checklist snapshot. Status is `pending`, `active`, `done` or `skipped`; it changes no drawing
  and creates no undo step. It works without an open drawing, is described by capabilities
  and `commands {name: "plan"}`, and MCP accepts and returns the same operation. The agent
  emits a `plan` event; the panel shows the latest snapshot above that turn's answer and keeps
  it in conversation history. Titles are the model's words; status labels are English/Arabic,
  styled with brand tokens and no motion dependency.
- Mizan plans in words before drafting: parts or rooms and their sizes, checked against the
  user's numbers. Then it works one phase at a time, one batch per phase. Applicable phases
  include grid, outer walls, inner walls, openings, rooms, dimensions/table and sheet; other
  drafting follows the same loop. After each phase it captures, checks and fixes by handle
  before continuing. It never draws trial geometry and undoes it to discover behaviour:
  validation and wall reads supply that information. Openings are added to existing walls.
  Both prompt languages enforce this rule within the 900-word budget.
- A batch is one undo step, all or nothing, and can read earlier results through
  `{"$step": 0, "pointer": "/added/0"}`. `Session::batch_with` sets the batch's layer and colour,
  making a missing layer, then puts the previous current state back. Errors name the field,
  schema and example; unknown names suggest close matches. Changed geometry comes with a
  drawing check. The loop discovers a command, acts, checks and fixes by handle.
- Before execution, the core checks all fields together against the command registry: missing
  fields, wrong values and ranges are errors with JSON pointers; unknown fields are warnings
  on reads and errors on edits. Results include `defaults_used` with each field, value and unit. Errors preserve
  the serde field message, schema and example; unknown names still suggest close matches.
  All automation errors carry `code` beside `message`; `capabilities.error_codes` lists them.
- `qiyas_read validate {command}` or `{commands}` checks inputs without changing the drawing,
  including whether `$step` references point to earlier steps. It returns `valid`, `errors`,
  `warnings`, `defaults_used` and `not_checked` (geometry, locks and object existence need a run).
  Batch validation also names defaults affected by earlier sheet or scale changes and record
  checks that depend on earlier steps; execution reports the defaults each step actually used.
  `surface::OPERATIONS` declares each operation's environment: nothing, drawing, window or file
  host; selection and screen capture explicitly list both window and drawing. A file host
  reports whether it supplies a window through `DrawingHost::has_window`. Commands,
  capabilities, record_schema and validate work without an open drawing;
  unavailable environments answer `no_drawing`, `needs_window` or `needs_host`.
- Call counts produce `tool_cost` efficiency advice at 80% of the old 40/120 thresholds, once
  per turn (W1; it is never a reason to end a requested job). More than ten destructive
  commands produces `many_removals` advice with the count.
  Neither counter refuses work; Mizan decides. Actual memory/provider limits still apply. A tool
  card carries registry category/effect metadata (`qiyas-ai/src/activity.rs`) and an optional
  structured result count. The window builds a short translated sentence from these fields;
  it never humanises internal command names. Legacy cards use generic read/edit wording.
  Command labels, input JSON and full summaries remain in the Log. Card disclosures appear only
  when Settings → Show technical details is enabled (`show_technical_details`, false by default).
  Recoverable failures collapse into one calm line per user turn; unresolved failures stay visible.
  Execute calls remain mutating except plan, select and view; execution is unchanged.
- Settings live in `settings.json` in the app's config folder (Linux:
  `~/.config/com.qsmind.qiyas/settings.json`, mode 0600): provider (`anthropic`,
  `openai_compatible`, `claude_code` or `chatgpt_plan`; subscription status above), `providers.anthropic.model` (default `claude-opus-5-5`) and
  `providers.openai_compatible.{model,base_url}` (a new profile has no model selected), per-profile effort,
  MCP on/off, port and token, language, theme. Loading an old flat model and base URL moves them
  into the selected provider's profile and writes the new shape; the other provider keeps its
  defaults. Switching providers in Settings switches the edited profile and clears any unsaved
  key text. The model is chosen from a list (`src/shell/ModelPicker.tsx`, IPC
  `ai_models`, `qiyas-ai/src/models.rs`): Anthropic's Models API (`GET /v1/models`, paged with
  `after_id`) or an OpenAI-compatible service's `GET {base_url}/models`, with a key explicitly typed for that address or its saved, address-bound
  keychain entry; a filter shows past 12 models, "Other…" takes a typed id,
  and when the service can't be asked Claude's built-in list shows (`CLAUDE_MODELS`) with why.
  A readable service list missing the typed/saved model blocks Save; models explicitly lacking
  tools or image input are hidden. Missing capability metadata stays unknown (no paid probe). An empty
  compatible profile can be saved, but sending asks for a model. Both providers explain a
  missing model with a stable error code and direct the user to Settings, in English
  or Arabic in the window. The support log keeps the service and model details.
- The API key is never in a file: it lives in the OS keychain (service `qiyas`, account = the
  provider plus normalized complete base URL, including path), is set only in the Settings dialog,
  and is never sent back to the window. Legacy entries migrate only to their saved address before
  that address changes. Draft model discovery can use a key explicitly typed for that address; without one it requires
  saving the address first. It never reads
  a saved key for a different destination. Remote endpoints require HTTPS; HTTP is accepted only
  for loopback. Malformed endpoints are rejected before credentials are fetched, and HTTP
  redirects are disabled for provider requests.
  Agents never read or type a real key; the founder pastes it himself.
- `QIYAS_AI_MOCK=1` with a debug build (`pnpm tauri dev`) uses a canned provider: it says a
  sentence, measures the first polyline on layer A-WALL and answers; with a selection in the
  message's `[state]` it calls `qiyas_capture` with source screen and `qiyas_execute` select on it instead. No key, no
  network. Ask "draw a demo" or "ارسم" without a selection to see an outline and its partitions
  drawn in two steps on MIZAN-DEMO. Release builds leave it out.
- AI measurements are marked `ai`; the card's Undo only undoes when the last step is the AI's.
- The in-app agent is called Mizan (ميزان): the side tab, the panel and the system prompt use the
  name; code keeps `assistant` / `useAssistant`.
- Mizan never assumes on its own (founder, 28 October 2026): engineering decisions are the
  engineer's. It assumes only when the engineer asks it to assume or to use best practice, and then
  writes each assumption down. When a step needs a missing fact or decision it stops at that step
  and calls `qiyas_execute ask` once, with every question that step needs together (see "W1:
  Mizan's behaviour and cost" below); the turn ends and the answer is the next message. A plain
  text question still ends the turn cleanly (`agent.rs` test `a_question_ends_the_turn_cleanly...`).
  The fixed prompt
  stays under 900 words (a test, excluding the index), in `prompt_en.txt` and `prompt_ar.txt`.
  Both languages append the registry's deterministic `index_text()` under "Commands by category",
  then the drawing facts. The stable index keeps prompt caching working.
- Mizan's eyes (`qiyas-ai/src/image.rs`, `marks.rs`, `execute.rs` `picture`): `qiyas_capture` source picture draws a
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
  `ai_window_reply { id, value, error }`, without the session's lock. `qiyas_capture` source screen (`ai:capture
  { id, maxSide }`: the window draws its main view and reads it back in the same task,
  `Renderer.read`, `Viewport.capture`, `viewport/capture.ts`; the core then marks it, what the user
  selected first), `qiyas_read` selection (`ai:pointed { id }`: the selection, the marked area, the
  view), `qiyas_execute` select (`ai:select { handles, zoom }`: the Select tool's selection set,
  entities drawn at the top of the sheet only; no undo step). Without a window they say so.
  MCP clients reach them too.
- What the user points at (`qiyas-ai/src/context.rs`, `src/ai/pointing.ts`,
  `useAssistantWiring`): the Select tool's selection (or its one pick inside a block, with its
  chain) shows as a chip above the composer ("Selection · 3", its kinds), × leaves it out until
  it changes; the composer's area button starts the `mizanArea` tool (drag a box, or click
  corners and Enter or double click; it then goes back to the tool before and shows Mizan's conversation)
  and an "Area · w × h m · n entities" chip; the canvas menu's "Ask Mizan about this" opens the
  tab with the selection. `ai_send { text, context: { selection, region, view, picture }, images }`:
  `Agent::run_turn_with(UserMessage)` adds to `[state]` the view on screen, the selection and the
  region's entities (handle, kind, layer, length and area in metres, a text's words; 50 listed,
  the rest counted by kind; totals without dimensions and block references), and when the
  selection or region changed since the last message (`picture`) a 1000 px picture of it, marked.
  Pictures pasted, dropped or attached (`src/ai/pictures.ts`: png, jpeg, webp, gif, at most 4, at
  most 1600 px a side, 6 MB of base64 each) go before the words as `ContentBlock::Image`
  (Claude: an image block; OpenAI-compatible: an `image_url` data URL) and show as thumbnails
  in the bubble. At a token-watermark trim, more than 12 pictures or 16 MB makes the
  oldest give way to a line of text until 8 within 10 MB are left (`trim_pictures`); between
  trims the prefix stays unchanged. Identical captures return `unchanged`; a
  conversation may make 40 MB of pictures (`IMAGE_BUDGET`).
- The panel's header has New chat (`ai.newChat`, `assistant.clear`): a new conversation, disabled
  while Mizan works or the thread is empty.
- Mizan never explains how Qiyas draws or fails unless a tool result shows it, and writes formulas
  as plain text; a stray TeX formula in an answer reads as text anyway (`ai/plainMath.ts`). It
  fixes what it drew through core commands (`edit_text`, `modify`, `delete_entities`) or writable records,
  never by drawing it again, never hides layers to hide a mistake, and makes dimensions with
  the core dimension commands.
- Capture source picture draws what the engineer sees: `Session::picture_batch` (the sheet drawn afresh for the
  picture's zoom, nothing cached, so the window's batch and curves stay) and
  `render_png_with_text`: solid fills under the lines, the text as its glyphs from the atlas
  (the text boxes left out). Mizan once saw no text at all and drew labels over labels.
- Looking at a user's drawing from the command line: `cargo run -p qiyas-plot --example plot_file
  -- <drawing> <out.pdf>` (fit on A1; `pdftoppm -png` for a picture) and `cargo run -p qiyas-ai
  --example inspect -- <drawing> [tool[={json}] …]` (read-only tools' JSON; a picture tool writes
  `<drawing>.<tool>.png`).
- Mizan's file operations (`qiyas-ai/src/host.rs` behind `surface::route`): `qiyas_read` drawings
  (tabs and recents), and `qiyas_execute` new_drawing (`units`, ask when not given), open_drawing
  (`path`), switch_drawing (`id`), save_drawing and close_drawing (`id` optional). They run through the
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
- Measuring a drawn thing is by handle (`qiyas_read` records or query, then measure for
  numbers or `qiyas_execute` run with the take-off commands): exact for lines, arcs, circles, LWPOLYLINE and heavy POLYLINE with bulges
  (tested in DXF and DWG). Points are for points the engineer gave. A points measurement whose
  extent matches one drawn entity (within a tenth) still measures, but the answer carries `hint`
  and `candidates` (handle, kind, layer, length, area) so Mizan removes it and measures the
  handle. An AI/MCP item with no geometry and quantity 0 reports `unmeasured_item` advice
  unless `empty_item` is explicit; `add_markup` calls `Command::AddMarkup`.
- Mizan writes into the drawing with any core command through `qiyas_execute` run or batch:
  text (TEXT or MTEXT), tables and cells, lines, polylines with bulges, rectangles, circles,
  arcs, hatches, clouds and block references. Sizes are drawing units; default text height is
  2.5 mm on paper at the annotation scale. `qiyas_read` rooms and the core `LabelSpaces` command
  find the smallest closed region round each room-name text or seed via `Session::room_boundary_in`
  with the enclosure policy above: `enclosed_area_m2` is what the line work encloses, columns that overlap a wall
  are already out, `net_area_m2` also takes the islands off (fountain, shaft;
  `subtract_islands` false keeps them). Bulged edges are exact; texts that look like areas are
  skipped so a second run does not label labels. `label_spaces` writes
  "Area NN.NN m²", or "المساحة … م²" with Arabic digits, just under each name. Verified on
  `samples/test-plan.dwg`: living room 58.29 - 4 x 0.0025 = 58.28 enclosed, 51.211 net of the
  fountain; bedroom 22.085; kitchen 17.385 (the 0.4 m columns overlap the inset corners by
  0.05 x 0.05).
- Offline drafting evaluation: `qiyas-ai/tests/drafting_eval.rs` runs the production Agent
  through the three generic tools using the queued scripted provider shared with
  `tests/surface.rs` (`tests/support/scripted.rs`). A villa and a flange exercise drafting,
  checks, stated-intent verification and captures; a synthetic replay checks earlier Mizan
  fault classes. This proves the specified tool sequence works, not model judgement or
  design approval. See `docs/2026-10-20-mizan-drafting-eval.md` for independent quantities,
  room-boundary limits and the later Claude/Gemini comparison, requiring Kareem's approval
  for each live run.
- Context maintenance counts estimated tokens, not messages. At 100,000 tokens, result
  trimming targets 55,000 and requires at least 30,000 tokens cleared in one batch. It keeps
  the first digest, newest four detailed results, calls, signatures and user words. Full
  compaction starts at 150,000 estimated tokens, keeps a recent window of about 55,000 tokens
  and at least eight messages, and respects complete call/result boundaries. Its local note
  retains user requests word for word, failed outcomes, recent steps, the first digest,
  open tasks, discovered contract references and an index of full-evidence artifacts. It
  makes no separate summary model call;
  the next request uses the same system prompt and tools. `MAX_INPUT_TOKENS` (250,000)
  stops only when this cannot make room. Every rewrite logs an expected cache rebuild.
- An answer cut off by its length limit before it wrote text or a call (its room went on
  thinking: Gemini through Runware once thought 94 s and wrote nothing) is asked again with
  twice `max_tokens`, up to `MAX_TOKENS_CAP` (64,000), the empty answer dropped; past the cap
  the turn stops with `max_tokens` as before. Log line `RETRY`.
- Agent log: `logs/agent.log` beside `settings.json` (mode 0600, moved to `agent.log.1` past
  5 MB). Each conversation's provider, model and address, then every user message, request,
  answer (text, calls, tokens, time), tool run (input, result) and error with the service's
  whole answer, and for a 4xx the request sent (`provider::request_for_log`: compact JSON,
  pictures as `<image>`, tools as their names, the system prompt as its length, scrubbed of the
  key first, then its LAST 4,000 characters after a `…` when longer); other texts clipped at
  4,000 characters, never the key. Settings → Technical details (for support) reads the newest
  256 KB (`ai_log`) on demand and includes the window's last 50 diagnostic failures. The chat
  header has no Log button; errors link to Settings. Raw Thinking is hidden in the thread.
- OpenAI-compatible streams: Gemini (also behind Runware) sends parallel calls whole, all at
  index 0 or with none, so a new id, or a second name, starts a new call (`openai.rs`, `slot`).
  Input that was not valid JSON goes back as `{}`, as for Claude, so one bad call can't make
  every later request fail. An assistant message with calls and no text sends `"content": ""`
  (Gemini's endpoint, Runware in front of it, refuses `null`); a tool message holds only `role`,
  `tool_call_id` and string `content`. A call's streamed `extra_content` (Gemini 3's thought
  signature) is kept by call id in the provider and sent back on that call. Each request asks for
  the effort (`reasoning_effort`: low, medium, high for high/xhigh/max), the answer's length
  (`max_completion_tokens`, else `max_tokens`) and the usage (`stream_options.include_usage`); a
  400 explicitly refusing an unsupported parameter can drop that parameter and retry. Output
  caps always remain: an unsupported `max_completion_tokens` switches to `max_tokens`; a range
  error or refusal of both names stops with `ai.error.output_limit`. Cap-name choices are not
  restored across conversations. Cached and reasoning tokens are read into `Usage` and the log ("(n
  thinking)"); reasoning accepts both `usage.completion_tokens_details.reasoning_tokens` and
  `usage.reasoning_tokens`. Each actual request, including optional-field retries, logs
  `sent:` with the controls present in its body. Both providers turn a missing-model reply
  into the service and model name followed by “choose one in Settings”.

## MCP

- `DrawingHost::after_tool(name, input, ok, took)` hears every finished tool call; the
  headless host appends `{"done", "op", "ok", "elapsed_ms"}` to `mcp-events.jsonl`, so a run's
  per-step speed can be read (the goal X gate: every step under 3 s in debug, 1 s installed).
- Headless: `qiyas-cli mcp --drawing PATH [--workdir DIR]` serves the same three tools
  over rmcp stdio, without the desktop, settings or keychain. The workdir defaults to the
  drawing's folder; an outside drawing is copied into it. Only that drawing is opened,
  without project sidecars or external drawing resources. PDF page 1 uses eval's uncalibrated
  preparation. `qiyas_capture source picture` works; screen and selection return `needs_window`.
- MCP instructions give neutral task rules and the command categories. Quantity workflow is
  read from `workflow {language: "en"|"ar"}` only when needed. `finish {answer?}` records
  completion and reports without saving the drawing; `save_drawing` is explicit. Stdin EOF
  still saves partial work in the run folder. Drawing completion has no requirement for BOQ
  rows and writes a drawing check in `end-report.json`. Quantity jobs additionally write
  `takeoff.json`, `measurement.json`, `takeoff.xlsx`, `self-check.json` and `conflicts.json`.
  Each command/batch keeps its existing single undo boundary, source `mcp`.
- Headless file paths stay in the workdir; traversal, symlinks, truth, recipes and settings
  are refused. Only `path`/`*_path` fields are files; a `path` under `updates` or `where` is
  a JSON pointer and is never checked as a file. One server owns one drawing; start another process to open a different drawing.
  This host does not supply desktop tabs or a project workspace.

Outside AI apps can drive Qiyas through its MCP server. `tools/list` offers exactly the same
three tools as Mizan: `qiyas_read`, `qiyas_execute` and `qiyas_capture`, using `surface::tools`.
`mcp::tools` adds the optional `request_id` to execute: the same id and arguments return the
first result without another change or UI event; different arguments answer `request_id_reused`.
Each MCP connection retains its last 256 ids, including failed results; concurrent retries wait
for the original result. Mizan can omit the id.
`surface::route` sends window operations to the window runner, file operations to the host,
and session operations to the shared surface runner. Measurements are marked `mcp`; undo_own
only undoes that source's newest step. Mizan runs session operations on the blocking pool with
cancel and epoch checks; MCP runs them synchronously. File operations move the active drawing
and Mizan's turn epoch together.

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

- Subscription evaluation: `qiyas-cli eval-agent (--case ID | --cases a,b | --set live-dev)
  --agent codex [--agent-cmd TEMPLATE] [--model M] [--effort E] [--timeout-min N]`.
  Defaults: `gpt-6.1-sol`, medium effort, 20 minutes per case. It never loads Qiyas provider
  settings, keys or a budget ledger. The external client uses its existing subscription login.
  Each case starts with a drawing copy and, when declared, clause-only `preamble.md` in a
  fresh `.local/benchmark/runs/` directory.
  The harness keeps agent events, MCP calls and eval-compatible scoring reports, headed
  `AGENT ALONE (<agent>)`, with cost `subscription`. Logged access to truth, recipes, the
  benchmark store or repository outside the run contaminates the case and excludes its score.
  This is an event-log audit, not a complete filesystem isolation guarantee. Custom clients
  must emit command/file-read events and use subscription authentication.
  See `docs/2026-10-22-agent-eval.md` for the template contract and reviewer command. Do not
  run real agent evaluations during implementation checks.

- Final exam: `qiyas-cli eval-agent --holdout --i-understand-this-opens-the-holdout
  --agent codex --model gpt-6.1-sol --effort medium --timeout-min 20`. Only the reviewer runs
  this, deliberately once; implementation checks never open the real holdout. Without the long
  flag it refuses. The exam verifies every published SHA-256 over `drawing.dxf || truth.json`
  before starting any agent, restores the sealed directory's permissions, and retains verified
  inputs in memory. Agents receive only a drawing and clause-only preamble in fresh case folders
  under `.local/benchmark/runs/holdout-exam-<timestamp>/`. The shared scorer traces saved geometry and embedded rules;
  the usual contamination guard still excludes contaminated cases. `report.json` includes explicit
  north-star accuracy within ±2% over all truth items (target ≥95%), coverage (target >90%),
  submitted-item traceability, seeded-error recall, item precision and seeded-error precision.
  Undefined rates are null. `.local/benchmark/holdout-log.jsonl` appends an opening event before
  accessing sealed contents and a completed/incomplete/failed event with metrics afterward; both
  retain time, agent, model, git commit and report path. An interrupted opening remains recorded.
  No automatic retry. Development loaders and `bench list` remain sealed/hash-only.

- Command scripts (`qiyas-core/src/script.rs`, `.scr`): one command per line, its points on the
  same line or the lines after it; a line starting with `;` is a comment; blank lines are
  skipped; words in any case. Points as the window's command line: `x,y`, `@dx,dy`, `@d<angle`
  (degrees), drawing units, world coordinates, Arabic digits and `٫`/`،` accepted; the last
  point carries across commands for `@`. Double quotes keep a name with spaces together. The
  whole file is checked before anything runs; an error is `line N: …`, and a run stops at the
  first failing step with its line.
  Words outside the legacy script grammar fall back to `cmdline::Machine`, including
  `LINE` / `L`, `PLINE` / `PL`, `CIRCLE` / `C`, `NEWLAYER`, `NEWLAYOUT` and `RENAMELAYOUT`.
  Answers may follow on later lines, and the last point carries between the legacy grammar
  and the machine. A blank line or end of file finishes a variable point sequence; `C`
  closes a polyline. Quoted names, Arabic digits and separators work in both paths.
  The legacy grammar has priority (`MEASURE` still takes an entity handle in scripts).

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
  | `MEASURE <decimal handle or 0x…>` | `MeasureEntity` of the drawing's own entity. |
  | `EXPORT [xlsx path]` | The take-off to Excel; bare, `{name}.{ext}.xlsx`. |
  | `IMPORTBOQ <xlsx path>` | BOQ from Excel, then `SetBoq`. |
  | `COMPARE` | `compare_session`, default tolerance. |
  | `ZOOM E\|EXTENTS\|IN\|OUT` | Nothing headless (`W` is refused). |
  | `ECHO <text>` | Prints the text in the report. |
  | `{"type":…}` | Any `Command` in its wire form, e.g. `{"type":"draw","kind":"line","a":[0,0],"b":[0,5]}`; its `path` and `…_path` fields go through the folder rule below and become absolute. |

  Script coordinates remain world coordinates: the whole file is parsed before its commands
  run, so a UCS command does not change how later script points are read. `ME` (measure along)
  now reaches the registry fallback; `MEASURE` retains the legacy handle-only grammar. Paths are
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
- `qiyas-cli` (`crates/qiyas-cli`, clap without colour): no window. Ordinary drawing commands
  and `bench` are offline and never read provider settings or credentials. Only `eval`, with
  explicit approval for each run, reads saved provider settings and the OS keychain itself.
  - `bench generate`: 60 deterministic dev cases in six project types, each with 12 variants,
    in git-ignored `.local/benchmark`; six separate holdout cases are sealed and listed by hash.
    ResPlan conversion skips and reports invalid whole plans before writing any case, continuing
    to later plans; `<source stem>.conversion.json` and stdout retain rejection reasons.
  - `bench run`: recognised element mappings, three repetitions (quick set: one),
    with scored JSON/Markdown reports. `bench score --case ID --submission FILE` accepts
    JSON take-off/BOQ or Excel. Dev commands never enumerate or open the holdout.
  - `eval --case ID --approve-live-run [--provider-from-settings]`: real headless Mizan,
    OS keychain credentials, local budget reservations and token/cost accounting. Requires
    explicit model pricing in `.local/autopilot/budget.json`; the daily default is $5.
    Never run live evaluation without Kareem's OK for that run. Tests use a scripted provider.
  - Schema, scoring definitions, source conversion and limitations:
    `docs/2026-10-21-qiyas-qs-benchmark.md`.
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
- The founder approved embedding Thmanyah in GitHub installers on 23 October. The release
  workflow unseals the existing private font archive and verifies all three weights. Official
  builds fail if its secret is absent; forks explicitly report their system-font fallback.
  Never read or print the secret or commit the unsealed fonts. Vite embeds the local fonts.

## Where the plans live

In this repo, `docs/` (write new specs, plans, checks and reports here, never in the workspace):

- QS roadmap: [docs/2026-10-21-qiyas-qs-roadmap.md](docs/2026-10-21-qiyas-qs-roadmap.md)
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

## Drawing package rules (live-run corrections, 21 October 2026)

- (1) `missing_center_mark` excludes circles containing a nonempty TEXT/MTEXT insertion on
  the same sheet, regardless of layer. Grid bubbles carry `QIYAS_GRID` provenance, so removing
  their labels or changing their layer never turns them into holes. Plain circles still need marks.
- (3) `add_wall_openings` reports resolved sill/head in metres from the picked run (window 0.9,
  door 0, head min(2.1, height)). `edit_wall_opening` reports retained omitted properties and
  their physical units; removing openings has no implicit size defaults.
- (7) Verification results include `tied_to_drawing`. Arithmetic on literal points is false;
  entity targets and counts read the drawing. The overall `untied_checks` and `line` explicitly
  say how many checks prove nothing about the drawing, even when their arithmetic passes.
- (9) `opening_schedule {at, source_sheet?}` / `OPENINGSCHEDULE` builds an ACAD_TABLE on the
  current sheet from wall systems on the source (Model by default). Rows group by explicit ID,
  falling back to a size mark from kind, width and head minus sill, with dimensions in mm.
  Display marks such as D1 or D-900x2100 are assigned to inserts in `QIYAS_OPENING_MARK` xdata and returned by `wall_groups`; those marks
  can be used in plan tags. Count cells and the total use `QiyasOpeningCount` fields linked
  to the source sheet handle. `QIYAS_OPENING_SCHEDULE` keeps the source on the table. Drawing
  edits refresh groups, marks and counts within their undo step, including newly added sizes;
  DWG/DXF retain the values and links. Table location and column widths stay as placed.
  On layer 0 the generated table uses `A-ANNO-TABL`, created in the same undo step without
  changing the current layer. Initial cells are sized from displayed quantities, not field code.

## Goal F: window language and layout (23 October 2026)

- Arabic glyphs use Thmanyah first in every UI and monospace stack. Its Unicode-range faces
  leave Latin text in Instrument Sans or monospace. Inputs, selects and textareas inherit the
  font. All heading levels, legends and tabs enable swash, regardless of UI language.
- Chat language is inferred from prose after ignoring code, layer identifiers and attachment
  metadata. List items and table cells each follow their own prose direction. Latin
  words and quantities are isolated with `bdi`; Arabic translation substitutions use Unicode
  isolation. The UI keeps Western digits. See the glossary in the Goal F report.
- PDF transport metadata is rendered as file-name/page chips, including restored conversations.
  Paths are reduced to file names in chat. Approval buttons and the posted approval sentence
  follow the chat language. Both system prompts forbid tool/command/field/handle names in
  answers, and require brief surveying language. This is a generation rule, not a claim that
  every live provider has been evaluated.
- Caught backend errors go through `src/i18n/errors.ts`: known failures get a plain translated
  reason; unknown failures get a translated fallback with support guidance. Raw errors stay
  in the support buffer. `userError` preserves an already translated message across catches.
  Assistant control-flow codes remain internal and are translated at their display boundary.
- `entityKind` supplies plain English/Arabic labels across properties, selections, extraction,
  recovery and fields. User views omit handles; evidence actions say Show on drawing with a
  count. Explanation and generated-quantity panels show readable quantities and citations;
  formulas containing internal fields, definitions and diagnostic notes sit under Technical
  details. Script results show plain status with command/output details folded. Project outputs show file names. Take-off rows omit unspecified source/basis filler
  and explain count limitations in plain words. No quantity, command or check rule changed.
- Delete all conversations opens a confirmation naming the count; Cancel or Escape preserves
  the saved chats. Single-conversation deletion is unchanged.
- Report and evidence: `docs/2026-10-23-qiyas-goal-F.md`. No new dependencies or licence changes.

## Honest commands and stable handles (Goal B, 23 October 2026)

- Drawing edits keep the requested field changes before derivation and verify them after
  object geometry, hosted positions, schedules, measurement and linked fields finish. An
  overridden field now reports `derived_value_changed` advice with requested/actual values
  and the responsible update; other valid changes remain. Integrity failures still roll back. Entity
  fields refresh only in the final derivation pass, after object geometry and schedules.
  Computed closed outlines and hatch boundaries compare by their full edges, arcs and widths, allowing a different
  starting vertex or direction and floating-point round-off. Ordinary vertex order stays
  significant. Hosted angles compare modulo a full turn.
  Entity comparisons ignore raw drawing caches. Unchanged entities and unchanged take-off snapshots
  do not enter the change list or create undo steps; a no-op retains redo. Unchanged table
  cells also skip rebuilding their display blocks.
  If an edit changes no entity, drawing table or take-off value, derivations are skipped too;
  this includes empty edits after reopening a DWG or DXF. Explicit `update_fields` still refreshes.
  Unchanged take-off, BOQ and conflict edits likewise report no change and retain redo.
- Opening `mark` is an explicit ID. Automatic size marks remain display defaults and never
  replace that property. Opening schedules group by the explicit ID when present.
  `edit_wall_opening` accepts `mark`; an empty edit keeps its handles and makes no undo step.
  Old size marks written into the property by previous versions remain automatic; explicit
  edits carry an ID-source marker.
- Moving a hosted opening along its wall updates the position property named by its host-cut
  recipe, in metres. A perpendicular move is refused. Copy an opening with its host, or use
  `add_wall_openings`; copying it alone is refused before creating overlapping geometry.
- Generated parts cannot receive raw record, text or vertex edits. Their owner is named in
  the refusal. Whole-object transforms and erase through a part selection act on its owner.
  Colour, linetype and lineweight through `set_property` apply to the owner and
  its generated graphics. A part's direct layer or shape edit is refused. Retained room boundaries
  follow enclosing walls; direct vertex, shape and transform edits are refused.
- Live schedules refuse row/column/merge changes. Cells are editable only for a recipe that
  explicitly preserves overrides. Change the source objects or create the desired schedule.
- Layer renames and merges update persisted layer references in object instances, types,
  schedules, wall metadata and drawing knowledge, including layer-property defaults and
  choices. `plan.layer_properties` binds editable
  text properties to role layers; `plan.host_layer_property` binds a hosted driver's layer
  to its host's property. Wall layer properties use these data bindings and actually apply.
  Role-layer edits on a joined wall update every run because the outlines and hatch are shared.
  Placement role layers initialise their bound properties; conflicting explicit values are
  refused. Unchanged generated graphics reserve their handles before changed graphics match.
- Wall rebuilds reuse surviving driver and member handles and retain presentation properties.
  Surviving centrelines retain their constraints. Joined groups give shared graphics to
  the smallest surviving driver handle, matching the object engine even when run order differs.
  Selected-object schedules and linked object fields follow replacement maps. Within a
  transaction, literal handles and bound `$step` results follow single replacements; ambiguous
  replacements produce a clear refusal. The MCP boundary resolves them before validation.
- Command handles are decimal. Decimal strings and explicit `0x` hexadecimal strings are
  accepted at the JSON boundary, including object record keys. Target objects also accept
  a bare handle. MCP record, entity and measurement reads accept the same handle strings;
  operation shortcuts normalize them before their argument validation, including captures.
  Script `MEASURE` takes decimal, or explicit `0x` hexadecimal, matching the
  command boundary. DWG field codes keep their required hexadecimal storage syntax.
  Older schedule templates using `id_hex` remain readable but display decimal handles.
  Properties, constraints, field target labels, recognition results and BOQ evidence also
  display decimal handles. DWG field-code construction keeps its hexadecimal encoding.
  Display tests that expected `2A` for handle 42 now expect `42`; field-code tests still
  require the DWG hexadecimal form.
- Unknown fields on editing, file, session and view commands are errors. Read commands retain
  their warnings. Tests that previously required ignored edit fields now require refusal and
  unchanged state; default-value coverage remains on valid inputs.

## Goal E: providers and clear errors (23 October 2026)

- Provider failures return stable `ai.error.*` codes with plain English `message` and Arabic
  `message_ar`. `AiError::for_window` serializes this envelope in the existing `ai:done.error`
  string; model-list failures use the same envelope. `src/i18n/errors.ts` recognizes the code
  before legacy text patterns. Both languages live in `src/i18n/en.json` and `ar.json`; Rust
  includes those catalogs. Raw, scrubbed diagnostics remain in the support log.
- Billing/credit/quota failures never retry, even when delivered as 400/403/429/5xx or a stream
  error. Classification sees the complete scrubbed response, before support-log clipping. Temporary 429, 5xx, connection failures and interrupted/overloaded streams use bounded
  backoff. Each wait logs `RETRY`, the reason, wait and available partial text, and emits
  `retrying {seconds, code, discard_partial}`. Unfinished text is removed before a repeated
  response; no tool from an unfinished response executes. The existing retry after an answer
  uses all its room thinking also emits the notice. Cancellation interrupts the wait.
  Strict-budget providers disable automatic retries; model discovery does too. HTTP requests
  now have a 10-minute total deadline in addition to connect/read timeouts.
- A context-limit refusal gets at most one smaller request. Large tool-result text and pictures
  are shortened, with explicit reread markers and intact call/result pairing and user words.
  If no space can be made, or the retry fails, the translated error asks for a new chat. Normal
  compaction, prompts and drafting workflow are unchanged. A provider error never emits a
  take-off end report into a drawing chat; it leaves the already completed work in place.
- Model discovery accepts a draft key only for the explicitly typed address, after endpoint
  validation. Editing that address clears the draft key. Without a draft key, Goal A's saved
  address check still happens before keychain access. Discovery refreshes after typing a key.
  Save checks a typed model against a readable filtered list. Unknown capability metadata is
  kept; explicit lack of tools or vision is hidden. No model invocation tests run on Save.
- Profiles keep independent effort: new Anthropic profiles default to medium, compatible ones
  to low. None omits the effort field (the service default); compatible services show only
  none/low/medium/high. Old global effort migrates only into the selected profile; old
  compatible xhigh/max becomes its previous effective value, high. Headless eval reads the
  selected profile too. Changing effort applies to the next message without clearing the chat.
  The legacy top-level effort remains for old-file compatibility.
- Streaming tokens append in memory without copying the full conversation. Checkpoints are
  made at response/step boundaries, including errors and cancellation. Desktop progress saves
  occur on the opening message and completed steps. The existing AI lock still serializes
  file writes with conversation reset/deletion, avoiding stale or overlapping writes.
  Final conversation saves retain the existing recovery contract.
- Partial compatible-provider usage objects update only the supplied counters, including
  cached/reasoning tokens and DeepSeek cache aliases. Output limits are never dropped merely
  because a value error names the field.
- Settings offers **Replace token on Save**. It generates 16 random bytes locally, updates the
  displayed setup snippets, and only Save persists it and restarts MCP. Cancel keeps the old
  token. Existing MCP address/origin restrictions remain in place.
- Evidence and remaining sandbox restrictions: `docs/2026-10-23-qiyas-goal-E.md`. No new
  dependencies, paid calls, commits or pushes.

Headless take-off evaluation keeps its own brief stopped-run summary when a request/cost cap or
provider error arrives before a completed answer. That report belongs to the evaluation host;
it is not appended to desktop drawing chats. The existing budget-stop report test remains intact.


## Goal G: drawing quality and honest checks (23 October 2026)

- `ink.rs` builds a spatial index of plotted geometry with top-level ownership. It flattens
  tables, dimensions, blocks, attributes, leaders and hatches, excludes non-plotting/hidden
  layers, and shapes text through the same `TextEngine` as the viewport. Query bounds also use
  shaped text. Checks use full oriented footprints, including text edges, solid fill interiors
  and table cells. Text is also checked against its own cell borders or symbol geometry; the reserved table
  footprint itself is excluded from these internal checks. Dimension extension-line connections
  and arrows touching measured geometry are allowed.
- Added `annotation_overlap`, `annotation_over_grid`, `label_outside_room` and
  `object_tag_mismatch`. The minimum readable text height is 2.5 mm on paper. Annotation
  warnings introduced in a Mizan phase join its existing error gate. Automatic results report
  `layout_findings` and coverage; the English/Arabic prompts add only a short repair note.
- Registry `free_space {size:[w,h] | entity, near?, side?:any|right|below, clearance_mm?,
  sheet?, within?}` reads without changing drawing/history. Size and coordinates are drawing
  units; clearance is paper mm (default 2.5). It returns up to eight ranked top-left positions,
  bounds, deltas, current collisions, and occupied plotted bounds. A finite candidate search
  can return no position; that does not prove that none exists. Incomplete source geometry is
  refused rather than treated as empty space.
- Registry `auto_place {targets, near?, side?, clearance_mm?}` places tables and labels in
  available space in one undo step. Live room labels stay inside their own region, wrapping
  when necessary. Linear/aligned dimensions retain measured endpoints and search both sides
  for a clear dimension line, including extension lines against grid bubbles and annotations.
  Explicit dimension creation coordinates stay literal: use the helper to resolve findings.
  Failed placement rolls back. `room_schedule` and `opening_schedule` accept omitted `at`,
  placing beyond the annotated extents with 10 mm paper clearance. Explicit `at` is top-left.
- Registry `object_tags {targets, property, height?}` labels any text/number object property,
  including door/window `mark`, at 2.5 mm paper height by default. Tags store the object's
  persistent identity, follow its position/property through edits, undo and DWG/DXF, and reuse
  existing tags. Rebuilt source handles follow the existing field-remapping pass. They use the
  same free-space search. Manual offsets are retained when the
  source moves. Missing sources or divergent tag text are findings. Opening schedules supply
  their marks; custom mark policy remains Goal B's work.
- The legacy door recipe uses `host_width / 2` for the hinge's normal offset: the face on the
  swing side. No object or wall engine logic changed. New linetype painting leaves measured
  paths continuous and applies dash phase across path vertices. Screen/capture/PDF share it.
  Hatch/room boundary tracing uses `flatten_continuous_within`, including block arrays: printed
  gaps never open or divide the underlying CAD geometry.
  Imported PDMODE 1 points draw nothing; PDMODE 0 is a dot. Positive PDSIZE is a drawing-unit
  size; negative PDSIZE is a percentage of the sheet extent (not a zoom-dependent viewport size).
- Dimension generation accepts the rendered text width for gaps and leader landings, so radius
  and ordinate text no longer rely on a character-count estimate.
- Approval remains in the room/take-off report. Plan wording stays `Area 19.31 m²` /
  `المساحة ١٩٫٣١ م²`, with Arabic digits and decimal separator. No new dependencies or licence changes.
- Evidence and exact check results: `docs/2026-10-23-qiyas-goal-G.md`.
## Goal M: measurement by name (23 October 2026)

- `mcp`, `eval` and `eval-agent` accept `--method "POMI (RICS)"` (or any method name).
  Eval overrides the saved choice for that run. External agents receive the same name through
  the generated MCP configuration and request; custom runner templates also have `{method}`.
  Headless MCP does not load app settings or credentials. Local method rules are optional.
- Rules authored with `measurement.set_method` remain drawing knowledge through save/reopen.
  Reapplying the same name-only Settings selection does not erase them or add an undo step.
  Clearing Settings leaves an explicit project override in force.
- Generated items retain their method and rule citations through dimension lines. The standard
  take-off workbook now shows Method and Source / clause on its BOQ sheet too. Default BOQ
  deliverables show both columns; dimension deliverables show Method beside their citations.
  Citation objects render as source / clause instead of empty reference text. Custom layouts
  keep their chosen columns; lossless records retain the complete trace.
- Offline tests use visibly invented clause markers and geometric transport fixtures, never
  a published standard's rules. They check plumbing and persistence, not model knowledge or
  correctness of any standard interpretation. No paid provider is used. Evidence and real
  check results are in `docs/2026-10-23-qiyas-goal-M.md`.

## Goal D: Mizan's judgement (23 October 2026)

- Drawing chats receive no injected take-off workflow or BOQ completion notice. Job scope comes
  from the request and operations used, not drawing names or layer conventions. Continuations
  retain their scope; a fresh drawing request resets it. Measuring a handle for a numeric read
  does not start a take-off. Actual quantity commands and `takeoff_kit` do. The stable system
  prompt and neutral command index stay cached. Old saved prompts with an unconditional
  quantity workflow are rebuilt on resume; their tied provider signatures are dropped, while
  requests and tool results remain. Both fixed prompts remain below 900 words.
- Workflow and budget guidance follow the chat language. The separate workflow read accepts a
  language; the kit keeps its existing size bound. Prior injected workflow guidance is removed
  at the next turn, then supplied only for the current job in the current chat language.
  Mizan supplies the chat language itself. Drawing budget notices ask for requested checks
  and remaining work. Interrupted take-off reports run only for quantity jobs and follow the chat language.
- `finish` never calls Save, opens Save As, or reports a saved drawing. The default desktop host
  records completion only. Explicit Save and headless EOF retention remain separate operations.
- An identical failed or ineffective input gets advice on attempts two through five; only
  the sixth is stopped after five failures in one turn. The fingerprint includes the complete
  normalized input and drawing revision. Reads do not reset it; changed input or state is a
  different attempt. MCP request-ID retries return the cached result without another execution;
  a successful `finish` ends its turn and resets retry/churn tracking.
  Core edit results expose `no_change` from the committed undo revision, including table edits.
- `redo_own` and typed `REDO` restore the original next redo step only when the caller owns it;
  typed `UNDO`/`U` use `undo_own`. Handles and the original transaction are preserved. Sheet
  authority, drawing epoch, and ordinary undo safety still apply.
- `qiyas_read panel_state {}` reads through the existing window request/reply channel
  (`ai:panels` / `ai_window_reply`). It returns visible panel folds, the active side tab,
  current layer, flat/folder mode, expanded groups, selected layer rows, open layer properties,
  drawing selection and open dialog labels. It uses rendered React state, including choices
  not saved to storage. It changes nothing and needs no drawing. Headless replies `needs_window`
  with a plain no-window explanation. A screen capture shows the drawing canvas only.
- New warnings/errors on touched geometry are attached as `open_findings` to the saved visible
  turn. Final answers and phase completion proceed. The full baseline check excludes findings
  that predate Mizan's changes; unfinished findings survive resume. Selected approval modes
  still pause at their explicit boundaries. Prompts ask for readable labels, picture review
  and consideration of the findings. Panel questions authorize explanation, not unsolicited edits.
- Scripted-provider tests and headless evidence are documented in
  `docs/2026-10-23-qiyas-goal-D.md`. No paid calls or new dependencies.

## Goal C: reliable inputs and incremental updates (23 October 2026)

- Record `fields` and `where.path` accept summary names, including `layer`, `length`, `area`,
  `text`, `key` and `type`; `handle` aliases the key. Bare raw-data keys also work. Existing
  JSON pointers keep their data meaning and can fall back to summary paths such as `/length`.
  A malformed path or a path absent from every record is an error naming available fields.
  On mixed records a missing projected property is omitted and named in page `warnings`;
  an explicit null stays null. An empty scope reports that paths could not be checked.
  `exists` can deliberately test an absent path. Missing properties never satisfy `ne` or
  `not_in`. `in`/`not_in` need arrays and match text without case; ordered comparisons require
  two numbers or two strings. Record lengths, areas and bounds remain in drawing units.
- All query kinds accept `bounds` (normalised with a warning to `within`); text queries also accept a case-insensitive `layer`.
  Returned text rows include the effective layer, with layer 0 inherited inside blocks and
  the external drawing prefix. Supplying both bounds names is refused as a duplicate.
- JSON `set_layer` accepts several named properties in one undo step: color, linetype,
  lineweight, on, frozen, locked and plot. The existing prop/value form remains valid; mixing
  the two forms is refused. The wire normalizer maps the shorthand to the native properties
  operation. Colors accept the existing ACI/hex notation, layer color indices 1–255, and
  red/yellow/green/cyan/blue/magenta/white. Named colors work in other color commands too.
  `delete_entities {handles}` aliases targets and uses the same handle normalization and
  protection rules. Supplying handles and targets together is refused.
- `qiyas_execute run {command, layer?, color?}` uses the existing atomic batch context,
  including making a missing layer and restoring the current drawing properties afterwards.
  Its wrapped batch outcome reports the actual entity changes. Read-only run stays read-only.
- `commands {name}` discovers surface operations such as query and finish as well as core
  commands. Qualified names (`qiyas_read.query`, `qiyas_execute.run`, `qiyas_capture.picture`)
  select the exact operation when names overlap. Operation results include schema and example.
  Scalar enum unions list every allowed value in validation errors. Record schemas seed nested
  codec enum variants, so valid text alignments are accepted. Record documentation and
  capabilities say 20 records by default, 200 maximum, and a 12,000-byte budget for records
  and warnings, excluding the page envelope; a single oversized record can exceed it. The generic surface has its separate result byte limit.
- Derivation compares immutable entity snapshots and source definitions from before the edit.
  Object refresh follows hosts, union groups and block references; retained regions follow their
  sheet/enclosure sources; schedules follow their selected objects; linked fields follow their
  referenced entities and counts. New or edited take-off rows refresh retained-region validity
  and metadata without retracing unchanged enclosures. Definition/lookup changes conservatively
  refresh dependents.
  Generated label defaults use the driver's own sheet, so switching the viewed sheet cannot resize them.
  Full derivation establishes a consistent state when opening or reloading linked drawings
  and remains the differential-test reference. No persistent derivation cache can go stale after undo or rollback.
  Legacy wall migration preserves the role/index of already converted parts with the same
  owner, so editing a copied wall does not rewrite its source group's metadata.
  Goal B's requested-field comparisons and final net change reports remain in force (Goal L makes overrides advice).
- Evidence, timings, checks and limitations: `docs/2026-10-23-qiyas-goal-C.md`.


## Goal I: readable panels and tables (24 October 2026)

- Take-off, BOQ and conflicts use the shared content-sized, wrapping DataGrid described above.
  Text width measurement visits only rendered rows, caches by font and text, and resets after
  fonts load. Inputs and lists use their visible value, excluding unchosen options. User resizing starts at the rendered width. A hidden panel cannot zero its row
  estimates. Markup measurements show Note and no unit; the footer counts Notes separately
  from measured totals. Totals wrap and isolate their numeric values in either language.
- (Replaced by W4: the side tab strip and `SideTabs` are gone. The conversation and every output
  stay mounted in Mizan's column, one shown.)
- Arabic chat has a complete Thmanyah face for prose, inherited line height and baseline inline
  elements. Body text explicitly disables swash; headings keep it. Latin identifiers still use
  isolated `bdi` runs. The original raised words are plain text in the DOM; local HarfBuzz gives
  their base letters zero vertical offsets. WebKitGTK painted confirmation remains unavailable
  in this sandbox, so these safeguards are not a claim of a visually verified glyph fix.
- Settings stores `show_technical_details` through the existing settings IPC and settings.json;
  old files default to false. The app reads it at startup and applies saved changes immediately.
  Tool cards, review-plan diagnostics and refusal diagnostics hide disclosures when off. The
  support Log and stored transcript remain intact. Recoverable errors use one line per turn,
  with grouped diagnostics available when enabled; final failed steps and actionable alerts stay.
  Recovery presentation requires a later successful retry with the same operation label in the
  same turn; unrelated successful reads cannot hide failed edits. While a turn is still busy,
  errors share a Fixing a problem line. No provider or agent execution behavior changed.
- Evidence and remaining visual acceptance: `docs/2026-10-24-qiyas-goal-I.md`.
## Goal J: phased work and stable edits (24 October 2026)

- Mizan and MCP track their own top-level additions/removals by drawing and phase. Rebuilding
  again returns `repeated_rebuild` advice naming handles and cheaper edits; it does not refuse.
  Results include per-call `churn` and cumulative `phase_churn` counts. Reads and phase revisits
  preserve the counts; each user turn (or successful MCP finish) starts afresh.
- Plans accept 1–24 phases; one phase returns small-phase advice. Unique IDs and one active
  phase keep the checklist valid. Text/dimension edits retain their identities through
  `auto_place`, `dim_space`, `modify move`, `dim_edit` and `edit_text`.
- A new-design request on existing content supplies destination context to Mizan, which decides
  whether a question is needed. Qiyas no longer stops before calling the provider.
- Each turn supplies reply-language guidance from the latest user prose. Arabic defaults to
  simple Modern Standard Arabic; only current dialectal prose invites a dialect. An earlier
  Arabic greeting or interface language does not override later English. All-capitals English
  is prose; digits alone keep the current chat language. Generated guidance is not user history
  when the conversation is compacted.
- Unambiguous input shortcuts report `normalised_input` warnings: one `where` object becomes
  an array; a single `handles` entry becomes `handle` where the schema needs one; a lone
  command with no execute `op` becomes `run`; query-window `corner1`/`corner2` become `within`.
  Text/table/dimension/block/layer queries accept a page `limit`. Competing forms and multiple
  handles for a single-handle operation remain errors. Registry validation and execution use
  the same canonical forms. Normalization follows the matching schema variant, preserving
  legitimate plural handles in text and attribute schedules.
- An empty room-name query lists candidate text layers and counts of short names inside
  closed regions, including when the requested layer does not exist.
- AI/MCP quantity notes that state a value no measured take-off row shows return `uncited_quantity`
  advice and remain in the drawing (W3: a value a row shows is cited by that value; a `takeoff:<id>`
  is still read but is never to be printed, see W3). This covers descriptions, notes and comment-label edits.
  Text does not establish a measured quantity. Engine discrepancy comments retain their query
  evidence and undo behavior. English and Arabic notes use the same advice rule.
- Unsupported measurement-method versions name supported version 1; the registry and
  take-off kit examples include `version: 1`. No dependency or provider changes.
- Evidence and limitations: `docs/2026-10-24-qiyas-goal-J.md`.
## Goal H: subscription replies, activity and choices (24 October 2026)

- ChatGPT assembles its completed turn from output-item, text, content-part and function-argument
  stream events. Final `response.completed.output` fills gaps; an empty or partial final array
  cannot erase streamed text or calls. Item IDs, output indices and call IDs keep repeated
  snapshots from duplicating calls or visible text. Reasoning items retain their opaque payload
  for subsequent Responses requests. Interrupted/incomplete responses still execute no calls.
- A completed response with no answer, refusal or call fails with `ai.error.chatgpt_empty` in
  English/Arabic. The support log records item types and usage, never encrypted reasoning or
  authentication material. Possible credential prefixes are held across text/thinking deltas,
  so joining fragments cannot reveal them; assembled refusal text is scrubbed too.
  Completed text, calls and token counts use the ordinary Agent answer
  log. Retained answers and call/results are sent in later requests; the request line also states
  the actual Responses input-item count, distinct from the conversation-message count.
- ChatGPT selects the account's first visible model when no model is selected. Desktop sign-in,
  account selection, model discovery and the first send/resume persist that choice; Settings
  shows its display name and sends its slug. Account changes clear the old choice. Discovery
  failure keeps sign-in and reports the failure. A failed settings write does not update memory.
- ChatGPT effort is optional and sent only when the selected model's discovery entry advertises
  the level in `supported_reasoning_levels` (strings or objects with `effort`). Unknown metadata
  means the service default. The profile keeps its own choice. A named unsupported reasoning
  capability removes effort for the rest of this conversation, including resume; credentials
  and account identity never enter that saved capability state.
- Claude Code status reads only its ordinary version/help/auth-status commands. Model aliases
  and effort levels come from the selected executable's help text. Settings offers Default,
  documented model examples and Other for a full model name; effort offers Default and the
  documented levels. A missing flag hides its control; checking an older compatible executable
  clears choices it cannot accept. A late status from a previous provider cannot change the
  current choice. Checks remain explicit, never polled.
  Default omits the flag; an explicit model or effort must contain only ASCII letters, digits,
  `.`, `_`, `:`, or `-` and must not start with `-`. Values are separate process arguments.
  Legacy global API effort no longer migrates into a subscription profile.
- Claude partial tool events are assembled before the full assistant summary and forwarded as
  the existing activity cards. Received text, calls, results, retry/status and terminal result
  events are written while the turn runs, so cancelling does not lose the diagnostic trail.
  Raw thinking and account/init metadata remain excluded. Goal S's executable, environment,
  private MCP, permissions, cancellation and no-retry safety rules remain in force.
- No model calls, real login, new dependencies or licence changes. See
  `docs/2026-10-24-qiyas-goal-H.md` for before/after evidence and actual check limitations.

## Goal K: wall openings and panic recovery (24 October 2026)

- A wall rebuild establishes surviving source handles and hosted references before the
  object engine generates outlines and hatches. Temporary wall drivers do not generate a
  second set of graphics. The missing-member guard remains; shared graphics belong to the
  smallest surviving driver. Boolean cuts use stable host/position order, independent of
  entity storage order after removal. Goal B's requested-field comparison now reports Goal L advice.
- A batch containing only independent literal opening additions validates inputs in order
  and rebuilds each affected wall group once. Mixed edits and step references keep sequential
  execution. Grouped step outcomes describe the final shared wall state; AI/MCP marks them
  `shared_wall_result` and their change lists describe the whole grouped edit. The batch's net
  change lists remain authoritative. Error steps and fitting ranges retain input order.
- Whole-wall results calculate each complete assembly once instead of summing repeated
  prefix unions. Plain band graphics skip repeated quantity aliases; computed expressions,
  symbols and labels still evaluate quantities. Only the owning driver derives shared geometry.
  Explicit assemblies do not repeatedly evaluate vertical grouping for every member. A name-only measurement method still reports
  missing rules and invalid sources without calculating unused numeric quantities.
- Failed transactions restore structural state, including handle allocation and block
  membership, alongside recorded entities and view state. Wall edits also preserve that
  structure on an individual failure. Document edits and transaction groups catch panics,
  roll back and return a plain error. AI session tools and desktop command boundaries catch
  remaining panics before leaving the session lock. The existing autosave panic hook can
  still report that the active drawing is locked while unwinding; the lock is released when
  the recovered command returns, and subsequent edits and undo continue normally.
- Test-only fault injection is enabled by the existing fixtures feature, on the calling
  thread, after a chosen document mutation. It has no command or MCP input. Regression tests
  exercise the core, AI surface and desktop host; wall rollback covers mm, cm, m and inches.
- `crates/qiyas-ai/tests/fixtures/villa-openings.json` contains the founder's drawing/opening
  inputs only. The ordinary test replays them without a provider; an ignored four-unit variant
  and a release speed guard cover the larger runs. `scripts/check.sh` runs the release guard
  against `tests/support/performance.rs`. Evidence and measured results are in
  `docs/2026-10-24-qiyas-goal-K.md`.


## Goal L: facts, revisions and drawing history (24 October 2026)

- Qiyas refuses impossible operations, broken links/data, stale reads, locked/read-only access,
  invalid requests and actual safety/capability limits. Behaviour judgments return advice for
  Mizan. Both fixed prompts remain below 900 words and explain that distinction.
- Every wire read/edit result includes `revision`: session identity, `document_revision`, a
  monotonic `knowledge_revision`, effective sheet and working-context fingerprint. Captures
  include it in the caption; no drawing uses null. Native `run_input` also reports revision,
  `advice` and `handle_map`. Ordinary Rust outcomes remain ordinary enums.
- Execute accepts optional `expected_revision` in args or the envelope. Stale input is refused
  before a mutation and returns `stale_read`, added/changed/removed handles, composed
  `handle_map`, per-revision maps, changed components and the current revision. History retains
  128 changes with at most 32,768 handle entries each; an expired/foreign/untracked interval
  reports `history_complete:false` and requires a fresh full read. Transaction rollback does
  not advance revision. Undo/redo and knowledge/table/context edits invalidate reads too.
- Rebuild results always include `handle_map`, including retained and removed identities.
  Mizan must use the latest result's revision, including its own edits, and reread changed handles after `stale_read`. MCP keeps
  request-ID deduplication. Desktop file mutations recheck revision under the applying lock.
- Final open findings appear beside the answer in short English/Arabic and persist in History.
  Raw rule names and handles appear only with technical details. Arabic headings use the
  existing Thmanyah swash styling.
- Recovery associates source, autosave/crash copy and recovered filename in the conversation
  store. Save As joins the new path to that history, including inactive chats and scripts.
  `conversations/drawings.json` records path families atomically; old checkpoints cannot erase
  them. Listing/resuming resolves the current path; chat bodies and user messages are retained.
- Evidence, guard inventory and actual checks: `docs/2026-10-24-qiyas-goal-L.md`.

Take-off kit revision workflow examples are named `revision_commands`; `revision` is always the drawing read token. Strict provider schemas require a nullable top-level `expected_revision`; native/MCP callers can omit it or put it in `args`.

Wire array/scalar reads are wrapped as `{value, revision}`; object reads keep their existing fields. Compact kit prose reserves space for revision metadata without increasing its byte limit. A repeated-failure stop still produces the local interrupted take-off summary in the chat language.

New untitled drawings skip placeholder names already present in chat history or a recovery/Save As path family, so a new drawing cannot inherit a previous drawing's conversations. A mixed move whose retained room is restored by derivation keeps the unrelated move, reports the room override, preserves measured area and remains one undo step.


## Goal W: one workspace per drawing (25 October 2026)

- `DrawingWorkspace` owns its stable session Arc, undo state, Mizan, cancel token, provider
  profile, transcript, window request channel, pictures and published data. The registry keeps
  unique tab ids; closed ids are not reused. One canvas displays the active workspace. Hidden
  workspaces run without canvas redraws. View, selection and composer drafts follow the drawing.
- Drawing IPC resolves its workspace before awaiting. Agent file tools and each MCP connection
  remain bound to their original workspace even when they show another tab. MCP callers can
  explicitly address a workspace; retry ids and callbacks remain local to that drawing.
  Autosave visits every workspace with try-lock; busy drawings retry on the next tick.
- Each Mizan panel chooses provider, model and effort. Settings holds defaults for new
  workspaces. Changing a workspace preserves its conversation through the existing resume
  compatibility rules. Keys remain in the keychain under provider and service address.
  Shared autonomy and measurement rules are read again for each turn.
- Settings `max_concurrent_turns` defaults to three (1–8). Waiting turns are cancellable and
  visible. Claude turns queue separately, and the existing executable semaphore remains held
  until the child is reaped. Below 1 GiB available system memory, admission falls to one;
  existing turns finish. The window explains the reduced concurrency.
- Busy drawing reads use published metadata/render buffers. Picks retry asynchronously with
  their original workspace id. Other workspaces never wait on that drawing's session lock.
  Batches publish after their commit; their intermediate edits remain private. Save dialogs
  keep the drawing they opened for even if the visible tab changes.
- A webview subscribes before attaching to the workspace transcript and progress snapshot.
  Cursor checks remove duplicate or wrong-workspace events. Reload does not clear or cancel
  turns. Tabs show working, queued, waiting, done or failed; hidden completion retains a short
  summary when opened. Closing busy work requires explicit Stop and close.
- `qiyas_read events {after,session?}` exposes the existing bounded commit journal and changed
  handles. Failed batches add no commit; undo is a new commit. Expired, foreign, future or
  truncated history requires resync. The window has a separate 512-entry progress journal.
- Design, measured acceptance results and remaining limitations are in
  `docs/2026-10-25-qiyas-goal-W-design.md` and `docs/2026-10-25-qiyas-goal-W.md`.
  No new dependencies, paid calls or changes to the founder's running checkout.

## Goal A: Mizan's tools and judgement aids (25 October 2026)

- Discovery suggests command kinds as well as command names: `set_method` suggests
  `measurement.set_method`. `run` accepts command fields directly in `args` and wraps them in
  `command`, with `normalised_input`. All query kinds accept `within` or `bounds`; the latter
  produces a warning. Supplying both spatial filters is ambiguous and refused.
- Opening fit refusals include the nearest valid `at` for that width, when a fit exists. It is
  advice in drawing units. Nothing moves until the caller submits that placement.
- `expected_revision` accepts any subset of the returned fields, comparing only non-null
  supplied parts. `document_revision`, `geometry_revision`, `knowledge_revision`,
  `sheet_revision`, and `selection_revision` are separate; `session`, `sheet`, and the legacy
  working `context` remain available. Selection is reported by the window. Stale responses
  identify changed supplied parts, changed handles, completeness, and the current token.
- A plan unchanged across eight mutating calls receives advice listing unfinished phases;
  the panel marks it stale. Reposting the same plan does not reset the count. Pending, active,
  and skipped phases are listed in the final answer. Plans never prevent a valid edit.
- `spatial_query` reads exact `contains`, `nearest`, and `intersections` on chosen entities,
  with measured values and tolerances. It refuses sampled/unsupported geometry and open
  containment boundaries, rather than asserting a fact it cannot establish.
- Both prompts stay below 900 words. Phases follow the user's brief. Exact measurements and
  spatial/room/clearance checks precede pictures. Use the latest revision, including after
  your own edits. Check rooms, sizes, wall thicknesses, scale, survey point, directions, and
  all requested deliverables before finishing; state every unmet requirement plainly.
- Model capability `image_input` is true, false, or unknown. The state and opening context
  explain it; Settings displays a warning for false or unknown and permits an explicit
  choice when the provider has no metadata. Text-only tool-capable models remain selectable.
  A false capability withholds both new and historical images. Unknown capability never
  licenses a claim to have inspected a picture.
- Optional data: `read_object_pack {name:"interiors"}` returns the cited, editable JSON starter
  pack; `install_object_pack {name:"interiors"}` installs it in one undo step. Alternatively
  supply `data` with your own pack. It defines WC, basin and sink types, back/body/clearance
  recipes and optional `placement` checks. Dimensions and tolerances live in JSON, never
  Rust. Starter dimensions are assumptions, not product specifications or certification.
  `place_objects {objects:[{type:"WC",host:123,props:{distance:2}}]}` places one fixture on
  wall 123, two metres along it, without cutting the wall. Properties follow their declared
  units (length properties use metres); command geometry uses drawing units. `object_zones`
  reads exact regions. `records {collection:"knowledge_placement",detail:"full"}` reads the
  installed checks; edit their data with `set_knowledge {kind:"placement",name,data}`.
  Back contact, clearance occupation and door-swing intersections return
  ordinary findings with citations. Missing/unsupported evidence cannot report a complete
  clean check. Existing drawings are unchanged until the optional pack is installed.
- `set_site_origin {base_point:[0,0],site_point:[674250000,2734180000],x_axis:[1,0]}` is the
  one-step survey placement for a millimetre drawing: move/rotate model geometry, set the UCS
  and insertion base, and retain site metadata, all undoable together. Optional `targets`
  restrict the geometry; an empty list sets only the coordinate metadata. JSON commands use
  world/site coordinates; typed points use the UCS. `site_coordinates {targets:[123]}` reads
  exact defining points in site units, metres, and local UCS coordinates. Do not add the
  origin again to the reported site points.
- `qiyas_read brief_check` takes `expectations:[{id,text,verify:{...}}]` or items with
  `read:{op,args,pointer,expected,tolerance?,subtract?}`. `verify` uses the existing verifier
  and must be tied to the drawing. A read selects a measured field with a JSON pointer;
  `subtract` compares signed change from an earlier observed value. It never runs edits.
  Each item returns pass/fail, measured value and evidence. The caller supplies the user's
  actual words and thresholds; the engine has no room, villa or trade-specific brief rules.
  An `artifact` read checks only existence/size of an export beside the drawing, not contents.
  Unmet items are appended to the final answer. Later edits mark earlier checks as needing
  rechecking; the saved checklist survives a conversation restart conservatively.

## Goal U: clinic app fixes (25 October 2026)

- A drawing that was empty fits its first visible content once. Mizan's reveal frames the
  new handles on the visible sheet, including when reduced motion is enabled.
- Local evidence and nested worktrees belong under `.local/` (`.local/wt/` for worktrees).
  They must never reload the app. Vite ignores `**/.local/**` and `**/target/**`;
  `.taurignore` excludes the same folders. Apply watcher changes while the founder is idle.
- After a connection failure, the assistant panel retains completed work and offers
  **Continue**, using the existing conversation. It does not clear the conversation.
- The glib iterator alert remains tracked in `docs/glib-variant-iterator.md`; its offline
  source guard must be rerun when Tauri/GTK dependencies change.

- `new_drawing` reuses an empty untitled drawing with matching units, including a parked
  tab. Untitled desktop exports use Documents; saved drawings export beside their file.
  File-tool summaries include the export folder and export results include the file path.
- Client PDF and Excel pages omit internal diagnostics and entity handles. Full diagnostics
  remain in the lossless record (hidden sheets/columns in Excel, attached JSON in PDF).
  Citations are deduplicated; long cells wrap and quantities use consistent SI symbols.
- Room-name and area placement searches inside the room, avoids plotted ink, and tries
  wrapping then a quarter-turn without reducing text height. Drawing-check results include
  complete per-rule counts even when only a short sample of findings is returned.
- New drawing annotations default to English unless the host sets the sheet language (W3: Mizan's
  agent sets it from the user's message). `label_spaces` may match existing Arabic room
  names from a supplied drawing; explicit `language` wins. Arabic/Persian digits and the
  Arabic decimal separator are accepted by area checks and numeric field readers.
- Block symbols are not room islands by default. Explicit geometry and object types with
  `room_void:true` can define voids; room results retain the list of deducted islands.
- `set_measurement_scope` marks a closed region or named group with `measured:false` (view
  only); `measured:true` removes that marker. Manual take-off and generated quantities
  respect the marker, which persists in the drawing and follows undo/redo. Possible enlarged
  duplicate details are advice until Mizan or the engineer explicitly marks them.
- The BOQ comparison command is `discrepancy_check`; `check` remains its legacy command
  alias. Drawing checks are the read operation `check` (native `check_drawing`). Matching
  named layer, layout, object-type, block-definition and group creates return an existing-item
  note without adding an undo step; conflicting definitions require an explicit change.
- Edit commands accept `sheet` by display name or block name without switching the visible
  sheet. This JSON routing field adds no positional typed-command prompt. Wrong-sheet
  errors name the owner and how to target it. Edit results check the
  edited sheet and reveal only entities on the visible sheet. Batches remain all or nothing.
- Radius and diameter text follows the picked point, including outside the circle, with a
  leader ending clear of the text. Regression geometry uses mm, cm, m and inches.
- Fast checks and the reviewer's combined end-to-end checklist are recorded in
  `docs/2026-10-25-qiyas-goal-U.md`. Goal U does not run separate clinic/MCP replays under
  the founder's revised acceptance scope.

## Goal C: AI context, evidence and cache reuse (25 October 2026)

- Provider history only appends between token-watermark trims. Duplicate reads, old workflow
  text, pictures and schema discovery never move or disappear during an ordinary request.
  Cost and workflow guidance enter new messages. Fixed result placeholders keep the call id,
  operation, important handles and ids, errors and an artifact pointer. A storage failure
  keeps the original evidence instead of discarding it. After an explicit provider context
  refusal, one logged recovery trim may retire even recent results into artifacts; it still
  preserves user words, call/result pairs and signatures. The unmodified native Claude CLI
  owns its private inner history; Qiyas controls its MCP evidence and outer conversation.
- Model-facing results pass through `surface::format_result` after execution and before
  Agent/MCP serialization. Core execution and batch bindings retain full results. Execute
  replies report status, revision, handle counts with samples, grouped check findings and
  advice once per rule. UI events keep full handle lists. Reads accept `detail: summary|full`
  and `fields`; record reads retain their existing row pagination and field semantics.
  Explicit full record reads use field projection to recover arrays beyond the native
  display cap, preserving complete geometry in their artifacts.
- Full evidence is immutable JSON in the Qiyas data home's `artifacts` directory, named by
  SHA-256 of canonical JSON. `qiyas_read artifact {id, offset}` returns at most 12,000 UTF-8
  bytes with `next_offset`, `artifact_revision`, historical `revision` and `current_revision`.
  An old artifact never acquires permission to edit a newer drawing. Exact quantities are
  preserved before display rounding; legacy private tool display behavior remains unchanged.
  Artifacts currently persist; there is no automatic expiry that could break a conversation.
- Discovery contracts carry a content version and full-evidence pointer. A repeat supplies a
  reference in the new reply. Command families link qualified names such as `draw.line`;
  command lists and large take-off kits page with `offset`. The registry index remains in
  the stable system prompt. Compaction retains contract references, never overlapping full
  schema lists.
- Anthropic marks tools, system and a stable history prefix for one hour, with automatic
  five-minute caching on the newest eligible block (four breakpoints maximum). Signed
  thinking is never marked or changed. OpenAI-compatible and ChatGPT-plan requests carry
  the saved conversation id as `prompt_cache_key`. DeepSeek reasoning text is replayed
  exactly. JSON keys and JSON-encoded argument objects are ordered deterministically.
  Changes to model/effort/limits or rejected optional features log expected rebuilds.
- Support logs record total input, cache reads, cache writes, output and reasoning for each
  successful request, plus the turn hit percentage. Hits below 70% include the known reason
  (cold start, trim, setting change or possible TTL expiry); unknown provider residency is
  stated as unknown. Cumulative usage events retain the turn totals after cancellation or
  a later failed request. Discarding an unfinished answer on retry preserves those totals.
  Settings' Technical details switch reveals one cache row per turn.
- Image capability comes from model metadata; known built-in Claude models accept images.
  Unknown or explicitly unsupported models receive no image bytes and get plain guidance
  to use exact queries/checks. The model picker explains this in English/Arabic. No billable
  capability probe runs. Repeated captures compare PNG hashes and return `unchanged`.
- The ignored `cache_replay::clinic_policy_replay` test consumes local copied clinic fixtures
  through the production formatter and history policy. Its character-weighted estimate is
  calibrated to logged input tokens. Missing retired bodies and provider residency are
  explicit limitations; this is not a live cache-hit measurement. By the founder's revised
  scope, end-to-end runs and acceptance gates are left to the reviewer's combined run.
  See the Goal C report for fast-test results and the combined-run checklist.

## Goal P: quantity evaluation and room reuse (25 October 2026)

- `objects_for(ids)` and `object_quantity(id)` select drivers before evaluating quantities.
  Fields, tags, explicit schedule IDs, object record selectors and explanations use these
  narrow reads. Joined bands retain the existing ordered prefix allocation and exact arithmetic.
- Evaluation caches definitions, parsed expressions, object contexts, band geometry and records.
  Its generation compares retained entity snapshots plus raw knowledge, units and document
  tables; it does not rely on `document_revision` advancing inside a transaction. Rollback,
  undo, redo and external-reference reload discard cached evaluations. Identical metadata,
  layer, visibility and hosted-frame writes retain unchanged snapshots and recorder first-touch
  order. A temporary binary encoding compares exact type tags, lengths and float bits without
  repeatedly escaping knowledge strings. This changes no drawing or API format.
- Generation-bound indexes cover assembly members, host openings, generated parts, fields,
  schedules and source-to-region candidates. Derivation uses both previous and current links
  so removals still update reverse dependencies. Adding an unused type definition does not
  rederive existing objects; changing a used definition still does.
- Room queries reuse the source graph, uncut wall footprint and exact-tolerance arrangements.
  Opening changes preserve that footprint. Topology changes reuse a seeded face only when its
  ordered source support and effective tolerance are unchanged. Undo, redo and rollback clear
  this cache. Boundary policy and island meaning remain unchanged.
- Edit checks select old/current neighbours and reverse dependencies before expensive rules.
  Primitive geometry and text ink can be reused only while their source snapshots and plotting
  inputs match. Caller-visible finding filtering remains unchanged; phase and final checks stay
  full. Old text positions use rendered glyph bounds to find former neighbours. Plain room
  names without retained regions follow connected enclosure candidates after source edits.
- Mixed opening edits remain sequential: grouping them changed intermediate results and
  allocated handles in the MCP differential test. Only the existing Goal K all-addition batch
  optimization is retained. Broader P5/P6 deferral remains unfinished.
- Dev builds optimize `qiyas-core` and `qiyas-param` at level 3, retaining debug assertions and
  normal floating-point semantics. The offline clinic fixture preserves all 85 supplied calls
  and applies the name-only `POMI (RICS)` method first (a fixture name, never shown). Its release guard uses the existing CPU
  reference. Exact comparison retains quantities, handles, findings and revision counters.
- Validation and measured gates are recorded in `docs/2026-10-25-qiyas-goal-P.md`.
  Absolute clinic gates pass with exact results; the stricter relative clinic guard still
  fails. Local-port AI tests are blocked by sandbox permissions. No new dependencies, model
  calls or licence changes.

## W1: Mizan's behaviour and cost (26 October 2026)

Evidence, the founder's live runs and the offline replay: `docs/2026-10-26-qiyas-W1.md`.

- `qiyas_execute ask {questions: [{id?, question, options: [{label, description?, recommended?}]}]}`
  (surface.rs `read_questions`): 1 to 6 questions in ONE call, 2 to 4 options each, at most one
  recommended, free text always allowed (`allow_other`). The agent emits `AgentEvent::Question
  {questions}` and ends the turn (no `Done`; the desktop shows status "waiting"); the answer is the
  next user message. The panel shows a numbered list with lettered options (`kind: "question"` in
  `ai/messages.ts`); the new shell renders the real design. MCP clients get the echoed questions.
- Assume, do not ask: the prompts say so; `[state]` carries `Country/practice` (Settings `country`,
  else inferred from the selected method's name: Egypt, KSA, UAE; else "ask once").
- Finish gate (`plan_watch.rs` `finish_gate`, `agent.rs` `end_gate`, `mcp.rs`): the `finish` op, or a
  plain end of turn, is refused while plan phases are pending or active, unless the user said stop
  (`judgement::wants_to_stop`), the answer ends with a question, a metered run says `[finish now]`,
  or it was refused twice for the same plan. The refusal names the phases; the window drops the
  early answer (`Retrying` with `ai.error.finish_gate`). `skipped` phases count as decided.
- "Continue", "complete everything", "apply your assumptions" (`judgement::wants_to_continue`) add a
  `[continue]` note listing the open phases and keep the job. The cost note (`[efficiency note]`)
  and `tool_cost` advice are efficiency advice, each given once per turn.
- Reply language: the latest message decides; an answer clearly in the other language
  (`judgement::language_mismatch`) is sent back once (`ai.error.language_retry`).
- Saving (`agent.rs` `keep_work`): after a finished phase that changed a titled drawing the agent
  saves through the host; an untitled one is not touched mid-work, and at the turn's end the host
  opens the window's Save As once and the answer says so. Nothing is saved when Mizan changed nothing.
- Drawing on assumptions the engineer allowed: Mizan puts them on the sheet as general
  notes (title block status "For review") and lists them in the answer. Qiyas draws; loads, design
  calculations and approval are the engineer's and the company's (company policy).
- Exports (`surface.rs` `quantity_gate`): `export_takeoff` and `deliverable.export` of boq,
  dimensions or variation by Mizan or MCP are refused (`quantity_check_failed`) while `takeoff_check`
  reports measurement errors or unmatched schedule rows, unless the file name contains DRAFT; then
  the answer carries `draft_export` advice and the file name marks it.
- Fewer wasted calls (`result.rs`): a long list in a shortened result comes as `rows` (plain fields,
  nested names in `+`, 40 rows, `more`), not a bare count; `artifact {id, pointer}` reads one part of
  the saved evidence; `commands` of a command with kinds lists each kind's required fields and an
  example (`kinds_contract`); a page of records shortens one oversized record (`shortened`) and
  points tables at `query tables`; `records` with a field no record has returns the page with a
  warning (a `where` on one is still an error); `fields` on other reads names `missing_fields`;
  `commands` takes up to 40 names.
- Cache: the Responses body of a long turn only appends between trims (`chatgpt_plan/responses.rs`
  test `a_long_turn_only_appends_between_trims...`); the live miss rate is provider side (see the
  report). Both fixed prompts stay under 900 words.

## W4: the new window shell (27 October 2026)

Design B, `.local/design/2026-10-26-shell/b-column.html`. What changed per note, the shortcut audit and
what was not seen in the real window: `docs/2026-10-27-qiyas-W4.md`. Chromium renders of the real
React app with a faked IPC are in `.local/w4/` (`stub.js`, `shoot.mjs`).

- One column on the end side (`SidePanel` in `app/Panels.tsx`, `ai/AssistantPanel.tsx`). `shell.sideTab` is
  `"assistant"` (the conversation) or an output: `takeoff`, `boq`, `conflicts`, `review`
  (`shell/outputs.ts`). An output shows in the same column in place of the chat (`ArtifactFrame`: back
  arrow, a switch menu, its meta, one action, close) with a strip under it back to the chat
  (`.back-strip`) and the box still below ("Ask Mizan about this BOQ"). Its head carries the
  output's buttons (Export take-off; Import BOQ; Compare and Export conflicts; Refresh), so the
  panels under it have no heading or buttons of their own. The Artifacts icon in Mizan's header,
  after History (`OutputsMenu`, its count as a small badge; Arabic says المخرجات), opens one; `setSideTab("boq")` etc. from the take-off hooks still work. The
  Projects, Comments tabs and `SideTabs` are removed; the conversation and all outputs stay mounted.
- The box (`.composer`): attach, mark an area and the model pill (`WorkspacePicker`, a popover: the model's name, or the
  provider's, with a spark; provider, model, effort of this drawing) under the text, send at the end. Typing "/" lists commands
  (`ai/slashCommands.ts`: takeoff, boq, compare, sheet, check, connect, new; English words in the
  English window, `حصر مقايسة مقارنة لوحة فحص ربط جديد` in the Arabic one, either language's word runs
  when typed whole). A prompt command sends its prompt (`ai.slash.<id>.prompt`); `compare` asks for
  a BOQ then compares (`useTakeoff.importAndCompare`), `connect` opens "Connect an AI app", `new`
  starts a chat.
- Questions (`ai/QuestionList.tsx`): a plain numbered list in Mizan's message, no boxes, the
  recommended option in the accent with "· recommended"; clicking an option puts it in the box, "Other"
  is the box. Runs of identical finished reads are one line with a count (`messages.ts readRuns`).
- File tabs: a dot before the name while Mizan works (animated, still under reduced motion, no
  words), a dot after it while unsaved. Ribbon: double-click a tab, the arrow at the end of the row or
  Ctrl+F1 collapses it (`qiyas.ribbon.collapsed`); a tab clicked while collapsed shows its panel over
  the drawing until Esc, a click elsewhere or a tool. Layout tabs (`SheetTabs`): no scrollbar; the
  edge with more fades and has an arrow. Layer names wrap and are their own tooltip.
- Settings is in sections (`SettingsDialog`, a vertical Base UI Tabs: Mizan, Measurement, General,
  Connected apps, Support; every panel `keepMounted`, so Save reads them all).
- "Connect an AI app" (`shell/ConnectAiDialog.tsx`; plug in the top bar, `/connect`, the empty chat):
  one click turns the MCP server on through the settings (`set_settings` restarts it) and shows the
  Claude Code command and Claude Desktop config. Security as before: 127.0.0.1, the token (made if
  missing), no other web origin, off by default.
- Save (`useDrawings.askSavePath`, `app/saveName.ts`): a toast "Choose where to save the drawing…"
  before the native dialog; an untitled drawing is offered a real name (title block project or
  title, else the first text that reads as a name, else "Qiyas drawing <date>") in the last used
  folder or Documents; "Saved <name>" after. The `.qiyas` is `<name>.qiyas` beside the drawing (it
  was always written; `crates/qiyas-core/tests/blank.rs` now proves it comes back).
- Shortcuts (`tools/shortcuts.ts`, `app/useCommandLine.ts`): Ctrl+Shift+S, Ctrl+O, Ctrl+P, Ctrl+F
  (find; AutoCAD's is the object snap, F3 here), Ctrl+A, Ctrl+X, "/" and Ctrl+K (Mizan's box),
  Ctrl+/ (cheat sheet, `shell/ShortcutsDialog.tsx`, `shortcutList.ts`, also SHORTCUTS), Enter and a tap
  of Space repeat the last tool, a second Esc leaves the tool. File keys work while typing in Mizan's
  box. Undo, redo, erase, cut, select all and F3/F7/F8/F10 say what they did (toasts).
- Numbers: no confidence percentage or band anywhere in the window, and no professional body in
  method names or Mizan's words (`shell/methodName.ts`: `plainMethod`, `withoutBody`).

## W2: speed (27 October 2026)

Measurements, the ledger of every attempt and what is still slow: `docs/2026-10-27-qiyas-W2.md`.
Tools: `bench_gen`, `bench_open`, `bench_edit` (`crates/qiyas-core/examples/`, a 0.5 M and a 2.5 M segment
drawing: `bench_gen out.dwg 400|2000`), and `scripts/perf/` (the real React app in headless Chromium with the
W4 stub: `perf.mjs`, `inter.mjs`, `move.mjs`, `renders.mjs`, `split.mjs`, `batch.mjs`; `launch.py` and
`proxy.mjs` run the real `qiyas-desktop` on a Broadway display, no window on any screen). No `perf` or
`heaptrack` on the founder's laptop: timers, `/proc` and Chromium's CPU profiler. Check the heat guard's state file
before a heavy step and never take a timing during a pause.

- `pnpm tauri:fast` runs the debug Rust app on the production bundle (`tauri.fast.conf.json`, `vite preview`
  on port 1420): the window is 1.5 times faster to start and 2.4 to 3.7 times faster to use than under
  `pnpm tauri dev`, whose React is the development build with `StrictMode`. No hot reload; the normal
  command is unchanged.
- The click index (`PickIndex`, 85% of a batch's cost) is built by the first `pick`, `snap` or window pick of a
  batch (`Cached::index`), not by `batch()`. Do not call `index()` from the draw path;
  `tests/pick_index_lazy.rs` fails if a batch builds it.
- The pointer is not React state (`src/tools/pointer.ts`): `useTool.onMove` writes a `PointerStore`; the
  status bar follows it (`usePointer`); the app renders on a move only while a tool needs the cursor
  (`needsPointer` in `useTool`: a snap, points in progress or a tool state). A bare move used to render the
  whole window. Do not put the cursor, a hover or any per-move value in a hook the app's root calls. The
  ribbon takes its handlers through `useStableCallbacks` and is `memo`; `Markdown` is `memo`.
  `tools/pointerMove.test.tsx` counts renders.
- The layers panel renders only the rows in view (`@tanstack/react-virtual`; headless-tree's `scrollToItem`
  mounts the row a key moves to). `data-layer-state` (Mizan's `panel_state`) still lists every row and is
  rebuilt when the rows, the selection, the open groups or `hidden` change. `LayersPanel.test.tsx` has a
  1,500 layer test.
- An animation that loops forever may move only `transform`, `opacity` or a stroke offset
  (`shell/animations.test.ts`): a `box-shadow` ring on one 7 px dot cost 6% of a core. Mizan's mark in the
  panel heading rests (`MizanMark still`) unless Mizan is working.
- `public/theme-init.js` (a blocking script in `index.html`) puts `data-theme` on `<html>` before the first
  paint, so the window's first frame is the theme's own background (`html` in `index.html`). It must choose
  what `initialTheme` (`app/shared.ts`) chooses: `app/themeInit.test.ts`.


## W3: output quality (27 October 2026)

What a client gets from Qiyas and Mizan: the notes, the root cause of each and the evidence are in
`docs/2026-10-27-qiyas-W3.md`. Tests: `crates/qiyas-core/tests/w3_output.rs` (every unit),
`crates/qiyas-boq/tests/w3_workbook.rs`, and the offline replay `crates/qiyas-ai/tests/w3.rs` (the
villa request in Arabic through the scripted provider).

- Numbers a client reads (`numbers.rs`): an area or length in text, a label, a schedule cell or a BOQ
  row is shown to two decimals with its unit (`17.81 m²`, `١٧٫٨١ م²`, `14.00 m`); the take-off and the
  lossless records keep full precision. A field's unit is left out only when the text after it gives
  one or the table heading does (`Area (m²)`). `QiyasObject` fields default to 2 decimals for area and
  length names, 3 otherwise; an object label template does the same (`label_number`).
- No internal identifier on a sheet. Drawing check rule `internal_id_in_text` (general, a layout rule)
  finds `takeoff:`, `boq-row:`, handle references (`handle 12`, `0x1A`), unique ids and `QIYAS_` names in
  plotted text and table cells. The root cause was the advice that told Mizan to cite `takeoff:<row id>`
  in notes: a note is now cited by its value (a stated value equal to a measured row's), and a writer
  through the AI or MCP door (text, mtext, edit_text, cells, notes) gets `internal_id_in_text` advice.
- Opening tags: with an opening schedule, doors and windows no tag shows are `opening_untagged`
  (construction, a layout rule). `object_tags {category:"opening", property:"mark"}` tags all of them in
  one step (marks are given out first when no schedule has yet). Text that sits inside a room but is not a
  room label (a tag on a wall) goes to free space when it cannot stay inside.
- Totals add like with like. The opening schedule has `total_by` (kind): one total row per kind. The
  report layouts total only rows of one item in one unit; the shipped BOQ and variation layouts total
  nothing. `ObjectSchedule.language: "ar"` (`ar_headers`, `ar_templates`, `ar_totals`) gives Arabic
  headers, kinds and digits; `opening_schedule` and `room_schedule` take `language`.
- BOQ: lines split by size or mark for doors and windows when the mapping does not say it
  (`{mark}`, `{width}`, `{size}`... in its text): code `A-D01-D-900x2100`, description `… — 900 × 2100 mm`.
  Other objects keep the mapping's code (a client BOQ is matched by it). One code with several descriptions
  is numbered (`-2`), with a warning, not refused. `Mapping.informational` (also read from "gross floor
  area", "GFA", "إجمالي المسطح"…) keeps a line out of the BOQ and the comparison; the dimension sheets keep it.
  Section and trade titles are `TableRow.section`: a band in Excel, spaced in the PDF. In an Arabic
  BOQ an English-only method or source column is left out (and said in the report); the dimension sheets
  keep it. No visible sheet of a client workbook holds JSON (`Qiyas record` is hidden).
- Measurement: a deduction beyond an object's gross quantity limits it to nothing, adds a line saying so
  and a report warning (what to look at; deduct from `geometry.gross_volume`, not the net figure); it no
  longer refuses the whole method. A negative operand names the term, the expression and the object.
- One export folder (`Session::export_folder`): the plot, the take-off workbook and every deliverable go
  to the drawing's folder, or Documents for an untitled drawing; results name the whole path.
- `make_sheet` (`sheet_setup.rs`, `Command::MakeSheet`, `Outcome::Sheet`): a sized layout (smallest of
  A3 to A0, or ANSI B to E in inches and feet, that holds the model at 1:100 or larger; orientation by
  the better scale), a viewport at the largest standard scale that fits (`STANDARD_SCALES`), a captioned
  title block (project, title, drawing number, scale, date, drawn by Qiyas, revision P01, status For
  review, sheet) in the sheet's language, general notes at 3 mm and a scale bar. Titles are 4 mm; nothing
  is under 2.5 mm. Fields nobody gave stay empty and are listed. `plot` with no sheet prints the
  layout. The layout's own viewport (number 1) no longer makes a sheet "incomplete" for `free_space`.
  A schedule added to a layout without `at` goes inside the frame, clear of the viewports and the title block
  (`place_new_table`); `free_space` treats a viewport's frame as occupied.
- Language: the agent sets `Session::set_sheet_language` from the user's latest message each turn;
  `label_spaces`, schedules, `make_sheet` and `deliverable` take it when `language` is omitted.
- One retained room per enclosure (`room_with_ring`); `A-ROOM-BOUNDARY` is made non-plotting even when
  the layer existed; retained boundaries are not line work for the duplicate and overlap checks.
- The quantity check no longer reads a sheet number (`A-101`) as a reference to element 101, nor a
  schedule Qiyas drew as a table nothing explains.


## W6: take-off checks (27 October 2026)

The scope is `docs/2026-10-26-qiyas-test-notes-plan.md`, W6. These checks support the engineer's
review of a take-off; every evidence page carries For review / للمراجعة.

- Blind second reading: `crates/qiyas-core/src/second_reading.rs` stores the readings, compares
  them within the tolerance for the unit, and raises a drawing query when they differ. An
  unresolved disagreement stays out of the generated BOQ. `crates/qiyas-ai/src/blind_reader.rs`
  runs a fresh reader with no conversation history and only drawing reads; it gets the item's
  description, unit and rule, never the first quantity. The BOQ says agrees, differs or not
  checked. Scripted-provider coverage is in `crates/qiyas-ai/tests/w3.rs`; core coverage is in
  `crates/qiyas-core/tests/w3_output.rs`.
- Drawing-pack card: `crates/qiyas-core/src/pack_check.rs`, read by `measurement.pack_check`,
  checks known units, annotation scale, vector/scanned PDF pages, scan calibration, page
  orientation, and sheet number/revision from the register rules. Each line passes or names
  the fix; calibration is recorded by the existing calibration command.
- Evidence PDF: `crates/qiyas-plot/src/evidence.rs` uses the existing plot snapshots, page maps
  and embedded text fonts. Each source sheet has heavy coloured outlines and BOQ item codes
  beside its elements, plus a legend of code, description, quantity and unit in the BOQ's
  language. Long legends continue on pages that repeat the drawing and review notice. Missing
  source geometry refuses the export instead of silently omitting an item. The drawing is
  unchanged. `qiyas_execute` op `export_evidence {path?, language?: "en"|"ar"}` in
  `crates/qiyas-ai/src/surface.rs` uses the quantity gate; desktop IPC `export_evidence` lives in
  `crates/qiyas-desktop/src/plot.rs` and writes off the session lock. It defaults to
  `<drawing> evidence.pdf` in `Session::export_folder`, beside the BOQ; pass the BOQ's language
  if it had an explicit override. Tests added to `crates/qiyas-plot/tests/plot.rs` cover the W3
  sample villa's items, both languages, continuation-page notices and the export folder;
  `crates/qiyas-ai/tests/surface.rs` covers the agent export.

## W5: drafting and design knowledge (27 October 2026)

W5 adds built-in drafting knowledge and villa planning checks from
`data/drafting-standards.json` and `data/code-register.json`. `drafting_knowledge`
and `space_check` are read-only inquiry commands returning measurements; source
citations and `practice_default` labels travel with limits, and missing evidence
is reported separately. `symbol` draws single/double doors with leaves and swings,
straight stairs with floor-specific UP/cut or DN/void/handrails, elevation levels,
ground/foundation lines and labelled material hatches through the existing drawing,
block, text and leader APIs in one undo step. Coordinates use drawing units and
`_m` fields stay in metres. Mizan accesses them through the ordinary registry tools;
draw at once, one floor and one part at a time; read only the knowledge the current part needs and check each floor once its walls exist (changed 28 October 2026 at the founder's request).
Outputs remain for review, with design and approval assigned to the engineer.
`tests/w5_drafting.rs` is included in `tests/all.rs`; the registry-index ceiling is
5,000 bytes. This handoff has not yet been compiled or tested.

## Builds and tests (27 October 2026)

- qiyas-core, qiyas-ai and qiyas-boq each build their integration tests as ONE binary, `tests/all.rs`
  (`autotests = false`). A new test file needs its `#[path = "x.rs"] mod x;` line there; the test
  `every_test_file_is_listed` fails otherwise. `villa_openings` and `clinic_performance` stay their
  own binaries because `scripts/check.sh` names them.
- Debug info is line tables for our crates and none for dependencies (`[profile.dev]`).
- Child-process tests run the same binary again: name the test by its module path (`test_path`).
