/** Thin wrappers over the Tauri commands of qiyas-desktop (crates/qiyas-desktop/src/commands.rs). */
import { invoke } from "@tauri-apps/api/core";

export type Sheet = { name: string; block: string; is_model: boolean };

export type LayerInfo = {
  name: string;
  color: [number, number, number];
  /** Its colour as "ACI n" or "#rrggbb" (optional in tests that don't edit layers). */
  color_name?: string;
  off: boolean;
  frozen: boolean;
  locked: boolean;
  linetype?: string;
  /** Hundredths of a millimetre; -3 Default. */
  lineweight?: number;
  /** Plotted. */
  plot?: boolean;
};

/** A group of the drawing: its name and entities (`qiyas_core::GroupInfo`). */
export type GroupInfo = { name: string; handles: number[]; kinds: string[]; selectable: boolean };

/** A paper-space viewport of the sheet on screen and the layers it freezes on its own. */
export type ViewportInfo = { handle: number; frozen: string[] };

/** A layer property `set_layer` changes: on, frozen, locked and plot take booleans; color
 *  "ACI n" or "#rrggbb"; linetype a name; lineweight a number. */
export type LayerProp = "on" | "frozen" | "locked" | "color" | "linetype" | "lineweight" | "plot";

/** A property the palette edits (`qiyas_core::EntityProp`). */
export type EntityProp =
  | "layer"
  | "color"
  | "linetype"
  | "lineweight"
  | "text"
  | "text_height"
  | "text_rotation"
  | "text_style"
  | "radius"
  | "closed"
  | "global_width"
  | "insert_scale"
  | "insert_rotation"
  | "dim_style"
  /** A paper-space viewport's scale (layout units per model unit) and the model point at its
   *  centre ([x, y]). */
  | "viewport_scale"
  | "viewport_centre";

/** What match properties copies (`qiyas_core::MatchProp`). */
export type MatchProp =
  | "layer"
  | "color"
  | "linetype"
  | "lineweight"
  | "text_style"
  | "text_height"
  | "dim_style"
  | "hatch_pattern";

export const MATCH_ALL: MatchProp[] = [
  "layer",
  "color",
  "linetype",
  "lineweight",
  "text_style",
  "text_height",
  "dim_style",
  "hatch_pattern",
];

/** One line of the layer translator. */
export type LayerMapping = { from: string; to: string; color?: string | null };

export type Units = "inches" | "feet" | "millimetres" | "centimetres" | "metres" | "unknown";

/** One xref (another drawing shown in this one, `qiyas_core::XrefInfo`). */
export type XrefInfo = {
  name: string;
  /** The file was found. */
  found: boolean;
  /** The file was found and read (false when unreadable or it is the drawing itself). */
  loaded: boolean;
  /** The user chose its file ("Locate…"); "Clear chosen file" forgets it. */
  chosen: boolean;
  /** The file opened, or the path the drawing stores when none was found. */
  path: string;
  overlay: boolean;
  /** Unloaded in the drawing: listed, not opened. */
  unloaded: boolean;
  /** Layers it adds to the list, named `NAME|LAYER`. */
  layer_count: number;
  /** What went wrong finding or reading it (also in the drawing's `problems`). */
  problems: string[];
};

export type DrawingInfo = {
  path: string;
  sheets: Sheet[];
  /** The drawing's layers, then its xrefs' as `NAME|LAYER`, sorted by name. */
  layers: LayerInfo[];
  units: Units;
  problems: string[];
  /** The drawing holds its Arabic text backwards (typed in visual order for old CAD fonts). */
  arabic_reversed: boolean;
  /** Its xrefs and those inside them, in load order. */
  xrefs: XrefInfo[];
  /** Block name of the sheet on screen. */
  sheet: string;
  /** Physical layout paper size in millimetres, after rotation; null on Model. */
  paper?: Paper | null;
  /** The layers the user has hidden, sorted. */
  hidden_layers: string[];
  /** The layer isolated on screen (LAYISO), if one is. */
  isolated_layer?: string | null;
  /** What new entities are drawn with. */
  current: Current;
  /** The drawing's own text styles, sorted. */
  text_styles: string[];
  /** The style new text takes: the drawing's current one, or "Standard". */
  text_style: string;
  /** The drawing's own dimension styles, sorted. */
  dim_styles: DimStyleInfo[];
  /** The style new dimensions take: the drawing's current one, or "Standard". */
  dim_style: string;
  /** The annotation scale (CANNOSCALE), e.g. "1:50". */
  annotation_scale: string;
  /** Drawing units per paper unit it gives (50 for "1:50"): new text and annotations are that
   *  many times their paper size. */
  annotation_factor: number;
  /** The block being edited in place, if one is. */
  block_edit: string | null;
  /** The drawing's linetypes, sorted (optional in older tests). */
  linetypes?: string[];
  /** The drawing's layer states. */
  layer_states?: string[];
  /** The drawing's groups. */
  groups?: GroupInfo[];
  /** The paper-space viewports of the sheet on screen. */
  viewports?: ViewportInfo[];
  /** The user coordinate system (world when left out). */
  ucs?: Ucs;
  /** The drawing has (or had) parametric constraints (left out: none). */
  has_constraints?: boolean;
  /** A new drawing never saved yet: `path` is a placeholder, Save goes through Save As. */
  untitled?: boolean;
};

/** A user coordinate system: an origin and a unit x axis in the plan (`qiyas_core::Ucs`). */
export type Ucs = { origin: P; x_axis: P };

/** One viewport of a layout and how model space draws inside it (`qiyas_core::PaperView`):
 *  its frame on the layout, the model point at its centre, layout units per model unit, and the
 *  spans [first, count] of the model batch's segments, fills and glyph quads it draws (null:
 *  all of them). */
export type PaperView = {
  handle: number;
  rect: [P, P];
  centre: P;
  scale: number;
  lines: [number, number][] | null;
  fills: [number, number][] | null;
  texts: [number, number][] | null;
};

/** A dimension style's common fields, in drawing units before its `scale` (DIMASZ, DIMTXT,
 *  DIMDEC, DIMEXO, DIMEXE, DIMGAP, DIMSCALE). */
export type DimStyleFields = {
  arrow_size: number;
  text_height: number;
  precision: number;
  ext_offset: number;
  ext_extension: number;
  gap: number;
  scale: number;
  /** Show the drawing's unit after the number ("12 m"); saved with the style. */
  show_units: boolean;
};

export type DimStyleInfo = { name: string; fields: DimStyleFields };

/** A dimension to add (`qiyas_core::DimSpec`); points in drawing units, `rotation` in degrees
 *  (null: horizontal or vertical by where `line_point` is). */
export type DimSpec =
  | { kind: "linear"; p1: P; p2: P; line_point: P; rotation: number | null }
  | { kind: "aligned"; p1: P; p2: P; offset: number }
  | { kind: "angular"; vertex: P; p1: P; p2: P; arc_point: P }
  | { kind: "radius"; target: EntityTarget; point: P }
  | { kind: "diameter"; target: EntityTarget; point: P }
  | { kind: "arc_length"; target: EntityTarget; point: P }
  | { kind: "ordinate"; feature: P; leader_end: P }
  | { kind: "baseline"; from: EntityTarget; points: P[] }
  | { kind: "continue"; from: EntityTarget; points: P[] }
  | { kind: "quick"; targets: EntityTarget[]; line_point: P };

/** What a dimension edit changes: its text (`<>` is the measurement, "" clears), its arrows, or
 *  where its text sits. */
export type DimEditOp = { text_override: string } | "flip" | { move_text: P };

/** In `problems` when the drawing opened with nothing in model space (`qiyas_core::EMPTY_MODEL`). */
export const EMPTY_MODEL = "This drawing has no geometry in Model";

export type EntityInfo = {
  handle: number;
  kind: string;
  /** The layer it is drawn on (layer "0" inside a block takes its insert's). */
  layer: string;
  /** `layer` as shown to the QS: a bound xref's layer without its `XREF$0$` (A-WALL). */
  layer_short: string;
  /** In drawing units, where its block references place it. */
  length: number | null;
  area: number | null;
  /** For a block reference, its block (a dynamic block by its own name). */
  block_name: string | null;
  text: string | null;
  /** `text` as read: an MText's without its formatting codes, its lines split by "\n". */
  plain_text?: string | null;
  /** The block the entity sits in, when it is inside one. */
  inside_block: string | null;
  /** The block references it sits inside, outermost first; empty on the sheet. */
  chain: number[];
  /** The block names along `chain`, outermost first. */
  block_path: string[];
  /** The xref the entity is in; `handle` is then that drawing's. */
  xref?: string | null;
  /** Its own colour: "ByLayer", "ByBlock", "None", "ACI n" or "#rrggbb". */
  color: string;
  /** The colour it is drawn with (ByLayer and ByBlock resolved). */
  color_rgb: [number, number, number];
  /** Its linetype name, or "ByLayer". */
  linetype: string;
  /** In hundredths of a millimetre; -1 ByLayer, -2 ByBlock, -3 Default. */
  lineweight: number;
  /** A circle's or an arc's radius, in drawing units. */
  radius: number | null;
  /** A block reference's attributes, as [tag, value]. */
  attributes: [string, string][];
  /** A text's height, in drawing units, and its style. */
  height: number | null;
  style: string | null;
  /** A table's cells. */
  table?: TableInfo | null;
  /** A text's or a block reference's own rotation, in degrees. */
  rotation?: number | null;
  /** A polyline is closed; its constant width. */
  closed?: boolean | null;
  global_width?: number | null;
  /** A block reference's scale. */
  scale?: number | null;
  /** A paper-space viewport's scale (layout units per model unit) and the model point at its
   *  centre. */
  viewport_scale?: number | null;
  viewport_centre?: P | null;
  /** A line's or an arc's end points, a circle's, arc's or ellipse's centre, in world
   *  coordinates and drawing units. */
  start?: P | null;
  end?: P | null;
  centre?: P | null;
  /** An arc's start and end angles in degrees (0 up to 360, counter-clockwise). */
  start_angle?: number | null;
  end_angle?: number | null;
  /** A polyline's vertices. */
  vertex_count?: number | null;
  /** Where a text, an MText or a block reference is inserted, in world coordinates. */
  position?: P | null;
  /** A hatch's pattern name ("SOLID" for a solid fill). */
  pattern?: string | null;
};

/** A table's cells for the table editor (`qiyas_core::TableInfo`). */
export type TableInfo = {
  /** Each cell's text as read (formatting codes gone), row by row. */
  cells: string[][];
  /** Column widths and row heights, in drawing units. */
  widths: number[];
  heights: number[];
  /** Merged cells: [top row, left column, bottom row, right column]. */
  merges: [number, number, number, number][];
  /** The cells holding a field: [row, column]. */
  fields: [number, number][];
};

