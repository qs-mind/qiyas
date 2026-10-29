//! Paper layouts, their physical sizes and Qiyas's simple sheet frame and title block.

use std::collections::BTreeMap;

use opencadcodec::entities::AttachmentPoint;
use opencadcodec::objects::{Layout, ObjectType};
use opencadcodec::{CadDocument, EntityType, MText, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::text::{escape_mtext, mark_logical};
use crate::{CoreError, Current, DrawSpec, Session, Units};

pub use crate::viewports::ViewportScale;

/// A paper size in millimetres. Catalog sizes are portrait; a layout's size includes its rotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Paper {
    pub name: String,
    pub w_mm: f64,
    pub h_mm: f64,
}

/// The built-in paper sizes, portrait: ISO A0 to A4, ANSI A to E and ARCH A to E1.
pub fn paper_catalog() -> Vec<Paper> {
    [
        ("ISO A0", 841.0, 1189.0),
        ("ISO A1", 594.0, 841.0),
        ("ISO A2", 420.0, 594.0),
        ("ISO A3", 297.0, 420.0),
        ("ISO A4", 210.0, 297.0),
        ("ANSI A", 215.9, 279.4),
        ("ANSI B", 279.4, 431.8),
        ("ANSI C", 431.8, 558.8),
        ("ANSI D", 558.8, 863.6),
        ("ANSI E", 863.6, 1117.6),
        ("ARCH A", 228.6, 304.8),
        ("ARCH B", 304.8, 457.2),
        ("ARCH C", 457.2, 609.6),
        ("ARCH D", 609.6, 914.4),
        ("ARCH E1", 762.0, 1066.8),
        ("ARCH E", 914.4, 1219.2),
    ]
    .into_iter()
    .map(|(name, w_mm, h_mm)| Paper {
        name: name.to_string(),
        w_mm,
        h_mm,
    })
    .collect()
}

pub(crate) fn lookup_paper(name: &str) -> Result<Paper, CoreError> {
    let name = name.trim();
    paper_catalog()
        .into_iter()
        .find(|paper| {
            paper.name.eq_ignore_ascii_case(name)
                || paper
                    .name
                    .strip_prefix("ISO ")
                    .is_some_and(|iso| iso.eq_ignore_ascii_case(name))
        })
        .ok_or_else(|| invalid(format!("No paper size {name} in the paper catalog")))
}

/// The orientation of a new layout's paper.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayoutOrientation {
    #[default]
    Portrait,
    Landscape,
}

pub(crate) fn default_margin() -> f64 {
    10.0
}

pub(crate) fn default_binding() -> f64 {
    20.0
}

pub(crate) fn default_title_layer() -> String {
    "A-ANNO-TTLB".into()
}

pub(crate) const TITLE_FIELDS: [&str; 14] = [
    "title",
    "project",
    "client",
    "consultant",
    "project_no",
    "drawing_no",
    "scale",
    "date",
    "drawn_by",
    "checked_by",
    "approved_by",
    "revision",
    "status",
    "sheet",
];

/// The rows of a captioned title block (`make_sheet`), top to bottom.
pub(crate) const SHEET_FIELDS: [&str; 14] = [
    "project",
    "title",
    "client",
    "consultant",
    "project_no",
    "drawing_no",
    "scale",
    "date",
    "drawn_by",
    "checked_by",
    "approved_by",
    "revision",
    "status",
    "sheet",
];

/// One supplied revision entry. Empty values remain empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetRevision {
    pub rev: String,
    pub date: String,
    pub description: String,
}

pub(crate) fn revision_height(revisions: &[SheetRevision]) -> f64 {
    8.0 + revisions
        .iter()
        .map(|r| revision_row_height(&r.description))
        .sum::<f64>()
        .max(8.0)
}

fn revision_row_height(description: &str) -> f64 {
    // The description column is 110 mm wide, with 3 mm text and 4 mm padding.
    description
        .lines()
        .map(|line| (line.chars().count() as f64 / 55.0).ceil().max(1.0))
        .sum::<f64>()
        .max(1.0)
        * 4.5
        + 4.0
}

