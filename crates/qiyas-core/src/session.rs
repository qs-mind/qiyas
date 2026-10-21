use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use opencadcodec::entities::EntityCommon;
use opencadcodec::entities::Insert;
use opencadcodec::objects::ObjectType;
use opencadcodec::tables::normalize_name;
use opencadcodec::{BlockRecord, CadDocument, Color, DwgWriter, DxfWriter, EntityType, Handle};
use opencadkernel::geom2d::{ring_nesting_depths, Curve};
use qiyas_geom::{
    dimension_text, drawn_style, exact_area, exact_hatch_areas, exact_length, flatten_with,
    insert_instances, is_xref_block, snap_shapes, DimLook, Xform, XrefDocs, MAX_XREF_DEPTH,
};
use qiyas_render::{
    add_images, add_pick_outlines, build_batch_with_xrefs, decode_codes, layer_shown, place_text,
    resolve_color, Batch, PickIndex, TextEngine, TextOptions, Visibility, PATTERN, TEXT_BOX,
};
use serde::{Deserialize, Serialize};

use crate::blocks::{fade, BlockEdit};
use crate::commands::Part;
use crate::drawing::short_layer_name;
use crate::embed::{embed_measurements, read_embedded};
use crate::project::{self, project_path, Project};
use crate::underlay::{rebase_links, Underlays};
use crate::undo::{DocChange, History, Parts, Step, TablesSnapshot, ViewDelta, ViewRefs};
use crate::xref::{XrefInfo, XrefSet};
use crate::{
    Boq, Command, Conflict, CoreError, Current, DimStyleInfo, Drafting, Drawing, LayerInfo,
    Outcome, Schedule, Sheet, Source, TableInfo, Takeoff, Ucs, Units,
};

/// Flatten tolerance for drawing on screen in a millimetre drawing: 0.05 mm. Other units use the
/// same real size (`Session::batch_tol`), so a circle is as round in metres as in millimetres.
pub const BATCH_TOL: f64 = 0.05;

/// `BATCH_TOL`'s real size in drawing units of `units`: 0.05 mm whatever the drawing's units.
pub fn batch_tol_for(units: Units) -> f64 {
    BATCH_TOL * 0.001 / units.to_metres()
}

/// Zoomed in past `batch_tol`, curves on screen are flattened within this many screen pixels
/// (AutoCAD's automatic regen): never a visible corner on a circle, however close.
pub const ZOOM_TOL_PX: f64 = 0.25;

/// The tolerance the sheet on screen should draw curves within at `px_per_unit` screen pixels per
/// drawing unit, and whether `current` is far enough from it to draw them again: more than twice
/// as coarse (a corner could show), or eight times finer than needed (memory) while finer than
/// the drawing's own `base`. The window's `curvesNeedRedraw` asks the same.
pub fn zoom_tol(base: f64, current: f64, px_per_unit: f64) -> Option<f64> {
    if !(px_per_unit > 0.0 && px_per_unit.is_finite()) {
        return None;
    }
    let want = (ZOOM_TOL_PX / px_per_unit).min(base);
    let coarse = current > 2.0 * want;
    let fine = current < want / 8.0 && current < base;
    (coarse || fine).then_some(want)
}
/// What `Session::segments_near_in` finds.
pub(crate) struct Near {
    /// The entities the drawn segments there draw (handle and block chain, as `locate` takes them), once
    /// each.
    pub sources: Vec<(u64, Vec<u64>)>,
    /// The drawn segments (absolute `x0 y0 x1 y1`), each with the index of its source.
    pub drawn: Vec<(usize, [f64; 4])>,
    /// The square held the whole sheet: there is nothing further out.
    pub whole_sheet: bool,
}

/// Sheets `Session::with_sheet` keeps drawn besides the one on screen.
const OTHER_SHEETS: usize = 2;

/// Segments looked at past locked ones for a pick while a block is edited in place.
const PICK_PAST_LOCKED: usize = 4096;

/// Flatten tolerance for measured lengths and areas: 1 µm (0.001 in a millimetre drawing), the
/// same real size in every unit (`measure_tol_for`). Finer than `BATCH_TOL` so arcs measure close
/// to their true length.
pub(crate) const MEASURE_TOL_M: f64 = 1e-6;

/// `MEASURE_TOL_M` in drawing units of `units`.
pub(crate) fn measure_tol_for(units: Units) -> f64 {
    MEASURE_TOL_M / units.to_metres()
}

/// `MEASURE_TOL_M` in the drawing units of `doc` (its header's INSUNITS).
pub(crate) fn measure_tol_of(doc: &CadDocument) -> f64 {
    measure_tol_for(Units::from_header(doc.header.insertion_units))
}

/// Gives every opened session its own number, so the batch version of one drawing never equals
/// that of another opened later in the same window.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// The open drawing and what the user is looking at. Tauri-free, so it can be tested directly.
pub struct Session {
    pub drawing: Drawing,
    /// Block name of the sheet on screen, e.g. `*Model_Space`.
    pub sheet: String,
    pub visibility: Visibility,
    pub takeoff: Takeoff,
    /// The imported BOQ; `None` until one is imported.
    pub boq: Option<Boq>,
    /// The conflicts of the last comparison, with the user's statuses and notes.
    pub conflicts: Vec<Conflict>,
    /// The schedules read off every sheet at the last comparison.
    pub schedules: Vec<Schedule>,
    /// The drawing holds its Arabic text backwards (typed in visual order for old CAD fonts):
    /// draw every Arabic run reversed. Saved in the project file.
    pub arabic_reversed: bool,
    /// Snaps, ortho, polar, grid and display precision. Saved in the project file.
    pub drafting: Drafting,
    /// The drawing's xrefs, opened read-only.
    pub xrefs: XrefSet,
    /// The file the user chose for an xref, by xref name; looked at first. Saved in the project
    /// file.
    pub xref_overrides: HashMap<String, String>,
    /// The layer, colour, linetype and lineweight new entities take. Saved in the project file;
    /// not an undo step.
    pub current: Current,
    /// The hidden layers kept with each layer state, by state name. Saved in the project file.
    pub(crate) state_hidden: BTreeMap<String, Vec<String>>,
    /// The layer isolated on screen (LAYISO), with what isolating it hid; LAYUNISO puts back only
    /// that. Not an undo step, like hiding a layer.
    pub(crate) isolated: Option<crate::layers::Isolation>,
    /// What the last command or transaction did to the drawing's entities (`last_change`).
    pub(crate) last_change: crate::Change,
    /// How many transactions are open (`crate::transaction`).
    pub(crate) tx_depth: usize,
    /// The user coordinate system (see `crate::ucs`). Saved in the project file and the
    /// drawing; not an undo step.
    pub ucs: Ucs,
    pub(crate) history: History,
    /// The block being edited in place, if one is (see `crate::blocks`).
    pub(crate) block_edit: Option<BlockEdit>,
    /// The drawing whose autosave this one was recovered from; its first save deletes that
    /// autosave (see `crate::autosave`).
    pub(crate) recovered_from: Option<PathBuf>,
    /// Which session this is; see `NEXT_EPOCH`.
    epoch: u64,
    /// A new drawing never saved yet (`Session::new_blank`): its path is a placeholder, Save goes
    /// through Save As and no project file is written beside it.
    untitled: bool,
    /// The curve tolerance of the sheet on screen when zoomed in past `batch_tol` (`zoom_tol`);
    /// `None` draws at `batch_tol`.
    zoom_tol: std::cell::Cell<Option<f64>>,
    /// Counts the changes to the drawing itself (not the take-off), which is all a batch shows.
    pub(crate) document_revision: u64,
    /// `document_revision` at the last save (or the open): the drawing has unsaved changes
    /// when they differ.
    pub(crate) saved_revision: u64,
    /// `document_revision` at the last autosave attempt (see `crate::autosave`).
    pub(crate) autosave_revision: std::cell::Cell<u64>,
    /// Model space holds nothing (`EMPTY_MODEL` is then among the problems); looked at again on
    /// every drawing change.
    empty_model: bool,
    /// Batch and pick index of the last sheet and visibility drawn; rebuilt when either changes.
    cache: RefCell<Option<Cached>>,
    /// Sheets drawn besides the one on screen (model space inside a layout's viewports, the
    /// second view of a split window), at most `OTHER_SHEETS`, the latest last: each with the
    /// version it was drawn at (`version_of`); see `with_sheet`.
    others: RefCell<Vec<(String, u64, Batch)>>,
    /// Fonts, glyph atlas and glyph cache, kept across batches so the atlas pages the window
    /// holds stay valid.
    pub(crate) text: RefCell<TextEngine>,
    /// The pictures' (PDF underlays and raster images) image ids, PDF page sizes and problems.
    pub(crate) underlays: RefCell<Underlays>,
}

struct Cached {
    sheet: String,
    hidden_layers: HashSet<String>,
    arabic_reversed: bool,
    /// The curve tolerance it was flattened within (`screen_tol`).
    tol: f64,
    batch: Batch,
    index: PickIndex,
    /// The point entities on the sheet (top level, on shown layers), for node snaps: where, and
    /// their handle. They are not drawn, so the pick index can't find them.
    points: Vec<PointAt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DrawingInfo {
    pub path: String,
    pub sheets: Vec<Sheet>,
    pub layers: Vec<LayerInfo>,
    /// "inches", "feet", "millimetres", "centimetres", "metres" or "unknown".
    pub units: String,
    pub problems: Vec<String>,
    /// See `Session::arabic_reversed`.
    pub arabic_reversed: bool,
    /// The drawing's xrefs (and those inside them), in load order.
    pub xrefs: Vec<XrefInfo>,
    /// Block name of the sheet on screen.
    pub sheet: String,
    /// The layout's physical paper size in millimetres, including its rotation; none on Model.
    pub paper: Option<crate::Paper>,
    /// The layers the user has hidden, sorted.
    pub hidden_layers: Vec<String>,
    /// The layer isolated on screen (LAYISO), if one is.
    pub isolated_layer: Option<String>,
    /// What new entities are drawn with (`Session::current`).
    pub current: Current,
    /// The drawing's own text styles, sorted (`Session::text_styles`).
    pub text_styles: Vec<String>,
    /// The style new text takes (`Session::text_style`).
    pub text_style: String,
    /// The drawing's own dimension styles, sorted (`Session::dim_styles`).
    pub dim_styles: Vec<DimStyleInfo>,
    /// The style new dimensions take (`Session::dim_style`).
    pub dim_style: String,
    /// The annotation scale (CANNOSCALE), e.g. "1:50".
    pub annotation_scale: String,
    /// Drawing units per paper unit it gives (50 for "1:50"): new annotations are that many
    /// times their paper size.
    pub annotation_factor: f64,
    /// The block being edited in place (`Command::EditBlockInPlace`), if one is.
    pub block_edit: Option<String>,
    /// The drawing's linetypes, sorted.
    pub linetypes: Vec<String>,
    /// The drawing's layer states, as stored.
    pub layer_states: Vec<String>,
    /// The drawing's groups.
    pub groups: Vec<crate::GroupInfo>,
    /// The paper-space viewports of the sheet on screen, with the layers each freezes.
    pub viewports: Vec<crate::ViewportInfo>,
    /// The user coordinate system (`Session::ucs`).
    pub ucs: Ucs,
    /// The drawing has (or had) parametric constraints: its `QIYAS-PARAM` application is
    /// registered, so the window keeps the constraint bar's list read.
    pub has_constraints: bool,
    /// A new drawing never saved yet (`Session::is_untitled`): Save goes through Save As.
    pub untitled: bool,
}

/// An entity by its handle and the block references it sits inside (outermost first).
pub(crate) type EntityAt = (u64, Vec<u64>);
/// A point entity: where it is, and its handle.
pub(crate) type PointAt = ([f64; 2], u64);

/// In the problems of a drawing that opens with nothing in model space.
pub const EMPTY_MODEL: &str = "This drawing has no geometry in Model";
/// `Session::save` on a new drawing that has no file yet.
pub const UNTITLED: &str = "This drawing has not been saved yet: use Save As";

/// Shown when some block content was too deep or too large to draw.
const TRUNCATED: &str = "Drawing too complex, some block content not drawn";

/// What is under the cursor: the entity itself (`entity`), the block references it sits inside
/// (`chain`, outermost first, empty at the top level) and the top-level entity on the sheet
/// (`top`: the first of `chain`, or `entity` itself).
///
/// Inside an xref, `chain` ends with the xref's reference (it lists the host's references only),
/// `entity` is the entity at the top of the xref's model space (a handle of the xref drawing; a
/// block reference or a nested xref there is picked whole) and `xref` names the xref.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picked {
    pub top: u64,
    pub entity: u64,
    pub chain: Vec<u64>,
    /// Name of the xref the entity belongs to (the outermost, for a nested xref).
    #[serde(default)]
    pub xref: Option<String>,
    /// The kind of the entity at the top (`top`), e.g. "Line", "Insert", "Dimension".
    #[serde(default)]
    pub kind: Option<String>,
}

