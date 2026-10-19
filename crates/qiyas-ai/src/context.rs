//! What the user points Mizan at with a message: the entities selected in the window, a region
//! they marked, the view on screen, and the pictures they pasted. The window sends it with each
//! message (`ai_send`); the agent writes it into the `[state]` block and, when the selection or
//! region is new, sends a picture of it with the selection highlighted and marked.

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;

use qiyas_core::{Session, Target};
use serde::{Deserialize, Serialize};

use crate::execute::Look;
use crate::prompt::Lang;

/// Most entities listed one by one; the rest are counted by kind.
const MAX_LISTED: usize = 50;
/// Width of the picture sent with a selection or region, in pixels.
const PICTURE_WIDTH: u32 = 1000;
/// Margin round the selection or region in that picture, as a share of its size.
const PICTURE_MARGIN: f64 = 0.15;
/// Most pictures the user may attach to one message.
pub const MAX_USER_IMAGES: usize = 4;
/// Largest picture the user may attach, in base64 bytes.
pub const MAX_USER_IMAGE: usize = 6 * 1024 * 1024;
/// The kinds of picture the user may attach.
pub const USER_IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// What the user points at with a message.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnContext {
    /// The entities selected in the window: the Select tool's selection set (whole entities on
    /// the sheet), or the one entity it picked (inside blocks too, with its chain).
    pub selection: Vec<Target>,
    /// A region the user marked for Mizan.
    pub region: Option<Region>,
    /// The part of the drawing on screen, `[[min_x, min_y], [max_x, max_y]]`.
    pub view: Option<[[f64; 2]; 2]>,
    /// The selection or the region changed since the last message: send a picture of it.
    pub picture: bool,
}

/// A region the user marked: a box (two opposite corners) or a polygon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub kind: RegionKind,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    Box,
    Polygon,
}

impl Region {
    /// Its corners in order round it: a box's four, a polygon's own.
    pub fn ring(&self) -> Vec<[f64; 2]> {
        match (self.kind, self.points.as_slice()) {
            (RegionKind::Box, [a, b, ..]) => vec![
                [a[0].min(b[0]), a[1].min(b[1])],
                [a[0].max(b[0]), a[1].min(b[1])],
                [a[0].max(b[0]), a[1].max(b[1])],
                [a[0].min(b[0]), a[1].max(b[1])],
            ],
            _ => self.points.clone(),
        }
    }
}

/// A picture the user pasted or dropped into a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserImage {
    /// One of `USER_IMAGE_TYPES`.
    pub media_type: String,
    /// Base64.
    pub data: String,
}

impl UserImage {
    /// Why the agent can't take it, if it can't.
    pub fn refused(&self) -> Option<String> {
        if !USER_IMAGE_TYPES.contains(&self.media_type.as_str()) {
            return Some(format!("{} pictures can't be sent", self.media_type));
        }
        if self.data.len() > MAX_USER_IMAGE {
            return Some("A picture is too large to send".to_string());
        }
        None
    }
}

/// A user's message: what they wrote, what they point at, the pictures they attached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserMessage {
    pub text: String,
    pub context: TurnContext,
    pub images: Vec<UserImage>,
}

impl UserMessage {
    pub fn text(text: impl Into<String>) -> UserMessage {
        UserMessage {
            text: text.into(),
            ..UserMessage::default()
        }
    }
}

