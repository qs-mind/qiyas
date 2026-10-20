//! The command bus: every change to the take-off, the BOQ, the conflicts or the drawing goes
//! through `Session::run` (the user) or `Session::run_as` (the AI, or an outside AI through MCP).

use std::collections::BTreeMap;

use opencadcodec::objects::ObjectType;
use opencadcodec::tables::normalize_name;
use opencadcodec::{EntityType, Handle};
use qiyas_geom::{object_to_world, Path};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::drawing::short_layer_name;
use crate::inquiry::{Field, Spacing};
use crate::session::{effective_block_name, locate, variant_name, xref_of_chain, Located};
use crate::takeoff::in_metres;
use crate::text::Finder;
use crate::{
    AttachMode, Boq, Conflict, ConflictStatus, CoreError, Current, DimEditOp, DimSpec,
    DimStyleFields, Drafting, DrawSpec, EditSpec, EntityFilter, EntityProp, FieldKind, Kind,
    LayerMapping, LayerProp, MatchProp, Measurement, ModifySpec, PreparedPdf, Schedule, Session,
    SnapKind, Source, TextMatch, TextScope,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Length along `points`, in drawing coordinates. Needs 2 points or more.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "measure_distance", "points": [[0, 0], [5, 0], [5, 3]]}))]
    MeasureDistance { points: Vec<[f64; 2]> },
    /// Area inside the ring `points` (closed for you). Needs 3 points or more.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "measure_area", "points": [[0, 0], [5, 0], [5, 4], [0, 4]]}))]
    MeasureArea { points: Vec<[f64; 2]> },
    /// Area of a closed entity, length of an open one, or 1 piece for a block reference.
    /// `chain` lists the block references the entity sits inside, outermost first (empty, the
    /// default, for an entity on the sheet); it is measured where they place it. A chain ending in
    /// an xref reference names an entity at the top of that xref (see `Picked`).
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "measure_entity", "handle": 1234}))]
    MeasureEntity {
        handle: u64,
        #[serde(default)]
        chain: Vec<u64>,
    },
    /// Block references of `block_name` on the current sheet, on visible layers only
    /// (and only on `layer` when given), and those at the top of the sheet's xrefs.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "count_block", "block_name": "DOOR-900", "layer": null}))]
    CountBlock {
        block_name: String,
        layer: Option<String>,
    },
    /// Removes the take-off row `id` (its link to the BOQ goes with it).
    #[schemars(extend("x-category" = "takeoff", "x-destructive" = true))]
    #[schemars(example = json!({"type": "remove_measurement", "id": "m1"}))]
    RemoveMeasurement { id: String },
    /// Sets the take-off row `id`'s description, as the BOQ export shows it.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "set_description", "id": "m1", "text": "Ground floor walls"}))]
    SetDescription { id: String, text: String },
    /// Hides or shows the layer `name` on screen only (the drawing's own on, off and freeze stay as
    /// they are; `SetLayer` changes those). View state: not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "set_layer_hidden", "name": "A-FURN", "hidden": true}))]
    SetLayerHidden { name: String, hidden: bool },
    /// Shows only the layer `name`, hiding every other (LAYISO); the layer isolated already
    /// un-isolates. Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "isolate_layer", "name": "A-WALL"}))]
    IsolateLayer { name: String },
    /// Puts back what isolating a layer hid (LAYUNISO). Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "unisolate_layers"}))]
    UnisolateLayers,
    /// Shows the sheet whose block is `block`: `*Model_Space`, or a layout's `*Paper_Space…` block
    /// (`DrawingInfo::sheets`). View state: not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "set_sheet", "block": "*Model_Space"}))]
    SetSheet { block: String },
    /// Says whether the drawing holds its Arabic text backwards (`Session::arabic_reversed`).
    /// Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "set_arabic_reversed", "on": true}))]
    SetArabicReversed { on: bool },
    /// Replaces the drafting settings (`Session::drafting`). Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "set_drafting", "drafting": {"osnap": true, "ortho": true}}))]
    SetDrafting { drafting: Drafting },
    /// A measurement of any kind, described by the caller (usually the AI). Geometry is `points`
    /// (drawing coordinates) or `handles` (measured from the entities' flattened paths, summed),
    /// not both. A length, area or count is worked out from the geometry, so `value` must be
    /// `None`; a volume is an area from the geometry times `height` (drawing units, above 0); an
    /// item needs `value` and `unit` and may have no geometry.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "add_measurement", "kind": "area", "points": [], "handles": [1234], "description": "Living room floor", "note": ""}))]
    #[schemars(example = json!({"type": "add_measurement", "kind": "item", "value": 1, "unit": "lump sum", "points": [], "handles": [], "description": "Site mobilisation", "note": ""}))]
    AddMeasurement {
        kind: Kind,
        value: Option<f64>,
        unit: Option<String>,
        height: Option<f64>,
        points: Vec<[f64; 2]>,
        handles: Vec<u64>,
        /// The block references every handle sits inside, outermost first; empty (the default)
        /// for entities on the sheet.
        #[serde(default)]
        chain: Vec<u64>,
        description: String,
        note: String,
    },
    /// Turns an area into a volume of that area times `height` (drawing units, above 0), keeping
    /// its geometry; on a volume, changes its height.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "set_height", "id": "m1", "height": 3.0}))]
    SetHeight { id: String, height: f64 },
    /// Links the measurement to the BOQ item `code`; `None` unlinks it.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "link_boq", "id": "m1", "code": "03.01"}))]
    LinkBoq { id: String, code: Option<String> },
    /// Sets the take-off row `id`'s note (the engineer's remark beside the quantity).
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "set_note", "id": "m1", "text": "Checked against section A-A"}))]
    SetNote { id: String, text: String },
    /// Replaces the BOQ with `boq`, usually one just read by `qiyas_boq::import_boq`.
    #[schemars(extend("x-category" = "boq", "x-internal" = true, "x-destructive" = true))]
    #[schemars(example = json!({"type": "set_boq", "boq": {"source": "boq.xlsx", "imported": "2026-10-20", "items": [{"code": "03.01", "description": "Blockwork 200 mm", "unit": "m2", "quantity": 120.5, "row": 5}]}}))]
    SetBoq { boq: Boq },
    /// Sets the allowed difference, in percent (0 or more), of every BOQ item with `code`;
    /// `None` goes back to the user's setting.
    #[schemars(extend("x-category" = "boq"))]
    #[schemars(example = json!({"type": "set_boq_tolerance", "code": "03.01", "percent": 5.0}))]
    SetBoqTolerance { code: String, percent: Option<f64> },
    /// Replaces the conflicts with those of a new comparison (`qiyas_takeoff::compare`), and the
    /// schedules with the ones it read off the sheets.
    #[schemars(extend("x-category" = "boq", "x-internal" = true, "x-destructive" = true))]
    #[schemars(example = json!({"type": "set_conflicts", "conflicts": [], "schedules": []}))]
    SetConflicts {
        conflicts: Vec<Conflict>,
        schedules: Vec<Schedule>,
    },
    /// Sets the status and note of the conflict `id`.
    #[schemars(extend("x-category" = "boq", "x-destructive" = true))]
    #[schemars(example = json!({"type": "resolve_conflict", "id": "c1", "status": "resolved", "note": "The BOQ includes 5% waste"}))]
    ResolveConflict {
        id: String,
        status: ConflictStatus,
        note: String,
    },
    /// Adds page `page` (from 1) of the PDF at `path` to the sheet on screen as lines, polylines
    /// and texts (`qiyas_pdf::extract_vectors`), `scale` drawing units per PDF point, the page's
    /// bottom-left corner at `insert_at`. One undo step; `Outcome::Added` lists the new entities.
    #[schemars(extend("x-category" = "attach", "x-needs" = "pdf"))]
    #[schemars(example = json!({"type": "import_pdf", "path": "plan.pdf", "page": 1, "insert_at": [0, 0], "scale": 0.3528}))]
    ImportPdf {
        path: String,
        page: u32,
        insert_at: [f64; 2],
        scale: f64,
    },
    /// Attaches page `page` of the PDF at `path` to the sheet on screen as a PDF underlay
    /// (an `Underlay` entity and its `UnderlayDefinition`, as AutoCAD stores them), `scale`
    /// drawing units per PDF point, bottom-left corner at `insert_at`. `Outcome::Added` holds the
    /// underlay's handle.
    #[schemars(extend("x-category" = "attach", "x-needs" = "pdf"))]
    #[schemars(example = json!({"type": "attach_pdf_underlay", "path": "plan.pdf", "page": 1, "insert_at": [0, 0], "scale": 0.3528}))]
    AttachPdfUnderlay {
        path: String,
        page: u32,
        insert_at: [f64; 2],
        scale: f64,
    },
    /// Rescales the underlay or image `handle` about `p1` so that `p1` to `p2` (drawing
    /// coordinates) becomes `real_distance` drawing units long.
    #[schemars(extend("x-category" = "attach"))]
    #[schemars(example = json!({"type": "calibrate", "handle": 1234, "p1": [0, 0], "p2": [10, 0], "real_distance": 5000}))]
    Calibrate {
        handle: u64,
        p1: [f64; 2],
        p2: [f64; 2],
        real_distance: f64,
    },
    /// Attaches the raster image at `path` (png, jpg, tif, bmp, webp) to the sheet on screen as
    /// an AutoCAD image (a `RasterImage` and its `ImageDefinition`) on layer `IMAGE-UNDERLAY`,
    /// bottom-left corner at `insert_at`, `width_units` drawing units wide. `Outcome::Added` holds
    /// the image's handle.
    #[schemars(extend("x-category" = "attach"))]
    #[schemars(example = json!({"type": "attach_image", "path": "site.jpg", "insert_at": [0, 0], "width_units": 20}))]
    AttachImage {
        path: String,
        insert_at: [f64; 2],
        width_units: f64,
    },
    /// Sets the brightness, contrast and fade (0 to 100 each; 50, 50 and 0 show the image as it
    /// is) of the image `handle`.
    #[schemars(extend("x-category" = "attach"))]
    #[schemars(example = json!({"type": "set_image_display", "handle": 1234, "brightness": 50, "contrast": 50, "fade": 30}))]
    SetImageDisplay {
        handle: u64,
        brightness: u8,
        contrast: u8,
        fade: u8,
    },
    /// The geometry of one entity as placed in the world (radius, diameter, centre and angles of
    /// an arc, circle or ellipse; length, angle and deltas of a line; a polyline's segment nearest
    /// `at`; a block reference's insertion, scale and rotation; a text's content, height and
    /// style), in metres and degrees. `Outcome::Info`; not an undo step.
    #[schemars(extend("x-category" = "inquiry"))]
    #[schemars(example = json!({"type": "measure_geometry", "handle": 1234}))]
    MeasureGeometry {
        handle: u64,
        #[serde(default)]
        chain: Vec<u64>,
        #[serde(default)]
        at: Option<[f64; 2]>,
    },
    /// Divides the entity's path into `count` equal parts (2 to 1000) with point entities on
    /// layer `QIYAS-POINT`, and counts them in the take-off: one undo step.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "divide_along", "handle": 1234, "count": 5}))]
    DivideAlong {
        handle: u64,
        #[serde(default)]
        chain: Vec<u64>,
        count: usize,
    },
    /// Places a point entity every `step` drawing units along the entity's path from its start,
    /// on layer `QIYAS-POINT`, and counts them in the take-off: one undo step.
    #[schemars(extend("x-category" = "takeoff"))]
    #[schemars(example = json!({"type": "measure_along", "handle": 1234, "step": 0.6}))]
    MeasureAlong {
        handle: u64,
        #[serde(default)]
        chain: Vec<u64>,
        step: f64,
    },
    /// Where the point `p` (drawing coordinates) is, in metres, and the snap `kind` that found it.
    /// `Outcome::Info`; not an undo step.
    #[schemars(extend("x-category" = "inquiry"))]
    #[schemars(example = json!({"type": "id_point", "p": [12.5, 3.0]}))]
    IdPoint {
        p: [f64; 2],
        #[serde(default)]
        kind: Option<SnapKind>,
    },
    /// One row of `fields` per entity `filter` finds (up to 5,000; `limit` says how many).
    /// `Outcome::Table`; not an undo step.
    #[schemars(extend("x-category" = "inquiry"))]
    #[schemars(example = json!({"type": "extract_data", "filter": {"block_name": "DOOR-900"}, "fields": ["handle", "layer", "x", "y", {"attribute": "MARK"}]}))]
    ExtractData {
        filter: EntityFilter,
        fields: Vec<Field>,
    },
    /// Writes the named view `name` (replacing one of that name) showing `bounds`,
    /// `[[min_x, min_y], [max_x, max_y]]` in drawing coordinates, to the drawing's view table.
    /// Model sheet only.
    #[schemars(extend("x-category" = "view"))]
    #[schemars(example = json!({"type": "save_view", "name": "Kitchen", "bounds": [[0, 0], [10, 8]]}))]
    SaveView { name: String, bounds: [[f64; 2]; 2] },
    /// Removes the named view `name`; `Outcome::Nothing` when there is none.
    #[schemars(extend("x-category" = "view", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_view", "name": "Kitchen"}))]
    DeleteView { name: String },
    /// Deletes the entities `targets` from the sheet on screen: one undo step. Each must be an
    /// entity of the drawing itself at the top of that sheet (an empty chain: not inside a block,
    /// not an xref's); otherwise nothing is deleted.
    #[schemars(extend("x-category" = "modify", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_entities", "targets": [{"handle": 1234}, {"handle": 1235}]}))]
    DeleteEntities { targets: Vec<Target> },
    /// Adds what the spec describes to the sheet on screen, on the current layer with the current
    /// colour, linetype and lineweight (`Session::current`): one undo step. `Outcome::Added` lists
    /// the new entities.
    #[schemars(extend("x-category" = "draw"))]
    #[schemars(example = json!({"type": "draw", "kind": "line", "a": [0, 0], "b": [5, 0]}))]
    Draw(DrawSpec),
    /// Commands run in order as ONE undo step, all or nothing: when one fails, everything the
    /// batch did is put back and the error names the command (from 0). Any command but those
    /// that can't run in a batch (editing a block in place, writing a block, the clipboard,
    /// another batch). With `layer` the commands draw on that layer (made when missing) and with
    /// `color` in that colour ("ByLayer", "ACI n", "#rrggbb"); the current state is put back
    /// after, and undo puts back a current state the commands changed. `Outcome::Batch`: each
    /// command's outcome and every entity added.
    #[schemars(extend("x-category" = "draw", "x-batch" = false))]
    #[schemars(example = json!({"type": "batch", "commands": [{"type": "new_layer", "name": "A-WALL"}, {"type": "set_current", "current": {"layer": "A-WALL"}}, {"type": "draw", "kind": "rectangle", "a": [0, 0], "b": [5, 4]}]}))]
    #[schemars(example = json!({"type": "batch", "layer": "A-DOOR", "commands": [{"type": "draw", "kind": "arc", "centre": [0, 0], "radius": 0.9, "start": 0, "end": 90}, {"type": "draw", "kind": "line", "a": [0, 0], "b": [0.9, 0]}]}))]
    Batch {
        commands: Vec<Command>,
        #[serde(default)]
        layer: Option<String>,
        #[serde(default)]
        color: Option<String>,
    },
    /// Sets the drawing state new entities take (`Session::current`). Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "set_current", "current": {"layer": "A-WALL", "color": "ByLayer", "linetype": "ByLayer", "lineweight": -1}}))]
    SetCurrent { current: Current },
    /// Moves, copies, turns, scales, mirrors, stretches, aligns, arrays or explodes entities at
    /// the top of the sheet on screen (see `ModifySpec`): one undo step.
    #[schemars(extend("x-category" = "modify"))]
    #[schemars(example = json!({"type": "modify", "kind": "move", "targets": [{"handle": 1234}], "delta": [2, 0]}))]
    Modify(ModifySpec),
    /// Trims, extends, breaks, joins, lengthens, reverses, offsets, fillets, chamfers, blends,
    /// edits polylines, removes duplicates or flattens entities at the top of the sheet on screen
    /// (see `EditSpec`): one undo step.
    #[schemars(extend("x-category" = "modify"))]
    #[schemars(example = json!({"type": "edit", "kind": "offset", "target": {"handle": 1234}, "distance": 0.2, "side_point": [1, 1]}))]
    Edit(EditSpec),
    /// One line of text at `at` on the sheet on screen, `height` drawing units tall, turned
    /// `rotation` degrees, in the text style `style` (the drawing's current one when `None`),
    /// with the current layer, colour and linetype: one undo step, `Outcome::Added`. Stored in
    /// reading order (see `crate::text`).
    #[schemars(extend("x-category" = "text"))]
    #[schemars(example = json!({"type": "text", "at": [2, 3], "height": 0.25, "rotation": 0, "value": "LIVING ROOM"}))]
    Text {
        at: [f64; 2],
        height: f64,
        rotation: f64,
        value: String,
        #[serde(default)]
        style: Option<String>,
    },
    /// Multiline text: `value` is plain text, lines split by `\n`, in a box `width` drawing units
    /// wide (0: no wrapping) whose top left corner is `at`. As `Text` otherwise.
    #[serde(rename = "mtext")]
    #[schemars(extend("x-category" = "text"))]
    #[schemars(example = json!({"type": "mtext", "at": [2, 6], "width": 4, "height": 0.2, "value": "Floor: porcelain tiles\nSkirting: 100 mm"}))]
    MText {
        at: [f64; 2],
        width: f64,
        height: f64,
        value: String,
        #[serde(default)]
        style: Option<String>,
    },
    /// Changes the text of `target` (a text or an MText at the top of the sheet on screen) to
    /// `value`, plain text (an MText's formatting goes); for a block reference the value of its
    /// attribute `tag` (which may be cleared). One undo step.
    #[schemars(extend("x-category" = "text"))]
    #[schemars(example = json!({"type": "edit_text", "target": {"handle": 1234}, "value": "KITCHEN"}))]
    EditText {
        target: Target,
        value: String,
        #[serde(default)]
        tag: Option<String>,
    },
    /// Replaces `find` with `replace` in the texts, MText (plain runs: formatting codes stay),
    /// attribute values and dimension text overrides of `scope`, in any case unless
    /// `match_case`, inside words too unless `whole_word`: one undo step (none when nothing
    /// changes). A TEXT the replacement would leave blank or break into two lines, or a
    /// single-line attribute it would break, is left as it is and counted as skipped.
    /// `Outcome::Replaced`.
    #[schemars(extend("x-category" = "text"))]
    #[schemars(example = json!({"type": "find_replace", "find": "BED ROOM", "replace": "BEDROOM", "scope": "drawing"}))]
    FindReplace {
        find: String,
        replace: String,
        scope: TextScope,
        #[serde(default)]
        match_case: bool,
        #[serde(default)]
        whole_word: bool,
    },
    /// What `FindReplace` would replace: `Outcome::Found`. Not an undo step.
    #[schemars(extend("x-category" = "text"))]
    #[schemars(example = json!({"type": "find_text", "find": "KITCHEN", "scope": "sheet"}))]
    FindText {
        find: String,
        scope: TextScope,
        #[serde(default)]
        match_case: bool,
        #[serde(default)]
        whole_word: bool,
    },
    /// A revision cloud around the polygon `points` with the comment `text` beside it, both on
    /// the layer COMMENTS (see `crate::comment`); `arc_len` None works the arcs out from the
    /// outline. One undo step, `Outcome::Added` (the cloud, then its label).
    #[schemars(extend("x-category" = "markup"))]
    #[schemars(example = json!({"type": "comment", "points": [[0, 0], [4, 0], [4, 3], [0, 3]], "text": "Check this wall thickness"}))]
    Comment {
        points: Vec<[f64; 2]>,
        text: String,
        #[serde(default)]
        arc_len: Option<f64>,
    },
    /// The comment clouds of every sheet: `Outcome::Comments`. Not an undo step.
    #[schemars(extend("x-category" = "markup"))]
    #[schemars(example = json!({"type": "list_comments"}))]
    ListComments,
    /// Adds the dimensions `DimSpec` describes on the current layer in the current dimension
    /// style, each with its `*D` block: one undo step, `Outcome::Added` (see `crate::dimension`).
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "dimension", "kind": "linear", "p1": [0, 0], "p2": [5, 0], "line_point": [2.5, -1]}))]
    Dimension(DimSpec),
    /// Changes the dimension `target` (its text, its arrows, where its text sits) and rebuilds
    /// its block: one undo step.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "dim_edit", "target": {"handle": 1234}, "op": {"text_override": "<> TYP."}}))]
    #[schemars(example = json!({"type": "dim_edit", "target": {"handle": 1234}, "op": "flip"}))]
    DimEdit { target: Target, op: DimEditOp },
    /// Breaks the dimension `target`'s lines where the entity `cutter` crosses them.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "dim_break", "target": {"handle": 1234}, "cutter": {"handle": 1235}}))]
    DimBreak { target: Target, cutter: Target },
    /// Spaces the linear dimensions `targets`, parallel to `base`, `spacing` drawing units apart
    /// going out from it (none: twice the text height).
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "dim_space", "base": {"handle": 1234}, "targets": [{"handle": 1235}, {"handle": 1236}]}))]
    DimSpace {
        base: Target,
        targets: Vec<Target>,
        #[serde(default)]
        spacing: Option<f64>,
    },
    /// A jog in the linear dimension `target`'s line, nearest `at`.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "dim_jog_line", "target": {"handle": 1234}, "at": [2, -1]}))]
    DimJogLine { target: Target, at: [f64; 2] },
    /// A leader through `points` (arrowhead at the first) with `text` beside the last as an
    /// MText: `Outcome::Added` with the leader, then the text.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "leader", "points": [[3, 3], [5, 5], [6, 5]], "text": "RC column 400x400"}))]
    Leader { points: Vec<[f64; 2]>, text: String },
    /// A leader with a short landing before its text (Qiyas writes a multileader as a leader and
    /// an MText).
    #[serde(rename = "mleader")]
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "mleader", "points": [[3, 3], [5, 5]], "text": "See detail 3"}))]
    MLeader { points: Vec<[f64; 2]>, text: String },
    /// A feature control frame: `frame` in AutoCAD's codes, its left middle at `at`.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "tolerance", "at": [0, 0], "frame": "{\\Fgdt;p}%%v0.5%%vA"}))]
    Tolerance { at: [f64; 2], frame: String },
    /// The annotation scale (CANNOSCALE), "paper:drawing" such as "1:50": new dimensions,
    /// leaders and tolerances are drawn that many times their style's size.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "annotation_scale", "name": "1:100"}))]
    AnnotationScale { name: String },
    /// The dimension style `name` (made from the current one when new) takes `fields`; with
    /// `current`, new dimensions take it.
    #[schemars(extend("x-category" = "dimensions"))]
    #[schemars(example = json!({"type": "set_dim_style", "name": "QIYAS-100", "fields": {"arrow_size": 2.5, "text_height": 2.5, "precision": 2, "ext_offset": 0.625, "ext_extension": 1.25, "gap": 0.625, "scale": 100}, "current": true}))]
    SetDimStyle {
        name: String,
        fields: DimStyleFields,
        #[serde(default)]
        current: bool,
    },
    /// A table of `rows` x `cols` cells, its top left corner at `at`, columns `col_width` and rows
    /// `row_height` drawing units, cells filled row by row from `data` (plain text; field codes
    /// make a cell a field), on the current layer in the current text style (its text half the
    /// row height), with its `*T` block: one undo step, `Outcome::Added` (see `crate::table`).
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "table", "at": [0, 10], "rows": 3, "cols": 2, "col_width": 4, "row_height": 0.8, "data": [["Room", "Area (m²)"], ["Living", "58.28"], ["Kitchen", "17.39"]]}))]
    Table {
        at: [f64; 2],
        rows: usize,
        cols: usize,
        col_width: f64,
        row_height: f64,
        #[serde(default)]
        data: Vec<Vec<String>>,
    },
    /// Cell `row`, `col` (from 0) of the table `target` holds `text` (plain text; a cell inside a
    /// merged cell edits the merged cell).
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "edit_cell", "target": {"handle": 1234}, "row": 1, "col": 1, "text": "58.30"}))]
    EditCell {
        target: Target,
        row: usize,
        col: usize,
        text: String,
    },
    /// A new row before row `at` of the table `target` (`at` = its row count: after the last).
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "insert_row", "target": {"handle": 1234}, "at": 3}))]
    InsertRow { target: Target, at: usize },
    /// A new column before column `at`.
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "insert_col", "target": {"handle": 1234}, "at": 2}))]
    InsertCol { target: Target, at: usize },
    /// Removes row `row` (a table keeps one).
    #[schemars(extend("x-category" = "tables", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_row", "target": {"handle": 1234}, "row": 2}))]
    DeleteRow { target: Target, row: usize },
    /// Removes column `col` (a table keeps one).
    #[schemars(extend("x-category" = "tables", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_col", "target": {"handle": 1234}, "col": 1}))]
    DeleteCol { target: Target, col: usize },
    /// Merges the cells from `first` to `last` (`[row, column]`); one cell given unmerges the
    /// merged cell it is in.
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "merge_cells", "target": {"handle": 1234}, "first": [0, 0], "last": [0, 1]}))]
    MergeCells {
        target: Target,
        first: [usize; 2],
        last: [usize; 2],
    },
    /// A one-line text at `at`, `height` tall, holding the field `field` (see `crate::field`).
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "field", "at": [2, 2], "height": 0.25, "field": {"kind": "area", "target": {"handle": 1234}}}))]
    Field {
        at: [f64; 2],
        height: f64,
        field: FieldKind,
    },
    /// Works every field of the drawing out again: one undo step when one changes.
    #[schemars(extend("x-category" = "tables"))]
    #[schemars(example = json!({"type": "update_fields"}))]
    UpdateFields,
    /// A new block `name` (not one the drawing has; none of `<>/\":;?*|,=` and the backquote)
    /// from the entities `targets` at the top of the sheet: they leave the sheet, their
    /// coordinates taken relative to `base`. With `convert_to_insert` a reference to the block
    /// takes their place at `base`. Rows measuring them keep their values without the link.
    /// `Outcome::Edited` (see `crate::blocks`).
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "create_block", "name": "WC-UNIT", "base": [0, 0], "targets": [{"handle": 1234}, {"handle": 1235}], "convert_to_insert": true}))]
    CreateBlock {
        name: String,
        base: [f64; 2],
        targets: Vec<Target>,
        #[serde(default)]
        convert_to_insert: bool,
    },
    /// A reference to the block `name` at `at`, `scale` times its size (above 0), turned
    /// `rotation` degrees, on the current layer and state; its attributes take
    /// `attribute_values` by tag, else their defaults. With `from_path`, a block the drawing
    /// lacks is first copied, with what it uses, from the drawing there (read-only).
    /// `Outcome::Added`.
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "insert_block", "name": "DOOR-900", "at": [3, 0], "rotation": 90}))]
    InsertBlock {
        name: String,
        at: [f64; 2],
        #[serde(default = "one")]
        scale: f64,
        #[serde(default)]
        rotation: f64,
        #[serde(default)]
        attribute_values: BTreeMap<String, String>,
        #[serde(default)]
        from_path: Option<String>,
    },
    /// Writes the block `name` as a DWG of its own at `path`, a file in the drawing's folder or
    /// below it (relative paths start there): its entities in model space, its base point at
    /// the origin. Not an undo step.
    #[schemars(extend("x-category" = "blocks", "x-effect" = "file", "x-batch" = false))]
    #[schemars(example = json!({"type": "write_block", "name": "DOOR-900", "path": "blocks/door-900.dwg"}))]
    WriteBlock { name: String, path: String },
    /// Edits the block of the reference `target` in place, until `SaveBlockEdits` or
    /// `DiscardBlockEdits` (see `crate::blocks`). `Outcome::Added` lists the block's entities as
    /// copied onto the sheet. Not an undo step.
    #[schemars(extend("x-category" = "blocks", "x-effect" = "session", "x-batch" = false))]
    #[schemars(example = json!({"type": "edit_block_in_place", "target": {"handle": 1234}}))]
    EditBlockInPlace { target: Target },
    /// Ends the edit in place, the block taking what was edited (every reference changes).
    /// Not an undo step: the edit's steps leave the history. `Outcome::BlockEditClosed`.
    #[schemars(extend("x-category" = "blocks", "x-effect" = "session", "x-batch" = false))]
    #[schemars(example = json!({"type": "save_block_edits"}))]
    SaveBlockEdits,
    /// Ends the edit in place, the block as it was; everything else done meanwhile stays.
    /// `Outcome::BlockEditClosed`.
    #[schemars(extend("x-category" = "blocks", "x-effect" = "session", "x-batch" = false, "x-destructive" = true))]
    #[schemars(example = json!({"type": "discard_block_edits"}))]
    DiscardBlockEdits,
    /// An attribute definition `tag` (one word, upper-cased) on the current layer at `at`,
    /// `height` tall: it asks `prompt` on insert and takes `default` when nothing is given.
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "att_def", "tag": "ROOM_NO", "prompt": "Room number", "default": "G01", "at": [0, 0], "height": 0.25}))]
    AttDef {
        tag: String,
        prompt: String,
        default: String,
        at: [f64; 2],
        height: f64,
    },
    /// The block reference `target`'s attributes take `values` by tag.
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "edit_attributes", "target": {"handle": 1234}, "values": {"ROOM_NO": "G02"}}))]
    EditAttributes {
        target: Target,
        values: BTreeMap<String, String>,
    },
    /// The attribute definition `tag` of the block `block` asks `prompt` and defaults to
    /// `default`.
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "edit_att_def", "block": "ROOM-TAG", "tag": "ROOM_NO", "prompt": "Room number", "default": "G01"}))]
    EditAttDef {
        block: String,
        tag: String,
        prompt: String,
        default: String,
    },
    /// Every reference to `block` gets its attributes as the block defines them now, keeping
    /// each tag's value (AutoCAD's ATTSYNC).
    #[schemars(extend("x-category" = "blocks"))]
    #[schemars(example = json!({"type": "sync_attributes", "block": "ROOM-TAG"}))]
    SyncAttributes { block: String },
    /// The block `name` takes the contents of the block of that name in the drawing at
    /// `from_path` (read-only); every reference changes.
    #[schemars(extend("x-category" = "blocks", "x-destructive" = true))]
    #[schemars(example = json!({"type": "redefine_block", "name": "DOOR-900", "from_path": "library/doors.dwg"}))]
    RedefineBlock { name: String, from_path: String },
    /// Every layer's on, frozen, locked, plot, colour, linetype and lineweight, kept in the
    /// drawing as the layer state `name` (AutoCAD's Layer States Manager sees it; one of that
    /// name is replaced), with the hidden layers kept beside it in the project file.
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "save_layer_state", "name": "Walls only"}))]
    SaveLayerState { name: String },
    /// The layers take what the state `name` kept; the hidden layers come back too.
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "restore_layer_state", "name": "Walls only"}))]
    RestoreLayerState { name: String },
    /// Deletes the layer state `name`; the layers stay as they are.
    #[schemars(extend("x-category" = "layers", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_layer_state", "name": "Walls only"}))]
    DeleteLayerState { name: String },
    /// Renames the layer state `from` to `to`.
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "rename_layer_state", "from": "Walls only", "to": "Structure"}))]
    RenameLayerState { from: String, to: String },
    /// One property of the drawing's layer `name` (see `LayerProp` for the values).
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "set_layer", "name": "A-WALL", "prop": "color", "value": "ACI 1"}))]
    #[schemars(example = json!({"type": "set_layer", "name": "A-WALL", "prop": "lineweight", "value": 50}))]
    #[schemars(example = json!({"type": "set_layer", "name": "A-FURN", "prop": "locked", "value": true}))]
    SetLayer {
        name: String,
        prop: LayerProp,
        value: serde_json::Value,
    },
    /// A new layer, white and Continuous.
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "new_layer", "name": "A-DOOR"}))]
    NewLayer { name: String },
    /// Deletes an empty layer: never 0, Defpoints or the current layer.
    #[schemars(extend("x-category" = "layers", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_layer", "name": "TEMP"}))]
    DeleteLayer { name: String },
    /// Renames a layer; its entities, the take-off rows on it, the current and hidden layers
    /// follow. A name another layer has is refused (`TranslateLayers` merges).
    #[schemars(extend("x-category" = "layers"))]
    #[schemars(example = json!({"type": "rename_layer", "from": "WALLS", "to": "A-WALL"}))]
    RenameLayer { from: String, to: String },
    /// Renames, merges (when `to` exists) and recolours layers in one step; with `from_path`
    /// each layer then takes the colour, linetype, lineweight and plot setting of that drawing's
    /// layer of its new name (opened read-only).
    #[schemars(extend("x-category" = "layers", "x-destructive" = true))]
    #[schemars(example = json!({"type": "translate_layers", "mappings": [{"from": "WALLS", "to": "A-WALL", "color": "ACI 7"}]}))]
    TranslateLayers {
        #[serde(default)]
        from_path: Option<String>,
        mappings: Vec<LayerMapping>,
    },
    /// The paper-space viewport `viewport` (on the sheet on screen) freezes `layer` inside it,
    /// or thaws it.
    #[schemars(extend("x-category" = "layouts"))]
    #[schemars(example = json!({"type": "set_viewport_frozen", "viewport": {"handle": 1234}, "layer": "A-FURN", "frozen": true}))]
    SetViewportFrozen {
        viewport: Target,
        layer: String,
        frozen: bool,
    },
    /// A new group `name` of the entities `targets` at the top of the sheet.
    #[schemars(extend("x-category" = "groups"))]
    #[schemars(example = json!({"type": "create_group", "name": "KITCHEN-UNITS", "targets": [{"handle": 1234}, {"handle": 1235}]}))]
    CreateGroup { name: String, targets: Vec<Target> },
    /// Adds the entities `targets` at the top of the sheet to the group `name`.
    #[schemars(extend("x-category" = "groups"))]
    #[schemars(example = json!({"type": "add_to_group", "name": "KITCHEN-UNITS", "targets": [{"handle": 1236}]}))]
    AddToGroup { name: String, targets: Vec<Target> },
    /// Takes the entities `targets` out of the group `name`; they stay in the drawing.
    #[schemars(extend("x-category" = "groups"))]
    #[schemars(example = json!({"type": "remove_from_group", "name": "KITCHEN-UNITS", "targets": [{"handle": 1236}]}))]
    RemoveFromGroup { name: String, targets: Vec<Target> },
    /// The group goes; its entities stay (AutoCAD's explode of a group).
    #[schemars(extend("x-category" = "groups", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_group", "name": "KITCHEN-UNITS"}))]
    DeleteGroup { name: String },
    /// Every group holding one of `targets` goes; the entities stay.
    #[schemars(extend("x-category" = "groups", "x-destructive" = true))]
    #[schemars(example = json!({"type": "ungroup", "targets": [{"handle": 1234}]}))]
    Ungroup { targets: Vec<Target> },
    /// Each of `targets` takes the `props` of `source` (which may be inside a block or an
    /// xref) where it has them.
    #[schemars(extend("x-category" = "modify"))]
    #[schemars(example = json!({"type": "match_properties", "source": {"handle": 1234}, "targets": [{"handle": 1235}], "props": ["layer", "color"]}))]
    MatchProperties {
        source: Target,
        targets: Vec<Target>,
        props: Vec<MatchProp>,
    },
    /// The palette's edit: one property of the entity `target` at the top of the sheet (see
    /// `EntityProp` for the values).
    #[schemars(extend("x-category" = "modify"))]
    #[schemars(example = json!({"type": "set_property", "target": {"handle": 1234}, "prop": "layer", "value": "A-WALL"}))]
    #[schemars(example = json!({"type": "set_property", "target": {"handle": 1234}, "prop": "viewport_scale", "value": 0.01}))]
    SetProperty {
        target: Target,
        prop: EntityProp,
        value: serde_json::Value,
    },
    /// The clipboard text for the entities `targets` at the top of the sheet, their coordinates
    /// taken from `base` (by default the lower left corner of what they draw): a small DXF
    /// drawing with the layers, blocks and styles they use (see `crate::clipboard`). Not an undo
    /// step; `Outcome::Clipboard`.
    #[schemars(extend("x-category" = "clipboard", "x-needs" = "clipboard", "x-batch" = false))]
    #[schemars(example = json!({"type": "copy_to_clipboard", "targets": [{"handle": 1234}]}))]
    CopyToClipboard {
        targets: Vec<Target>,
        #[serde(default)]
        base: Option<[f64; 2]>,
    },
    /// Pastes the clipboard `text` (the desktop app fills it from the system clipboard) on the
    /// sheet on screen, its base point at `at`: Qiyas's entities with new handles (the layers,
    /// blocks and styles the drawing lacks come along), as one block reference with `as_block`;
    /// plain text as a text `height` tall (2.5 mm at the annotation scale by default).
    /// `Outcome::Added`.
    #[schemars(extend("x-category" = "clipboard", "x-needs" = "clipboard"))]
    #[schemars(example = json!({"type": "paste_from_clipboard", "at": [10, 0]}))]
    PasteFromClipboard {
        at: [f64; 2],
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        as_block: bool,
        #[serde(default)]
        height: Option<f64>,
    },
    /// A paper-space viewport on the layout on screen with the corners `corner1` and
    /// `corner2`, showing all of model space (see `crate::viewports`); on the current layer.
    /// `Outcome::Added`.
    #[serde(rename = "mview")]
    #[schemars(extend("x-category" = "layouts"))]
    #[schemars(example = json!({"type": "mview", "corner1": [10, 10], "corner2": [400, 280]}))]
    MView {
        corner1: [f64; 2],
        corner2: [f64; 2],
    },
    /// The user coordinate system becomes the one at `origin` with its x axis along `x_axis`
    /// (`[1, 0]` from `[0, 0]` is the world's). View state: not an undo step (see `crate::ucs`).
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "ucs", "origin": [100, 50], "x_axis": [1, 1]}))]
    Ucs { origin: [f64; 2], x_axis: [f64; 2] },
    /// The user coordinate system becomes the entity `target`'s (AutoCAD's UCS OB; see
    /// `Session::ucs_of_entity`). Not an undo step.
    #[schemars(extend("x-category" = "view", "x-effect" = "view"))]
    #[schemars(example = json!({"type": "ucs_object", "target": {"handle": 1234}}))]
    UcsObject { target: Target },
    /// AutoCAD's PURGE: what nothing uses, of `kinds` (every kind when empty; see
    /// `crate::purge`). A dry run only lists it (no step); otherwise it goes as one undo step.
    /// `Outcome::Purged`.
    #[schemars(extend("x-category" = "manage", "x-destructive" = true))]
    #[schemars(example = json!({"type": "purge", "kinds": ["layers", "blocks"], "dry_run": true}))]
    #[schemars(example = json!({"type": "purge", "kinds": [], "dry_run": false}))]
    Purge {
        #[serde(default)]
        kinds: Vec<crate::purge::PurgeKind>,
        dry_run: bool,
    },
    /// AutoCAD's AUDIT: what points at something the drawing does not have (see
    /// `crate::audit`); with `fix` the safe repairs, as one undo step. `Outcome::Audited`.
    #[schemars(extend("x-category" = "manage"))]
    #[schemars(example = json!({"type": "audit", "fix": false}))]
    #[schemars(example = json!({"type": "audit", "fix": true}))]
    Audit {
        #[serde(default)]
        fix: bool,
    },
    /// The geometric constraint `kind` on the parts of `targets` (at the top of the sheet) picked
    /// at `points`, one point each (see `crate::param`): coincident, fixed and symmetric's first
    /// two take vertices or centres; horizontal and vertical a line (or two points); parallel,
    /// perpendicular, collinear and symmetric's third a line or straight segment; concentric
    /// circles or arcs; tangent, smooth and equal any edge. The first entity picked stays
    /// where it is when it can; the rest move to hold it. `Outcome::Constrained`.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "add_constraint", "kind": "horizontal", "targets": [{"handle": 1234}], "points": [[2, 0]]}))]
    AddConstraint {
        kind: crate::ConstraintKind,
        targets: Vec<Target>,
        #[serde(default)]
        points: Vec<[f64; 2]>,
    },
    /// Removes the constraint `id` (a dimensional one with its Dimension).
    #[schemars(extend("x-category" = "parametric", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_constraint", "id": 3}))]
    DeleteConstraint { id: u32 },
    /// Removes every constraint holding one of `targets` (AutoCAD's DELCONSTRAINT).
    /// `Outcome::Constrained` with the ids removed.
    #[schemars(extend("x-category" = "parametric", "x-destructive" = true))]
    #[schemars(example = json!({"type": "delete_constraints_on", "targets": [{"handle": 1234}]}))]
    DeleteConstraintsOn { targets: Vec<Target> },
    /// The constraints already true of `targets` within `settings`' tolerances (AutoCAD's
    /// AUTOCONSTRAIN), less those there already. `Outcome::Constrained`.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "auto_constrain", "targets": [{"handle": 1234}, {"handle": 1235}]}))]
    AutoConstrain {
        targets: Vec<Target>,
        #[serde(default)]
        settings: crate::AutoSettings,
    },
    /// The dimensional constraint `kind` (linear, aligned, angular, radius, diameter) on the
    /// parts of `targets` picked at `points`, its Dimension through `line_point`, its value
    /// `expression` (the value measured now when none). `Outcome::Constrained`.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "add_dim_constraint", "kind": "linear", "targets": [{"handle": 1234}], "points": [[2, 0]], "line_point": [2, -1], "expression": "4.5"}))]
    AddDimConstraint {
        kind: crate::ConstraintKind,
        targets: Vec<Target>,
        #[serde(default)]
        points: Vec<[f64; 2]>,
        line_point: [f64; 2],
        #[serde(default)]
        expression: Option<String>,
    },
    /// The parameter `name` (a dimensional constraint's) takes `expression` (a number, or a
    /// formula of other parameters such as `2*d2 + 1.5`); its constraints and every one using it
    /// are solved again.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "set_parameter", "name": "d1", "expression": "2*d2 + 1.5"}))]
    SetParameter { name: String, expression: String },
    /// Records of a collection (`crate::records`): the entities of a sheet or block, layers,
    /// linetypes, text and dimension styles, blocks, layouts, viewports, groups, views, the header,
    /// layer states, xrefs, the take-off, BOQ, conflicts, schedules, comments, constraints and
    /// parameters. Filter by handles, names, type, layer, sheet, bounds and `where` conditions on
    /// JSON pointers into each record's `data`; `fields` picks values; `detail` "full" gives the
    /// whole `data`. Pages of 20 (200 at most), under 40 KB. `Outcome::Records`; not an undo step.
    #[schemars(extend("x-category" = "records"))]
    #[schemars(example = json!({"type": "records", "query": {"collection": "entities", "type": "Line", "layer": "A-WALL"}}))]
    #[schemars(example = json!({"type": "records", "query": {"collection": "layers", "detail": "full", "names": ["A-WALL"]}}))]
    #[schemars(example = json!({"type": "records", "query": {"collection": "entities", "where": [{"path": "/radius", "op": "gt", "value": 0.5}], "fields": ["/center", "/radius"]}}))]
    Records {
        query: Box<crate::records::RecordQuery>,
    },
    /// The paths of a record type ("Line", "LwPolyline", "Text", "Viewport", "layer",
    /// "text_style", "header"), each with its value type and whether `set_records` may set it (and
    /// the command to use when not), with the shapes of the types they use. Not an undo step.
    #[schemars(extend("x-category" = "records"))]
    #[schemars(example = json!({"type": "record_schema", "name": "Circle"}))]
    RecordSchema { name: String },
    /// Sets properties of records: each update puts `value` at the JSON pointer `path` of the
    /// record `key`'s data (an entity's handle, a layer's or text style's name, "header"), checked
    /// by the drawing library's types; `expected` refuses it when the record changed since it was
    /// read. Entities (and viewports), layers, text styles and some header variables; other paths
    /// and collections say which command does it (record_schema lists them). One undo step, all
    /// or nothing; the take-off, fields and blocks of dimensions and tables follow, and an edited
    /// entity loses its constraints.
    #[schemars(extend("x-category" = "records"))]
    #[schemars(example = json!({"type": "set_records", "collection": "entities", "updates": [{"key": 1234, "path": "/radius", "value": 0.75}]}))]
    #[schemars(example = json!({"type": "set_records", "collection": "layers", "updates": [{"key": "A-WALL", "path": "/line_weight", "value": "W50"}]}))]
    #[schemars(example = json!({"type": "set_records", "collection": "viewports", "updates": [{"key": 1234, "path": "/view_height", "value": 50.0}]}))]
    SetRecords {
        collection: crate::records::Collection,
        updates: Vec<crate::RecordUpdate>,
    },
    /// Adds entities of any type the drawing library makes (record_schema says which): each
    /// starts as its type's default on the current layer, colour and linetype, `data` merged over
    /// it as its record's data would be (missing fields keep the default). On `sheet` (its name or
    /// block; the sheet on screen by default); a layer named that the drawing lacks is made. One
    /// undo step; `Outcome::Added`. Dimensions, tables, viewports, block references and pictures
    /// are refused with their own command.
    #[schemars(extend("x-category" = "draw"))]
    #[schemars(example = json!({"type": "create_entities", "entities": [{"type": "Circle", "data": {"center": {"x": 2, "y": 2, "z": 0}, "radius": 0.3, "common": {"layer": "A-COLS"}}}]}))]
    CreateEntities {
        entities: Vec<crate::NewEntity>,
        #[serde(default)]
        sheet: Option<String>,
    },
    /// The constraints of the sheet on screen. `Outcome::Constraints`; not an undo step.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "list_constraints"}))]
    ListConstraints,
    /// Every parameter of the drawing. `Outcome::Parameters`; not an undo step.
    #[schemars(extend("x-category" = "parametric"))]
    #[schemars(example = json!({"type": "list_parameters"}))]
    ListParameters,
}

