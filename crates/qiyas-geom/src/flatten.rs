use std::collections::HashMap;
use std::f64::consts::TAU;

use opencadcodec::entities::{
    AttributeEntity, BoundaryEdge, Dimension, EntityCommon, EntityType, HorizontalAlignment,
    Insert, MText, Table, Text, TextHorizontalAlignment, TextVerticalAlignment, VerticalAlignment,
};
use opencadcodec::tables::normalize_name;
use opencadcodec::types::Matrix3;
use opencadcodec::{CadDocument, Color, Layer, Vector3};
use opencadkernel::geom2d::NurbsCurve;

use crate::dimension::{dimension_entities, leader_entities, tolerance_entities};
use crate::paint::{mline_paths, pattern_spans, solid_outline, wipeout_outline, FlatFill, Paint};
use crate::path::same_point;
use crate::table::{table_block, table_entities, table_xform};
use crate::{Path, Xform};

/// Deepest block nesting followed; deeper inserts (or cycles) are skipped.
const MAX_DEPTH: usize = 32;
/// Upper bound on segments per curve, so a zero or tiny `tol` cannot exhaust memory.
const MAX_SEGMENTS: usize = 1 << 16;
/// Largest angle one segment of an arc, circle or ellipse spans: a full circle gets at least 16,
/// so a circle far smaller than `tol` never draws as a triangle or a square. Rounder than that
/// when zoomed in is the screen's zoom regen (`qiyas_core::zoom_tol`), not more segments here: a
/// floor of 64 made a millimetre drawing full of small circles 6.6 times heavier.
const MAX_STEP: f64 = TAU / 16.0;
/// Most block instances plus paths one `flatten` call produces; past it, output stops.
/// Keeps a block that inserts itself, or a huge nested array, from hanging the app. A caller
/// flattening many entities shares one budget through `flatten_within`.
pub const MAX_WORK: usize = 1_000_000;
/// The xdata application marking a text Qiyas wrote: its value is in reading order, so it is
/// never drawn reversed, whatever "Arabic text is reversed" says (see `is_logical_text`).
pub const LOGICAL_TEXT_APP: &str = "QIYAS_TEXT";
/// Bulges smaller than this are straight segments.
const MIN_BULGE: f64 = 1e-12;
/// Deepest xref nesting drawn: an xref inside an xref inside ... four levels down.
pub const MAX_XREF_DEPTH: usize = 4;

/// The loaded xref drawings, by xref block name as `normalize_name` gives it (upper case).
pub type XrefDocs<'a> = HashMap<String, &'a CadDocument>;

/// The xref drawing and the name inside it of an xref-dependent symbol `XREF|NAME`, when `XREF`
/// is a loaded xref. `None` for any other name.
pub fn xref_symbol<'a, 'n>(
    xrefs: &XrefDocs<'a>,
    name: &'n str,
) -> Option<(&'a CadDocument, &'n str)> {
    let (xref, inner) = name.split_once('|')?;
    xrefs.get(&normalize_name(xref)).map(|doc| (*doc, inner))
}

/// The layer `name` as drawn: the host drawing's own (an `XREF|LAYER` the host keeps wins, as
/// AutoCAD keeps xref layer changes), else for `XREF|LAYER` the layer `LAYER` of that xref.
pub fn find_layer<'a>(
    host: &'a CadDocument,
    xrefs: &XrefDocs<'a>,
    name: &str,
) -> Option<&'a Layer> {
    host.layers.get(name).or_else(|| {
        let (doc, inner) = xref_symbol(xrefs, name)?;
        doc.layers.get(inner)
    })
}

/// True when `block_name` is an overlaid xref in `doc`: not shown by a drawing that references
/// `doc` itself as an xref.
pub fn is_overlay_block(doc: &CadDocument, block_name: &str) -> bool {
    doc.block_records
        .get(block_name)
        .is_some_and(|b| b.flags.is_xref_overlay)
}

/// True when `block_name` is an xref (attached or overlaid) in `doc`.
pub fn is_xref_block(doc: &CadDocument, block_name: &str) -> bool {
    doc.block_records
        .get(block_name)
        .is_some_and(|b| b.flags.is_xref || b.flags.is_xref_overlay)
}

/// One flattened path with what it is drawn with. `layer` and `color` are resolved the AutoCAD
/// way inside blocks: an entity on layer "0" takes its insert's layer, ByLayer takes that
/// layer's colour, ByBlock takes the insert's colour (ByBlock stays only at the top level).
#[derive(Debug, Clone, PartialEq)]
pub struct FlatPath {
    pub path: Path,
    pub layer: String,
    pub color: Color,
    /// Handles of the block references the path sits inside, outermost first; empty at the top.
    pub chain: Vec<u64>,
    /// Handle of the entity the path comes from: the innermost one, or the insert itself for an
    /// attribute's text box.
    pub entity: u64,
    /// Name of the xref the path comes from (the outermost one, for a nested xref). Its `chain`
    /// then ends with that xref's reference, and `entity` is the entity at the top of the xref's
    /// model space (a block reference inside the xref is picked whole).
    pub xref: Option<String>,
    /// Inside an xref, the layers of the block references (and nested xref references) the path
    /// sits inside there, outermost first, as drawn (`XREF|LAYER`, or the outer one's for "0").
    /// `chain` stops at the xref, so whoever hides layers checks these too.
    pub xref_insert_layers: Vec<String>,
    /// For a text's box (picked, not drawn): what the text draws.
    pub text: Option<Box<FlatText>>,
    /// What the path draws: an outline, or (from `flatten_within`, for the batch) a hatch's
    /// pattern line, a fill or a ray.
    pub paint: Paint,
}

/// What a text (TEXT, MTEXT, or an attribute) draws, for the text renderer.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatText {
    /// A TEXT's or attribute's string, or an MTEXT's with its formatting codes.
    pub value: String,
    /// Text style name.
    pub style: String,
    /// Text height in entity coordinates; 0 when the entity gives none (the style's then).
    pub height: f64,
    pub layout: TextLayout,
    /// Entity (object) coordinates to world: the extrusion normal, then the block references.
    pub xf: Xform,
    /// Written by Qiyas in reading order (`LOGICAL_TEXT_APP`): never drawn reversed.
    pub logical: bool,
}

/// Whether the entity carries the `LOGICAL_TEXT_APP` mark: its text was written by Qiyas in
/// reading order.
pub fn is_logical_text(common: &EntityCommon) -> bool {
    common.extended_data.get_record(LOGICAL_TEXT_APP).is_some()
}