/** A field (`qiyas_core::FieldKind`): today's date, the file name, an entity's area or length,
 *  or a count of a block's references on the sheet. */
export type FieldKind =
  | { kind: "date" }
  | { kind: "file_name" }
  | { kind: "area"; target: EntityTarget }
  | { kind: "length"; target: EntityTarget }
  | { kind: "count"; block: string };

/** What is under the cursor (`qiyas_core::Picked`): the entity, the block references it sits inside
 *  (outermost first, empty on the sheet) and the top-level entity on the sheet. Inside an xref,
 *  `chain` ends with the xref's reference and `xref` names it. */
/** `kind`: the kind of the entity at the top ("Line", "Insert", "Dimension"…). */
export type Picked = { top: number; entity: number; chain: number[]; xref: string | null; kind?: string | null };

/** Filter for the session's `find_entities` query and `extract_data`. Names match case-insensitively. */
export type EntityFilter = {
  /** Sheet block or layout name; the current sheet when null. */
  sheet?: string | null;
  layer?: string | null;
  kind?: string | null;
  block_name?: string | null;
  text_contains?: string | null;
  /** [[min_x, min_y], [max_x, max_y]]: entities with at least one point inside. */
  within?: [[number, number], [number, number]] | null;
  offset?: number;
  /** Rows per page, at most 500 (5,000 for `extract_data`); 100 when left out. */
  limit?: number;
};

/** One column of `extract_data` (`qiyas_core::Field`): lengths, areas, X and Y in metres. */
export type Field =
  | "handle"
  | "kind"
  | "layer"
  | "block_name"
  | "text"
  | "length"
  | "area"
  | "x"
  | "y"
  | "sheet"
  | "xref"
  | { attribute: string };

/** One entity found. Length and area in drawing units; bounds in world coordinates. */
export type EntityRow = {
  handle: number;
  kind: string;
  layer: string;
  block_name: string | null;
  text: string | null;
  length: number | null;
  area: number | null;
  bounds: [[number, number], [number, number]];
  /** Set for an entity of an xref; measure it with `chain`. */
  xref?: string;
  chain?: number[];
};

/** One page of results; `total` counts every match. */
export type Page<T> = { rows: T[]; total: number; next_offset: number | null };

export type TextRow = {
  handle: number;
  text: string;
  at: [number, number];
  height: number;
  inside_block: string | null;
  xref?: string;
};

/** A dimension; `measurement` in drawing units (degrees for angular), null when not computed. */
export type DimRow = {
  handle: number;
  kind: string;
  measurement: number | null;
  text_override: string | null;
  at: [number, number];
};

/** A table read off a sheet. `source` is the table entity's handle, or 0 for a text grid. */
export type Schedule = {
  name: string;
  sheet: string;
  source: number;
  columns: string[];
  rows: string[][];
};

/** References of one block on one layer; `attributes` are their distinct attribute values. */
export type BlockCount = { block_name: string; layer: string; count: number; attributes: string[] };

/** Opens the drawing in a new tab, or shows its tab when it is open already. */
export const openDrawing = (path: string) => invoke<DrawingInfo>("open_drawing", { path });

/** A new drawing's units: metres, centimetres or millimetres (INSUNITS 6, 5, 4). */
export type NewUnits = "m" | "cm" | "mm";

/** A new, empty drawing in a new tab, untitled until its first Save As; its info. The assistant
 *  starts a new conversation. */
export const newDrawing = (units: NewUnits) => invoke<DrawingInfo>("new_drawing", { units });

/** One file tab (`qiyas_desktop::tabs::TabInfo`). */
export type TabInfo = { id: number; path: string; title: string; dirty: boolean; untitled?: boolean };

/** Every open drawing's tab, in order, and the one on screen (null when none is open). */
export type Tabs = { tabs: TabInfo[]; active: number | null };

export const tabs = () => invoke<Tabs>("tabs");

/** Shows the drawing of tab `id`; its info. The assistant starts a new conversation, unless
 *  `keepConversation` (Mizan asked to close it, and it is shown first). */
export const activateTab = (id: number, keepConversation = false) =>
  invoke<DrawingInfo>("activate_tab", { id, keepConversation });

/** What `closeTab` refuses with for a drawing with unsaved changes, without `force`. */
export const UNSAVED = "unsaved";

/** Closes tab `id`; the tabs left. `keepConversation` as for `activateTab`. */
export const closeTab = (id: number, force = false, keepConversation = false) =>
  invoke<Tabs>("close_tab", { id, force, keepConversation });

/** A drawing opened or saved lately. */
export type Recent = { path: string; name: string; folder: string; missing: boolean };

/** The recent drawings, most recent first (at most 10). */
/** Untitled drawings whose autosave is still in the app's folder (Qiyas closed before their
 *  first Save As): asked about at launch with the recent drawings. */
export const untitledAutosaves = () => invoke<string[]>("untitled_autosaves");

export const recents = () => invoke<Recent[]>("recents");

export const clearRecents = () => invoke<void>("clear_recents");

/** A named view of model space: its name and the box it shows on the drawing. */
export type NamedView = { name: string; bounds: [[number, number], [number, number]] };

export const views = () => invoke<NamedView[]>("views");

export const drawingInfo = () => invoke<DrawingInfo>("drawing_info");

export const setSheet = (block: string) => invoke<void>("set_sheet", { block });

export const setLayerHidden = (name: string, hidden: boolean) =>
  invoke<void>("set_layer_hidden", { name, hidden });

/** Looks for every xref again and reopens it; the drawing's info with the new list. */
export const reloadXrefs = () => invoke<DrawingInfo>("reload_xrefs");

/** Uses the file `path` for the xref `name` (null: look for it again), then reloads the xrefs.
 *  Saving the project remembers the choice. */
export const setXrefPath = (name: string, path: string | null) =>
  invoke<DrawingInfo>("set_xref_path", { name, path });

/** Names the current sheet's batch: the same number means the same batch. */
export const batchVersion = () => invoke<number>("batch_version");

/** What `render_batch` sends instead of a batch that is still `ifVersion`: "QINM" and four zeros. */
export function isNotModified(bytes: ArrayBuffer): boolean {
  const marker = [0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 0];
  const got = new Uint8Array(bytes);
  return got.length === marker.length && marker.every((b, i) => got[i] === b);
}

/** Raw bytes of the current sheet's batch; decode with `parseBatch`. `null` when the batch is
 *  still `ifVersion` (from `batchVersion`), so the caller keeps the one it has. `pxPerUnit` is the
 *  zoom to draw the text glyphs for (screen pixels per drawing unit); left out, the core keeps the
 *  zoom of its batch, or fits a new sheet. */
export async function renderBatch(
  ifVersion?: number,
  pxPerUnit?: number,
): Promise<ArrayBuffer | null> {
  const bytes = await invoke<ArrayBuffer>("render_batch", { ifVersion, pxPerUnit });
  return isNotModified(bytes) ? null : bytes;
}

/** Names the batch of the sheet `sheet` (block name), which need not be the one on screen. */
export const sheetBatchVersion = (sheet: string) => invoke<number>("sheet_batch_version", { sheet });

/** `renderBatch` for the sheet `sheet`, which stays off screen: model space for a layout's
 *  viewports, or a split window's second view. */
export async function sheetBatch(
  sheet: string,
  ifVersion?: number,
  pxPerUnit?: number,
): Promise<ArrayBuffer | null> {
  const bytes = await invoke<ArrayBuffer>("sheet_batch", { sheet, ifVersion, pxPerUnit });
  return isNotModified(bytes) ? null : bytes;
}

/** The viewports of the sheet `sheet` (the one on screen when left out) and how model space
 *  draws inside each. */
export const paperViews = (sheet?: string) => invoke<PaperView[]>("paper_views", { sheet: sheet ?? null });

/** Raw bytes of the glyph atlas pages changed after `sinceVersion` (all when left out); decode
 *  with `parseAtlas`. */
export const atlasPages = (sinceVersion?: number) =>
  invoke<ArrayBuffer>("atlas_pages", { sinceVersion });

/** How `attachPdf` brings a page in: a picture under the drawing, kept in the DWG as a PDF underlay,
 *  or its vectors and texts as entities. */
export type AttachMode = "underlay" | "import";

/** Event with the percent drawn (0 to 100) of the underlay picture pdfium is rendering. */
export const PDF_PROGRESS = "pdf:progress";

/** How many pages the PDF at `path` has. Fails with "PDF support is not installed: run
 *  scripts/pdfium.sh" when pdfium is missing. */
export const pdfPageCount = (path: string) => invoke<number>("pdf_page_count", { path });

/** Width and height in points (1/72 inch) of page `page` (from 1), as shown. */
export const pdfPageSize = (path: string, page: number) =>
  invoke<[number, number]>("pdf_page_size", { path, page });

/** Raw bytes of pages `first` to `first + count - 1` drawn to fit `px` x `px`; decode with
 *  `parseThumbnails`. */
export const pdfThumbnails = (path: string, first: number, count: number, px: number) =>
  invoke<ArrayBuffer>("pdf_thumbnails", { path, first, count, px });

/** Attaches page `page` of the PDF at `path` to the sheet on screen, its bottom-left corner at
 *  `at`, `scale` drawing units per point. Resolves to the underlay's handle, or for an import the
 *  number of entities added. One undo step. */
export const attachPdf = (
  path: string,
  page: number,
  at: [number, number],
  scale: number,
  mode: AttachMode,
) => invoke<number>("attach_pdf", { path, page, x: at[0], y: at[1], scale, mode });

/** True when the drawing itself has changes not saved yet (PDF imports, underlays, calibration);
 *  the take-off is saved in the project file after every command. */
export const isDirty = () => invoke<boolean>("is_dirty");

/** Rescales the PDF underlay or image under `p1` about `p1` so that `p1` to `p2` is `distance`
 *  drawing units. One undo step. */
export const calibrate = (p1: [number, number], p2: [number, number], distance: number) =>
  invoke<void>("calibrate", { x1: p1[0], y1: p1[1], x2: p2[0], y2: p2[1], distance });

/** Raw bytes of picture `id` (from the batch's image table: a PDF underlay or an image) for a zoom
 *  of `pxPerUnit` screen pixels per drawing unit; decode with `parsePicture`. */
export const underlayImage = (id: number, pxPerUnit: number) =>
  invoke<ArrayBuffer>("underlay_image", { id, pxPerUnit });

/** The raster image types Attach image offers. */
export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"];

/** Width and height in pixels of the image at `path`, as shown (its EXIF orientation applied). */
export const imageSize = (path: string) => invoke<[number, number]>("image_size", { path });

/** Attaches the image at `path` to the sheet on screen, bottom-left corner at `at`, `width` drawing
 *  units wide. Resolves to its handle. One undo step. */
export const attachImage = (path: string, at: [number, number], width: number) =>
  invoke<number>("attach_image", { path, x: at[0], y: at[1], width });

