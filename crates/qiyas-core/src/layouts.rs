//! Paper layouts, their physical sizes and Qiyas's simple sheet frame and title block.

use std::collections::BTreeMap;

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

pub(crate) const TITLE_FIELDS: [&str; 7] = [
    "title",
    "project",
    "drawing_no",
    "scale",
    "date",
    "drawn_by",
    "sheet",
];

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
        let doc = &mut self.drawing.doc;
        if doc
            .objects
            .values()
            .any(|o| matches!(o, ObjectType::Layout(l) if l.name.eq_ignore_ascii_case(name)))
        {
            return Err(invalid(format!(
                "The drawing already has a layout named {name}"
            )));
        }
        let paper = paper_catalog()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(paper.trim()))
            .ok_or_else(|| invalid(format!("No paper size {paper} in the paper catalog")))?;
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
        let paper = paper_catalog()
            .into_iter()
            .find(|p| p.name.eq_ignore_ascii_case(paper.trim()))
            .ok_or_else(|| invalid(format!("No paper size {paper} in the paper catalog")))?;
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
        let order = TITLE_FIELDS;
        let mut rows: Vec<&str> = order.to_vec();
        rows.extend(
            fields
                .keys()
                .filter(|k| !order.contains(&k.as_str()))
                .map(String::as_str),
        );
        let (left, bottom) = (margin_mm.max(binding_mm), margin_mm);
        let (right, top) = (paper.w_mm - margin_mm, paper.h_mm - margin_mm);
        let row_h = 8.0;
        let block_h = row_h * rows.len() as f64;
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
        for (i, key) in rows.into_iter().enumerate() {
            let y = bottom + block_h - i as f64 * row_h;
            if i > 0 {
                entities.extend(self.draw_entities(DrawSpec::Line {
                    a: [block_left, y],
                    b: [right, y],
                })?);
            }
            if let Some(value) = fields.get(key).filter(|v| !v.is_empty()) {
                // Keep values literal: title-block dates and names never become generated fields.
                let mut text = MText::with_value(
                    escape_mtext(value),
                    Vector3::new(block_left + 2.0, y - 2.0, 0.0),
                )
                .with_height(if key == "title" { 3.5 } else { 2.5 });
                text.rectangle_width = block_w - 4.0;
                text.style = style.clone();
                mark_logical(&mut text.common);
                entities.push(EntityType::MText(text));
            }
        }
        if !fields.is_empty() {
            self.register_text_app();
        }
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
