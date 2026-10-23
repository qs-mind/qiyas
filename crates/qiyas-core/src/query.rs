//! Read-only questions about the open drawing: what is there, what it says, what it measures.
//! These are the AI's eyes; nothing here changes the session.

use schemars::JsonSchema;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

use opencadcodec::entities::mtext_format::parse_mtext;
use opencadcodec::entities::{Dimension, Insert};
use opencadcodec::tables::normalize_name;
use opencadcodec::{CadDocument, EntityType};
use qiyas_geom::{
    flatten_with, insert_instances, is_overlay_block, is_xref_block, Xform, XrefDocs,
    MAX_XREF_DEPTH,
};
use qiyas_render::{layer_on_in_file_with, layer_shown_with};
use serde::{Deserialize, Serialize};

use crate::session::{effective_block_name, variant_name, Extent};
use crate::Session;

/// Most rows one `find_entities` page returns.
const MAX_LIMIT: usize = 500;
/// Deepest block nesting `read_text` follows; deeper inserts (or cycles) are skipped.
const MAX_DEPTH: usize = 32;
/// Most block instances `read_text` walks, so a huge array of blocks cannot hang the app.
const MAX_INSTANCES: usize = 100_000;

/// Which entities `find_entities` returns. Every filter that is set must match; names match
/// case-insensitively. Missing fields take their defaults when read from JSON.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EntityFilter {
    /// Sheet block or layout name; the current sheet when `None`.
    pub sheet: Option<String>,
    pub layer: Option<String>,
    /// The entity type, e.g. "Line", "LwPolyline", "Insert", "Text".
    pub kind: Option<String>,
    /// Only inserts of this block.
    pub block_name: Option<String>,
    /// Text in a Text or MText, or in an insert's attribute values.
    pub text_contains: Option<String>,
    /// `[[min_x, min_y], [max_x, max_y]]`: entities with at least one point inside.
    pub within: Option<[[f64; 2]; 2]>,
    pub offset: usize,
    /// Rows per page, at most 500. 0 returns only the total.
    pub limit: usize,
}

impl Default for EntityFilter {
    fn default() -> Self {
        EntityFilter {
            sheet: None,
            layer: None,
            kind: None,
            block_name: None,
            text_contains: None,
            within: None,
            offset: 0,
            limit: 100,
        }
    }
}

/// One entity found. Length and area are in drawing units; bounds in world coordinates. An
/// entity at the top of an xref has `xref` set and `chain` holding the xref's reference (give both
/// handle and chain to measure it); its handle is the xref drawing's.
#[derive(Debug, Clone, Serialize)]
pub struct EntityRow {
    pub handle: u64,
    pub kind: String,
    /// For an xref's entity, `XREF|LAYER` (layer "0" takes the xref reference's).
    pub layer: String,
    pub block_name: Option<String>,
    /// A Text's value, an MText's plain text, or an insert's attribute values joined by " | ".
    pub text: Option<String>,
    pub length: Option<f64>,
    pub area: Option<f64>,
    /// `[[min_x, min_y], [max_x, max_y]]`.
    pub bounds: [[f64; 2]; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xref: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<u64>,
}

/// One page of results. `total` counts every match; `next_offset` is set when more remain.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub rows: Vec<T>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

/// One piece of text in world coordinates. `handle` is the Text or MText, or the insert that
/// carries an attribute.
#[derive(Debug, Clone, Serialize)]
pub struct TextRow {
    pub handle: u64,
    pub text: String,
    pub at: [f64; 2],
    pub height: f64,
    /// The block the text sits in, when it was reached through an insert.
    pub inside_block: Option<String>,
    /// The xref the text is in (the outermost, for a nested xref); `handle` is then that
    /// drawing's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xref: Option<String>,
}

/// One dimension. `measurement` is computed from its definition points: drawing units, or
/// degrees for angular ones; `None` for kinds not measured yet.
#[derive(Debug, Clone, Serialize)]
pub struct DimRow {
    pub handle: u64,
    /// "Linear", "Aligned", "Angular", "Angular3Point", "Diameter", "Radius", ...
    pub kind: String,
    pub measurement: Option<f64>,
    /// The text written instead of the measured value, if any.
    pub text_override: Option<String>,
    /// Where the dimension text sits.
    pub at: [f64; 2],
}

/// How many references of one block sit on one layer.
#[derive(Debug, Clone, Serialize)]
pub struct BlockCount {
    pub block_name: String,
    pub layer: String,
    pub count: usize,
    /// The distinct attribute values (trimmed, not empty) of the references counted, sorted.
    pub attributes: Vec<String>,
    pub count_scope: &'static str,
    pub visibility_policy: &'static str,
    pub limitations: &'static str,
}

/// A reference on a sheet to a loaded xref, placed by its first cell.
pub(crate) struct XrefAt<'a> {
    /// The reference, in the host drawing.
    pub insert: &'a Insert,
    /// The xref drawing.
    pub doc: &'a CadDocument,
    /// Xref model space to world.
    pub xf: Xform,
}