/** An image's brightness, contrast and fade, 0 to 100 each (50, 50 and 0: as it is). */
export type ImageDisplay = { brightness: number; contrast: number; fade: number };

/** The brightness, contrast and fade of the image `handle`; null when it is not an image. */
export const imageDisplay = (handle: number) =>
  invoke<ImageDisplay | null>("image_display", { handle });

/** The entity nearest to the drawing point (x, y) within `tol` drawing units, down to the entity
 *  inside any blocks. */
export const pick = (x: number, y: number, tol: number) =>
  invoke<Picked | null>("pick", { x, y, tol });

/** What a selection box from `bounds[0]` to `bounds[1]` (drawing coordinates) picks: the entities
 *  wholly inside it, or with `crossing` those touching it. Always whole entities at the top of the
 *  sheet (`top === entity`, no chain), sorted by handle. */
export const pickWindow = (bounds: [P, P], crossing: boolean) =>
  invoke<Picked[]>("pick_window", {
    minX: bounds[0][0],
    minY: bounds[0][1],
    maxX: bounds[1][0],
    maxY: bounds[1][1],
    crossing,
  });

/** What a selection polygon `points` (drawing coordinates, three corners or more) picks: the
 *  entities wholly inside it, or with `crossing` those touching it. Whole entities at the top of
 *  the sheet, sorted by handle, as `pickWindow`. */
export const pickPolygon = (points: P[], crossing: boolean) =>
  invoke<Picked[]>("pick_polygon", { points, crossing });

/** What kind of point an object snap found (`qiyas_core::SnapKind`), best first. */
export type SnapKind =
  | "end"
  | "intersection"
  | "mid"
  | "centre"
  | "node"
  | "perpendicular"
  | "nearest";

/** A point found by an object snap, on the entity `handle` inside the block references `chain`. */
export type Snap = { point: [number, number]; kind: SnapKind; handle: number; chain: number[] };

export type SnapModes = Record<SnapKind, boolean>;

/** How the window draws and measures (`qiyas_core::Drafting`), saved in the project file. */
export type Drafting = {
  /** Object snaps on at all (F3). */
  osnap: boolean;
  snap_modes: SnapModes;
  /** Each next point straight across or up from the one before (F8). */
  ortho: boolean;
  /** Each next point's direction snapped to a multiple of `polar_step_deg` when close (F10). */
  polar: boolean;
  polar_step_deg: number;
  /** The dot grid shown (F7), `grid_spacing` drawing units apart, every tenth dot a major one. */
  grid: boolean;
  grid_spacing: number;
  /** Points go to the nearest grid dot when no object snap is found. */
  grid_snap: boolean;
  /** Decimals shown in the take-off and the status bar: 2, 3 or 4. */
  precision: number;
};

/** What `boundaryAt` finds (`qiyas_core::Boundary`): the ring around the point (counter-clockwise)
 *  and its islands, with each ring's bulges (the ring first; an edge along a drawn circle or arc
 *  is that arc), no closed area, or too much line work there to look through (zoom in). */
export type Boundary =
  | { found: "ring"; ring: [number, number][]; holes: [number, number][][]; bulges: number[][] }
  | { found: "none" }
  | { found: "too_dense" };

/** The closed area around (x, y) the drawn lines make, for a hatch: looked for `start` drawing
 *  units around it first, then 4 times as far each time until one fits. */
export const boundaryAt = (x: number, y: number, start: number) =>
  invoke<Boundary>("boundary_at", { x, y, start });

/** The object snap within `tol` drawing units of (x, y) with the drawing's snap modes; `from` is the
 *  previous point, for perpendicular snaps. Null when there is none or snaps are off. */
export const snap = (x: number, y: number, tol: number, from: [number, number] | null = null) =>
  invoke<Snap | null>("snap", { x, y, tol, fromX: from?.[0] ?? null, fromY: from?.[1] ?? null });

export const getDrafting = () => invoke<Drafting>("get_drafting");

/** Replaces the drafting settings (not an undo step). Rejects a value out of range. */
export const setDrafting = (drafting: Drafting) => invoke<void>("set_drafting", { drafting });

/** One entity; give `chain` for an entity inside blocks. */
export const entityInfo = (handle: number, chain: number[] = []) =>
  invoke<EntityInfo>("entity_info", { handle, chain });

/** The cell [row, column] of the table `handle` under `p`; a merged cell by its first. */
export const tableCellAt = (handle: number, p: [number, number]) =>
  invoke<[number, number] | null>("table_cell_at", { handle, x: p[0], y: p[1] });

export type Kind = "length" | "area" | "count" | "volume" | "item";

/** Who made a measurement: the user, the in-app AI, or an outside AI through MCP. */
export type Source = "user" | "ai" | "mcp";

/**
 * One take-off line. `value` is in drawing units (units² for areas, units³ for volumes, the
 * item's own unit for items); `shown` is in `unit`.
 */
export type Measurement = {
  quantity_valid?: boolean;
  area_origin?: "unspecified" | "existing_boundary" | "wall_face" | "manual_or_entity";
  measurement_basis?: "unspecified" | "gross" | "wall_face" | "unapproved_net" | "approved_net";
  count_scope?: string | null;
  id: string;
  kind: Kind;
  value: number | null;
  /** "m", "m2", "m3", "pcs", or an item's own unit. */
  unit: string;
  shown: number | null;
  description: string;
  layer: string;
  sheet: string;
  handles: number[];
  points: [number, number][];
  block_name: string | null;
  self_intersecting: boolean;
  source: Source;
  note: string;
  created: string;
  /** A volume's height, in drawing units. */
  height: number | null;
  /** Code of the linked BOQ item. */
  boq_code: string | null;
  /** The block references the measured entity sits inside, outermost first; empty otherwise. */
  chain: number[];
  /** The block names along `chain`, outermost first. */
  block_path: string[];
  /** The xref the measured entities are in (`chain` ends with its reference). */
  xref?: string | null;
};

export type Total = { kind: Kind; unit: Measurement["unit"]; count: number; sum: number };

/** One BOQ line (`qiyas_core::BoqItem`). */
export type BoqItem = {
  code: string;
  description: string;
  /** "m", "m2", "m3", "pcs", "kg", "t", or the workbook's own text. */
  unit: string;
  quantity: number;
  rate: number | null;
  /** The section heading above the item; "" when none. */
  section: string;
  /** Allowed difference in percent; null: the user's setting. */
  tolerance: number | null;
  /** The item's row in its sheet, from 1 as Excel shows it. */
  row: number;
};

/** An imported bill of quantities (`qiyas_core::Boq`). */
export type Boq = {
  /** The workbook it was read from. */
  source: string;
  /** RFC 3339. */
  imported: string;
  items: BoqItem[];
  /** Item rows left out because their quantity is not a number. */
  skipped_rows: number;
};

/** What a comparison found (`qiyas_core::ConflictKind`). */
export type ConflictKind =
  | "quantity_mismatch"
  | "missing_from_boq"
  | "not_in_drawings"
  | "schedule_vs_plan"
  | "drawing_vs_drawing"
  | "unit_doubt"
  | "duplicate_item"
  | "relation_mismatch"
  | "document_mismatch";

export type ConflictStatus = "open" | "resolved" | "ignored";

/** One conflict of a comparison (`qiyas_core::Conflict`). Quantities are in `unit`. */
export type Conflict = {
  /** 16 hex digits, the same on every comparison for the same finding. */
  id: string;
  kind: ConflictKind;
  boq_code: string | null;
  /** Ids of the measurements involved, sorted. */
  measurements: string[];
  /** Name of the schedule involved. */
  schedule: string | null;
  /** Names of the sheets involved (layout names), sorted. */
  sheets: string[];
  /** What the BOQ or the schedule (or the first sheet) says. */
  expected: number | null;
  /** What the drawings measure (or the second sheet counts). */
  found: number | null;
  unit: string;
  /** A short English sentence with the numbers in it. */
  reason: string;
  reason_ar: string;
  status: ConflictStatus;
  note: string;
  /** When it was first found, RFC 3339. */
  created: string;
  /** What `id` is hashed from, for debugging. */
  hash_key: string;
  query?: {
    severity: "information" | "warning" | "error";
    question_en: string;
    question_ar: string;
    references: { handle: number; sheet: string; grid: string | null }[];
    citations: { source: string; clause: string }[];
    clouds: number[];
    rule: string | null;
  } | null;
};

type P = [number, number];

/** A hatch's fill (`qiyas_core::HatchPattern`): a pattern of the built-in table by name (ANSI31,
 *  ANSI37, LINE, NET, DASH, BRICK) turned by `angle` degrees and scaled by `scale`. */
export type HatchPattern =
  | "solid"
  | "ansi31"
  | "ansi37"
  | { custom: { name: string; angle: number; scale: number } };

/** What a draw adds (`qiyas_core::DrawSpec`), in drawing coordinates; angles in degrees. */
export type DrawSpec =
  | { kind: "line"; a: P; b: P }
  /** One bulge per point (the segment starting there), or none. */
  | { kind: "polyline"; points: P[]; bulges: number[]; closed: boolean }
  | { kind: "rectangle"; a: P; b: P }
  | { kind: "polygon"; centre: P; sides: number; radius: number; inscribed: boolean }
  | { kind: "circle"; centre: P; radius: number }
  /** Counter-clockwise from `start` to `end`. */
  | { kind: "arc"; centre: P; radius: number; start: number; end: number }
  | { kind: "three_point_arc"; a: P; b: P; c: P }
  /** `major`: the major semi-axis from the centre; `ratio` minor over major (0 to 1]. */
  | { kind: "ellipse"; centre: P; major: P; ratio: number }
  | { kind: "spline"; fit_points: P[] }
  | { kind: "point"; p: P }
  /** Inner and outer diameters. */
  | { kind: "donut"; centre: P; inner: number; outer: number }
  | { kind: "ray"; origin: P; dir: P }
  | { kind: "xline"; origin: P; dir: P }
  | { kind: "revcloud"; points: P[]; arc_len: number }
  | { kind: "wipeout"; points: P[] }
  /** Inside the closed entities `boundary_handles` or the ring `points` (less the islands
   *  `holes`, with each ring's `bulges` as `Boundary` gives them): one of the two. */
  | {
      kind: "hatch";
      boundary_handles: number[];
      points: P[];
      holes?: P[][];
      bulges?: number[][];
      pattern: HatchPattern;
    }
  | { kind: "solid2d"; points: P[] }
  | { kind: "trace"; points: P[]; width: number }
  | { kind: "mline"; points: P[]; style: string | null; scale: number | null }
  | { kind: "sketch"; points: P[] };

/** An entity as picked: its handle and the block references it sits inside (empty on the sheet). */
export type EntityTarget = { handle: number; chain: number[] };

