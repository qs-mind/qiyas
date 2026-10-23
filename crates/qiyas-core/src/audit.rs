//! AUDIT (AutoCAD's): what in the drawing points at something it does not have, with what is
//! only worth knowing (`Command::Audit`).
//!
//! Problems: entities whose block is gone (not in any block's list, their owner no block or entity
//! any more), references to blocks the drawing does not have, dimensions and tables whose `*D` /
//! `*T` block is gone (they draw from their own points and cells), group members gone from the
//! drawing, entities (and attributes) on layers the drawing has no entry for, and texts,
//! attributes and attribute definitions in text styles it has none for. Listed only: empty block
//! definitions and layers with nothing on them (what PURGE would take).
//!
//! `fix` repairs what is safe, as one undo step (`run_as` records it): an entity or attribute on
//! a missing layer goes to layer 0, a missing text style becomes Standard, gone group members are
//! dropped. References to missing blocks are listed, never deleted; entities whose block is gone
//! and dimensions or tables without their block are listed only.

use std::collections::{BTreeMap, HashMap, HashSet};

use opencadcodec::objects::ObjectType;
use opencadcodec::tables::normalize_name;
use opencadcodec::{CadDocument, EntityType, Handle};
use serde::Serialize;

use crate::purge::no_block;
use crate::{CoreError, Session};

/// The handles an item lists at most; `count` stays exact.
pub const MAX_HANDLES: usize = 100;

/// What an audit line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    /// Entities whose block is gone; `name` is the gone owner's handle in hex.
    OwnerGone,
    /// References to a block the drawing does not have; `name` is the block.
    MissingBlock,
    /// A dimension whose block is gone; `name` is the block.
    DimensionBlockGone,
    /// A table whose block is gone; `name` is the block.
    TableBlockGone,
    /// Members a group lists that are gone from the drawing; `name` is the group.
    GroupMemberGone,
    /// Entities on a layer the drawing has no entry for; `name` is the layer.
    MissingLayer,
    /// Texts in a style the drawing has none for; `name` is the style.
    MissingTextStyle,
    /// A block definition with nothing in it (listed only).
    EmptyBlock,
    /// A layer with no entity on it anywhere (listed only; PURGE takes it).
    UnusedLayer,
}

/// One line of the report: a kind of problem and the name it is about, how many entities,
/// attributes or members it touches (1 for a table entry), the first `MAX_HANDLES` of them (a
/// table entry has none), the line to show, and whether `fix` repaired it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuditItem {
    pub kind: AuditKind,
    pub name: String,
    pub count: usize,
    pub handles: Vec<u64>,
    pub line: String,
    pub fixed: bool,
}

/// What an audit found: `counts` by kind (entities, attributes or members, or table entries),
/// how many of them the repair changed (0 without `fix`), and one item per kind and name, in
/// the order of `AuditKind` and then by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AuditReport {
    pub fix: bool,
    pub counts: BTreeMap<AuditKind, usize>,
    pub repaired: usize,
    pub items: Vec<AuditItem>,
}

/// Items being gathered, by kind and upper-cased name: the name as first seen and the handles.
#[derive(Default)]
struct Found(BTreeMap<(AuditKind, String), (String, Vec<u64>, usize)>);

impl Found {
    fn add(&mut self, kind: AuditKind, name: &str, handle: Option<Handle>) {
        let entry = self
            .0
            .entry((kind, normalize_name(name)))
            .or_insert_with(|| (name.to_string(), Vec::new(), 0));
        entry.2 += 1;
        if let Some(h) = handle {
            if entry.1.len() < MAX_HANDLES && entry.1.last() != Some(&h.value()) {
                entry.1.push(h.value());
            }
        }
    }
}

