//! Blocks and attributes: making a block from entities on the sheet (`Command::CreateBlock`),
//! placing references to it (`InsertBlock`, also from another drawing), writing it to a drawing
//! of its own (`WriteBlock`), editing it in place (`EditBlockInPlace`, then `SaveBlockEdits` or
//! `DiscardBlockEdits`), attribute definitions and values (`AttDef`, `EditAttributes`,
//! `EditAttDef`, `SyncAttributes`) and redefining it from another drawing (`RedefineBlock`).
//!
//! Editing in place is a mode of the session (`Session::block_edit`): the block's entities are
//! copied onto the sheet where the reference draws them (temporary entities, never an undo step
//! of their own), the reference is hidden, everything else is drawn faded and can't be picked or
//! changed. The usual commands then work on the copies, each one undo step that can't reach back
//! before the edit began. Closing never undoes anything: saving makes what is on the sheet beyond
//! what was there before the block's new contents, through the reference's inverse transform
//! (every reference changes); discarding drops it. Either way the copies leave the sheet, and the
//! steps made while editing leave the history (with any earlier step that could put the block's
//! records back under it), as AutoCAD's REFCLOSE does; the outcome says how many.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use opencadcodec::entities::{AttributeDefinition, AttributeEntity, Insert};
use opencadcodec::types::{Matrix3, Matrix4, Transform};
use opencadcodec::{BlockRecord, CadDocument, DwgWriter, EntityType, Handle, Vector3};
use qiyas_geom::is_xref_block;
use qiyas_render::{Batch, Range, FILL_BACKGROUND};
use serde::Serialize;

use crate::drawing::Drawing;
use crate::modify::{add_fresh_to, place, Placing};
use crate::text::mark_logical;
use crate::undo::{DocChange, TablesSnapshot};
use crate::units::insert_unit_factor;
use crate::{Command, CoreError, Outcome, Session, Target};

/// Characters AutoCAD refuses in a block name.
pub(crate) const BAD_NAME_CHARS: &[char] = &[
    '<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`',
];

/// A named block of the drawing and its attribute definitions, in the order they sit in it.
#[derive(Debug, Clone, Serialize)]
pub struct BlockInfo {
    pub name: String,
    pub attdefs: Vec<AttDefInfo>,
}

/// One attribute definition: its tag, the prompt asked on insert, the value it takes when none
/// is given, and whether it is constant (never asked: it draws from the block).
#[derive(Debug, Clone, Serialize)]
pub struct AttDefInfo {
    pub tag: String,
    pub prompt: String,
    pub default: String,
    pub constant: bool,
}

/// A block being edited in place (see the module documentation).
#[derive(Debug)]
pub(crate) struct BlockEdit {
    /// The block, as the drawing spells it.
    pub name: String,
    /// The reference edited through, hidden while editing.
    pub insert: Handle,
    /// The sheet the copies are on.
    pub sheet: String,
    /// Each temporary copy on the sheet, with the block entity it shows and the copy as it was
    /// made (a copy still like that leaves its block entity as it is).
    pub temps: HashMap<u64, (Handle, Arc<EntityType>)>,
    /// The sheet's entities before the edit began: drawn faded, and locked.
    pub outside: HashSet<u64>,
    /// The world back to block coordinates.
    pub inverse: Transform,
    /// The drawing had no unsaved changes when the edit began.
    pub was_clean: bool,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn finite(values: &[f64]) -> Result<(), CoreError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid("Points, sizes and angles must be finite numbers"))
    }
}

fn v3([x, y]: [f64; 2]) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

/// A name a new block may take: not empty, at most 255 characters, none AutoCAD refuses, not
/// anonymous (`*`), and not one the drawing has already.
fn check_new_name(doc: &CadDocument, name: &str) -> Result<String, CoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("Give the block a name"));
    }
    if name.chars().count() > 255 {
        return Err(invalid("A block name has at most 255 characters"));
    }
    if let Some(c) = name
        .chars()
        .find(|c| BAD_NAME_CHARS.contains(c) || c.is_control())
    {
        return Err(invalid(format!("A block name can't hold \"{c}\"")));
    }
    if doc.block_records.contains(name) {
        return Err(invalid(format!("The drawing has a block {name} already")));
    }
    Ok(name.to_string())
}

/// `a` and `b` name the same file: as written, or once resolved.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(x), Ok(y)) if x == y
        )
}

/// The named block `name` of `doc` (not a sheet, an xref or an anonymous block), or why not.
fn named_block<'a>(doc: &'a CadDocument, name: &str) -> Result<&'a BlockRecord, CoreError> {
    let record = doc
        .block_records
        .get(name.trim())
        .ok_or_else(|| invalid(format!("The drawing has no block {}", name.trim())))?;
    if record.name.starts_with('*') {
        return Err(invalid(format!("{} is not a named block", record.name)));
    }
    if is_xref_block(doc, &record.name) {
        return Err(invalid(format!("{} is an xref", record.name)));
    }
    Ok(record)
}

/// The block `block` of `doc` holds a reference to the block `name`, at any depth (up to 32).
fn uses(doc: &CadDocument, block: &str, name: &str, depth: usize) -> bool {
    depth < 32
        && doc.entities_in_block(block).any(|e| match e {
            EntityType::Insert(i) => {
                i.block_name.eq_ignore_ascii_case(name) || uses(doc, &i.block_name, name, depth + 1)
            }
            _ => false,
        })
}

/// The named blocks of `doc` (not sheets, xrefs or anonymous blocks) with their attribute
/// definitions, sorted by name.
fn block_infos(doc: &CadDocument) -> Vec<BlockInfo> {
    let mut out: Vec<BlockInfo> = doc
        .block_records
        .iter()
        .filter(|b| !b.name.starts_with('*') && !is_xref_block(doc, &b.name))
        .map(|b| BlockInfo {
            name: b.name.clone(),
            attdefs: doc
                .entities_in_block(&b.name)
                .filter_map(|e| match e {
                    EntityType::AttributeDefinition(a) => Some(AttDefInfo {
                        tag: a.tag.clone(),
                        prompt: a.prompt.clone(),
                        default: a.default_value.clone(),
                        constant: a.flags.constant,
                    }),
                    _ => None,
                })
                .collect(),
        })
        .collect();
    out.sort_by_key(|b| b.name.to_uppercase());
    out
}