/** What a modify changes (`qiyas_core::ModifySpec`): entities at the top of the sheet, in drawing
 *  coordinates; angles in degrees counter-clockwise. */
export type ModifySpec =
  | { kind: "move"; targets: EntityTarget[]; delta: P }
  /** `count` copies, each `delta` further than the one before. */
  | { kind: "copy"; targets: EntityTarget[]; delta: P; count: number }
  | { kind: "rotate"; targets: EntityTarget[]; base: P; angle: number; copy: boolean }
  | { kind: "scale"; targets: EntityTarget[]; base: P; factor: number; copy: boolean }
  /** `keep`: mirror a copy and keep the originals. */
  | { kind: "mirror"; targets: EntityTarget[]; p1: P; p2: P; keep: boolean }
  /** `window`: [lower left, upper right]; the vertices inside it move. */
  | { kind: "stretch"; targets: EntityTarget[]; window: [P, P]; delta: P }
  /** One or two [source, destination] pairs. */
  | { kind: "align"; targets: EntityTarget[]; pairs: [P, P][] }
  | { kind: "array_rect"; targets: EntityTarget[]; rows: number; cols: number; dy: number; dx: number }
  /** `count` items, the originals included, each turned `angle` from the one before. */
  | { kind: "array_polar"; targets: EntityTarget[]; centre: P; count: number; angle: number }
  | { kind: "explode"; targets: EntityTarget[] };

/** A polyline edit (`qiyas_core::PEditOp`). */
export type PEditOp =
  | "close"
  | "open"
  | "fit_curve"
  | "decurve"
  | "convert_to_polyline"
  | { width: number }
  /** A vertex on the polyline nearest the point. */
  | { add_vertex: P }
  /** The vertex nearest the point goes. */
  | { delete_vertex: P };

/** What an edit changes (`qiyas_core::EditSpec`): entities at the top of the sheet, in drawing
 *  coordinates. `at_*` points are where the entity was clicked: which piece, end or side. */
export type EditSpec =
  /** No cutters: everything drawn cuts. */
  | { kind: "trim"; cutters: EntityTarget[]; target: EntityTarget; at_point: P }
  /** No boundaries: everything drawn. */
  | { kind: "extend"; boundaries: EntityTarget[]; target: EntityTarget; at_end: P }
  | { kind: "break"; target: EntityTarget; p1: P; p2: P }
  | { kind: "join"; targets: EntityTarget[] }
  | {
      kind: "lengthen";
      target: EntityTarget;
      at_end: P;
      by: { delta: number } | { total: number } | { percent: number };
    }
  | { kind: "reverse"; targets: EntityTarget[] }
  | { kind: "offset"; target: EntityTarget; distance: number; side_point: P }
  | { kind: "fillet"; a: EntityTarget; b: EntityTarget; at_a: P; at_b: P; radius: number }
  | { kind: "chamfer"; a: EntityTarget; b: EntityTarget; at_a: P; at_b: P; d1: number; d2: number }
  | { kind: "blend"; a: EntityTarget; b: EntityTarget; at_a: P; at_b: P }
  | { kind: "pedit"; target: EntityTarget; op: PEditOp }
  | { kind: "overkill"; targets: EntityTarget[]; tol: number }
  | { kind: "flatten"; targets: EntityTarget[] };

/** What new entities are drawn with (`qiyas_core::Current`). */
export type Current = {
  layer: string;
  /** "ByLayer", "ByBlock", "ACI n" or "#rrggbb". */
  color: string;
  /** "ByLayer", "ByBlock" or a linetype of the drawing. */
  linetype: string;
  /** Hundredths of a millimetre; -1 ByLayer, -2 ByBlock, -3 Default. */
  lineweight: number;
};

