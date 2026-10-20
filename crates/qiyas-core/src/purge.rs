//! PURGE (AutoCAD's): what nothing in the drawing uses, listed by a dry run and taken out in one
//! undo step (`Command::Purge`). It grows the save-time purge (`purge_for_save` in `session.rs`:
//! anonymous dimension `*Dn` and table `*Tn` blocks nothing uses any more, group members that are
//! gone) by the same rules, and adds:
//!
//! - layers with no entity on them anywhere (inside blocks too, attributes counted), never 0,
//!   Defpoints, the current layer (Qiyas's or the header's), an xref's or the take-off's
//!   `QIYAS-*` layers every save writes;
//! - named blocks no reference uses; a reference inside another block counts while that block is
//!   kept, so a block used only inside an unused block goes with it. Never a layout, an xref, a
//!   dynamic block's definition, an anonymous block other than an orphan `*D`/`*T`, or a block a
//!   dimension style uses for its arrowheads;
//! - text styles, linetypes and dimension styles nothing names: never Standard, ByLayer, ByBlock,
//!   Continuous, the current ones, shape files, an xref's, or a dimension style's `$n` child while
//!   the style itself stays;
//! - empty groups (every member gone).
//!
//! "Uses" is read wide on purpose: besides the fields that name an entry (an insert's block, a
//! text's style, an entity's layer and linetype, a layer's linetype), every handle and name the
//! drawing's objects, header, dimension styles and linetypes hold, and those of the entities whose
//! references are less plain (multileaders, tables, dimension overrides, proxies), keeps an entry.
//! A name that might be a use keeps the entry; a purge never leaves a dangling reference.

use schemars::JsonSchema;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;

use opencadcodec::objects::ObjectType;
use opencadcodec::tables::{normalize_name, BlockRecord};
use opencadcodec::{CadDocument, EntityType, Handle};
use serde::{Deserialize, Serialize};

use crate::{CoreError, Session};

/// What a purge looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PurgeKind {
    Layers,
    Blocks,
    /// Text styles.
    Styles,
    Linetypes,
    Dimstyles,
    Groups,
}

/// What a purge took out (or, in a dry run, would take out), by kind, each list sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PurgeReport {
    pub dry_run: bool,
    pub layers: Vec<String>,
    pub blocks: Vec<String>,
    /// Text styles.
    pub styles: Vec<String>,
    pub linetypes: Vec<String>,
    pub dimstyles: Vec<String>,
    /// Groups left with no member.
    pub groups: Vec<String>,
    /// Members gone from the drawing that groups which stay still listed.
    pub group_members: usize,
    /// Everything above: the entries listed and the gone members.
    pub total: usize,
    /// Why only layers were looked at, when the drawing holds data Qiyas can't read (objects
    /// or entities of kinds it does not know), whose references to blocks, styles and linetypes
    /// it can't see; `None` when every kind asked for was looked at.
    pub limited_reason: Option<String>,
}

/// Handles and names something mentions, read off its `Debug` text: every `Handle(n)` and every
/// quoted string (names upper-cased as tables compare them).
#[derive(Default)]
pub(crate) struct Mentions {
    handles: HashSet<u64>,
    names: HashSet<String>,
}