/// A title block's field captions in the sheet's language.
pub(crate) fn caption(key: &str, language: crate::rooms::LabelLanguage) -> String {
    use crate::rooms::LabelLanguage::{Ar, En};
    match (key, language) {
        ("client", En) => "Client",
        ("client", Ar) => "العميل",
        ("consultant", En) => "Consultant",
        ("consultant", Ar) => "الاستشاري",
        ("project_no", En) => "Project no.",
        ("project_no", Ar) => "رقم المشروع",
        ("checked_by", En) => "Checked by",
        ("checked_by", Ar) => "دقق بواسطة",
        ("approved_by", En) => "Approved by",
        ("approved_by", Ar) => "اعتمد بواسطة",
        ("description", En) => "Description",
        ("description", Ar) => "الوصف",
        ("project", En) => "Project",
        ("project", Ar) => "المشروع",
        ("title", En) => "Title",
        ("title", Ar) => "عنوان اللوحة",
        ("drawing_no", En) => "Drawing no.",
        ("drawing_no", Ar) => "رقم اللوحة",
        ("scale", En) => "Scale",
        ("scale", Ar) => "مقياس الرسم",
        ("date", En) => "Date",
        ("date", Ar) => "التاريخ",
        ("drawn_by", En) => "Drawn by",
        ("drawn_by", Ar) => "رسم بواسطة",
        ("revision", En) => "Revision",
        ("revision", Ar) => "المراجعة",
        ("status", En) => "Status",
        ("status", Ar) => "الحالة",
        ("sheet", En) => "Sheet",
        ("sheet", Ar) => "اللوحة",
        (other, _) => return other.replace('_', " "),
    }
    .to_string()
}

fn size_layout(layout: &mut Layout, paper: &Paper, orientation: LayoutOrientation) {
    layout.paper_size = paper.name.clone();
    layout.paper_width = paper.w_mm;
    layout.paper_height = paper.h_mm;
    layout.plot_paper_units = 1;
    layout.plot_type = 5;
    layout.plot_rotation = i16::from(orientation == LayoutOrientation::Landscape);
    layout.min_limits = (0.0, 0.0);
    layout.max_limits = match orientation {
        LayoutOrientation::Portrait => (paper.w_mm, paper.h_mm),
        LayoutOrientation::Landscape => (paper.h_mm, paper.w_mm),
    };
    layout.min_extents = (0.0, 0.0, 0.0);
    layout.max_extents = (layout.max_limits.0, layout.max_limits.1, 0.0);
    // The typed plot settings now own these values, including on an imported DXF layout.
    layout.raw_plot_settings_codes = None;
}

/// Apply template paper only when making a drawing; imported layouts remain untouched.
pub(crate) fn template_paper(doc: &mut CadDocument, units: Units) {
    let name = if matches!(units, Units::Inches | Units::Feet) {
        "ANSI B"
    } else {
        "ISO A3"
    };
    let paper = paper_catalog()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap();
    for object in doc.objects.values_mut() {
        if let ObjectType::Layout(layout) = object {
            if !layout.name.eq_ignore_ascii_case("model") {
                size_layout(layout, &paper, LayoutOrientation::Landscape);
            }
        }
    }
}