/** The command bus (`qiyas_core::Command`). Points are in drawing coordinates. */
export type RecognitionCandidate = {
  handle: number; rule: string; confidence: number; state: "pending" | "accepted" | "rejected";
  placement: { type: string; props: Record<string, string | number | boolean> };
  evidence: { handle?: number; path?: string; page?: number; detail: string }[];
  issues: string[]; unresolved: string[]; object: number | null;
};
export type Command =
  | { type: "recognition"; kind: "queue"; state?: "pending" | "accepted" | "rejected" }
  | { type: "recognition"; kind: "review"; handle: number; decision: "accepted" | "rejected";
      props?: Record<string, string | number | boolean>; object_type?: string; save_as?: string }

  | { type: "measure_distance"; points: [number, number][] }
  | { type: "measure_area"; points: [number, number][] }
  /** `chain`: the block references the entity sits inside, outermost first; empty on the sheet. */
  | { type: "measure_entity"; handle: number; chain: number[] }
  | { type: "count_block"; block_name: string; layer: string | null }
  | { type: "remove_measurement"; id: string }
  | { type: "set_description"; id: string; text: string }
  | { type: "set_layer_hidden"; name: string; hidden: boolean }
  /** Shows only the layer (LAYISO); the layer isolated already un-isolates. Not an undo step. */
  | { type: "isolate_layer"; name: string }
  /** Puts back what isolating hid (LAYUNISO). Not an undo step. */
  | { type: "unisolate_layers" }
  | { type: "set_sheet"; block: string }
  /** The drawing holds its Arabic text backwards; not an undo step. */
  | { type: "set_arabic_reversed"; on: boolean }
  /** The drafting settings; not an undo step. */
  | { type: "set_drafting"; drafting: Drafting }
  /** Length, area and count values come from the geometry (points or handles, not both). */
  | {
      type: "add_measurement";
      kind: Kind;
      value: number | null;
      unit: string | null;
      height: number | null;
      points: [number, number][];
      handles: number[];
      description: string;
      note: string;
    }
  | { type: "set_height"; id: string; height: number }
  | { type: "link_boq"; id: string; code: string | null }
  | { type: "set_note"; id: string; text: string }
  /** Replaces the BOQ; `importBoq` reads one from Excel and runs this. */
  | { type: "set_boq"; boq: Boq }
  /** Allowed difference in percent (0 or more) for the BOQ items with `code`; null: the user's setting. */
  | { type: "set_boq_tolerance"; code: string; percent: number | null }
  /** Replaces the conflicts and schedules; `compare` runs this. */
  | { type: "set_conflicts"; conflicts: Conflict[]; schedules: Schedule[] }
  | { type: "resolve_conflict"; id: string; status: ConflictStatus; note: string }
  /** `attachPdf` runs these two; `scale` is drawing units per PDF point. */
  | { type: "import_pdf"; path: string; page: number; insert_at: [number, number]; scale: number }
  | {
      type: "attach_pdf_underlay";
      path: string;
      page: number;
      insert_at: [number, number];
      scale: number;
    }
  /** `calibrate` runs this. */
  | {
      type: "calibrate";
      handle: number;
      p1: [number, number];
      p2: [number, number];
      real_distance: number;
    }
  /** `attachImage` runs this; `width_units` is the image's width in drawing units. */
  | { type: "attach_image"; path: string; insert_at: [number, number]; width_units: number }
  /** 0 to 100 each; one undo step. */
  | {
      type: "set_image_display";
      handle: number;
      brightness: number;
      contrast: number;
      fade: number;
    }
  /** The entity's geometry as an `info` outcome; `at` picks a polyline's nearest segment. */
  | { type: "measure_geometry"; handle: number; chain: number[]; at: [number, number] | null }
  /** Points on layer QIYAS-POINT dividing the path into `count` parts (2 to 1000), counted. */
  | { type: "divide_along"; handle: number; chain: number[]; count: number }
  /** A point every `step` drawing units along the path from its start, counted. */
  | { type: "measure_along"; handle: number; chain: number[]; step: number }
  /** Where `p` is, in metres, and the snap that found it, as an `info` outcome. */
  | { type: "id_point"; p: [number, number]; kind: SnapKind | null }
  /** A `table` outcome: one row of `fields` per entity `filter` finds (up to 5,000). */
  | { type: "extract_data"; filter: EntityFilter; fields: Field[] }
  /** Saves the named view `name` (replacing one of that name) showing `bounds`; Model sheet only. */
  | { type: "save_view"; name: string; bounds: [[number, number], [number, number]] }
  /** Removes the named view `name`; `nothing` when there is none. */
  | { type: "delete_view"; name: string }
  /** Adds what the spec describes on the current layer: one undo step, an `added` outcome. */
  | ({ type: "draw" } & DrawSpec)
  /** What new entities are drawn with; not an undo step. */
  | { type: "set_current"; current: Current }
  /** Deletes entities at the top of the sheet: one undo step. */
  | { type: "delete_entities"; targets: EntityTarget[] }
  /** Moves, copies, turns… entities at the top of the sheet: one undo step (`changed`, or `added`
   *  with the new handles). */
  | ({ type: "modify" } & ModifySpec)
  /** Trims, extends, breaks, joins… entities at the top of the sheet: one undo step (`edited`, or
   *  `added` for offset and blend). */
  | ({ type: "edit" } & EditSpec)
  /** One line of text on the current layer; `rotation` in degrees; `style` null: the drawing's
   *  current one. One undo step, an `added` outcome. */
  | { type: "text"; at: P; height: number; rotation: number; value: string; style: string | null }
  /** Plain text (lines split by "\n") in a box `width` wide (0: no wrapping), top left at `at`. */
  | { type: "mtext"; at: P; width: number; height: number; value: string; style: string | null }
  /** A text's or an MText's new value (plain text), or a block reference's attribute `tag`'s. */
  | { type: "edit_text"; target: EntityTarget; value: string; tag: string | null }
  /** One undo step (none when nothing matches): a `replaced` outcome. */
  | {
      type: "find_replace";
      find: string;
      replace: string;
      scope: TextScope;
      match_case: boolean;
      whole_word: boolean;
    }
  /** The matches, without changing anything: a `found` outcome. */
  | { type: "find_text"; find: string; scope: TextScope; match_case: boolean; whole_word: boolean }
  /** Dimensions on the current layer in the current style, each with its block: an `added`
   *  outcome, one undo step. */
  | ({ type: "dimension" } & DimSpec)
  | { type: "dim_edit"; target: EntityTarget; op: DimEditOp }
  /** A gap in the dimension's lines where `cutter` crosses them. */
  | { type: "dim_break"; target: EntityTarget; cutter: EntityTarget }
  /** Parallel linear dimensions `spacing` apart going out from `base` (null: twice the text). */
  | { type: "dim_space"; base: EntityTarget; targets: EntityTarget[]; spacing: number | null }
  /** A jog in a linear dimension's line nearest `at`. */
  | { type: "dim_jog_line"; target: EntityTarget; at: P }
  /** A leader through `points` (arrow at the first) with `text` beside the last: `added` with
   *  the leader, then the text. MLEADER adds a short landing. */
  | { type: "leader"; points: P[]; text: string }
  | { type: "mleader"; points: P[]; text: string }
  /** A revision cloud around the polygon `points` with the comment `text` beside it, on the
   *  layer COMMENTS: `added` with the cloud, then its label. `arc_len` null: from the outline. */
  | { type: "comment"; points: P[]; text: string; arc_len?: number | null }
  /** The comment clouds of every sheet: `comments`. */
  | { type: "list_comments" }
  /** A feature control frame (`{\Fgdt;p}` a symbol, `%%v` between fields) at `at`. */
  | { type: "tolerance"; at: P; frame: string }
  /** CANNOSCALE, "paper:drawing" such as "1:50". */
  | { type: "annotation_scale"; name: string }
  /** The style `name` (from the current one when new) takes `fields`; `current` makes it the one
   *  new dimensions take. */
  | { type: "set_dim_style"; name: string; fields: DimStyleFields; current: boolean }
  /** A table on the current layer, top left at `at`, cells filled from `data` (plain text): an
   *  `added` outcome, one undo step. The others change a table, one undo step each. */
  | {
      type: "table";
      at: P;
      rows: number;
      cols: number;
      col_width: number;
      row_height: number;
      data: string[][];
    }
  | { type: "edit_cell"; target: EntityTarget; row: number; col: number; text: string }
  | { type: "insert_row"; target: EntityTarget; at: number }
  | { type: "insert_col"; target: EntityTarget; at: number }
  | { type: "delete_row"; target: EntityTarget; row: number }
  | { type: "delete_col"; target: EntityTarget; col: number }
  /** One cell (`first` = `last`) unmerges the merged cell it is in. */
  | { type: "merge_cells"; target: EntityTarget; first: [number, number]; last: [number, number] }
  /** A one-line text holding the field: an `added` outcome. */
  | { type: "field"; at: P; height: number; field: FieldKind }
  /** Works every field out again: one undo step when one changes. */
  | { type: "update_fields" }
  /** A new block from `targets` (they leave the sheet, relative to `base`); with
   *  `convert_to_insert` a reference takes their place. An `edited` outcome. */
  | { type: "create_block"; name: string; base: P; targets: EntityTarget[]; convert_to_insert: boolean }
  /** A reference to `name` on the current layer, `rotation` in degrees; attributes by tag. With
   *  `from_path`, a block the drawing lacks is copied from that drawing first. */
  | {
      type: "insert_block";
      name: string;
      at: P;
      scale: number;
      rotation: number;
      attribute_values: Record<string, string>;
      from_path?: string | null;
    }
  /** Writes the block to a .dwg in the drawing's folder (the Save dialog uses `writeBlock`). */
  | { type: "write_block"; name: string; path: string }
  /** Edits the reference's block in place until saved or discarded: not an undo step. */
  | { type: "edit_block_in_place"; target: EntityTarget }
  /** End the edit in place, the block taking what was edited or not: a `block_edit_closed`
   *  outcome, no undo step (the edit's steps leave the history). */
  | { type: "save_block_edits" }
  | { type: "discard_block_edits" }
  | { type: "att_def"; tag: string; prompt: string; default: string; at: P; height: number }
  | { type: "edit_attributes"; target: EntityTarget; values: Record<string, string> }
  | { type: "edit_att_def"; block: string; tag: string; prompt: string; default: string }
  /** Every reference gets the block's attributes as defined now, keeping each tag's value. */
  | { type: "sync_attributes"; block: string }
  | { type: "redefine_block"; name: string; from_path: string }
  /** Layer states, kept in the drawing as AutoCAD keeps them (the hidden layers beside them). */
  | { type: "save_layer_state"; name: string }
  | { type: "restore_layer_state"; name: string }
  | { type: "delete_layer_state"; name: string }
  | { type: "rename_layer_state"; from: string; to: string }
  | { type: "set_layer"; name: string; prop: LayerProp; value: boolean | string | number }
  | { type: "new_layer"; name: string }
  | { type: "delete_layer"; name: string }
  | { type: "rename_layer"; from: string; to: string }
  | { type: "translate_layers"; from_path: string | null; mappings: LayerMapping[] }
  | { type: "set_viewport_frozen"; viewport: EntityTarget; layer: string; frozen: boolean }
  /** An empty name: the app asks for one (GROUP). */
  | { type: "create_group"; name: string; targets: EntityTarget[] }
  | { type: "add_to_group"; name: string; targets: EntityTarget[] }
  | { type: "remove_from_group"; name: string; targets: EntityTarget[] }
  | { type: "delete_group"; name: string }
  | { type: "ungroup"; targets: EntityTarget[] }
  | { type: "match_properties"; source: EntityTarget; targets: EntityTarget[]; props: MatchProp[] }
  | { type: "set_property"; target: EntityTarget; prop: EntityProp; value: boolean | string | number | P }
  /** The clipboard: the entities as Qiyas's clipboard text (base null: their lower left), and
   *  a paste of the system clipboard at `at` (as one block reference with `as_block`). */
  | { type: "copy_to_clipboard"; targets: EntityTarget[]; base: P | null }
  | { type: "paste_from_clipboard"; at: P; as_block: boolean; height?: number | null }
  /** Paper layouts and a title block containing only supplied values. */
  | { type: "new_layout"; name: string; paper: string; orientation?: "portrait" | "landscape" }
  | { type: "set_layout_paper"; name: string; paper: string; orientation?: "portrait" | "landscape" }
  | { type: "rename_layout"; from: string; to: string }
  | { type: "title_block"; fields: Record<string, string>; margin_mm?: number; binding_mm?: number; layer?: string }
  /** A viewport: ratio string or paper millimetres per model unit, centred at a model point. */
  | { type: "mview"; corner1?: P; corner2?: P; scale?: string | number | null; centre?: P | null; model_bounds?: [P, P] | null; paper_centre?: P | null; target?: EntityTarget | null }
  /** The user coordinate system (not an undo step): an origin and an x axis, or an entity's. */
  | { type: "ucs"; origin: P; x_axis: P }
  | { type: "ucs_object"; target: EntityTarget }
  /** PURGE: what nothing uses, of `kinds` (every kind when empty). A dry run only lists it; the
   *  real run takes it out as one undo step (refused while a block is edited in place). */
  | { type: "purge"; kinds: PurgeKind[]; dry_run: boolean }
  /** AUDIT: what points at something the drawing does not have; `fix` repairs what is safe to,
   *  as one undo step. */
  | { type: "audit"; fix: boolean }
  /** A geometric constraint on the parts of `targets` picked at `points` (one each); the first
   *  entity picked stays where it is when it can. */
  | { type: "add_constraint"; kind: ConstraintKind; targets: EntityTarget[]; points: P[] }
  | { type: "delete_constraint"; id: number }
  /** DELCONSTRAINT: every constraint holding one of `targets`. */
  | { type: "delete_constraints_on"; targets: EntityTarget[] }
  /** AUTOCONSTRAIN: what is already true of `targets`, within the settings' tolerances. */
  | { type: "auto_constrain"; targets: EntityTarget[]; settings?: AutoSettings }
  /** A dimensional constraint (linear, aligned, angular, radius, diameter), its Dimension
   *  through `line_point`, its value `expression` (the measured value when null). */
  | {
      type: "add_dim_constraint";
      kind: ConstraintKind;
      targets: EntityTarget[];
      points: P[];
      line_point: P;
      expression?: string | null;
    }
  /** A parameter takes a number or a formula of others (`2*d2 + 1.5`); circles are refused. */
  | { type: "set_parameter"; name: string; expression: string }
  | { type: "list_constraints" }
  | { type: "list_parameters" };

/** What a constraint keeps (`qiyas_core::ConstraintKind`): the geometric kinds, then the
 *  dimensional ones. */
export const GEOMETRIC_KINDS = [
  "coincident",
  "collinear",
  "concentric",
  "fixed",
  "parallel",
  "perpendicular",
  "horizontal",
  "vertical",
  "tangent",
  "smooth",
  "symmetric",
  "equal",
] as const;
export const DIMENSIONAL_KINDS = ["linear", "aligned", "angular", "radius", "diameter"] as const;
export type ConstraintKind = (typeof GEOMETRIC_KINDS)[number] | (typeof DIMENSIONAL_KINDS)[number];

/** AUTOCONSTRAIN's kinds and tolerances (drawing units, degrees); the core's defaults when left out. */
export type AutoSettings = { kinds?: ConstraintKind[]; distance_tolerance?: number; angle_tolerance?: number };

/** A constraint on the sheet on screen: the entities it holds, where its first part is, and a
 *  dimensional one's parameter, expression, value (drawing units or degrees) and Dimension. */
/** A comment cloud: its cloud's and label's handles, the comment, its sheet (block name) and
 *  the cloud's bounds in drawing coordinates. */
export type CommentInfo = {
  id: string;
  handle: number;
  label: number | null;
  text: string;
  sheet: string;
  bounds: [P, P];
};

export type ConstraintInfo = {
  id: number;
  kind: ConstraintKind;
  handles: number[];
  at: P;
  name: string | null;
  expression: string | null;
  value: number | null;
  dimension: number | null;
};

/** A dimensional constraint's parameter; `value` null when its expression can't be worked out. */
export type ParameterInfo = {
  id: number;
  name: string;
  kind: ConstraintKind;
  expression: string;
  value: number | null;
  dimension: number;
  /** Why its expression can't be worked out (its constraint is left out of solves until it is
   *  set again or deleted); null when it can. */
  error?: string | null;
};

/** What a purge looks at (`qiyas_core::purge::PurgeKind`); `styles` are text styles. */
export const PURGE_KINDS = ["layers", "blocks", "styles", "linetypes", "dimstyles", "groups"] as const;
export type PurgeKind = (typeof PURGE_KINDS)[number];

/** What a purge took out (or, in a dry run, would take out), by kind, each sorted by name.
 *  `limited_reason`: why only layers were looked at (the drawing holds data Qiyas can't read). */
export type PurgeReport = { dry_run: boolean; group_members: number; total: number; limited_reason: string | null } & {
  [K in PurgeKind]: string[];
};

/** What an audit line is about (`qiyas_core::audit::AuditKind`); the last two are listed only. */
export type AuditKind =
  | "owner_gone"
  | "missing_block"
  | "dimension_block_gone"
  | "table_block_gone"
  | "group_member_gone"
  | "missing_layer"
  | "missing_text_style"
  | "empty_block"
  | "unused_layer";

/** One audit line: a kind and the name it is about, how many entities it touches, the first 100
 *  of them, a plain sentence (English, from the core) and whether the fix repaired it. */
export type AuditItem = { kind: AuditKind; name: string; count: number; handles: number[]; line: string; fixed: boolean };

/** What an audit found: counts by kind, how many the fix changed, one item per kind and name. */
export type AuditReport = { fix: boolean; counts: Partial<Record<AuditKind, number>>; repaired: number; items: AuditItem[] };

/** Where find and replace looks: the sheet on screen, or every sheet. */
export type TextScope = "sheet" | "drawing";