impl Mentions {
    fn scan(&mut self, value: &impl Debug) {
        let text = format!("{value:?}");
        let bytes = text.as_bytes();
        // Indexes move by bytes, but only ever stop on ASCII bytes (a quote, `H`, a digit) or
        // on the char boundaries `char_indices` gives, so slicing never splits a character.
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    let (name, end) = debug_string(&text[i + 1..]);
                    i += 1 + end;
                    if name.is_empty() {
                        continue;
                    }
                    // DXF raw pairs keep handles as hex text.
                    if name.len() <= 16 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
                        if let Ok(h) = u64::from_str_radix(&name, 16) {
                            self.handles.insert(h);
                        }
                    }
                    self.names.insert(normalize_name(&name));
                }
                // A char literal (`'"'`, `'\''`, `'Ä'`): skipped whole, so a quote in it
                // starts no string.
                b'\'' => {
                    let rest = &text[i + 1..];
                    let mut chars = rest.char_indices();
                    let skip = match chars.next() {
                        // The escaped character is ASCII (`'`, `n`, `u`...): the closing
                        // quote is the first one after it.
                        Some((_, '\\')) => rest.get(2..).and_then(|r| r.find('\'')).map(|k| k + 3),
                        Some((_, c)) => rest[c.len_utf8()..]
                            .starts_with('\'')
                            .then(|| c.len_utf8() + 1),
                        None => None,
                    };
                    i += 1 + skip.unwrap_or(0);
                }
                b'H' if bytes[i..].starts_with(b"Handle(") => {
                    let digits = bytes[i + 7..]
                        .iter()
                        .take_while(|b| b.is_ascii_digit())
                        .count();
                    if let Ok(h) = text[i + 7..i + 7 + digits].parse::<u64>() {
                        self.handles.insert(h);
                    }
                    i += 7 + digits;
                }
                // Past one whole character.
                _ => i += text[i..].chars().next().map_or(1, char::len_utf8),
            }
        }
    }

    fn has(&self, name: &str, handle: Handle) -> bool {
        self.names.contains(&normalize_name(name))
            || (!handle.is_null() && self.handles.contains(&handle.value()))
    }
}

/// A Debug string's text up to its closing quote, unescaped (`\"`, `\\`, `\n`, `\u{64e}`),
/// and how many bytes it took with the quote.
fn debug_string(rest: &str) -> (String, usize) {
    let mut name = String::new();
    let mut chars = rest.char_indices().peekable();
    while let Some((j, c)) = chars.next() {
        match c {
            '"' => return (name, j + 1),
            '\\' => match chars.next() {
                Some((_, 'n')) => name.push('\n'),
                Some((_, 't')) => name.push('\t'),
                Some((_, 'r')) => name.push('\r'),
                Some((_, '0')) => name.push('\0'),
                Some((k, 'u')) => {
                    // `\u{64e}`
                    let hex: String = rest[k + 1..]
                        .chars()
                        .skip(1)
                        .take_while(|c| *c != '}')
                        .collect();
                    if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        name.push(c);
                    }
                    for _ in 0..hex.len() + 2 {
                        chars.next();
                    }
                }
                Some((_, other)) => name.push(other),
                None => {}
            },
            c => name.push(c),
        }
    }
    (name, rest.len())
}

/// What the entities left in the drawing name, with the handles and names their less plain
/// references hold (`any`). Names are upper-cased.
#[derive(Default)]
pub(crate) struct Uses {
    pub layers: HashSet<String>,
    pub blocks: HashSet<String>,
    pub styles: HashSet<String>,
    pub linetypes: HashSet<String>,
    pub dimstyles: HashSet<String>,
    any: Mentions,
    /// A dimension with no block name may still own one: then every `*D` block stays.
    unnamed_dimension: bool,
}

impl Uses {
    fn add(&mut self, doc: &CadDocument, e: &EntityType) {
        let common = e.common();
        self.layers.insert(normalize_name(&common.layer));
        self.linetypes.insert(normalize_name(&common.linetype));
        if let Some(h) = common.linetype_handle {
            self.any.handles.insert(h.value());
        }
        match e {
            EntityType::Insert(i) => {
                self.blocks.insert(normalize_name(&i.block_name));
                for a in &i.attributes {
                    self.layers.insert(normalize_name(&a.common.layer));
                    self.linetypes.insert(normalize_name(&a.common.linetype));
                    self.styles.insert(normalize_name(&a.text_style));
                }
            }
            EntityType::Dimension(d) => {
                let base = d.base();
                self.unnamed_dimension |= no_block(&base.block_name);
                self.blocks.insert(normalize_name(&base.block_name));
                self.dimstyles.insert(normalize_name(&base.style_name));
                // Style overrides (DSTYLE) name arrowhead blocks, text styles and linetypes.
                self.any.scan(&common.extended_data);
            }
            EntityType::Table(t) => {
                self.blocks.insert(normalize_name(&t.block_name));
                if let Some(b) = qiyas_geom::table_block(doc, t) {
                    self.blocks.insert(normalize_name(&b.name));
                }
                self.any.scan(t);
            }
            EntityType::Text(t) => {
                self.styles.insert(normalize_name(&t.style));
            }
            EntityType::MText(t) => {
                self.styles.insert(normalize_name(&t.style));
            }
            EntityType::AttributeDefinition(a) => {
                self.styles.insert(normalize_name(&a.text_style));
            }
            EntityType::AttributeEntity(a) => {
                self.styles.insert(normalize_name(&a.text_style));
            }
            EntityType::Leader(l) => {
                self.dimstyles.insert(normalize_name(&l.dimension_style));
                self.any.scan(&common.extended_data);
            }
            EntityType::Tolerance(t) => {
                self.dimstyles
                    .insert(normalize_name(&t.dimension_style_name));
                self.any.scan(&common.extended_data);
                if let Some(h) = t.dimension_style_handle {
                    self.any.handles.insert(h.value());
                }
            }
            EntityType::Shape(s) => {
                self.styles.insert(normalize_name(&s.style_name));
                if let Some(h) = s.style_handle {
                    self.any.handles.insert(h.value());
                }
            }
            EntityType::MultiLeader(_)
            | EntityType::Extended(_)
            | EntityType::Unknown(_)
            | EntityType::SectionSymbol(_)
            | EntityType::ViewBorder(_) => self.any.scan(e),
            _ => {}
        }
    }
}