/// The text style a text-like entity names, if it is one.
fn text_style(e: &EntityType) -> Option<&str> {
    match e {
        EntityType::Text(t) => Some(&t.style),
        EntityType::MText(t) => Some(&t.style),
        EntityType::AttributeDefinition(a) => Some(&a.text_style),
        EntityType::AttributeEntity(a) => Some(&a.text_style),
        _ => None,
    }
}

/// The groups' names as their dictionaries keep them, by the group object's handle.
fn group_names(doc: &CadDocument) -> HashMap<Handle, String> {
    doc.objects
        .values()
        .filter_map(|o| match o {
            ObjectType::Dictionary(d) => Some(&d.entries),
            _ => None,
        })
        .flatten()
        .filter(|(_, h)| matches!(doc.objects.get(h), Some(ObjectType::Group(_))))
        .map(|(name, h)| (*h, name.clone()))
        .collect()
}

fn line(kind: AuditKind, name: &str, count: usize, fixed: bool) -> String {
    let entities = |n: usize| {
        if n == 1 {
            "1 entity".to_string()
        } else {
            format!("{n} entities")
        }
    };
    match (kind, fixed) {
        (AuditKind::OwnerGone, _) => format!(
            "{} whose block (handle {name}) is gone: not drawn",
            entities(count)
        ),
        (AuditKind::MissingBlock, _) => format!(
            "{count} reference{} to block {name}, which the drawing does not have: left as {}",
            if count == 1 { "" } else { "s" },
            if count == 1 { "it is" } else { "they are" }
        ),
        (AuditKind::DimensionBlockGone, _) => {
            format!("Dimension block {name} is gone: the dimension draws from its own points")
        }
        (AuditKind::TableBlockGone, _) => {
            format!("Table block {name} is gone: the table draws from its cells")
        }
        (AuditKind::GroupMemberGone, false) => format!(
            "Group {name} lists {} the drawing no longer has",
            entities(count)
        ),
        (AuditKind::GroupMemberGone, true) => format!(
            "Group {name} listed {} the drawing no longer has: dropped",
            entities(count)
        ),
        (AuditKind::MissingLayer, false) => format!(
            "{} on layer {name}, which the drawing has no layer for",
            entities(count)
        ),
        (AuditKind::MissingLayer, true) => format!(
            "{} on layer {name}, which the drawing has no layer for: moved to layer 0",
            entities(count)
        ),
        (AuditKind::MissingTextStyle, false) => format!(
            "{} in text style {name}, which the drawing has no style for",
            entities(count)
        ),
        (AuditKind::MissingTextStyle, true) => format!(
            "{} in text style {name}, which the drawing has no style for: now in Standard",
            entities(count)
        ),
        (AuditKind::EmptyBlock, _) => format!("Block {name} has nothing in it"),
        (AuditKind::UnusedLayer, _) => format!("Layer {name} has nothing on it"),
    }
}

impl Session {
    /// `Command::Audit`: the report, and with `fix` the safe repairs (see the module).
    pub(crate) fn audit(&mut self, fix: bool) -> Result<AuditReport, CoreError> {
        // The edit's working copies sit on the sheet: a repair would drop group members it took
        // out for the while (as purge, only the dry run goes ahead).
        if let Some(name) = self.block_edit_name().filter(|_| fix) {
            return Err(CoreError::InvalidCommand(format!(
                "Block {name} is being edited in place: save or discard the edit first (REFCLOSE)"
            )));
        }
        let found = self.audit_scan();
        let doc = &self.drawing.doc;
        let zero = doc.layers.get("0").map(|l| l.name.clone());
        let standard = doc.text_styles.get("Standard").map(|s| s.name.clone());
        let mut repairs = HashSet::new();
        if fix {
            if let Some(zero) = &zero {
                self.repair_layers(zero);
                repairs.insert(AuditKind::MissingLayer);
            }
            if let Some(standard) = &standard {
                self.repair_styles(standard);
                repairs.insert(AuditKind::MissingTextStyle);
            }
            self.repair_groups();
            repairs.insert(AuditKind::GroupMemberGone);
        }
        let mut report = AuditReport {
            fix,
            ..AuditReport::default()
        };
        for ((kind, _), (name, handles, count)) in found.0 {
            let fixed = repairs.contains(&kind);
            *report.counts.entry(kind).or_default() += count;
            if fixed {
                report.repaired += count;
            }
            report.items.push(AuditItem {
                kind,
                line: line(kind, &name, count, fixed),
                name,
                count,
                handles,
                fixed,
            });
        }
        Ok(report)
    }