/** An entity holding a match (a block reference, for an attribute) and its sheet (block name). */
export type TextMatch = { handle: number; sheet: string };

export type Outcome =
  | { type: "recognition"; result: { candidates?: RecognitionCandidate[]; candidate?: RecognitionCandidate } }
  | { type: "measured"; measurement: Measurement }
  | { type: "removed"; id: string }
  | { type: "changed" }
  /** Entities added to the drawing (PDF import or underlay). */
  | { type: "added"; handles: number[] }
  | { type: "title_block"; handles: number[]; empty_fields: string[] }
  /** An explode's parts, how many entities picked had none and were left as they are, and the
   *  take-off rows that followed (as `takeoff_followed`). */
  | { type: "exploded"; handles: number[]; left_alone: number; remeasured: number; unlinked: number }
  /** An update of fields or attributes that left locked entities as they were: how many changed,
   *  how many locked ones were left. */
  | { type: "updated"; count: number; skipped: number }
  /** An erase (or a move, turn, scale... that left a row unmeasurable): take-off rows measured
   *  again without an erased entity, and rows that lost their link (their values stay). */
  | { type: "takeoff_followed"; remeasured: number; unlinked: number }
  /** An edit in place: entities it added and removed, how many picked it left as they were, how
   *  many take-off rows lost their link (their entities could no longer be measured), and for a
   *  join how many rows now measure the joined polyline. */
  | {
      type: "edited";
      added: number[];
      removed: number[];
      left_alone: number;
      unlinked: number;
      joined_rows: number;
      /** Parametric constraints dropped because the edit reshaped an entity they held. */
      constraints_removed?: number;
    }
  /** A find and replace: how many replacements, the entities changed, and how many texts were
   *  left as they were (a text can't be left blank or break onto two lines). */
  | { type: "replaced"; count: number; handles: number[]; skipped: number }
  /** A find: how many matches, and each entity holding one with its sheet. */
  | { type: "found"; count: number; matches: TextMatch[] }
  | { type: "nothing" }
  /** Facts to show: rows of a label key ("radius", "centre"…) and its value written out. */
  | { type: "info"; title: string; rows: [string, string][] }
  | { type: "table"; columns: string[]; rows: string[][] }
  /** An edit in place ended: rows that lost their link, undo steps that left with it. */
  | { type: "block_edit_closed"; name: string; saved: boolean; unlinked: number; dropped: number }
  /** A copy went onto the system clipboard: how many entities. */
  | { type: "clipboard"; text: string; count: number }
  | { type: "purged"; report: PurgeReport }
  | { type: "audited"; report: AuditReport }
  /** Constraints added (or removed), by id. */
  | { type: "constrained"; ids: number[] }
  | { type: "constraints"; constraints: ConstraintInfo[] }
  | { type: "parameters"; parameters: ParameterInfo[] }
  | { type: "comments"; comments: CommentInfo[] };

export const runCommand = (cmd: Command) => invoke<Outcome>("run_command", { cmd });

/** Registry words and aliases, in command order; each command's first word is its full name. */
export type CommandWord = { word: string; command: string; summary: string };
export const commandWords = () => invoke<CommandWord[]>("command_words");
export type CmdlineStep =
  | { type: "command"; command: Command; last: P | null }
  | { type: "prompt"; prompt: string; last: P | null }
  | { type: "cancelled" };
/** The core reads a line; the caller applies a completed command through the usual window path. */
export const cmdlineStep = (line: string, last?: P | null) =>
  invoke<CmdlineStep>("cmdline_step", { line, ...(last === undefined ? {} : { last }) });

/** The constraints of the sheet on screen. */
export async function listConstraints(): Promise<ConstraintInfo[]> {
  const o = await runCommand({ type: "list_constraints" });
  return o.type === "constraints" ? o.constraints : [];
}

/** The comment clouds of every sheet. */
export async function listComments(): Promise<CommentInfo[]> {
  const o = await runCommand({ type: "list_comments" });
  return o.type === "comments" ? o.comments : [];
}

/** Every parameter of the drawing, by name. */
export async function listParameters(): Promise<ParameterInfo[]> {
  const o = await runCommand({ type: "list_parameters" });
  return o.type === "parameters" ? o.parameters : [];
}

/** False when there was nothing to undo. */
export const undo = () => invoke<boolean>("undo");

/** False when there was nothing to redo. */
export const redo = () => invoke<boolean>("redo");

export const takeoff = () => invoke<Measurement[]>("takeoff");

export const totals = () => invoke<Total[]>("totals");

/** Saves the take-off to the drawing's `.qiyas` project file; resolves to the path written. */
export const saveProject = () => invoke<string>("save_project");

/** Saves the drawing over its own file with the measurements embedded, and its project file. */
export const saveDrawing = () => invoke<void>("save_drawing");

/** Saves the drawing to `path` (.dwg or .dxf); from then on the open drawing is that file. */
export const saveDrawingAs = (path: string) => invoke<void>("save_drawing_as", { path });

/** Writes the take-off and its totals to an Excel workbook at `path`. */
export const exportTakeoff = (path: string) => invoke<void>("export_takeoff", { path });

/** Writes a table (from `extract_data`) to an Excel workbook at `path`: one sheet "Data". */
export const exportTable = (path: string, columns: string[], rows: string[][]) =>
  invoke<void>("export_table", { path, columns, rows });

/** Reads the BOQ in the Excel workbook at `path` (English or Arabic headers) and makes it the
 *  drawing's BOQ, as an undo step. Rejects with "No BOQ header found. …" when no sheet has one. */
export const importBoq = (path: string) => invoke<Boq>("import_boq", { path });

/** The drawing's BOQ; null when none is imported. */
export const boq = () => invoke<Boq | null>("boq");

/** Compares the BOQ and the take-off with the schedules and block counts of every sheet and keeps
 *  the conflicts, as an undo step; resolves to them. `tolerance` is the allowed difference in
 *  percent for BOQ items with none of their own (5 when left out). Conflicts found before keep
 *  their status and note. */
export const compare = (tolerance?: number) =>
  invoke<Conflict[]>("compare", { toleranceDefault: tolerance ?? null });

/** The conflicts of the last comparison. */
export const conflicts = () => invoke<Conflict[]>("conflicts");

/** Sets a conflict's status and note, as an undo step. */
export const resolveConflict = (id: string, status: ConflictStatus, note: string) =>
  runCommand({ type: "resolve_conflict", id, status, note });

/** Writes the conflicts to an Excel workbook at `path` (sheets "Conflicts" and "Summary"). */
export const exportConflicts = (path: string) => invoke<void>("export_conflicts", { path });

/** Who answers the in-app assistant. */
export type Provider = "anthropic" | "openai_compatible";

export const EFFORTS = ["none", "low", "medium", "high", "xhigh", "max"] as const;
export type Effort = (typeof EFFORTS)[number];

/** A provider keeps its model and service address when another provider is chosen. */
export type ProviderProfile = { effort?: Effort | null; model: string; base_url?: string | null };

/** The app settings (`settings.json`). The API key is not here: it is in the OS keychain. */
export type Settings = {
  provider: Provider;
  providers: Record<Provider, ProviderProfile>;
  effort: Effort;
  /** The plan approval setting (per_job when absent in older settings). */
  autonomy?: "per_job" | "per_phase" | "autonomous";
  /** A local rule set name; null means geometric quantities. */
  measurement_method?: string | null;
  mcp_enabled: boolean;
  mcp_port: number;
  /** 32 hex characters, made once. */
  mcp_token: string;
  language: "en" | "ar";
  theme: "system" | "light" | "dark";
  /** The user's command line aliases, alias to command, over the built-in ones. */
  aliases: Record<string, string>;
  /** The tool palette; changed only with `setToolPalette`. */
  tool_palette?: PaletteItem[];
  /** Minutes between autosaves of drawings with unsaved changes; 0 off (5 when left out). */
  autosave_minutes?: number;
  /** Decimals of values shown where no drawing is open (2 to 4, 3); a drawing's own drafting
   *  precision wins while it is open. */
  display_precision?: number;
  /** Screen pixels around the cursor an object snap is looked for in (1 to 50, 10). */
  snap_aperture_px?: number;
  /** Screen pixels around the cursor a click picks in (1 to 50, 6). */
  pick_box_px?: number;
  /** "#rrggbb", or null for the theme's. */
  background_color?: string | null;
  crosshair_color?: string | null;
  highlight_color?: string | null;
  /** The user's keyboard shortcuts, command to key ("Ctrl+Shift+C", "F3", "D"), over the built-in
   *  ones; no two commands on one key. */
  shortcuts?: Record<string, string>;
  /** The user's ribbon; null for the default. */
  ribbon?: RibbonLayout | null;
};

/** How the window runs a ribbon button: picks a tool, runs an action, or a checkbox. */
export type RibbonButtonKind = "tool" | "action" | "toggle" | "command";
/** One ribbon button; `label` is an i18n key (or the user's own text). */
export type RibbonButton = { kind: RibbonButtonKind; command: string; label: string };
/** `main`: how many of the buttons the group shows; the rest wait behind its "⋯" button (all when left out). */
export type RibbonGroup = { id: string; label: string; buttons: RibbonButton[]; main?: number };
export type RibbonTab = { id: string; label: string; groups: RibbonGroup[] };
/** The ribbon as data (`qiyas_desktop::ribbon`): a saved one may only hold the default's buttons. */
export type RibbonLayout = { tabs: RibbonTab[] };

/** The ribbon Qiyas ships, for Customize's tree and its "Reset to default". */
export const getDefaultRibbon = () => invoke<RibbonLayout>("get_default_ribbon");

/** What one autosave round did: the `.sv$` files written, and why others could not be. */
export type AutosaveRun = { written: string[]; failed: string[] };

/** Autosaves every open drawing with unsaved changes (`<name>.sv$` beside it). */
export const autosaveDrawings = () => invoke<AutosaveRun>("autosave_drawings");

/** Who has a drawing open (its `<file>.qlock`): the process, its computer, and when it took the
 *  lock (seconds since 1970). */
export type DrawingLock = { pid: number; host: string; started_at: number };

/** An older autosave of a drawing (`recover_autosave` and `discard_autosave` take its path): one a
 *  crash left, kept aside as `<file>.sv$.crash-<utc>`, or an earlier Qiyas window's on this
 *  computer that has ended (`<file>.sv$.<pid>`). */
export type OlderCopy = { path: string; saved_at: number; kind: "crash" | "instance" };

/** What there is to say about a drawing before it opens: its autosave (`<file>.sv$` whatever its
 *  age; `offered` in `recoveryQueue.ts` decides), a crash, another Qiyas holding it, older
 *  autosaves. Times in seconds since 1970. */