/// What the user sees about one picked entity. Length and area are in drawing units, in world
/// coordinates (an entity inside blocks is measured as placed by its block references).
#[derive(Debug, Clone, Serialize)]
pub struct EntityInfo {
    pub handle: u64,
    /// The entity type, e.g. "Line", "LwPolyline", "Insert", "Text".
    pub kind: String,
    /// The layer it is drawn on: its own, or for one on layer "0" inside a block, its insert's.
    pub layer: String,
    /// `layer` as shown to the QS: a bound xref's layer without its `XREF$0$` (see
    /// `short_layer_name`).
    pub layer_short: String,
    pub length: Option<f64>,
    /// Sum of the closed paths' areas; `None` when the entity has no closed path.
    pub area: Option<f64>,
    /// For a block reference, its block (for a dynamic block, the block it was made from).
    pub block_name: Option<String>,
    pub text: Option<String>,
    /// `text` as read: an MText's without its formatting codes, its lines split by `\n`.
    pub plain_text: Option<String>,
    /// The block the entity sits in, when it was picked inside one.
    pub inside_block: Option<String>,
    /// The block references the entity sits inside, outermost first; empty at the top level.
    pub chain: Vec<u64>,
    /// The block names along `chain`, outermost first.
    pub block_path: Vec<String>,
    /// The xref the entity belongs to; its `handle` is then one of that drawing's.
    pub xref: Option<String>,
    /// Its own colour: "ByLayer", "ByBlock", "None", "ACI n" or "#rrggbb".
    pub color: String,
    /// The colour it is drawn with: ByLayer takes its layer's, ByBlock its block reference's;
    /// white when nothing gives one.
    pub color_rgb: [u8; 3],
    /// Its linetype name, or "ByLayer".
    pub linetype: String,
    /// Its lineweight in hundredths of a millimetre; -1 ByLayer, -2 ByBlock, -3 Default.
    pub lineweight: i16,
    /// A circle's or an arc's radius in drawing units, as placed in the world.
    pub radius: Option<f64>,
    /// A block reference's attributes, as (tag, value).
    pub attributes: Vec<(String, String)>,
    /// A text's height, in drawing units.
    pub height: Option<f64>,
    /// A text's style name.
    pub style: Option<String>,
    /// A table's cells (`crate::table::TableInfo`).
    pub table: Option<TableInfo>,
    /// A text's, an MText's or a block reference's own rotation, in degrees.
    pub rotation: Option<f64>,
    /// A lightweight polyline is closed.
    pub closed: Option<bool>,
    /// A lightweight polyline's constant width, in drawing units.
    pub global_width: Option<f64>,
    /// A block reference's scale along its x axis, negative when it is mirrored.
    pub scale: Option<f64>,
    /// A paper-space viewport's scale (layout units per model unit) and the model point at its
    /// centre.
    pub viewport_scale: Option<f64>,
    pub viewport_centre: Option<[f64; 2]>,
    /// A line's or an arc's start and end points, in world coordinates (drawing units).
    pub start: Option<[f64; 2]>,
    pub end: Option<[f64; 2]>,
    /// A circle's, an arc's or an ellipse's centre, in world coordinates.
    pub centre: Option<[f64; 2]>,
    /// An arc's start and end angles in degrees, 0 up to 360, counter-clockwise in the world. An
    /// arc whose ends meet is a full turn (see `qiyas_geom::snap_shapes`).
    pub start_angle: Option<f64>,
    pub end_angle: Option<f64>,
    /// How many vertices a polyline has.
    pub vertex_count: Option<usize>,
    /// Where a text, an MText or a block reference is inserted, in world coordinates.
    pub position: Option<[f64; 2]>,
    /// A hatch's pattern name (SOLID for a solid fill).
    pub pattern: Option<String>,
}

/// An entity found by handle and chain, with what places it in the world.
pub(crate) struct Located<'a> {
    pub entity: &'a EntityType,
    /// The drawing the entity is in: the host, or the xref the chain ends in.
    pub doc: &'a CadDocument,
    /// The xref the entity belongs to, when the chain ends in an xref reference.
    pub xref: Option<String>,
    /// Block-to-world transform of the chain (identity at the top level).
    pub xf: Xform,
    /// The layer the entity is drawn on (layer "0" inside a block takes its insert's).
    pub layer: String,
    /// The block names along the chain, outermost first (dynamic blocks by their own name).
    pub block_path: Vec<String>,
}

/// Finds `handle` inside the block references `chain` (outermost first; empty for an entity
/// on a sheet). Each reference must sit in the block of the one before it, and the entity in
/// the last one's block. An array (MINSERT) is placed by its first cell. A chain may end with a
/// reference to an xref in `xrefs`: the entity is then one at the top of that xref's model space,
/// on layer `XREF|LAYER` (layer "0" takes the reference's).
pub(crate) fn locate<'a>(
    doc: &'a CadDocument,
    xrefs: &XrefDocs<'a>,
    handle: u64,
    chain: &[u64],
) -> Result<Located<'a>, CoreError> {
    let get = |h: u64| {
        doc.get_entity(Handle::new(h))
            .ok_or(CoreError::NoSuchEntity(h))
    };
    let mut xf = Xform::identity();
    let mut layer: Option<&str> = None;
    let mut block: Option<&str> = None;
    let mut block_path = Vec::new();
    let mut xref: Option<(&'a str, &'a CadDocument)> = None;
    for &h in chain {
        if xref.is_some() {
            return Err(CoreError::InvalidCommand(format!(
                "Entity {h} is inside an xref: an xref reference ends the chain"
            )));
        }
        let e = get(h)?;
        let EntityType::Insert(insert) = e else {
            return Err(CoreError::InvalidCommand(format!(
                "Entity {h} in the chain is not a block reference"
            )));
        };
        check_inside(doc, h, block)?;
        let placed = insert_instances(doc, insert, &xf).next();
        xf = placed.ok_or_else(|| {
            CoreError::InvalidCommand(format!("Block {} is missing", insert.block_name))
        })?;
        layer = Some(drawn_layer(&insert.common.layer, layer));
        block = Some(&insert.block_name);
        block_path.push(effective_block_name(doc, insert));
        if is_xref_block(doc, &insert.block_name) {
            let xdoc = xrefs
                .get(&normalize_name(&insert.block_name))
                .ok_or_else(|| {
                    CoreError::InvalidCommand(format!("Xref {} is not loaded", insert.block_name))
                })?;
            xref = Some((&insert.block_name, xdoc));
        }
    }
    let Some((name, xdoc)) = xref else {
        let entity = get(handle)?;
        check_inside(doc, handle, block)?;
        return Ok(Located {
            entity,
            doc,
            xref: None,
            xf,
            layer: drawn_layer(&entity.common().layer, layer).to_string(),
            block_path,
        });
    };
    let entity = xdoc
        .get_entity(Handle::new(handle))
        .ok_or(CoreError::NoSuchEntity(handle))?;
    check_inside(xdoc, handle, Some("*Model_Space"))?;
    let own = &entity.common().layer;
    let layer = match layer {
        Some(outer) if own == "0" => outer.to_string(),
        _ => format!("{name}|{own}"),
    };
    Ok(Located {
        entity,
        doc: xdoc,
        xref: Some(name.to_string()),
        xf,
        layer,
        block_path,
    })
}

/// The xref an entity picked with `chain` belongs to: the block of the chain's last reference,
/// when that is an xref.
pub(crate) fn xref_of_chain(doc: &CadDocument, chain: &[u64]) -> Option<String> {
    match doc.get_entity(Handle::new(*chain.last()?))? {
        EntityType::Insert(insert) if is_xref_block(doc, &insert.block_name) => {
            Some(insert.block_name.clone())
        }
        _ => None,
    }
}

/// An entity's layer as drawn: layer "0" inside a block takes the insert's.
fn drawn_layer<'a>(own: &'a str, insert: Option<&'a str>) -> &'a str {
    match insert {
        Some(outer) if own == "0" => outer,
        _ => own,
    }
}

/// Ok when `block` is `None` (no chain yet) or holds the entity `handle`.
fn check_inside(doc: &CadDocument, handle: u64, block: Option<&str>) -> Result<(), CoreError> {
    let Some(block) = block else {
        return Ok(());
    };
    let inside = doc
        .block_records
        .get(block)
        .is_some_and(|b| b.entity_handles.contains(&Handle::new(handle)));
    if inside {
        Ok(())
    } else {
        Err(CoreError::InvalidCommand(format!(
            "Entity {handle} is not inside block {block}"
        )))
    }
}

/// An entity's own colour as the properties palette shows it.
pub(crate) fn color_name(color: &Color) -> String {
    match color {
        Color::ByLayer => "ByLayer".into(),
        Color::ByBlock => "ByBlock".into(),
        Color::None => "None".into(),
        Color::Index(i) => format!("ACI {i}"),
        Color::Rgb { r, g, b } => format!("#{r:02x}{g:02x}{b:02x}"),
    }
}

/// The block an insert shows. An anonymous dynamic-block copy (`*U…`) is named after the
/// dynamic block it was made from, when the drawing links them; otherwise the insert's own name.
pub(crate) fn effective_block_name(doc: &CadDocument, insert: &Insert) -> String {
    let anonymous = insert
        .block_name
        .get(..2)
        .is_some_and(|p| p.eq_ignore_ascii_case("*U"));
    if anonymous && !doc.block_representations.is_empty() {
        if let Some(definition) = doc.dynamic_definition_for_insert(insert.common.handle) {
            if let Some(record) = doc.block_records.iter().find(|b| b.handle == definition) {
                return record.name.clone();
            }
        }
    }
    insert.block_name.clone()
}

impl Session {
    /// Opens a drawing on its model sheet with nothing hidden, with the take-off and BOQ from its
    /// project file when there is one, else from the measurements embedded in the drawing. A project file
    /// that can't be read is a problem, not a failure. When the project file exists it wins: the
    /// embedded measurements are ignored and rewritten on the next save. Then opens its xrefs
    /// (with the files the project chose for them); a missing xref is a problem, not a failure.
    /// Measurements embedded inside xrefs are never read.
    pub fn open(path: &Path) -> Result<Session, CoreError> {
        Session::open_with(path, &project_path(path), &project::file_name(path))
    }