    /// Everything the audit lists, before any repair.
    fn audit_scan(&self) -> Found {
        let doc = &self.drawing.doc;
        let mut found = Found::default();
        let listed: HashSet<Handle> = doc
            .block_records
            .iter()
            .flat_map(|b| b.entity_handles.iter().copied())
            .collect();
        let blocks: HashSet<Handle> = doc.block_records.iter().map(|b| b.handle).collect();
        let no_layer = |name: &str| doc.layers.get(name).is_none();
        let no_style = |name: &str| doc.text_styles.get(name).is_none();
        for e in doc.entities() {
            let common = e.common();
            let h = common.handle;
            let owner = common.owner_handle;
            let sub_entity = matches!(e, EntityType::AttributeEntity(_) | EntityType::Seqend(_));
            if !sub_entity
                && !listed.contains(&h)
                && !blocks.contains(&owner)
                && doc.get_entity(owner).is_none()
            {
                found.add(AuditKind::OwnerGone, &format!("{}", owner.value()), Some(h));
            }
            if no_layer(&common.layer) {
                found.add(AuditKind::MissingLayer, &common.layer, Some(h));
            }
            if let Some(style) = text_style(e).filter(|s| no_style(s)) {
                found.add(AuditKind::MissingTextStyle, style, Some(h));
            }
            match e {
                EntityType::Insert(i) => {
                    if doc.block_records.get(&i.block_name).is_none() {
                        found.add(AuditKind::MissingBlock, &i.block_name, Some(h));
                    }
                    for a in &i.attributes {
                        if no_layer(&a.common.layer) {
                            found.add(AuditKind::MissingLayer, &a.common.layer, Some(h));
                        }
                        if no_style(&a.text_style) {
                            found.add(AuditKind::MissingTextStyle, &a.text_style, Some(h));
                        }
                    }
                }
                EntityType::Dimension(d) => {
                    let name = &d.base().block_name;
                    if !no_block(name) && doc.block_records.get(name).is_none() {
                        found.add(AuditKind::DimensionBlockGone, name, Some(h));
                    }
                }
                EntityType::Table(t) => {
                    let named = !t.block_name.is_empty() || t.block_record_handle.is_some();
                    if named && qiyas_geom::table_block(doc, t).is_none() {
                        let name = if t.block_name.is_empty() {
                            format!("{}", t.block_record_handle.unwrap_or_default().value())
                        } else {
                            t.block_name.clone()
                        };
                        found.add(AuditKind::TableBlockGone, &name, Some(h));
                    }
                }
                _ => {}
            }
        }
        let names = group_names(doc);
        for (&gh, o) in &doc.objects {
            let ObjectType::Group(g) = o else { continue };
            let name = names
                .get(&gh)
                .cloned()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| g.name.clone());
            for &m in g.entities.iter().filter(|&&m| doc.get_entity(m).is_none()) {
                found.add(AuditKind::GroupMemberGone, &name, Some(m));
            }
        }
        for b in doc.block_records.iter() {
            let layout = !b.layout.is_null()
                || b.name.eq_ignore_ascii_case("*Model_Space")
                || b.name.to_ascii_uppercase().starts_with("*PAPER_SPACE");
            let xref = b.flags.is_xref || b.flags.is_xref_overlay || !b.xref_path.is_empty();
            let empty = !b
                .entity_handles
                .iter()
                .any(|&h| doc.get_entity(h).is_some());
            if !layout && !xref && empty {
                found.add(AuditKind::EmptyBlock, &b.name, None);
            }
        }
        for name in self.unused_layers() {
            found.add(AuditKind::UnusedLayer, &name, None);
        }
        found
    }

    /// Entities and attributes on a layer the drawing has no entry for go to `zero`.
    fn repair_layers(&mut self, zero: &str) {
        let doc = &self.drawing.doc;
        let missing = |name: &str| doc.layers.get(name).is_none();
        let wrong: Vec<Handle> = doc
            .entities()
            .filter(|e| {
                missing(&e.common().layer)
                    || matches!(e, EntityType::Insert(i)
                        if i.attributes.iter().any(|a| missing(&a.common.layer)))
            })
            .map(|e| e.common().handle)
            .collect();
        let layers: HashSet<String> = doc.layers.iter().map(|l| normalize_name(&l.name)).collect();
        let missing = |name: &str| !layers.contains(&normalize_name(name));
        for h in wrong {
            let Some(e) = self.drawing.doc.get_entity_mut(h) else {
                continue;
            };
            if missing(&e.common().layer) {
                e.common_mut().layer = zero.to_string();
            }
            if let EntityType::Insert(i) = e {
                for a in &mut i.attributes {
                    if missing(&a.common.layer) {
                        a.common.layer = zero.to_string();
                    }
                }
            }
        }
    }

    /// Texts, attributes and attribute definitions in a text style the drawing has none for
    /// take `standard`.
    fn repair_styles(&mut self, standard: &str) {
        let doc = &self.drawing.doc;
        let styles: HashSet<String> = doc
            .text_styles
            .iter()
            .map(|s| normalize_name(&s.name))
            .collect();
        let missing = |name: &str| !styles.contains(&normalize_name(name));
        let wrong: Vec<Handle> = doc
            .entities()
            .filter(|e| {
                text_style(e).is_some_and(missing)
                    || matches!(e, EntityType::Insert(i)
                        if i.attributes.iter().any(|a| missing(&a.text_style)))
            })
            .map(|e| e.common().handle)
            .collect();
        for h in wrong {
            let style = match self.drawing.doc.get_entity_mut(h) {
                Some(EntityType::Text(t)) => &mut t.style,
                Some(EntityType::MText(t)) => &mut t.style,
                Some(EntityType::AttributeDefinition(a)) => &mut a.text_style,
                Some(EntityType::AttributeEntity(a)) => &mut a.text_style,
                Some(EntityType::Insert(i)) => {
                    for a in &mut i.attributes {
                        if missing(&a.text_style) {
                            a.text_style = standard.to_string();
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            if missing(style) {
                *style = standard.to_string();
            }
        }
    }

    /// Groups no longer list members gone from the drawing.
    fn repair_groups(&mut self) {
        let doc = &mut self.drawing.doc;
        let live: Vec<(Handle, Vec<Handle>)> = doc
            .objects
            .iter()
            .filter_map(|(&h, o)| match o {
                ObjectType::Group(g) if g.entities.iter().any(|&m| doc.get_entity(m).is_none()) => {
                    let live = g
                        .entities
                        .iter()
                        .copied()
                        .filter(|&m| doc.get_entity(m).is_some())
                        .collect();
                    Some((h, live))
                }
                _ => None,
            })
            .collect();
        for (h, members) in live {
            if let Some(ObjectType::Group(g)) = doc.objects.get_mut(&h) {
                g.entities = members;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_plainly() {
        assert_eq!(
            line(AuditKind::MissingLayer, "GHOST", 3, true),
            "3 entities on layer GHOST, which the drawing has no layer for: moved to layer 0"
        );
        assert_eq!(
            line(AuditKind::MissingBlock, "CHAIR", 1, false),
            "1 reference to block CHAIR, which the drawing does not have: left as it is"
        );
    }
}