export type Recoverable = {
  drawing: string;
  /** The drawing's `<file>.sv$`; null when there is none. */
  autosave: string | null;
  /** When the autosave was written; null for a lock alone. */
  saved_at: number | null;
  /** When the drawing was last written; null when it is gone. */
  drawing_at: number | null;
  /** A lock on this computer whose process has ended: Qiyas closed with the drawing open. */
  crashed: boolean;
  /** The drawing is open in another Qiyas (a running process's lock, or another computer's). */
  locked_elsewhere: DrawingLock | null;
  /** Older autosaves: crash copies and ended windows' own, newest first. */
  older_copies: OlderCopy[];
};

/** What there is to say about each of `paths`: an autosave or a lock other than this Qiyas's. */
export const recoverableDrawings = (paths: string[]) =>
  invoke<Recoverable[]>("recoverable_drawings", { paths });

/** Opens the autosave of the drawing `path` in a new tab, as a new unsaved drawing beside it. */
export const recoverAutosave = (path: string) => invoke<DrawingInfo>("recover_autosave", { path });

/** Deletes the autosave of the drawing `path`. */
export const discardAutosave = (path: string) => invoke<void>("discard_autosave", { path });

/** What the failsafe reader made of a file (`qiyas_core::ReadReport`). */
export type ReadReport = {
  records: number;
  decoded: number;
  skipped: number;
  entities: number;
  recovered_errors: number;
  dropped: { kind: string; count: number }[];
};

/** RECOVER: opens a damaged drawing in a new tab and says what its reader dropped. */
export const recoverDrawing = (path: string) =>
  invoke<{ info: DrawingInfo; report: ReadReport }>("recover_drawing", { path });

/** One line of a command script as run: its number (from 1), its text, and what it did or why it
 *  failed. */
export type ScriptLine = { line_no: number; text: string; ok: boolean; message: string };

/** What a script run did: whether it ran at all (`ran` false: the script didn't read, its bad
 *  line is the only one), the lines run (the failing one last), whether a drawing, its take-off or
 *  the tabs may have changed, and how long it took. */
export type ScriptResult = { ran: boolean; lines: ScriptLine[]; changed: boolean; took_ms: number };

/** SCRIPT: runs the `.scr` command script at `path` on the drawing on screen (its OPEN lines open
 *  tabs). Refused while a block is edited in place. */
export const runScript = (path: string) => invoke<ScriptResult>("run_script", { path });

/** Stops the running script before its next line (or during its DELAY). */
export const stopScript = () => invoke<void>("stop_script");

/** Sent with the line number before each script line runs. */
export const SCRIPT_LINE = "script:line";

/** A sheet set (`.qss`): drawings by path relative to it, and their layouts (none: all). */
export type SheetSet = { name: string; drawings: { path: string; layouts: string[] }[] };

/** A sheet set drawing as listed: its path on this computer, and whether the file is missing. */
export type SheetSetEntry = { path: string; layouts: string[]; full: string; missing: boolean };

export type SheetSetView = { set: SheetSet; entries: SheetSetEntry[] };

export const sheetSetOpen = (path: string) => invoke<SheetSetView>("sheet_set_open", { path });

export const sheetSetSave = (path: string, set: SheetSet) =>
  invoke<SheetSetView>("sheet_set_save", { path, set });

/** The sheet set drawing for `drawing` (inside the set's folder) with all its layouts. */
export const sheetSetEntry = (qss: string, drawing: string) =>
  invoke<SheetSet["drawings"][number]>("sheet_set_entry", { qss, drawing });

/** A new sheet set of the open drawings in the folder of `qss` (not written yet); `skipped` names
 *  those outside it. */
export const sheetSetFromTabs = (qss: string, name: string) =>
  invoke<{ set: SheetSet; skipped: string[] }>("sheet_set_from_tabs", { qss, name });

export const getSettings = () => invoke<Settings>("get_settings");

export type MeasurementMethod = {
  name: string;
  title: string;
  based_on: string;
  source_note: string;
  rules_loaded: boolean;
};
export const measurementMethods = () => invoke<MeasurementMethod[]>("measurement_methods");

/** One button of the tool palette: a block to insert or a command line to run. */
export type PaletteItem = { kind: "block" | "command"; label: string; value: string };

/** Keeps the tool palette in the settings file. */
export const setToolPalette = (items: PaletteItem[]) => invoke<void>("set_tool_palette", { items });

/** An attribute definition: its tag, the prompt asked on insert, its default value, and whether
 *  it is constant (never asked). */
export type AttDefInfo = { tag: string; prompt: string; default: string; constant: boolean };

/** A named block of the drawing, with its attribute definitions. */
export type BlockInfo = { name: string; attdefs: AttDefInfo[] };

export const blocks = () => invoke<BlockInfo[]>("blocks");

/** A PNG of the block, `px` wide. */
export const blockThumbnail = (name: string, px: number) =>
  invoke<ArrayBuffer>("block_thumbnail", { name, px });

/** The named blocks of another drawing, opened read-only, with their attribute definitions
 *  (the Design Center). */
export const drawingBlocks = (path: string) => invoke<BlockInfo[]>("drawing_blocks", { path });
/** Another drawing's layers, read-only (the layer translator's standard). */
export const drawingLayers = (path: string) => invoke<LayerInfo[]>("drawing_layers", { path });

/** WBLOCK to a .dwg the user chose in the Save dialog. */
export const writeBlock = (name: string, path: string) => invoke<void>("write_block", { name, path });

/** A paper size in millimetres, portrait (the catalog's short side first). */
export type Paper = { name: string; w_mm: number; h_mm: number };

/** What part of a sheet a plot takes: everything drawn, a layout's limits (else its extents; model
 *  space's extents), or two corners in sheet coordinates. */
export type PlotArea = "extents" | "layout" | { window: [P, P] };

/** Fit to the paper (5 mm margin), or `num` paper millimetres per `den` sheet millimetres (1:100
 *  is [1, 100]); model space is in the drawing's units, a layout's are taken as millimetres. */
export type PlotScale = "fit" | { ratio: [number, number] };

/** One pen of a CTB (`Color_1` to `Color_255`, by the drawn colour's ACI) or a named style of an
 *  STB (by the layer's plot style name). */
export type PlotStyleEntry = {
  name: string;
  localized_name: string;
  description: string;
  /** The pen's colour; null plots with the object's. */
  color: [number, number, number] | null;
  /** Bit 0 dither, bit 1 grayscale. */
  color_policy: number;
  physical_pen: number;
  virtual_pen: number;
  /** Screening, 0 to 100: 100 plots the colour as it is; 0 does not plot. */
  screen: number;
  linepattern_size: number;
  linetype: number;
  adaptive_linetype: boolean;
  /** Index into `PlotStyle.lineweights`; 0 plots the object's lineweight. */
  lineweight: number;
  fill_style: number;
  end_style: number;
  join_style: number;
  /** Keys Qiyas does not know, kept as written. */
  extra: [string, string][];
};

/** A CTB or STB plot style table as `load_plot_style` reads it. */
export type PlotStyle = {
  kind: "ctb" | "stb";
  description: string;
  scale_factor: number;
  apply_factor: boolean;
  /** How lineweights are shown: 0 millimetres, 1 inches. */
  display_units: number;
  styles: PlotStyleEntry[];
  /** The lineweights in millimetres the pens choose from (index 0, 0.00, is the object's). */
  lineweights: number[];
  extra: [string, string][];
};

/** One plot: which sheet (its block, e.g. `*Model_Space`), area, paper, scale and plot style,
 *  and the PDF: a name or path inside the drawing's folder, or null for `<drawing>-<sheet>.pdf`
 *  beside it. */
export type PlotJob = {
  sheet: string;
  area: PlotArea;
  paper: Paper;
  orientation: "portrait" | "landscape";
  scale: PlotScale;
  centre: boolean;
  style: PlotStyle | null;
  lineweights: boolean;
  output: string | null;
};

/** A plot written, and what was left out of it (pictures not found, ...). */
export type PlotOutput = { path: string; skipped: string[] };

/** The built-in paper sizes, portrait: ISO A0 to A4, ANSI A to E, ARCH A to E1. */
export const paperCatalog = () => invoke<Paper[]>("paper_catalog");
/** Plots `job` to `path`, the PDF the user chose in the Save dialog (anywhere). */
export const plotPdfTo = (job: PlotJob, path: string) => invoke<PlotOutput>("plot_pdf_to", { job, path });
/** Plots `job` to a PDF in the temporary folder and prints it on `printer` (null: on Linux the
 *  PDF viewer opens, elsewhere the default printer); what happened, in English. */
export const plotAndPrint = (job: PlotJob, printer: string | null) =>
  invoke<string>("plot_and_print", { job, printer });
/** The printers this computer knows, and the default one. */
export const listPrinters = () => invoke<string[]>("list_printers");
export const defaultPrinter = () => invoke<string | null>("default_printer");
/** Reads a CTB or STB plot style file. */
export const loadPlotStyle = (path: string) => invoke<PlotStyle>("load_plot_style", { path });
/** Writes a plot style: a CTB to a `.ctb` file, an STB to `.stb`. */
export const savePlotStyle = (path: string, style: PlotStyle) =>
  invoke<void>("save_plot_style", { path, style });

/** Rejects a port outside 1024–65535 or an unknown effort. */
export const setSettings = (settings: Settings) => invoke<void>("set_settings", { settings });

/** The user's command line aliases (alias to command). */
export const aliases = () => invoke<Record<string, string>>("aliases");

/** Stores the key in the OS keychain; it can't be read back from the window. */
export const setApiKey = (provider: Provider, key: string) =>
  invoke<void>("set_api_key", { provider, key });

export const hasApiKey = (provider: Provider) => invoke<boolean>("has_api_key", { provider });

export const deleteApiKey = (provider: Provider) => invoke<void>("delete_api_key", { provider });

/** The running app's folder, where the `qiyas-mcp` bridge sits. */
export const appDir = () => invoke<string>("app_dir");

/** The MCP server for outside AI apps: on, listening, or why it could not start. */
export type McpStatus = {
  enabled: boolean;
  listening: boolean;
  /** "127.0.0.1:<port>" while listening. */
  addr: string | null;
  error: string | null;
};

export const mcpStatus = () => invoke<McpStatus>("mcp_status");

/** What "Check for updates" found. */
export type UpdateCheck =
  | { status: "up_to_date" }
  | { status: "available"; version: string; notes: string | null };

/** Refusals of `checkUpdate` and `installUpdate` the About box says in its own words; other
 *  errors are the updater's own text. Not configured: this build's public key or feed address
 *  is still the placeholder. */
export const UPDATES_NOT_CONFIGURED = "updates_not_configured";
export const UNSAVED_DRAWINGS = "unsaved_drawings";
export const NO_UPDATE = "no_update";
/** Another install runs (the launch dialog's). */
export const UPDATE_BUSY = "update_busy";
/** The download was stopped (`cancelUpdate`). */
export const UPDATE_CANCELLED = "update_cancelled";
/** Installed, but a drawing got unsaved changes meanwhile: Qiyas didn't restart. */
export const RESTART_LATER = "installed_restart_later";