/// The handles and names held outside the entities: the header, the dimension styles (arrowhead
/// blocks, text style, linetypes), the linetypes (a complex one's text style) and the objects
/// (multileader and table styles, multiline styles, layer states...). An object owned by an
/// entry itself (its extension dictionary, its draw order) does not keep it.
#[derive(Default)]
pub(crate) struct Held {
    header: Mentions,
    dimstyles: Mentions,
    linetypes: Mentions,
    object_names: HashSet<String>,
    /// For each table entry's handle, the objects that mention it.
    object_refs: HashMap<u64, Vec<Handle>>,
}

impl Held {
    pub(crate) fn read(doc: &CadDocument) -> Held {
        let mut header = Mentions::default();
        header.scan(&doc.header);
        // The section view style's arrowhead blocks are bare numbers, not `Handle`s.
        if let Some(style) = &doc.section_view_style {
            header.scan(style);
            header.handles.extend(
                [style.arrow_start_handle, style.arrow_end_handle]
                    .into_iter()
                    .filter(|&h| h != 0),
            );
        }
        let mut dimstyles = Mentions::default();
        for d in doc.dim_styles.iter() {
            dimstyles.scan(d);
        }
        let mut linetypes = Mentions::default();
        for l in doc.line_types.iter() {
            linetypes.scan(l);
        }
        let entries: HashSet<u64> = doc
            .block_records
            .iter()
            .map(|b| b.handle)
            .chain(doc.text_styles.iter().map(|s| s.handle))
            .chain(doc.line_types.iter().map(|l| l.handle))
            .chain(doc.dim_styles.iter().map(|d| d.handle))
            .map(|h| h.value())
            .collect();
        let mut object_names = HashSet::new();
        let mut object_refs: HashMap<u64, Vec<Handle>> = HashMap::new();
        for (&h, o) in &doc.objects {
            let mut m = Mentions::default();
            m.scan(o);
            object_names.extend(m.names);
            for r in m.handles.into_iter().filter(|r| entries.contains(r)) {
                object_refs.entry(r).or_default().push(h);
            }
        }
        Held {
            header,
            dimstyles,
            linetypes,
            object_names,
            object_refs,
        }
    }

    /// An object other than one the entry `handle` owns mentions it (by handle or by `name`).
    fn by_objects(&self, doc: &CadDocument, name: &str, handle: Handle) -> bool {
        self.object_names.contains(&normalize_name(name))
            || self
                .object_refs
                .get(&handle.value())
                .is_some_and(|objects| objects.iter().any(|&o| !doc.owner_chain_reaches(o, handle)))
    }
}

/// What a purge of `kinds` takes out. Names as the tables keep them.
#[derive(Default)]
struct Plan {
    layers: Vec<String>,
    blocks: Vec<String>,
    styles: Vec<String>,
    linetypes: Vec<String>,
    dimstyles: Vec<String>,
    groups: Vec<String>,
    /// Groups that stay, with the members still in the drawing; and how many went.
    group_members: Vec<(Handle, Vec<Handle>)>,
    gone_members: usize,
    limited_reason: Option<String>,
}

