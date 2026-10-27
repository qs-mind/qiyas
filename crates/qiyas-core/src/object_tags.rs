//! Linked labels for any object property, including door and window marks.
use crate::{CoreError, Session, Target};
use opencadcodec::{EntityType, Handle};
use serde::{Deserialize, Serialize};
const APP: &str = "QIYAS_OBJECT_TAG";

pub(crate) fn is_tag(entity: &EntityType) -> bool {
    entity.common().extended_data.get_record(APP).is_some()
}

#[derive(Clone, Serialize, Deserialize)]
struct Link {
    identity: String,
    source: u64,
    property: String,
    last_anchor: [f64; 2],
    last_value: String,
}
pub(crate) fn tag_dependency(entity: &EntityType) -> Option<u64> {
    crate::xdata::read_json::<Link>(entity.common(), APP).map(|link| link.source)
}

fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}
fn anchor(e: &EntityType) -> Option<[f64; 2]> {
    match e {
        EntityType::Insert(i) => Some([i.insert_point.x, i.insert_point.y]),
        EntityType::Point(p) => Some([p.location.x, p.location.y]),
        EntityType::LwPolyline(p) => p.vertices.first().map(|v| [v.location.x, v.location.y]),
        _ => None,
    }
}
fn value(o: &crate::objects::ObjectRecord, property: &str) -> Option<String> {
    o.instance.props.get(property).and_then(|v| match v {
        serde_json::Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// What a tag shows: the object's own property, or for an opening without an explicit ID, the
/// automatic size mark it displays (an explicit ID always wins, as in schedules).
fn shown(
    doc: &opencadcodec::CadDocument,
    o: &crate::objects::ObjectRecord,
    property: &str,
) -> Option<String> {
    value(o, property).or_else(|| {
        (property == "mark")
            .then(|| doc.get_entity(Handle::new(o.instance.id)))
            .flatten()
            .and_then(crate::opening_schedule::mark)
    })
}
impl Session {
    fn tag_source(&self, link: &Link) -> Option<u64> {
        if self
            .drawing
            .doc
            .get_entity(Handle::new(link.source))
            .and_then(crate::objects::instance)
            .is_some()
            && self.revision_identity(link.source) == link.identity
        {
            return Some(link.source);
        }
        // A carried revision can retain an identity under a different local handle.
        self.drawing
            .doc
            .entities()
            .filter_map(crate::objects::instance)
            .find(|object| self.revision_identity(object.id) == link.identity)
            .map(|object| object.id)
    }

    pub(crate) fn object_tags(
        &mut self,
        targets: &[Target],
        property: &str,
        height: Option<f64>,
        category: Option<&str>,
    ) -> Result<Vec<u64>, CoreError> {
        let from_category: Vec<Target>;
        let targets = match category {
            Some(category) if targets.is_empty() => {
                from_category = self
                    .objects(&crate::objects::ObjectFilter {
                        category: Some(category.into()),
                        sheet: Some(self.sheet.clone()),
                        ..Default::default()
                    })?
                    .into_iter()
                    .filter(|o| o.valid)
                    .map(|o| Target {
                        handle: o.instance.id,
                        chain: vec![],
                    })
                    .collect::<Vec<_>>();
                &from_category[..]
            }
            Some(_) => return Err(invalid("Give targets or a category, not both")),
            None => targets,
        };
        let targets = self.object_edit_targets(targets);
        let handles = self.sheet_targets(&targets)?;
        if handles.is_empty() {
            return Err(invalid("Choose objects to label"));
        }
        let mut objects =
            self.objects_for(&handles.iter().map(|h| h.value()).collect::<Vec<_>>())?;
        // Door and window marks are assigned from their size: give them out first, so tagging
        // works before an opening schedule exists.
        if property == "mark"
            && objects.iter().any(|o| {
                o.category.eq_ignore_ascii_case("opening")
                    && o.instance
                        .props
                        .get("mark")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|m| m.trim().is_empty())
            })
        {
            if let Some(block) = self.drawing.doc.block_records.get(&self.sheet) {
                let handle = block.handle;
                self.mark_openings(handle)?;
                objects =
                    self.objects_for(&handles.iter().map(|h| h.value()).collect::<Vec<_>>())?;
            }
        }
        let height = height.unwrap_or_else(|| self.default_text_height());
        let mut added = Vec::new();
        let app = crate::xdata::register(&mut self.drawing.doc, APP);
        for handle in handles {
            let o = objects
                .iter()
                .find(|o| o.instance.id == handle.value())
                .ok_or_else(|| invalid("Choose an object with properties"))?;
            let text = shown(&self.drawing.doc, o, property).ok_or_else(|| {
                invalid(format!("The object has no text or number in {property}"))
            })?;
            let at = anchor(self.drawing.doc.get_entity(handle).expect("checked object"))
                .ok_or_else(|| invalid("This object has no label anchor"))?;
            let existing = self
                .drawing
                .doc
                .entities()
                .find(|e| {
                    crate::xdata::read_json::<Link>(e.common(), APP)
                        .is_some_and(|l| l.identity == o.identity && l.property == property)
                })
                .map(|e| e.common().handle.value());
            if let Some(h) = existing {
                added.push(h);
                continue;
            }
            let h = self.add_text(
                [at[0], at[1] - self.paper_distance(7.5)],
                height,
                0.0,
                text.clone(),
                None,
            )?;
            self.auto_place(
                &[Target {
                    handle: h,
                    chain: vec![],
                }],
                Some([at[0], at[1] - self.paper_distance(7.5)]),
                crate::placement::Side::Any,
                Some(1.0),
            )?;
            let link = Link {
                identity: o.identity.clone(),
                source: o.instance.id,
                property: property.into(),
                last_anchor: at,
                last_value: text,
            };
            crate::xdata::write_json(
                self.drawing
                    .doc
                    .get_entity_mut(Handle::new(h))
                    .expect("new label")
                    .common_mut(),
                APP,
                app,
                &link,
            )?;
            added.push(h);
        }
        Ok(added)
    }
    /// A rebuilt driver may get a new handle. Keep the link in the existing undo step.
    pub(crate) fn remap_object_tags(
        &mut self,
        mapping: &std::collections::BTreeMap<u64, Vec<u64>>,
    ) -> Result<(), CoreError> {
        let app = crate::xdata::app_handle(&self.drawing.doc, APP);
        let links: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                let mut link = crate::xdata::read_json::<Link>(e.common(), APP)?;
                let replacements = mapping.get(&link.source)?;
                // A split or deleted source is ambiguous: keep the broken link for the checker.
                if replacements.len() != 1 {
                    return None;
                }
                link.source = replacements[0];
                link.identity = self.revision_identity(link.source);
                Some((e.common().handle, link))
            })
            .collect();
        for (h, link) in links {
            crate::xdata::write_json(
                self.drawing
                    .doc
                    .get_entity_mut(h)
                    .expect("tag")
                    .common_mut(),
                APP,
                app,
                &link,
            )?;
        }
        Ok(())
    }
    pub(crate) fn refresh_object_tags(&mut self) -> Result<(), CoreError> {
        if !self.drawing.doc.app_ids.contains(APP) {
            return Ok(());
        }
        let app = crate::xdata::app_handle(&self.drawing.doc, APP);
        let links: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| {
                crate::xdata::read_json::<Link>(e.common(), APP).map(|l| (e.common().handle, l))
            })
            .collect();
        let objects = self.objects_for(
            &links
                .iter()
                .filter_map(|(_, link)| self.tag_source(link))
                .collect::<Vec<_>>(),
        )?;
        for (h, mut link) in links {
            let Some(o) = objects.iter().find(|o| o.identity == link.identity) else {
                continue;
            };
            let Some(at) = self
                .drawing
                .doc
                .get_entity(Handle::new(o.instance.id))
                .and_then(anchor)
            else {
                continue;
            };
            let next = shown(&self.drawing.doc, o, &link.property).unwrap_or_else(|| "?".into());
            let delta = [at[0] - link.last_anchor[0], at[1] - link.last_anchor[1]];
            let Some(EntityType::Text(t)) = self.drawing.doc.get_entity_mut(h) else {
                continue;
            };
            if next != link.last_value {
                t.value = next.clone();
            }
            t.insertion_point.x += delta[0];
            t.insertion_point.y += delta[1];
            if next != link.last_value || delta != [0.0, 0.0] {
                t.common.graphic_data = None;
            }
            link.last_anchor = at;
            link.last_value = next;
            crate::xdata::write_json(&mut t.common, APP, app, &link)?;
        }
        Ok(())
    }
    pub(crate) fn check_object_tags(
        &self,
        sheet: &str,
        active: &impl Fn(u64) -> bool,
        push: &mut impl FnMut(crate::check::Finding),
    ) {
        if !self.drawing.doc.app_ids.contains(APP) {
            return;
        }
        if self.validate_object_metadata().is_err() {
            return;
        }
        for e in self.drawing.doc.entities_in_block(sheet) {
            let Some(link) = crate::xdata::read_json::<Link>(e.common(), APP) else {
                continue;
            };
            if !active(e.common().handle.value()) && !active(link.source) {
                continue;
            }
            let expected = self
                .tag_source(&link)
                .and_then(|id| self.object_quantity(id).ok().flatten())
                .filter(|o| o.identity == link.identity)
                .and_then(|o| shown(&self.drawing.doc, &o, &link.property));
            let actual = match e {
                EntityType::Text(t) => Some(t.value.clone()),
                _ => None,
            };
            if expected.is_none() || actual != expected {
                push(crate::check::Finding {
                    rule: "object_tag_mismatch",
                    pack: "general",
                    severity: crate::check::Severity::Warning,
                    handles: vec![e.common().handle.value()],
                    at: None,
                    message: "A linked label has lost its object or differs from its property"
                        .into(),
                });
            }
        }
    }

    /// Doors and windows that no tag shows on the plan, when an opening schedule exists: the
    /// marks in the schedule are what the tags say, so an untagged opening cannot be found in it.
    pub(crate) fn check_opening_tags(
        &self,
        sheet: &str,
        active: &impl Fn(u64) -> bool,
        push: &mut impl FnMut(crate::check::Finding),
    ) {
        if !self.has_opening_schedules() {
            return;
        }
        let Ok(openings) = self.objects(&crate::objects::ObjectFilter {
            category: Some("opening".into()),
            sheet: Some(sheet.into()),
            ..Default::default()
        }) else {
            return;
        };
        let tagged: std::collections::BTreeSet<String> = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| crate::xdata::read_json::<Link>(e.common(), APP))
            .filter(|link| link.property == "mark")
            .map(|link| link.identity)
            .collect();
        let missing: Vec<u64> = openings
            .iter()
            .filter(|o| o.valid && active(o.instance.id) && !tagged.contains(&o.identity))
            .map(|o| o.instance.id)
            .collect();
        if missing.is_empty() {
            return;
        }
        push(crate::check::Finding {
            rule: "opening_untagged",
            pack: "construction",
            severity: crate::check::Severity::Warning,
            message: format!(
                "{} doors or windows have no tag on the plan, so their schedule marks cannot be found: object_tags {{\"category\":\"opening\",\"property\":\"mark\"}} tags them all",
                missing.len()
            ),
            handles: missing.into_iter().take(100).collect(),
            at: None,
        });
    }
}