/// The codec keeps DXF plot settings as text, but parses their padded integers without
/// trimming. Restore the paper units and rotation from those preserved values on open.
pub(crate) fn restore_paper_settings(doc: &mut CadDocument) {
    for object in doc.objects.values_mut() {
        let ObjectType::Layout(layout) = object else {
            continue;
        };
        let Some(codes) = &layout.raw_plot_settings_codes else {
            continue;
        };
        for (code, value) in codes {
            if !matches!(*code, 72 | 73) {
                continue;
            }
            let Ok(value) = value.trim().parse::<i16>() else {
                continue;
            };
            match *code {
                72 if (0..=2).contains(&value) => layout.plot_paper_units = value,
                73 if (0..=3).contains(&value) => layout.plot_rotation = value,
                _ => {}
            }
        }
    }
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn check_name(name: &str) -> Result<&str, CoreError> {
    let name = name.trim();
    if name.is_empty()
        || name.eq_ignore_ascii_case("model")
        || name.chars().any(char::is_control)
        || name.contains(['|', '<', '>', '/', '\\', '"', ':', ';', '?', '*', '=', '`'])
    {
        return Err(invalid(
            "Give the layout a name other than Model, without |<>/\\\":;?*=`",
        ));
    }
    Ok(name)
}

impl Session {
    /// One paper millimetre in the sheet's coordinates, before the model's annotation scale.
    /// Layout coordinates are always paper millimetres.
    pub fn paper_unit(&self) -> f64 {
        if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            0.001 / self.drawing.units.to_metres()
        } else {
            1.0
        }
    }

    fn current_layout(&self) -> Option<&Layout> {
        let block = self.drawing.doc.block_records.get(&self.sheet)?;
        self.drawing.doc.objects.values().find_map(|o| match o {
            ObjectType::Layout(l)
                if l.block_record == block.handle && !l.name.eq_ignore_ascii_case("model") =>
            {
                Some(l)
            }
            _ => None,
        })
    }

    /// The layout on screen's physical paper size, after rotation; none on Model or an unsized sheet.
    pub fn paper(&self) -> Option<Paper> {
        let layout = self.current_layout()?;
        let (mut w_mm, mut h_mm) = (layout.paper_width, layout.paper_height);
        if ![w_mm, h_mm].iter().all(|v| v.is_finite() && *v > 0.0) {
            return None;
        }
        if matches!(layout.plot_rotation, 1 | 3) {
            std::mem::swap(&mut w_mm, &mut h_mm);
        }
        Some(Paper {
            name: layout.paper_size.clone(),
            w_mm,
            h_mm,
        })
    }

    /// Adds a named paper layout using a catalog size. The sheet on screen stays selected.
    pub(crate) fn new_layout(
        &mut self,
        name: &str,
        paper: &str,
        orientation: LayoutOrientation,
    ) -> Result<(), CoreError> {
        let name = check_name(name)?;
        let paper = lookup_paper(paper)?;
        let doc = &mut self.drawing.doc;
        if let Some(old) = doc.objects.values().find_map(|o| match o {
            ObjectType::Layout(layout) if layout.name.eq_ignore_ascii_case(name) => Some(layout),
            _ => None,
        }) {
            if old.paper_width == paper.w_mm
                && old.paper_height == paper.h_mm
                && old.plot_rotation == i16::from(orientation == LayoutOrientation::Landscape)
            {
                self.last_advice.push(serde_json::json!({"code":"already_exists", "message":format!("Layout {name} already exists with this paper size and orientation."), "message_ar":format!("اللوحة {name} موجودة بهذا المقاس والاتجاه.")}));
                return Ok(());
            }
            return Err(invalid(format!("Layout {name} already exists with different paper settings. Use set_layout_paper to change them.")));
        }
        // The codec counts paper blocks to choose the next name; an imported drawing may
        // have gaps, so refuse a collision before the codec allocates anything.
        let count = doc
            .block_records
            .iter()
            .filter(|b| b.is_paper_space())
            .count();
        let block = if count == 0 {
            "*Paper_Space".to_string()
        } else {
            format!("*Paper_Space{}", count - 1)
        };
        if doc.block_records.contains(&block) {
            return Err(invalid(format!(
                "The backing layout block {block} is already taken"
            )));
        }
        let handle = doc.add_layout(name).map_err(|e| invalid(e.to_string()))?;
        let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&handle) else {
            return Err(invalid("The new layout could not be found"));
        };
        size_layout(layout, &paper, orientation);
        Ok(())
    }

    /// Sizes an existing layout without changing the session's sheet or its entities.
    pub(crate) fn set_layout_paper(
        &mut self,
        name: &str,
        paper: &str,
        orientation: LayoutOrientation,
    ) -> Result<(), CoreError> {
        let paper = lookup_paper(paper)?;
        let layout = self
            .drawing
            .doc
            .objects
            .values_mut()
            .find_map(|object| match object {
                ObjectType::Layout(layout)
                    if !layout.name.eq_ignore_ascii_case("model")
                        && layout.name.eq_ignore_ascii_case(name.trim()) =>
                {
                    Some(layout)
                }
                _ => None,
            })
            .ok_or_else(|| {
                invalid(format!(
                    "No paper layout {name}: use new_layout to create one"
                ))
            })?;
        size_layout(layout, &paper, orientation);
        Ok(())
    }

    /// Renames a paper layout and its dictionary entry, preserving its backing block and entities.
    pub(crate) fn rename_layout(&mut self, from: &str, to: &str) -> Result<(), CoreError> {
        let to = check_name(to)?;
        let doc = &mut self.drawing.doc;
        let handle = doc
            .objects
            .iter()
            .find_map(|(h, o)| match o {
                ObjectType::Layout(l)
                    if !l.name.eq_ignore_ascii_case("model")
                        && l.name.eq_ignore_ascii_case(from.trim()) =>
                {
                    Some(*h)
                }
                _ => None,
            })
            .ok_or_else(|| invalid(format!("No paper layout {from} in this drawing")))?;
        if doc.objects.iter().any(|(h, o)| {
            *h != handle && matches!(o, ObjectType::Layout(l) if l.name.eq_ignore_ascii_case(to))
        }) {
            return Err(invalid(format!(
                "The drawing already has a layout named {to}"
            )));
        }
        if let Some(ObjectType::Layout(l)) = doc.objects.get_mut(&handle) {
            l.name = to.to_string();
        }
        if let Some(ObjectType::Dictionary(dict)) =
            doc.objects.get_mut(&doc.header.acad_layout_dict_handle)
        {
            for (name, h) in &mut dict.entries {
                if *h == handle {
                    let old = std::mem::replace(name, to.to_string());
                    for owned in &mut dict.hard_owner_entries {
                        if owned.eq_ignore_ascii_case(&old) {
                            *owned = to.to_string();
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Draws a frame and a title block in paper millimetres on the sized layout on screen.
    /// Only supplied field values become text; omitted fields stay empty.
    pub(crate) fn title_block(
        &mut self,
        fields: &BTreeMap<String, String>,
        margin_mm: f64,
        binding_mm: f64,
        layer: &str,
        revisions: &[SheetRevision],
        language: crate::rooms::LabelLanguage,
    ) -> Result<Vec<u64>, CoreError> {
        let mut handles =
            self.title_block_with(fields, margin_mm, binding_mm, layer, Some(language))?;
        let extra = fields
            .keys()
            .filter(|k| !SHEET_FIELDS.contains(&k.as_str()))
            .count();
        let bottom = margin_mm + (SHEET_FIELDS.len() - 1 + extra) as f64 * 8.0 + 14.0;
        handles.extend(
            self.sheet_revision_table(revisions, language, bottom, margin_mm, binding_mm, layer)?,
        );
        Ok(handles)
    }

    /// The same, with a caption on each row in `captions`' language (`make_sheet`): the value
    /// at 3 mm, the sheet's title at 4 mm, the caption at 2.5 mm; Arabic reads from the right.
    pub(crate) fn title_block_with(
        &mut self,
        fields: &BTreeMap<String, String>,
        margin_mm: f64,
        binding_mm: f64,
        layer: &str,
        captions: Option<crate::rooms::LabelLanguage>,
    ) -> Result<Vec<u64>, CoreError> {
        let paper = self
            .paper()
            .ok_or_else(|| invalid("Title blocks need a sized layout: use new_layout to create one or set_layout_paper to size an existing layout, then set_sheet to select it"))?;
        if ![margin_mm, binding_mm]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
        {
            return Err(invalid(
                "Margins must be finite, non-negative paper millimetres",
            ));
        }
        if fields
            .values()
            .any(|v| v.contains('\0') || v.len() > 64 * 1024)
        {
            return Err(invalid(
                "A title-block value contains a null character or is too long",
            ));
        }
        let order: &[&str] = if captions.is_some() {
            &SHEET_FIELDS
        } else {
            &TITLE_FIELDS
        };
        let mut rows: Vec<&str> = order.to_vec();
        rows.extend(
            fields
                .keys()
                .filter(|k| !order.contains(&k.as_str()))
                .map(String::as_str),
        );
        let heights: Vec<f64> = rows
            .iter()
            .map(|k| {
                if captions.is_some() && *k == "title" {
                    14.0
                } else {
                    8.0
                }
            })
            .collect();
        let (left, bottom) = (margin_mm.max(binding_mm), margin_mm);
        let (right, top) = (paper.w_mm - margin_mm, paper.h_mm - margin_mm);
        let block_h: f64 = heights.iter().sum();
        if right - left < 40.0 || top - bottom < block_h {
            return Err(invalid("The margins and title block do not fit this paper"));
        }
        let block_w = 180.0_f64.min(right - left);
        let block_left = right - block_w;
        let mut entities = self.draw_entities(DrawSpec::Rectangle {
            a: [left, bottom],
            b: [right, top],
        })?;
        // The frame already supplies the block's bottom and right edges.
        if block_left > left {
            entities.extend(self.draw_entities(DrawSpec::Line {
                a: [block_left, bottom],
                b: [block_left, bottom + block_h],
            })?);
        }
        if bottom + block_h < top {
            entities.extend(self.draw_entities(DrawSpec::Line {
                a: [block_left, bottom + block_h],
                b: [right, bottom + block_h],
            })?);
        }
        let style = self.text_style();
        let arabic = captions == Some(crate::rooms::LabelLanguage::Ar);
        // Caption column 40 mm wide; the value beside it. Arabic mirrors both.
        let (caption_w, gap) = (60.0_f64.min(block_w / 3.0), 4.0);
        for (i, key) in rows.into_iter().enumerate() {
            let y = bottom + block_h - heights[..i].iter().sum::<f64>();
            if i > 0 {
                entities.extend(self.draw_entities(DrawSpec::Line {
                    a: [block_left, y],
                    b: [right, y],
                })?);
            }
            let put = |entities: &mut Vec<EntityType>,
                       text: &str,
                       x: f64,
                       width: f64,
                       height: f64,
                       right_aligned: bool| {
                // Keep values literal: title-block dates and names never become generated fields.
                let mut text = MText::with_value(escape_mtext(text), Vector3::new(x, y - 2.0, 0.0))
                    .with_height(height);
                text.rectangle_width = width;
                text.style = style.clone();
                if right_aligned {
                    text.attachment_point = AttachmentPoint::TopRight;
                }
                mark_logical(&mut text.common);
                entities.push(EntityType::MText(text));
            };
            if let Some(language) = captions {
                let name = caption(key, language);
                if arabic {
                    put(
                        &mut entities,
                        &name,
                        right - 2.0,
                        caption_w - 2.0,
                        2.5,
                        true,
                    );
                } else {
                    put(
                        &mut entities,
                        &name,
                        block_left + 2.0,
                        caption_w - 2.0,
                        2.5,
                        false,
                    );
                }
            }
            if let Some(value) = fields.get(key).filter(|v| !v.is_empty()) {
                let height = match (captions.is_some(), key) {
                    (true, "title") => 4.0,
                    (true, _) => 3.0,
                    (false, "title") => 3.5,
                    (false, _) => 2.5,
                };
                if captions.is_some() {
                    let width = block_w - caption_w - gap - 2.0;
                    if arabic {
                        put(
                            &mut entities,
                            value,
                            right - caption_w - gap,
                            width,
                            height,
                            true,
                        );
                    } else {
                        put(
                            &mut entities,
                            value,
                            block_left + caption_w + gap,
                            width,
                            height,
                            false,
                        );
                    }
                } else {
                    put(
                        &mut entities,
                        value,
                        block_left + 2.0,
                        block_w - 4.0,
                        height,
                        false,
                    );
                }
            }
        }
        if !fields.is_empty() || captions.is_some() {
            self.register_text_app();
        }
        self.add_on_annotation_layer(layer, entities, false)
    }

    /// Revision table above the title block, with no fabricated entry or date.
    pub(crate) fn sheet_revision_table(
        &mut self,
        revisions: &[SheetRevision],
        language: crate::rooms::LabelLanguage,
        bottom: f64,
        margin: f64,
        binding: f64,
        layer: &str,
    ) -> Result<Vec<u64>, CoreError> {
        if revisions
            .iter()
            .flat_map(|r| [&r.rev, &r.date, &r.description])
            .any(|v| v.contains('\0') || v.len() > 64 * 1024)
        {
            return Err(invalid(
                "A revision entry contains invalid text / يحتوي سجل المراجعة على نص غير صالح",
            ));
        }
        let paper = self.paper().ok_or_else(|| {
            invalid("A revision table needs paper / جدول المراجعات يحتاج إلى لوحة")
        })?;
        let right = paper.w_mm - margin;
        let left = right - 180.0_f64.min(paper.w_mm - margin - margin.max(binding));
        let top = bottom + revision_height(revisions);
        if top > paper.h_mm - margin {
            return Err(invalid(
                "The revision table exceeds the sheet / جدول المراجعات يتجاوز اللوحة",
            ));
        }
        let arabic = language == crate::rooms::LabelLanguage::Ar;
        // Its bottom edge is already the title block's top edge.
        let mut entities = Vec::new();
        if left > margin.max(binding) {
            entities.extend(self.draw_entities(DrawSpec::Line {
                a: [left, bottom],
                b: [left, top],
            })?);
        }
        entities.extend(self.draw_entities(DrawSpec::Line {
            a: [left, top],
            b: [right, top],
        })?);
        let mut y = top;
        let header = [
            caption("revision", language),
            caption("date", language),
            caption("description", language),
        ];
        let rows = std::iter::once((header, 8.0)).chain(revisions.iter().map(|r| {
            (
                [r.rev.clone(), r.date.clone(), r.description.clone()],
                revision_row_height(&r.description),
            )
        }));
        let widths = [25.0, 45.0, (right - left - 70.0).max(1.0)];
        for (cells, height) in rows {
            if y < top {
                entities.extend(self.draw_entities(DrawSpec::Line {
                    a: [left, y],
                    b: [right, y],
                })?);
            }
            let mut offset = 0.0;
            for (cell, width) in cells.iter().zip(widths) {
                let x = if arabic {
                    right - offset - 2.0
                } else {
                    left + offset + 2.0
                };
                if !cell.trim().is_empty() {
                    let mut text =
                        MText::with_value(escape_mtext(cell), Vector3::new(x, y - 2.0, 0.0))
                            .with_height(3.0);
                    text.rectangle_width = width - 4.0;
                    text.style = self.text_style();
                    if arabic {
                        text.attachment_point = AttachmentPoint::TopRight;
                    }
                    mark_logical(&mut text.common);
                    entities.push(EntityType::MText(text));
                }
                offset += width;
            }
            y -= height;
        }
        if revisions.is_empty() {
            entities.extend(self.draw_entities(DrawSpec::Line {
                a: [left, top - 8.0],
                b: [right, top - 8.0],
            })?);
        }
        for offset in [25.0, 70.0] {
            let x = if arabic {
                right - offset
            } else {
                left + offset
            };
            entities.extend(self.draw_entities(DrawSpec::Line {
                a: [x, bottom],
                b: [x, top],
            })?);
        }
        self.register_text_app();
        self.add_on_annotation_layer(layer, entities, false)
    }

    /// Add generated sheet annotations and their layer in the surrounding command's undo step.
    pub(crate) fn add_on_annotation_layer(
        &mut self,
        name: &str,
        entities: Vec<EntityType>,
        non_plotting: bool,
    ) -> Result<Vec<u64>, CoreError> {
        if !self.drawing.doc.layers.contains(name) {
            self.new_layer(name)?;
        }
        let layer = self
            .drawing
            .doc
            .layers
            .get_mut(name)
            .expect("annotation layer");
        if layer.flags.locked {
            return Err(invalid(format!(
                "Layer {} is locked: unlock it first",
                layer.name
            )));
        }
        if non_plotting {
            layer.is_plottable = false;
        }
        let current = std::mem::replace(
            &mut self.current,
            Current {
                layer: layer.name.clone(),
                ..Current::default()
            },
        );
        let result = self.add_styled(entities);
        self.current = current;
        result
    }
}
