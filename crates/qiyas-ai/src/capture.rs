//! Pictures of the drawing and bounds for the window's view.

use crate::image::{self, normalized, Inset, Raster, Scene, TextSize, View, MAX_SIDE};
use crate::marks::Mark;
use crate::surface_io::{sheet_block, sheet_units};
use crate::{ToolError, ToolOutput};
use qiyas_core::Session;
use qiyas_render::Batch;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

const DEFAULT_WIDTH: u32 = 800;
const MIN_WIDTH: u32 = 16;
const MARGIN: f64 = 0.02;
const MAX_MARKS: usize = 40;
const MAX_RASTER_PX: f64 = 4096.0;
const MODEL: &str = "*Model_Space";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoomInput {
    handles: Option<Vec<u64>>,
    bounds: Option<[[f64; 2]; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageInput {
    bounds: Option<[[f64; 2]; 2]>,
    width: Option<u32>,
    sheet: Option<String>,
    #[serde(default)]
    highlight: Vec<u64>,
    #[serde(default)]
    marks: bool,
}

fn parse<T: DeserializeOwned>(input: &Value) -> Result<T, ToolError> {
    T::deserialize(input).map_err(|e| ToolError::BadInput(format!("Bad input: {e}")))
}

pub(crate) fn view(session: &Session, args: &Value) -> Result<Value, ToolError> {
    let input: ZoomInput = parse(args)?;
    let (bounds, handles) = match (input.handles, input.bounds) {
        (None, Some(bounds)) => (normalized(bounds), Vec::new()),
        (Some(handles), None) if !handles.is_empty() => (drawn_bounds(session, &handles)?, handles),
        (Some(_), None) => return Err(bad("Give at least one handle")),
        (Some(_), Some(_)) => return Err(bad("Give handles or bounds, not both")),
        (None, None) => return Err(bad("Give handles or bounds")),
    };
    Ok(json!({
        "bounds": bounds,
        "ui": { "action": "zoom", "bounds": bounds, "handles": handles },
    }))
}

pub(crate) fn capture(session: &Session, args: &Value) -> Result<ToolOutput, ToolError> {
    let input: ImageInput = parse(args)?;
    let look = Look {
        sheet: input.sheet.as_deref(),
        bounds: input.bounds,
        width: input.width.unwrap_or(DEFAULT_WIDTH),
        highlight: &input.highlight,
        region: &[],
        marks: input.marks,
    };
    let (png, caption) = picture(session, &look)?;
    Ok(ToolOutput::Image { png, caption })
}

/// What a capture or a picture of the user selection shows.
pub(crate) struct Look<'a> {
    /// A sheet by layout or block name; the one on screen when `None`.
    pub sheet: Option<&'a str>,
    /// The part shown, drawing coordinates; the whole sheet when `None`.
    pub bounds: Option<[[f64; 2]; 2]>,
    pub width: u32,
    /// Entities at the top of the sheet drawn thick violet and marked first.
    pub highlight: &'a [u64],
    /// A region the user marked, dashed.
    pub region: &'a [[f64; 2]],
    /// Numbered marks on the entities in view, listed in the caption.
    pub marks: bool,
}

/// A PNG of the drawing as `look` asks, drawn as the engineer sees it (pictures and PDF
/// underlays under the lines, model space in a layout's viewports, the text as letters), and
/// its caption: where it is, how big a pixel is, how tall the text is, the marks' handles and
/// what could not be drawn.
pub(crate) fn picture(session: &Session, look: &Look) -> Result<(Vec<u8>, String), ToolError> {
    let width = look.width.clamp(MIN_WIDTH, MAX_SIDE);
    let sheet = sheet_block(session, look.sheet)?;
    let bounds = match look.bounds {
        Some(b) => normalized(b),
        None => {
            let drawn = if sheet == session.sheet {
                session.batch()
            } else {
                session.sheet_batch(&sheet)
            };
            let (pictures, problems) = session.plot_pictures(&sheet, 1.0, MAX_RASTER_PX);
            let geometry = !drawn.segments.is_empty() || !drawn.fills.is_empty();
            if !geometry && pictures.is_empty() {
                if !problems.is_empty() {
                    return Err(bad(&format!(
                        "Pictures unavailable: {}",
                        problems.join("; ")
                    )));
                }
                return Err(bad("Nothing is drawn on that sheet"));
            }
            let mut bounds = if geometry {
                drawn.bounds
            } else {
                [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]]
            };
            for (_, placement, _) in pictures {
                for point in placement.corners {
                    for axis in 0..2 {
                        bounds[0][axis] = bounds[0][axis].min(point[axis]);
                        bounds[1][axis] = bounds[1][axis].max(point[axis]);
                    }
                }
            }
            with_margin(bounds)
        }
    };
    let bounds = fit(bounds, width);
    let view = View::new(bounds, width);
    // Drawn afresh for this picture's zoom, its text as glyphs: what the engineer sees.
    let px = view.px_per_unit();
    let (batch, pages) = session.picture_batch_of(&sheet, px);

    // Pictures in view, under the lines.
    let (placed, mut left_out) = session.plot_pictures(&sheet, px, MAX_RASTER_PX);
    let mut rasters = Vec::new();
    let mut shown = Vec::new();
    for (_, placement, image) in placed {
        if !overlaps(&placement.corners, bounds) {
            continue;
        }
        let file = image.path.file_name().map_or_else(
            || image.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match qiyas_core::draw_picture(&image, |_| {}) {
            Ok(drawn) => {
                let (w, h) = (drawn.width(), drawn.height());
                if let Some(r) = Raster::from_rgba(placement.corners, w, h, drawn.into_raw()) {
                    rasters.push(r);
                    shown.push(file);
                }
            }
            Err(e) => left_out.push(format!("{file}: {e}")),
        }
    }

    // Model space in the layout's viewports in view, drawn for the closest of their zooms.
    let frames = session.paper_views_against(&sheet, &Batch::default());
    let scale = frames
        .iter()
        .filter(|v| boxes_overlap(v.rect, bounds))
        .map(|v| v.scale)
        .fold(0.0, f64::max);
    let (model, model_pages, views) = if scale > 0.0 {
        let (model, pages) = session.picture_batch_of(MODEL, px * scale);
        let views = session.paper_views_against(&sheet, &model);
        (model, pages, views)
    } else {
        (Batch::default(), Vec::new(), Vec::new())
    };
    let insets: Vec<Inset> = views
        .iter()
        .map(|view| Inset {
            batch: &model,
            pages: &model_pages,
            view,
        })
        .collect();

    let measurements: Vec<_> = session
        .takeoff
        .items
        .iter()
        .filter(|m| m.sheet == sheet)
        .cloned()
        .collect();
    let drawn = image::draw(
        &Scene {
            batch: &batch,
            pages: &pages,
            rasters: &rasters,
            insets: &insets,
            measurements: &measurements,
            highlight: look.highlight,
            region: look.region,
            marks: if look.marks { MAX_MARKS } else { 0 },
        },
        view,
    );
    let mut caption = format!(
        "Sheet {}, bounds [[{:.3}, {:.3}], [{:.3}, {:.3}]] in {}, {} x {} px, {}.",
        sheet_label(session, &sheet),
        bounds[0][0],
        bounds[0][1],
        bounds[1][0],
        bounds[1][1],
        sheet_units(session, &sheet),
        drawn.width,
        drawn.height,
        pixel_size(session, &sheet, px),
    );
    if !shown.is_empty() {
        caption.push_str(&format!(" Pictures: {}.", shown.join(", ")));
    }
    if !look.highlight.is_empty() {
        caption.push_str(" Highlighted entities are thick violet.");
    }
    if !look.region.is_empty() {
        caption.push_str(" The region the user marked is dashed blue.");
    }
    caption.push_str(&text_note(drawn.text));
    caption.push_str(&marks_legend(session, &drawn.marks));
    if !left_out.is_empty() {
        caption.push_str(&format!(" Left out: {}.", left_out.join("; ")));
    }
    Ok((drawn.png, caption))
}

/// How big a pixel is: drawing units, and millimetres.
fn pixel_size(session: &Session, sheet: &str, px_per_unit: f64) -> String {
    let per_px = 1.0 / px_per_unit.max(f64::MIN_POSITIVE);
    if !sheet.eq_ignore_ascii_case("*Model_Space") {
        return format!("1 px = {per_px:.4} paper millimetres");
    }
    let mm = per_px * session.drawing.units.to_metres() * 1000.0;
    format!("1 px = {per_px:.4} drawing units ({mm:.1} mm)")
}

/// What the caption says of the text in view.
pub(crate) fn text_note(text: Option<TextSize>) -> String {
    match text {
        Some(t) => format!(
            " Text: the smallest text is about {:.1} px tall, most text about {:.1} px; letters \
             read from about 8 px, so zoom in with bounds to read smaller text.",
            t.smallest, t.typical
        ),
        None => " No CAD text in view; pictures may contain text.".to_string(),
    }
}

/// The caption's list of marks: number, handle, kind, layer, and a text's words.
pub(crate) fn marks_legend(session: &Session, marks: &[Mark]) -> String {
    if marks.is_empty() {
        return String::new();
    }
    let rows: Vec<String> = marks
        .iter()
        .map(|m| match session.entity_info(m.top, &[]) {
            Ok(info) => {
                let mut row = format!(
                    "#{} handle {} {} {}",
                    m.n, m.top, info.kind, info.layer_short
                );
                if let Some(text) = info.plain_text.as_ref().or(info.text.as_ref()) {
                    let short: String = text.chars().take(40).collect();
                    row.push_str(&format!(" \"{short}\""));
                }
                row
            }
            Err(_) => format!("#{} handle {}", m.n, m.top),
        })
        .collect();
    format!(
        " Marks (orange badges; use the handle, never guess one): {}.",
        rows.join("; ")
    )
}

/// The sheet's layout name, with its block name when they differ.
fn sheet_label(session: &Session, block: &str) -> String {
    session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.block.eq_ignore_ascii_case(block))
        .map_or_else(
            || block.to_string(),
            |s| {
                if s.name == s.block {
                    s.name
                } else {
                    format!("{} ({})", s.name, s.block)
                }
            },
        )
}