/// The named blocks of the drawing at `path`, opened read-only, with their attribute
/// definitions (the Design Center).
pub fn drawing_blocks(path: &Path) -> Result<Vec<BlockInfo>, CoreError> {
    Ok(block_infos(&Drawing::open(path)?.doc))
}

/// Block coordinates of `record` to where `insert` draws them (its first cell).
fn forward(record: &BlockRecord, insert: &Insert) -> Transform {
    let b = record.base_point;
    Transform::from_translation(Vector3::new(-b.x, -b.y, -b.z)).then(&insert.get_transform())
}

/// The inverse of `forward`, for a reference with nonzero x and y scales.
fn inverse(record: &BlockRecord, insert: &Insert) -> Transform {
    let p = insert.insert_point;
    let z = if insert.z_scale() == 0.0 {
        1.0
    } else {
        insert.z_scale()
    };
    let back = Matrix4::scaling(1.0 / insert.x_scale(), 1.0 / insert.y_scale(), 1.0 / z)
        * Matrix4::rotation_z(-insert.rotation)
        * Matrix4::translation(-p.x, -p.y, -p.z)
        * Matrix4::from_matrix3(Matrix3::arbitrary_axis(insert.normal).transpose());
    Transform::from_matrix(back).then(&Transform::from_translation(record.base_point))
}

/// The attributes a reference to the block `record` of `doc` placed by `xf` gets: one per
/// attribute definition that is not constant (a constant one draws from the block), with the
/// value `values` gives its tag (any case), else its default. Every tag of `values` must be the
/// block's.
fn attributes_for(
    doc: &CadDocument,
    record: &BlockRecord,
    xf: &Transform,
    values: &BTreeMap<String, String>,
) -> Result<Vec<AttributeEntity>, CoreError> {
    let attdefs: Vec<&AttributeDefinition> = doc
        .entities_in_block(&record.name)
        .filter_map(|e| match e {
            EntityType::AttributeDefinition(a) => Some(a),
            _ => None,
        })
        .collect();
    for tag in values.keys() {
        if !attdefs
            .iter()
            .any(|a| a.tag.eq_ignore_ascii_case(tag.trim()))
        {
            return Err(invalid(format!(
                "Block {} has no attribute {tag}",
                record.name
            )));
        }
    }
    let mut out = Vec::with_capacity(attdefs.len());
    for a in attdefs.into_iter().filter(|a| !a.flags.constant) {
        let given = values
            .iter()
            .find(|(tag, _)| a.tag.eq_ignore_ascii_case(tag.trim()))
            .map(|(_, v)| v.clone());
        if given.as_deref().is_some_and(|v| v.contains(['\n', '\r'])) && !a.is_multiline {
            return Err(invalid(format!("Attribute {} takes one line", a.tag)));
        }
        let typed = given.is_some();
        let mut e = EntityType::AttributeEntity(AttributeEntity::from_definition(a, given));
        e.apply_transform(xf);
        let EntityType::AttributeEntity(mut attribute) = e else {
            unreachable!("still an attribute");
        };
        if typed {
            mark_logical(&mut attribute.common);
        }
        out.push(attribute);
    }
    Ok(out)
}

/// Takes off `c` the handles that point into the drawing it was copied from: its linetype's
/// (the writers find it by name), its colour book's, visual styles', material's and plot style's
/// (both by layer then).
fn drop_source_handles(c: &mut opencadcodec::entities::EntityCommon) {
    c.linetype_handle = None;
    c.color_book_handle = None;
    c.full_visual_style_handle = None;
    c.face_visual_style_handle = None;
    c.edge_visual_style_handle = None;
    if c.material_handle.take().is_some() {
        c.material_flags = 0;
    }
    if c.plotstyle_handle.take().is_some() {
        c.plotstyle_flags = 0;
    }
}

/// Copies block definitions, and the layers, linetypes, text and dimension styles their entities
/// use, from one drawing into another. A block or table entry the target has already (by name)
/// is kept as it is; an anonymous block gets a free name of its own.
pub(crate) struct Importer<'a> {
    src: &'a CadDocument,
    dst: &'a mut CadDocument,
    /// Blocks copied, source name to the name in `dst`.
    blocks: HashMap<String, String>,
    /// Images and PDF underlays left out (their files are not copied along).
    skipped: usize,
    /// How many blocks deep the copy is now (see `MAX_IMPORT_DEPTH`).
    depth: usize,
}

/// Most blocks nested inside one another that an import copies, as flatten draws them.
const MAX_IMPORT_DEPTH: usize = 32;

impl<'a> Importer<'a> {
    pub(crate) fn new(src: &'a CadDocument, dst: &'a mut CadDocument) -> Self {
        Importer {
            src,
            dst,
            blocks: HashMap::new(),
            skipped: 0,
            depth: 0,
        }
    }

    fn layer(&mut self, name: &str) {
        if name.is_empty() || self.dst.layers.contains(name) {
            return;
        }
        let Some(layer) = self.src.layers.get(name) else {
            return;
        };
        let mut layer = layer.clone();
        self.linetype(&layer.line_type);
        layer.handle = self.dst.allocate_handle();
        layer.material = Handle::NULL;
        layer.plotstyle_handle = Handle::NULL;
        layer.xref_block_record_handle = Handle::NULL;
        let _ = self.dst.layers.add(layer);
    }

    fn linetype(&mut self, name: &str) {
        let special = ["", "BYLAYER", "BYBLOCK"].contains(&name.to_uppercase().as_str());
        if special || self.dst.line_types.contains(name) {
            return;
        }
        let Some(lt) = self.src.line_types.get(name) else {
            return;
        };
        let mut lt = lt.clone();
        lt.handle = self.dst.allocate_handle();
        lt.xref_block_record_handle = Handle::NULL;
        // A text or shape segment's style comes along; one the source lacks is dropped.
        for complex in lt.elements.iter_mut().filter_map(|e| e.complex.as_mut()) {
            let name = self
                .src
                .text_styles
                .iter()
                .find(|s| s.handle == complex.style_handle)
                .map(|s| s.name.clone());
            if let Some(name) = &name {
                self.text_style(name);
            }
            complex.style_handle = name
                .and_then(|n| self.dst.text_styles.get(&n).map(|s| s.handle))
                .unwrap_or(Handle::NULL);
        }
        let _ = self.dst.line_types.add(lt);
    }

