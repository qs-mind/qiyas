//! The properties palette's edits (`Command::SetProperty`) and matching one entity's properties
//! onto others (`Command::MatchProperties`, AutoCAD's MATCHPROP). Only the drawing's own
//! entities at the top of the sheet on screen change; a match may read its source anywhere (inside
//! a block, in an xref). One undo step each. A change of shape (a radius, a closed flag, a width,
//! a block reference's scale or turn) measures the take-off rows on the entity again
//! (`follow_edit`); a dimension or a table draws its block again when its text height or style
//! changes; a row measuring an entity whose layer changes takes the new layer.

use opencadcodec::entities::{CellStyle, Dimension, EntityCommon};
use opencadcodec::types::Transform;
use opencadcodec::{EntityType, Handle, LineWeight, Vector3};
use qiyas_geom::{dimscale, object_to_world, set_dimtxt, DimLook};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::draw::{parse_color, LINEWEIGHTS};
use crate::modify::{measures, place, rotation, Placing};
use crate::session::{locate, variant_name};
use crate::{CoreError, Session, Target};

/// A property the palette edits, and its value: `layer` a layer of the drawing; `color`
/// "ByLayer", "ByBlock", "ACI n" or "#rrggbb"; `linetype` "ByLayer", "ByBlock" or a linetype of
/// the drawing; `lineweight` hundredths of a millimetre (-1 ByLayer, -2 ByBlock, -3 Default);
/// `text` a text's or an MText's text; `text_height` (drawing units, above 0) of a text, an
/// MText, a dimension or a table; `text_rotation` (degrees) of a text or an MText; `text_style`
/// a text style of the drawing; `radius` (above 0) of a circle or an arc; `closed` (true or
/// false) and `global_width` (0 or more) of a polyline; `insert_scale` (the size of its x scale:
/// all three axes scale together about the insertion point, a mirrored reference stays mirrored)
/// and `insert_rotation` (degrees) of a block reference, its attributes following; `dim_style` a dimension style of
/// the drawing; `viewport_scale` (above 0) and `viewport_centre` (`[x, y]` in model space) of a
/// paper-space viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntityProp {
    Layer,
    Color,
    Linetype,
    Lineweight,
    Text,
    TextHeight,
    TextRotation,
    TextStyle,
    Radius,
    Closed,
    GlobalWidth,
    InsertScale,
    InsertRotation,
    DimStyle,
    /// A paper-space viewport's scale: layout units per model unit (above 0).
    ViewportScale,
    /// The model point a paper-space viewport shows at its centre, `[x, y]`.
    ViewportCentre,
}

/// What `Command::MatchProperties` copies. Text style and height go text to text, dimension
/// style dimension to dimension, hatch pattern hatch to hatch; the rest to anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchProp {
    Layer,
    Color,
    Linetype,
    Lineweight,
    TextStyle,
    TextHeight,
    DimStyle,
    HatchPattern,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn number(value: &Value) -> Result<f64, CoreError> {
    value
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(format!("{value} is not a number")))
}

fn positive(value: &Value) -> Result<f64, CoreError> {
    Some(number(value)?)
        .filter(|v| *v > 0.0)
        .ok_or_else(|| invalid("The value must be more than 0"))
}

fn text(value: &Value) -> Result<&str, CoreError> {
    value
        .as_str()
        .ok_or_else(|| invalid(format!("{value} is not text")))
}

/// An xref's layer (`XREF|LAYER`), which no entity of the drawing may take.
fn is_xref_layer(name: &str, xref_dependent: bool) -> bool {
    xref_dependent || name.contains('|')
}

fn xref_layer(name: &str) -> CoreError {
    invalid(format!(
        "Layer {name} belongs to an xref: pick a layer of this drawing"
    ))
}

/// Where a block reference's insertion point is on the plan.
fn insert_at(i: &opencadcodec::entities::Insert) -> [f64; 2] {
    let p = i.insert_point;
    object_to_world(&i.normal, p.z).apply([p.x, p.y])
}