impl XrefAt<'_> {
    /// The layer an entity at the top of the xref is drawn on: `XREF|LAYER`, or the reference's
    /// own layer for one on layer "0".
    pub fn layer_of(&self, e: &EntityType) -> String {
        match e.common().layer.as_str() {
            "0" => self.insert.common.layer.clone(),
            own => format!("{}|{own}", self.insert.block_name),
        }
    }
}

/// An entity `find_entities` looks at: one on the sheet, or one at the top of a sheet's xref.
pub(crate) struct Candidate<'a> {
    pub entity: &'a EntityType,
    /// The drawing it is in.
    pub doc: &'a CadDocument,
    pub layer: Cow<'a, str>,
    /// Its drawing's model space to world (identity on the sheet).
    pub xf: Xform,
    pub chain: Vec<u64>,
    pub xref: Option<&'a str>,
}

impl Session {
    /// The references on `sheet` (top level, not invisible) to loaded xrefs.
    pub(crate) fn sheet_xrefs(&self, sheet: &str) -> Vec<XrefAt<'_>> {
        let host = &self.drawing.doc;
        let docs = self.xref_docs();
        self.drawing
            .entities_in(sheet)
            .filter_map(|e| match e {
                EntityType::Insert(insert)
                    if !insert.common.invisible && is_xref_block(host, &insert.block_name) =>
                {
                    let doc = *docs.get(&normalize_name(&insert.block_name))?;
                    let xf = insert_instances(host, insert, &Xform::identity()).next()?;
                    Some(XrefAt { insert, doc, xf })
                }
                _ => None,
            })
            .collect()
    }

    /// The entities of a sheet that pass `f`, in drawing order, one page at a time; then those at
    /// the top of each xref on the sheet (a block reference inside an xref is one row).
    pub fn find_entities(&self, f: &EntityFilter) -> Page<EntityRow> {
        let xrefs = self.xref_docs();
        let matches = self.matching(f, &xrefs);
        let total = matches.len();
        let limit = f.limit.min(MAX_LIMIT);
        let rows: Vec<EntityRow> = matches
            .into_iter()
            .skip(f.offset)
            .take(limit)
            .map(|c| self.entity_row(&xrefs, c))
            .collect();
        let end = f.offset.saturating_add(rows.len());
        Page {
            next_offset: (end < total && !rows.is_empty()).then_some(end),
            rows,
            total,
        }
    }

    /// Every entity `find_entities` would list for `f`, before paging.
    pub(crate) fn matching<'a>(
        &'a self,
        f: &EntityFilter,
        xrefs: &XrefDocs<'a>,
    ) -> Vec<Candidate<'a>> {
        let layer = f.layer.as_deref().map(normalize_name);
        let block = f.block_name.as_deref().map(normalize_name);
        let needle = f.text_contains.as_deref().map(str::to_lowercase);
        let within = f.within.map(normalized_box);
        let sheet = self.sheet_block(f.sheet.as_deref());
        let host = &self.drawing.doc;
        let mut candidates: Vec<Candidate> = self
            .drawing
            .entities_in(&sheet)
            .map(|entity| Candidate {
                entity,
                doc: host,
                layer: Cow::Borrowed(&entity.common().layer),
                xf: Xform::identity(),
                chain: Vec::new(),
                xref: None,
            })
            .collect();
        for x in self.sheet_xrefs(&sheet) {
            for entity in x.doc.entities_in_block("*Model_Space") {
                candidates.push(Candidate {
                    entity,
                    doc: x.doc,
                    layer: Cow::Owned(x.layer_of(entity)),
                    xf: x.xf,
                    chain: vec![x.insert.common.handle.value()],
                    xref: Some(&x.insert.block_name),
                });
            }
        }
        candidates
            .into_iter()
            .filter(|c| {
                layer
                    .as_ref()
                    .is_none_or(|l| *l == normalize_name(&c.layer))
            })
            .filter(|c| {
                f.kind
                    .as_ref()
                    .is_none_or(|k| variant_name(c.entity).eq_ignore_ascii_case(k))
            })
            .filter(|c| {
                block.as_ref().is_none_or(|b| match c.entity {
                    // Either name of a dynamic block's anonymous copy matches.
                    EntityType::Insert(insert) => {
                        *b == normalize_name(&insert.block_name)
                            || *b == normalize_name(&effective_block_name(c.doc, insert))
                    }
                    _ => false,
                })
            })
            .filter(|c| {
                needle.as_ref().is_none_or(|n| {
                    texts_of(c.entity)
                        .iter()
                        .any(|t| t.to_lowercase().contains(n.as_str()))
                })
            })
            .filter(|c| {
                within.is_none_or(|b| self.points_of(xrefs, c).iter().any(|p| inside(*p, b)))
            })
            .collect()
    }

    /// Every Text, MText (formatting stripped) and insert attribute on a sheet, including those
    /// inside blocks and xrefs, in world coordinates. `within` keeps only text placed inside that
    /// box.
    pub fn read_text(&self, sheet: Option<&str>, within: Option<[[f64; 2]; 2]>) -> Vec<TextRow> {
        let sheet = self.sheet_block(sheet);
        let xrefs = self.xref_docs();
        let mut walk = TextWalk {
            xrefs: &xrefs,
            budget: MAX_INSTANCES,
            out: Vec::new(),
        };
        walk.collect(
            &self.drawing.doc,
            self.drawing.entities_in(&sheet),
            &Xform::identity(),
            None,
            0,
            &mut Vec::new(),
        );
        let mut out = walk.out;
        if let Some(b) = within.map(normalized_box) {
            out.retain(|t| inside(t.at, b));
        }
        out
    }

    /// The dimensions on a sheet, with the values they measure.
    pub fn read_dimensions(&self, sheet: Option<&str>) -> Vec<DimRow> {
        let sheet = self.sheet_block(sheet);
        self.drawing
            .entities_in(&sheet)
            .filter_map(|e| match e {
                EntityType::Dimension(d) => Some(dim_row(d)),
                _ => None,
            })
            .collect()
    }

    /// Block references on a sheet per block and layer, on visible layers only, most first.
    /// Counts top-level references, like the count command, by the insert's own layer; a dynamic
    /// block's anonymous copies count as the dynamic block. References at the top of the sheet's
    /// xrefs count on their `XREF|LAYER`.
    pub fn block_summary(&self, sheet: Option<&str>) -> Vec<BlockCount> {
        self.block_counts(sheet, true)
    }

    /// Block references on a sheet as the file has them: layers the user has hidden count, only
    /// layers off or frozen in the drawing do not. One row per block, layer and set of attribute
    /// values, so the references tagged "D1" are counted apart from those tagged "D2". Used by
    /// the comparison, which must not change with what the user is looking at.
    pub fn block_summary_in_file(&self, sheet: Option<&str>) -> Vec<BlockCount> {
        self.block_counts(sheet, false)
    }

    /// Shared bounded enumeration. An array INSERT is one record, not its visible cells.
    pub(crate) fn visit_countable_inserts(
        &self,
        sheet: Option<&str>,
        as_shown: bool,
        mut visit: impl FnMut(&Insert, &CadDocument, &str, Option<&Xform>),
    ) {
        let sheet = self.sheet_block(sheet);
        let host = &self.drawing.doc;
        let xrefs = self.xref_docs();
        let counted = |layer: &str| {
            if as_shown {
                layer_shown_with(host, &xrefs, &self.visibility, layer)
            } else {
                layer_on_in_file_with(host, &xrefs, layer)
            }
        };
        for e in self.drawing.entities_in(&sheet) {
            if let EntityType::Insert(insert) = e {
                if !insert.common.invisible && counted(&insert.common.layer) {
                    visit(insert, host, &insert.common.layer, None);
                }
            }
        }
        for x in self.sheet_xrefs(&sheet) {
            if x.insert.common.invisible || !counted(&x.insert.common.layer) {
                continue;
            }
            for e in x.doc.entities_in_block("*Model_Space") {
                if let EntityType::Insert(insert) = e {
                    let layer = x.layer_of(e);
                    if !insert.common.invisible && counted(&layer) {
                        visit(insert, x.doc, &layer, Some(&x.xf));
                    }
                }
            }
        }
    }

    /// `block_summary` when `as_shown`, else `block_summary_in_file`.
    fn block_counts(&self, sheet: Option<&str>, as_shown: bool) -> Vec<BlockCount> {
        // (block, layer, attribute values when split by them) -> (count, attribute values).
        type Key = (String, String, Vec<String>);
        let mut counts: HashMap<Key, (usize, BTreeSet<String>)> = HashMap::new();
        self.visit_countable_inserts(sheet, as_shown, |insert, doc, layer, _| {
            let values: BTreeSet<String> = insert
                .attributes
                .iter()
                .map(|a| a.value.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect();
            let split = if as_shown {
                Vec::new()
            } else {
                values.iter().cloned().collect()
            };
            let block = effective_block_name(doc, insert);
            let entry = counts.entry((block, layer.to_string(), split)).or_default();
            entry.0 += 1;
            entry.1.extend(values);
        });
        let mut summary: Vec<BlockCount> = counts
            .into_iter()
            .map(|((block_name, layer, _), (count, values))| BlockCount {
                block_name,
                layer,
                count,
                attributes: values.into_iter().collect(),
                count_scope: crate::INSERT_COUNT_SCOPE,
                visibility_policy: if as_shown { "as_shown" } else { "in_file" },
                limitations: crate::INSERT_COUNT_LIMITATIONS,
            })
            .collect();
        summary.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.block_name.cmp(&b.block_name))
                .then_with(|| a.layer.cmp(&b.layer))
                .then_with(|| a.attributes.cmp(&b.attributes))
        });
        summary
    }

    /// The block of the sheet named by its block or layout name (case-insensitive); the
    /// current sheet when `None`. An unknown name is used as a block name as it is.
    pub(crate) fn sheet_block(&self, sheet: Option<&str>) -> String {
        let Some(name) = sheet else {
            return self.sheet.clone();
        };
        self.drawing
            .sheets()
            .into_iter()
            .find(|s| s.block.eq_ignore_ascii_case(name) || s.name.eq_ignore_ascii_case(name))
            .map_or_else(|| name.to_string(), |s| s.block)
    }

    pub(crate) fn entity_row(&self, xrefs: &XrefDocs, c: Candidate) -> EntityRow {
        let e = c.entity;
        let Extent { length, area, .. } = self.extent(c.doc, e, &c.xf);
        let texts = texts_of(e);
        EntityRow {
            handle: e.common().handle.value(),
            kind: variant_name(e),
            block_name: match e {
                EntityType::Insert(insert) => Some(effective_block_name(c.doc, insert)),
                _ => None,
            },
            text: (!texts.is_empty()).then(|| texts.join(" | ")),
            length,
            area,
            bounds: bounds(&self.points_of(xrefs, &c)),
            layer: c.layer.into_owned(),
            xref: c.xref.map(str::to_string),
            chain: c.chain,
        }
    }

    /// The entity's flattened points in world coordinates; for an entity that does not
    /// flatten (a dimension or table), the corners of its own bounding box.
    pub(crate) fn points_of(&self, xrefs: &XrefDocs, c: &Candidate) -> Vec<[f64; 2]> {
        let paths = flatten_with(c.doc, xrefs, c.entity, &c.xf, self.batch_tol()).paths;
        let points: Vec<[f64; 2]> = paths
            .into_iter()
            .flat_map(|p| {
                p.text
                    .as_ref()
                    .and_then(|text| {
                        self.text.borrow_mut().lay_out_text(
                            c.doc,
                            xrefs,
                            text,
                            self.arabic_reversed,
                        )
                    })
                    .map(|laid| laid.corners.to_vec())
                    .unwrap_or(p.path.points)
            })
            .collect();
        if !points.is_empty() {
            return points;
        }
        let b = c.entity.as_entity().bounding_box();
        [
            [b.min.x, b.min.y],
            [b.max.x, b.min.y],
            [b.max.x, b.max.y],
            [b.min.x, b.max.y],
        ]
        .map(|p| c.xf.apply(p))
        .to_vec()
    }
}