impl Plan {
    fn report(&self, dry_run: bool) -> PurgeReport {
        let sorted = |names: &[String]| {
            let mut names = names.to_vec();
            names.sort_by_key(|n| normalize_name(n));
            names
        };
        let report = PurgeReport {
            dry_run,
            layers: sorted(&self.layers),
            blocks: sorted(&self.blocks),
            styles: sorted(&self.styles),
            linetypes: sorted(&self.linetypes),
            dimstyles: sorted(&self.dimstyles),
            groups: sorted(&self.groups),
            group_members: self.gone_members,
            total: 0,
            limited_reason: self.limited_reason.clone(),
        };
        let total = report.layers.len()
            + report.blocks.len()
            + report.styles.len()
            + report.linetypes.len()
            + report.dimstyles.len()
            + report.groups.len()
            + report.group_members;
        PurgeReport { total, ..report }
    }
}

/// A dimension's block name that names no block: none, or `*U0`, which the DWG reader gives a
/// null block handle.
pub(crate) fn no_block(name: &str) -> bool {
    name.is_empty() || name.eq_ignore_ascii_case("*U0")
}

/// Every kind, in the report's order.
const ALL_KINDS: [PurgeKind; 6] = [
    PurgeKind::Layers,
    PurgeKind::Blocks,
    PurgeKind::Styles,
    PurgeKind::Linetypes,
    PurgeKind::Dimstyles,
    PurgeKind::Groups,
];

/// How many objects and entities the codec keeps unread (unknown kinds, proxies): what they
/// point at can't be seen.
fn unread_data(doc: &CadDocument) -> usize {
    let objects = doc
        .objects
        .values()
        .filter(|o| matches!(o, ObjectType::Unknown { .. } | ObjectType::ProxyObject(_)))
        .count();
    let entities = doc
        .entities()
        .filter(|e| matches!(e, EntityType::Unknown(_)))
        .count();
    objects + entities
}

/// `*D12` is an anonymous block of kind `D`.
pub(crate) fn anonymous_kind(name: &str) -> Option<char> {
    let mut chars = name.chars();
    let kind = match (chars.next(), chars.next()) {
        (Some('*'), Some(k)) => k.to_ascii_uppercase(),
        _ => return None,
    };
    let rest = chars.as_str();
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())).then_some(kind)
}

/// The take-off's layers, which every save writes again (`crate::embed`): purging them only
/// makes the next save put them back.
const TAKEOFF_LAYERS: [&str; 5] = [
    "QIYAS-LENGTH",
    "QIYAS-AREA",
    "QIYAS-COUNT",
    "QIYAS-ITEM",
    "QIYAS-MARKUP",
];

/// A name held by an xref (`XREF|LAYER`), or by none.
fn foreign_name(name: &str) -> bool {
    name.trim().is_empty() || name.contains('|')
}

/// Names a table holds twice: either one could be the one in use, so neither goes.
fn twice<'a>(names: impl Iterator<Item = &'a str>) -> HashSet<String> {
    let mut seen = HashSet::new();
    names
        .map(normalize_name)
        .filter(|n| !seen.insert(n.clone()))
        .collect()
}

/// A layout, an xref or a dynamic block's definition: never purged, used or not.
fn kept_block(doc: &CadDocument, b: &BlockRecord) -> bool {
    let dynamic = doc.block_representations.values().any(|&d| d == b.handle)
        || doc.block_visibility_param_for_def(b.handle).is_some()
        || doc
            .extension_dictionary_handle(b.handle)
            .and_then(|x| match doc.objects.get(&x) {
                Some(ObjectType::Dictionary(d)) => Some(d),
                _ => None,
            })
            .is_some_and(|d| {
                d.entries
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case("ACAD_ENHANCEDBLOCK"))
            });
    !b.layout.is_null()
        || b.flags.is_xref
        || b.flags.is_xref_overlay
        || b.flags.is_external
        || !b.xref_path.is_empty()
        || foreign_name(&b.name)
        || dynamic
}