    fn text_style(&mut self, name: &str) {
        if name.is_empty() || self.dst.text_styles.contains(name) {
            return;
        }
        let Some(style) = self.src.text_styles.get(name) else {
            return;
        };
        let mut style = style.clone();
        style.handle = self.dst.allocate_handle();
        style.xref_block_record_handle = Handle::NULL;
        let _ = self.dst.text_styles.add(style);
    }

    fn dim_style(&mut self, name: &str) {
        if name.is_empty() || self.dst.dim_styles.contains(name) {
            return;
        }
        let Some(style) = self.src.dim_styles.get(name) else {
            return;
        };
        let mut style = style.clone();
        self.text_style(&style.dimtxsty);
        style.handle = self.dst.allocate_handle();
        style.xref_handle = Handle::NULL;
        // Arrowheads by block handle are the source's: the default arrows instead.
        style.dimblk = Handle::NULL;
        style.dimldrblk = Handle::NULL;
        style.dimltex_handle = Handle::NULL;
        style.dimtxsty_handle = self
            .dst
            .text_styles
            .get(&style.dimtxsty)
            .map_or(Handle::NULL, |s| s.handle);
        let _ = self.dst.dim_styles.add(style);
    }

    /// `e` ready to go into `dst`: what it uses copied first, the blocks it draws renamed when
    /// they had to be. `None` for an image or underlay.
    pub(crate) fn entity(&mut self, e: &EntityType) -> Result<Option<EntityType>, CoreError> {
        let mut e = e.clone();
        let c = e.common();
        let (layer, linetype) = (c.layer.clone(), c.linetype.clone());
        self.layer(&layer);
        self.linetype(&linetype);
        drop_source_handles(e.common_mut());
        match &mut e {
            EntityType::RasterImage(_) | EntityType::Underlay(_) => {
                self.skipped += 1;
                return Ok(None);
            }
            EntityType::Text(t) => self.text_style(&t.style.clone()),
            EntityType::MText(t) => self.text_style(&t.style.clone()),
            EntityType::AttributeDefinition(a) => self.text_style(&a.text_style.clone()),
            EntityType::Insert(i) => {
                for a in i.attributes.clone() {
                    self.layer(&a.common.layer);
                    self.text_style(&a.text_style);
                }
                i.view_rep_handle = None;
                for a in &mut i.attributes {
                    a.attdef_handle = Handle::NULL;
                    drop_source_handles(&mut a.common);
                }
                i.block_name = self.block(&i.block_name.clone())?;
            }
            EntityType::Dimension(d) => {
                let style = d.base().style_name.clone();
                self.dim_style(&style);
                let block = d.base().block_name.clone();
                if !block.is_empty() && self.src.block_records.contains(&block) {
                    d.base_mut().block_name = self.block(&block)?;
                }
            }
            EntityType::Table(t) => {
                if let Some(block) = qiyas_geom::table_block(self.src, t).map(|b| b.name.clone()) {
                    let name = self.block(&block)?;
                    t.block_record_handle = self.dst.block_records.get(&name).map(|b| b.handle);
                    t.block_name = name;
                }
            }
            _ => {}
        }
        Ok(Some(e))
    }

    /// Copies the source entity `e` (with what it uses) into the `dst` block `owner`; its new
    /// handle, or `None` for an image or underlay (left out).
    pub(crate) fn copy(&mut self, e: &EntityType, owner: Handle) -> Result<Option<u64>, CoreError> {
        match self.entity(e)? {
            Some(e) => add_fresh_to(self.dst, e, Some(owner)).map(Some),
            None => Ok(None),
        }
    }

    /// The name in `dst` of the source block `name`, copied over (with what it uses) when `dst`
    /// has no block of that name; an anonymous one always gets a fresh name.
    fn block(&mut self, name: &str) -> Result<String, CoreError> {
        if let Some(done) = self.blocks.get(&name.to_uppercase()) {
            return Ok(done.clone());
        }
        let Some(source) = self.src.block_records.get(name) else {
            return Ok(name.to_string());
        };
        let anonymous = source.name.starts_with('*');
        if !anonymous && self.dst.block_records.contains(name) {
            self.blocks.insert(name.to_uppercase(), name.to_string());
            return Ok(name.to_string());
        }
        let fresh = if anonymous {
            let prefix: String = source.name.chars().take(2).collect();
            (1..)
                .map(|n| format!("{prefix}{n}"))
                .find(|n| !self.dst.block_records.contains(n))
                .expect("a free name")
        } else {
            source.name.clone()
        };
        self.blocks.insert(name.to_uppercase(), fresh.clone());
        if self.depth >= MAX_IMPORT_DEPTH {
            return Err(invalid(format!(
                "Blocks nest more than {MAX_IMPORT_DEPTH} deep there: they can't be copied"
            )));
        }
        self.depth += 1;
        let handle = self.new_record(&fresh, source)?;
        let filled = self.fill(&source.name, handle, Vector3::ZERO);
        self.depth -= 1;
        filled?;
        Ok(fresh)
    }

    /// Adds an empty block record `name` to `dst` like `source` (flags, base point, description).
    pub(crate) fn new_record(
        &mut self,
        name: &str,
        source: &BlockRecord,
    ) -> Result<Handle, CoreError> {
        let mut record = BlockRecord::new(name);
        record.handle = self.dst.allocate_handle();
        record.block_entity_handle = self.dst.allocate_handle();
        record.block_end_handle = self.dst.allocate_handle();
        record.flags.anonymous = source.flags.anonymous;
        record.flags.has_attributes = source.flags.has_attributes;
        record.base_point = source.base_point;
        record.description = source.description.clone();
        record.units = source.units;
        let handle = record.handle;
        self.dst.block_records.add(record).map_err(invalid)?;
        Ok(handle)
    }

    /// Copies the entities of the source block `name` into the `dst` block `owner`, moved by
    /// `shift`.
    pub(crate) fn fill(
        &mut self,
        name: &str,
        owner: Handle,
        shift: Vector3,
    ) -> Result<(), CoreError> {
        let src = self.src;
        let moved = Transform::from_translation(shift);
        for e in src.entities_in_block(name) {
            if let Some(mut e) = self.entity(e)? {
                if shift != Vector3::ZERO {
                    place(&mut e, &Placing::plain(moved));
                }
                add_fresh_to(self.dst, e, Some(owner))?;
            }
        }
        Ok(())
    }
}

