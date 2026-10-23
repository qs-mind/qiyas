//! Editing a generated face resolves to its driver. Legacy command adapters may expand that
//! driver selection while the shared object engine owns regeneration and quantity remapping.
use crate::objects::{instance, JoinRule, ObjectPart, PART_APP};
use crate::{CoreError, Session, Target};
use opencadcodec::Handle;
use std::collections::{BTreeMap, BTreeSet};

impl Session {
    pub(crate) fn refuse_generated_edit(
        &self,
        handles: &[Handle],
        geometry: bool,
    ) -> Result<(), CoreError> {
        for &handle in handles {
            let Some(entity) = self.drawing.doc.get_entity(handle) else {
                continue;
            };
            if let Some(part) = crate::xdata::read_json::<ObjectPart>(entity.common(), PART_APP) {
                return Err(CoreError::InvalidCommand(format!(
                    "Entity {} is the {} of object {}. Change the owner's properties with set_object_props; generated parts cannot be edited directly.",
                    handle.value(), part.role, part.owner)));
            }
            if geometry && (crate::object_regions::retained_region(entity).is_some()) {
                return Err(CoreError::InvalidCommand(format!(
                    "Room boundary {} follows its enclosing walls. Edit the walls, or measure a room at a new point; its vertices cannot be edited directly.", handle.value())));
            }
        }
        Ok(())
    }

    /// Shared union graphics describe all members. Before erasing their current owner,
    /// transfer them to a surviving driver so regeneration can remap their measurements.
    pub(crate) fn transfer_union_parts_before_erase(
        &mut self,
        handles: &[Handle],
    ) -> Result<(), CoreError> {
        let removed: BTreeSet<_> = handles.iter().map(|h| h.value()).collect();
        let mut owners = BTreeMap::new();
        for handle in handles {
            let Some(object) = self
                .drawing
                .doc
                .get_entity(*handle)
                .and_then(instance)
                .filter(|object| object.compatibility.is_none())
            else {
                continue;
            };
            let definition = self.object_type(&object.type_name)?;
            if !matches!(definition.join, JoinRule::Union) {
                continue;
            }
            if let Some(survivor) = self
                .band_members(&object, &definition)?
                .iter()
                .find(|member| !removed.contains(&member.id))
            {
                owners.insert(handle.value(), survivor.id);
            }
        }
        if owners.is_empty() {
            return Ok(());
        }
        let app = crate::xdata::app_handle(&self.drawing.doc, PART_APP);
        for entity in self.drawing.doc.entities_mut() {
            let Some(mut part) = crate::xdata::read_json::<ObjectPart>(entity.common(), PART_APP)
            else {
                continue;
            };
            if !matches!(part.role.as_str(), "outline" | "hatch") {
                continue;
            }
            if let Some(owner) = owners.get(&part.owner) {
                part.owner = *owner;
                crate::xdata::write_json(entity.common_mut(), PART_APP, app, &part)?;
            }
        }
        Ok(())
    }

    pub(crate) fn object_edit_targets(&self, targets: &[Target]) -> Vec<Target> {
        targets
            .iter()
            .map(|target| {
                if !target.chain.is_empty() {
                    return target.clone();
                }
                let owner = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(target.handle))
                    .and_then(|e| crate::xdata::read_json::<ObjectPart>(e.common(), PART_APP))
                    .and_then(|part| self.drawing.doc.get_entity(Handle::new(part.owner)))
                    .and_then(instance)
                    .filter(|object| object.compatibility.is_none());
                match owner {
                    Some(object) => Target {
                        handle: object.id,
                        chain: vec![],
                    },
                    None => target.clone(),
                }
            })
            .collect()
    }
}