/// Where a text sits and how it is aligned, in entity coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextLayout {
    /// TEXT and attributes: one line. When the alignment is not left and baseline, `alignment`
    /// (if any) is the point aligned to; aligned and fit text runs from `insertion` to it.
    Line {
        insertion: [f64; 2],
        alignment: Option<[f64; 2]>,
        rotation: f64,
        /// 0 when the entity gives none (the style's then).
        width_factor: f64,
        horizontal: HAlign,
        vertical: VAlign,
        /// Generation flag 2: drawn mirrored in X.
        backward: bool,
        /// Generation flag 4: drawn mirrored in Y.
        upside_down: bool,
    },
    /// MTEXT: lines wrapped at `width` (0: no wrapping), placed by `attachment` (1 top left to
    /// 9 bottom right, as in DXF), `line_spacing` a factor of the default spacing.
    Block {
        insertion: [f64; 2],
        rotation: f64,
        width: f64,
        attachment: u8,
        line_spacing: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
    Aligned,
    Middle,
    Fit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Baseline,
    Bottom,
    Middle,
    Top,
}

/// What `flatten` produced. `truncated` is true when block content was left out: nesting deeper
/// than 32 (or a block inserting itself), or more than a million instances and paths.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlattenOutcome {
    pub paths: Vec<FlatPath>,
    pub truncated: bool,
}

impl FlattenOutcome {
    /// The bare outlines, for measuring.
    pub fn into_paths(self) -> Vec<Path> {
        self.paths
            .into_iter()
            .filter(|f| f.paint == Paint::Outline)
            .map(|f| f.path)
            .collect()
    }
}

/// The 2D outline of `entity`, mapped through `xf`. Curves are sampled so the chord error stays
/// within `tol` after `xf` is applied. Entities in object coordinates (arcs, circles, polylines,
/// hatches, text, inserts) are mapped through their extrusion normal first, so a mirrored block
/// (normal 0,0,-1) lands where it is drawn. Invisible entities inside blocks are left out.
/// Xref references draw nothing here; `flatten_with` draws them.
pub fn flatten(doc: &CadDocument, entity: &EntityType, xf: &Xform, tol: f64) -> FlattenOutcome {
    flatten_with(doc, &XrefDocs::new(), entity, xf, tol)
}

/// `flatten`, with a reference to a loaded xref drawn as that drawing's model space under the
/// reference's transform (xrefs inside it too, `MAX_XREF_DEPTH` deep; a cycle stops). Inside an
/// xref, layers are named `XREF|LAYER` (layer "0" takes the reference's, as in a block) and text
/// styles `XREF|STYLE`, both named after the innermost xref.
pub fn flatten_with(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    entity: &EntityType,
    xf: &Xform,
    tol: f64,
) -> FlattenOutcome {
    walk(doc, xrefs, entity, xf, tol, &mut MAX_WORK.clone(), false)
}

/// `flatten_with` for drawing a sheet: spends `budget` (block instances plus paths) and leaves
/// what is left in it, so a caller flattening many entities stops at one budget for all; and
/// paints besides the outlines (see `Paint`): hatch fills (solid hatches) and pattern lines,
/// 2D solid and trace fills, wipeouts (filled with the background), rays and construction lines,
/// and points.
pub fn flatten_within(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    entity: &EntityType,
    xf: &Xform,
    tol: f64,
    budget: &mut usize,
) -> FlattenOutcome {
    walk(doc, xrefs, entity, xf, tol, budget, true)
}

fn walk(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    entity: &EntityType,
    xf: &Xform,
    tol: f64,
    budget: &mut usize,
    paint: bool,
) -> FlattenOutcome {
    let mut walk = Walk {
        host: doc,
        xrefs,
        doc,
        tol,
        paint,
        out: Vec::new(),
        budget: *budget,
        truncated: false,
        chain: Vec::new(),
        layer_colors: HashMap::new(),
        xref_stack: Vec::new(),
        xref_entity: None,
        xref_insert_layers: Vec::new(),
        owner: None,
    };
    let style = walk.style(entity.common(), None);
    walk.entity(entity, xf, &style, 0);
    *budget = walk.budget;
    FlattenOutcome {
        paths: walk.out,
        truncated: walk.truncated,
    }
}

/// The layer and colour an entity is drawn with, resolved exactly as `flatten_with` resolves them:
/// `chain` holds the block references it sits inside (outermost first, all in `doc`); `xref`
/// names the xref the last of them shows, when the entity is one at the top of that xref's model
/// space. The colour is still ByLayer or ByBlock only when nothing resolves it.
pub fn drawn_style(
    doc: &CadDocument,
    xrefs: &XrefDocs,
    chain: &[&EntityCommon],
    entity: &EntityCommon,
    xref: Option<&str>,
) -> (String, Color) {
    let mut walk = Walk {
        host: doc,
        xrefs,
        doc,
        tol: 0.0,
        paint: false,
        out: Vec::new(),
        budget: 0,
        truncated: false,
        chain: Vec::new(),
        layer_colors: HashMap::new(),
        xref_stack: Vec::new(),
        xref_entity: None,
        xref_insert_layers: Vec::new(),
        owner: None,
    };
    let mut parent: Option<Style> = None;
    for common in chain {
        parent = Some(walk.style(common, parent.as_ref()));
    }
    if let Some(name) = xref {
        walk.xref_stack.push(name.to_string());
    }
    let style = walk.style(entity, parent.as_ref());
    (style.layer, style.color)
}

/// The layer and colour an entity is drawn with.
struct Style {
    layer: String,
    color: Color,
}

struct Walk<'a> {
    /// The drawing `flatten` was called on: layers are looked up here (and in `xrefs`).
    host: &'a CadDocument,
    xrefs: &'a XrefDocs<'a>,
    /// The drawing whose blocks are being walked: the host, or an xref inside it.
    doc: &'a CadDocument,
    tol: f64,
    /// Paint fills, pattern lines and rays too (see `Paint`), not only outlines.
    paint: bool,
    out: Vec<FlatPath>,
    /// Block instances plus paths still allowed.
    budget: usize,
    truncated: bool,
    /// Inserts entered so far, outermost first.
    chain: Vec<u64>,
    /// Each layer's colour by its name as drawn, so a block drawn many times looks it up once.
    layer_colors: HashMap<String, Color>,
    /// The xrefs entered, outermost first, by block name as written.
    xref_stack: Vec<String>,
    /// Inside an xref: the entity at the top of the outermost xref's model space, which every
    /// path inside it is reported as.
    xref_entity: Option<u64>,
    /// Inside an xref: the layers of the references entered there (see `FlatPath`).
    xref_insert_layers: Vec<String>,
    /// Inside what a dimension, leader or tolerance draws: that entity, which every path is
    /// reported as (its block's block references are never part of a chain).
    owner: Option<u64>,
}

