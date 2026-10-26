//! Groups: named sets of entities kept as AutoCAD keeps them (`GROUP` objects in the
//! `ACAD_GROUP` dictionary, each member pointing back at its group), so AutoCAD sees them. Only
//! the drawing's own entities at the top of the sheet join one. Each command is one undo step;
//! deleting a group (AutoCAD's "explode" of a group) leaves its entities as they are.

use opencadcodec::objects::{Dictionary, Group, ObjectType};
use opencadcodec::Handle;
use serde::Serialize;

use crate::blocks::BAD_NAME_CHARS;
use crate::session::variant_name;
use crate::{CoreError, Session, Target};

/// A group of the drawing: its name, its entities (those still in the drawing) with each one's
/// kind ("Line", "Dimension"…), and whether picking one picks the group.
#[derive(Debug, Clone, Serialize)]
pub struct GroupInfo {
    pub name: String,
    pub handles: Vec<u64>,
    pub kinds: Vec<String>,
    pub selectable: bool,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

impl Session {
    /// The `ACAD_GROUP` dictionary, if the drawing has one.
    fn group_dictionary(&self) -> Option<Handle> {
        let doc = &self.drawing.doc;
        let is_dictionary =
            |h: &Handle| matches!(doc.objects.get(h), Some(ObjectType::Dictionary(_)));
        let from_header = doc.header.acad_group_dict_handle;
        if is_dictionary(&from_header) {
            return Some(from_header);
        }
        match doc.objects.get(&doc.header.named_objects_dict_handle) {
            Some(ObjectType::Dictionary(root)) => root
                .entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("ACAD_GROUP"))
                .map(|(_, h)| *h)
                .filter(is_dictionary),
            _ => None,
        }
    }

    /// The `ACAD_GROUP` dictionary, made (under the root dictionary) when the drawing has none.
    fn ensure_group_dictionary(&mut self) -> Handle {
        if let Some(h) = self.group_dictionary() {
            return h;
        }
        let doc = &mut self.drawing.doc;
        let root = doc.header.named_objects_dict_handle;
        let mut dictionary = Dictionary::new();
        dictionary.handle = doc.allocate_handle();
        dictionary.owner = root;
        dictionary.hard_owner = true;
        let h = dictionary.handle;
        doc.objects.insert(h, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root) {
            root.add_entry("ACAD_GROUP", h);
        }
        doc.header.acad_group_dict_handle = h;
        h
    }