/// Fades every colour of `batch` drawn by an entity at the top of the sheet in `faded` half way
/// to grey, so what is edited in place stands out.
pub(crate) fn fade(batch: &mut Batch, faded: &HashSet<u64>) {
    let mix = |c: u32| {
        let [_, r, g, b] = c.to_be_bytes();
        let m = |v: u8| ((u16::from(v) + 0x80) / 2) as u8;
        u32::from_be_bytes([0, m(r), m(g), m(b)])
    };
    let over = |ranges: &[Range], colors: &mut [u32]| {
        for r in ranges.iter().filter(|r| faded.contains(&r.top)) {
            let span = r.first as usize..(r.first + r.count) as usize;
            for c in colors.get_mut(span).into_iter().flatten() {
                if *c & FILL_BACKGROUND == 0 {
                    *c = mix(*c);
                }
            }
        }
    };
    over(&batch.ranges, &mut batch.colors);
    over(&batch.fill_ranges, &mut batch.fill_colors);
    for r in batch.text_ranges.iter().filter(|r| faded.contains(&r.top)) {
        let span = r.first as usize..(r.first + r.count) as usize;
        for q in batch.quads.get_mut(span).into_iter().flatten() {
            q.color = mix(q.color);
        }
    }
    for t in batch.texts.iter_mut().filter(|t| faded.contains(&t.top)) {
        t.color = mix(t.color);
    }
}

impl Session {
    /// The drawing's named blocks (not sheets, xrefs or anonymous blocks) with their attribute
    /// definitions, sorted by name.
    pub fn blocks(&self) -> Vec<BlockInfo> {
        block_infos(&self.drawing.doc)
    }

    /// The lines of the block `name` as a sheet of their own, in block coordinates, for a
    /// thumbnail.
    pub fn block_batch(&self, name: &str) -> Result<Batch, CoreError> {
        let record = named_block(&self.drawing.doc, name)?;
        Ok(qiyas_render::build_batch(
            &self.drawing.doc,
            &record.name,
            &qiyas_render::Visibility::default(),
            self.batch_tol(),
        ))
    }

    /// The name of the block being edited in place, if one is.
    pub fn block_edit_name(&self) -> Option<String> {
        self.block_edit.as_ref().map(|b| b.name.clone())
    }

    /// Refuses what can't be done while a block is edited in place: making or writing a block,
    /// another edit in place, redefining the block (or a block holding it), and inserting the
    /// block into itself (or into something it holds).
    /// Everything else goes on as usual.
    pub(crate) fn check_block_edit(&self, cmd: &Command) -> Result<(), CoreError> {
        let Some(edit) = &self.block_edit else {
            return match cmd {
                Command::SaveBlockEdits | Command::DiscardBlockEdits => {
                    Err(invalid("No block is being edited in place"))
                }
                _ => Ok(()),
            };
        };
        match cmd {
            Command::RedefineBlock { name, .. }
                if !(name.trim().eq_ignore_ascii_case(&edit.name)
                    || uses(&self.drawing.doc, name.trim(), &edit.name, 0)) =>
            {
                Ok(())
            }
            Command::EditBlockInPlace { .. }
            | Command::CreateBlock { .. }
            | Command::WriteBlock { .. }
            | Command::RedefineBlock { .. } => Err(invalid(format!(
                "Block {} is being edited in place: save or discard the edit first (REFCLOSE)",
                edit.name
            ))),
            Command::InsertBlock { name, .. }
                if name.trim().eq_ignore_ascii_case(&edit.name)
                    || uses(&self.drawing.doc, name.trim(), &edit.name, 0) =>
            {
                Err(invalid(format!(
                    "Block {} can't go inside itself",
                    edit.name
                )))
            }
            _ => Ok(()),
        }
    }