impl Walk<'_> {
    /// The style of an entity with `common`, inside a block reference drawn with `parent`
    /// (`None` at the top level).
    fn style(&mut self, common: &EntityCommon, parent: Option<&Style>) -> Style {
        let layer = match (parent, self.xref_stack.last()) {
            (Some(p), _) if common.layer == "0" => p.layer.clone(),
            (_, Some(xref)) => format!("{xref}|{}", common.layer),
            _ => common.layer.clone(),
        };
        let color = match common.color {
            Color::ByLayer => match self.layer_colors.get(layer.as_str()) {
                Some(c) => *c,
                None => {
                    let c = find_layer(self.host, self.xrefs, &layer)
                        .map_or(Color::ByLayer, |l| l.color);
                    self.layer_colors.insert(layer.clone(), c);
                    c
                }
            },
            Color::ByBlock => parent.map_or(Color::ByBlock, |p| p.color),
            other => other,
        };
        Style { layer, color }
    }

    fn entity(&mut self, entity: &EntityType, xf: &Xform, style: &Style, depth: usize) {
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        let handle = entity.common().handle.value();
        let xf = &match ocs_of(entity) {
            Some(ocs) => ocs.then(xf),
            None => *xf,
        };
        // Sample in entity coordinates with the tolerance shrunk by the transform's largest stretch.
        let local_tol = self.tol / max_stretch(xf);
        let (points, closed) = match entity {
            EntityType::Line(e) => (vec![xy(&e.start), xy(&e.end)], false),
            EntityType::LwPolyline(e) => {
                let vertices: Vec<_> = e
                    .vertices
                    .iter()
                    .map(|v| ([v.location.x, v.location.y], v.bulge))
                    .collect();
                (bulge_points(&vertices, e.is_closed, local_tol), e.is_closed)
            }
            EntityType::Polyline2D(e) => {
                let vertices: Vec<_> = e
                    .vertices
                    .iter()
                    .map(|v| (xy(&v.location), v.bulge))
                    .collect();
                (
                    bulge_points(&vertices, e.is_closed(), local_tol),
                    e.is_closed(),
                )
            }
            EntityType::Polyline(e) => (
                e.vertices.iter().map(|v| xy(&v.location)).collect(),
                e.is_closed(),
            ),
            EntityType::Polyline3D(e) => (
                e.vertices.iter().map(|v| xy(&v.position)).collect(),
                e.is_closed(),
            ),
            EntityType::Circle(e) => (
                arc_points(xy(&e.center), e.radius, 0.0, TAU, local_tol),
                true,
            ),
            EntityType::Arc(e) => (
                arc_points(
                    xy(&e.center),
                    e.radius,
                    e.start_angle,
                    e.end_angle,
                    local_tol,
                ),
                false,
            ),
            EntityType::Ellipse(e) => (
                ellipse_points(
                    xy(&e.center),
                    xy(&e.major_axis),
                    minor_axis(&e.normal, &e.major_axis, e.minor_axis_ratio),
                    e.start_parameter,
                    e.end_parameter,
                    local_tol,
                ),
                sweep(e.start_parameter, e.end_parameter) == TAU,
            ),
            // A paper-space viewport's frame (the sheet's own, number 1, is no frame).
            EntityType::Viewport(v) if v.id != 1 && depth == 0 => {
                let (cx, cy) = (v.center.x, v.center.y);
                let (w, h) = (v.width / 2.0, v.height / 2.0);
                (
                    vec![
                        [cx - w, cy - h],
                        [cx + w, cy - h],
                        [cx + w, cy + h],
                        [cx - w, cy + h],
                    ],
                    true,
                )
            }
            EntityType::Spline(e) => {
                let control: Vec<[f64; 2]> = e.control_points.iter().map(xy).collect();
                let weights = (e.weights.len() == control.len()).then(|| e.weights.clone());
                let curve = usize::try_from(e.degree)
                    .ok()
                    .and_then(|d| nurbs_points(d, control, e.knots.clone(), weights, local_tol));
                // Without control points, the curve through the fit points.
                let fit: Vec<[f64; 2]> = e.fit_points.iter().map(xy).collect();
                let points = curve
                    .or_else(|| fit_curve_points(&fit, local_tol))
                    .unwrap_or(fit);
                (points, e.flags.closed)
            }
            EntityType::Hatch(e) => {
                let mut rings = Vec::with_capacity(e.paths.len());
                for path in &e.paths {
                    let mut ring = Vec::new();
                    for edge in &path.edges {
                        extend_ring(&mut ring, edge_points(edge, local_tol));
                    }
                    if ring.len() > 1 && same_point(ring[0], ring[ring.len() - 1]) {
                        ring.pop();
                    }
                    self.push(ring.clone(), true, xf, style, handle);
                    rings.push(ring);
                }
                if !self.paint {
                    return;
                }
                if e.gradient_color.is_enabled() {
                    // A gradient is filled with its first colour.
                    let before = self.out.len();
                    self.push_fill(rings, false, xf, style, handle);
                    let first = e.gradient_color.colors.first().map(|c| c.color);
                    if let (Some(color), Some(fill)) = (first, self.out.get_mut(before)) {
                        fill.color = match color {
                            Color::ByLayer | Color::ByBlock => style.color,
                            other => other,
                        };
                    }
                } else if e.is_solid {
                    self.push_fill(rings, false, xf, style, handle);
                } else {
                    for [a, b] in pattern_spans(&e.pattern.lines, &rings) {
                        self.push_with(vec![a, b], false, xf, style, handle, None, Paint::Pattern);
                    }
                }
                return;
            }
            EntityType::Solid(e) => {
                let outline = solid_outline(e);
                if self.paint {
                    self.push_fill(vec![outline.clone()], false, xf, style, handle);
                }
                (outline, true)
            }
            EntityType::Wipeout(e) => {
                let outline = wipeout_outline(e);
                if self.paint {
                    self.push_fill(vec![outline.clone()], true, xf, style, handle);
                }
                (outline, true)
            }
            EntityType::MLine(e) => {
                let (lines, closed) = mline_paths(self.doc, e);
                if !self.paint {
                    // Measured along its centre line: the points it was drawn through.
                    let centre = e.vertices.iter().map(|v| xy(&v.position)).collect();
                    self.push(centre, closed, xf, style, handle);
                    return;
                }
                for line in lines {
                    self.push(line, closed, xf, style, handle);
                }
                return;
            }
            EntityType::Point(e) => {
                if self.paint {
                    let size = self.host.header.point_display_size;
                    let paint = Paint::Point {
                        size: if size > 0.0 { size } else { 0.0 },
                    };
                    let at = vec![xy(&e.location)];
                    self.push_with(at, false, xf, style, handle, None, paint);
                }
                return;
            }
            EntityType::Ray(e) => {
                if self.paint {
                    let (b, d) = (xy(&e.base_point), xy(&e.direction));
                    let points = vec![b, [b[0] + d[0], b[1] + d[1]]];
                    let paint = Paint::Infinite { both_ways: false };
                    self.push_with(points, false, xf, style, handle, None, paint);
                }
                return;
            }
            EntityType::XLine(e) => {
                if self.paint {
                    let (b, d) = (xy(&e.base_point), xy(&e.direction));
                    let points = vec![b, [b[0] + d[0], b[1] + d[1]]];
                    let paint = Paint::Infinite { both_ways: true };
                    self.push_with(points, false, xf, style, handle, None, paint);
                }
                return;
            }
            EntityType::Insert(e) => {
                self.insert(e, xf, style, depth);
                return;
            }
            EntityType::Dimension(d) => {
                self.dimension(d, xf, style, depth);
                return;
            }
            EntityType::Table(t) => {
                self.table(t, xf, style, depth);
                return;
            }
            EntityType::Leader(l) => {
                let parts = leader_entities(self.doc, l);
                self.owned(handle, &parts.iter().collect::<Vec<_>>(), xf, style, depth);
                return;
            }
            EntityType::Tolerance(t) => {
                let parts = tolerance_entities(self.doc, t);
                self.owned(handle, &parts.iter().collect::<Vec<_>>(), xf, style, depth);
                return;
            }
            EntityType::Text(e) => {
                let text = line_text(e, xf);
                let points = text_box(&text);
                self.push_text(points, xf, style, handle, text);
                return;
            }
            EntityType::MText(e) => {
                let text = block_text(e, xf);
                let points = text_box(&text);
                self.push_text(points, xf, style, handle, text);
                return;
            }
            // An attribute definition shows its tag on a sheet; inside a block only a constant
            // one draws, as its value (the references' attributes draw the others).
            EntityType::AttributeDefinition(e) if depth == 0 || e.flags.constant => {
                let value = if depth == 0 { &e.tag } else { &e.default_value };
                let mut shown = AttributeEntity::from_definition(e, Some(value.clone()));
                shown.common = e.common.clone();
                let at = ocs(&e.normal, e.insertion_point.z).map_or(*xf, |o| o.then(xf));
                let text = attribute_text(&shown, &at);
                let points = text_box(&text);
                self.push_text(points, &at, style, handle, text);
                return;
            }
            _ => return,
        };
        self.push(points, closed, xf, style, handle);
    }

    /// The insert's attribute text boxes (picked as the insert), then its block's content; for
    /// an xref, the xref drawing's model space.
    fn insert(&mut self, e: &Insert, xf: &Xform, style: &Style, depth: usize) {
        if is_xref_block(self.doc, &e.block_name) {
            self.xref(e, xf, style, depth);
            return;
        }
        let handle = e.common.handle.value();
        // Attributes sit where the insert sits, not in the block's coordinates.
        for a in &e.attributes {
            if a.common.invisible || a.flags.invisible || a.value.is_empty() {
                continue;
            }
            let at = ocs(&a.normal, a.insertion_point.z).map_or(*xf, |o| o.then(xf));
            let attribute_style = self.style(&a.common, Some(style));
            let text = attribute_text(a, &at);
            let points = text_box(&text);
            self.push_text(points, &at, &attribute_style, handle, text);
        }
        if depth >= MAX_DEPTH {
            self.truncated = true;
            return;
        }
        // Inside an xref the chain stays at the xref's reference; the reference's layer is kept
        // instead, so hiding it hides what it shows. Inside a dimension's block (an arrowhead
        // block) neither: it is all the dimension.
        let tracked = self.xref_stack.is_empty() && self.owner.is_none();
        let in_xref = !self.xref_stack.is_empty();
        if tracked {
            self.chain.push(handle);
        } else if in_xref {
            self.xref_insert_layers.push(style.layer.clone());
        }
        let doc = self.doc;
        // Styles depend only on the block's entities, not on where each instance is.
        let children: Vec<(&EntityType, Style)> = doc
            .entities_in_block(&e.block_name)
            .filter(|child| !child.common().invisible)
            .map(|child| (child, self.style(child.common(), Some(style))))
            .collect();
        for instance in insert_instances(doc, e, xf) {
            if self.budget == 0 {
                self.truncated = true;
                break;
            }
            self.budget -= 1;
            for (child, child_style) in &children {
                self.entity(child, &instance, child_style, depth + 1);
            }
        }
        if tracked {
            self.chain.pop();
        } else if in_xref {
            self.xref_insert_layers.pop();
        }
    }

    /// A dimension: its `*D` block when it has one with something in it, else the geometry
    /// generated from its definition points and style. Both are in world coordinates (on its
    /// plane) and drawn as the dimension itself.
    fn dimension(&mut self, d: &Dimension, xf: &Xform, style: &Style, depth: usize) {
        let base = d.base();
        let xf = ocs(&base.normal, 0.0).map_or(*xf, |o| o.then(xf));
        let doc = self.doc;
        let inside: Vec<&EntityType> = if base.block_name.is_empty() {
            Vec::new()
        } else {
            doc.entities_in_block(&base.block_name)
                .filter(|c| !c.common().invisible)
                .collect()
        };
        let handle = base.common.handle.value();
        if inside.is_empty() {
            let generated = dimension_entities(doc, d).entities;
            self.owned(
                handle,
                &generated.iter().collect::<Vec<_>>(),
                &xf,
                style,
                depth,
            );
        } else {
            self.owned(handle, &inside, &xf, style, depth);
        }
    }

    /// A table: its `*T` block when it has one with something in it, else the grid and texts
    /// generated from its cells. Both are in table coordinates and drawn as the table itself.
    fn table(&mut self, t: &Table, xf: &Xform, style: &Style, depth: usize) {
        let doc = self.doc;
        let place = table_xform(t).then(xf);
        let record = table_block(doc, t);
        let inside: Vec<&EntityType> = record
            .map(|b| {
                doc.entities_in_block(&b.name)
                    .filter(|c| !c.common().invisible)
                    .collect()
            })
            .unwrap_or_default();
        let handle = t.common.handle.value();
        if inside.is_empty() {
            let generated = table_entities(t);
            let parts: Vec<&EntityType> = generated.iter().collect();
            self.owned(handle, &parts, &place, style, depth);
        } else {
            let base = record.map_or_else(Xform::identity, |b| {
                Xform::translate(-b.base_point.x, -b.base_point.y)
            });
            self.owned(handle, &inside, &base.then(&place), style, depth);
        }
    }

    /// Draws `parts` (in the coordinates `xf` maps) as the entity `owner` drawn with `style`:
    /// layer "0" and ByBlock take its own.
    fn owned(
        &mut self,
        owner: u64,
        parts: &[&EntityType],
        xf: &Xform,
        style: &Style,
        depth: usize,
    ) {
        if depth >= MAX_DEPTH {
            self.truncated = true;
            return;
        }
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        self.budget -= 1;
        let outer = self.owner.replace(owner);
        let styled: Vec<Style> = parts
            .iter()
            .map(|p| self.style(p.common(), Some(style)))
            .collect();
        for (part, part_style) in parts.iter().zip(&styled) {
            self.entity(part, xf, part_style, depth + 1);
        }
        self.owner = outer;
    }

    /// An xref reference: the loaded xref's model space under the reference's transform. A
    /// missing or unloaded xref, one nested too deep, one already being drawn (a cycle) and an
    /// overlay inside an xref (as AutoCAD) draw nothing.
    fn xref(&mut self, e: &Insert, xf: &Xform, style: &Style, depth: usize) {
        let key = normalize_name(&e.block_name);
        let Some(&xdoc) = self.xrefs.get(&key) else {
            return;
        };
        let outermost = self.xref_stack.is_empty();
        let cycle = self.xref_stack.iter().any(|n| normalize_name(n) == key);
        if cycle || self.xref_stack.len() >= MAX_XREF_DEPTH {
            return;
        }
        if !outermost && is_overlay_block(self.doc, &e.block_name) {
            return;
        }
        if outermost {
            self.chain.push(e.common.handle.value());
        } else {
            self.xref_insert_layers.push(style.layer.clone());
        }
        let host_side = self.doc;
        self.xref_stack.push(e.block_name.clone());
        self.doc = xdoc;
        let children: Vec<(&EntityType, Style)> = xdoc
            .entities_in_block("*Model_Space")
            .filter(|child| !child.common().invisible)
            .map(|child| (child, self.style(child.common(), Some(style))))
            .collect();
        for instance in insert_instances(host_side, e, xf) {
            if self.budget == 0 {
                self.truncated = true;
                break;
            }
            self.budget -= 1;
            for (child, child_style) in &children {
                if outermost {
                    self.xref_entity = Some(child.common().handle.value());
                }
                self.entity(child, &instance, child_style, depth + 1);
            }
        }
        self.doc = host_side;
        self.xref_stack.pop();
        if outermost {
            self.xref_entity = None;
            self.chain.pop();
        } else {
            self.xref_insert_layers.pop();
        }
    }

    /// Appends a text's closed box, carrying what the text draws.
    fn push_text(
        &mut self,
        points: Vec<[f64; 2]>,
        xf: &Xform,
        style: &Style,
        entity: u64,
        mut text: FlatText,
    ) {
        if let Some(xref) = self.xref_stack.last() {
            text.style = format!("{xref}|{}", text.style);
        }
        let text = Some(Box::new(text));
        self.push_with(points, true, xf, style, entity, text, Paint::Outline);
    }

    /// Maps `points` through `xf` and appends them as a path, spending one unit of the budget;
    /// a closed path drops a final point that repeats its first.
    fn push(
        &mut self,
        points: Vec<[f64; 2]>,
        closed: bool,
        xf: &Xform,
        style: &Style,
        entity: u64,
    ) {
        self.push_with(points, closed, xf, style, entity, None, Paint::Outline);
    }

    /// Appends a fill of `rings` (even-odd, kept in the entity's own coordinates with `xf`),
    /// spending one unit of the budget.
    fn push_fill(
        &mut self,
        rings: Vec<Vec<[f64; 2]>>,
        background: bool,
        xf: &Xform,
        style: &Style,
        entity: u64,
    ) {
        let rings: Vec<Vec<[f64; 2]>> = rings.into_iter().filter(|r| r.len() >= 3).collect();
        if rings.is_empty() {
            return;
        }
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        self.budget -= 1;
        self.out.push(FlatPath {
            path: Path {
                points: Vec::new(),
                closed: true,
            },
            layer: style.layer.clone(),
            color: style.color,
            chain: self.chain.clone(),
            entity: self.xref_entity.or(self.owner).unwrap_or(entity),
            xref: self.xref_stack.first().cloned(),
            xref_insert_layers: self.xref_insert_layers.clone(),
            text: None,
            paint: Paint::Fill(Box::new(FlatFill {
                rings,
                xf: *xf,
                source: entity,
                background,
            })),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn push_with(
        &mut self,
        mut points: Vec<[f64; 2]>,
        closed: bool,
        xf: &Xform,
        style: &Style,
        entity: u64,
        text: Option<Box<FlatText>>,
        paint: Paint,
    ) {
        if points.is_empty() {
            return;
        }
        if self.budget == 0 {
            self.truncated = true;
            return;
        }
        self.budget -= 1;
        if closed && points.len() > 1 && same_point(points[0], points[points.len() - 1]) {
            points.pop();
        }
        self.out.push(FlatPath {
            path: Path {
                points: points.into_iter().map(|p| xf.apply(p)).collect(),
                closed,
            },
            layer: style.layer.clone(),
            color: style.color,
            chain: self.chain.clone(),
            entity: self.xref_entity.or(self.owner).unwrap_or(entity),
            xref: self.xref_stack.first().cloned(),
            xref_insert_layers: self.xref_insert_layers.clone(),
            text,
            paint,
        });
    }
}

/// Block-to-world transform of each instance of `e` (one per array cell), followed by `xf`.
/// Includes the insert's own extrusion normal. Nothing when the block is missing. Lazy, so a
/// caller can stop early on a huge array.
pub fn insert_instances<'a>(
    doc: &CadDocument,
    e: &'a Insert,
    xf: &'a Xform,
) -> impl Iterator<Item = Xform> + 'a {
    let base = doc.block_records.get(&e.block_name).map(|block| {
        Xform::translate(-block.base_point.x, -block.base_point.y)
            .then(&Xform::scale(e.x_scale(), e.y_scale()))
    });
    let to_world = match ocs(&e.normal, e.insert_point.z) {
        Some(o) => o.then(xf),
        None => *xf,
    };
    base.into_iter().flat_map(move |base| {
        (0..e.column_count.max(1)).flat_map(move |col| {
            (0..e.row_count.max(1)).map(move |row| {
                // Array offsets sit in the insert's own axes, so they rotate with it.
                let offset = Xform::translate(
                    f64::from(col) * e.column_spacing,
                    f64::from(row) * e.row_spacing,
                );
                base.then(&offset)
                    .then(&Xform::rotate(e.rotation))
                    .then(&Xform::translate(e.insert_point.x, e.insert_point.y))
                    .then(&to_world)
            })
        })
    })
}

