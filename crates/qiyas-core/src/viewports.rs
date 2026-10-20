//! Paper-space viewports: `Command::MView` makes one on the layout on screen, looking at model
//! space's extents; its frame draws on the layout as a line (`qiyas_geom` flatten), a click
//! inside it picks it (`Session::pick`). The window draws model space inside each one
//! (`Session::paper_views`): model space is drawn once (`Session::with_model`, fetched by the
//! window through `Session::sheet_batch_bytes_if`) and shown in each
//! viewport through its own camera, clipped to its frame, without the layers the viewport freezes
//! (spans of that one batch). Twisted views are drawn without their twist.

use std::collections::{HashMap, HashSet};

use opencadcodec::entities::Viewport;
use opencadcodec::tables::normalize_name;
use opencadcodec::{CadDocument, EntityType, Handle, Vector3};
use qiyas_render::{layer_shown, Batch, Range};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{locate, Picked};
use crate::{CoreError, Session};

const MODEL: &str = "*Model_Space";

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

/// A physical paper:model ratio, or a custom scale in paper millimetres per model unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ViewportScale {
    /// A paper:model ratio such as "1:100", accounting for the drawing's model units.
    Ratio(String),
    /// Paper millimetres per model drawing unit, as the viewport's custom scale stores it.
    Number(f64),
}

impl ViewportScale {
    fn custom_scale(&self, metres_per_unit: f64) -> Result<f64, CoreError> {
        let value = match self {
            Self::Number(value) => *value,
            Self::Ratio(ratio) => {
                let (paper, model) = ratio
                    .split_once(':')
                    .ok_or_else(|| invalid("Write a viewport scale as 1:100 or a number"))?;
                let parse = |value: &str| {
                    value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| v.is_finite() && *v > 0.0)
                };
                let (Some(paper), Some(model)) = (parse(paper), parse(model)) else {
                    return Err(invalid(
                        "A viewport ratio needs two finite positive numbers",
                    ));
                };
                paper / model * metres_per_unit / 0.001
            }
        };
        if !value.is_finite() || value <= 0.0 {
            return Err(invalid("A viewport scale must be finite and positive"));
        }
        Ok(value)
    }
}

/// One viewport of the layout on screen, as the window draws model space inside it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PaperView {
    pub handle: u64,
    /// Its frame on the layout, `[lower left, upper right]`.
    pub rect: [[f64; 2]; 2],
    /// The model space point at the frame's centre.
    pub centre: [f64; 2],
    /// Layout units per model space unit.
    pub scale: f64,
    /// The spans `[first, count]` of the model batch's segments, fill triangles and glyph quads
    /// drawn inside it; `None` for all (it freezes no layer).
    pub lines: Option<Vec<[u32; 2]>>,
    pub fills: Option<Vec<[u32; 2]>>,
    pub texts: Option<Vec<[u32; 2]>>,
}

/// The model point at `v`'s centre and its scale; `None` when it shows nothing (the sheet's own,
/// number 1, off, or flat).
fn view_of(v: &Viewport) -> Option<([f64; 2], f64)> {
    if v.id == 1 || !v.status.is_on || !(v.width > 0.0 && v.height > 0.0) {
        return None;
    }
    let scale = v.scale();
    if !(scale > 0.0 && scale.is_finite()) {
        return None;
    }
    let centre = [
        v.view_target.x + v.view_center.x,
        v.view_target.y + v.view_center.y,
    ];
    Some((centre, scale))
}

/// `[first, count]` runs of the ranges `hidden` keeps, adjacent ones joined.
fn spans(ranges: &[Range], mut hidden: impl FnMut(&Range) -> bool) -> Vec<[u32; 2]> {
    let mut out: Vec<[u32; 2]> = Vec::new();
    for r in ranges {
        if r.count == 0 || hidden(r) {
            continue;
        }
        match out.last_mut() {
            Some(last) if last[0] + last[1] == r.first => last[1] += r.count,
            _ => out.push([r.first, r.count]),
        }
    }
    out
}