/// The lines `ctx` adds to the `[state]` block: the view on screen, and the selection and the
/// region with their entities (kind, layer, length and area in metres, a text's words).
pub fn describe(session: &Session, ctx: &TurnContext, lang: Lang) -> String {
    let ar = lang == Lang::Ar;
    let t = |en: &'static str, arabic: &'static str| if ar { arabic } else { en };
    let mut s = String::new();
    if let Some([lo, hi]) = ctx.view {
        let _ = writeln!(
            s,
            "{}[[{:.3}, {:.3}], [{:.3}, {:.3}]] {}",
            t("View on screen: ", "المعروض على الشاشة: "),
            lo[0],
            lo[1],
            hi[0],
            hi[1],
            t("(drawing units)", "(بوحدات الرسم)"),
        );
    }
    if !ctx.selection.is_empty() {
        let n = ctx.selection.len();
        let _ = writeln!(
            s,
            "{}",
            if ar {
                format!("حدّده المستخدم ({n} عنصر؛ «هذا» و«هذه» و«هنا» تعنيها):")
            } else {
                format!("Selected by the user ({n}; \"this\", \"these\" and \"here\" mean them):")
            }
        );
        list(&mut s, session, &ctx.selection, ar);
    }
    if let Some(region) = &ctx.region {
        let ring = region.ring();
        let to_m = session.drawing.units.to_metres();
        let area = ring_area(&ring) * to_m * to_m;
        let corners: Vec<String> = ring
            .iter()
            .map(|p| format!("[{:.3}, {:.3}]", p[0], p[1]))
            .collect();
        let inside: Vec<Target> = session
            .pick_polygon(&ring, true)
            .into_iter()
            .map(|p| Target {
                handle: p.top,
                chain: Vec::new(),
            })
            .collect();
        let kind = match (region.kind, ar) {
            (RegionKind::Box, false) => "box",
            (RegionKind::Polygon, false) => "polygon",
            (RegionKind::Box, true) => "مستطيل",
            (RegionKind::Polygon, true) => "مضلع",
        };
        let _ = writeln!(
            s,
            "{}",
            if ar {
                format!(
                    "منطقة حدّدها المستخدم ({kind}؛ الأركان {} بوحدات الرسم؛ {area:.3} م²)، فيها أو تقطعها {} عنصر («هنا» تعنيها):",
                    corners.join(" "),
                    inside.len()
                )
            } else {
                format!(
                    "Region marked by the user ({kind}; corners {} in drawing units; {area:.3} m²), {} entities in it or crossing it (\"here\" means it):",
                    corners.join(" "),
                    inside.len()
                )
            }
        );
        list(&mut s, session, &inside, ar);
    }
    s
}

/// One row per entity (the first `MAX_LISTED`), the rest counted by kind, then the totals of
/// length and area (dimensions and block references left out, as the properties palette sums).
fn list(s: &mut String, session: &Session, targets: &[Target], ar: bool) {
    let to_m = session.drawing.units.to_metres();
    let (mut length, mut area) = (0.0, 0.0);
    let mut more: BTreeMap<String, usize> = BTreeMap::new();
    let mut gone = 0;
    for (k, target) in targets.iter().enumerate() {
        let Ok(info) = session.entity_info(target.handle, &target.chain) else {
            gone += 1;
            continue;
        };
        let summed = !info.kind.starts_with("Dimension") && info.kind != "Insert";
        if summed {
            length += info.length.unwrap_or(0.0) * to_m;
            area += info.area.unwrap_or(0.0) * to_m * to_m;
        }
        if k >= MAX_LISTED {
            *more.entry(info.kind).or_default() += 1;
            continue;
        }
        let mut row = format!(
            "- handle {} {}{} {} {}",
            info.handle,
            info.kind,
            info.block_name
                .as_ref()
                .map_or_else(String::new, |b| format!(" ({b})")),
            if ar { "على" } else { "on" },
            info.layer_short
        );
        if !target.chain.is_empty() {
            let _ = write!(
                row,
                ", {} {:?} ({})",
                if ar {
                    "داخل مراجع الكتل"
                } else {
                    "inside block references"
                },
                target.chain,
                info.block_path.join(" > ")
            );
        }
        if let Some(l) = info.length {
            let _ = write!(
                row,
                ", {} {:.3} {}",
                if ar { "الطول" } else { "length" },
                l * to_m,
                if ar { "م" } else { "m" }
            );
        }
        if let Some(a) = info.area {
            let _ = write!(
                row,
                ", {} {:.3} {}",
                if ar { "المساحة" } else { "area" },
                a * to_m * to_m,
                if ar { "م²" } else { "m²" }
            );
        }
        if let Some(text) = info.plain_text.as_ref().or(info.text.as_ref()) {
            let short: String = text.chars().take(60).collect();
            let _ = write!(row, ": \"{short}\"");
        }
        let _ = writeln!(s, "{row}");
    }
    if !more.is_empty() {
        let n: usize = more.values().sum();
        let kinds: Vec<String> = more.iter().map(|(k, c)| format!("{k} {c}")).collect();
        let _ = writeln!(
            s,
            "- {} {n} {}: {}",
            if ar { "و" } else { "and" },
            if ar { "أخرى" } else { "more" },
            kinds.join(", ")
        );
    }
    if gone > 0 {
        let _ = writeln!(
            s,
            "- {gone} {}",
            if ar {
                "لم تعد في المخطط"
            } else {
                "no longer in the drawing"
            }
        );
    }
    if targets.len() - gone > 1 {
        let _ = writeln!(
            s,
            "{}",
            if ar {
                format!("المجموع: الطول {length:.3} م، المساحة {area:.3} م² (بدون الأبعاد ومراجع الكتل).")
            } else {
                format!("Totals: length {length:.3} m, area {area:.3} m² (dimensions and block references left out).")
            }
        );
    }
}