/// `InsertBlock`'s scale when none is given.
fn one() -> f64 {
    1.0
}

/// An entity as picked (see `Picked`): its handle and the block references it sits inside,
/// outermost first (empty, the default, for an entity on the sheet). A chain ending in an xref
/// reference names an entity of that xref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Target {
    pub handle: u64,
    #[serde(default)]
    pub chain: Vec<u64>,
}

/// The part of the session a command changes, kept whole in its undo step.
pub(crate) enum Part {
    Takeoff,
    Boq,
    /// The conflicts and the schedules.
    Conflicts,
    /// The drawing itself: the entities the command touches are kept as they were (their
    /// before images), with the drawing's tables when `tables` and the take-off when `takeoff`
    /// (divide and measure along change both).
    Document {
        tables: bool,
        takeoff: bool,
    },
}

impl Command {
    /// What the command is, as the history and errors name it: its wire name, with the kind
    /// after a dot for one that has kinds ("draw.line", "set_layer").
    pub fn label(&self) -> String {
        let wire = serde_json::to_value(self).unwrap_or_default();
        let name = wire["type"].as_str().unwrap_or("command");
        match wire["kind"].as_str() {
            Some(kind) if matches!(name, "draw" | "modify" | "edit" | "dimension") => {
                format!("{name}.{kind}")
            }
            _ => name.to_string(),
        }
    }

