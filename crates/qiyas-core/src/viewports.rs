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
use crate::{CoreError, Session, Target};

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

const FIT_APP: &str = "QIYAS_VIEW_FIT";
#[derive(Serialize, Deserialize)]
struct AutoFit {
    bounds: crate::ink::Bounds,
    scale: f64,
    automatic: bool,
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

pub(crate) struct ViewportFrame {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub centre: [f64; 2],
    pub scale: f64,
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
    pub(crate) fn remember_viewport_fit(
        &mut self,
        handle: u64,
        bounds: crate::ink::Bounds,
        automatic: bool,
    ) -> Result<(), CoreError> {
        let app = crate::xdata::register(&mut self.drawing.doc, FIT_APP);
        let Some(EntityType::Viewport(v)) = self.drawing.doc.get_entity_mut(Handle::new(handle))
        else {
            return Ok(());
        };
        let value = AutoFit {
            bounds,
            scale: v.scale(),
            automatic,
        };
        crate::xdata::write_json(&mut v.common, FIT_APP, app, &value)
    }

    pub(crate) fn refit_arranged_viewports(&mut self, sheet: &str) -> Result<(), CoreError> {
        let views: Vec<_> = self
            .drawing
            .doc
            .entities_in_block(sheet)
            .filter_map(|e| {
                let EntityType::Viewport(v) = e else {
                    return None;
                };
                (v.id > 1 && v.status.is_on).then_some((
                    v.common.handle.value(),
                    crate::xdata::read_json::<AutoFit>(&v.common, FIT_APP),
                    v.scale(),
                ))
            })
            .collect();
        if views.iter().all(|(_, fit, _)| fit.is_none()) {
            return Ok(());
        }
        let usable = self.viewport_usable_area(sheet)?;
        let columns = (views.len() as f64).sqrt().ceil() as usize;
        let rows = views.len().div_ceil(columns);
        let cell = [
            (usable[1][0] - usable[0][0]) / columns as f64,
            (usable[1][1] - usable[0][1]) / rows as f64,
        ];
        for (index, (h, fit, current_scale)) in views.iter().enumerate() {
            let Some(fit) = fit else {
                continue;
            };
            if !fit.automatic || (fit.scale - current_scale).abs() > fit.scale.abs() * 1e-9 {
                self.advise_viewport_fill(fit.bounds, *current_scale, usable);
                continue;
            }
            if self.is_locked(Handle::new(*h)) {
                continue;
            }
            let lo = [
                usable[0][0] + (index % columns) as f64 * cell[0],
                usable[0][1] + (index / columns) as f64 * cell[1],
            ];
            let area = [lo, [lo[0] + cell[0], lo[1] + cell[1]]];
            let mm = self.drawing.units.to_metres() * 1000.0;
            let model = [
                (fit.bounds[1][0] - fit.bounds[0][0]).abs(),
                (fit.bounds[1][1] - fit.bounds[0][1]).abs(),
            ];
            let Some(den) = crate::sheet_setup::fitting_denominator(
                model.map(|v| v * mm),
                [
                    area[1][0] - area[0][0] - 10.0,
                    area[1][1] - area[0][1] - 10.0,
                ],
            ) else {
                continue;
            };
            let scale = mm / den;
            let centre = [
                (area[0][0] + area[1][0]) / 2.0,
                (area[0][1] + area[1][1]) / 2.0,
            ];
            let frame = ViewportFrame {
                a: [
                    centre[0] - model[0] * scale / 2.0,
                    centre[1] - model[1] * scale / 2.0,
                ],
                b: [
                    centre[0] + model[0] * scale / 2.0,
                    centre[1] + model[1] * scale / 2.0,
                ],
                centre: [
                    (fit.bounds[0][0] + fit.bounds[1][0]) / 2.0,
                    (fit.bounds[0][1] + fit.bounds[1][1]) / 2.0,
                ],
                scale,
            };
            self.resize_viewport(
                &Target {
                    handle: *h,
                    chain: vec![],
                },
                frame,
            )?;
            self.remember_viewport_fit(*h, fit.bounds, true)?;
        }
        Ok(())
    }