/** Asks the update feed whether a newer Qiyas is out. */
export const checkUpdate = () => invoke<UpdateCheck>("check_update");

/** Downloads the update the last check found, checks its signature, installs it and restarts
 *  Qiyas; it only comes back, rejected, when that failed. */
export const installUpdate = () => invoke<void>("install_update");

/** Stops the download of the running install; nothing once the install itself started. */
export const cancelUpdate = () => invoke<void>("cancel_update");

/** Sent during the download: how much has come, of how much when the server says. */
export const UPDATE_PROGRESS = "update:progress";
export type UpdateProgress = { downloaded: number; total: number | null };
/** Sent when the download is done and checked and the install starts: it can't be stopped now. */
export const UPDATE_INSTALLING = "update:installing";

/** Sent when the OS asked Qiyas to open files (a double click on a drawing, or a second launch
 *  with one); `takeOpenRequests` gives them. */
export const OPEN_REQUESTED = "open:requested";

/** The drawings the OS asked to open, oldest first; the queue is empty after. */
export const takeOpenRequests = () => invoke<string[]>("take_open_requests");

/** Sent when Mizan (or an MCP client) put another drawing on screen, or closed the last one. */
export const DRAWING_SHOWN = "drawing:shown";
/** Sent when Mizan asked to save a drawing the window asks about first (untitled, or repaired on
 *  open): the window runs its Save. */
export const DRAWING_SAVE = "drawing:save";
/** Sent with a tab id when Mizan asked to close a drawing with unsaved changes: the window asks. */
export const DRAWING_CLOSE = "drawing:close";

/** Sent when an outside AI app changed the take-off through the MCP server. */
export const TAKEOFF_CHANGED = "takeoff:changed";
/** Sent (with the tool result's `ui`) after an MCP tool showed another sheet, hid or showed layers,
 *  or asked for a zoom. */
export const SESSION_CHANGED = "session:changed";

/** What the window should do after a tool, from the tool's result. Bounds in drawing coordinates. */
export type AiUi = {
  /** "reveal", "zoom", "show_sheet", "show_markup" or "redraw". */
  action: string;
  bounds?: [[number, number], [number, number]];
  handles?: number[];
  sheet?: string;
  /** After `redraw` from show_layers: the layers hidden now. */
  hidden?: string[];
  id?: string;
};

export type PlanPhase = {
  id: string;
  title: string;
  status: "pending" | "active" | "done" | "skipped";
};

/** One event of the assistant's turn (`qiyas_ai::AgentEvent`), sent as `ai:event`. */
export type Activity = { category: string; effect: string };

export type AgentEvent =
  | { type: "retrying"; seconds: number; code: string; discard_partial: boolean }
  | { type: "text"; text: string }
  | { type: "thinking"; text: string }
  | { type: "plan"; phases: PlanPhase[] }
  | { type: "awaiting_approval"; plan: { phases: PlanPhase[] } }
  | { type: "tool_call"; id: string; name: string; label?: string | null; input: unknown; mutating: boolean; activity?: Activity | null }
  | {
      type: "tool_result";
      id: string;
      summary: string;
      count?: number | null;
      handles: number[];
      ui: AiUi | null;
      is_error: boolean;
    }
  | { type: "refused"; category: string | null; text: string }
  | { type: "stopped"; reason: string }
  | { type: "done"; usage: Record<string, number> };

/** Sent as `ai:done` when a turn has ended. */
export type AiDone = { error: string | null };

export type AiStatus = {
  /** "anthropic", "openai_compatible", or "mock" in a debug build with QIYAS_AI_MOCK=1. */
  provider: string;
  model: string;
  has_key: boolean;
  busy: boolean;
  message_count: number;
};

/** A region the user marked for Mizan: a box (two opposite corners) or a polygon, drawing
 *  coordinates (`qiyas_ai::Region`). */
export type AiRegion = { kind: "box" | "polygon"; points: P[] };

/** What the user points Mizan at with a message (`qiyas_ai::TurnContext`): the entities selected,
 *  a marked region, the view on screen; `picture` when the selection or region is new since the
 *  last message, so a picture of it goes too. */
export type AiContext = {
  selection: EntityTarget[];
  region: AiRegion | null;
  view: [P, P] | null;
  picture: boolean;
};

/** A picture the user attached to a message: its type and base64 data (`qiyas_ai::UserImage`). */
export type AiImage = { media_type: string; data: string };

/** Starts a turn and returns at once; events follow as `ai:event`, then `ai:done`. Rejects with
 *  "busy", "no_api_key" or "no_base_url". */
export const aiSend = (text: string, context?: AiContext, images?: AiImage[], pointed?: string[]) =>
  invoke<void>("ai_send", { text, context: context ?? null, images: images ?? null, pointed: pointed ?? null });

export type TranscriptEntry =
  | { type: "user"; text: string; pointed: string[]; images: string[] }
  | { type: "event"; event: AgentEvent };

export type ConversationSummary = {
  id: string;
  title: string;
  drawing: string | null;
  drawing_exists: boolean;
  /** Milliseconds since 1970. */
  updated: number;
  turns: number;
};

export const aiConversations = (drawing?: string) =>
  invoke<ConversationSummary[]>("ai_conversations", { drawing: drawing ?? null });
export const aiResume = (id: string) =>
  invoke<{ transcript: TranscriptEntry[]; drawing: string | null }>("ai_resume", { id });
export const aiDeleteConversation = (id: string) => invoke<void>("ai_delete_conversation", { id });
export const aiDeleteConversations = () => invoke<void>("ai_delete_conversations");

/** Mizan's `view_screen` asks for a picture of the main view: `{ id, maxSide }`, answered with
 *  `{ png, bounds }` (a base64 PNG and the drawing area it shows). */
export const AI_CAPTURE = "ai:capture";
/** Mizan's `get_selection` (and `view_screen`) ask what the user points at: `{ id }`, answered
 *  with an `AiContext`. */
export const AI_POINTED = "ai:pointed";
/** Mizan's `select_entities` asks the window to select: `{ handles, zoom }`. */
export const AI_SELECT = "ai:select";

/** The window's answer to the core's question `id` (`ai:capture`, `ai:pointed`), or why it has
 *  none. */
export const aiWindowReply = (id: number, answer: { value: unknown } | { error: string }) =>
  invoke<void>("ai_window_reply", {
    id,
    value: "value" in answer ? answer.value : null,
    error: "error" in answer ? answer.error : null,
  });

export const aiCancel = () => invoke<void>("ai_cancel");

/** Starts a new conversation; `ai:cleared` follows. */
export const aiClear = () => invoke<void>("ai_clear");

export const aiStatus = () => invoke<AiStatus>("ai_status");

/** One model a provider offers: its id, as the API takes it, and a name to show. */
export type ModelInfo = { id: string; label: string };
/** The models a provider offers (`qiyas_ai::models::ModelList`): from the service, or the
 *  built-in list with why (`error`; "no_base_url" when there is no address to ask). */
export type ModelList = { models: ModelInfo[]; from_service: boolean; error: string | null };

/** The models `provider` offers at `base_url` (the saved ones when left out); the key stays in
 *  the core. */
export const aiModels = (provider?: Provider, baseUrl?: string | null, typedKey?: string) =>
  invoke<ModelList>("ai_models", { provider: provider ?? null, baseUrl: baseUrl ?? null, typedKey: typedKey || null });

/** The agent log: where the file is and its newest part (empty before the first conversation). */
export type AiLog = { path: string; text: string };

export const aiLog = () => invoke<AiLog>("ai_log");

/** Undoes the assistant's newest change, only while it is still the newest change to the
 *  take-off; false when it is not (the user changed the take-off since). */
export const aiUndo = () => invoke<boolean>("ai_undo");

/** The assistant answers in this language; a change starts a new conversation. */
export const aiSetLanguage = (lang: "en" | "ar") => invoke<void>("ai_set_language", { lang });

/** The clipboard's text for a paste into a text field, or null (`files::clipboard_read_text`). */
export const clipboardReadText = () => invoke<string | null>("clipboard_read_text");
/** The clipboard's picture: width, height (u32 little-endian), RGBA; empty when none. */
export const clipboardReadImage = () => invoke<ArrayBuffer>("clipboard_read_image");

export type PdfAttachment = { name: string; path: string; pages: number };
export const aiAttachPdf = (name: string, bytes: number[]) => invoke<PdfAttachment>("ai_attach_pdf", { name, bytes });

export type MeasurementCitation = { source: string; clause: string };
export type DimensionLine = {
  label: string;
  operation: "add" | "deduct";
  timesing: number;
  factors: number[];
  expression: string;
  value: number;
  unit: string;
  method: string;
  included: boolean;
  decision: string | null;
  citations: MeasurementCitation[];
  sources: { handle: number; sheet: string; grid: string | null }[];
};
export type GeneratedBoqItem = {
  confidence?: number;
  code: string;
  description_en: string;
  description_ar: string;
  section: string;
  trade: string;
  unit: string;
  quantity: number;
  lines: DimensionLine[];
};
export type MeasurementReport = {
  confidence_summary?: { low: number; medium: number; high: number };
  items: GeneratedBoqItem[];
  warnings: string[];
};
export const measurementReport = () => invoke<MeasurementReport>("measurement_report");

export type ProjectReply = {
  type: "project";
  result: {
    path?: string;
    projects?: { path: string; name: string; drawings?: number; checked?: boolean; error?: string }[];
    entries?: { path: string; full: string; missing: boolean }[];
    set?: { name: string; project: {
      method: { title: string } | null;
      boq: { item: GeneratedBoqItem }[]; queries: unknown[]; warnings: string[]; checked: boolean;
      register: { drawing: string; sheet: string; flags: string[] }[];
      drawings: { drawing: string; status: string; items: number; warnings: string[]; error: string | null }[];
    } };
  };
  outputs?: { workbook: string; morning_report: string; failed: number; checked: boolean };
};
export const projectCommand = (command: Record<string, unknown>) => invoke<ProjectReply>("project_command", { command: { ...command, type: "project" } });
export const projectHistory = (redo: boolean) => invoke<boolean>("project_history", { redo });

export type Explanation = {
  item: string; quantity: number; unit: string; confidence: number;
  confidence_band: "low" | "medium" | "high";
  dimension_lines: DimensionLine[];
  drawings: string[];
  evidence: { handle: number; sheet: string; drawing: string }[];
  elements?: unknown[]; recognition?: unknown[]; recognition_rules?: unknown[]; measurement_rules?: unknown[];
  contributions?: { explanation: Explanation }[];
  notes?: string[];
};
export const explainItem = (item: string, project?: string) => invoke<Explanation>("explain_item", { item, project });