/// The picture sent with a new selection or region, and its caption: the two together with a
/// margin, the selected entities highlighted and marked first, the region dashed. `None` when
/// nothing is pointed at, `ctx.picture` is false, or it can't be drawn.
pub(crate) fn picture(session: &Session, ctx: &TurnContext) -> Option<(Vec<u8>, String)> {
    if !ctx.picture {
        return None;
    }
    let mut tops: Vec<u64> = ctx
        .selection
        .iter()
        .map(|t| t.chain.first().copied().unwrap_or(t.handle))
        .collect();
    tops.sort_unstable();
    tops.dedup();
    let ring = ctx.region.as_ref().map(Region::ring).unwrap_or_default();
    let mut points = drawn_points(session, &tops);
    points.extend(ring.iter().copied());
    let [first, rest @ ..] = points.as_slice() else {
        return None;
    };
    let (mut lo, mut hi) = (*first, *first);
    for p in rest {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let m = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * PICTURE_MARGIN).max(f64::MIN_POSITIVE);
    let look = Look {
        sheet: None,
        bounds: Some([[lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]]),
        width: PICTURE_WIDTH,
        highlight: &tops,
        region: &ring,
        marks: true,
    };
    crate::execute::picture(session, &look).ok()
}

/// The ends of every segment the sheet on screen draws for the entities `tops`.
fn drawn_points(session: &Session, tops: &[u64]) -> Vec<[f64; 2]> {
    if tops.is_empty() {
        return Vec::new();
    }
    let wanted: HashSet<u64> = tops.iter().copied().collect();
    let batch = session.batch();
    let [ox, oy] = batch.origin;
    let mut points = Vec::new();
    for r in batch.ranges.iter().filter(|r| wanted.contains(&r.top)) {
        let segments = &batch.segments[r.first as usize * 4..(r.first + r.count) as usize * 4];
        points.extend(
            segments
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[x, y]| [f64::from(*x) + ox, f64::from(*y) + oy]),
        );
    }
    points
}

/// The area inside `ring` (shoelace), in square drawing units.
fn ring_area(ring: &[[f64; 2]]) -> f64 {
    let n = ring.len();
    let twice: f64 = (0..n)
        .map(|k| {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum();
    twice.abs() / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_is_four_corners_round() {
        let r = Region {
            kind: RegionKind::Box,
            points: vec![[4.0, 3.0], [0.0, 0.0]],
        };
        assert_eq!(
            r.ring(),
            vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]]
        );
        assert!((ring_area(&r.ring()) - 12.0).abs() < 1e-12);
    }

    #[test]
    fn a_context_reads_with_any_field_left_out() {
        let ctx: TurnContext = serde_json::from_value(serde_json::json!({
            "selection": [{ "handle": 5, "chain": [] }],
            "region": { "kind": "polygon", "points": [[0, 0], [1, 0], [0, 1]] },
            "view": [[0, 0], [10, 10]],
            "picture": true
        }))
        .unwrap();
        assert_eq!(ctx.selection.len(), 1);
        assert_eq!(ctx.region.unwrap().kind, RegionKind::Polygon);
        let empty: TurnContext = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(empty, TurnContext::default());
    }

    #[test]
    fn only_known_and_small_pictures_are_taken() {
        let ok = UserImage {
            media_type: "image/png".into(),
            data: "AAAA".into(),
        };
        assert!(ok.refused().is_none());
        let pdf = UserImage {
            media_type: "application/pdf".into(),
            ..ok.clone()
        };
        assert!(pdf.refused().is_some());
        let big = UserImage {
            data: "A".repeat(MAX_USER_IMAGE + 1),
            ..ok
        };
        assert!(big.refused().is_some());
    }
}