    /// The drawing field inside the title frame and its reserved strips.
    pub(crate) fn viewport_usable_area(
        &self,
        sheet: &str,
    ) -> Result<crate::ink::Bounds, CoreError> {
        if let Some(zones) = self.sheet_zones(sheet) {
            return Ok(zones.plans);
        }
        let owner = self.drawing.doc.block_records.get(sheet).map(|b| b.handle);
        self.drawing
            .doc
            .objects
            .values()
            .find_map(|o| {
                let opencadcodec::objects::ObjectType::Layout(l) = o else {
                    return None;
                };
                if Some(l.block_record) != owner {
                    return None;
                }
                let (mut w, mut h) = (l.paper_width, l.paper_height);
                if matches!(l.plot_rotation, 1 | 3) {
                    std::mem::swap(&mut w, &mut h);
                }
                if w <= 0.0 || h <= 0.0 {
                    return None;
                }
                let mut area = crate::sheet_setup::zones_of(w, h, &[]).plans;
                for entity in self.drawing.doc.entities_in_block(sheet).filter(|e| {
                    e.common().layer.eq_ignore_ascii_case("A-ANNO-TTLB")
                        && matches!(e, EntityType::LwPolyline(p) if p.is_closed)
                }) {
                    let frame = crate::ink::union(
                        qiyas_geom::flatten(
                            &self.drawing.doc,
                            entity,
                            &qiyas_geom::Xform::identity(),
                            0.01,
                        )
                        .into_paths()
                        .iter()
                        .filter_map(|p| p.bounds()),
                    );
                    if let Some(frame) =
                        frame.filter(|b| b[1][0] - b[0][0] > w * 0.8 && b[1][1] - b[0][1] > h * 0.8)
                    {
                        for i in 0..2 {
                            area[0][i] = area[0][i].max(frame[0][i]);
                            area[1][i] = area[1][i].min(frame[1][i]);
                        }
                    }
                }
                Some(area)
            })
            .ok_or_else(|| {
                invalid("Size the layout with set_layout_paper before fitting a viewport")
            })
    }