/// The text an entity carries: a Text's value, an MText's plain text, an insert's attribute values.
pub(crate) fn texts_of(e: &EntityType) -> Vec<String> {
    match e {
        EntityType::Text(t) => vec![t.value.clone()],
        EntityType::MText(t) => vec![plain_text(&t.value)],
        EntityType::Insert(insert) => insert.attributes.iter().map(|a| a.value.clone()).collect(),
        _ => Vec::new(),
    }
}

/// MText (or table cell) content with its formatting codes removed; paragraphs on new lines.
pub(crate) fn plain_text(value: &str) -> String {
    parse_mtext(value, true).to_plain_text()
}

/// Text collected by `read_text`.
struct TextWalk<'x, 'a> {
    xrefs: &'x XrefDocs<'a>,
    /// Block instances still allowed.
    budget: usize,
    out: Vec<TextRow>,
}

impl TextWalk<'_, '_> {
    /// Appends the text of `entities` of `doc`, placed by `xf`, walking into blocks and loaded
    /// xrefs. `xref_stack` holds the xrefs entered, outermost first.
    fn collect<'d>(
        &mut self,
        doc: &'d CadDocument,
        entities: impl Iterator<Item = &'d EntityType>,
        xf: &Xform,
        inside_block: Option<&str>,
        depth: usize,
        xref_stack: &mut Vec<String>,
    ) {
        for e in entities {
            let handle = e.common().handle.value();
            let xref = xref_stack.first().cloned();
            let out = &mut self.out;
            let mut push = |text: String, at: &opencadcodec::Vector3, height: f64| {
                out.push(TextRow {
                    handle,
                    text,
                    at: xf.apply([at.x, at.y]),
                    height: scaled_height(xf, height),
                    inside_block: inside_block.map(str::to_string),
                    xref: xref.clone(),
                })
            };
            match e {
                EntityType::Text(t) => push(t.value.clone(), &t.insertion_point, t.height),
                EntityType::MText(t) => push(plain_text(&t.value), &t.insertion_point, t.height),
                EntityType::Insert(insert) => {
                    // Attributes are stored where the insert sits, not in the block's coordinates.
                    for a in &insert.attributes {
                        push(a.value.clone(), &a.insertion_point, a.height);
                    }
                    if depth >= MAX_DEPTH {
                        continue;
                    }
                    let (inner, block, inside): (&CadDocument, &str, Option<&str>) =
                        if is_xref_block(doc, &insert.block_name) {
                            let key = normalize_name(&insert.block_name);
                            let Some(&xdoc) = self.xrefs.get(&key) else {
                                continue;
                            };
                            let cycle = xref_stack.iter().any(|n| normalize_name(n) == key);
                            if cycle || xref_stack.len() >= MAX_XREF_DEPTH {
                                continue;
                            }
                            // An overlay inside an xref is not shown (as AutoCAD).
                            if !xref_stack.is_empty() && is_overlay_block(doc, &insert.block_name) {
                                continue;
                            }
                            (xdoc, "*Model_Space", None)
                        } else {
                            (doc, &insert.block_name, Some(&insert.block_name))
                        };
                    let entering = inside.is_none();
                    if entering {
                        xref_stack.push(insert.block_name.clone());
                    }
                    for instance in insert_instances(doc, insert, xf) {
                        if self.budget == 0 {
                            break;
                        }
                        self.budget -= 1;
                        let entities = inner.entities_in_block(block);
                        self.collect(inner, entities, &instance, inside, depth + 1, xref_stack);
                    }
                    if entering {
                        xref_stack.pop();
                    }
                    if self.budget == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

/// A text height after `xf`: the length its vertical stroke becomes.
fn scaled_height(xf: &Xform, height: f64) -> f64 {
    let [x0, y0] = xf.apply([0.0, 0.0]);
    let [x1, y1] = xf.apply([0.0, height]);
    (x1 - x0).hypot(y1 - y0)
}

fn dim_row(d: &Dimension) -> DimRow {
    let base = d.base();
    let measurement = match d {
        Dimension::Linear(_)
        | Dimension::Aligned(_)
        | Dimension::Radius(_)
        | Dimension::Diameter(_)
        | Dimension::Angular2Ln(_)
        | Dimension::Angular3Pt(_) => Some(d.measurement()).filter(|m| m.is_finite()),
        _ => None,
    };
    DimRow {
        handle: base.common.handle.value(),
        kind: format!("{:?}", base.dimension_type),
        measurement,
        text_override: base
            .text_override()
            .filter(|t| !t.is_empty() && *t != "<>")
            .map(str::to_string),
        at: [base.text_middle_point.x, base.text_middle_point.y],
    }
}

/// The box with its corners in order, whichever way round they were given.
fn normalized_box([a, b]: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    [
        [a[0].min(b[0]), a[1].min(b[1])],
        [a[0].max(b[0]), a[1].max(b[1])],
    ]
}

fn inside(p: [f64; 2], [lo, hi]: [[f64; 2]; 2]) -> bool {
    (lo[0]..=hi[0]).contains(&p[0]) && (lo[1]..=hi[1]).contains(&p[1])
}

/// `[[min_x, min_y], [max_x, max_y]]` of `points`; all zeros when there are none.
pub(crate) fn bounds(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let Some(&first) = points.first() else {
        return [[0.0; 2]; 2];
    };
    points.iter().fold([first, first], |[lo, hi], p| {
        [
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        ]
    })
}