/// The object-to-world transform of an entity stored in object coordinates, projected on plan;
/// `None` when it needs none (world-coordinate entities, and normals along +Z). Inserts apply
/// theirs in `insert_instances`.
pub(crate) fn ocs_of(entity: &EntityType) -> Option<Xform> {
    match entity {
        EntityType::Arc(e) => ocs(&e.normal, e.center.z),
        EntityType::Circle(e) => ocs(&e.normal, e.center.z),
        EntityType::LwPolyline(e) => ocs(&e.normal, e.elevation),
        EntityType::Polyline2D(e) => ocs(&e.normal, e.elevation),
        EntityType::Hatch(e) => ocs(&e.normal, e.elevation),
        EntityType::Solid(e) => ocs(&e.normal, e.first_corner.z),
        EntityType::Text(e) => ocs(&e.normal, e.insertion_point.z),
        _ => None,
    }
}

/// Object (OCS) to world coordinates on plan for an entity with extrusion `normal`, at height `z`
/// in object coordinates: the identity for +Z, a mirror in X for -Z.
pub fn object_to_world(normal: &Vector3, z: f64) -> Xform {
    ocs(normal, z).unwrap_or_else(Xform::identity)
}

/// The plan projection of the AutoCAD arbitrary-axis transform for `normal`, at height `z` in
/// object coordinates. `None` for +Z (identity) or a zero normal.
pub(crate) fn ocs(normal: &Vector3, z: f64) -> Option<Xform> {
    let up = normal.x.abs() < 1e-12 && normal.y.abs() < 1e-12 && normal.z > 0.0;
    if up || normal.length() < 1e-12 {
        return None;
    }
    let m = Matrix3::arbitrary_axis(*normal).m;
    Some(Xform {
        m: [
            [m[0][0], m[0][1], m[0][2] * z],
            [m[1][0], m[1][1], m[1][2] * z],
            [0.0, 0.0, 1.0],
        ],
    })
}

