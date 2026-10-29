//! Linked labels for any object property, including door and window marks.
use crate::{CoreError, Session, Target};
use opencadcodec::{EntityType, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const APP: &str = "QIYAS_OBJECT_TAG";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagShape {
    Circle,
    Hexagon,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TagOrder {
    /// Given target order; category selections use ascending object handle.
    /// ترتيب الاختيار، أو ترتيب معرفات العناصر تصاعديا عند اختيار فئة.
    #[default]
    Selection,
    LeftToRight,
    TopToBottom,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TagOptions {
    /// Symbol by opening kind or object category, e.g. {"door":"circle","window":"hexagon"}.
    /// Defaults: doors circle, windows hexagon. Other property labels remain plain text.
    /// شكل الرمز حسب النوع أو الفئة؛ افتراضيا دائرة للأبواب وسداسي للنوافذ.
    pub shapes: BTreeMap<String, TagShape>,
    /// Missing opening marks receive D01, W01 in this order, skipping existing marks.
    /// Selection means target order, or ascending handles for a category selection.
    /// ترقيم الرموز الناقصة بهذا الترتيب مع تجاوز الرموز الموجودة.
    pub order: TagOrder,
}

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
    #[serde(default)]
    symbol: Option<u64>,
    #[serde(default)]
    shape: Option<TagShape>,
    #[serde(default)]
    outline: bool,
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

/// Tags use explicit object properties, never the schedule's legacy size fallback.
fn shown(o: &crate::objects::ObjectRecord, property: &str) -> Option<String> {
    value(o, property)
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
        options: &TagOptions,
    ) -> Result<Vec<u64>, CoreError> {
        if self
            .drawing
            .doc
            .layers
            .get(&self.current.layer)
            .is_some_and(|l| l.flags.locked)
        {
            return Err(invalid("The tag layer is locked / طبقة الرموز مقفلة"));
        }
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
        let mut handles = self.sheet_targets(&targets)?;
        match options.order {
            TagOrder::Selection if category.is_some() => handles.sort_by_key(|h| h.value()),
            TagOrder::Selection => {}
            order => handles.sort_by(|a, b| {
                let a = self
                    .drawing
                    .doc
                    .get_entity(*a)
                    .and_then(anchor)
                    .unwrap_or([0.0; 2]);
                let b = self
                    .drawing
                    .doc
                    .get_entity(*b)
                    .and_then(anchor)
                    .unwrap_or([0.0; 2]);
                match order {
                    TagOrder::LeftToRight => {
                        a[0].total_cmp(&b[0]).then_with(|| b[1].total_cmp(&a[1]))
                    }
                    _ => b[1].total_cmp(&a[1]).then_with(|| a[0].total_cmp(&b[0])),
                }
            }),
        }
        if handles.is_empty() {
            return Err(invalid("Choose objects to label"));
        }
        let mut objects =
            self.objects_for(&handles.iter().map(|h| h.value()).collect::<Vec<_>>())?;
        if property == "mark" {
            let mut used: BTreeSet<String> = self
                .objects(&crate::objects::ObjectFilter::default())?
                .iter()
                .filter_map(|o| value(o, "mark"))
                .collect();
            for handle in &handles {
                let o = objects
                    .iter()
                    .find(|o| o.instance.id == handle.value())
                    .ok_or_else(|| invalid("Choose an object / اختر عنصرا"))?;
                let entity = self.drawing.doc.get_entity(*handle).expect("object source");
                let legacy_size = entity
                    .common()
                    .extended_data
                    .get_record(crate::opening_schedule::ID_SOURCE_APP)
                    .is_none()
                    && crate::opening_schedule::mark(entity)
                        .is_some_and(|mark| value(o, "mark").as_deref() == Some(mark.as_str()));
                if !o.category.eq_ignore_ascii_case("opening")
                    || (value(o, "mark").is_some() && !legacy_size)
                {
                    continue;
                }
                let kind = value(o, "kind").unwrap_or_default();
                let prefix = match kind.as_str() {
                    "door" => "D",
                    "window" => "W",
                    _ => {
                        return Err(invalid(
                            "Give a mark for this opening kind / حدد رمز هذا النوع من الفتحات",
                        ))
                    }
                };
                let mut number = 1;
                let mark = loop {
                    let candidate = format!("{prefix}{number:02}");
                    if used.insert(candidate.clone()) {
                        break candidate;
                    }
                    number += 1;
                };
                self.set_object_props(&[handle.value()], &BTreeMap::from([("mark".into(), serde_json::json!(mark))]),
                    None, &BTreeMap::new(), crate::measurement::PropSource {
                        kind: crate::measurement::SourceKind::Derived,
                        text: match options.order {
                            TagOrder::Selection => "Numbered in selection order (category: ascending handles) / ترقيم حسب ترتيب الاختيار (الفئة: ترتيب معرفات العناصر)".into(),
                            TagOrder::LeftToRight => "Numbered left to right / ترقيم من اليسار إلى اليمين".into(),
                            TagOrder::TopToBottom => "Numbered top to bottom / ترقيم من أعلى إلى أسفل".into(),
                        },
                        ..Default::default()
                    })?;
            }
            objects = self.objects_for(&handles.iter().map(|h| h.value()).collect::<Vec<_>>())?;
        }
        let height = height.unwrap_or_else(|| self.default_text_height());
        let mut added = Vec::new();
        let app = crate::xdata::register(&mut self.drawing.doc, APP);
        for handle in handles {
            let o = objects
                .iter()
                .find(|o| o.instance.id == handle.value())
                .ok_or_else(|| invalid("Choose an object with properties"))?;
            let text = shown(o, property).ok_or_else(|| {
                invalid(format!("The object has no text or number in {property}"))
            })?;
            let at = anchor(self.drawing.doc.get_entity(handle).expect("checked object"))
                .ok_or_else(|| invalid("This object has no label anchor"))?;
            let kind = value(o, "kind").unwrap_or_default();
            let shape = options
                .shapes
                .get(&kind)
                .or_else(|| options.shapes.get(&o.category))
                .copied()
                .or(match (o.category.as_str(), property, kind.as_str()) {
                    ("opening", "mark", "door") => Some(TagShape::Circle),
                    ("opening", "mark", "window") => Some(TagShape::Hexagon),
                    _ => None,
                });
            if shape.is_some() && text.contains(['\n', '\r']) {
                return Err(invalid(
                    "A symbol mark must fit one line / رمز العنصر في سطر واحد",
                ));
            }
            let existing = self.drawing.doc.entities().find_map(|e| {
                let link = crate::xdata::read_json::<Link>(e.common(), APP)?;
                (!link.outline && link.identity == o.identity && link.property == property)
                    .then_some((e.common().handle.value(), link))
            });
            if let Some((h, link)) = &existing {
                if shape.is_none()
                    || (link.shape == shape
                        && link
                            .symbol
                            .is_some_and(|h| self.drawing.doc.get_entity(Handle::new(h)).is_some()))
                {
                    added.push(*h);
                    continue;
                }
                self.sheet_targets(&[Target {
                    handle: *h,
                    chain: vec![],
                }])?;
                if let Some(symbol) = link.symbol {
                    self.sheet_targets(&[Target {
                        handle: symbol,
                        chain: vec![],
                    }])?;
                    self.drawing.doc.remove_entity(Handle::new(symbol));
                }
            }
            let h = if let Some((h, _)) = existing {
                h
            } else {
                self.add_text(at, height, 0.0, text.clone(), None)?
            };
            let symbol = if let Some(shape) = shape {
                // Reserve the whole symbol, measured from the rendered mark, so neither its
                // outline nor its text can overlap an opening or previously placed label.
                let ink = self.sheet_ink(&self.sheet);
                let bounds =
                    crate::ink::union(ink.pieces.iter().filter(|p| p.owner == h).map(|p| p.bounds))
                        .ok_or_else(|| {
                            invalid("The tag has no text bounds / لا توجد حدود لنص الرمز")
                        })?;
                let radius = ((bounds[1][0] - bounds[0][0]) / 2.0)
                    .hypot((bounds[1][1] - bounds[0][1]) / 2.0)
                    + self.paper_distance(1.0);
                let radius = match shape {
                    TagShape::Circle => radius,
                    TagShape::Hexagon => radius / (std::f64::consts::PI / 6.0).cos(),
                };
                let space = self.free_space(&crate::placement::SpaceQuery {
                    size: Some([2.0 * radius, 2.0 * radius]),
                    near: Some([at[0] - radius, at[1] - self.paper_distance(7.5)]),
                    clearance_mm: Some(1.0),
                    ..Default::default()
                })?;
                let position = space
                    .positions
                    .first()
                    .ok_or_else(|| invalid("No clear tag position / لا يوجد موضع خال للرمز"))?;
                let centre = [position.at[0] + radius, position.at[1] - radius];
                if let Some(EntityType::Text(t)) = self.drawing.doc.get_entity_mut(Handle::new(h)) {
                    t.value = text.clone();
                    t.insertion_point = opencadcodec::Vector3::new(centre[0], centre[1], 0.0);
                    t.alignment_point = Some(t.insertion_point);
                    t.horizontal_alignment =
                        opencadcodec::entities::TextHorizontalAlignment::Center;
                    t.vertical_alignment = opencadcodec::entities::TextVerticalAlignment::Middle;
                    t.common.graphic_data = None;
                }
                let spec = match shape {
                    TagShape::Circle => crate::DrawSpec::Circle { centre, radius },
                    TagShape::Hexagon => crate::DrawSpec::Polygon {
                        centre,
                        radius,
                        sides: 6,
                        inscribed: true,
                    },
                };
                // A circumscribed hexagon must fit the same reserved square.
                let entities = self.draw_entities(spec)?;
                Some(self.add_styled(entities)?[0])
            } else {
                self.auto_place(
                    &[Target {
                        handle: h,
                        chain: vec![],
                    }],
                    Some([at[0], at[1] - self.paper_distance(7.5)]),
                    crate::placement::Side::Any,
                    Some(1.0),
                )?;
                None
            };
            let link = Link {
                identity: o.identity.clone(),
                source: o.instance.id,
                property: property.into(),
                last_anchor: at,
                last_value: text,
                symbol,
                shape,
                outline: false,
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
            if let Some(symbol) = symbol {
                let mut outline = link.clone();
                outline.outline = true;
                outline.symbol = None;
                let layer = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(h))
                    .expect("tag text")
                    .common()
                    .layer
                    .clone();
                let entity = self
                    .drawing
                    .doc
                    .get_entity_mut(Handle::new(symbol))
                    .expect("tag outline");
                entity.common_mut().layer = layer;
                crate::xdata::write_json(entity.common_mut(), APP, app, &outline)?;
            }
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
                if replacements.len() != 1 || replacements[0] == link.source {
                    return None;
                }
                link.source = replacements[0];
                link.identity = self.revision_identity(link.source);
                Some((e.common().handle, link))
            })
            .collect();
        for (h, link) in links {
            if self.is_locked(h) {
                return Err(invalid("A linked tag is locked / رمز مرتبط مقفل"));
            }
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
                crate::xdata::read_json::<Link>(e.common(), APP)
                    .filter(|l| !l.outline)
                    .map(|l| (e.common().handle, l))
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
            let next = shown(o, &link.property).unwrap_or_else(|| "?".into());
            let delta = [at[0] - link.last_anchor[0], at[1] - link.last_anchor[1]];
            if self.is_locked(h)
                || link
                    .symbol
                    .is_some_and(|id| self.is_locked(Handle::new(id)))
            {
                if next != link.last_value || delta != [0.0, 0.0] {
                    return Err(invalid("A linked tag is locked / رمز مرتبط مقفل"));
                }
                continue;
            }
            let Some(EntityType::Text(t)) = self.drawing.doc.get_entity_mut(h) else {
                continue;
            };
            if next != link.last_value {
                t.value = next.clone();
            }
            t.insertion_point.x += delta[0];
            t.insertion_point.y += delta[1];
            if let Some(p) = &mut t.alignment_point {
                p.x += delta[0];
                p.y += delta[1];
            }
            if next != link.last_value || delta != [0.0, 0.0] {
                t.common.graphic_data = None;
            }
            let centre = t.alignment_point.unwrap_or(t.insertion_point);
            let resized = next != link.last_value;
            link.last_anchor = at;
            link.last_value = next;
            crate::xdata::write_json(&mut t.common, APP, app, &link)?;
            let radius = if resized && link.symbol.is_some() {
                let ink = self.sheet_ink(&o.sheet);
                crate::ink::union(
                    ink.pieces
                        .iter()
                        .filter(|p| p.owner == h.value())
                        .map(|p| p.bounds),
                )
                .map(|b| {
                    ((b[1][0] - b[0][0]) / 2.0).hypot((b[1][1] - b[0][1]) / 2.0)
                        + self.paper_distance(1.0)
                })
            } else {
                None
            };
            if let Some(symbol) = link
                .symbol
                .and_then(|h| self.drawing.doc.get_entity_mut(Handle::new(h)))
            {
                match symbol {
                    EntityType::Circle(c) => {
                        c.center = centre;
                        if let Some(radius) = radius {
                            c.radius = radius;
                        }
                    }
                    EntityType::LwPolyline(p) if !p.vertices.is_empty() => {
                        let count = p.vertices.len() as f64;
                        let x = p.vertices.iter().map(|v| v.location.x).sum::<f64>() / count;
                        let y = p.vertices.iter().map(|v| v.location.y).sum::<f64>() / count;
                        let old_radius =
                            (p.vertices[0].location.x - x).hypot(p.vertices[0].location.y - y);
                        let factor = radius
                            .filter(|_| old_radius > 0.0)
                            .map_or(1.0, |r| r / (std::f64::consts::PI / 6.0).cos() / old_radius);
                        for v in &mut p.vertices {
                            v.location.x = centre.x + (v.location.x - x) * factor;
                            v.location.y = centre.y + (v.location.y - y) * factor;
                        }
                    }
                    _ => {}
                }
                symbol.common_mut().graphic_data = None;
            }
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
            if link.outline {
                continue;
            }
            if !active(e.common().handle.value()) && !active(link.source) {
                continue;
            }
            let expected = self
                .tag_source(&link)
                .and_then(|id| self.object_quantity(id).ok().flatten())
                .filter(|o| o.identity == link.identity)
                .and_then(|o| shown(&o, &link.property));
            let actual = match e {
                EntityType::Text(t) => Some(t.value.clone()),
                _ => None,
            };
            let missing_symbol = link.shape.is_some()
                && link
                    .symbol
                    .is_none_or(|h| self.drawing.doc.get_entity(Handle::new(h)).is_none());
            if expected.is_none() || actual != expected || missing_symbol {
                push(crate::check::Finding {
                    rule: "object_tag_mismatch",
                    pack: "general",
                    severity: crate::check::Severity::Warning,
                    handles: vec![e.common().handle.value()],
                    at: None,
                    message: "A linked tag has lost its object or symbol, or differs from its property / فقد الرمز المرتبط عنصره أو شكله، أو يختلف عن خاصيته"
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
            .filter(|link| !link.outline && link.property == "mark")
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