/// Whether the corners' box overlaps `bounds`.
fn overlaps(corners: &[[f64; 2]; 4], bounds: [[f64; 2]; 2]) -> bool {
    boxes_overlap(bounds_of(corners), bounds)
}

fn boxes_overlap(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

fn bad(why: &str) -> ToolError {
    ToolError::BadInput(why.to_string())
}

/// The box around the drawn segments of `handles` (entities on the sheet) on the current sheet.
fn drawn_bounds(session: &Session, handles: &[u64]) -> Result<[[f64; 2]; 2], ToolError> {
    let batch = session.batch();
    // A block reference has one range per entity drawn inside it.
    let mut ranges: HashMap<u64, Vec<(u32, u32)>> = HashMap::new();
    for r in &batch.ranges {
        ranges.entry(r.top).or_default().push((r.first, r.count));
    }
    let mut points = Vec::new();
    for &h in handles {
        session.entity_info(h, &[])?;
        let drawn = ranges
            .get(&h)
            .ok_or_else(|| bad(&format!("Entity {h} is not drawn on the sheet shown")))?;
        for &(first, count) in drawn {
            for i in first..first + count {
                let [x0, y0, x1, y1] = batch.world_segment(i as usize);
                points.extend([[x0, y0], [x1, y1]]);
            }
        }
    }
    Ok(bounds_of(&points))
}

fn bounds_of(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let first = points.first().copied().unwrap_or_default();
    points.iter().fold([first, first], |[lo, hi], p| {
        [
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        ]
    })
}

fn with_margin([lo, hi]: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let m = (hi[0] - lo[0]).max(hi[1] - lo[1]) * MARGIN;
    [[lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]]
}

/// Widens or heightens `bounds` about its centre so a picture `width` pixels wide shows all of
/// it undistorted within 1600 pixels of height. A box with no size becomes one drawing unit.
fn fit([lo, hi]: [[f64; 2]; 2], width: u32) -> [[f64; 2]; 2] {
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (mut dx, mut dy) = (hi[0] - lo[0], hi[1] - lo[1]);
    if dx <= 0.0 && dy <= 0.0 {
        (dx, dy) = (1.0, 1.0);
    }
    let w = f64::from(width);
    // Height = width * dy / dx must stay within 1 and 1600 pixels.
    dx = dx.max(dy * w / f64::from(MAX_SIDE));
    dy = dy.max(dx / w);
    [
        [centre[0] - dx / 2.0, centre[1] - dy / 2.0],
        [centre[0] + dx / 2.0, centre[1] + dy / 2.0],
    ]
}