/// What the entity's kind is called in a refusal.
fn no_such(entity: &EntityType, what: &str) -> CoreError {
    invalid(format!("A {} has no {what}", variant_name(entity)))
}

/// Text height of a text or an MText.
fn text_height(e: &EntityType) -> Option<f64> {
    match e {
        EntityType::Text(t) => Some(t.height),
        EntityType::MText(t) => Some(t.height),
        _ => None,
    }
}

/// Text style of a text or an MText.
fn text_style(e: &EntityType) -> Option<&str> {
    match e {
        EntityType::Text(t) => Some(&t.style),
        EntityType::MText(t) => Some(&t.style),
        _ => None,
    }
}

/// What a change to one entity needs afterwards.
#[derive(Default)]
struct After {
    /// Its shape changed: the take-off rows on it are measured again.
    reshaped: bool,
    /// A dimension's or a table's block is drawn again.
    rebuild: bool,
    /// A dimension took another style: its unit suffix follows the new one.
    restyled: bool,
}

impl Session {
    /// `Command::SetProperty`.
    pub(crate) fn set_property(
        &mut self,
        target: &Target,
        prop: EntityProp,
        value: &Value,
    ) -> Result<(), CoreError> {
        let h = self.sheet_targets(std::slice::from_ref(target))?[0];
        if prop == EntityProp::Layer {
            if let Some(object) = crate::objects::instance(self.entity(h)) {
                let definition = self.object_type(&object.type_name)?;
                if let Some(property) = definition
                    .plan
                    .layer_properties
                    .get("centreline")
                    .filter(|_| definition.plan.centreline_driver)
                {
                    return self.set_object_props(
                        &[h.value()],
                        &std::collections::BTreeMap::from([(property.clone(), value.clone())]),
                        None,
                    );
                }
            }
        }
        if prop == EntityProp::Text {
            let e = self.entity(h);
            if !matches!(e, EntityType::Text(_) | EntityType::MText(_)) {
                return Err(no_such(e, "text"));
            }
            return self.edit_text(target, text(value)?, None);
        }
        let mut e = self.entity(h).clone();
        let after = self.apply_property(&mut e, prop, value)?;
        self.store(h, e, after)
    }

    /// Puts `e` back as `h` when it changed, then does what `after` asks.
    fn store(&mut self, h: Handle, e: EntityType, after: After) -> Result<(), CoreError> {
        if e == *self.entity(h) {
            return Ok(());
        }
        let layer = e.common().layer.clone();
        *self.drawing.doc.get_entity_mut(h).expect("a sheet target") = e;
        for m in &mut self.takeoff.items {
            if m.chain.is_empty() && measures(m, h.value()) {
                m.layer.clone_from(&layer);
            }
        }
        if after.restyled {
            self.restyled_dimension(h);
        }
        if after.rebuild {
            match self.entity(h) {
                EntityType::Dimension(_) => self.rebuild_dimension(h)?,
                EntityType::Table(_) => self.rebuild_table(h)?,
                _ => {}
            }
        }
        if after.reshaped {
            self.follow_edit(&[h], &[], &[], &[]);
        }
        Ok(())
    }