    /// Opens a self-contained benchmark drawing without reading project sidecars or external
    /// resources. Linked drawings and pictures must be resolved before entering the benchmark.
    pub fn open_standalone(path: &Path) -> Result<Session, CoreError> {
        let drawing = Drawing::open(path)?;
        if drawing
            .doc
            .block_records
            .iter()
            .any(|b| is_xref_block(&drawing.doc, &b.name))
            || drawing.doc.objects.values().any(|o| {
                matches!(
                    o,
                    opencadcodec::objects::ObjectType::ImageDefinition(_)
                        | opencadcodec::objects::ObjectType::UnderlayDefinition(_)
                )
            })
            || drawing
                .doc
                .entities()
                .any(|e| matches!(e, EntityType::RasterImage(_) | EntityType::Underlay(_)))
        {
            return Err(CoreError::InvalidCommand(
                "Standalone drawings cannot reference external drawings, images or underlays"
                    .into(),
            ));
        }
        let mut session = Self::from_drawing(drawing, None);
        session.size_stock_dim_styles();
        Ok(session)
    }

    /// `open`, with the project file at `project_file`, saved for the drawing named `name`.
    pub(crate) fn open_with(
        path: &Path,
        project_file: &Path,
        name: &str,
    ) -> Result<Session, CoreError> {
        let mut session = Session::from_drawing(Drawing::open(path)?, Some((project_file, name)));
        session.size_stock_dim_styles();
        Ok(session)
    }

    /// A new, empty drawing (`Drawing::blank`) on its model sheet, with an empty take-off and the
    /// header's INSUNITS `insunits`. It has no file yet (Save goes through Save As,
    /// `is_untitled`) and stands at `placeholder` until then: the app's own folder, so its
    /// autosave has a home. Nothing to save until it changes, so closing it untouched asks nothing.
    pub fn new_blank(placeholder: &Path, insunits: i16) -> Session {
        let mut session = Session::from_drawing(Drawing::blank(placeholder, insunits), None);
        session.untitled = true;
        session.metric_template();
        session
    }

    /// A new drawing's annotation defaults, as AutoCAD's metric templates have them in paper
    /// millimetres, in this drawing's units, at an annotation scale of 1:100 (a building plan's):
    /// dimension text and arrows 2.5 mm on paper, extension offset 0.625, extension 1.25, gap
    /// 0.625, baseline spacing 3.75, text 2.5 mm x 100, linetype scale 100. So a dimension or text
    /// drawn at once is readable, whatever the units: 0.25 m in metres, 250 in millimetres.
    fn metric_template(&mut self) {
        const SCALE: f64 = 100.0;
        let mm = 0.001 / self.drawing.units.to_metres();
        let decimals = match self.drawing.units {
            Units::Metres => 2,
            Units::Centimetres => 1,
            _ => 0,
        };
        let doc = &mut self.drawing.doc;
        let current = doc.header.current_dimstyle_name.clone();
        let name = if doc.dim_styles.get(&current).is_some() {
            current
        } else {
            "Standard".to_string()
        };
        if let Some(style) = doc.dim_styles.get_mut(&name) {
            style.dimtxt = 2.5 * mm;
            style.dimasz = 2.5 * mm;
            style.dimexo = 0.625 * mm;
            style.dimexe = 1.25 * mm;
            style.dimgap = 0.625 * mm;
            style.dimdli = 3.75 * mm;
            style.dimcen = 2.5 * mm;
            style.dimscale = 1.0;
            style.dimdec = decimals;
        }
        let h = &mut doc.header;
        h.dim_text_height = 2.5 * mm;
        h.dim_arrow_size = 2.5 * mm;
        h.dim_ext_line_offset = 0.625 * mm;
        h.dim_ext_line_extension = 1.25 * mm;
        h.dim_line_gap = 0.625 * mm;
        h.dim_line_increment = 3.75 * mm;
        h.dim_center_mark = 2.5 * mm;
        h.dim_scale = 1.0;
        h.dim_decimal_places = decimals;
        h.text_height = 2.5 * mm * SCALE;
        h.linetype_scale = mm * SCALE;
        // A fresh drawing's variable dictionary always takes it.
        let _ = self.set_annotation_scale("1:100");
    }

    /// The session over a drawing just read (or made), with the project file `project` (its path
    /// and the drawing's name it should be saved for) when there is one to read.
    fn from_drawing(mut drawing: Drawing, project: Option<(&Path, &str)>) -> Session {
        let sheet = drawing.sheets()[0].block.clone();
        let mut takeoff = Takeoff::default();
        let mut boq = None;
        let mut conflicts = Vec::new();
        let mut schedules = Vec::new();
        let mut arabic_reversed = false;
        // The grid's dots a metre apart until the drawing's project file says otherwise (1 drawing
        // unit would be a millimetre grid, never shown, in a millimetre drawing).
        let mut drafting = Drafting {
            grid_spacing: 1.0 / drawing.units.to_metres(),
            ..Drafting::default()
        };
        let mut xref_overrides = HashMap::new();
        let mut current = Current::default();
        let mut state_hidden = BTreeMap::new();
        let mut ucs = None;
        if let Some((project_file, name)) = project.filter(|(file, _)| file.exists()) {
            match project::load(project_file) {
                Ok(project) => {
                    if !project.drawing.eq_ignore_ascii_case(name) {
                        drawing
                            .problems
                            .push(format!("Project file was saved for {}", project.drawing));
                    }
                    takeoff = project.takeoff;
                    boq = project.boq;
                    conflicts = project.conflicts;
                    schedules = project.schedules;
                    arabic_reversed = project.arabic_reversed;
                    drafting = project.drafting.clamped();
                    xref_overrides = project.xref_overrides;
                    state_hidden = project.qiyas_hidden;
                    ucs = project.ucs;
                    // A layer gone from the drawing since: back to layer "0".
                    if drawing.doc.layers.contains(&project.current.layer) {
                        current = project.current;
                    } else {
                        current = Current {
                            layer: "0".to_string(),
                            ..project.current
                        };
                    }
                }
                // Keep a copy, since the next save replaces the unreadable file.
                Err(e) => drawing.problems.push(match project::back_up(project_file) {
                    Ok(bak) => {
                        format!("Project file could not be read (backed up to {bak}): {e}")
                    }
                    Err(_) => format!("Project file could not be read: {e}"),
                }),
            }
        } else if project.is_some() {
            takeoff.items = read_embedded(&drawing.doc);
        }
        let xrefs = XrefSet::load(&drawing, &xref_overrides, MAX_XREF_DEPTH);
        let empty_model = model_is_empty(&drawing);
        let ucs = ucs
            .or_else(|| Ucs::from_header(&drawing.doc))
            .unwrap_or_default();
        Session {
            drawing,
            sheet,
            visibility: Visibility::default(),
            takeoff,
            boq,
            conflicts,
            schedules,
            arabic_reversed,
            drafting,
            xrefs,
            xref_overrides,
            current,
            state_hidden,
            isolated: None,
            last_change: crate::Change::default(),
            tx_depth: 0,
            ucs,
            history: History::default(),
            block_edit: None,
            recovered_from: None,
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            untitled: false,
            zoom_tol: std::cell::Cell::new(None),
            document_revision: 0,
            saved_revision: 0,
            autosave_revision: std::cell::Cell::new(0),
            empty_model,
            cache: RefCell::new(None),
            others: RefCell::new(Vec::new()),
            text: RefCell::new(TextEngine::new()),
            underlays: RefCell::new(Underlays::default()),
        }
    }

    /// Saves the take-off, the BOQ, the conflicts and the schedules to the drawing's project file
    /// and returns its path. An untitled drawing has none until its first Save As: nothing is
    /// written.
    pub fn save_project(&self) -> Result<PathBuf, CoreError> {
        let path = project_path(&self.drawing.path);
        if !self.untitled {
            project::save(&path, &self.project())?;
        }
        Ok(path)
    }

    /// The screen's flatten tolerance in this drawing's units: 0.05 mm (`batch_tol_for`).
    pub fn batch_tol(&self) -> f64 {
        batch_tol_for(self.drawing.units)
    }

    /// The measuring flatten tolerance in this drawing's units: 1 µm (`measure_tol_for`).
    pub(crate) fn measure_tol(&self) -> f64 {
        measure_tol_for(self.drawing.units)
    }

    /// What the sheet on screen flattens curves within now: `batch_tol`, or finer while zoomed
    /// in close (`zoom_tol`).
    pub fn screen_tol(&self) -> f64 {
        self.zoom_tol.get().unwrap_or_else(|| self.batch_tol())
    }

    /// Makes this an untitled drawing (see `new_blank`): an untitled drawing's autosave recovered
    /// after a crash is still one, Save going through Save As.
    pub fn mark_untitled(&mut self) {
        self.untitled = true;
    }

    /// A new drawing never saved yet (see `new_blank`).
    pub fn is_untitled(&self) -> bool {
        self.untitled
    }

    /// The project file's contents for the session as it is.
    pub(crate) fn project(&self) -> Project {
        Project {
            version: 2,
            drawing: project::file_name(&self.drawing.path),
            takeoff: self.takeoff.clone(),
            boq: self.boq.clone(),
            conflicts: self.conflicts.clone(),
            schedules: self.schedules.clone(),
            arabic_reversed: self.arabic_reversed,
            drafting: self.drafting.clone(),
            xref_overrides: self.xref_overrides.clone(),
            current: self.current.clone(),
            qiyas_hidden: self.state_hidden.clone(),
            ucs: Some(self.ucs),
        }
    }

    /// Saves the drawing over its own file; see `save_as`. Refused for an untitled drawing
    /// (`UNTITLED`): it has no file of its own yet.
    pub fn save(&mut self) -> Result<(), CoreError> {
        if self.untitled {
            return Err(CoreError::InvalidCommand(UNTITLED.to_string()));
        }
        let path = self.drawing.path.clone();
        self.save_as(&path)
    }

    /// Writes the drawing to `path` (DWG or DXF by extension, in the drawing's own version) with
    /// the take-off embedded on the `QIYAS-*` layers, then saves the project file next to it.
    /// Only the host drawing is written: xref files are never touched, and a measurement of an
    /// xref's entity is embedded from its points only.
    /// A file already at `path` is first copied to `<path>.bak`. From then on the session's
    /// drawing is the one at `path`; after a failed write it is unchanged.
    pub fn save_as(&mut self, path: &Path) -> Result<(), CoreError> {
        self.write_as(path, true)
    }

    /// `save_as` without the app (`qiyas_core::script`, `qiyas-cli`): the autosaves and lock
    /// files beside the drawings are the app's, holding work this session never saw, and are left
    /// as they are. Refused while a running Qiyas has `path` open (`refuse_if_open_elsewhere`).
    pub fn save_as_headless(&mut self, path: &Path) -> Result<(), CoreError> {
        crate::autosave::refuse_if_open_elsewhere(path)?;
        self.write_as(path, false)
    }

