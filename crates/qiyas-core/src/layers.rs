//! Layers beyond hiding and showing them: the drawing's own layer properties (on, frozen, locked,
//! colour, linetype, lineweight, plot), new, deleted and renamed layers, named layer states kept
//! as AutoCAD keeps them (`ACAD_LAYERSTATES`, so its Layer States Manager sees them), the layer
//! translator (rename, merge and recolour from another drawing's standard), and the layers a
//! paper-space viewport freezes on its own. Each command is one undo step.
//!
//! The layers the user hides (`Session::visibility`) stay a Qiyas view setting; a layer state
//! keeps them too, in the project file (`Project::qiyas_hidden`), so restoring a state brings
//! both back.

use schemars::JsonSchema;
use std::collections::HashSet;
use std::path::Path;

use opencadcodec::tables::{normalize_name, Layer};
use opencadcodec::{CadDocument, Color, EntityType, Handle, LineWeight};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blocks::BAD_NAME_CHARS;
use crate::draw::{parse_color, LINEWEIGHTS};
use crate::{CoreError, Drawing, LayerInfo, Session, Target};

/// A layer property `Command::SetLayer` changes. Its value: `on`, `frozen`, `locked` and `plot`
/// take true or false; `color` "ACI n" (1 to 255) or "#rrggbb"; `linetype` a linetype of the
/// drawing (CENTER and HIDDEN are added when missing); `lineweight` hundredths of a millimetre
/// from AutoCAD's set, or -3 (Default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerProp {
    /// Several layer properties, applied together in one undo step.
    Properties,
    On,
    Frozen,
    Locked,
    Color,
    Linetype,
    Lineweight,
    Plot,
}

/// One line of the layer translator: the drawing's layer `from` becomes `to` (renamed, or merged
/// into `to` when the drawing has it), then takes `color` ("ACI n" or "#rrggbb") when given.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LayerMapping {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// A paper-space viewport on the sheet on screen and the layers it freezes on its own.
#[derive(Debug, Clone, Serialize)]
pub struct ViewportInfo {
    pub handle: u64,
    /// Layer names, sorted.
    pub frozen: Vec<String>,
}

/// The layers of the drawing at `path`, opened read-only (the layer translator's standard).
pub fn drawing_layers(path: &Path) -> Result<Vec<LayerInfo>, CoreError> {
    Ok(Drawing::open(path)?.layers())
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn as_bool(value: &Value) -> Result<bool, CoreError> {
    value
        .as_bool()
        .ok_or_else(|| invalid(format!("{value} is not true or false")))
}

fn as_str(value: &Value) -> Result<&str, CoreError> {
    value
        .as_str()
        .map(str::trim)
        .ok_or_else(|| invalid(format!("{value} is not a name")))
}

/// A colour a layer can take: an index or a true colour, never ByLayer or ByBlock.
pub(crate) fn layer_color(text: &str) -> Result<Color, CoreError> {
    match parse_color(text) {
        Some(c @ (Color::Index(_) | Color::Rgb { .. })) => Ok(c),
        _ => Err(invalid(format!(
            "{text} is not a layer colour: give ACI 1 to 255 or #rrggbb"
        ))),
    }
}

/// A name a new layer (or a renamed one) may take: not empty, at most 255 characters, none of
/// the characters AutoCAD refuses.
fn check_layer_name(name: &str) -> Result<&str, CoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(invalid("Give the layer a name"));
    }
    if name.chars().count() > 255 {
        return Err(invalid("A layer name has at most 255 characters"));
    }
    if let Some(c) = name
        .chars()
        .find(|c| BAD_NAME_CHARS.contains(c) || c.is_control())
    {
        return Err(invalid(format!("A layer name can't hold \"{c}\"")));
    }
    Ok(name)
}

/// The drawing's layer `name` (any case), as it spells it.
fn own_layer(doc: &CadDocument, name: &str) -> Result<String, CoreError> {
    doc.layers
        .get(name.trim())
        .map(|l| l.name.clone())
        .ok_or_else(|| invalid(format!("The drawing has no layer {}", name.trim())))
}