/// The paper-space viewports on the sheet `sheet` that draw: visible, on a shown layer.
fn viewports_on<'a>(session: &'a Session, sheet: &str) -> Vec<&'a Viewport> {
    let doc = &session.drawing.doc;
    doc.entities_in_block(sheet)
        .filter_map(|e| match e {
            EntityType::Viewport(v)
                if !v.common.invisible
                    && layer_shown(doc, &session.visibility, &v.common.layer) =>
            {
                Some(v)
            }
            _ => None,
        })
        .collect()
}

/// The names (normalised) of the layers `v` freezes.
fn frozen_names(doc: &CadDocument, v: &Viewport) -> HashSet<String> {
    v.frozen_layers
        .iter()
        .filter_map(|h| doc.layers.iter().find(|l| l.handle == *h))
        .map(|l| normalize_name(&l.name))
        .collect()
}

impl Session {
    /// Fits the model bounds in a paper rectangle without adding a viewport.
    pub(crate) fn viewport_fit_default(
        &self,
        a: [f64; 2],
        b: [f64; 2],
    ) -> Result<([f64; 2], f64), CoreError> {
        if ![a, b].iter().flatten().all(|v| v.is_finite()) {
            return Err(invalid("Points must have finite coordinates"));
        }
        let (w, h) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
        if !(w > 0.0 && h > 0.0) {
            return Err(invalid("Give the viewport two opposite corners"));
        }
        let (bounds, empty) = self.with_model(None, |m| {
            (
                m.bounds,
                m.ranges.is_empty() && m.fill_ranges.is_empty() && m.text_ranges.is_empty(),
            )
        });
        let [[x0, y0], [x1, y1]] = bounds;
        let (ew, eh) = (x1 - x0, y1 - y0);
        Ok(if empty || !(ew > 0.0 || eh > 0.0) {
            ([x0, y0], 1.0)
        } else {
            let fit = |paper: f64, model: f64| {
                if model > 0.0 {
                    paper / model
                } else {
                    f64::INFINITY
                }
            };
            (
                [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
                fit(w, ew).min(fit(h, eh)),
            )
        })
    }

    /// `Command::MView`: a viewport on the layout on screen with the corners `a` and `b`,
    /// showing model extents unless a scale or model centre is supplied; its handle.
    pub(crate) fn mview(
        &mut self,
        a: [f64; 2],
        b: [f64; 2],
        scale: Option<&ViewportScale>,
        centre: Option<[f64; 2]>,
    ) -> Result<u64, CoreError> {
        if ![a, b].iter().flatten().all(|v| v.is_finite()) {
            return Err(invalid("Points must have finite coordinates"));
        }
        if centre.is_some_and(|p| !p.into_iter().all(f64::is_finite)) {
            return Err(invalid("The model centre must have finite coordinates"));
        }
        let given_scale = scale
            .map(|s| s.custom_scale(self.drawing.units.to_metres()))
            .transpose()?;
        if self.sheet.eq_ignore_ascii_case(MODEL) {
            return Err(invalid(
                "Viewports go on a layout: choose a layout tab first",
            ));
        }
        let (w, h) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
        let (fit_centre, fit_scale) = self.viewport_fit_default(a, b)?;
        let centre = centre.unwrap_or(fit_centre);
        let scale = given_scale.unwrap_or(fit_scale);
        let id = self
            .drawing
            .doc
            .entities()
            .filter_map(|e| match e {
                EntityType::Viewport(v) => Some(v.id),
                _ => None,
            })
            .max()
            .unwrap_or(1)
            .max(1)
            + 1;
        let middle = Vector3::new((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0);
        let mut v = Viewport::with_size(middle, w, h);
        v.id = id;
        v.view_target = Vector3::ZERO;
        v.view_center = Vector3::new(centre[0], centre[1], 0.0);
        v.set_scale(scale);
        Ok(self.add_styled(vec![EntityType::Viewport(v)])?[0])
    }

    /// The viewports of the layout on screen and how the window draws model space inside each
    /// (see `PaperView`); none on model space.
    pub fn paper_views(&self) -> Vec<PaperView> {
        self.paper_views_of(&self.sheet)
    }

    /// `paper_views` for the sheet `sheet` (block name), on screen or not (a split window's
    /// second view).
    pub fn paper_views_of(&self, sheet: &str) -> Vec<PaperView> {
        if sheet.eq_ignore_ascii_case(MODEL) {
            return Vec::new();
        }
        let viewports = viewports_on(self, sheet);
        if viewports.is_empty() {
            return Vec::new();
        }
        self.with_model(None, |batch| self.views_against(viewports, batch))
    }

    /// `paper_views_of` with its spans counted in `batch`, a model space batch drawn for another
    /// zoom (Mizan's picture of a layout).
    pub fn paper_views_against(&self, sheet: &str, batch: &Batch) -> Vec<PaperView> {
        if sheet.eq_ignore_ascii_case(MODEL) {
            return Vec::new();
        }
        self.views_against(viewports_on(self, sheet), batch)
    }

    fn views_against(&self, viewports: Vec<&Viewport>, batch: &Batch) -> Vec<PaperView> {
        let doc = &self.drawing.doc;
        let xrefs = self.xref_docs();
        viewports
            .into_iter()
            .filter_map(|v| {
                let (centre, scale) = view_of(v)?;
                let (cx, cy, w, h) = (v.center.x, v.center.y, v.width / 2.0, v.height / 2.0);
                let frozen = frozen_names(doc, v);
                let (lines, fills, texts) = if frozen.is_empty() {
                    (None, None, None)
                } else {
                    let mut memo: HashMap<(u64, u32), bool> = HashMap::new();
                    let mut hidden = |r: &Range| {
                        *memo.entry((r.entity, r.chain)).or_insert_with(|| {
                            let chain = batch.chain_of(r);
                            let outer = chain.iter().any(|&h| {
                                doc.get_entity(Handle::new(h)).is_some_and(|e| {
                                    frozen.contains(&normalize_name(&e.common().layer))
                                })
                            });
                            outer
                                || locate(doc, &xrefs, r.entity, chain)
                                    .is_ok_and(|l| frozen.contains(&normalize_name(&l.layer)))
                        })
                    };
                    (
                        Some(spans(&batch.ranges, &mut hidden)),
                        Some(spans(&batch.fill_ranges, &mut hidden)),
                        Some(spans(&batch.text_ranges, &mut hidden)),
                    )
                };
                Some(PaperView {
                    handle: v.common.handle.value(),
                    rect: [[cx - w, cy - h], [cx + w, cy + h]],
                    centre,
                    scale,
                    lines,
                    fills,
                    texts,
                })
            })
            .collect()
    }

    /// Model space as drawn inside the layout's viewports (`with_model`).
    pub fn model_batch(&self) -> Batch {
        self.with_model(None, Batch::clone)
    }

    /// Names what `sheet_batch_bytes_if` would give for the sheet `sheet` (see `batch_version`).
    pub fn sheet_batch_version(&self, sheet: &str) -> u64 {
        self.version_of(sheet)
    }

    /// The batch of the sheet `sheet` (block name; model space for the viewports of a layout, or
    /// the second view of a split window) as `Batch::to_bytes` lays it out, its glyphs drawn for
    /// `px_per_unit` (see `batch_bytes_if`); `None` when `if_version` is its version. The sheet
    /// on screen stays as it is.
    pub fn sheet_batch_bytes_if(
        &self,
        sheet: &str,
        if_version: Option<u64>,
        px_per_unit: Option<f64>,
    ) -> Option<Vec<u8>> {
        if if_version == Some(self.sheet_batch_version(sheet)) {
            return None;
        }
        Some(self.with_sheet(sheet, px_per_unit, Batch::to_bytes))
    }

    /// The viewport of the layout on screen whose frame holds `p` (the last drawn, on top), as
    /// picked.
    pub(crate) fn viewport_at(&self, p: [f64; 2]) -> Option<Picked> {
        if self.sheet.eq_ignore_ascii_case(MODEL) {
            return None;
        }
        let v = viewports_on(self, &self.sheet)
            .into_iter()
            .filter(|v| v.id != 1)
            .rev()
            .find(|v| {
                let (w, h) = (v.width / 2.0, v.height / 2.0);
                (p[0] - v.center.x).abs() <= w && (p[1] - v.center.y).abs() <= h
            })?;
        let h = v.common.handle.value();
        Some(Picked {
            top: h,
            entity: h,
            chain: Vec::new(),
            xref: None,
            kind: self.kind_of(h),
        })
    }
}