    /// `Command::CreateBlock`: the entities `targets` leave the sheet for a new block `name`,
    /// their coordinates relative to `base`; with `convert_to_insert` a reference to it takes
    /// their place at `base`. Rows measuring them keep their values without the link.
    pub(crate) fn create_block(
        &mut self,
        name: &str,
        base: [f64; 2],
        targets: &[Target],
        convert_to_insert: bool,
    ) -> Result<Outcome, CoreError> {
        finite(&base)?;
        let name = check_new_name(&self.drawing.doc, name)?;
        let handles = self.sheet_targets(targets)?;
        for &h in &handles {
            let e = self.entity(h);
            let refuse = |why: &str| {
                Err(invalid(format!(
                    "{} {:X} {why}",
                    crate::session::variant_name(e),
                    h.value()
                )))
            };
            match e {
                EntityType::Insert(i) if is_xref_block(&self.drawing.doc, &i.block_name) => {
                    return refuse("is an xref: it can't go into a block");
                }
                EntityType::RasterImage(_) | EntityType::Underlay(_) => {
                    return refuse("stays on the sheet: pictures are not drawn inside blocks");
                }
                EntityType::Viewport(_) | EntityType::Unknown(_) | EntityType::Extended(_) => {
                    return refuse("can't go into a block");
                }
                _ => {}
            }
        }
        let doc = &mut self.drawing.doc;
        let mut record = BlockRecord::new(name.clone());
        record.handle = doc.allocate_handle();
        record.block_entity_handle = doc.allocate_handle();
        record.block_end_handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).map_err(invalid)?;
        let shift = [-base[0], -base[1]];
        let mut has_attributes = false;
        for &h in &handles {
            let mut e = self.entity(h).clone();
            has_attributes |= matches!(e, EntityType::AttributeDefinition(_));
            if let Some(block) = crate::modify::place_keeping_blocks(&mut e, &Placing::shift(shift))
            {
                self.shift_block(&block, shift);
            }
            self.drawing.doc.remove_entity_arc(h);
            self.add_fresh(e, Some(owner))?;
        }
        if let Some(r) = self.drawing.doc.block_records.get_mut(&name) {
            r.flags.has_attributes = has_attributes;
        }
        let unlinked = self
            .takeoff
            .items
            .iter()
            .filter(|m| {
                handles
                    .iter()
                    .any(|&h| crate::modify::measures(m, h.value()))
            })
            .count();
        self.follow_takeoff(&[], &handles);
        let added = if convert_to_insert {
            vec![self.insert_block(&name, base, 1.0, 0.0, &BTreeMap::new(), None)?]
        } else {
            Vec::new()
        };
        Ok(Outcome::Edited {
            added,
            removed: handles.iter().map(|h| h.value()).collect(),
            left_alone: 0,
            unlinked,
            joined_rows: 0,
            constraints_removed: 0,
        })
    }

    /// `Command::InsertBlock`: a reference to the block `name` at `at`, `scale` times its size,
    /// turned `rotation` degrees, on the current layer and state, with its attributes. With
    /// `from_path`, a block the drawing lacks is first copied from the drawing there.
    pub(crate) fn insert_block(
        &mut self,
        name: &str,
        at: [f64; 2],
        scale: f64,
        rotation: f64,
        values: &BTreeMap<String, String>,
        from_path: Option<&str>,
    ) -> Result<u64, CoreError> {
        finite(&[at[0], at[1], scale, rotation])?;
        if scale <= 0.0 {
            return Err(invalid("The scale must be more than 0"));
        }
        if !values.is_empty() {
            // Typed values are marked as in reading order.
            self.register_text_app();
        }
        let name = name.trim();
        // The drawing a block came from: its INSUNITS stand in for a block without units.
        let mut source_units = None;
        if let (Some(path), false) = (from_path, self.drawing.doc.block_records.contains(name)) {
            let other = Drawing::open(Path::new(path))?;
            named_block(&other.doc, name)?;
            Importer::new(&other.doc, &mut self.drawing.doc).block(name)?;
            source_units = Some(other.doc.header.insertion_units);
        }
        let doc = &self.drawing.doc;
        let record = named_block(doc, name)?;
        // As AutoCAD's INSERT: a block drawn in other units comes in at this drawing's size (a
        // 900 mm door from a millimetre library is 0.9 m in a metre plan, not 900 m).
        let block_units = match record.units {
            0 => source_units.unwrap_or(0),
            units => units,
        };
        let scale = scale * insert_unit_factor(block_units, doc.header.insertion_units);
        let mut insert = Insert::new(record.name.clone(), v3(at))
            .with_uniform_scale(scale)
            .with_rotation(rotation.to_radians());
        let xf = forward(record, &insert);
        insert.attributes = attributes_for(doc, record, &xf, values)?;

        let doc = &mut self.drawing.doc;
        let h = doc.allocate_handle();
        insert.common.handle = h;
        for a in &mut insert.attributes {
            a.common.handle = doc.allocate_handle();
            a.common.owner_handle = h;
        }
        Ok(self.add_styled(vec![EntityType::Insert(insert)])?[0])
    }

    /// Writes the block `name` to `path` as a drawing of its own (DWG): its entities in model
    /// space with its base point at the origin, with the layers, linetypes, text and dimension
    /// styles and blocks they use. Images and PDF underlays are left out. Not an undo step.
    pub fn write_block(&self, name: &str, path: &Path) -> Result<(), CoreError> {
        let is_dwg = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
        if !is_dwg {
            return Err(invalid("Write the block as a .dwg file"));
        }
        if same_file(path, &self.drawing.path) {
            return Err(invalid(
                "A block can't be written over the drawing it comes from",
            ));
        }
        let src = &self.drawing.doc;
        let record = named_block(src, name)?;
        let mut doc = CadDocument::new();
        doc.header.insertion_units = src.header.insertion_units;
        let model = doc
            .block_records
            .get("*Model_Space")
            .map(|b| b.handle)
            .ok_or_else(|| invalid("A new drawing has no model space"))?;
        let b = record.base_point;
        Importer::new(src, &mut doc).fill(&record.name, model, Vector3::new(-b.x, -b.y, -b.z))?;
        let bytes =
            DwgWriter::write_to_vec(&doc).map_err(|e| CoreError::Unwritable(e.to_string()))?;
        // Written over only through the Save dialog, which asked: keep the old file as .bak.
        crate::project::write_with_backup(path, &bytes)
            .map_err(|e| CoreError::Unwritable(e.to_string()))
    }

    /// `path` as a real path inside the drawing's folder or one of its subfolders (relative
    /// paths are taken from that folder), or why not: the rule for files the command bus writes.
    fn inside_drawing_folder(&self, path: &str) -> Result<PathBuf, CoreError> {
        let parent = self
            .drawing
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let folder = parent.canonicalize()?;
        let full = folder.join(path);
        let outside = || {
            invalid("Only files in the drawing's folder can be written this way: choose the file in the Save dialog")
        };
        let dir = full.parent().ok_or_else(outside)?;
        let real = dir.canonicalize().map_err(|_| outside())?;
        if !real.starts_with(&folder) {
            return Err(outside());
        }
        Ok(real.join(full.file_name().ok_or_else(outside)?))
    }

    /// `Command::WriteBlock`: `write_block` to a new file in the drawing's folder (a file there
    /// already is only written over through the Save dialog, which asks).
    pub(crate) fn write_block_in_folder(&self, name: &str, path: &str) -> Result<(), CoreError> {
        named_block(&self.drawing.doc, name)?;
        let path = self.inside_drawing_folder(path)?;
        if path.exists() {
            return Err(invalid(format!(
                "{} exists already: choose it in the Save dialog to write over it",
                path.display()
            )));
        }
        self.write_block(name, &path)
    }

    /// `Command::EditBlockInPlace`: starts editing the block of the reference `target` in place
    /// (see the module documentation). Returns the temporary copies.
    pub(crate) fn edit_block_in_place(&mut self, target: &Target) -> Result<Vec<u64>, CoreError> {
        let h = self.sheet_targets(std::slice::from_ref(target))?[0];
        let doc = &self.drawing.doc;
        let EntityType::Insert(insert) = self.entity(h) else {
            return Err(invalid("Pick a block reference to edit its block"));
        };
        let record = named_block(doc, &insert.block_name)?;
        if insert.column_count > 1 || insert.row_count > 1 {
            return Err(invalid(
                "An arrayed block reference can't be edited in place",
            ));
        }
        if insert.x_scale() == 0.0 || (insert.x_scale() - insert.y_scale()).abs() > 1e-10 {
            return Err(invalid(
                "Edit in place takes a block reference scaled the same both ways",
            ));
        }
        let inside: Vec<&EntityType> = doc.entities_in_block(&record.name).collect();
        if let Some(e) = inside
            .iter()
            .find(|e| matches!(e, EntityType::Dimension(_) | EntityType::Table(_)))
        {
            return Err(invalid(format!(
                "Block {} holds a {}: it can't be edited in place yet",
                record.name,
                crate::session::variant_name(e)
            )));
        }
        let (forward, inverse) = (forward(record, insert), inverse(record, insert));
        let name = record.name.clone();
        let parts: Vec<(Handle, EntityType)> = inside
            .iter()
            .map(|e| (e.common().handle, (*e).clone()))
            .collect();
        let outside: HashSet<u64> = self
            .drawing
            .entities_in(&self.sheet)
            .map(|e| e.common().handle.value())
            .collect();
        // Not recorded: the copies are never an undo step of their own.
        let owner = self.sheet_owner();
        let mut temps = HashMap::with_capacity(parts.len());
        let mut handles = Vec::with_capacity(parts.len());
        for (original, mut e) in parts {
            place(&mut e, &Placing::plain(forward));
            let t = self.add_fresh(e, owner)?;
            let made = self
                .drawing
                .doc
                .get_entity_arc(Handle::new(t))
                .expect("just added");
            temps.insert(t, (original, made));
            handles.push(t);
        }
        if let Some(e) = self.drawing.doc.get_entity_mut(h) {
            e.common_mut().invisible = true;
        }
        self.block_edit = Some(BlockEdit {
            name,
            insert: h,
            sheet: self.sheet.clone(),
            temps,
            outside,
            inverse,
            was_clean: !self.is_dirty(),
        });
        self.history.set_floor(self.history.undo_len());
        self.drawing_changed();
        Ok(handles)
    }

    /// `Command::SaveBlockEdits` (`save`) and `DiscardBlockEdits`: ends the edit in place. Saving
    /// makes what is on the edit's sheet beyond what was there before (the copies left, and
    /// anything drawn there meanwhile) the block's contents, taken back through the reference: a
    /// changed copy replaces its block entity under that entity's handle, an unchanged one leaves
    /// it as it is, a block entity whose copy went is removed, anything new is added. Discarding
    /// keeps the block as it is. Either way those entities leave the sheet, the reference shows
    /// again, and nothing else done while editing is put back. Not an undo step, and none before
    /// it can be undone after it (an earlier step would put back a take-off or a block that no
    /// longer match): saving clears the whole history, and so does discarding unless every step
    /// made while editing was undone (then only those steps' redo goes). Take-off rows on the
    /// entities that left lose their link; rows on the references are measured again. A failed
    /// save leaves the edit open, its copies as they were.
    pub(crate) fn close_block_edit(&mut self, save: bool) -> Result<Outcome, CoreError> {
        let edit = self
            .block_edit
            .take()
            .ok_or_else(|| invalid("No block is being edited in place"))?;
        let working: Vec<Handle> = self
            .drawing
            .entities_in(&edit.sheet)
            .map(|e| e.common().handle)
            .filter(|h| !edit.outside.contains(&h.value()))
            .collect();
        let mut unlinked = self
            .takeoff
            .items
            .iter()
            .filter(|m| {
                working
                    .iter()
                    .any(|&h| crate::modify::measures(m, h.value()))
            })
            .count();
        if save {
            // All or nothing: a failure puts back what it wrote and leaves the edit open.
            let tables = TablesSnapshot::take(&self.drawing.doc);
            let recorder = self.drawing.doc.begin_entity_change_recording();
            let done = self.write_back(&edit, &working);
            self.drawing.doc.end_entity_change_recording();
            if let Err(e) = done {
                let doc = &self.drawing.doc;
                DocChange::from_before_images(
                    doc,
                    recorder.take_before_images(),
                    Some(tables),
                    None,
                )
                .swap(
                    &mut self.drawing.doc,
                    &mut self.takeoff,
                    &mut crate::undo::ViewRefs {
                        current: &mut self.current,
                        hidden: &mut self.visibility.hidden_layers,
                        state_hidden: &mut self.state_hidden,
                    },
                );
                self.block_edit = Some(edit);
                return Err(e);
            }
        }
        let doc = &mut self.drawing.doc;
        for &h in &working {
            doc.remove_entity_arc(h);
        }
        if let Some(sheet) = doc.block_records.get_mut(&edit.sheet) {
            let gone: HashSet<Handle> = working.iter().copied().collect();
            sheet.entity_handles.retain(|h| !gone.contains(h));
        }
        if let Some(e) = doc.get_entity_mut(edit.insert) {
            e.common_mut().invisible = false;
        }
        self.follow_takeoff(&[], &working);
        if save {
            let references = self.references_to(&edit.name);
            unlinked += self.follow_takeoff(&references, &[]);
        }
        let quiet = self.history.edit_was_quiet();
        let dropped = self.history.close_edit(!save && quiet);
        self.drawing_changed();
        // A discarded edit that did nothing leaves the drawing as clean as it was.
        if !save && quiet && edit.was_clean {
            self.mark_saved();
        }
        Ok(Outcome::BlockEditClosed {
            name: edit.name,
            saved: save,
            unlinked,
            dropped,
        })
    }

    /// The block `edit.name` holds the entities `working` (on the sheet, in world coordinates)
    /// taken back through the reference; see `close_block_edit`. Not recorded.
    fn write_back(&mut self, edit: &BlockEdit, working: &[Handle]) -> Result<(), CoreError> {
        let doc = &mut self.drawing.doc;
        let record = doc
            .block_records
            .get(&edit.name)
            .ok_or_else(|| invalid(format!("Block {} is gone", edit.name)))?;
        let (owner, originals) = (record.handle, record.entity_handles.clone());
        let back_xf = Placing::plain(edit.inverse);
        let mut untouched = HashSet::new();
        let mut back: HashMap<Handle, EntityType> = HashMap::new();
        let mut fresh = Vec::new();
        for &h in working {
            let Some(now) = doc.get_entity(h) else {
                continue;
            };
            let copy_of = edit.temps.get(&h.value());
            if let Some((original, made)) = copy_of {
                if now == made.as_ref() {
                    untouched.insert(*original);
                    continue;
                }
            }
            let mut e = now.clone();
            // A dimension's block is drawn where the dimension is: it goes back too.
            if let Some(block) = crate::modify::place_keeping_blocks(&mut e, &back_xf) {
                let inside = doc
                    .block_records
                    .get(&block)
                    .map(|b| b.entity_handles.clone())
                    .unwrap_or_default();
                for part in inside {
                    if let Some(p) = doc.get_entity_mut(part) {
                        place(p, &back_xf);
                    }
                }
            }
            match copy_of {
                Some((original, _)) => {
                    back.insert(*original, e);
                }
                None => fresh.push(e),
            }
        }
        for h in originals {
            if doc.get_entity(h).is_none() || untouched.contains(&h) {
                continue;
            }
            match back.remove(&h) {
                Some(mut e) => {
                    let c = e.common_mut();
                    c.handle = h;
                    c.owner_handle = owner;
                    *doc.get_entity_mut(h).expect("checked above") = e;
                }
                None => {
                    doc.remove_entity_arc(h);
                }
            }
        }
        for e in fresh {
            add_fresh_to(doc, e, Some(owner))?;
        }
        let has_attributes = doc
            .entities_in_block(&edit.name)
            .any(|e| matches!(e, EntityType::AttributeDefinition(_)));
        // Not recorded, so the list keeps only what is there.
        let live: Vec<Handle> = doc
            .block_records
            .get(&edit.name)
            .map(|r| r.entity_handles.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|&h| doc.get_entity(h).is_some())
            .collect();
        if let Some(r) = doc.block_records.get_mut(&edit.name) {
            r.flags.has_attributes = has_attributes;
            r.entity_handles = live;
        }
        Ok(())
    }

    /// The references that draw the block `name`, or something holding it.
    fn references_to(&self, name: &str) -> Vec<Handle> {
        let doc = &self.drawing.doc;
        doc.entities()
            .filter_map(|e| match e {
                EntityType::Insert(i)
                    if i.block_name.eq_ignore_ascii_case(name)
                        || uses(doc, &i.block_name, name, 0) =>
                {
                    Some(e.common().handle)
                }
                _ => None,
            })
            .collect()
    }

    /// Take-off rows measuring a reference that draws the block `name` (or something inside
    /// one) are measured again.
    fn follow_block(&mut self, name: &str) {
        let references = self.references_to(name);
        self.follow_takeoff(&references, &[]);
    }

    /// `Command::AttDef`: an attribute definition `tag` (upper-cased; no spaces) on the current
    /// layer at `at`, `height` tall, in the current text style.
    pub(crate) fn add_attdef(
        &mut self,
        tag: &str,
        prompt: &str,
        default: &str,
        at: [f64; 2],
        height: f64,
    ) -> Result<u64, CoreError> {
        finite(&[at[0], at[1], height])?;
        let height = if height == 0.0 {
            self.default_text_height()
        } else {
            height
        };
        if height <= 0.0 {
            return Err(invalid("The height must be more than 0"));
        }
        let tag = tag.trim().to_uppercase();
        if tag.is_empty() || tag.chars().any(char::is_whitespace) {
            return Err(invalid("An attribute tag is one word"));
        }
        if [prompt, default].iter().any(|s| s.contains(['\n', '\r'])) {
            return Err(invalid("A prompt and a default take one line each"));
        }
        let mut a = AttributeDefinition::new(tag, prompt.to_string(), default.to_string());
        a.set_position(v3(at));
        a.height = height;
        a.text_style = self.text_style();
        self.register_text_app();
        mark_logical(&mut a.common);
        Ok(self.add_styled(vec![EntityType::AttributeDefinition(a)])?[0])
    }

    /// `Command::EditAttributes`: the reference `target`'s attributes take `values` by tag (any
    /// case); every tag must be one it has.
    pub(crate) fn edit_attributes(
        &mut self,
        target: &Target,
        values: &BTreeMap<String, String>,
    ) -> Result<(), CoreError> {
        if values.is_empty() {
            return Err(invalid("Give a value for an attribute"));
        }
        let h = self.sheet_targets(std::slice::from_ref(target))?[0];
        let EntityType::Insert(insert) = self.entity(h) else {
            return Err(invalid("Only a block reference has attributes"));
        };
        let tags: Vec<String> = insert.attributes.iter().map(|a| a.tag.clone()).collect();
        if let Some(tag) = values
            .keys()
            .find(|t| !tags.iter().any(|a| a.eq_ignore_ascii_case(t.trim())))
        {
            return Err(invalid(format!(
                "This block reference has no attribute {tag}"
            )));
        }
        for (tag, value) in values {
            self.edit_text(target, value, Some(tag))?;
        }
        Ok(())
    }

    /// `Command::EditAttDef`: the attribute definition `tag` of the block `block` takes `prompt`
    /// and `default`.
    pub(crate) fn edit_attdef(
        &mut self,
        block: &str,
        tag: &str,
        prompt: &str,
        default: &str,
    ) -> Result<(), CoreError> {
        if [prompt, default].iter().any(|s| s.contains(['\n', '\r'])) {
            return Err(invalid("A prompt and a default take one line each"));
        }
        let record = named_block(&self.drawing.doc, block)?;
        let found = record.entity_handles.iter().copied().find(|&h| {
            matches!(self.drawing.doc.get_entity(h),
                Some(EntityType::AttributeDefinition(a)) if a.tag.eq_ignore_ascii_case(tag.trim()))
        });
        let h = found.ok_or_else(|| invalid(format!("Block {block} has no attribute {tag}")))?;
        self.register_text_app();
        if let Some(EntityType::AttributeDefinition(a)) = self.drawing.doc.get_entity_mut(h) {
            a.prompt = prompt.to_string();
            a.default_value = default.to_string();
            mark_logical(&mut a.common);
        }
        Ok(())
    }

    /// `Command::SyncAttributes` (AutoCAD's ATTSYNC): every reference to `block` on any sheet gets
    /// the block's attributes as its definitions place and style them, keeping the value of each
    /// tag it had; tags the block no longer has go. A locked reference (`Session::is_locked`) is
    /// left as it is. How many references changed, and how many locked ones were left.
    pub(crate) fn sync_attributes(&mut self, block: &str) -> Result<(usize, usize), CoreError> {
        let doc = &self.drawing.doc;
        let record = named_block(doc, block)?;
        let references: Vec<Handle> = doc
            .entities()
            .filter(|e| {
                matches!(e, EntityType::Insert(i) if i.block_name.eq_ignore_ascii_case(&record.name))
            })
            .map(|e| e.common().handle)
            .collect();
        let mut changed = 0;
        let mut skipped = 0;
        for h in references {
            let doc = &self.drawing.doc;
            let Some(EntityType::Insert(insert)) = doc.get_entity(h) else {
                continue;
            };
            let record = named_block(doc, block)?;
            let mut values = BTreeMap::new();
            for a in &insert.attributes {
                values.insert(a.tag.to_uppercase(), a.value.clone());
            }
            let tags: Vec<String> = doc
                .entities_in_block(&record.name)
                .filter_map(|e| match e {
                    EntityType::AttributeDefinition(a) => Some(a.tag.to_uppercase()),
                    _ => None,
                })
                .collect();
            values.retain(|tag, _| tags.contains(tag));
            let mut attributes = attributes_for(doc, record, &forward(record, insert), &values)?;
            // Keep each surviving attribute's own handle, layer and colour.
            for a in &mut attributes {
                match insert
                    .attributes
                    .iter()
                    .find(|o| o.tag.eq_ignore_ascii_case(&a.tag))
                {
                    Some(old) => a.common = old.common.clone(),
                    None => {
                        a.common.handle = Handle::NULL;
                        a.common.owner_handle = h;
                    }
                }
            }
            if attributes == insert.attributes {
                continue;
            }
            if self.is_locked(h) {
                skipped += 1;
                continue;
            }
            let doc = &mut self.drawing.doc;
            for a in &mut attributes {
                if a.common.handle.is_null() {
                    a.common.handle = doc.allocate_handle();
                }
            }
            if let Some(EntityType::Insert(i)) = doc.get_entity_mut(h) {
                i.attributes = attributes;
                i.common.graphic_data = None;
            }
            changed += 1;
        }
        Ok((changed, skipped))
    }

    /// `Command::RedefineBlock`: the block `name` holds what the block of that name holds in the
    /// drawing at `from_path` (opened read-only), with what it uses. Every reference changes.
    pub(crate) fn redefine_block(&mut self, name: &str, from_path: &str) -> Result<(), CoreError> {
        let own = named_block(&self.drawing.doc, name)?;
        let (owner, own_name, originals) =
            (own.handle, own.name.clone(), own.entity_handles.clone());
        let other = Drawing::open(Path::new(from_path))?;
        let source = named_block(&other.doc, name).map_err(|_| {
            invalid(format!(
                "{} has no block {}",
                Path::new(from_path)
                    .file_name()
                    .map_or_else(|| from_path.into(), |f| f.to_string_lossy()),
                name.trim()
            ))
        })?;
        let (base, flags) = (source.base_point, source.flags.has_attributes);
        let source_name = source.name.clone();
        for h in originals {
            self.drawing.doc.remove_entity_arc(h);
        }
        let doc = &mut self.drawing.doc;
        if let Some(r) = doc.block_records.get_mut(&own_name) {
            r.entity_handles.clear();
            r.base_point = base;
            r.flags.has_attributes = flags;
        }
        let mut importer = Importer::new(&other.doc, doc);
        importer
            .blocks
            .insert(source_name.to_uppercase(), own_name.clone());
        importer.fill(&source_name, owner, Vector3::ZERO)?;
        self.follow_block(&own_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use opencadcodec::tables::{
        LineType, LineTypeComplexContent, LineTypeComplexData, LineTypeElement,
    };
    use opencadcodec::{Line, TextStyle};

    use super::*;

    #[test]
    fn imported_entities_carry_no_handles_of_the_drawing_they_came_from() {
        let foreign = Some(Handle::new(0xBEEF));
        let mut src = CadDocument::new();
        let mut style = TextStyle::new("SHAPES");
        style.handle = src.allocate_handle();
        let style_handle = style.handle;
        src.text_styles.add(style).unwrap();
        let mut dashed = LineType::new("TRACKS");
        dashed.handle = src.allocate_handle();
        let mut element = LineTypeElement::dash(1.0);
        element.complex = Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text { text: "X".into() },
            style_handle,
            ..Default::default()
        });
        let mut lost = LineTypeElement::dash(1.0);
        lost.complex = Some(LineTypeComplexData {
            content: LineTypeComplexContent::Text { text: "Y".into() },
            style_handle: Handle::new(0xBEEF),
            ..Default::default()
        });
        dashed.elements = vec![element, lost];
        src.line_types.add(dashed).unwrap();

        let mut line = EntityType::Line(Line::from_coords(0.0, 0.0, 0.0, 1.0, 0.0, 0.0));
        let c = line.common_mut();
        c.linetype = "TRACKS".into();
        c.linetype_handle = foreign;
        c.color_book_handle = foreign;
        c.full_visual_style_handle = foreign;
        c.face_visual_style_handle = foreign;
        c.edge_visual_style_handle = foreign;
        c.material_flags = 3;
        c.material_handle = foreign;
        c.plotstyle_flags = 3;
        c.plotstyle_handle = foreign;
        let mut insert = Insert::new("NONE", Vector3::ZERO);
        insert.view_rep_handle = foreign;
        let mut a = AttributeEntity::new("TAG".into(), "V".into());
        a.attdef_handle = Handle::new(0xBEEF);
        a.common.linetype_handle = foreign;
        insert.attributes.push(a);

        let mut dst = CadDocument::new();
        let mut importer = Importer::new(&src, &mut dst);
        let line = importer.entity(&line).unwrap().unwrap();
        let insert = importer
            .entity(&EntityType::Insert(insert))
            .unwrap()
            .unwrap();
        let c = line.common();
        assert_eq!(
            (
                c.linetype_handle,
                c.color_book_handle,
                c.full_visual_style_handle,
                c.face_visual_style_handle,
                c.edge_visual_style_handle,
                c.material_handle,
                c.plotstyle_handle,
            ),
            (None, None, None, None, None, None, None)
        );
        assert_eq!((c.material_flags, c.plotstyle_flags), (0, 0), "by layer");
        let EntityType::Insert(i) = insert else {
            panic!("a reference");
        };
        assert_eq!(i.view_rep_handle, None);
        assert!(i.attributes[0].attdef_handle.is_null());
        assert_eq!(i.attributes[0].common.linetype_handle, None);
        // The linetype's text style comes along; a style the source lacks is dropped.
        let lt = dst.line_types.get("TRACKS").unwrap();
        let copied = dst.text_styles.get("SHAPES").unwrap().handle;
        let styles: Vec<Handle> = lt
            .elements
            .iter()
            .map(|e| e.complex.as_ref().unwrap().style_handle)
            .collect();
        assert_eq!(styles, [copied, Handle::NULL]);
    }
}