/// Layer "0" and Defpoints are AutoCAD's own: never renamed, merged or deleted.
fn is_reserved(name: &str) -> bool {
    let key = normalize_name(name);
    key == "0" || key == normalize_name("Defpoints")
}

impl Session {
    /// The names of the drawing's layer states, as stored.
    pub(crate) fn layer_state_names(&self) -> Vec<String> {
        self.drawing
            .doc
            .layer_states()
            .into_iter()
            .map(|s| s.name)
            .collect()
    }

    /// Before the first layer state is saved: the layer table gets the extension dictionary the
    /// states live in, outside the undo step, so taking the step back never leaves the table
    /// pointing at a dictionary that is gone. An empty dictionary is harmless to AutoCAD.
    pub(crate) fn prepare_layer_states(&mut self) {
        let doc = &mut self.drawing.doc;
        let table = doc.layers.handle();
        if !table.is_null() {
            doc.ensure_extension_dictionary(table);
        }
    }

    /// `Command::SaveLayerState`: every layer's on, frozen, locked, plot, colour, linetype and
    /// lineweight under `name` (replacing a state of that name), with Qiyas's current layer, and
    /// the hidden layers beside it.
    pub(crate) fn save_layer_state(&mut self, name: &str) -> Result<(), CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("Give the layer state a name"));
        }
        let name = match self.drawing.doc.layer_state(name) {
            Some(old) => old.name,
            None => name.to_string(),
        };
        let doc = &mut self.drawing.doc;
        doc.capture_layer_state(name.clone(), "");
        if let Some(mut state) = doc.layer_state(&name) {
            state.current_layer = self.current.layer.clone();
            doc.store_layer_state(state);
        }
        let hidden = self.hidden_layers();
        self.state_hidden
            .retain(|k, _| !k.eq_ignore_ascii_case(&name));
        self.state_hidden.insert(name, hidden);
        Ok(())
    }

    /// `Command::RestoreLayerState`: the layers take what the state `name` saved, and the hidden
    /// layers come back when the project kept them. Qiyas's current layer stays, and stays
    /// thawed.
    pub(crate) fn restore_layer_state(&mut self, name: &str) -> Result<(), CoreError> {
        let doc = &mut self.drawing.doc;
        let Some(state) = doc.layer_state(name.trim()) else {
            return Err(invalid(format!("No layer state named {}", name.trim())));
        };
        let header = (
            doc.header.current_layer_name.clone(),
            doc.header.current_layer_handle,
        );
        doc.restore_layer_state(&state.name);
        (
            doc.header.current_layer_name,
            doc.header.current_layer_handle,
        ) = header;
        // The current layer is never frozen (AutoCAD refuses it too).
        if let Some(current) = doc.layers.get_mut(&self.current.layer) {
            current.flags.frozen = false;
        }
        let hidden = self
            .state_hidden
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&state.name))
            .map(|(_, v)| v.clone());
        if let Some(hidden) = hidden {
            self.visibility.hidden_layers = hidden.into_iter().collect();
            // The state says what is hidden now: nothing is isolated any more.
            self.isolated = None;
        }
        Ok(())
    }

    /// `Command::DeleteLayerState`.
    pub(crate) fn delete_layer_state(&mut self, name: &str) -> Result<(), CoreError> {
        let name = name.trim();
        if !self.drawing.doc.delete_layer_state(name) {
            return Err(invalid(format!("No layer state named {name}")));
        }
        self.state_hidden
            .retain(|k, _| !k.eq_ignore_ascii_case(name));
        Ok(())
    }

    /// `Command::RenameLayerState`.
    pub(crate) fn rename_layer_state(&mut self, from: &str, to: &str) -> Result<(), CoreError> {
        let (from, to) = (from.trim(), to.trim());
        if to.is_empty() {
            return Err(invalid("Give the layer state a name"));
        }
        let doc = &mut self.drawing.doc;
        let Some(old) = doc.layer_state(from) else {
            return Err(invalid(format!("No layer state named {from}")));
        };
        if let Some(other) = doc
            .layer_state(to)
            .filter(|s| !s.name.eq_ignore_ascii_case(&old.name))
        {
            return Err(invalid(format!(
                "There is a layer state named {} already",
                other.name
            )));
        }
        doc.rename_layer_state(&old.name, to);
        let hidden = self
            .state_hidden
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(&old.name))
            .map(|(k, v)| (k.clone(), v.clone()));
        if let Some((key, list)) = hidden {
            self.state_hidden.remove(&key);
            self.state_hidden.insert(to.to_string(), list);
        }
        Ok(())
    }

    /// `Command::SetLayer`: one property of the drawing's layer `name`.
    pub(crate) fn set_layer_prop(
        &mut self,
        name: &str,
        prop: LayerProp,
        value: &Value,
    ) -> Result<(), CoreError> {
        if prop == LayerProp::Properties {
            let values = value
                .as_object()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| invalid("Give at least one layer property"))?;
            for (key, value) in values {
                let property: LayerProp = serde_json::from_value(Value::String(key.clone()))
                    .map_err(|_| invalid(format!("Unknown layer property {key}; choose color, linetype, lineweight, on, frozen, locked or plot")))?;
                if property == LayerProp::Properties {
                    return Err(invalid("Layer properties cannot be nested"));
                }
                self.set_layer_prop(name, property, value)?;
            }
            return Ok(());
        }
        let name = own_layer(&self.drawing.doc, name)?;
        let linetype = match prop {
            LayerProp::Linetype => {
                let lt = as_str(value)?;
                self.ensure_convention_linetype(lt)?;
                Some(
                    self.drawing
                        .doc
                        .line_types
                        .get(lt)
                        .filter(|_| {
                            !lt.eq_ignore_ascii_case("ByLayer")
                                && !lt.eq_ignore_ascii_case("ByBlock")
                        })
                        .map(|t| t.name.clone())
                        .ok_or_else(|| invalid(format!("The drawing has no linetype {lt}")))?,
                )
            }
            _ => None,
        };
        if prop == LayerProp::Frozen
            && as_bool(value)?
            && normalize_name(&name) == normalize_name(&self.current.layer)
        {
            return Err(invalid(format!(
                "Layer {name} is the current layer: it can't be frozen"
            )));
        }
        let layer = self.drawing.doc.layers.get_mut(&name).expect("found above");
        match prop {
            LayerProp::Properties => unreachable!("handled above"),
            LayerProp::On => layer.flags.off = !as_bool(value)?,
            LayerProp::Frozen => layer.flags.frozen = as_bool(value)?,
            LayerProp::Locked => layer.flags.locked = as_bool(value)?,
            LayerProp::Plot => layer.is_plottable = as_bool(value)?,
            LayerProp::Color => {
                layer.color = if let Some(index) = value.as_u64().filter(|i| (1..=255).contains(i))
                {
                    Color::Index(index as u8)
                } else {
                    layer_color(as_str(value)?)?
                };
                layer.color_name = None;
                layer.book_name = None;
            }
            LayerProp::Linetype => layer.line_type = linetype.expect("checked above"),
            LayerProp::Lineweight => {
                let w = value
                    .as_i64()
                    .and_then(|w| i16::try_from(w).ok())
                    .filter(|w| LINEWEIGHTS.contains(w) || *w == -3)
                    .ok_or_else(|| invalid(format!("{value} is not a layer lineweight")))?;
                layer.line_weight = LineWeight::from_value(w);
            }
        }
        Ok(())
    }

    /// `Command::NewLayer`: a layer `name` the drawing lacks, white, Continuous.
    pub(crate) fn new_layer(&mut self, name: &str) -> Result<(), CoreError> {
        let name = check_layer_name(name)?;
        let doc = &mut self.drawing.doc;
        if let Some(old) = doc.layers.get(name) {
            self.last_advice.push(serde_json::json!({
                "code":"already_exists", "name":old.name,
                "message":format!("Layer {} already exists; its properties are unchanged.", old.name),
                "message_ar":format!("الطبقة {} موجودة بالفعل؛ خصائصها لم تتغير.", old.name),
            }));
            return Ok(());
        }
        let mut layer = Layer::new(name);
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).map_err(invalid)
    }

    /// `Command::DeleteLayer`: never layer 0, Defpoints, the current layer or one with entities
    /// on it (inside blocks too). Viewports forget freezing it.
    pub(crate) fn delete_layer(&mut self, name: &str) -> Result<(), CoreError> {
        let name = own_layer(&self.drawing.doc, name)?;
        if is_reserved(&name) {
            return Err(invalid(format!("Layer {name} can't be deleted")));
        }
        let key = normalize_name(&name);
        if normalize_name(&self.current.layer) == key {
            return Err(invalid(format!(
                "Layer {name} is the current layer: make another one current first"
            )));
        }
        let doc = &self.drawing.doc;
        let used = doc
            .entities()
            .filter(|e| normalize_name(&e.common().layer) == key)
            .count();
        if used > 0 {
            return Err(invalid(format!(
                "Layer {name} has {used} entities on it: move or erase them first"
            )));
        }
        let handle = doc.layers.get(&name).expect("found above").handle;
        self.forget_viewport_layer(handle, None);
        self.drawing.doc.layers.remove(&name);
        self.visibility
            .hidden_layers
            .retain(|n| normalize_name(n) != key);
        if let Some(iso) = &mut self.isolated {
            iso.hid.retain(|n| normalize_name(n) != key);
            if normalize_name(&iso.layer) == key {
                self.isolated = None;
            }
        }
        Ok(())
    }

    /// Viewports that freeze the layer `from` freeze `to` instead (or nothing, with `None`).
    fn forget_viewport_layer(&mut self, from: Handle, to: Option<Handle>) {
        let doc = &mut self.drawing.doc;
        let viewports: Vec<Handle> = doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Viewport(v) if v.frozen_layers.contains(&from) => Some(v.common.handle),
                _ => None,
            })
            .collect();
        for h in viewports {
            if let Some(EntityType::Viewport(v)) = doc.get_entity_mut(h) {
                v.frozen_layers.retain(|l| *l != from);
                if let Some(to) = to.filter(|t| !v.frozen_layers.contains(t)) {
                    v.frozen_layers.push(to);
                }
            }
        }
    }

    /// `Command::RenameLayer`: the layer, its entities (inside blocks too), the take-off rows on
    /// it, the current layer, the hidden layers and the layer states follow. A name another layer
    /// has is refused: the translator merges.
    pub(crate) fn rename_layer(&mut self, from: &str, to: &str) -> Result<(), CoreError> {
        let from = own_layer(&self.drawing.doc, from)?;
        let to = check_layer_name(to)?.to_string();
        if let Some(other) = self
            .drawing
            .doc
            .layers
            .get(&to)
            .filter(|l| normalize_name(&l.name) != normalize_name(&from))
        {
            return Err(invalid(format!(
                "The drawing has a layer {} already: translate layers to merge them",
                other.name
            )));
        }
        if is_reserved(&from) {
            return Err(invalid(format!("Layer {from} can't be renamed")));
        }
        if from == to {
            return Ok(());
        }
        self.drawing.doc.rename_layer(&from, &to).map_err(invalid)?;
        self.rename_layer_references(&from, &to)?;
        self.layer_renamed(&from, &to);
        Ok(())
    }

    /// What follows a layer `from` that is now `to`: the take-off rows on it, the current
    /// layer, the hidden layers and the hidden layers kept with layer states.
    fn layer_renamed(&mut self, from: &str, to: &str) {
        let key = normalize_name(from);
        for m in &mut self.takeoff.items {
            if normalize_name(&m.layer) == key {
                m.layer = to.to_string();
            }
        }
        if normalize_name(&self.current.layer) == key {
            self.current.layer = to.to_string();
        }
        let hidden = &mut self.visibility.hidden_layers;
        if hidden.iter().any(|n| normalize_name(n) == key) {
            hidden.retain(|n| normalize_name(n) != key);
            hidden.insert(to.to_string());
        }
        if let Some(iso) = &mut self.isolated {
            if normalize_name(&iso.layer) == key {
                iso.layer = to.to_string();
            }
            for n in iso.hid.iter_mut() {
                if normalize_name(n) == key {
                    *n = to.to_string();
                }
            }
        }
        for list in self.state_hidden.values_mut() {
            for n in list.iter_mut() {
                if normalize_name(n) == key {
                    *n = to.to_string();
                }
            }
        }
    }

    /// `Command::TranslateLayers`: each mapping in turn renames its layer, or merges it into
    /// the layer of that name (its entities move, the old layer goes); then the layer takes the
    /// colour, linetype, lineweight and plot setting of the standard's layer of that name (the
    /// drawing at `from_path`, read-only) and the mapping's own colour. Everything is checked
    /// first, so a refused translation changes nothing.
    pub(crate) fn translate_layers(
        &mut self,
        from_path: Option<&str>,
        mappings: &[crate::LayerMapping],
    ) -> Result<(), CoreError> {
        if mappings.is_empty() {
            return Err(invalid("Map one layer at least"));
        }
        let standard = from_path.map(|p| Drawing::open(Path::new(p))).transpose()?;
        let mut checked = Vec::with_capacity(mappings.len());
        let mut seen = HashSet::new();
        for m in mappings {
            let from = own_layer(&self.drawing.doc, &m.from)?;
            let to = check_layer_name(&m.to)?.to_string();
            let color = m.color.as_deref().map(layer_color).transpose()?;
            let same = normalize_name(&from) == normalize_name(&to);
            if !same && is_reserved(&from) {
                return Err(invalid(format!("Layer {from} can't be renamed or merged")));
            }
            if !seen.insert(normalize_name(&from)) {
                return Err(invalid(format!("Layer {from} is mapped twice")));
            }
            checked.push((from, to, color));
        }
        for (from, to, color) in checked {
            let target = self.translate_one(&from, &to)?;
            let theirs = standard.as_ref().and_then(|d| d.doc.layers.get(&target));
            let line_type = theirs
                .map(|l| l.line_type.clone())
                .filter(|lt| self.drawing.doc.line_types.contains(lt));
            let doc = &mut self.drawing.doc;
            let layer = doc.layers.get_mut(&target).expect("translated above");
            if let Some(theirs) = theirs {
                layer.color = theirs.color;
                layer.color_name.clone_from(&theirs.color_name);
                layer.book_name.clone_from(&theirs.book_name);
                layer.line_weight = theirs.line_weight;
                layer.is_plottable = theirs.is_plottable;
                if let Some(lt) = line_type {
                    layer.line_type = lt;
                }
            }
            if let Some(color) = color {
                layer.color = color;
                layer.color_name = None;
                layer.book_name = None;
            }
        }
        Ok(())
    }

    /// Renames `from` to `to`, or merges it into the drawing's `to`: the name it has now.
    fn translate_one(&mut self, from: &str, to: &str) -> Result<String, CoreError> {
        let doc = &self.drawing.doc;
        if normalize_name(from) == normalize_name(to) {
            return Ok(from.to_string());
        }
        let Some(into) = doc.layers.get(to).map(|l| (l.name.clone(), l.handle)) else {
            self.drawing.doc.rename_layer(from, to).map_err(invalid)?;
            self.layer_renamed(from, to);
            return Ok(to.to_string());
        };
        let key = normalize_name(from);
        let moving: Vec<Handle> = doc
            .entities()
            .filter(|e| normalize_name(&e.common().layer) == key)
            .map(|e| e.common().handle)
            .collect();
        for h in moving {
            if let Some(e) = self.drawing.doc.get_entity_mut(h) {
                e.common_mut().layer.clone_from(&into.0);
            }
        }
        let old = self.drawing.doc.layers.get(from).expect("checked").handle;
        self.forget_viewport_layer(old, Some(into.1));
        self.drawing.doc.layers.remove(from);
        self.rename_layer_references(from, &into.0)?;
        self.layer_renamed(from, &into.0);
        Ok(into.0)
    }

    /// `Command::SetViewportFrozen`: the paper-space viewport `viewport` (on the sheet on screen)
    /// freezes the layer `layer` inside it, or thaws it.
    pub(crate) fn set_viewport_frozen(
        &mut self,
        viewport: &Target,
        layer: &str,
        frozen: bool,
    ) -> Result<(), CoreError> {
        let h = self.sheet_targets_any(std::slice::from_ref(viewport))?[0];
        if !matches!(
            self.drawing.doc.get_entity(h),
            Some(EntityType::Viewport(_))
        ) {
            return Err(invalid("Pick a viewport"));
        }
        let name = own_layer(&self.drawing.doc, layer)?;
        let layer = self.drawing.doc.layers.get(&name).expect("found").handle;
        let Some(EntityType::Viewport(v)) = self.drawing.doc.get_entity(h) else {
            unreachable!("checked above");
        };
        if v.frozen_layers.contains(&layer) == frozen {
            return Ok(());
        }
        let Some(EntityType::Viewport(v)) = self.drawing.doc.get_entity_mut(h) else {
            unreachable!("checked above");
        };
        if frozen {
            v.frozen_layers.push(layer);
        } else {
            v.frozen_layers.retain(|l| *l != layer);
        }
        Ok(())
    }

    /// The paper-space viewports of the sheet on screen (not the sheet's own, number 1), with
    /// the layers each freezes.
    pub(crate) fn viewports(&self) -> Vec<ViewportInfo> {
        let doc = &self.drawing.doc;
        doc.entities_in_block(&self.sheet)
            .filter_map(|e| match e {
                EntityType::Viewport(v) if v.id != 1 => {
                    let mut frozen: Vec<String> = v
                        .frozen_layers
                        .iter()
                        .filter_map(|h| doc.layers.iter().find(|l| l.handle == *h))
                        .map(|l| l.name.clone())
                        .collect();
                    frozen.sort();
                    Some(ViewportInfo {
                        handle: v.common.handle.value(),
                        frozen,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// The drawing's linetypes, sorted.
    pub(crate) fn linetypes(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .drawing
            .doc
            .line_types
            .iter()
            .map(|t| t.name.clone())
            .filter(|n| !n.eq_ignore_ascii_case("ByLayer") && !n.eq_ignore_ascii_case("ByBlock"))
            .collect();
        names.sort();
        names
    }
}

/// A layer isolated on screen (LAYISO): the layer, the layers isolating it hid, and whether it
/// was hidden itself and so shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Isolation {
    pub layer: String,
    pub hid: Vec<String>,
    pub showed: bool,
}

impl Session {
    /// Shows only the layer `name` (AutoCAD's LAYISO): every other layer of the drawing and its
    /// xrefs is hidden, and `name` shown. Isolating the layer already isolated un-isolates it;
    /// isolating another first puts back what the first hid. Not an undo step.
    pub fn isolate_layer(&mut self, name: &str) -> Result<(), CoreError> {
        let key = normalize_name(name);
        let layer = self
            .layers()
            .into_iter()
            .map(|l| l.name)
            .find(|n| normalize_name(n) == key)
            .ok_or_else(|| CoreError::InvalidCommand(format!("No layer {name}")))?;
        let again = self
            .isolated
            .as_ref()
            .is_some_and(|i| normalize_name(&i.layer) == key);
        self.unisolate_layers();
        if again {
            return Ok(());
        }
        let hidden = &mut self.visibility.hidden_layers;
        let showed = hidden.iter().any(|n| normalize_name(n) == key);
        hidden.retain(|n| normalize_name(n) != key);
        let mut hid = Vec::new();
        for other in self.layers().into_iter().map(|l| l.name) {
            let other_key = normalize_name(&other);
            if other_key == key {
                continue;
            }
            let hidden = &mut self.visibility.hidden_layers;
            if !hidden.iter().any(|n| normalize_name(n) == other_key) {
                hidden.insert(other.clone());
                hid.push(other);
            }
        }
        self.isolated = Some(Isolation { layer, hid, showed });
        Ok(())
    }

    /// Puts back what isolating a layer changed (AutoCAD's LAYUNISO): shows the layers it hid
    /// that are still hidden, and hides the isolated layer again if it was hidden before. Layers
    /// the user hid or showed since stay as they are. Nothing to do when no layer is isolated.
    pub fn unisolate_layers(&mut self) {
        let Some(iso) = self.isolated.take() else {
            return;
        };
        let hidden = &mut self.visibility.hidden_layers;
        for name in &iso.hid {
            let key = normalize_name(name);
            hidden.retain(|n| normalize_name(n) != key);
        }
        if iso.showed {
            hidden.insert(iso.layer);
        }
    }
}
