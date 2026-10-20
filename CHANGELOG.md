# Changelog

All notable changes to Qiyas are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). A release's notes on GitHub are its
section below, so add the section before pushing the tag.

## [Unreleased]

### Added

- Check a command or a whole batch before running it, with all input problems reported
  together. Unknown fields are shown as warnings, and results say which defaults were used.
- Outside agents can safely retry an edit with a request id without drawing it twice.
- Drawing examples are checked automatically in millimetres, centimetres, metres and inches.
- Offline drafting evaluation through Mizan's three generic tools: a villa and a flange
  check geometry, layers, quantities and captures, with regressions for earlier drafting
  faults. The scripted evaluation does not assess live model judgement.
- Verify drawing intent with pass/fail results, measured values and tolerances: sizes,
  areas, geometric relationships, symmetry, bounds, intersections and counts.
- Draw centre marks, centre lines, symmetry axes and labelled reference grids in one undo
  step, sized for the drawing's units and annotation scale.
- Select general drafting or construction check packs. Construction checks turn on
  automatically for drawings with take-off rows or room, space or area layers; general
  checks now flag circles missing a centre mark. Mizan checks stated intent before finishing.
- Core commands now supply their command-line words and aliases to suggestions, ribbon
  customisation, scripts and Mizan. Typed commands share schema-driven prompts, relative
  points, UCS coordinates and Arabic number input while existing window tools keep their
  command-line behaviour.
- Draw joined walls with clean corners and junctions, measurable wall hatches, room holes,
  and reusable door and window symbols. Wall quantities report length, gross and net plan
  area, and volume with opening heights, in metres across drawing units.
- Create and rename paper layouts, set a viewport's scale and model centre, and add a sheet
  frame and title block containing only the information supplied. Text and dimensions on
  layouts use paper millimetres.
- Define reusable blocks directly from drawing objects through the command registry.
- Mizan's drawing changes appear live, entity by entity, with consecutive steps queued.
  Reduced motion shows them at once; a command, undo or drawing switch ends the animation.
- Mizan can now use every command in Qiyas, discover the properties of drawing objects, and
  make related changes together as one undo step. Outside AI apps have the same access.
- Check a drawing for gaps, overlaps, stray lines, unreadable or colliding text, room sizes
  that disagree with their labels, and take-off rows whose drawing links are gone.
- Scripts can find rooms, label their unapproved candidate areas in English or Arabic, and
  add take-off clouds and notes, using the same commands as Mizan.
- Text and tables can choose their own readable sizes for the drawing's units and annotation
  scale when no size is given.
- Choose the AI model from a list: the provider's own models (Anthropic, or an OpenAI-compatible
  service's), with a filter for long lists and "Other…" for an id the list lacks.
- Isolating a layer can be undone: isolate it again, LAYUNISO or Un-isolate in the Layers panel
  puts back only what isolating hid; the panel says which layer is isolated.
- New drawing (New on the quick-access bar, Ctrl+N or NEW): an empty drawing in metres,
  centimetres or millimetres, saved with Save As the first time.
- Mizan can list, make, open, switch, save and close drawings, in the same conversation. Opening a
  file you have not opened or named before, Save As and closing unsaved work stay your choice in
  Qiyas. The MCP server has the same tools.
- New chat button at the top of Mizan's panel.
- Mizan can move, copy, rotate, scale, mirror, array, offset, trim, extend, fillet, chamfer and
  erase what is drawn, and undo its own last step.
- The origin icon: X and Y arrows on 0,0 (or the UCS origin), parked in the corner when 0,0 is off
  screen; UCSICON ON / OFF or View, Origin icon.
- Point Mizan at something: what you select shows as a chip above Mizan's box and goes with your
  message, with a picture of it; the box's area button marks an area (drag a box, or click its
  corners); right-click, Ask Mizan about this.
- Paste, drop or attach pictures (a screenshot, a site photo, a scanned schedule) in Mizan's box.
- Mizan sees the drawing as you do: its pictures show images and PDF underlays, model space in a
  layout's viewports and any sheet, with numbered marks it can act on; it can look at your screen
  as it is and select entities for you.

### Fixed

- A wall path that returns to its start now joins the first corner correctly. Boundaries share
  the same closure rule; a plain open polyline keeps its return vertex.