/// An ellipse's minor semi-axis on plan: the normal crossed with the major axis, times the ratio.
pub(crate) fn minor_axis(normal: &Vector3, major: &Vector3, ratio: f64) -> [f64; 2] {
    let n = if normal.length() < 1e-12 {
        Vector3::new(0.0, 0.0, 1.0)
    } else {
        normal.normalize()
    };
    let m = n.cross(major);
    [m.x * ratio, m.y * ratio]
}

/// Points on a circular arc, counter-clockwise from `a0` to `a1` (radians), ends included.
/// Equal angles give the full circle. Chord error stays within `tol`.
pub fn arc_points(center: [f64; 2], r: f64, a0: f64, a1: f64, tol: f64) -> Vec<[f64; 2]> {
    let sweep = sweep(a0, a1);
    let n = segment_count(r, sweep, tol);
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            [center[0] + r * a.cos(), center[1] + r * a.sin()]
        })
        .collect()
}

/// Points along the NURBS curve of `degree` through `control` with `knots` (and `weights`, all
/// ones when `None`), each piece within `tol` of the curve at its middle. `None` when there is
/// no curve: too few control points for the degree, or points that are not finite.
pub(crate) fn nurbs_points(
    degree: usize,
    control: Vec<[f64; 2]>,
    knots: Vec<f64>,
    weights: Option<Vec<f64>>,
    tol: f64,
) -> Option<Vec<[f64; 2]>> {
    if degree > 10 || !control.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let curve = NurbsCurve::new(degree, control, knots, weights)?;
    let (start, end) = curve.domain();
    if !(start.is_finite() && end.is_finite() && end > start) {
        return None;
    }
    // Every knot span, halved until the curve's middle is within `tol` of the chord.
    let mut spans: Vec<f64> = curve.knots()[degree..=curve.control_points().len()].to_vec();
    spans.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let tol = tol.max(1e-9);
    let mut out = vec![curve.point_at_knot(start)];
    let mut stack: Vec<(f64, f64, usize)> = Vec::new();
    for pair in spans.windows(2).rev() {
        stack.push((pair[0], pair[1], 0));
    }
    while let Some((u0, u1, depth)) = stack.pop() {
        let (a, b) = (curve.point_at_knot(u0), curve.point_at_knot(u1));
        // Quarter points too, so an S-shaped piece crossing its chord at the middle is split.
        let off = [0.25, 0.5, 0.75].iter().any(|f| {
            let m = curve.point_at_knot(u0 + f * (u1 - u0));
            chord_distance(a, b, m) > tol
        });
        if off && depth < 16 && out.len() + stack.len() < MAX_SEGMENTS {
            let mid = (u0 + u1) / 2.0;
            stack.push((mid, u1, depth + 1));
            stack.push((u0, mid, depth + 1));
        } else {
            out.push(b);
        }
    }
    Some(out)
}