    /// `prop` of `e` takes `value`, checked against the drawing.
    fn apply_property(
        &self,
        e: &mut EntityType,
        prop: EntityProp,
        value: &Value,
    ) -> Result<After, CoreError> {
        let doc = &self.drawing.doc;
        let mut after = After::default();
        match prop {
            EntityProp::Layer => {
                let name = text(value)?.trim();
                let layer = doc
                    .layers
                    .get(name)
                    .ok_or_else(|| invalid(format!("The drawing has no layer {name}")))?;
                if is_xref_layer(&layer.name, layer.flags.xref_dependent) {
                    return Err(xref_layer(&layer.name));
                }
                e.common_mut().layer.clone_from(&layer.name);
            }
            EntityProp::Color => {
                let c = text(value)?;
                let color =
                    parse_color(c).ok_or_else(|| invalid(format!("{c} is not a colour")))?;
                let common = e.common_mut();
                common.color = color;
                common.color_name = None;
                common.color_book_handle = None;
            }
            EntityProp::Linetype => {
                let lt = text(value)?.trim();
                let name = if lt.eq_ignore_ascii_case("ByLayer") {
                    String::new()
                } else if lt.eq_ignore_ascii_case("ByBlock") {
                    "ByBlock".to_string()
                } else {
                    doc.line_types
                        .get(lt)
                        .map(|t| t.name.clone())
                        .ok_or_else(|| invalid(format!("The drawing has no linetype {lt}")))?
                };
                let common = e.common_mut();
                common.linetype = name;
                common.linetype_handle = None;
            }
            EntityProp::Lineweight => {
                let w = value
                    .as_i64()
                    .and_then(|w| i16::try_from(w).ok())
                    .filter(|w| LINEWEIGHTS.contains(w) || matches!(w, -3..=-1))
                    .ok_or_else(|| invalid(format!("{value} is not a lineweight")))?;
                e.common_mut().line_weight = LineWeight::from_value(w);
            }
            EntityProp::Text => unreachable!("set_property edits text itself"),
            EntityProp::TextHeight => {
                let h = positive(value)?;
                match e {
                    EntityType::Text(t) => t.height = h,
                    EntityType::MText(t) => t.height = h,
                    EntityType::Dimension(d) => {
                        // The override is before DIMSCALE, as AutoCAD keeps DIMTXT.
                        let style = qiyas_geom::style_named(doc, &d.base().style_name);
                        let k = dimscale(&d.base().common).unwrap_or(style.dimscale);
                        let k = if k > 0.0 && k.is_finite() { k } else { 1.0 };
                        set_dimtxt(&mut d.base_mut().common, h / k);
                        after.rebuild = true;
                    }
                    EntityType::Table(t) => {
                        let base = t.base_style.clone().unwrap_or_else(CellStyle::new);
                        for cell in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                            cell.style.get_or_insert_with(|| base.clone()).text_height = h;
                        }
                        after.rebuild = true;
                    }
                    other => return Err(no_such(other, "text height")),
                }
            }
            EntityProp::TextRotation => {
                let a = number(value)?.to_radians();
                match e {
                    EntityType::Text(t) => t.rotation = a,
                    EntityType::MText(t) => t.rotation = a,
                    other => return Err(no_such(other, "text rotation")),
                }
            }
            EntityProp::TextStyle => {
                let name = text(value)?.trim();
                let style = doc
                    .text_styles
                    .get(name)
                    .map(|s| s.name.clone())
                    .ok_or_else(|| invalid(format!("The drawing has no text style {name}")))?;
                match e {
                    EntityType::Text(t) => t.style = style,
                    EntityType::MText(t) => t.style = style,
                    other => return Err(no_such(other, "text style")),
                }
            }
            EntityProp::Radius => {
                let r = positive(value)?;
                match e {
                    EntityType::Circle(c) => c.radius = r,
                    EntityType::Arc(a) => a.radius = r,
                    other => return Err(no_such(other, "radius")),
                }
                after.reshaped = true;
            }
            EntityProp::Closed => {
                let closed = value
                    .as_bool()
                    .ok_or_else(|| invalid(format!("{value} is not true or false")))?;
                match e {
                    EntityType::LwPolyline(p) => p.is_closed = closed,
                    other => return Err(no_such(other, "closed flag")),
                }
                after.reshaped = true;
            }
            EntityProp::GlobalWidth => {
                let w = number(value)?;
                if w < 0.0 {
                    return Err(invalid("A width can't be below 0"));
                }
                match e {
                    EntityType::LwPolyline(p) => {
                        p.constant_width = w;
                        for v in &mut p.vertices {
                            v.start_width = 0.0;
                            v.end_width = 0.0;
                        }
                    }
                    other => return Err(no_such(other, "global width")),
                }
                after.reshaped = true;
            }
            // Scaled and turned about its insertion point as a whole, so its attributes follow.
            EntityProp::InsertScale => {
                let s = number(value)?.abs();
                let EntityType::Insert(i) = &*e else {
                    return Err(no_such(e, "scale"));
                };
                let now = i.x_scale().abs();
                if s.is_nan() || now.is_nan() || s <= 0.0 || now <= 0.0 {
                    return Err(invalid("The scale must be more than 0"));
                }
                let k = s / now;
                let (x, y, z) = (i.x_scale(), i.y_scale(), i.z_scale());
                let t = Transform::from_scaling_with_origin(
                    Vector3::new(k, k, k),
                    Vector3::new(insert_at(i)[0], insert_at(i)[1], 0.0),
                );
                place(e, &Placing::plain(t));
                // Exactly as typed, its mirror kept.
                if let EntityType::Insert(i) = e {
                    i.set_x_scale(x.signum() * s);
                    i.set_y_scale(y * k);
                    i.set_z_scale(z * k);
                }
                after.reshaped = true;
            }
            EntityProp::InsertRotation => {
                let a = number(value)?.to_radians();
                let EntityType::Insert(i) = &*e else {
                    return Err(no_such(e, "rotation"));
                };
                // Its angle is in its own (OCS) plane: seen from below (a -Z normal) a turn of
                // its angle is the opposite turn on the plan.
                let turn = (a - i.rotation) * if i.normal.z < 0.0 { -1.0 } else { 1.0 };
                let (x, y, z) = (i.x_scale(), i.y_scale(), i.z_scale());
                place(e, &Placing::plain(rotation(insert_at(i), turn)));
                // A turn keeps the scales; exactly as typed when the turn landed there.
                if let EntityType::Insert(i) = e {
                    if (i.x_scale() - x).abs() < 1e-9 * x.abs().max(1.0)
                        && (i.y_scale() - y).abs() < 1e-9 * y.abs().max(1.0)
                    {
                        i.set_x_scale(x);
                        i.set_y_scale(y);
                        i.set_z_scale(z);
                    }
                    let off = (i.rotation - a).rem_euclid(std::f64::consts::TAU);
                    if off < 1e-9 || std::f64::consts::TAU - off < 1e-9 {
                        i.rotation = a;
                    }
                }
                after.reshaped = true;
            }
            EntityProp::ViewportScale => {
                let scale = positive(value)?;
                match e {
                    EntityType::Viewport(v) if v.id != 1 => v.set_scale(scale),
                    other => return Err(no_such(other, "viewport scale")),
                }
            }
            EntityProp::ViewportCentre => {
                let at = value
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
                    .filter(|p| p.iter().all(|v| v.is_finite()))
                    .ok_or_else(|| invalid(format!("{value} is not a point [x, y]")))?;
                match e {
                    EntityType::Viewport(v) if v.id != 1 => {
                        v.view_center =
                            Vector3::new(at[0] - v.view_target.x, at[1] - v.view_target.y, 0.0);
                    }
                    other => return Err(no_such(other, "viewport centre")),
                }
            }
            EntityProp::DimStyle => {
                let name = text(value)?.trim();
                let style = doc
                    .dim_styles
                    .get(name)
                    .map(|s| s.name.clone())
                    .ok_or_else(|| invalid(format!("The drawing has no dimension style {name}")))?;
                match e {
                    EntityType::Dimension(d) => d.base_mut().style_name = style,
                    other => return Err(no_such(other, "dimension style")),
                }
                after.rebuild = true;
                after.restyled = true;
            }
        }
        Ok(after)
    }

    /// `Command::MatchProperties`: each of `targets` takes `props` of `source` where both have
    /// them.
    pub(crate) fn match_properties(
        &mut self,
        source: &Target,
        targets: &[Target],
        props: &[MatchProp],
    ) -> Result<(), CoreError> {
        let handles = self.sheet_targets(targets)?;
        self.refuse_generated_edit(&handles, false)?;
        let xrefs = self.xref_docs();
        let located = locate(&self.drawing.doc, &xrefs, source.handle, &source.chain)?;
        let from = located.entity.clone();
        // Layer "0" inside a block is drawn on its reference's: that is the layer it shows.
        let own = self.drawing.doc.layers.get(&located.layer);
        if props.contains(&MatchProp::Layer)
            && (located.xref.is_some()
                || own.is_some_and(|l| is_xref_layer(&l.name, l.flags.xref_dependent)))
        {
            return Err(xref_layer(&located.layer));
        }
        let layer = own.map(|l| l.name.clone());
        let source_doc_is_host = located.xref.is_none();
        drop(xrefs);
        let doc = &self.drawing.doc;
        let linetype = {
            let lt = &from.common().linetype;
            let ok = lt.is_empty()
                || lt.eq_ignore_ascii_case("ByBlock")
                || lt.eq_ignore_ascii_case("ByLayer")
                || doc.line_types.contains(lt);
            ok.then(|| lt.clone())
        };
        let style = text_style(&from)
            .filter(|s| doc.text_styles.contains(s))
            .map(str::to_string);
        let dim_style = match &from {
            EntityType::Dimension(d) if doc.dim_styles.contains(&d.base().style_name) => {
                Some(d.base().style_name.clone())
            }
            _ => None,
        };
        for h in handles {
            if source_doc_is_host && source.chain.is_empty() && h.value() == source.handle {
                continue;
            }
            let mut e = self.entity(h).clone();
            let mut after = After::default();
            for prop in props {
                let common: &mut EntityCommon = e.common_mut();
                let theirs = from.common();
                match prop {
                    MatchProp::Layer => {
                        if let Some(layer) = &layer {
                            common.layer.clone_from(layer);
                        }
                    }
                    MatchProp::Color => {
                        common.color = theirs.color;
                        common.color_name.clone_from(&theirs.color_name);
                        common.color_book_handle = None;
                    }
                    MatchProp::Linetype => {
                        if let Some(lt) = &linetype {
                            common.linetype.clone_from(lt);
                            common.linetype_handle = None;
                        }
                    }
                    MatchProp::Lineweight => common.line_weight = theirs.line_weight,
                    MatchProp::TextStyle => match (&mut e, &style) {
                        (EntityType::Text(t), Some(s)) => t.style.clone_from(s),
                        (EntityType::MText(t), Some(s)) => t.style.clone_from(s),
                        _ => {}
                    },
                    MatchProp::TextHeight => match (&mut e, text_height(&from)) {
                        (EntityType::Text(t), Some(height)) => t.height = height,
                        (EntityType::MText(t), Some(height)) => t.height = height,
                        _ => {}
                    },
                    MatchProp::DimStyle => {
                        if let (EntityType::Dimension(d), Some(s)) = (&mut e, &dim_style) {
                            d.base_mut().style_name.clone_from(s);
                            after.rebuild = true;
                            after.restyled = true;
                        }
                    }
                    MatchProp::HatchPattern => {
                        if let (EntityType::Hatch(to), EntityType::Hatch(theirs)) = (&mut e, &from)
                        {
                            to.pattern = theirs.pattern.clone();
                            to.is_solid = theirs.is_solid;
                            to.pattern_type = theirs.pattern_type;
                            to.pattern_angle = theirs.pattern_angle;
                            to.pattern_scale = theirs.pattern_scale;
                            to.is_double = theirs.is_double;
                            to.gradient_color = theirs.gradient_color.clone();
                        }
                    }
                }
            }
            self.store(h, e, after)?;
        }
        Ok(())
    }
}

/// Text height a dimension draws with, for the palette.
pub(crate) fn dimension_text_height(doc: &opencadcodec::CadDocument, d: &Dimension) -> f64 {
    DimLook::of(doc, d).text_height
}