    /// The part of the session the command changes, and so keeps in its undo step; `None` when
    /// it is not an undo step.
    pub(crate) fn changes(&self) -> Option<Part> {
        match self {
            Command::SetLayerHidden { .. }
            | Command::IsolateLayer { .. }
            | Command::UnisolateLayers
            | Command::SetSheet { .. }
            | Command::SetArabicReversed { .. }
            | Command::SetDrafting { .. }
            | Command::MeasureGeometry { .. }
            | Command::IdPoint { .. }
            | Command::ExtractData { .. }
            | Command::FindText { .. }
            | Command::SetCurrent { .. }
            | Command::WriteBlock { .. }
            | Command::EditBlockInPlace { .. }
            | Command::SaveBlockEdits
            | Command::DiscardBlockEdits
            | Command::CopyToClipboard { .. }
            | Command::Ucs { .. }
            | Command::UcsObject { .. }
            | Command::ListConstraints
            | Command::ListParameters
            | Command::Records { .. }
            | Command::RecordSchema { .. }
            | Command::ListComments => None,
            Command::SetBoq { .. } | Command::SetBoqTolerance { .. } => Some(Part::Boq),
            Command::SetConflicts { .. } | Command::ResolveConflict { .. } => Some(Part::Conflicts),
            // Entities only.
            Command::Calibrate { .. }
            | Command::SetImageDisplay { .. }
            | Command::DeleteEntities { .. }
            | Command::Modify(_)
            | Command::Edit(_) => Some(Part::Document {
                tables: false,
                takeoff: false,
            }),
            // A dimension's block, its list of entities, the text mark's application, the
            // header's annotation scale and dimension style.
            Command::FindReplace { .. }
            // The comment layer and the comment and text marks' applications.
            | Command::Comment { .. }
            | Command::Dimension(_)
            | Command::DimEdit { .. }
            | Command::DimBreak { .. }
            | Command::DimSpace { .. }
            | Command::DimJogLine { .. }
            | Command::Leader { .. }
            | Command::MLeader { .. }
            | Command::Tolerance { .. }
            | Command::AnnotationScale { .. }
            | Command::SetDimStyle { .. }
            // A table's block; the field mark's application.
            | Command::Table { .. }
            | Command::EditCell { .. }
            | Command::InsertRow { .. }
            | Command::InsertCol { .. }
            | Command::DeleteRow { .. }
            | Command::DeleteCol { .. }
            | Command::MergeCells { .. }
            | Command::Field { .. }
            | Command::UpdateFields
            // A new or changed block record (and what a copied one uses).
            | Command::CreateBlock { .. }
            | Command::RedefineBlock { .. }
            // The layers, linetypes, styles and blocks the pasted entities bring.
            | Command::PasteFromClipboard { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // The text mark's application, when the drawing has none yet (`document_extras`).
            Command::Text { .. }
            | Command::MText { .. }
            | Command::EditText { .. }
            | Command::InsertBlock { .. }
            | Command::AttDef { .. }
            | Command::EditAttributes { .. }
            | Command::EditAttDef { .. }
            | Command::SyncAttributes { .. } => {
                Some(Part::Document {
                    tables: false,
                    takeoff: false,
                })
            }
            // Layers, the text mark's application and the current state, all in one step.
            Command::Batch { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // A wipeout registers its class with the drawing.
            Command::Draw(spec) => Some(Part::Document {
                tables: matches!(spec, DrawSpec::Wipeout { .. }),
                takeoff: false,
            }),
            // New layers, views, or underlay and image definitions too.
            Command::ImportPdf { .. }
            | Command::AttachPdfUnderlay { .. }
            | Command::AttachImage { .. }
            | Command::SaveView { .. }
            | Command::DeleteView { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // The layer table, layer states (objects) and groups (objects); a layer's entities.
            Command::SaveLayerState { .. }
            | Command::RestoreLayerState { .. }
            | Command::DeleteLayerState { .. }
            | Command::RenameLayerState { .. }
            | Command::SetLayer { .. }
            | Command::NewLayer { .. }
            | Command::DeleteLayer { .. }
            | Command::CreateGroup { .. }
            | Command::AddToGroup { .. }
            | Command::RemoveFromGroup { .. }
            | Command::DeleteGroup { .. }
            | Command::Ungroup { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // The take-off rows on a renamed or merged layer take its new name.
            Command::RenameLayer { .. } | Command::TranslateLayers { .. } => {
                Some(Part::Document {
                    tables: true,
                    takeoff: true,
                })
            }
            // The tables and take-off when a target needs them (`document_extras`).
            Command::MView { .. }
            | Command::SetViewportFrozen { .. }
            | Command::MatchProperties { .. }
            | Command::SetProperty { .. } => Some(Part::Document {
                tables: false,
                takeoff: false,
            }),
            // Any property of entities, layers, styles or the header; the take-off follows.
            Command::SetRecords { .. } => Some(Part::Document {
                tables: true,
                takeoff: true,
            }),
            // New layers, the text mark's application, a wipeout's class.
            Command::CreateEntities { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // Points on their own layer, and their count in the take-off.
            Command::DivideAlong { .. } | Command::MeasureAlong { .. } => Some(Part::Document {
                tables: true,
                takeoff: true,
            }),
            // The constraint mark's application, dimensional constraints' blocks.
            Command::AddConstraint { .. }
            | Command::DeleteConstraint { .. }
            | Command::DeleteConstraintsOn { .. }
            | Command::AutoConstrain { .. }
            | Command::AddDimConstraint { .. }
            | Command::SetParameter { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            // Lists only.
            Command::Purge { dry_run: true, .. } | Command::Audit { fix: false } => None,
            // Table entries, block records with their entities, groups (objects); entities'
            // layers and styles.
            Command::Purge { .. } | Command::Audit { .. } => Some(Part::Document {
                tables: true,
                takeoff: false,
            }),
            _ => Some(Part::Takeoff),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Measured {
        measurement: Box<Measurement>,
    },
    Removed {
        id: String,
    },
    Changed,
    /// Entities added to the drawing, by handle.
    Added {
        handles: Vec<u64>,
    },
    /// A page of records (`Command::Records`).
    Records {
        page: crate::records::RecordPage,
    },
    /// A record type's paths (`Command::RecordSchema`).
    RecordSchema {
        schema: serde_json::Value,
    },
    /// A batch's commands' outcomes, in order, and every entity it added.
    Batch {
        outcomes: Vec<Outcome>,
        added: Vec<u64>,
    },
    /// An explode's parts, by handle, and how many entities picked had no parts and were left
    /// as they are; the take-off rows that followed, as in `TakeoffFollowed`.
    Exploded {
        handles: Vec<u64>,
        left_alone: usize,
        remeasured: usize,
        unlinked: usize,
    },
    /// An update of block references' attributes that left locked references as they were (on a
    /// locked layer, or outside the block edited in place; `Changed` when none): how many
    /// references changed, and how many locked ones were left.
    Updated {
        count: usize,
        skipped: usize,
    },
    /// An erase that the take-off followed (or a move, turn, scale, mirror, stretch or align that
    /// left a row unmeasurable; `Changed` otherwise): how many take-off rows that measured an erased entity and others too were measured again
    /// without it (a count counts one less), and how many rows lost their link (every entity
    /// erased, or no longer measurable; their values stay).
    TakeoffFollowed {
        remeasured: usize,
        unlinked: usize,
    },
    /// An edit made in place: the entities it added (pieces of a trim or a break, a fillet's
    /// arc...) and removed (joined or duplicates), how many picked it left as they were, how
    /// many take-off rows lost their link because their entities could no longer be measured,
    /// and for a join how many rows now measure the joined polyline (0 otherwise).
    Edited {
        added: Vec<u64>,
        removed: Vec<u64>,
        left_alone: usize,
        unlinked: usize,
        joined_rows: usize,
        /// Parametric constraints dropped because the edit reshaped (or took away) an entity
        /// they held.
        constraints_removed: usize,
    },
    /// A find and replace: how many replacements, the entities changed (a block reference's,
    /// for an attribute), and how many entities with matches were left as they were because
    /// the result would not fit (a blank text, a line break in a single line) or they are locked
    /// (a locked layer, or outside the block edited in place).
    Replaced {
        count: usize,
        handles: Vec<u64>,
        skipped: usize,
    },
    /// A find: how many matches, and each entity holding one with its sheet.
    Found {
        count: usize,
        matches: Vec<TextMatch>,
    },
    /// The command named a measurement or BOQ item that does not exist.
    Nothing,
    /// Facts to show: a title (an entity type, or "Point") and label-value rows. Labels are keys
    /// such as "radius" or "centre"; values are written out in metres and degrees.
    Info {
        title: String,
        rows: Vec<(String, String)>,
    },
    /// A table: its column headings and its rows of text.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    /// The clipboard text `CopyToClipboard` made, and how many entities it holds. The desktop
    /// app puts the text on the system clipboard and sends it on empty.
    Clipboard {
        text: String,
        count: usize,
    },
    /// An edit in place of the block `name` ended, `saved` into the block or not: how many
    /// take-off rows lost their link, and how many undo steps left the history with it.
    BlockEditClosed {
        name: String,
        saved: bool,
        unlinked: usize,
        dropped: usize,
    },
    /// What a purge took out, or a dry run would.
    Purged {
        report: crate::purge::PurgeReport,
    },
    /// What an audit found, and repaired with `fix`.
    Audited {
        report: crate::audit::AuditReport,
    },
    /// The constraints added (or removed), by id.
    Constrained {
        ids: Vec<u32>,
    },
    /// The comment clouds of every sheet.
    Comments {
        comments: Vec<crate::CommentInfo>,
    },
    /// The constraints of the sheet on screen.
    Constraints {
        constraints: Vec<crate::ConstraintInfo>,
    },
    /// The drawing's parameters.
    Parameters {
        parameters: Vec<crate::ParameterInfo>,
    },
}

impl Session {
    /// Carries out `cmd` without recording an undo step; `run_as` does that. A new measurement
    /// is marked as made by `source`.
    pub(crate) fn execute(&mut self, cmd: Command, source: Source) -> Result<Outcome, CoreError> {
        let mut measurement = match cmd {
            Command::MeasureDistance { points } => {
                if points.len() < 2 {
                    return Err(invalid("A distance needs at least 2 points"));
                }
                let path = Path {
                    points,
                    closed: false,
                };
                let mut m = self.measurement(Kind::Length, path.length(), "Distance", "");
                m.points = path.points;
                m.auto_description = true;
                m
            }
            Command::MeasureArea { points } => {
                if points.len() < 3 {
                    return Err(invalid("An area needs at least 3 points"));
                }
                let ring = Path {
                    points,
                    closed: true,
                };
                let mut m = self.measurement(Kind::Area, ring.area(), "Area", "");
                m.area_origin = crate::AreaOrigin::ManualOrEntity;
                m.self_intersecting = ring.self_intersects();
                m.points = ring.points;
                m.auto_description = true;
                m
            }
            Command::MeasureEntity { handle, chain } => {
                let mut m = self.measure_entity(handle, chain)?;
                m.auto_description = true;
                m
            }
            Command::CountBlock { block_name, layer } => {
                let mut m = self.count_block(block_name, layer);
                m.auto_description = true;
                m
            }
            Command::RemoveMeasurement { id } => {
                return Ok(match self.takeoff.remove(&id) {
                    Some(_) => Outcome::Removed { id },
                    None => Outcome::Nothing,
                });
            }
            Command::SetDescription { id, text } => {
                return Ok(match self.takeoff.items.iter_mut().find(|m| m.id == id) {
                    Some(m) => {
                        m.description = text;
                        m.auto_description = false;
                        Outcome::Changed
                    }
                    None => Outcome::Nothing,
                });
            }
            Command::SetLayerHidden { name, hidden } => {
                self.set_layer_hidden(&name, hidden);
                return Ok(Outcome::Changed);
            }
            Command::IsolateLayer { name } => {
                self.isolate_layer(&name)?;
                return Ok(Outcome::Changed);
            }
            Command::UnisolateLayers => {
                self.unisolate_layers();
                return Ok(Outcome::Changed);
            }
            Command::SetSheet { block } => {
                self.set_sheet(&block)?;
                return Ok(Outcome::Changed);
            }
            Command::SetArabicReversed { on } => {
                self.arabic_reversed = on;
                return Ok(Outcome::Changed);
            }
            Command::SetDrafting { drafting } => {
                drafting.check()?;
                self.drafting = drafting;
                return Ok(Outcome::Changed);
            }
            Command::AddMeasurement {
                kind,
                value,
                unit,
                height,
                points,
                handles,
                chain,
                description,
                note,
            } => {
                let mut m =
                    self.add_measurement(kind, value, unit, height, points, handles, chain)?;
                m.description = description;
                m.note = note;
                m
            }
            Command::SetHeight { id, height } => {
                check_height(height)?;
                let units = self.drawing.units;
                return self.edit(&id, |m| {
                    let area = match (m.kind, m.height) {
                        (Kind::Area, _) => m.value,
                        (Kind::Volume, Some(old)) if old > 0.0 => m.value / old,
                        _ => return Err(invalid("Only an area or a volume takes a height")),
                    };
                    let value = area * height;
                    let (unit, shown) = in_metres(Kind::Volume, value, units);
                    m.kind = Kind::Volume;
                    m.value = value;
                    m.unit = unit.to_string();
                    m.shown = shown;
                    m.height = Some(height);
                    Ok(())
                });
            }
            Command::LinkBoq { id, code } => {
                return self.edit(&id, |m| {
                    m.boq_code = code;
                    Ok(())
                });
            }
            Command::SetNote { id, text } => {
                return self.edit(&id, |m| {
                    m.note = text;
                    Ok(())
                });
            }
            Command::SetBoq { boq } => {
                self.boq = Some(boq);
                return Ok(Outcome::Changed);
            }
            Command::SetBoqTolerance { code, percent } => {
                if percent.is_some_and(|p| !(p >= 0.0 && p.is_finite())) {
                    return Err(invalid("The tolerance must be a percentage of 0 or more"));
                }
                let mut items = self
                    .boq
                    .iter_mut()
                    .flat_map(|b| &mut b.items)
                    .filter(|i| i.code == code)
                    .peekable();
                if items.peek().is_none() {
                    return Ok(Outcome::Nothing);
                }
                items.for_each(|i| i.tolerance = percent);
                return Ok(Outcome::Changed);
            }
            Command::SetConflicts {
                conflicts,
                schedules,
            } => {
                self.conflicts = conflicts;
                self.schedules = schedules;
                return Ok(Outcome::Changed);
            }
            Command::ResolveConflict { id, status, note } => {
                return Ok(match self.conflicts.iter_mut().find(|c| c.id == id) {
                    Some(c) => {
                        c.status = status;
                        c.note = note;
                        Outcome::Changed
                    }
                    None => Outcome::Nothing,
                });
            }
            Command::ImportPdf {
                path,
                page,
                insert_at,
                scale,
            } => {
                let read = PreparedPdf::read(path.as_ref(), page, scale, AttachMode::Import)?;
                let handles = self.apply_prepared(read, insert_at)?;
                return Ok(Outcome::Added { handles });
            }
            Command::AttachPdfUnderlay {
                path,
                page,
                insert_at,
                scale,
            } => {
                let read = PreparedPdf::read(path.as_ref(), page, scale, AttachMode::Underlay)?;
                let handles = self.apply_prepared(read, insert_at)?;
                return Ok(Outcome::Added { handles });
            }
            Command::Calibrate {
                handle,
                p1,
                p2,
                real_distance,
            } => {
                self.rescale(handle, p1, p2, real_distance)?;
                return Ok(Outcome::Changed);
            }
            Command::AttachImage {
                path,
                insert_at,
                width_units,
            } => {
                let handle = self.add_image(path.as_ref(), insert_at, width_units)?;
                return Ok(Outcome::Added {
                    handles: vec![handle],
                });
            }
            Command::SetImageDisplay {
                handle,
                brightness,
                contrast,
                fade,
            } => {
                let display = crate::ImageDisplay {
                    brightness,
                    contrast,
                    fade,
                };
                self.apply_image_display(handle, display)?;
                return Ok(Outcome::Changed);
            }
            Command::MeasureGeometry { handle, chain, at } => {
                let (title, rows) = self.geometry_info(handle, &chain, at)?;
                return Ok(Outcome::Info { title, rows });
            }
            Command::DivideAlong {
                handle,
                chain,
                count,
            } => self.place_along(handle, chain, Spacing::Count(count))?,
            Command::MeasureAlong {
                handle,
                chain,
                step,
            } => self.place_along(handle, chain, Spacing::Step(step))?,
            Command::IdPoint { p, kind } => {
                if !p.iter().all(|c| c.is_finite()) {
                    return Err(invalid("The point must have finite coordinates"));
                }
                let rows = self.point_info(p, kind);
                return Ok(Outcome::Info {
                    title: "Point".to_string(),
                    rows,
                });
            }
            Command::ExtractData { filter, fields } => {
                let (columns, rows) = self.extract(&filter, &fields)?;
                return Ok(Outcome::Table { columns, rows });
            }
            Command::SaveView { name, bounds } => {
                self.save_view(&name, bounds)?;
                return Ok(Outcome::Changed);
            }
            Command::DeleteView { name } => {
                return Ok(if self.delete_view(&name) {
                    Outcome::Changed
                } else {
                    Outcome::Nothing
                });
            }
            Command::DeleteEntities { targets } => {
                let outcome = self.delete_entities(&targets)?;
                self.follow_fields()?;
                return Ok(outcome);
            }
            Command::Draw(spec) => {
                let handles = self.draw(spec)?;
                return Ok(Outcome::Added { handles });
            }
            Command::SetCurrent { current } => {
                self.set_current(current)?;
                return Ok(Outcome::Changed);
            }
            // `run_as` runs a batch as a transaction before it gets here.
            Command::Batch {
                commands,
                layer,
                color,
            } => {
                return self.run_batch(commands, layer, color, source);
            }
            Command::Modify(spec) => {
                let outcome = self.modify(spec)?;
                self.follow_fields()?;
                return Ok(outcome);
            }
            Command::Edit(spec) => {
                let outcome = self.edit_entities(spec)?;
                self.follow_fields()?;
                return Ok(outcome);
            }
            Command::Text {
                at,
                height,
                rotation,
                value,
                style,
            } => {
                let h = self.add_text(at, height, rotation, value, style.as_deref())?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::MText {
                at,
                width,
                height,
                value,
                style,
            } => {
                let h = self.add_mtext(at, width, height, &value, style.as_deref())?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::EditText { target, value, tag } => {
                self.edit_text(&target, &value, tag.as_deref())?;
                return Ok(Outcome::Changed);
            }
            Command::FindReplace {
                find,
                replace,
                scope,
                match_case,
                whole_word,
            } => {
                let finder = Finder::new(&find, match_case, whole_word)?;
                let (count, handles, skipped) = self.find_replace(&finder, &replace, scope)?;
                return Ok(Outcome::Replaced {
                    count,
                    handles,
                    skipped,
                });
            }
            Command::FindText {
                find,
                scope,
                match_case,
                whole_word,
            } => {
                let finder = Finder::new(&find, match_case, whole_word)?;
                let (count, matches) = self.find_text(&finder, scope);
                return Ok(Outcome::Found { count, matches });
            }
            Command::Comment {
                points,
                text,
                arc_len,
            } => {
                let handles = self.add_comment(points, &text, arc_len)?;
                return Ok(Outcome::Added { handles });
            }
            Command::ListComments => {
                return Ok(Outcome::Comments {
                    comments: self.list_comments(),
                });
            }
            Command::Dimension(spec) => {
                let handles = self.add_dimensions(spec)?;
                return Ok(Outcome::Added { handles });
            }
            Command::DimEdit { target, op } => {
                self.dim_edit(&target, &op)?;
                return Ok(Outcome::Changed);
            }
            Command::DimBreak { target, cutter } => {
                self.dim_break(&target, &cutter)?;
                return Ok(Outcome::Changed);
            }
            Command::DimSpace {
                base,
                targets,
                spacing,
            } => {
                self.dim_space(&base, &targets, spacing)?;
                return Ok(Outcome::Changed);
            }
            Command::DimJogLine { target, at } => {
                self.dim_jog_line(&target, at)?;
                return Ok(Outcome::Changed);
            }
            Command::Leader { points, text } => {
                let handles = self.add_leader(&points, &text, false)?;
                return Ok(Outcome::Added { handles });
            }
            Command::MLeader { points, text } => {
                let handles = self.add_leader(&points, &text, true)?;
                return Ok(Outcome::Added { handles });
            }
            Command::Tolerance { at, frame } => {
                let h = self.add_tolerance(at, &frame)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::AnnotationScale { name } => {
                self.set_annotation_scale(&name)?;
                return Ok(Outcome::Changed);
            }
            Command::SetDimStyle {
                name,
                fields,
                current,
            } => {
                self.set_dim_style(&name, &fields, current)?;
                return Ok(Outcome::Changed);
            }
            Command::Table {
                at,
                rows,
                cols,
                col_width,
                row_height,
                data,
            } => {
                let h = self.add_table(at, rows, cols, col_width, row_height, &data)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::EditCell {
                target,
                row,
                col,
                text,
            } => {
                self.edit_cell(&target, row, col, &text)?;
                return Ok(Outcome::Changed);
            }
            Command::InsertRow { target, at } => {
                self.insert_row(&target, at)?;
                return Ok(Outcome::Changed);
            }
            Command::InsertCol { target, at } => {
                self.insert_col(&target, at)?;
                return Ok(Outcome::Changed);
            }
            Command::DeleteRow { target, row } => {
                self.delete_row(&target, row)?;
                return Ok(Outcome::Changed);
            }
            Command::DeleteCol { target, col } => {
                self.delete_col(&target, col)?;
                return Ok(Outcome::Changed);
            }
            Command::MergeCells {
                target,
                first,
                last,
            } => {
                self.merge_cells(&target, first, last)?;
                return Ok(Outcome::Changed);
            }
            Command::Field { at, height, field } => {
                let h = self.add_field(at, height, &field)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::UpdateFields => {
                self.update_fields(false)?;
                return Ok(Outcome::Changed);
            }
            Command::CreateBlock {
                name,
                base,
                targets,
                convert_to_insert,
            } => return self.create_block(&name, base, &targets, convert_to_insert),
            Command::InsertBlock {
                name,
                at,
                scale,
                rotation,
                attribute_values,
                from_path,
            } => {
                let path = from_path.as_deref();
                let h = self.insert_block(&name, at, scale, rotation, &attribute_values, path)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::WriteBlock { name, path } => {
                self.write_block_in_folder(&name, &path)?;
                return Ok(Outcome::Changed);
            }
            Command::EditBlockInPlace { target } => {
                let handles = self.edit_block_in_place(&target)?;
                return Ok(Outcome::Added { handles });
            }
            Command::SaveBlockEdits => return self.close_block_edit(true),
            Command::DiscardBlockEdits => return self.close_block_edit(false),
            Command::AttDef {
                tag,
                prompt,
                default,
                at,
                height,
            } => {
                let h = self.add_attdef(&tag, &prompt, &default, at, height)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::EditAttributes { target, values } => {
                self.edit_attributes(&target, &values)?;
                return Ok(Outcome::Changed);
            }
            Command::EditAttDef {
                block,
                tag,
                prompt,
                default,
            } => {
                self.edit_attdef(&block, &tag, &prompt, &default)?;
                return Ok(Outcome::Changed);
            }
            Command::SyncAttributes { block } => {
                let (count, skipped) = self.sync_attributes(&block)?;
                return Ok(Self::updated(count, skipped));
            }
            Command::RedefineBlock { name, from_path } => {
                self.redefine_block(&name, &from_path)?;
                return Ok(Outcome::Changed);
            }
            Command::SaveLayerState { name } => {
                return self.save_layer_state(&name).map(|()| Outcome::Changed)
            }
            Command::RestoreLayerState { name } => {
                return self.restore_layer_state(&name).map(|()| Outcome::Changed)
            }
            Command::DeleteLayerState { name } => {
                return self.delete_layer_state(&name).map(|()| Outcome::Changed)
            }
            Command::RenameLayerState { from, to } => {
                return self
                    .rename_layer_state(&from, &to)
                    .map(|()| Outcome::Changed)
            }
            Command::SetLayer { name, prop, value } => {
                return self
                    .set_layer_prop(&name, prop, &value)
                    .map(|()| Outcome::Changed)
            }
            Command::NewLayer { name } => return self.new_layer(&name).map(|()| Outcome::Changed),
            Command::DeleteLayer { name } => {
                return self.delete_layer(&name).map(|()| Outcome::Changed)
            }
            Command::RenameLayer { from, to } => {
                return self.rename_layer(&from, &to).map(|()| Outcome::Changed)
            }
            Command::TranslateLayers {
                from_path,
                mappings,
            } => {
                return self
                    .translate_layers(from_path.as_deref(), &mappings)
                    .map(|()| Outcome::Changed)
            }
            Command::SetViewportFrozen {
                viewport,
                layer,
                frozen,
            } => {
                return self
                    .set_viewport_frozen(&viewport, &layer, frozen)
                    .map(|()| Outcome::Changed)
            }
            Command::CreateGroup { name, targets } => {
                return self
                    .create_group(&name, &targets)
                    .map(|()| Outcome::Changed)
            }
            Command::AddToGroup { name, targets } => {
                return self
                    .add_to_group(&name, &targets)
                    .map(|()| Outcome::Changed)
            }
            Command::RemoveFromGroup { name, targets } => {
                return self
                    .remove_from_group(&name, &targets)
                    .map(|()| Outcome::Changed)
            }
            Command::DeleteGroup { name } => {
                return self.delete_group(&name).map(|()| Outcome::Changed)
            }
            Command::Ungroup { targets } => {
                return self.ungroup(&targets).map(|()| Outcome::Changed)
            }
            Command::MatchProperties {
                source,
                targets,
                props,
            } => {
                self.match_properties(&source, &targets, &props)?;
                self.follow_fields()?;
                return Ok(Outcome::Changed);
            }
            Command::Ucs { origin, x_axis } => {
                self.ucs = crate::Ucs::new(origin, x_axis)?;
                return Ok(Outcome::Changed);
            }
            Command::UcsObject { target } => {
                self.ucs = self.ucs_of_entity(&target)?;
                return Ok(Outcome::Changed);
            }
            Command::Purge { kinds, dry_run } => {
                let report = self.purge(&kinds, dry_run)?;
                return Ok(Outcome::Purged { report });
            }
            Command::Audit { fix } => {
                let report = self.audit(fix)?;
                return Ok(Outcome::Audited { report });
            }
            Command::MView { corner1, corner2 } => {
                let h = self.mview(corner1, corner2)?;
                return Ok(Outcome::Added { handles: vec![h] });
            }
            Command::CopyToClipboard { targets, base } => {
                let (text, count) = self.copy_to_clipboard(&targets, base)?;
                return Ok(Outcome::Clipboard { text, count });
            }
            Command::PasteFromClipboard {
                at,
                text,
                as_block,
                height,
            } => {
                let handles = self.paste_from_clipboard(at, text.as_deref(), as_block, height)?;
                return Ok(Outcome::Added { handles });
            }
            Command::SetProperty {
                target,
                prop,
                value,
            } => {
                self.set_property(&target, prop, &value)?;
                self.follow_fields()?;
                return Ok(Outcome::Changed);
            }
            Command::AddConstraint {
                kind,
                targets,
                points,
            } => {
                let id = self.add_constraint(kind, &targets, &points)?;
                return Ok(Outcome::Constrained { ids: vec![id] });
            }
            Command::DeleteConstraint { id } => {
                self.delete_constraint(id)?;
                return Ok(Outcome::Constrained { ids: vec![id] });
            }
            Command::DeleteConstraintsOn { targets } => {
                let ids = self.delete_constraints_on(&targets)?;
                return Ok(Outcome::Constrained { ids });
            }
            Command::AutoConstrain { targets, settings } => {
                let ids = self.auto_constrain(&targets, &settings)?;
                return Ok(Outcome::Constrained { ids });
            }
            Command::AddDimConstraint {
                kind,
                targets,
                points,
                line_point,
                expression,
            } => {
                let id = self.add_dim_constraint(
                    kind,
                    &targets,
                    &points,
                    line_point,
                    expression.as_deref(),
                )?;
                return Ok(Outcome::Constrained { ids: vec![id] });
            }
            Command::SetParameter { name, expression } => {
                self.set_parameter(&name, &expression)?;
                return Ok(Outcome::Changed);
            }
            Command::Records { query } => {
                return Ok(Outcome::Records {
                    page: self.records(&query)?,
                });
            }
            Command::RecordSchema { name } => {
                return Ok(Outcome::RecordSchema {
                    schema: crate::record_edit::record_schema(&name)?,
                });
            }
            Command::SetRecords {
                collection,
                updates,
            } => return self.set_records(collection, updates),
            Command::CreateEntities { entities, sheet } => {
                return Ok(Outcome::Added {
                    handles: self.create_entities(entities, sheet)?,
                });
            }
            Command::ListConstraints => {
                return Ok(Outcome::Constraints {
                    constraints: self.list_constraints(),
                });
            }
            Command::ListParameters => {
                return Ok(Outcome::Parameters {
                    parameters: self.list_parameters(),
                });
            }
        };
        measurement.source = source.name().to_string();
        self.takeoff.add(measurement.clone());
        Ok(Outcome::Measured {
            measurement: Box::new(measurement),
        })
    }

    /// Runs `f` on the measurement with `id`: `Changed`, or `Nothing` when there is none.
    fn edit(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut Measurement) -> Result<(), CoreError>,
    ) -> Result<Outcome, CoreError> {
        match self.takeoff.items.iter_mut().find(|m| m.id == id) {
            Some(m) => f(m).map(|()| Outcome::Changed),
            None => Ok(Outcome::Nothing),
        }
    }

    /// The measurement of an `AddMeasurement`, without its description and note.
    #[allow(clippy::too_many_arguments)]
    fn add_measurement(
        &self,
        kind: Kind,
        value: Option<f64>,
        unit: Option<String>,
        height: Option<f64>,
        points: Vec<[f64; 2]>,
        handles: Vec<u64>,
        chain: Vec<u64>,
    ) -> Result<Measurement, CoreError> {
        if kind != Kind::Item && points.is_empty() && handles.is_empty() {
            return Err(invalid("A measurement needs geometry"));
        }
        if !points.is_empty() && !handles.is_empty() {
            return Err(invalid("Give points or handles, not both"));
        }
        if kind == Kind::Item {
            if !value.is_some_and(f64::is_finite) {
                return Err(invalid("An item needs a value"));
            }
            if unit.as_deref().is_none_or(|u| u.trim().is_empty()) {
                return Err(invalid("An item needs a unit"));
            }
        } else if value.is_some() || unit.is_some() {
            return Err(invalid(
                "Only an item takes a value and a unit; this is measured from its geometry",
            ));
        }
        match (kind, height) {
            (Kind::Volume, Some(h)) => check_height(h)?,
            (Kind::Volume, None) => return Err(invalid("A volume needs a height")),
            (_, Some(_)) => return Err(invalid("Only a volume takes a height")),
            (_, None) => {}
        }
        if !chain.is_empty() && handles.is_empty() {
            return Err(invalid("A chain goes with handles"));
        }
        let xrefs = self.xref_docs();
        let entities = handles
            .iter()
            .map(|&h| locate(&self.drawing.doc, &xrefs, h, &chain))
            .collect::<Result<Vec<_>, _>>()?;

        let mut self_intersecting = false;
        let value = match kind {
            Kind::Length if !points.is_empty() => {
                if points.len() < 2 {
                    return Err(invalid("A distance needs at least 2 points"));
                }
                Path {
                    points: points.clone(),
                    closed: false,
                }
                .length()
            }
            Kind::Length => entities
                .iter()
                .map(|e| {
                    self.extent(e.doc, e.entity, &e.xf)
                        .length
                        .ok_or_else(|| invalid("This entity has no length"))
                })
                .sum::<Result<f64, _>>()?,
            Kind::Area | Kind::Volume if !points.is_empty() => {
                if points.len() < 3 {
                    return Err(invalid("An area needs at least 3 points"));
                }
                let ring = Path {
                    points: points.clone(),
                    closed: true,
                };
                self_intersecting = ring.self_intersects();
                ring.area()
            }
            Kind::Area | Kind::Volume => entities
                .iter()
                .map(|e| {
                    let extent = self.extent(e.doc, e.entity, &e.xf);
                    self_intersecting |= extent.self_intersecting;
                    extent
                        .area
                        .ok_or_else(|| invalid("This entity has no area"))
                })
                .sum::<Result<f64, _>>()?,
            // One piece per point or per handle, whichever was given.
            Kind::Count => points.len().max(handles.len()) as f64,
            Kind::Item => value.unwrap_or_default(),
        };
        let value = match height {
            Some(h) => value * h,
            None => value,
        };
        let layer = entities.first().map_or("", |e| e.layer.as_str());
        let mut m = self.measurement(kind, value, "", layer);
        if kind == Kind::Item {
            m.unit = unit.unwrap_or_default();
        }
        if matches!(kind, Kind::Area | Kind::Volume) {
            m.area_origin = if points.is_empty() {
                crate::AreaOrigin::ExistingBoundary
            } else {
                crate::AreaOrigin::ManualOrEntity
            };
        }
        m.height = height;
        m.self_intersecting = self_intersecting;
        m.points = points;
        m.handles = handles;
        if let Some(first) = entities.first() {
            m.block_path = first.block_path.clone();
            m.xref = first.xref.clone();
        }
        if kind == Kind::Count && m.xref.is_some() {
            // An xref's handles are never embedded: keep where each reference sits instead.
            m.points = entities.iter().filter_map(insert_point).collect();
        }
        m.chain = chain;
        Ok(m)
    }

    /// A measurement on the current sheet in the drawing's units.
    fn measurement(&self, kind: Kind, value: f64, description: &str, layer: &str) -> Measurement {
        Measurement::new(
            kind,
            value,
            self.drawing.units,
            description.to_string(),
            layer.to_string(),
            self.sheet.clone(),
        )
    }

    fn measure_entity(&self, handle: u64, chain: Vec<u64>) -> Result<Measurement, CoreError> {
        let xrefs = self.xref_docs();
        let located = locate(&self.drawing.doc, &xrefs, handle, &chain)?;
        let entity = located.entity;
        let layer = &located.layer;
        let description = format!("{} on {}", variant_name(entity), short_layer_name(layer));
        let mut m = if let EntityType::Insert(insert) = entity {
            let mut m = self.measurement(Kind::Count, 1.0, &description, layer);
            m.block_name = Some(effective_block_name(located.doc, insert));
            m
        } else {
            let extent = self.extent(located.doc, entity, &located.xf);
            match (extent.area, extent.length) {
                (Some(area), _) => {
                    let mut m = self.measurement(Kind::Area, area, &description, layer);
                    m.area_origin = crate::AreaOrigin::ExistingBoundary;
                    m.self_intersecting = extent.self_intersecting;
                    m
                }
                (None, Some(length)) => self.measurement(Kind::Length, length, &description, layer),
                (None, None) => return Err(invalid("This entity has no length or area")),
            }
        };
        if m.kind == Kind::Count && located.xref.is_some() {
            // An xref's handles are never embedded: keep where the reference sits instead.
            m.points = insert_point(&located).into_iter().collect();
        }
        m.handles = vec![handle];
        m.block_path = located.block_path;
        m.xref = located.xref;
        m.chain = chain;
        Ok(m)
    }

    /// Counts top-level references only: a block nested inside another block is not counted yet.
    /// A dynamic block's anonymous copies count as the dynamic block. References at the top of
    /// the sheet's xrefs count too (on their `XREF|LAYER`): they are kept as their points, since
    /// their handles are the xref drawing's.
    fn count_block(&self, block_name: String, layer: Option<String>) -> Measurement {
        let block = normalize_name(&block_name);
        let only_on = layer.as_deref().map(normalize_name);
        let wanted = |layer: &str| only_on.as_ref().is_none_or(|l| *l == normalize_name(layer));
        let mut handles = Vec::new();
        let mut points = Vec::new();
        self.visit_countable_inserts(Some(&self.sheet), true, |insert, doc, on, xf| {
            if normalize_name(&effective_block_name(doc, insert)) != block || !wanted(on) {
                return;
            }
            if let Some(xf) = xf {
                let p = insert.insert_point;
                let on_plan = object_to_world(&insert.normal, p.z).apply([p.x, p.y]);
                points.push(xf.apply(on_plan));
            } else {
                handles.push(insert.common.handle.value());
            }
        });
        let description = format!("{block_name} count");
        let mut m = self.measurement(
            Kind::Count,
            (handles.len() + points.len()) as f64,
            &description,
            layer.as_deref().unwrap_or(""),
        );
        m.handles = handles;
        m.points = points;
        m.block_name = Some(block_name);
        m.count_scope = Some(crate::INSERT_COUNT_SCOPE.to_string());
        m.note = crate::INSERT_COUNT_LIMITATIONS.to_string();
        m
    }

    /// Removes the entities `targets` from the sheet on screen, once every one is found at its
    /// top.
    /// Images and PDF underlays leave their definitions' links; take-off rows measuring a
    /// deleted entity follow (`Outcome::TakeoffFollowed` says how, `Changed` when none did).
    fn delete_entities(&mut self, targets: &[Target]) -> Result<Outcome, CoreError> {
        let handles = self.sheet_targets(targets)?;
        for &h in &handles {
            let doc = &mut self.drawing.doc;
            match doc.get_entity(h) {
                Some(EntityType::RasterImage(image)) => {
                    if let Some(reactor) = image.definition_reactor_handle {
                        doc.objects.remove(&reactor);
                    }
                }
                Some(EntityType::Underlay(underlay)) => {
                    let definition = underlay.definition_handle;
                    if let Some(ObjectType::UnderlayDefinition(def)) =
                        doc.objects.get_mut(&definition)
                    {
                        def.reactors.retain(|r| *r != h);
                    }
                }
                _ => {}
            }
            self.drawing.doc.remove_entity_arc(h);
        }
        Ok(match self.follow_erase(&handles) {
            (0, 0) => Outcome::Changed,
            (remeasured, unlinked) => Outcome::TakeoffFollowed {
                remeasured,
                unlinked,
            },
        })
    }

    /// The handles of `targets` (each once, in order) when every one is an entity of the drawing
    /// itself at the top of the sheet on screen (an empty chain: not inside a block, not an
    /// xref's) on a layer that is not locked; otherwise why not. For the commands that change
    /// what they are given.
    pub(crate) fn sheet_targets(&self, targets: &[Target]) -> Result<Vec<Handle>, CoreError> {
        let handles = self.sheet_targets_any(targets)?;
        let doc = &self.drawing.doc;
        for &h in &handles {
            let layer = &doc.get_entity(h).expect("found above").common().layer;
            if doc.layers.get(layer).is_some_and(|l| l.flags.locked) {
                return Err(CoreError::InvalidCommand(format!(
                    "Entity {:X} is on the locked layer {layer}: unlock it first",
                    h.value()
                )));
            }
        }
        Ok(handles)
    }

    /// The entity `h` is locked: on a locked layer, or on the sheet outside the block edited in
    /// place. Commands that change entities they look for themselves (find and replace, sync
    /// attributes) leave it as it is and count it. Fields are not held by it: they are derived
    /// values (`update_fields`).
    pub(crate) fn is_locked(&self, h: Handle) -> bool {
        let doc = &self.drawing.doc;
        let on_locked_layer = doc.get_entity(h).is_some_and(|e| {
            doc.layers
                .get(&e.common().layer)
                .is_some_and(|l| l.flags.locked)
        });
        on_locked_layer
            || self
                .block_edit
                .as_ref()
                .is_some_and(|e| e.outside.contains(&h.value()))
    }

    /// `Outcome::Changed`, or `Updated` when `skipped` locked entities were left as they were.
    fn updated(count: usize, skipped: usize) -> Outcome {
        if skipped == 0 {
            Outcome::Changed
        } else {
            Outcome::Updated { count, skipped }
        }
    }

    /// `sheet_targets`, locked layers allowed: for commands that only read what they are given
    /// (a cutter, a boundary, what a dimension or a field measures) or don't change it (groups,
    /// a viewport's layer freezes).
    pub(crate) fn sheet_targets_any(&self, targets: &[Target]) -> Result<Vec<Handle>, CoreError> {
        if targets.is_empty() {
            return Err(invalid("Pick the entities first"));
        }
        let doc = &self.drawing.doc;
        let sheet = self.sheet_owner();
        let mut handles: Vec<Handle> = Vec::with_capacity(targets.len());
        let mut seen = std::collections::HashSet::with_capacity(targets.len());
        for Target { handle: h, chain } in targets {
            if !chain.is_empty() {
                return Err(CoreError::InvalidCommand(match xref_of_chain(doc, chain) {
                    Some(name) => {
                        format!("Entity {h:X} belongs to the xref {name}, which is read-only")
                    }
                    None => {
                        format!("Entity {h:X} is inside a block: pick the block reference instead")
                    }
                }));
            }
            let entity = doc
                .get_entity(Handle::new(*h))
                .ok_or(CoreError::NoSuchEntity(*h))?;
            if Some(entity.common().owner_handle) != sheet {
                return Err(CoreError::InvalidCommand(format!(
                    "Entity {h:X} is not on this sheet"
                )));
            }
            if let Some(edit) = self.block_edit.as_ref().filter(|e| e.outside.contains(h)) {
                return Err(CoreError::InvalidCommand(format!(
                    "Entity {h:X} is locked while block {} is edited in place",
                    edit.name
                )));
            }
            if seen.insert(*h) {
                handles.push(Handle::new(*h));
            }
        }
        Ok(handles)
    }
}

/// Where a located block reference sits in the world: its insertion point (in its own object
/// coordinates) placed by the chain. `None` for anything else.
pub(crate) fn insert_point(located: &Located) -> Option<[f64; 2]> {
    let EntityType::Insert(insert) = located.entity else {
        return None;
    };
    let p = insert.insert_point;
    let on_plan = object_to_world(&insert.normal, p.z).apply([p.x, p.y]);
    Some(located.xf.apply(on_plan))
}

/// A height must be a number above 0.
fn check_height(height: f64) -> Result<(), CoreError> {
    if height > 0.0 && height.is_finite() {
        Ok(())
    } else {
        Err(invalid("The height must be more than 0"))
    }
}

fn invalid(why: &str) -> CoreError {
    CoreError::InvalidCommand(why.to_string())
}