- Windows use a 0.9 m sill and a head at 2.1 m or the wall height when those sizes are omitted.
  Contradictory opening heights are still refused.
- Command descriptions and input validation work without an open drawing, and wall descriptions
  now include their complete inputs.
- Switching AI provider restores that provider's model and address. A missing model offers a
  clear Settings instruction, and model lists show a warning when the saved model is absent.
- Every file picker starts in the drawing's folder, the last folder used, or Documents.
  The first save of an untitled drawing no longer starts in the app's working folder.
- The agent log records the effort and output limit actually sent on each request, and reads
  reasoning-token counts from both supported OpenAI-compatible usage formats.
- Grid bubbles at neighbouring corners no longer collide, and title blocks share the sheet
  frame without drawing its edges twice. Text collision checks respect text alignment.
- Drawing checks distinguish real gaps and overlaps from nearby parallel lines and curve
  approximations. Using the default layer is an informational note.
- Layers and layouts made by Mizan now appear in their panels as soon as the change arrives.
- Text typed with a literal `\n` now breaks onto a new line. A multiline Text becomes MText
  automatically.
- AI and MCP measurements share the same checks for empty items and counts of block references.
- The effort chosen for an OpenAI-compatible service (Gemini through Runware, OpenRouter, a local
  server) was never sent; it is now, with the answer's length, and the log shows thinking tokens.
- Undoing a layer rename after showing every layer hid the old set of layers again, and the
  drawing seemed to vanish; undo now takes back only what the rename changed.
- Circles and arcs drew as polygons in metre and centimetre drawings: curves are now drawn within
  0.05 mm in every unit, with 64 segments a circle at least, and again finer when zoomed in close.
- New drawings get readable dimensions and text (a metric template at 1:100), a metre grid and
  AutoCAD's multiline width, whatever their units.
- A block inserted from a drawing in other units comes in at its real size.
- AUTOCONSTRAIN no longer welds ends a few centimetres apart in metre drawings.
- Mizan's hatches use the right pattern scale for the drawing's units.
- A hatch made by clicking inside a square round a circle found slivers as islands.
- Mizan no longer makes up explanations of how Qiyas works, and its formulas read as plain text.
- Dimensions in metre drawings without a dimension style of their own were metres tall, with
  extension lines running off the sheet; such drawings are sized when they open.
- Mizan's picture of the drawing now shows its text and solid fills, so it no longer writes labels
  over labels it could not see; it can change a text in place.
- Table rows grow to hold text that wraps, instead of running over the next row.
- A new drawing's autosave is offered back after a crash, even before its first save.
- JOIN, trim and duplicate points work at large survey coordinates.

## [0.0.1] - 2026-10-12

First public release.

### Added

- Opens DWG and DXF drawings (AutoCAD 2000 to 2018) with xrefs, images and PDF underlays, and
  saves them back with a `.bak`.
- Take-off: lengths, areas and counts; a BOQ imported from Excel, compared with the take-off, and
  exported to Excel.
- Drawing and editing: lines, polylines, arcs, hatches, text, dimensions, tables, blocks, layers
  and groups, with undo for everything; parametric constraints and a parameters manager.
- Plot a sheet to PDF or a printer with CTB/STB plot styles, or every layout at once.
- Command scripts in the window or headless with `qiyas-cli` (convert, run, take-off, batch).
- Audit, purge, options, a customisable ribbon and command aliases.
- Autosave, lock files and crash recovery; automatic updates from GitHub releases.
- Mizan, the built-in AI assistant (Claude or any OpenAI-compatible service), and an MCP server
  for Claude Desktop and Claude Code.
- English and Arabic.
- The layers and the take-off panels fold away behind a thin rail on their inner edge, to give the
  drawing the whole width; the choice is remembered.
- Small dot-grid loading indicators (our own design) replace the spinner where the window waits:
  opening, comparing with the BOQ, drawing a PDF page, plotting, scripts and updates. Mizan works
  under the Qiyas mark itself, its mesh drawing itself line by line.
- Mizan's answers render as real Markdown: headings, rules, nested lists, tables (scrolling in
  their own box), quotes and code; links show where they point and never navigate.
- The ribbon is an icon ribbon, measure first: Measure, Draw and Annotate on the tab row, the rest
  under More. Every button is an icon with a tooltip (its name, what it does, its command line
  alias); a group shows its main buttons and puts the others behind a "⋯" button as the window
  narrows, so the panel does not scroll. The header's buttons share one size scale.