    pub(crate) fn advise_viewport_fill(
        &mut self,
        bounds: crate::ink::Bounds,
        scale: f64,
        area: crate::ink::Bounds,
    ) {
        let model = [
            (bounds[1][0] - bounds[0][0]).abs(),
            (bounds[1][1] - bounds[0][1]).abs(),
        ];
        let paper = [
            area[1][0] - area[0][0] - 10.0,
            area[1][1] - area[0][1] - 10.0,
        ];
        let mm = self.drawing.units.to_metres() * 1000.0;
        let Some(den) = crate::sheet_setup::fitting_denominator(model.map(|v| v * mm), paper)
        else {
            return;
        };
        let fill = model[0] * model[1] * scale * scale / (paper[0] * paper[1]);
        if fill < 0.4 && mm / den > scale * (1.0 + 1e-9) {
            self.last_advice.push(serde_json::json!({
                "code":"viewport_underfilled", "fill_fraction":fill, "suggested_scale":format!("1:{den}"),
                "message":format!("The drawing fills {:.0}% of the usable sheet area; 1:{den} would fit. The explicit scale was retained.", fill * 100.0),
                "message_ar":format!("يشغل الرسم {:.0}% من المساحة المتاحة باللوحة؛ يتسع مقياس 1:{den}. تم الاحتفاظ بالمقياس المحدد.", fill * 100.0)
            }));
        }
    }

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
            ([(x0 + x1) / 2.0, (y0 + y1) / 2.0], {
                let max_scale = fit(w, ew).min(fit(h, eh));
                let mm = self.drawing.units.to_metres() * 1000.0;
                let den = crate::sheet_setup::fitting_denominator(
                    [ew.max(1e-9) * mm, eh.max(1e-9) * mm],
                    [w, h],
                )
                .ok_or_else(|| invalid("The model cannot fit this viewport"))?;
                (mm / den).min(max_scale)
            })
        })
    }

    /// Resolve either an explicit paper frame or a model window at a physical scale.
    pub(crate) fn viewport_frame(
        &self,
        a: [f64; 2],
        b: [f64; 2],
        scale: Option<&ViewportScale>,
        centre: Option<[f64; 2]>,
        model_bounds: Option<[[f64; 2]; 2]>,
        paper_centre: Option<[f64; 2]>,
    ) -> Result<ViewportFrame, CoreError> {
        if self.sheet.eq_ignore_ascii_case(MODEL) {
            return Err(invalid("Viewports go on a layout: use new_layout to create one or set_layout_paper to size an existing layout, then set_sheet to select it"));
        }
        if ![a, b].iter().flatten().all(|v| v.is_finite()) {
            return Err(invalid("Points must have finite coordinates"));
        }
        if centre.is_some_and(|p| !p.into_iter().all(f64::is_finite)) {
            return Err(invalid("The model centre must have finite coordinates"));
        }
        let given_scale = scale
            .map(|s| s.custom_scale(self.drawing.units.to_metres()))
            .transpose()?;
        if let Some(bounds) = model_bounds {
            if a != [0.0, 0.0] || b != [0.0, 0.0] || centre.is_some() {
                return Err(invalid("Use model_bounds with scale and paper_centre, or corner1/corner2 with centre; do not combine the two forms"));
            }
            let area = if given_scale.is_none() || paper_centre.is_none() {
                self.viewport_usable_area(&self.sheet)?
            } else {
                [[0.0, 0.0], [0.0, 0.0]]
            };
            let paper_centre = paper_centre.unwrap_or([
                (area[0][0] + area[1][0]) / 2.0,
                (area[0][1] + area[1][1]) / 2.0,
            ]);
            let scale = if let Some(scale) = given_scale {
                scale
            } else {
                let mm = self.drawing.units.to_metres() * 1000.0;
                let available = [
                    2.0 * (paper_centre[0] - area[0][0]).min(area[1][0] - paper_centre[0]) - 10.0,
                    2.0 * (paper_centre[1] - area[0][1]).min(area[1][1] - paper_centre[1]) - 10.0,
                ];
                let den = crate::sheet_setup::fitting_denominator(
                    [
                        (bounds[1][0] - bounds[0][0]).abs() * mm,
                        (bounds[1][1] - bounds[0][1]).abs() * mm,
                    ],
                    available,
                )
                .ok_or_else(|| invalid("The model window cannot fit inside the sheet margins"))?;
                mm / den
            };
            if !bounds
                .iter()
                .flatten()
                .chain(paper_centre.iter())
                .all(|v| v.is_finite())
            {
                return Err(invalid(
                    "model_bounds and paper_centre must have finite coordinates",
                ));
            }
            let w = (bounds[1][0] - bounds[0][0]).abs() * scale;
            let h = (bounds[1][1] - bounds[0][1]).abs() * scale;
            if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
                return Err(invalid(
                    "model_bounds must enclose a finite nonzero width and height",
                ));
            }
            return Ok(ViewportFrame {
                a: [paper_centre[0] - w / 2.0, paper_centre[1] - h / 2.0],
                b: [paper_centre[0] + w / 2.0, paper_centre[1] + h / 2.0],
                centre: [
                    (bounds[0][0] + bounds[1][0]) / 2.0,
                    (bounds[0][1] + bounds[1][1]) / 2.0,
                ],
                scale,
            });
        }
        if paper_centre.is_some() {
            return Err(invalid(
                "paper_centre needs model_bounds; explicit frames use corner1 and corner2",
            ));
        }
        let (mut a, mut b) = (
            [a[0].min(b[0]), a[1].min(b[1])],
            [a[0].max(b[0]), a[1].max(b[1])],
        );
        if given_scale.is_none() {
            if let Ok(area) = self.viewport_usable_area(&self.sheet) {
                let lo = [a[0].max(area[0][0] + 5.0), a[1].max(area[0][1] + 5.0)];
                let hi = [b[0].min(area[1][0] - 5.0), b[1].min(area[1][1] - 5.0)];
                if (0..2).all(|i| hi[i] > lo[i]) {
                    a = lo;
                    b = hi;
                } else {
                    a = area[0].map(|v| v + 5.0);
                    b = area[1].map(|v| v - 5.0);
                }
            }
        }
        let (fit_centre, fit_scale) = self.viewport_fit_default(a, b)?;
        if given_scale.is_none() && centre.is_none() {
            let bounds = self.with_model(None, |m| m.bounds);
            let middle = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            for i in 0..2 {
                let size = (bounds[1][i] - bounds[0][i]) * fit_scale;
                if size > 0.0 {
                    a[i] = middle[i] - size / 2.0;
                    b[i] = middle[i] + size / 2.0;
                }
            }
        }
        Ok(ViewportFrame {
            a,
            b,
            centre: centre.unwrap_or(fit_centre),
            scale: given_scale.unwrap_or(fit_scale),
        })
    }

    /// Create a viewport from a checked frame; the generated layer is part of this command.
    pub(crate) fn mview(&mut self, frame: ViewportFrame) -> Result<u64, CoreError> {
        let ViewportFrame {
            a,
            b,
            centre,
            scale,
        } = frame;
        let (w, h) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
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
        let entities = vec![EntityType::Viewport(v)];
        let added = if self.current.layer.eq_ignore_ascii_case("0") {
            self.add_on_annotation_layer("A-VIEW", entities, true)?
        } else {
            self.add_styled(entities)?
        };
        Ok(added[0])
    }

    /// Resize the same viewport, preserving identity, frozen layers and its other settings.
    pub(crate) fn resize_viewport(
        &mut self,
        target: &Target,
        frame: ViewportFrame,
    ) -> Result<(), CoreError> {
        let handle = Handle::new(target.handle);
        if !target.chain.is_empty() {
            return Err(invalid("Choose a viewport directly on the current layout"));
        }
        let entity = self
            .drawing
            .doc
            .entities_in_block(&self.sheet)
            .find(|entity| entity.common().handle == handle)
            .ok_or_else(|| invalid("The viewport target must be on the current layout"))?;
        let EntityType::Viewport(view) = entity else {
            return Err(invalid("The target is not a viewport"));
        };
        if view.id == 1 {
            return Err(invalid(
                "The layout's overall viewport cannot be resized; choose a model viewport",
            ));
        }
        if self
            .drawing
            .doc
            .layers
            .get(&view.common.layer)
            .is_some_and(|layer| layer.flags.locked)
        {
            return Err(invalid("The viewport's layer is locked: unlock it first"));
        }
        let Some(EntityType::Viewport(view)) = self.drawing.doc.get_entity_mut(handle) else {
            unreachable!("checked viewport");
        };
        let ViewportFrame {
            a,
            b,
            centre,
            scale,
        } = frame;
        view.center = Vector3::new((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0);
        view.width = (b[0] - a[0]).abs();
        view.height = (b[1] - a[1]).abs();
        view.view_target = Vector3::ZERO;
        view.view_center = Vector3::new(centre[0], centre[1], 0.0);
        view.set_scale(scale);
        Ok(())
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