/// Points along the cubic through the fit points `fit` (chord-length parameters, natural ends),
/// as AutoCAD fits a spline without control points. `None` when there is no such curve.
fn fit_curve_points(fit: &[[f64; 2]], tol: f64) -> Option<Vec<[f64; 2]>> {
    use opencadkernel::geom2d::Parameterization;
    if fit.len() < 2 || !fit.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let curve = NurbsCurve::interpolate(fit, None, None, Parameterization::Chord)?;
    nurbs_points(
        curve.degree(),
        curve.control_points().to_vec(),
        curve.knots().to_vec(),
        None,
        tol,
    )
}

/// Distance from `p` to the segment `a b`.
fn chord_distance(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// Counter-clockwise sweep from `a0` to `a1` in `(0, TAU]`; equal angles mean a full turn.
pub(crate) fn sweep(a0: f64, a1: f64) -> f64 {
    let s = (a1 - a0).rem_euclid(TAU);
    if s < 1e-12 {
        TAU
    } else {
        s
    }
}

/// Segments needed so a chord of an arc of radius `r` stays within `tol` of it, and spans at
/// most `MAX_STEP`.
fn segment_count(r: f64, sweep: f64, tol: f64) -> usize {
    let step = (2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos()).min(MAX_STEP);
    // An arc spanning exactly whole steps (one piece of a circle flattened before, as a hatch
    // traced round a circle keeps it) must not gain a piece from rounding: its points would then
    // miss the circle's own and leave slivers between the two.
    ((sweep / step - 1e-9).ceil() as usize).clamp(1, MAX_SEGMENTS)
}

/// Points on an ellipse from parameter `t0` to `t1`, counter-clockwise, ends included.
fn ellipse_points(
    center: [f64; 2],
    major: [f64; 2],
    minor: [f64; 2],
    t0: f64,
    t1: f64,
    tol: f64,
) -> Vec<[f64; 2]> {
    let sweep = sweep(t0, t1);
    let n = segment_count(major[0].hypot(major[1]), sweep, tol);
    (0..=n)
        .map(|i| {
            let (s, c) = (t0 + sweep * i as f64 / n as f64).sin_cos();
            [
                center[0] + major[0] * c + minor[0] * s,
                center[1] + major[1] * c + minor[1] * s,
            ]
        })
        .collect()
}

/// A polyline whose segments may be bulged arcs. Each vertex carries the bulge of the
/// segment that starts at it. A closed result does not repeat the first point.
fn bulge_points(vertices: &[([f64; 2], f64)], closed: bool, tol: f64) -> Vec<[f64; 2]> {
    let n = vertices.len();
    if n == 0 {
        return Vec::new();
    }
    let mut points = vec![vertices[0].0];
    let segments = if closed { n } else { n - 1 };
    for i in 0..segments {
        let (p0, bulge) = vertices[i];
        let p1 = vertices[(i + 1) % n].0;
        if bulge.abs() >= MIN_BULGE {
            points.extend(bulge_interior(p0, p1, bulge, tol));
        }
        points.push(p1);
    }
    if closed {
        points.pop();
    }
    points
}

/// The sampled points strictly between `p0` and `p1` on the arc with this bulge.
/// Included angle is `4·atan(b)`; positive bulges turn counter-clockwise.
fn bulge_interior(p0: [f64; 2], p1: [f64; 2], b: f64, tol: f64) -> Vec<[f64; 2]> {
    let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
    let d = dx.hypot(dy);
    if d == 0.0 {
        return Vec::new();
    }
    let r = d * (1.0 + b * b) / (4.0 * b.abs());
    // Signed distance from the chord midpoint to the centre, along the chord's left normal.
    let h = d * (1.0 - b * b) / (4.0 * b);
    let c = [
        (p0[0] + p1[0]) / 2.0 - dy / d * h,
        (p0[1] + p1[1]) / 2.0 + dx / d * h,
    ];
    let a0 = (p0[1] - c[1]).atan2(p0[0] - c[0]);
    let a1 = (p1[1] - c[1]).atan2(p1[0] - c[0]);
    let mut arc = if b > 0.0 {
        arc_points(c, r, a0, a1, tol)
    } else {
        let mut arc = arc_points(c, r, a1, a0, tol);
        arc.reverse();
        arc
    };
    arc.pop();
    arc.remove(0);
    arc
}

/// Points of one hatch boundary edge, in edge order.
fn edge_points(edge: &BoundaryEdge, tol: f64) -> Vec<[f64; 2]> {
    match edge {
        BoundaryEdge::Line(l) => vec![[l.start.x, l.start.y], [l.end.x, l.end.y]],
        BoundaryEdge::CircularArc(a) => {
            let c = [a.center.x, a.center.y];
            if a.counter_clockwise {
                arc_points(c, a.radius, a.start_angle, a.end_angle, tol)
            } else {
                // Clockwise edges store their angles mirrored: walk -start to -end clockwise.
                let mut points = arc_points(c, a.radius, -a.end_angle, -a.start_angle, tol);
                points.reverse();
                points
            }
        }
        BoundaryEdge::EllipticArc(a) => {
            let (c, major) = (
                [a.center.x, a.center.y],
                [a.major_axis_endpoint.x, a.major_axis_endpoint.y],
            );
            let ratio = a.minor_axis_ratio;
            let minor = [-major[1] * ratio, major[0] * ratio];
            if a.counter_clockwise {
                ellipse_points(c, major, minor, a.start_angle, a.end_angle, tol)
            } else {
                let mut points = ellipse_points(c, major, minor, -a.end_angle, -a.start_angle, tol);
                points.reverse();
                points
            }
        }
        BoundaryEdge::Spline(s) => {
            // Control points carry their weight in z.
            let control: Vec<[f64; 2]> = s.control_points.iter().map(xy).collect();
            let weights = s
                .rational
                .then(|| s.control_points.iter().map(|p| p.z).collect());
            usize::try_from(s.degree)
                .ok()
                .and_then(|d| nurbs_points(d, control, s.knots.clone(), weights, tol))
                .unwrap_or_else(|| s.fit_points.iter().map(|p| [p.x, p.y]).collect())
        }
        BoundaryEdge::Polyline(p) => {
            // Polyline edge vertices are (x, y, bulge).
            let vertices: Vec<_> = p.vertices.iter().map(|v| ([v.x, v.y], v.z)).collect();
            bulge_points(&vertices, p.is_closed, tol)
        }
    }
}

/// Appends `points` to `ring`, skipping a point that repeats the ring's last one.
fn extend_ring(ring: &mut Vec<[f64; 2]>, points: Vec<[f64; 2]>) {
    for p in points {
        if ring.last().is_none_or(|&last| !same_point(last, p)) {
            ring.push(p);
        }
    }
}

/// What a TEXT draws, placed by `xf` (its normal already in it).
fn line_text(e: &Text, xf: &Xform) -> FlatText {
    use TextHorizontalAlignment as H;
    use TextVerticalAlignment as V;
    FlatText {
        value: e.value.clone(),
        style: e.style.clone(),
        height: e.height,
        layout: TextLayout::Line {
            insertion: xy(&e.insertion_point),
            alignment: e.alignment_point.as_ref().map(xy),
            rotation: e.rotation,
            width_factor: e.width_factor,
            horizontal: match e.horizontal_alignment {
                H::Left => HAlign::Left,
                H::Center => HAlign::Center,
                H::Right => HAlign::Right,
                H::Aligned => HAlign::Aligned,
                H::Middle => HAlign::Middle,
                H::Fit => HAlign::Fit,
            },
            vertical: match e.vertical_alignment {
                V::Baseline => VAlign::Baseline,
                V::Bottom => VAlign::Bottom,
                V::Middle => VAlign::Middle,
                V::Top => VAlign::Top,
            },
            backward: e.generation_flags & 2 != 0,
            upside_down: e.generation_flags & 4 != 0,
        },
        xf: *xf,
        logical: is_logical_text(&e.common),
    }
}

/// What an attribute draws, placed by `xf` (its normal already in it). A multi-line attribute
/// draws as its MTEXT.
fn attribute_text(a: &AttributeEntity, xf: &Xform) -> FlatText {
    if let Some(mtext) = a.embedded_mtext.as_deref().filter(|_| a.is_multiline) {
        let mut text = block_text(mtext, xf);
        text.logical |= is_logical_text(&a.common);
        return text;
    }
    use HorizontalAlignment as H;
    use VerticalAlignment as V;
    FlatText {
        value: a.value.clone(),
        style: a.text_style.clone(),
        height: a.height,
        layout: TextLayout::Line {
            insertion: xy(&a.insertion_point),
            alignment: Some(xy(&a.alignment_point)),
            rotation: a.rotation,
            width_factor: a.width_factor,
            horizontal: match a.horizontal_alignment {
                H::Left => HAlign::Left,
                H::Center => HAlign::Center,
                H::Right => HAlign::Right,
                H::Aligned => HAlign::Aligned,
                H::Middle => HAlign::Middle,
                H::Fit => HAlign::Fit,
            },
            vertical: match a.vertical_alignment {
                V::Baseline => VAlign::Baseline,
                V::Bottom => VAlign::Bottom,
                V::Middle => VAlign::Middle,
                V::Top => VAlign::Top,
            },
            backward: a.text_generation_flags & 2 != 0,
            upside_down: a.text_generation_flags & 4 != 0,
        },
        xf: *xf,
        logical: is_logical_text(&a.common),
    }
}

/// What an MTEXT draws, placed by `xf`.
fn block_text(e: &MText, xf: &Xform) -> FlatText {
    FlatText {
        value: e.value.clone(),
        style: e.style.clone(),
        height: e.height,
        layout: TextLayout::Block {
            insertion: xy(&e.insertion_point),
            rotation: e.rotation,
            width: e.rectangle_width,
            attachment: e.attachment_point as u8,
            line_spacing: e.line_spacing_factor,
        },
        xf: *xf,
        logical: is_logical_text(&e.common),
    }
}

/// Approximate glyph bounds for picking and drawing checks. Widths use the same simple
/// estimate as before, but placement follows the text's actual attachment and alignment.
fn text_box(text: &FlatText) -> Vec<[f64; 2]> {
    let height = text.height;
    let (bounds, place) = match text.layout {
        TextLayout::Line {
            insertion,
            alignment,
            rotation,
            width_factor,
            horizontal,
            vertical,
            backward,
            upside_down,
        } => {
            let width = text.value.chars().count() as f64 * 0.6;
            let factor = if width_factor > 0.0 {
                width_factor
            } else {
                1.0
            };
            let (mut sx, mut sy) = (height * factor, height);
            let (mut dx, mut dy) = (0.0, 0.0);
            let (mut at, mut angle) = (insertion, rotation);
            match alignment {
                Some(end) if matches!(horizontal, HAlign::Aligned | HAlign::Fit) && width > 0.0 => {
                    let (x, y) = (end[0] - insertion[0], end[1] - insertion[1]);
                    let length = x.hypot(y);
                    if length > 0.0 {
                        angle = y.atan2(x);
                        sx = length / width;
                        if horizontal == HAlign::Aligned {
                            sy = sx / factor;
                        }
                    }
                }
                Some(end) if horizontal != HAlign::Left || vertical != VAlign::Baseline => {
                    at = end;
                    dx = match horizontal {
                        HAlign::Center | HAlign::Middle => -width / 2.0,
                        HAlign::Right => -width,
                        _ => 0.0,
                    };
                    dy = match (horizontal, vertical) {
                        (HAlign::Middle, _) | (_, VAlign::Middle) => -0.5,
                        (_, VAlign::Top) => -1.0,
                        (_, VAlign::Bottom) => 1.0 / 3.0,
                        _ => 0.0,
                    };
                }
                _ => {}
            }
            let place = Xform::translate(dx, dy)
                .then(&Xform::scale(
                    if backward { -sx } else { sx },
                    if upside_down { -sy } else { sy },
                ))
                .then(&Xform::rotate(angle))
                .then(&Xform::translate(at[0], at[1]));
            ([0.0, 0.0, width, 1.0], place)
        }
        TextLayout::Block {
            insertion,
            rotation,
            width,
            attachment,
            line_spacing,
        } => {
            let plain = opencadcodec::entities::mtext_format::parse_mtext(&text.value, true)
                .to_plain_text();
            let wrap = (width > 0.0 && height > 0.0).then_some(width / height);
            let mut line_widths = Vec::new();
            for paragraph in plain.split('\n') {
                if let Some(limit) = wrap {
                    let mut line_width = 0.0;
                    for word in paragraph.split(' ') {
                        let word_width = word.chars().count() as f64 * 0.6;
                        if line_width > 0.0 && line_width + 0.6 + word_width > limit {
                            line_widths.push(line_width);
                            line_width = 0.0;
                        }
                        line_width += if line_width > 0.0 { 0.6 } else { 0.0 } + word_width;
                    }
                    line_widths.push(line_width);
                } else {
                    line_widths.push(paragraph.chars().count() as f64 * 0.6);
                }
            }
            let pitch = 5.0 / 3.0
                * if line_spacing > 0.0 {
                    line_spacing
                } else {
                    1.0
                };
            let total = 1.0 + pitch * line_widths.len().saturating_sub(1) as f64;
            let max_width = line_widths.into_iter().fold(0.0_f64, f64::max);
            let attachment = attachment.clamp(1, 9) - 1;
            let x = -f64::from(attachment % 3) * max_width / 2.0;
            let top = f64::from(attachment / 3) * total / 2.0;
            let place = Xform::scale(height, height)
                .then(&Xform::rotate(rotation))
                .then(&Xform::translate(insertion[0], insertion[1]));
            ([x, top - total, x + max_width, top], place)
        }
    };
    let [left, bottom, right, top] = bounds;
    [[left, bottom], [right, bottom], [right, top], [left, top]]
        .into_iter()
        .map(|p| place.apply(p))
        .collect()
}

/// The largest factor by which `xf` stretches a unit length along either axis.
fn max_stretch(xf: &Xform) -> f64 {
    let m = &xf.m;
    m[0][0].hypot(m[1][0]).max(m[0][1].hypot(m[1][1]))
}

fn xy(v: &Vector3) -> [f64; 2] {
    [v.x, v.y]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tiny_circle_still_gets_16_segments() {
        // A 0.4 mm dot at 0.05 mm: the tolerance alone would give 5.
        assert_eq!(segment_count(0.2, TAU, 0.05), 16);
        // A 12 mm bar keeps what its tolerance needs (25), a big one more.
        assert_eq!(segment_count(6.0, TAU, 0.05), 25);
        assert!(segment_count(1000.0, TAU, 0.05) > 300);
    }

    #[test]
    fn an_arc_of_exactly_whole_steps_gains_no_piece_from_rounding() {
        let step = TAU / 16.0;
        for k in 1..=16 {
            // Swept angles worked out from points, a hair over the exact multiple.
            let sweep = k as f64 * step * (1.0 + 1e-15);
            // Small enough for the floor to set the step.
            assert_eq!(segment_count(0.2, sweep, 0.05), k);
        }
    }

    fn box_bounds(points: &[[f64; 2]]) -> [f64; 4] {
        points.iter().fold(
            [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ],
            |[left, bottom, right, top], p| {
                [
                    left.min(p[0]),
                    bottom.min(p[1]),
                    right.max(p[0]),
                    top.max(p[1]),
                ]
            },
        )
    }

    fn assert_bounds(actual: [f64; 4], expected: [f64; 4]) {
        for (a, e) in actual.into_iter().zip(expected) {
            assert!((a - e).abs() < 1e-9, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn mtext_boxes_follow_all_nine_attachments_and_paragraphs() {
        let e = MText::with_value("AB\\PCD", Vector3::new(20.0, 30.0, 0.0)).with_height(3.0);
        let mut text = block_text(&e, &Xform::identity());
        for attachment in 1_u8..=9 {
            if let TextLayout::Block {
                attachment: value, ..
            } = &mut text.layout
            {
                *value = attachment;
            }
            let x = 20.0 - f64::from((attachment - 1) % 3) * 1.8;
            let top = 30.0 + f64::from((attachment - 1) / 3) * 4.0;
            assert_bounds(box_bounds(&text_box(&text)), [x, top - 8.0, x + 3.6, top]);
        }
    }

    #[test]
    fn text_boxes_follow_centering_rotation_width_and_fit() {
        let e = Text::with_value("AB", Vector3::new(0.0, 0.0, 0.0)).with_height(2.0);
        let mut text = line_text(&e, &Xform::identity());
        text.layout = TextLayout::Line {
            insertion: [0.0, 0.0],
            alignment: Some([10.0, 20.0]),
            rotation: std::f64::consts::FRAC_PI_2,
            width_factor: 2.0,
            horizontal: HAlign::Center,
            vertical: VAlign::Middle,
            backward: false,
            upside_down: false,
        };
        assert_bounds(box_bounds(&text_box(&text)), [9.0, 17.6, 11.0, 22.4]);
        if let TextLayout::Line {
            horizontal,
            alignment,
            ..
        } = &mut text.layout
        {
            *horizontal = HAlign::Fit;
            *alignment = Some([0.0, 6.0]);
        }
        assert_bounds(box_bounds(&text_box(&text)), [-2.0, 0.0, 0.0, 6.0]);
    }
}