- The Properties palette under Layers works like AutoCAD's: General (type, layer, colour, linetype,
  lineweight, handle) and Geometry with every value of what you picked, in metres, square metres
  and degrees to the drawing's precision. A line shows its start, end, delta, length and angle; a
  polyline its vertices, closed flag, length or perimeter, area and width; an arc its centre,
  radius, angles, total angle, length and chord; a circle its centre, radius, diameter,
  circumference and area; text its insertion, height, rotation and contents; a block reference its
  name, insertion, scale, rotation and attributes; a hatch its pattern and area; a dimension its
  measured value and text. Click a value to copy it. Sections fold, and stay folded. Pick several
  entities (Shift, a box, a group) to see how many, of which kinds, the total length and area and
  the layer, colour and linetype they share.
- Dimensions show the drawing's unit after the number ("12.00 m", cm, mm, inches, feet), the
  AutoCAD way: a suffix (DIMPOST) on each dimension Qiyas makes, so the DWG stays standard.
  Dimensions already in a drawing and styles that have a suffix of their own are left alone.
  "Show units" in the Dimension style panel turns it off for a style (on by default) and rebuilds
  that style's dimensions; undo brings it back.
- Mizan asks before it guesses: when a request is unclear or leaves out something it needs (which
  entities or layer, which BOQ item, units, scale, tolerance, where to put something, overwrite
  or add) it asks one short question with the options it sees, in your language, and waits for the
  answer. When the request is clear it just does it.
- Mizan measures what you drew, not a rough copy: for a length, area or perimeter it finds the
  entity and measures it by handle, which is exact for arcs and polylines with bulges (a curved
  wall is measured along the curve). Points it clicks itself only follow the shape roughly, so it
  uses them only between points you give, and a measurement made that way now carries a hint to
  redo it on the entity. Notes and analysis no longer land in the take-off as empty rows.
- Mizan writes into the drawing: text (`add_text`), the net area of every room under its name
  (`label_spaces`, from the enclosed spaces net of columns and islands; `room_areas` for the
  numbers alone), schedules as real tables (`add_table`, `edit_table_cell`), and shapes and block
  symbols (`draw`, `insert_block`) to build a legend. Each call is one undo step.
- Installers for Linux (AppImage, deb, rpm), Windows (setup.exe, MSI) and macOS (Apple silicon
  and Intel), each with a signature for the updater.
- A quieter, thinner interface on headless libraries (Base UI, TanStack Table and Virtual, Headless
  Tree, cmdk, Sonner; all MIT, styled in our own CSS):
  - The take-off, BOQ and conflicts tables are one data grid: columns sort and resize (and keep
    their widths), the header stays on top, long lists scroll without lag, numbers line up at the
    end in tabular figures, the description takes the room that is left and shows its full text in
    a tooltip, and rows are reached with the arrow keys. A conflict row shows itself on the drawing
    when clicked.
  - The layers panel is a tree with keyboard movement; a long list is grouped by A-, S-, E- prefix
    and by xref. Isolate, make current and properties show on hover or focus, so long layer names
    are no longer cut.
  - The command line lists matching commands above it with each shortcut as a small key.
  - The ribbon's and the side panel's tabs have a sliding indicator; the side panel's tabs never
    wrap (the last fold into a menu when the panel is narrow).
  - Messages such as "Saved to …", what a modify left out and errors are quiet toasts at the bottom
    end of the window (the bottom left in Arabic); the status bar keeps to status. After a purge,
    closing its dialog offers Undo for a few seconds.
  - Right-click the drawing for Move, Copy, Erase, Properties and Zoom extents.
  - OSNAP, ORTHO, POLAR and GRID are one segmented control.
  - Mizan's conversation keeps to the newest message and offers "Jump to latest" when you have
    scrolled up. A PDF page being drawn and an update downloading show a labelled progress bar.

Rebuilt on 2026-10-13 under the same version: the Windows and macOS installers were added, the
language and theme buttons no longer cover the ribbon's last tabs in a narrow window, the
installers' metadata says Apache-2.0 (it said "UNLICENSED"), and the header shows the Qiyas mark
with the name Qiyas only.

[Unreleased]: https://github.com/qs-mind/qiyas/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/qs-mind/qiyas/releases/tag/v0.0.1