    fn write_as(&mut self, path: &Path, discard_autosaves: bool) -> Result<(), CoreError> {
        if let Some(name) = self.block_edit_name() {
            return Err(CoreError::InvalidCommand(format!(
                "Block {name} is being edited in place: save or discard the edit first (REFCLOSE)"
            )));
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        let dwg = match ext.as_deref() {
            Some("dwg") => true,
            Some("dxf") => false,
            _ => {
                return Err(CoreError::InvalidCommand(
                    "Save as .dwg or .dxf".to_string(),
                ))
            }
        };
        // Embedded into a copy, so a failed write leaves the drawing in memory as it was.
        let mut doc = self.drawing.doc.clone();
        embed_measurements(&mut doc, &self.takeoff.items);
        // AutoCAD opens with the same UCS.
        self.ucs.write_to(&mut doc);
        // PDF underlay and image links stay right from the new folder.
        rebase_links(&mut doc, &self.drawing.path, path);
        // Left out of the file only: an undo after the save may still need them.
        let purged = purge_for_save(&mut doc);
        let bytes = if dwg {
            DwgWriter::write_to_vec(&doc)
        } else {
            DxfWriter::new(&doc).write_to_vec()
        };
        restore_purged(&mut doc, purged);
        let bytes = bytes.map_err(|e| CoreError::Unwritable(e.to_string()))?;
        // Into a temp file first, so a failed save never damages the drawing already there.
        project::write_with_backup(path, &bytes)?;
        self.drawing.doc = doc;
        // The embedded entities are part of the drawing now: draw and pick them too.
        self.drawing_changed();
        self.saved_revision = self.document_revision;
        // Saved: the autosaves of the old file and the new one are stale (best effort).
        if discard_autosaves {
            let _ = crate::discard_autosave(&self.drawing.path);
            let _ = crate::discard_autosave(path);
            if let Some(from) = self.recovered_from.take() {
                let _ = crate::discard_autosave(&from);
            }
        }
        self.drawing.path = path.to_path_buf();
        self.untitled = false;
        self.save_project().map_err(|e| {
            CoreError::Unwritable(format!(
                "Drawing saved, but the project file could not be written: {e}"
            ))
        })?;
        Ok(())
    }

    /// The drawing's sheets, layers and units, the sheet on screen and the layers hidden, and what went wrong reading it or drawing the
    /// current sheet (builds the sheet's batch when it is not cached yet).
    pub fn info(&self) -> DrawingInfo {
        let mut problems = self.drawing.problems.clone();
        if self.empty_model && !self.untitled {
            problems.push(EMPTY_MODEL.to_string());
        }
        problems.extend(self.xrefs.problems().cloned());
        if self.with_cache(None, |c| c.batch.truncated) {
            problems.push(TRUNCATED.to_string());
        }
        problems.extend(self.underlays.borrow().problems.iter().cloned());
        let annotation_scale = self.annotation_scale();
        DrawingInfo {
            path: self.drawing.path.display().to_string(),
            sheets: self.drawing.sheets(),
            layers: self.layers(),
            units: units_name(self.drawing.units).to_string(),
            problems,
            arabic_reversed: self.arabic_reversed,
            xrefs: self.xrefs.info(&self.xref_overrides),
            sheet: self.sheet.clone(),
            paper: self.paper(),
            hidden_layers: self.hidden_layers(),
            isolated_layer: self.isolated.as_ref().map(|i| i.layer.clone()),
            current: self.current.clone(),
            text_styles: self.text_styles(),
            text_style: self.text_style(),
            dim_styles: self.dim_styles(),
            dim_style: self.dim_style(),
            annotation_scale: annotation_scale.0,
            annotation_factor: annotation_scale.1,
            block_edit: self.block_edit_name(),
            linetypes: self.linetypes(),
            layer_states: self.layer_state_names(),
            groups: self.groups(),
            viewports: self.viewports(),
            ucs: self.ucs,
            has_constraints: !self.needs_param_app(),
            untitled: self.untitled,
        }
    }

    /// The drawing's layers and those its xrefs add (`XREF|LAYER`), sorted by name. An xref
    /// layer the drawing keeps a copy of itself is listed once, as the drawing has it.
    pub fn layers(&self) -> Vec<LayerInfo> {
        let mut layers = self.drawing.layers();
        let own: HashSet<String> = layers.iter().map(|l| normalize_name(&l.name)).collect();
        layers.extend(
            self.xrefs
                .layers()
                .into_iter()
                .filter(|l| !own.contains(&normalize_name(&l.name))),
        );
        layers.sort_by(|a, b| a.name.cmp(&b.name));
        layers
    }

    /// The opened xref drawings by upper-case name, for drawing, measuring and searching.
    pub(crate) fn xref_docs(&self) -> XrefDocs<'_> {
        self.xrefs.docs()
    }

    /// Looks for every xref again and reopens it (a changed or newly found file shows).
    pub fn reload_xrefs(&mut self) {
        self.xrefs = XrefSet::load(&self.drawing, &self.xref_overrides, MAX_XREF_DEPTH);
        *self.cache.get_mut() = None;
    }

    /// Uses `path` for the xref `name` from now on (`None` goes back to looking for it), then
    /// reloads the xrefs. Saved in the project file. Fails when the drawing has no such xref.
    pub fn set_xref_path(&mut self, name: &str, path: Option<&Path>) -> Result<(), CoreError> {
        let Some(xref) = self.xrefs.get(name) else {
            return Err(CoreError::InvalidCommand(format!("No xref named {name}")));
        };
        let key = normalize_name(&xref.name);
        let name = xref.name.clone();
        self.xref_overrides.retain(|n, _| normalize_name(n) != key);
        if let Some(path) = path {
            self.xref_overrides.insert(name, path.display().to_string());
        }
        self.reload_xrefs();
        Ok(())
    }

    /// Shows the sheet whose block is `block` (case-insensitive).
    pub fn set_sheet(&mut self, block: &str) -> Result<(), CoreError> {
        let sheet = self
            .drawing
            .sheets()
            .into_iter()
            .find(|s| s.block.eq_ignore_ascii_case(block))
            .ok_or_else(|| CoreError::NoSuchSheet(block.to_string()))?;
        self.sheet = sheet.block;
        Ok(())
    }

    /// Hides or shows a layer on top of the drawing's own layer state. Names match case-insensitively.
    pub fn set_layer_hidden(&mut self, name: &str, hidden: bool) {
        let key = normalize_name(name);
        // The user chose for this layer: un-isolating leaves it as they set it.
        if let Some(iso) = &mut self.isolated {
            iso.hid.retain(|n| normalize_name(n) != key);
        }
        let layers = &mut self.visibility.hidden_layers;
        layers.retain(|n| normalize_name(n) != key);
        if hidden {
            layers.insert(name.to_string());
        }
    }

    /// The layers the user has hidden, sorted.
    pub fn hidden_layers(&self) -> Vec<String> {
        let mut names: Vec<String> = self.visibility.hidden_layers.iter().cloned().collect();
        names.sort();
        names
    }

    /// Line segments and text glyphs of the current sheet with the current visibility.
    pub fn batch(&self) -> Batch {
        self.with_cache(None, |cached| cached.batch.clone())
    }

    /// What the sheet on screen draws, `[[min_x, min_y], [max_x, max_y]]`; zeros when nothing.
    pub(crate) fn sheet_bounds(&self) -> [[f64; 2]; 2] {
        self.with_cache(None, |cached| cached.batch.bounds)
    }

    /// The batch of the sheet `sheet` (block name), on screen or not, as `with_sheet` draws it
    /// (pictures only on the sheet on screen); for the PDF plot.
    pub fn sheet_batch(&self, sheet: &str) -> Batch {
        self.with_sheet(sheet, None, Batch::clone)
    }