/// What the entities name once the blocks `gone` (their entities `gone_entities`) are out.
fn live_uses(doc: &CadDocument, gone: &HashSet<String>, gone_entities: &HashSet<Handle>) -> Uses {
    let mut uses = Uses::default();
    for e in doc.entities() {
        if !gone_entities.contains(&e.common().handle) {
            uses.add(doc, e);
        }
    }
    // The BLOCK and ENDBLK markers of the blocks that stay are on a layer too.
    for b in doc.block_records.iter() {
        if gone.contains(&normalize_name(&b.name)) {
            continue;
        }
        for h in [b.block_entity_handle, b.block_end_handle] {
            if let Some(e) = doc.get_entity(h) {
                uses.layers.insert(normalize_name(&e.common().layer));
            }
        }
    }
    uses
}

impl Session {
    /// `Command::Purge`: what nothing uses, of `kinds` (every kind when empty). A dry run only
    /// lists it; otherwise it goes, as one undo step (`run_as` records it).
    pub(crate) fn purge(
        &mut self,
        kinds: &[PurgeKind],
        dry_run: bool,
    ) -> Result<PurgeReport, CoreError> {
        let plan = self.purge_plan(kinds);
        let report = plan.report(dry_run);
        if dry_run || report.total == 0 {
            return Ok(report);
        }
        if let Some(name) = self.block_edit_name() {
            return Err(CoreError::InvalidCommand(format!(
                "Block {name} is being edited in place: save or discard the edit first (REFCLOSE)"
            )));
        }
        let doc = &mut self.drawing.doc;
        for name in &plan.blocks {
            let Some(record) = doc.block_records.remove(name) else {
                continue;
            };
            for &h in record
                .entity_handles
                .iter()
                .chain([&record.block_entity_handle, &record.block_end_handle])
            {
                doc.remove_entity_arc(h);
            }
        }
        for name in &plan.layers {
            let Some(layer) = doc.layers.remove(name) else {
                continue;
            };
            // As DeleteLayer: viewports that froze it no longer do.
            let viewports: Vec<Handle> = doc
                .entities()
                .filter_map(|e| match e {
                    EntityType::Viewport(v) if v.frozen_layers.contains(&layer.handle) => {
                        Some(v.common.handle)
                    }
                    _ => None,
                })
                .collect();
            for h in viewports {
                if let Some(EntityType::Viewport(v)) = doc.get_entity_mut(h) {
                    v.frozen_layers.retain(|l| *l != layer.handle);
                }
            }
            let key = normalize_name(name);
            self.visibility
                .hidden_layers
                .retain(|n| normalize_name(n) != key);
        }
        let doc = &mut self.drawing.doc;
        for name in &plan.dimstyles {
            doc.dim_styles.remove(name);
        }
        for name in &plan.linetypes {
            doc.line_types.remove(name);
        }
        for name in &plan.styles {
            doc.text_styles.remove(name);
        }
        for (h, live) in &plan.group_members {
            if let Some(ObjectType::Group(g)) = doc.objects.get_mut(h) {
                g.entities = live.clone();
            }
        }
        for name in &plan.groups {
            self.delete_group(name)?;
        }
        Ok(report)
    }