    /// The drawing's groups as the dictionary names them, in its order.
    pub(crate) fn groups(&self) -> Vec<GroupInfo> {
        let doc = &self.drawing.doc;
        let Some(Some(ObjectType::Dictionary(dictionary))) =
            self.group_dictionary().map(|h| doc.objects.get(&h))
        else {
            return Vec::new();
        };
        dictionary
            .entries
            .iter()
            .filter_map(|(name, h)| match doc.objects.get(h) {
                Some(ObjectType::Group(g)) => {
                    let members: Vec<_> = g
                        .entities
                        .iter()
                        .filter_map(|e| doc.get_entity(*e))
                        .collect();
                    Some(GroupInfo {
                        name: name.clone(),
                        handles: members.iter().map(|e| e.common().handle.value()).collect(),
                        kinds: members.iter().map(|e| variant_name(e)).collect(),
                        selectable: g.selectable,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// The group `name` (any case): its name as kept and its object's handle.
    pub(crate) fn find_group(&self, name: &str) -> Result<(String, Handle), CoreError> {
        let name = name.trim();
        let doc = &self.drawing.doc;
        self.group_dictionary()
            .and_then(|d| match doc.objects.get(&d) {
                Some(ObjectType::Dictionary(d)) => d
                    .entries
                    .iter()
                    .find(|(k, h)| {
                        k.eq_ignore_ascii_case(name)
                            && matches!(doc.objects.get(h), Some(ObjectType::Group(_)))
                    })
                    .cloned(),
                _ => None,
            })
            .ok_or_else(|| invalid(format!("No group named {name}")))
    }

    /// The group object `h`, to change.
    fn group_mut(&mut self, h: Handle) -> &mut Group {
        match self.drawing.doc.objects.get_mut(&h) {
            Some(ObjectType::Group(g)) => g,
            _ => unreachable!("found as a group"),
        }
    }

    /// Each of `members` points back at the group `group` (`add`) or no longer does.
    fn set_reactors(&mut self, members: &[Handle], group: Handle, add: bool) {
        for &h in members {
            if let Some(e) = self.drawing.doc.get_entity_mut(h) {
                let reactors = &mut e.common_mut().reactors;
                reactors.retain(|r| *r != group);
                if add {
                    reactors.push(group);
                }
            }
        }
    }

    /// `Command::CreateGroup`: a new group `name` of the entities `targets`.
    pub(crate) fn create_group(&mut self, name: &str, targets: &[Target]) -> Result<(), CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("Give the group a name"));
        }
        if let Some(c) = name
            .chars()
            .find(|c| BAD_NAME_CHARS.contains(c) || c.is_control())
        {
            return Err(invalid(format!("A group name can't hold \"{c}\"")));
        }
        let members = self.sheet_targets_any(targets)?;
        if let Ok((old, handle)) = self.find_group(name) {
            let existing: std::collections::BTreeSet<_> =
                self.group_mut(handle).entities.iter().copied().collect();
            if existing == members.iter().copied().collect() {
                self.last_advice.push(serde_json::json!({"code":"already_exists", "message":format!("Group {old} already exists with these members."), "message_ar":format!("المجموعة {old} موجودة بهذه العناصر.")}));
                return Ok(());
            }
            return Err(invalid(format!("Group {old} already exists with different members. Use add_to_group or remove_from_group to change it.")));
        }
        let dictionary = self.ensure_group_dictionary();
        let doc = &mut self.drawing.doc;
        let mut group = Group::new(name);
        group.handle = doc.allocate_handle();
        group.owner = dictionary;
        group.add_entities(members.iter().copied());
        let h = group.handle;
        doc.objects.insert(h, ObjectType::Group(group));
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&dictionary) {
            d.add_entry(name, h);
        }
        self.set_reactors(&members, h, true);
        Ok(())
    }

    /// `Command::AddToGroup`.
    pub(crate) fn add_to_group(&mut self, name: &str, targets: &[Target]) -> Result<(), CoreError> {
        let (_, h) = self.find_group(name)?;
        let members = self.sheet_targets_any(targets)?;
        self.group_mut(h).add_entities(members.iter().copied());
        self.set_reactors(&members, h, true);
        Ok(())
    }

    /// `Command::RemoveFromGroup`: those of `targets` in the group leave it.
    pub(crate) fn remove_from_group(
        &mut self,
        name: &str,
        targets: &[Target],
    ) -> Result<(), CoreError> {
        let (name, h) = self.find_group(name)?;
        let group = self.group_mut(h);
        let leaving: Vec<Handle> = targets
            .iter()
            .map(|t| Handle::new(t.handle))
            .filter(|t| group.remove_entity(*t))
            .collect();
        if leaving.is_empty() {
            return Err(invalid(format!("None of those is in group {name}")));
        }
        self.set_reactors(&leaving, h, false);
        Ok(())
    }

    /// `Command::DeleteGroup`: the group goes, its entities stay.
    pub(crate) fn delete_group(&mut self, name: &str) -> Result<(), CoreError> {
        let (name, h) = self.find_group(name)?;
        self.drop_group(&name, h);
        Ok(())
    }

    /// `Command::Ungroup`: every group holding one of `targets` goes, its entities stay.
    pub(crate) fn ungroup(&mut self, targets: &[Target]) -> Result<(), CoreError> {
        let picked = self.sheet_targets_any(targets)?;
        let doc = &self.drawing.doc;
        let going: Vec<(String, Handle)> =
            match self.group_dictionary().map(|d| doc.objects.get(&d)) {
                Some(Some(ObjectType::Dictionary(d))) => d
                    .entries
                    .iter()
                    .filter(|(_, h)| match doc.objects.get(h) {
                        Some(ObjectType::Group(g)) => picked.iter().any(|p| g.contains(*p)),
                        _ => false,
                    })
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
        if going.is_empty() {
            return Err(invalid("None of those is in a group"));
        }
        for (name, h) in going {
            self.drop_group(&name, h);
        }
        Ok(())
    }

    /// Removes the group object `h` (kept as `name`) and its dictionary entry; its members no
    /// longer point at it.
    fn drop_group(&mut self, name: &str, h: Handle) {
        let members = self.group_mut(h).entities.clone();
        self.set_reactors(&members, h, false);
        let dictionary = self.group_dictionary();
        let doc = &mut self.drawing.doc;
        doc.objects.remove(&h);
        if let Some(Some(ObjectType::Dictionary(d))) = dictionary.map(|d| doc.objects.get_mut(&d)) {
            d.entries.retain(|(k, e)| !(k == name && *e == h));
            d.hard_owner_entries.retain(|k| k != name);
        }
    }
}