    /// The font file behind `ShapedGlyph::font` id `id` in this session's batches.
    pub fn font_bytes(&self, id: u16) -> Option<&'static [u8]> {
        self.text.borrow().font_bytes(id)
    }

    /// The sheet on screen drawn afresh for a picture at `px_per_unit` pixels per drawing unit
    /// (Mizan's view_image): its glyphs placed for that zoom, curves within half a pixel (never
    /// coarser than `batch_tol`), and a copy of the glyph atlas pages its quads draw from. Nothing
    /// is kept, so the window's batch, its glyphs and its curves are left as they are.
    pub fn picture_batch(&self, px_per_unit: f64) -> (Batch, Vec<Vec<u8>>) {
        self.picture_batch_of(&self.sheet, px_per_unit)
    }

    /// `picture_batch` for the sheet `sheet` (block name), on screen or not: a layout's own
    /// drawing, or model space for the viewports of one. Lines, fills and text only; the
    /// pictures are `plot_pictures`'.
    pub fn picture_batch_of(&self, sheet: &str, px_per_unit: f64) -> (Batch, Vec<Vec<u8>>) {
        let px = if px_per_unit > 0.0 && px_per_unit.is_finite() {
            px_per_unit
        } else {
            1.0
        };
        let options = TextOptions {
            arabic_reversed: self.arabic_reversed,
            px_per_unit: Some(px),
        };
        let mut text = self.text.borrow_mut();
        let batch = build_batch_with_xrefs(
            &self.drawing.doc,
            &self.xref_docs(),
            sheet,
            &self.visibility,
            (0.5 / px).min(self.batch_tol()),
            &mut text,
            options,
        );
        let pages = text.atlas_pages(None).map(|p| p.data.to_vec()).collect();
        (batch, pages)
    }

    /// The sheet `sheet` (block name) drawn afresh with curves flattened within `tol` drawing
    /// units, for the PDF plot (no pictures, nothing kept).
    pub fn sheet_batch_with_tol(&self, sheet: &str, tol: f64) -> Batch {
        let options = TextOptions {
            arabic_reversed: self.arabic_reversed,
            px_per_unit: None,
        };
        build_batch_with_xrefs(
            &self.drawing.doc,
            &self.xref_docs(),
            sheet,
            &self.visibility,
            tol,
            &mut self.text.borrow_mut(),
            options,
        )
    }

    /// Names what `batch` would give: it changes when the sheet, the hidden layers, the Arabic
    /// setting, the drawing itself (every drawing command, its undo and its redo) or its xrefs
    /// change, and not for take-off, BOQ or conflict changes or their undo or redo. Not for the zoom the glyphs are drawn for either: the window asks
    /// for that. A 64-bit FNV-1a hash of the session, sheet, hidden layers (sorted, case-folded),
    /// Arabic setting, document revision and xrefs (name, file found, when it was last changed
    /// and its size, each), cut to 53 bits so it survives a trip through a JavaScript number.
    pub fn batch_version(&self) -> u64 {
        let version = self.version_of(&self.sheet);
        // Finer curves for a close zoom are another batch.
        match self.zoom_tol.get() {
            None => version,
            Some(tol) => {
                let mut hash = Fnv::new();
                hash.write(&version.to_le_bytes());
                hash.write(&tol.to_bits().to_le_bytes());
                hash.0 & ((1 << 53) - 1)
            }
        }
    }

    /// `batch_version` for the sheet `sheet`.
    pub(crate) fn version_of(&self, sheet: &str) -> u64 {
        let mut hash = Fnv::new();
        hash.write(&self.epoch.to_le_bytes());
        hash.write(sheet.as_bytes());
        hash.write(&[0xFF]);
        let mut hidden: Vec<String> = self
            .visibility
            .hidden_layers
            .iter()
            .map(|n| normalize_name(n))
            .collect();
        hidden.sort();
        for name in hidden {
            hash.write(name.as_bytes());
            hash.write(&[0xFF]);
        }
        hash.write(&[u8::from(self.arabic_reversed)]);
        hash.write(&self.document_revision.to_le_bytes());
        for x in &self.xrefs.entries {
            hash.write(x.name.as_bytes());
            hash.write(&[0xFF]);
            if let Some(found) = &x.found {
                hash.write(found.as_os_str().as_encoded_bytes());
            }
            hash.write(&[0xFF, u8::from(x.drawing.is_some())]);
            let modified = x
                .modified
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            hash.write(&modified.to_le_bytes());
            hash.write(&x.size.unwrap_or(u64::MAX).to_le_bytes());
        }
        hash.0 & ((1 << 53) - 1)
    }

    /// The batch as `Batch::to_bytes` lays it out, or `None` when `if_version` is the current
    /// `batch_version`, so the caller keeps the batch it already holds. `px_per_unit` is the
    /// zoom to draw the glyphs for (screen pixels per drawing unit); `None` keeps the zoom of the
    /// batch in hand, or fits the sheet on a new one.
    pub fn batch_bytes_if(
        &self,
        if_version: Option<u64>,
        px_per_unit: Option<f64>,
    ) -> Option<Vec<u8>> {
        if if_version == Some(self.batch_version()) {
            return None;
        }
        Some(self.with_cache(px_per_unit, |cached| cached.batch.to_bytes()))
    }

    /// The glyph atlas pages changed after version `since` (all when `None`), as
    /// `Atlas::to_bytes` lays them out.
    pub fn atlas_bytes(&self, since: Option<u64>) -> Vec<u8> {
        self.text.borrow().atlas_bytes(since)
    }

    /// The entities at the top of the sheet that can't be picked: all but the block's while one
    /// is edited in place.
    fn locked(&self) -> Option<&HashSet<u64>> {
        self.block_edit.as_ref().map(|e| &e.outside)
    }

    /// The entity nearest to `p` (drawing coordinates) within `tol`, among what is drawn, down
    /// to the entity inside any blocks (while a block is edited in place, among its entities).
    /// On a layout, a click inside a viewport with nothing of the layout near it picks the
    /// viewport (never the model entities it shows).
    pub fn pick(&self, p: [f64; 2], tol: f64) -> Option<Picked> {
        self.pick_drawn(p, tol).or_else(|| self.viewport_at(p))
    }

    /// `pick` among what the sheet on screen draws.
    fn pick_drawn(&self, p: [f64; 2], tol: f64) -> Option<Picked> {
        self.with_cache(None, |cached| {
            let i = match self.locked() {
                None => cached.index.pick(p, tol)?,
                // While a block is edited in place, the nearest of its entities only.
                Some(locked) => cached
                    .index
                    .near(p, tol, PICK_PAST_LOCKED)
                    .into_iter()
                    .find(|&i| {
                        cached
                            .batch
                            .range_of(i)
                            .is_some_and(|r| !locked.contains(&r.top))
                    })?,
            };
            let range = cached.batch.range_of(i)?;
            let chain = cached.batch.chain_of(range).to_vec();
            Some(Picked {
                top: range.top,
                entity: range.entity,
                xref: xref_of_chain(&self.drawing.doc, &chain),
                chain,
                kind: self.kind_of(range.top),
            })
        })
    }

    /// The entities a selection box picks on the sheet on screen, among what is drawn: with
    /// `crossing`, each with a drawn segment inside or crossing `bounds` (`[[min_x, min_y],
    /// [max_x, max_y]]`, drawing coordinates); otherwise each drawn wholly inside it. Always the
    /// entity at the top of the sheet: a block reference, or an xref's reference, is picked whole
    /// (never what is inside it), so `entity == top` and `chain` is empty. Sorted by handle. While a
    /// block is edited in place, only its entities.
    pub fn pick_window(&self, bounds: [[f64; 2]; 2], crossing: bool) -> Vec<Picked> {
        self.with_cache(None, |cached| {
            let batch = &cached.batch;
            let [ox, oy] = batch.origin;
            // In the batch's own coordinates, as its segments are.
            let lo = [bounds[0][0] - ox, bounds[0][1] - oy];
            let hi = [bounds[1][0] - ox, bounds[1][1] - oy];
            let segment = |i: usize| -> [f64; 4] {
                let s = &batch.segments[4 * i..4 * i + 4];
                [s[0], s[1], s[2], s[3]].map(f64::from)
            };
            let near = cached
                .index
                .within(bounds[0], bounds[1], usize::MAX, |_| true);
            let mut tops = std::collections::BTreeSet::new();
            if crossing {
                for i in near {
                    if crosses_box(segment(i), lo, hi) {
                        tops.extend(batch.range_of(i).map(|r| r.top));
                    }
                }
            } else {
                let inside =
                    |p: [f64; 2]| lo[0] <= p[0] && p[0] <= hi[0] && lo[1] <= p[1] && p[1] <= hi[1];
                let reached: HashSet<u64> = near
                    .into_iter()
                    .filter_map(|i| batch.range_of(i))
                    .map(|r| r.top)
                    .collect();
                let mut outside = HashSet::new();
                for r in batch.ranges.iter().filter(|r| reached.contains(&r.top)) {
                    let out = (r.first..r.first + r.count).any(|i| {
                        let [x0, y0, x1, y1] = segment(i as usize);
                        !inside([x0, y0]) || !inside([x1, y1])
                    });
                    if out {
                        outside.insert(r.top);
                    }
                }
                tops.extend(reached.difference(&outside));
            }
            self.picked_tops(tops)
        })
    }

    /// The entities a selection polygon picks on the sheet on screen, as `pick_window` does for
    /// a box: `points` (drawing coordinates, three corners or more, either way round, concave
    /// too) with `crossing` takes each entity with a drawn segment inside or crossing it, else
    /// each drawn wholly inside it. Whole entities at the top of the sheet, sorted by handle.
    pub fn pick_polygon(&self, points: &[[f64; 2]], crossing: bool) -> Vec<Picked> {
        let mut run: crate::PointRun<true> = points.to_vec().into();
        run.normalize(self.drawing.units, true);
        let points = run.as_slice();
        if points.len() < 3 || points.iter().flatten().any(|v| !v.is_finite()) {
            return Vec::new();
        }
        let lo = points
            .iter()
            .fold([f64::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        let hi = points
            .iter()
            .fold([f64::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
        self.with_cache(None, |cached| {
            let batch = &cached.batch;
            let [ox, oy] = batch.origin;
            // In the batch's own coordinates, as its segments are.
            let ring: Vec<[f64; 2]> = points.iter().map(|p| [p[0] - ox, p[1] - oy]).collect();
            let segment = |i: usize| -> [f64; 4] {
                let s = &batch.segments[4 * i..4 * i + 4];
                [s[0], s[1], s[2], s[3]].map(f64::from)
            };
            let near = cached.index.within(lo, hi, usize::MAX, |_| true);
            let mut tops = std::collections::BTreeSet::new();
            if crossing {
                for i in near {
                    if touches_ring(segment(i), &ring) {
                        tops.extend(batch.range_of(i).map(|r| r.top));
                    }
                }
            } else {
                let reached: HashSet<u64> = near
                    .into_iter()
                    .filter_map(|i| batch.range_of(i))
                    .map(|r| r.top)
                    .collect();
                let mut outside = HashSet::new();
                for r in batch.ranges.iter().filter(|r| reached.contains(&r.top)) {
                    let out = (r.first..r.first + r.count)
                        .any(|i| !inside_ring(segment(i as usize), &ring));
                    if out {
                        outside.insert(r.top);
                    }
                }
                tops.extend(reached.difference(&outside));
            }
            self.picked_tops(tops)
        })
    }

    /// `tops` as picks of whole entities at the top of the sheet, without those a block edit in
    /// place has locked.
    fn picked_tops(&self, mut tops: std::collections::BTreeSet<u64>) -> Vec<Picked> {
        if let Some(locked) = self.locked() {
            tops.retain(|top| !locked.contains(top));
        }
        tops.into_iter()
            .map(|top| Picked {
                top,
                entity: top,
                chain: Vec::new(),
                xref: None,
                kind: self.kind_of(top),
            })
            .collect()
    }

    /// The kind of the drawing's entity `handle` ("Line", "Insert"…).
    pub(crate) fn kind_of(&self, handle: u64) -> Option<String> {
        self.drawing
            .doc
            .get_entity(Handle::new(handle))
            .map(variant_name)
    }

    /// Up to `max_entities` drawn entities with a segment within `tol` of `p`, nearest first (by
    /// handle and block-reference chain; text boxes left out), looking at no more than
    /// `max_segments` segments; and the sheet's point entities within `tol`.
    pub(crate) fn near_entities(
        &self,
        p: [f64; 2],
        tol: f64,
        max_entities: usize,
        max_segments: usize,
    ) -> (Vec<EntityAt>, Vec<PointAt>) {
        self.with_cache(None, |cached| {
            let mut entities: Vec<EntityAt> = Vec::new();
            for i in cached.index.near(p, tol, max_segments) {
                let Some(range) = cached.batch.range_of(i) else {
                    continue;
                };
                if range.flags & TEXT_BOX != 0 {
                    continue;
                }
                let chain = cached.batch.chain_of(range);
                if entities
                    .iter()
                    .any(|(h, c)| *h == range.entity && c == chain)
                {
                    continue;
                }
                entities.push((range.entity, chain.to_vec()));
                if entities.len() == max_entities {
                    break;
                }
            }
            let points = cached
                .points
                .iter()
                .filter(|(q, _)| (q[0] - p[0]).hypot(q[1] - p[1]) <= tol)
                .copied()
                .collect();
            (entities, points)
        })
    }

    /// The entities of `sheet` whose drawn segments reach into the square `radius` around `p`,
    /// text boxes and hatch pattern lines left out; `None` when more than `max` segments do.
    pub(crate) fn segments_near_in(
        &self,
        sheet: &str,
        p: [f64; 2],
        radius: f64,
        max: usize,
    ) -> Option<Near> {
        let near = |batch: &Batch, index: &PickIndex| {
            let lo = [p[0] - radius, p[1] - radius];
            let hi = [p[0] + radius, p[1] + radius];
            // Counted after text boxes and pattern lines go: a densely hatched room nearby
            // must not refuse a plain one. The tree walk stops at the cap all the same.
            let found = index.within(lo, hi, max + 1, |i| {
                batch
                    .range_of(i)
                    .is_some_and(|r| r.flags & (TEXT_BOX | PATTERN) == 0)
            });
            if found.len() > max {
                return None;
            }
            let [b0, b1] = batch.bounds;
            let whole_sheet = lo[0] <= b0[0] && lo[1] <= b0[1] && hi[0] >= b1[0] && hi[1] >= b1[1];
            let [ox, oy] = batch.origin;
            let mut sources: Vec<(u64, Vec<u64>)> = Vec::new();
            let mut seen: HashMap<(u64, u32), usize> = HashMap::new();
            let mut drawn = Vec::with_capacity(found.len());
            for i in found {
                let Some(r) = batch.range_of(i) else {
                    continue;
                };
                let source = *seen.entry((r.entity, r.chain)).or_insert_with(|| {
                    sources.push((r.entity, batch.chain_of(r).to_vec()));
                    sources.len() - 1
                });
                let s = &batch.segments[4 * i..4 * i + 4];
                let at = |k: usize, o: f64| f64::from(s[k]) + o;
                drawn.push((source, [at(0, ox), at(1, oy), at(2, ox), at(3, oy)]));
            }
            Some(Near {
                sources,
                drawn,
                whole_sheet,
            })
        };
        if self.sheet.eq_ignore_ascii_case(sheet) {
            self.with_cache(None, |cached| near(&cached.batch, &cached.index))
        } else {
            self.with_sheet(sheet, None, |batch| near(batch, &PickIndex::new(batch)))
        }
    }

    /// The entity `handle` inside the block references `chain` (outermost first; empty for an
    /// entity on a sheet), measured where those references place it. A chain ending in an xref
    /// reference names an entity of that xref.
    pub fn entity_info(&self, handle: u64, chain: &[u64]) -> Result<EntityInfo, CoreError> {
        let xrefs = self.xref_docs();
        let located = locate(&self.drawing.doc, &xrefs, handle, chain)?;
        let entity = located.entity;
        let Extent { length, area, .. } = self.extent(located.doc, entity, &located.xf);
        // A block reference is counted: the sum of its lines would only confuse.
        let (length, area) = match entity {
            EntityType::Insert(_) => (None, None),
            _ => (length, area),
        };
        let (block_name, text) = match entity {
            EntityType::Insert(insert) => (Some(effective_block_name(located.doc, insert)), None),
            EntityType::Text(t) => (None, Some(t.value.clone())),
            EntityType::MText(t) => (None, Some(t.value.clone())),
            // As drawn: %%c, %%d and the like as their signs.
            EntityType::Dimension(d) => {
                let text = dimension_text(d, &DimLook::of(located.doc, d));
                (None, Some(decode_codes(&text).into_owned()))
            }
            _ => (None, None),
        };
        let plain_text = match entity {
            EntityType::MText(t) => Some(
                opencadcodec::entities::mtext_format::parse_mtext(&t.value, true).to_plain_text(),
            ),
            _ => text.clone(),
        };
        let (height, style) = match entity {
            EntityType::Text(t) => (Some(t.height), Some(t.style.clone())),
            EntityType::MText(t) => (Some(t.height), Some(t.style.clone())),
            EntityType::Dimension(d) => (
                Some(crate::props::dimension_text_height(located.doc, d)),
                Some(d.base().style_name.clone()),
            ),
            _ => (None, None),
        };
        let attributes = match entity {
            EntityType::Insert(insert) => insert
                .attributes
                .iter()
                .map(|a| (a.tag.clone(), a.value.clone()))
                .collect(),
            _ => Vec::new(),
        };
        // The exact curve of a line, circle, arc or ellipse, placed in the world.
        let piece = match entity {
            EntityType::Line(_)
            | EntityType::Circle(_)
            | EntityType::Arc(_)
            | EntityType::Ellipse(_) => {
                snap_shapes(located.doc, entity, &located.xf, self.measure_tol())
                    .into_iter()
                    .next()
                    .and_then(|shape| shape.pieces.into_iter().next())
            }
            _ => None,
        };
        let radius = match &piece {
            Some(Curve::Circle(c)) => Some(c.radius),
            Some(Curve::Arc(a)) => Some(a.radius),
            _ => None,
        };
        let (start, end) = match &piece {
            Some(Curve::Line(l)) => (Some(l.start), Some(l.end)),
            Some(Curve::Arc(a)) => {
                let at = |t: f64| {
                    [
                        a.centre[0] + a.radius * t.cos(),
                        a.centre[1] + a.radius * t.sin(),
                    ]
                };
                (Some(at(a.start_angle)), Some(at(a.end_angle)))
            }
            _ => (None, None),
        };
        let centre = match &piece {
            Some(Curve::Circle(c)) => Some(c.centre),
            Some(Curve::Arc(a)) => Some(a.centre),
            Some(Curve::Ellipse(e)) => Some(e.ellipse.centre),
            _ => None,
        };
        let degrees = |t: f64| t.to_degrees().rem_euclid(360.0);
        let (start_angle, end_angle) = match &piece {
            Some(Curve::Arc(a)) => (Some(degrees(a.start_angle)), Some(degrees(a.end_angle))),
            _ => (None, None),
        };
        let vertex_count = match entity {
            EntityType::LwPolyline(p) => Some(p.vertices.len()),
            EntityType::Polyline2D(p) => Some(p.vertices.len()),
            EntityType::Polyline3D(p) => Some(p.vertices.len()),
            EntityType::Polyline(p) => Some(p.vertices.len()),
            _ => None,
        };
        let position = match entity {
            EntityType::Insert(_) => crate::commands::insert_point(&located),
            EntityType::Text(t) => {
                Some(located.xf.apply([t.insertion_point.x, t.insertion_point.y]))
            }
            EntityType::MText(t) => {
                Some(located.xf.apply([t.insertion_point.x, t.insertion_point.y]))
            }
            _ => None,
        };
        let pattern = match entity {
            EntityType::Hatch(h) => Some(if h.is_solid {
                "SOLID".to_string()
            } else {
                h.pattern.name.clone()
            }),
            _ => None,
        };
        let rotation = match entity {
            EntityType::Text(t) => Some(t.rotation.to_degrees()),
            EntityType::MText(t) => Some(t.rotation.to_degrees()),
            EntityType::Insert(i) => Some(i.rotation.to_degrees()),
            _ => None,
        };
        let (closed, global_width) = match entity {
            EntityType::LwPolyline(p) => (Some(p.is_closed), Some(p.constant_width)),
            _ => (None, None),
        };
        // Its x scale, negative when the reference is mirrored (x and y of opposite signs).
        let scale = match entity {
            EntityType::Insert(i) => Some(i.x_scale().abs() * (i.x_scale() * i.y_scale()).signum()),
            _ => None,
        };
        let (viewport_scale, viewport_centre) = match entity {
            EntityType::Viewport(v) if v.id != 1 => (
                Some(v.scale()),
                Some([
                    v.view_target.x + v.view_center.x,
                    v.view_target.y + v.view_center.y,
                ]),
            ),
            _ => (None, None),
        };
        let common = entity.common();
        let linetype = if common.linetype.is_empty() {
            "ByLayer".to_string()
        } else {
            common.linetype.clone()
        };
        let color_rgb = self.drawn_color(common, chain, located.xref.as_deref());
        Ok(EntityInfo {
            handle,
            kind: variant_name(entity),
            layer_short: short_layer_name(&located.layer).to_string(),
            layer: located.layer,
            length,
            area,
            block_name,
            text,
            plain_text,
            inside_block: located.block_path.last().cloned(),
            chain: chain.to_vec(),
            color: color_name(&common.color),
            color_rgb,
            linetype,
            lineweight: common.line_weight.value(),
            radius,
            attributes,
            height,
            style,
            table: match entity {
                EntityType::Table(t) => Some(crate::table::table_info(t)),
                _ => None,
            },
            block_path: located.block_path,
            xref: located.xref,
            rotation,
            closed,
            global_width,
            scale,
            viewport_scale,
            viewport_centre,
            start,
            end,
            centre,
            start_angle,
            end_angle,
            vertex_count,
            position,
            pattern,
        })
    }

    /// The colour the entity `common` inside the block references `chain` is drawn with,
    /// resolved as the renderer resolves it (`drawn_style`); white when nothing gives one.
    fn drawn_color(&self, common: &EntityCommon, chain: &[u64], xref: Option<&str>) -> [u8; 3] {
        let doc = &self.drawing.doc;
        let commons: Vec<&EntityCommon> = chain
            .iter()
            .filter_map(|&h| doc.get_entity(Handle::new(h)).map(EntityType::common))
            .collect();
        let (_, color) = drawn_style(doc, &self.xref_docs(), &commons, common, xref);
        resolve_color(&color, &Color::ByLayer)
    }

    /// Runs `cmd` for the user; see `run_as`.
    pub fn run(&mut self, cmd: Command) -> Result<Outcome, CoreError> {
        self.run_as(cmd, Source::User)
    }

    /// Runs `cmd` for `source` and, when it changes the take-off, the BOQ, the conflicts or the
    /// drawing, records an undo step.
    /// New measurements are marked as made by `source`.
    pub fn run_as(&mut self, cmd: Command, source: Source) -> Result<Outcome, CoreError> {
        if self.tx_depth == 0 {
            self.last_change = crate::Change::default();
        }
        let raw = serde_json::to_value(&cmd)
            .map_err(|error| CoreError::InvalidCommand(error.to_string()))?;
        let report = self.validate_input(&raw);
        if !report.valid {
            return Err(crate::input::with_warnings(
                CoreError::InvalidCommand(report.message()),
                &report.warnings,
            ));
        }
        self.run_command(cmd, source)
    }

    pub(crate) fn run_command(
        &mut self,
        mut cmd: Command,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        cmd.normalize_runs(self.drawing.units);
        if let Command::Batch {
            commands,
            layer,
            color,
        } = cmd
        {
            if self.tx_depth > 0 {
                return Err(CoreError::InvalidCommand(
                    "A batch can't run inside another batch".to_string(),
                ));
            }
            return self.run_batch(commands, layer, color, source);
        }
        if let Command::LabelSpaces {
            query,
            layer,
            height,
            decimals,
            language,
        } = cmd
        {
            return self.transaction(source, "label_spaces", |s| {
                s.label_spaces(&query, layer, height, decimals, language, source)
            });
        }
        let label = cmd.label();
        if self.tx_depth > 0 {
            let batchable = crate::registry::registry()
                .get(label.split('.').next().unwrap_or(""))
                .is_none_or(|info| info.batchable);
            if !batchable {
                return Err(CoreError::InvalidCommand(format!(
                    "{label} can't run inside a batch: run it on its own"
                )));
            }
        }
        self.check_block_edit(&cmd)?;
        if matches!(cmd, Command::SaveLayerState { .. }) {
            self.prepare_layer_states();
        }
        let before = match cmd.changes() {
            None => return self.execute(cmd, source),
            Some(Part::Document { tables, takeoff }) => {
                let (more_tables, more_takeoff) = self.document_extras(&cmd);
                // Constraints follow the command in the same step (`crate::param`).
                let (param_tables, param_takeoff) = self.param_extras(&cmd);
                let follow = !Session::keeps_constraints(&cmd);
                let reshape = crate::param::Reshape::of(&cmd);
                return self.record_document_change(
                    source,
                    &label,
                    tables || more_tables || param_tables,
                    takeoff || more_takeoff || param_takeoff,
                    |s| {
                        let snapshot = if follow { s.param_snapshot() } else { None };
                        let mut outcome = s.execute(cmd, source)?;
                        let removed = s.follow_param(snapshot, reshape)?;
                        if let Outcome::Edited {
                            constraints_removed,
                            ..
                        } = &mut outcome
                        {
                            *constraints_removed = removed;
                        }
                        Ok(outcome)
                    },
                );
            }
            Some(Part::Takeoff) => Step::Takeoff(self.takeoff.clone()),
            Some(Part::Boq) => Step::Boq(self.boq.clone()),
            Some(Part::Conflicts) => {
                Step::Conflicts(self.conflicts.clone(), self.schedules.clone())
            }
        };
        let outcome = match self.execute(cmd, source) {
            Ok(outcome) => outcome,
            Err(e) => {
                // A command that failed half way leaves its part as it was.
                match before {
                    Step::Takeoff(takeoff) => self.takeoff = takeoff,
                    Step::Boq(boq) => self.boq = boq,
                    Step::Conflicts(conflicts, schedules) => {
                        self.conflicts = conflicts;
                        self.schedules = schedules;
                    }
                    Step::Document(_) | Step::Group(..) => {}
                }
                return Err(e);
            }
        };
        if !matches!(outcome, Outcome::Nothing) {
            self.history.record(before, source, &label);
        }
        Ok(outcome)
    }

    /// Undoes the last take-off, BOQ, conflicts or drawing change. False when there is nothing to
    /// undo.
    pub fn undo(&mut self) -> bool {
        let mut doc_changed = false;
        let undone = self.history.undo(Parts {
            takeoff: &mut self.takeoff,
            boq: &mut self.boq,
            conflicts: &mut self.conflicts,
            schedules: &mut self.schedules,
            doc: &mut self.drawing.doc,
            view: ViewRefs {
                current: &mut self.current,
                hidden: &mut self.visibility.hidden_layers,
                state_hidden: &mut self.state_hidden,
            },
            doc_changed: &mut doc_changed,
        });
        if doc_changed {
            self.drawing_changed();
        }
        undone
    }

    /// True when the drawing itself (not the take-off, which is saved after every command) has
    /// changed since it was opened or last saved: every drawing command (imports, underlays,
    /// calibrations, deletions…), and their undo and redo.
    pub fn is_dirty(&self) -> bool {
        self.document_revision != self.saved_revision
    }

    /// Runs `f`, a change to the drawing, as one undo step made by `source`. The step holds the
    /// before images of the entities `f` touches (recorded by the drawing as it goes), with the
    /// drawing's tables when `tables` and the take-off when `takeoff` (taken whole before `f`).
    /// A change that leaves everything as it was is no step. Every step moves
    /// `document_revision` on, so the window draws the batch again. On failure everything is put
    /// back as it was.
    pub(crate) fn record_document_change<T>(
        &mut self,
        source: Source,
        label: &str,
        tables: bool,
        takeoff: bool,
        f: impl FnOnce(&mut Self) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        // Room boundaries, schedules and quantities follow all drawing mutations in this step.
        let rooms = self.has_rooms();
        let tables = tables || rooms || self.has_opening_schedules();
        let takeoff = takeoff || rooms;
        let view = tables.then(|| self.view_refs().get());
        let tables = tables.then(|| TablesSnapshot::take(&self.drawing.doc));
        let takeoff = takeoff.then(|| self.takeoff.clone());
        let recorder = self.drawing.doc.begin_entity_change_recording();
        let done = f(self).and_then(|value| {
            if self.has_rooms() {
                self.park();
                self.refresh_rooms()?;
            }
            if self.has_opening_schedules() {
                self.refresh_opening_schedules()?;
            }
            Ok(value)
        });
        self.drawing.doc.end_entity_change_recording();
        let mut change = DocChange::from_before_images(
            &self.drawing.doc,
            recorder.take_before_images(),
            tables,
            takeoff,
        );
        // Kept only when the change moved it, and then only what it moved, so undoing a step
        // never touches layers hidden or shown since.
        change.view = view
            .and_then(|before| ViewDelta::between(&before, &self.view_refs().get()))
            .map(Box::new);
        match done {
            Ok(done) => {
                if !change.is_empty(&self.drawing.doc) {
                    self.drawing_changed();
                    self.last_change.merge(crate::Change {
                        added: change.added.iter().map(|h| h.value()).collect(),
                        changed: change.replaced.iter().map(|(h, _)| h.value()).collect(),
                        removed: change
                            .removed
                            .iter()
                            .map(|e| e.common().handle.value())
                            .collect(),
                    });
                    self.history.record(Step::Document(change), source, label);
                }
                Ok(done)
            }
            Err(e) => {
                let mut view = ViewRefs {
                    current: &mut self.current,
                    hidden: &mut self.visibility.hidden_layers,
                    state_hidden: &mut self.state_hidden,
                };
                change.swap(&mut self.drawing.doc, &mut self.takeoff, &mut view);
                *self.cache.get_mut() = None;
                Err(e)
            }
        }
    }

    /// The session's current state, hidden layers and layer states' hidden layers, to keep or
    /// put back with a drawing change.
    fn view_refs(&mut self) -> ViewRefs<'_> {
        ViewRefs {
            current: &mut self.current,
            hidden: &mut self.visibility.hidden_layers,
            state_hidden: &mut self.state_hidden,
        }
    }

    /// The drawing counts as having changes not saved yet (a recovered autosave).
    pub(crate) fn mark_unsaved(&mut self) {
        self.document_revision += 1;
    }

    /// The drawing counts as saved as it is now.
    pub(crate) fn mark_saved(&mut self) {
        self.saved_revision = self.document_revision;
    }

    /// Drops the drawn batch and pick index (for a drawing put aside in a tab, which on a big
    /// drawing hold hundreds of MB); the next draw or pick builds them again.
    pub fn park(&mut self) {
        *self.cache.get_mut() = None;
        self.others.get_mut().clear();
    }

    /// The batch and pick index are built and kept (see `park`).
    pub fn is_drawn(&self) -> bool {
        self.cache.borrow().is_some()
    }

    /// The drawing itself changed: the batch is drawn again and its version moves on.
    pub(crate) fn drawing_changed(&mut self) {
        if !self.drawing.doc.block_records.contains(&self.sheet) {
            // Undoing a newly created layout can remove the sheet currently on screen.
            self.sheet = "*Model_Space".to_string();
        }
        self.document_revision += 1;
        self.empty_model = model_is_empty(&self.drawing);
        *self.cache.get_mut() = None;
        self.others.get_mut().clear();
    }

    /// Which opening of a drawing this session is: no two sessions in one run share it. What the
    /// window or the assistant fetched for one session is never used for another.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Undoes the last take-off or BOQ change only when `source` made it. False otherwise, with nothing
    /// changed: the AI's Undo never takes back a change the user made after it.
    pub fn undo_if(&mut self, source: Source) -> bool {
        self.history.top_source() == Some(source) && self.undo()
    }

    /// Who made the change `undo` would take back; None when there is nothing to undo.
    pub fn top_step_source(&self) -> Option<Source> {
        self.history.top_source()
    }

    /// Redoes the last undone take-off, BOQ, conflicts or drawing change. False when there is
    /// nothing to redo.
    pub fn redo(&mut self) -> bool {
        let mut doc_changed = false;
        let redone = self.history.redo(Parts {
            takeoff: &mut self.takeoff,
            boq: &mut self.boq,
            conflicts: &mut self.conflicts,
            schedules: &mut self.schedules,
            doc: &mut self.drawing.doc,
            view: ViewRefs {
                current: &mut self.current,
                hidden: &mut self.visibility.hidden_layers,
                state_hidden: &mut self.state_hidden,
            },
            doc_changed: &mut doc_changed,
        });
        if doc_changed {
            self.drawing_changed();
        }
        redone
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Length and area of `entity` of `doc` (the drawing or one of its xrefs) placed by `xf`:
    /// exact for lines, arcs, circles, full ellipses and (bulged) polylines under a uniform scale;
    /// otherwise the length of all its flattened paths and the area of the closed ones.
    pub(crate) fn extent(&self, doc: &CadDocument, entity: &EntityType, xf: &Xform) -> Extent {
        if crate::room_objects::is_room_boundary(entity) {
            let origin = xf.apply([0.0, 0.0]);
            let x = xf.apply([1.0, 0.0]);
            let y = xf.apply([0.0, 1.0]);
            let scale = ((x[0] - origin[0]) * (y[1] - origin[1])
                - (x[1] - origin[1]) * (y[0] - origin[0]))
                .abs();
            return Extent {
                area: crate::room_objects::room_area(entity).map(|area| area * scale),
                length: exact_length(entity, xf),
                self_intersecting: false,
            };
        }
        // A dimension measures what it says (an angle measures no length), not its lines.
        if let EntityType::Dimension(d) = entity {
            let [x0, y0] = xf.apply([0.0, 0.0]);
            let [x1, y1] = xf.apply([1.0, 0.0]);
            let m = d.measurement() * (x1 - x0).hypot(y1 - y0);
            return Extent {
                length: (!d.base().is_angular() && m.is_finite()).then_some(m),
                area: None,
                self_intersecting: false,
            };
        }
        // A table is no outline: its grid lines and text boxes measure nothing.
        if let EntityType::Table(_) = entity {
            return Extent {
                length: None,
                area: None,
                self_intersecting: false,
            };
        }
        let xrefs = self.xref_docs();
        let paths = flatten_with(doc, &xrefs, entity, xf, self.measure_tol()).into_paths();
        let closed: Vec<_> = paths.iter().filter(|p| p.closed).collect();
        let crossing: Vec<bool> = closed.iter().map(|p| p.self_intersects()).collect();
        let self_intersecting = crossing.contains(&true);
        let length = (!paths.is_empty()).then(|| {
            exact_length(entity, xf).unwrap_or_else(|| paths.iter().map(|p| p.length()).sum())
        });
        // A crossing outline's lobes would cancel in the exact sum; the flattened one adds them.
        let exact = exact_area(entity, xf).filter(|_| !self_intersecting);
        let area = (!closed.is_empty()).then(|| {
            exact.unwrap_or_else(|| match entity {
                // A hatch's boundary paths fill even-odd: an island is taken away. Each path
                // exactly when it is a bulged polyline that does not cross itself (one outline
                // per path, in order); one that crosses or touches itself counts as its simple
                // loops, flattened, so a click region's pinched-in island is taken away too.
                EntityType::Hatch(_) => {
                    let exact = exact_hatch_areas(entity, xf).filter(|a| a.len() == closed.len());
                    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
                    let mut areas: Vec<f64> = Vec::new();
                    for (i, p) in closed.iter().enumerate() {
                        if crossing[i] {
                            for points in p.loops() {
                                let area = qiyas_geom::Path {
                                    points: points.clone(),
                                    closed: true,
                                }
                                .area();
                                rings.push(points);
                                areas.push(area);
                            }
                        } else {
                            rings.push(p.points.clone());
                            areas.push(exact.as_ref().map_or_else(|| p.area(), |a| a[i]));
                        }
                    }
                    let depths = ring_nesting_depths(&rings);
                    areas
                        .iter()
                        .zip(depths)
                        .map(|(a, d)| if d % 2 == 0 { *a } else { -a })
                        .sum()
                }
                _ => closed.iter().map(|p| p.area()).sum(),
            })
        });
        Extent {
            length,
            area,
            self_intersecting,
        }
    }

    /// Runs `f` on model space as drawn for the viewports of a layout (see `with_sheet`). No
    /// pick index: a click inside a viewport picks the viewport.
    pub(crate) fn with_model<R>(&self, px_per_unit: Option<f64>, f: impl FnOnce(&Batch) -> R) -> R {
        self.with_sheet("*Model_Space", px_per_unit, f)
    }

    /// Runs `f` on the batch of the sheet `sheet` (block name) with the session's hidden layers:
    /// the one on screen's own (`with_cache`) for it, else one kept apart and rebuilt when that
    /// sheet would draw differently (`version_of`); its glyphs drawn again for `px_per_unit` when
    /// given and new. The pictures of a sheet not on screen are not drawn yet.
    pub(crate) fn with_sheet<R>(
        &self,
        sheet: &str,
        px_per_unit: Option<f64>,
        f: impl FnOnce(&Batch) -> R,
    ) -> R {
        if self.sheet.eq_ignore_ascii_case(sheet) {
            return self.with_cache(px_per_unit, |c| f(&c.batch));
        }
        let version = self.version_of(sheet);
        let mut others = self.others.borrow_mut();
        let mut text = self.text.borrow_mut();
        let at = others
            .iter()
            .position(|(name, v, _)| name.eq_ignore_ascii_case(sheet) && *v == version);
        let mut entry = match at {
            Some(i) => others.remove(i),
            None => {
                let options = TextOptions {
                    arabic_reversed: self.arabic_reversed,
                    px_per_unit,
                };
                let mut batch = build_batch_with_xrefs(
                    &self.drawing.doc,
                    &self.xref_docs(),
                    sheet,
                    &self.visibility,
                    self.batch_tol(),
                    &mut text,
                    options,
                );
                batch.curve_tol = self.batch_tol();
                batch.base_tol = batch.curve_tol;
                others.retain(|(name, _, _)| !name.eq_ignore_ascii_case(sheet));
                (sheet.to_string(), version, batch)
            }
        };
        if let Some(px) = px_per_unit.filter(|p| *p > 0.0 && p.is_finite()) {
            if !entry.2.texts.is_empty() && px != entry.2.px_per_unit {
                place_text(&mut entry.2, &mut text, px);
            }
        }
        drop(text);
        // Not held while `f` runs, so it may draw the sheet on screen.
        drop(others);
        let out = f(&entry.2);
        let mut others = self.others.borrow_mut();
        others.push(entry);
        if others.len() > OTHER_SHEETS {
            others.remove(0);
        }
        out
    }

    /// Runs `f` on the cached batch, rebuilding it first if the sheet, visibility or Arabic
    /// setting changed, and drawing its glyphs again for `px_per_unit` when given and new.
    fn with_cache<R>(&self, px_per_unit: Option<f64>, f: impl FnOnce(&Cached) -> R) -> R {
        let mut cache = self.cache.borrow_mut();
        let mut text = self.text.borrow_mut();
        // The zoom the window asks for may need the curves drawn again, finer or coarser.
        if let Some(px) = px_per_unit {
            let base = self.batch_tol();
            if let Some(want) = zoom_tol(base, self.screen_tol(), px) {
                self.zoom_tol.set((want < base).then_some(want));
            }
        }
        let stale = cache.as_ref().is_none_or(|c| {
            c.sheet != self.sheet
                || c.hidden_layers != self.visibility.hidden_layers
                || c.arabic_reversed != self.arabic_reversed
                || c.tol != self.screen_tol()
        });
        if stale {
            // Drawn here now: no copy kept beside it.
            self.others
                .borrow_mut()
                .retain(|(name, _, _)| !name.eq_ignore_ascii_case(&self.sheet));
            let options = TextOptions {
                arabic_reversed: self.arabic_reversed,
                px_per_unit,
            };
            let mut batch = build_batch_with_xrefs(
                &self.drawing.doc,
                &self.xref_docs(),
                &self.sheet,
                &self.visibility,
                self.screen_tol(),
                &mut text,
                options,
            );
            batch.curve_tol = self.screen_tol();
            batch.base_tol = self.batch_tol();
            let (pictures, outlines) = self.underlay_placements();
            add_images(&mut batch, &pictures);
            add_pick_outlines(&mut batch, &outlines);
            if let Some(edit) = &self.block_edit {
                fade(&mut batch, &edit.outside);
            }
            let doc = &self.drawing.doc;
            let points = doc
                .entities_in_block(&self.sheet)
                .filter_map(|e| match e {
                    EntityType::Point(pt)
                        if !pt.common.invisible
                            && layer_shown(doc, &self.visibility, &pt.common.layer) =>
                    {
                        Some(([pt.location.x, pt.location.y], pt.common.handle.value()))
                    }
                    _ => None,
                })
                .collect();
            *cache = Some(Cached {
                sheet: self.sheet.clone(),
                hidden_layers: self.visibility.hidden_layers.clone(),
                arabic_reversed: self.arabic_reversed,
                tol: self.screen_tol(),
                index: PickIndex::new(&batch),
                batch,
                points,
            });
        }
        let cached = cache.as_mut().expect("cache was just filled");
        if let Some(px) = px_per_unit.filter(|p| *p > 0.0 && p.is_finite()) {
            if !cached.batch.texts.is_empty() && px != cached.batch.px_per_unit {
                place_text(&mut cached.batch, &mut text, px);
            }
        }
        drop(text);
        f(cached)
    }
}

/// The segment `x0 y0 x1 y1` has a point in the box `lo hi` (Liang-Barsky clipping).
fn crosses_box([x0, y0, x1, y1]: [f64; 4], lo: [f64; 2], hi: [f64; 2]) -> bool {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-dx, x0 - lo[0]),
        (dx, hi[0] - x0),
        (-dy, y0 - lo[1]),
        (dy, hi[1] - y0),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Whether `p` lies inside the polygon `ring` or on its edge (even-odd).
fn in_ring(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (k, a) in ring.iter().enumerate() {
        let b = ring[(k + 1) % ring.len()];
        if on_edge(p, *a, b) {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
        {
            inside = !inside;
        }
    }
    inside
}

/// Whether `p` lies on the segment from `a` to `b`, within a billionth of its length.
fn on_edge(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let tol = len.max(f64::MIN_POSITIVE) * 1e-9;
    let cross = (p[0] - a[0]) * dy - (p[1] - a[1]) * dx;
    cross.abs() <= tol * len
        && p[0] >= a[0].min(b[0]) - tol
        && p[0] <= a[0].max(b[0]) + tol
        && p[1] >= a[1].min(b[1]) - tol
        && p[1] <= a[1].max(b[1]) + tol
}

/// The sign of the turn from `a` to `b` to `c`.
fn turn(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether the segments `p`–`q` and `a`–`b` cross each other, touching ends excluded.
fn cross_properly(p: [f64; 2], q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let (d1, d2) = (turn(a, b, p), turn(a, b, q));
    let (d3, d4) = (turn(p, q, a), turn(p, q, b));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Whether the segment `s` (`x0 y0 x1 y1`) lies wholly inside the polygon `ring`: both ends and
/// its middle inside, never crossing an edge.
fn inside_ring([x0, y0, x1, y1]: [f64; 4], ring: &[[f64; 2]]) -> bool {
    let (p, q) = ([x0, y0], [x1, y1]);
    in_ring(p, ring)
        && in_ring(q, ring)
        && in_ring([(x0 + x1) / 2.0, (y0 + y1) / 2.0], ring)
        && !ring
            .iter()
            .enumerate()
            .any(|(k, a)| cross_properly(p, q, *a, ring[(k + 1) % ring.len()]))
}

/// Whether the segment `s` touches the polygon `ring`: an end inside it, or meeting an edge.
fn touches_ring([x0, y0, x1, y1]: [f64; 4], ring: &[[f64; 2]]) -> bool {
    let (p, q) = ([x0, y0], [x1, y1]);
    in_ring(p, ring)
        || in_ring(q, ring)
        || ring.iter().enumerate().any(|(k, a)| {
            let b = ring[(k + 1) % ring.len()];
            cross_properly(p, q, *a, b) || on_edge(*a, p, q) || on_edge(b, p, q)
        })
}

/// Model space holds no entity.
/// What `purge_for_save` took out of a drawing: block records with their entities, and groups'
/// member lists as they were.
struct Purged {
    blocks: Vec<(BlockRecord, Vec<std::sync::Arc<EntityType>>)>,
    groups: Vec<(Handle, Vec<Handle>)>,
}

/// Takes out of `doc` what only clutters a saved file: anonymous dimension (`*Dn`) and table
/// (`*Tn`) blocks no dimension, table or reference uses any more (an erase, a discarded edit in
/// place), and group members that are gone. `restore_purged` puts them back.
fn purge_for_save(doc: &mut CadDocument) -> Purged {
    let mut used: HashSet<String> = HashSet::new();
    // A dimension read with no block name may still own a block: then keep every `*D`.
    let mut unnamed_dimension = false;
    for e in doc.entities() {
        match e {
            EntityType::Dimension(d) => {
                let name = &d.base().block_name;
                unnamed_dimension |= crate::purge::no_block(name);
                used.insert(normalize_name(name));
            }
            EntityType::Table(t) => {
                used.insert(normalize_name(&t.block_name));
                if let Some(b) = qiyas_geom::table_block(doc, t) {
                    used.insert(normalize_name(&b.name));
                }
            }
            EntityType::Insert(i) => {
                used.insert(normalize_name(&i.block_name));
            }
            _ => {}
        }
    }
    // One rule with PURGE (`crate::purge`).
    let anonymous = |name: &str, kind: char| crate::purge::anonymous_kind(name) == Some(kind);
    let orphans: Vec<String> = doc
        .block_records
        .iter()
        .filter(|b| {
            let n = b.name.as_str();
            ((anonymous(n, 'D') && !unnamed_dimension) || anonymous(n, 'T'))
                && !used.contains(&normalize_name(n))
        })
        .map(|b| b.name.clone())
        .collect();
    let mut purged = Purged {
        blocks: Vec::new(),
        groups: Vec::new(),
    };
    for name in orphans {
        let Some(record) = doc.block_records.remove(&name) else {
            continue;
        };
        let handles = record
            .entity_handles
            .iter()
            .chain([&record.block_entity_handle, &record.block_end_handle]);
        let entities = handles.filter_map(|&h| doc.remove_entity_arc(h)).collect();
        purged.blocks.push((record, entities));
    }
    let exists = |h: &Handle| doc.get_entity(*h).is_some();
    let stale: Vec<(Handle, Vec<Handle>, Vec<Handle>)> = doc
        .objects
        .iter()
        .filter_map(|(&h, o)| match o {
            ObjectType::Group(g) if !g.entities.iter().all(exists) => {
                let live = g.entities.iter().copied().filter(exists).collect();
                Some((h, g.entities.clone(), live))
            }
            _ => None,
        })
        .collect();
    for (h, members, live) in stale {
        if let Some(ObjectType::Group(g)) = doc.objects.get_mut(&h) {
            g.entities = live;
        }
        purged.groups.push((h, members));
    }
    purged
}

/// Puts back what `purge_for_save` took out.
fn restore_purged(doc: &mut CadDocument, purged: Purged) {
    for (record, entities) in purged.blocks {
        let _ = doc.block_records.add(record);
        for e in entities {
            doc.restore_entity_arc(e);
        }
    }
    for (h, members) in purged.groups {
        if let Some(ObjectType::Group(g)) = doc.objects.get_mut(&h) {
            g.entities = members;
        }
    }
}

fn model_is_empty(drawing: &Drawing) -> bool {
    drawing.entities_in("*Model_Space").next().is_none()
}

/// 64-bit FNV-1a.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// What one entity measures. `None` when it has no path, or no closed path for `area`.
pub(crate) struct Extent {
    pub length: Option<f64>,
    pub area: Option<f64>,
    /// True when any closed path crosses itself.
    pub self_intersecting: bool,
}

fn units_name(units: Units) -> &'static str {
    match units {
        Units::Inches => "inches",
        Units::Feet => "feet",
        Units::Millimetres => "millimetres",
        Units::Centimetres => "centimetres",
        Units::Metres => "metres",
        Units::Unknown => "unknown",
    }
}

/// The `EntityType` variant name, e.g. "LwPolyline". Taken from the derived `Debug` output,
/// which starts with the variant name; the writer stops at the first `(` so the entity's
/// data is never formatted.
pub(crate) fn variant_name(entity: &EntityType) -> String {
    struct UpToParen(String);
    impl Write for UpToParen {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            match s.find('(') {
                Some(i) => {
                    self.0.push_str(&s[..i]);
                    Err(fmt::Error)
                }
                None => {
                    self.0.push_str(s);
                    Ok(())
                }
            }
        }
    }
    let mut name = UpToParen(String::new());
    // The Err is our own early stop, not a failure.
    let _ = write!(name, "{entity:?}");
    name.0
}

#[cfg(test)]
mod tests {
    use opencadcodec::Line;

    use super::*;

    #[test]
    fn a_sheet_drawn_on_screen_or_a_drawing_change_leaves_no_copy_beside_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::fixtures::write_sample(dir.path(), "dxf");
        let mut s = Session::open(&path).unwrap();
        let layout = s.drawing.sheets()[1].block.clone();
        s.set_sheet(&layout).unwrap();
        s.batch();
        s.with_model(None, |_| ());
        assert_eq!(s.others.borrow().len(), 1);
        // Model space on screen: drawn there, not kept twice.
        s.set_sheet("*Model_Space").unwrap();
        s.batch();
        assert!(s.others.borrow().is_empty());
        // A drawing change drops what was kept for other sheets.
        s.with_sheet(&layout, None, |_| ());
        assert_eq!(s.others.borrow().len(), 1);
        s.drawing_changed();
        assert!(s.others.borrow().is_empty());
    }

    #[test]
    fn a_failed_drawing_change_is_put_back_and_is_no_step() {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::fixtures::write_sample(dir.path(), "dxf");
        let mut s = Session::open(&path).unwrap();
        let count = |s: &Session| s.drawing.doc.entities_in_block("*Model_Space").count();
        let (entities, version) = (count(&s), s.batch_version());
        let first = s
            .drawing
            .doc
            .entities_in_block("*Model_Space")
            .next()
            .unwrap();
        let first = first.common().handle;
        let err = s.record_document_change(Source::User, "test", true, false, |s| {
            s.ensure_layer("NEW", Color::Index(1));
            s.drawing
                .doc
                .add_entity(EntityType::Line(Line::from_coords(
                    0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
                )))
                .unwrap();
            s.drawing.doc.remove_entity_arc(first);
            Err::<(), _>(CoreError::InvalidCommand("stop".into()))
        });
        assert!(err.is_err());
        assert_eq!(count(&s), entities);
        assert!(s.drawing.doc.get_entity(first).is_some());
        assert!(!s.drawing.doc.layers.contains("NEW"));
        assert!(!s.can_undo());
        assert_eq!(s.batch_version(), version);
    }
}