    fn purge_plan(&self, kinds: &[PurgeKind]) -> Plan {
        let doc = &self.drawing.doc;
        let asked = |k| kinds.is_empty() || kinds.contains(&k);
        // Data the codec keeps unread may point at any table entry: then only layers, which
        // every entity names in the open, are looked at.
        let unread = unread_data(doc);
        let limited = unread > 0
            && ALL_KINDS
                .iter()
                .any(|&k| k != PurgeKind::Layers && asked(k));
        let want = |k| asked(k) && (k == PurgeKind::Layers || unread == 0);
        // Layers and groups need none of it, and reading it formats every object.
        let held = if [
            PurgeKind::Blocks,
            PurgeKind::Styles,
            PurgeKind::Linetypes,
            PurgeKind::Dimstyles,
        ]
        .into_iter()
        .any(want)
        {
            Held::read(doc)
        } else {
            Held::default()
        };
        let mut plan = Plan {
            limited_reason: limited.then(|| {
                format!(
                    "The drawing holds {unread} object{} Qiyas can't read, which may use blocks, \
                     styles or linetypes: only layers were looked at",
                    if unread == 1 { "" } else { "s" }
                )
            }),
            ..Plan::default()
        };

        // Blocks first: what an unused block holds uses nothing.
        let doubled = twice(doc.block_records.iter().map(|b| b.name.as_str()));
        let candidates: Vec<&BlockRecord> = if want(PurgeKind::Blocks) {
            doc.block_records
                .iter()
                .filter(|b| {
                    // Of the anonymous blocks, only the save-time purge's: `*Dn` and `*Tn`.
                    let eligible = match anonymous_kind(&b.name) {
                        Some(kind) => kind == 'D' || kind == 'T',
                        None => !b.name.starts_with('*'),
                    };
                    eligible && !doubled.contains(&normalize_name(&b.name)) && !kept_block(doc, b)
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut gone: HashSet<String> = HashSet::new();
        let mut gone_entities: HashSet<Handle> = HashSet::new();
        let uses = loop {
            let uses = live_uses(doc, &gone, &gone_entities);
            let more: Vec<&BlockRecord> = candidates
                .iter()
                .copied()
                .filter(|b| {
                    let name = normalize_name(&b.name);
                    !gone.contains(&name)
                        && !uses.blocks.contains(&name)
                        && !(anonymous_kind(&b.name) == Some('D') && uses.unnamed_dimension)
                        && !uses.any.has(&b.name, b.handle)
                        && !held.header.has(&b.name, b.handle)
                        && !held.dimstyles.has(&b.name, b.handle)
                        && !held.by_objects(doc, &b.name, b.handle)
                })
                .collect();
            if more.is_empty() {
                break uses;
            }
            for b in more {
                gone.insert(normalize_name(&b.name));
                plan.blocks.push(b.name.clone());
                gone_entities.extend(
                    b.entity_handles
                        .iter()
                        .chain([&b.block_entity_handle, &b.block_end_handle]),
                );
            }
        };

        if want(PurgeKind::Layers) {
            let doubled = twice(doc.layers.iter().map(|l| l.name.as_str()));
            let current = [
                normalize_name(&self.current.layer),
                normalize_name(&doc.header.current_layer_name),
            ];
            plan.layers = doc
                .layers
                .iter()
                .filter(|l| {
                    let name = normalize_name(&l.name);
                    !matches!(name.as_str(), "0" | "DEFPOINTS")
                        && !TAKEOFF_LAYERS.contains(&name.as_str())
                        && !foreign_name(&l.name)
                        && !l.flags.xref_dependent
                        && !doubled.contains(&name)
                        && !current.contains(&name)
                        && l.handle != doc.header.current_layer_handle
                        && !uses.layers.contains(&name)
                })
                .map(|l| l.name.clone())
                .collect();
        }
        let purged_layers: HashSet<String> =
            plan.layers.iter().map(|n| normalize_name(n)).collect();

        if want(PurgeKind::Dimstyles) {
            let doubled = twice(doc.dim_styles.iter().map(|d| d.name.as_str()));
            let unused: Vec<String> = doc
                .dim_styles
                .iter()
                .filter(|d| {
                    let name = normalize_name(&d.name);
                    name != "STANDARD"
                        && !foreign_name(&d.name)
                        && !d.xref_dependent
                        && !doubled.contains(&name)
                        && !uses.dimstyles.contains(&name)
                        && !uses.any.has(&d.name, d.handle)
                        && !held.header.has(&d.name, d.handle)
                        && !held.by_objects(doc, &d.name, d.handle)
                })
                .map(|d| d.name.clone())
                .collect();
            let going: HashSet<String> = unused.iter().map(|n| normalize_name(n)).collect();
            // `ISO-25$2` is ISO-25's style for one kind of dimension: it stays with ISO-25.
            plan.dimstyles = unused
                .into_iter()
                .filter(|n| match n.rsplit_once('$') {
                    Some((parent, digit))
                        if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) =>
                    {
                        doc.dim_styles.get(parent).is_none()
                            || going.contains(&normalize_name(parent))
                    }
                    _ => true,
                })
                .collect();
        }

        if want(PurgeKind::Linetypes) {
            let doubled = twice(doc.line_types.iter().map(|l| l.name.as_str()));
            let mut used = uses.linetypes.clone();
            used.insert(normalize_name(&self.current.linetype));
            used.insert(normalize_name(&doc.header.current_linetype_name));
            used.extend(
                doc.layers
                    .iter()
                    .filter(|l| !purged_layers.contains(&normalize_name(&l.name)))
                    .map(|l| normalize_name(&l.line_type)),
            );
            plan.linetypes = doc
                .line_types
                .iter()
                .filter(|l| {
                    let name = normalize_name(&l.name);
                    !matches!(name.as_str(), "BYLAYER" | "BYBLOCK" | "CONTINUOUS")
                        && !foreign_name(&l.name)
                        && !l.xref_dependent
                        && !doubled.contains(&name)
                        && !used.contains(&name)
                        && !uses.any.has(&l.name, l.handle)
                        && !held.header.has(&l.name, l.handle)
                        && !held.dimstyles.has(&l.name, l.handle)
                        && !held.by_objects(doc, &l.name, l.handle)
                })
                .map(|l| l.name.clone())
                .collect();
        }

        if want(PurgeKind::Styles) {
            let doubled = twice(doc.text_styles.iter().map(|s| s.name.as_str()));
            plan.styles = doc
                .text_styles
                .iter()
                .filter(|s| {
                    let name = normalize_name(&s.name);
                    name != "STANDARD"
                        && !s.is_shape_file
                        && !foreign_name(&s.name)
                        && !s.xref_dependent
                        && !doubled.contains(&name)
                        && !uses.styles.contains(&name)
                        && !uses.any.has(&s.name, s.handle)
                        && !held.header.has(&s.name, s.handle)
                        && !held.dimstyles.has(&s.name, s.handle)
                        && !held.linetypes.has(&s.name, s.handle)
                        && !held.by_objects(doc, &s.name, s.handle)
                })
                .map(|s| s.name.clone())
                .collect();
        }

        if want(PurgeKind::Groups) {
            plan.groups = self
                .groups()
                .into_iter()
                .filter(|g| g.handles.is_empty())
                .map(|g| g.name)
                .collect();
            for (&h, o) in &doc.objects {
                let ObjectType::Group(g) = o else { continue };
                let live: Vec<Handle> = g
                    .entities
                    .iter()
                    .copied()
                    .filter(|&e| doc.get_entity(e).is_some())
                    .collect();
                if !live.is_empty() && live.len() < g.entities.len() {
                    plan.gone_members += g.entities.len() - live.len();
                    plan.group_members.push((h, live));
                }
            }
        }
        plan
    }

    /// The layers purge would take out, for the audit's list of unreferenced layers.
    pub(crate) fn unused_layers(&self) -> Vec<String> {
        self.purge_plan(&[PurgeKind::Layers]).report(true).layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_read_handles_and_names_off_debug_text() {
        // The scan relies on how `Handle` prints: a codec that prints it otherwise fails here.
        assert_eq!(format!("{:?}", Handle::new(17)), "Handle(17)");
        let mut m = Mentions::default();
        m.scan(&(Handle::new(42), "Dash \"dot\"", Some(Handle::new(7))));
        assert!(m.handles.contains(&42) && m.handles.contains(&7));
        assert!(m.names.contains("DASH \"DOT\""));
    }

    #[test]
    fn mentions_survive_non_ascii_chars_and_read_escapes_and_hex() {
        let mut m = Mentions::default();
        // A char outside any string ('Ä' is two bytes), a quote char, then a handle.
        m.scan(&('Ä', '"', '\'', Handle::new(5), "\u{64e}جدار", "1A"));
        assert!(m.handles.contains(&5), "{:?}", m.handles);
        assert!(m.names.contains("\u{64e}جدار"), "{:?}", m.names);
        // DXF raw pairs keep handles as hex text.
        assert!(m.handles.contains(&0x1a));
    }

    #[test]
    fn anonymous_kinds() {
        assert_eq!(anonymous_kind("*D12"), Some('D'));
        assert_eq!(anonymous_kind("*t3"), Some('T'));
        assert_eq!(anonymous_kind("*U1"), Some('U'));
        assert_eq!(anonymous_kind("*D"), None);
        assert_eq!(anonymous_kind("*Model_Space"), None);
        assert_eq!(anonymous_kind("DOOR"), None);
    }
}
