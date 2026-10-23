//! What dimensions, leaders and tolerances draw. A dimension's geometry is generated from its
//! definition points and its dimension style, the way AutoCAD fills a dimension's `*D` block:
//! extension lines, the dimension line (or arc), closed filled arrowheads as 2D solids and the
//! text as a TEXT. Qiyas draws a dimension that has no block from this, and writes it into the
//! `*D` block of the dimensions it makes or edits. Everything is on layer "0" in ByBlock colour,
//! so it takes the dimension's own.
//!
//! Sizes are the style's (DIMASZ, DIMTXT, DIMEXO, DIMEXE, DIMGAP, DIMDLI) times its DIMSCALE, or
//! the dimension's own DIMSCALE override (AutoCAD's `ACAD` `DSTYLE` xdata, group 40) when it has
//! one. Numbers follow DIMDEC, DIMLFAC, DIMRND, DIMZIN, DIMDSEP and DIMPOST (decimal units only);
//! angles DIMADEC and DIMAZIN, in degrees.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use opencadcodec::entities::mtext_format::parse_mtext;
use opencadcodec::entities::{
    Dimension, EntityCommon, EntityType, Leader, LwPolyline, Solid, Text, TextHorizontalAlignment,
    TextVerticalAlignment, Tolerance,
};
use opencadcodec::tables::DimStyle;
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{Arc, CadDocument, Color, Line, LineWeight, Vector2, Vector3};
use opencadkernel::geom2d::{linear_dimension_leader, Line as Segment};

/// The xdata application AutoCAD keeps a dimension's own style overrides under.
const ACAD: &str = "ACAD";
/// DIMSCALE's DXF group code, as a `DSTYLE` override names it.
const DIMSCALE_CODE: i16 = 40;
/// DIMTXT's DXF group code.
const DIMTXT_CODE: i16 = 140;
/// DIMPOST's DXF group code.
const DIMPOST_CODE: i16 = 3;

/// The sizes and number format a dimension, leader or tolerance is drawn with: its style's
/// values times its scale, in drawing units.
#[derive(Debug, Clone, PartialEq)]
pub struct DimLook {
    pub arrow: f64,
    pub text_height: f64,
    pub ext_offset: f64,
    pub ext_extension: f64,
    pub gap: f64,
    /// Baseline spacing (DIMDLI).
    pub increment: f64,
    pub decimals: usize,
    pub angle_decimals: usize,
    pub linear_factor: f64,
    pub rounding: f64,
    pub separator: char,
    /// DIMZIN: 4 drops a leading zero, 8 trailing zeros.
    pub zeros: i16,
    /// DIMAZIN: 1 drops a leading zero, 2 trailing zeros.
    pub angle_zeros: i16,
    /// DIMPOST: `<>` stands for the number; text without it is a suffix.
    pub post: String,
    /// The text sits above the dimension line (DIMTAD 1, 2, 3); otherwise below it (4) or in a
    /// gap in it (0).
    pub text_above: bool,
    /// DIMTAD 4: the text sits below the dimension line.
    pub text_below: bool,
    pub text_style: String,
}

impl DimLook {
    /// `style` at `scale` (0 or less, or not a number: 1).
    pub fn new(style: &DimStyle, scale: f64) -> DimLook {
        let k = if scale > 0.0 && scale.is_finite() {
            scale
        } else {
            1.0
        };
        let separator = u8::try_from(style.dimdsep)
            .ok()
            .filter(|c| c.is_ascii_graphic())
            .map_or('.', char::from);
        DimLook {
            arrow: style.dimasz * k,
            text_height: style.dimtxt * k,
            ext_offset: style.dimexo * k,
            ext_extension: style.dimexe * k,
            gap: style.dimgap.abs() * k,
            increment: style.dimdli * k,
            decimals: usize::try_from(style.dimdec).unwrap_or(0).min(8),
            angle_decimals: usize::try_from(style.dimadec).unwrap_or(0).min(8),
            linear_factor: if style.dimlfac.is_finite() && style.dimlfac != 0.0 {
                style.dimlfac
            } else {
                1.0
            },
            rounding: style.dimrnd.max(0.0),
            separator,
            zeros: style.dimzin,
            angle_zeros: style.dimazin,
            post: style.dimpost.clone(),
            text_above: !matches!(style.dimtad, 0 | 4),
            text_below: style.dimtad == 4,
            text_style: if style.dimtxsty.is_empty() {
                "Standard".to_string()
            } else {
                style.dimtxsty.clone()
            },
        }
    }

    /// The look of `dim` in `doc`: its style (Standard, or AutoCAD's defaults, when the drawing
    /// has no such style) at its own DIMSCALE override or the style's.
    pub fn of(doc: &CadDocument, dim: &Dimension) -> DimLook {
        let base = dim.base();
        DimLook::named(doc, &base.style_name, &base.common)
    }

    /// The look of an annotation in the style `name` whose own data is `common`.
    pub fn named(doc: &CadDocument, name: &str, common: &EntityCommon) -> DimLook {
        let style = style_named(doc, name);
        let scale = dimscale(common).unwrap_or(style.dimscale);
        let mut look = DimLook::new(&style, scale);
        // Its own text height (a DIMTXT override), at the same scale.
        if let Some(height) = dimtxt(common) {
            let k = if scale > 0.0 && scale.is_finite() {
                scale
            } else {
                1.0
            };
            look.text_height = height * k;
        }
        // Its own suffix (a DIMPOST override, which may be empty: no suffix).
        if let Some(post) = dimpost(common) {
            look.post = post;
        }
        // The text style is held by handle in a DWG.
        if !style.dimtxsty_handle.is_null() {
            if let Some(s) = doc
                .text_styles
                .iter()
                .find(|s| s.handle == style.dimtxsty_handle)
            {
                look.text_style = s.name.clone();
            }
        }
        look
    }
}

/// The dimension style `name` in `doc`, else its Standard, else AutoCAD's defaults.
pub fn style_named(doc: &CadDocument, name: &str) -> DimStyle {
    doc.dim_styles
        .get(name)
        .or_else(|| doc.dim_styles.get("Standard"))
        .cloned()
        .unwrap_or_else(DimStyle::standard)
}

/// The DIMSCALE an annotation overrides its style's with (`ACAD` `DSTYLE` xdata), if any.
pub fn dimscale(common: &EntityCommon) -> Option<f64> {
    dstyle_value(common, DIMSCALE_CODE)
}

/// The DIMTXT (text height, before DIMSCALE) a dimension overrides its style's with, if any.
pub fn dimtxt(common: &EntityCommon) -> Option<f64> {
    dstyle_value(common, DIMTXT_CODE)
}

/// The `(group code, value)` overrides of the `ACAD` `DSTYLE` of `common`, in order. Each is a
/// 1070 code followed by its value, so a code is read only where one starts: a value that is
/// itself a small integer (DIMTAD's 1070) is never taken for a code.
fn dstyle_pairs(common: &EntityCommon) -> Vec<(i16, XDataValue)> {
    let mut pairs = Vec::new();
    let Some(record) = common.extended_data.get_record(ACAD) else {
        return pairs;
    };
    let values = &record.values;
    let Some(at) = values
        .iter()
        .position(|v| matches!(v, XDataValue::String(s) if s.eq_ignore_ascii_case("DSTYLE")))
    else {
        return pairs;
    };
    let mut rest = values[at + 1..].iter();
    while let Some(v) = rest.next() {
        match v {
            XDataValue::ControlString(s) if s == "}" => break,
            XDataValue::Integer16(c) => {
                if let Some(value) = rest.next() {
                    pairs.push((*c, value.clone()));
                }
            }
            _ => {}
        }
    }
    pairs
}

/// The positive number the `DSTYLE` override `code` gives, if any.
fn dstyle_value(common: &EntityCommon, code: i16) -> Option<f64> {
    match dstyle_pairs(common)
        .into_iter()
        .find(|(c, _)| *c == code)?
        .1
    {
        XDataValue::Real(r) | XDataValue::Distance(r) | XDataValue::ScaleFactor(r)
            if r.is_finite() && r > 0.0 =>
        {
            Some(r)
        }
        _ => None,
    }
}

/// Gives an annotation its own DIMSCALE (an `ACAD` `DSTYLE` override, as AutoCAD writes one),
/// replacing any overrides it had.
pub fn set_dimscale(common: &mut EntityCommon, scale: f64) {
    let mut record = ExtendedDataRecord::new(ACAD);
    for v in [
        XDataValue::String("DSTYLE".to_string()),
        XDataValue::ControlString("{".to_string()),
        XDataValue::Integer16(DIMSCALE_CODE),
        XDataValue::Real(scale),
        XDataValue::ControlString("}".to_string()),
    ] {
        record.add_value(v);
    }
    common.extended_data.upsert_record(record);
}

/// Gives a dimension its own DIMTXT (text height before DIMSCALE), keeping its other `DSTYLE`
/// overrides.
pub fn set_dimtxt(common: &mut EntityCommon, height: f64) {
    set_dstyle(common, DIMTXT_CODE, Some(XDataValue::Real(height)));
}

/// The DIMPOST a dimension overrides its style's with (`ACAD` `DSTYLE` xdata, group 3), if any;
/// an empty one means no suffix.
pub fn dimpost(common: &EntityCommon) -> Option<String> {
    match dstyle_pairs(common)
        .into_iter()
        .find(|(c, _)| *c == DIMPOST_CODE)?
        .1
    {
        XDataValue::String(s) => Some(s),
        _ => None,
    }
}

/// Gives a dimension its own DIMPOST (`<>` stands for the number; text without it is a suffix),
/// keeping its other `DSTYLE` overrides: how AutoCAD stores a suffix set on one dimension.
pub fn set_dimpost(common: &mut EntityCommon, post: &str) {
    set_dstyle(
        common,
        DIMPOST_CODE,
        Some(XDataValue::String(post.to_string())),
    );
}

/// Drops a dimension's own DIMPOST, keeping its other `DSTYLE` overrides: its style's applies.
pub fn clear_dimpost(common: &mut EntityCommon) {
    set_dstyle(common, DIMPOST_CODE, None);
}

/// Sets (or, with `None`, drops) the `DSTYLE` override `code`, keeping the others; a `DSTYLE`
/// left without any override is dropped.
fn set_dstyle(common: &mut EntityCommon, code: i16, value: Option<XDataValue>) {
    let mut pairs = dstyle_pairs(common);
    pairs.retain(|(c, _)| *c != code);
    pairs.extend(value.map(|v| (code, v)));
    if pairs.is_empty() {
        common.extended_data.remove_record(ACAD);
        return;
    }
    let mut record = ExtendedDataRecord::new(ACAD);
    record.add_value(XDataValue::String("DSTYLE".to_string()));
    record.add_value(XDataValue::ControlString("{".to_string()));
    for (code, value) in pairs {
        record.add_value(XDataValue::Integer16(code));
        record.add_value(value);
    }
    record.add_value(XDataValue::ControlString("}".to_string()));
    common.extended_data.upsert_record(record);
}

/// `v` written with `decimals` places, `zeros` as DIMZIN (4 leading, 8 trailing) and `separator`.
fn number(v: f64, decimals: usize, leading: bool, trailing: bool, separator: char) -> String {
    let mut s = format!("{:.*}", decimals, v.abs());
    if trailing && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if leading && s.starts_with("0.") {
        s.remove(0);
    }
    let zero = s.chars().all(|c| c == '0' || c == '.');
    if v < 0.0 && !zero {
        s.insert(0, '-');
    }
    s.replace('.', &separator.to_string())
}

/// A distance as the look writes it: DIMLFAC, DIMRND, DIMDEC, DIMZIN, DIMDSEP and DIMPOST.
pub fn format_measure(v: f64, look: &DimLook) -> String {
    let mut v = v * look.linear_factor;
    if look.rounding > 0.0 {
        v = (v / look.rounding).round() * look.rounding;
    }
    let s = number(
        v,
        look.decimals,
        look.zeros & 4 != 0,
        look.zeros & 8 != 0,
        look.separator,
    );
    if look.post.is_empty() {
        s
    } else if look.post.contains("<>") {
        look.post.replace("<>", &s)
    } else {
        format!("{s}{}", look.post)
    }
}

/// An angle in degrees as the look writes it (DIMADEC, DIMAZIN).
fn format_angle(degrees: f64, look: &DimLook) -> String {
    let s = number(
        degrees,
        look.angle_decimals,
        look.angle_zeros & 1 != 0,
        look.angle_zeros & 2 != 0,
        look.separator,
    );
    format!("{s}%%d")
}

/// The text `dim` shows: its measurement as the look writes it (with R or the diameter sign),
/// or its override with `<>` standing for the measurement, as plain text. A single space hides
/// the text: empty.
pub fn dimension_text(dim: &Dimension, look: &DimLook) -> String {
    let m = dim.measurement();
    let m = if m.is_finite() { m } else { 0.0 };
    let measured = match dim {
        Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => format_angle(m, look),
        Dimension::Radius(_) | Dimension::LargeRadial(_) => format!("R{}", format_measure(m, look)),
        Dimension::Diameter(_) => format!("%%c{}", format_measure(m, look)),
        _ => format_measure(m, look),
    };
    match dim.base().text_override() {
        None | Some("") => measured,
        Some(" ") => String::new(),
        Some(text) => {
            let text = text.replace("<>", &measured);
            if text.contains(['\\', '{', '}']) {
                parse_mtext(&text, true).to_plain_text()
            } else {
                text
            }
        }
    }
}

/// What a dimension draws, and where its text's middle is (none when it shows no text).
#[derive(Debug, Clone, PartialEq)]
pub struct DimGeometry {
    pub entities: Vec<EntityType>,
    pub text_at: Option<[f64; 2]>,
}

type P = [f64; 2];

fn xy(v: &Vector3) -> P {
    [v.x, v.y]
}
fn v3(p: P) -> Vector3 {
    Vector3::new(p[0], p[1], 0.0)
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P, k: f64) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn len(a: P) -> f64 {
    a[0].hypot(a[1])
}
fn unit(a: P) -> Option<P> {
    let l = len(a);
    (l > 1e-12 && l.is_finite()).then(|| mul(a, 1.0 / l))
}
fn polar(a: f64) -> P {
    [a.cos(), a.sin()]
}
fn left(a: P) -> P {
    [-a[1], a[0]]
}

/// An angle turned to read from left to right (in (-90°, 90°]).
fn readable(a: f64) -> f64 {
    let a = (a + PI).rem_euclid(TAU) - PI;
    if a > FRAC_PI_2 + 1e-9 || a <= -FRAC_PI_2 + 1e-9 {
        (a + TAU).rem_euclid(TAU) - PI
    } else {
        a
    }
}

/// A rough width of `text` drawn `height` tall, as text boxes are sized for picking.
fn text_width(text: &str, height: f64) -> f64 {
    text.chars().count() as f64 * height * 0.6
}

/// Gives a generated part layer "0" and ByBlock colour and weight: it takes its owner's.
fn by_block(mut e: EntityType) -> EntityType {
    let c = e.common_mut();
    c.layer = "0".to_string();
    c.color = Color::ByBlock;
    c.line_weight = LineWeight::ByBlock;
    e
}

/// Collects the parts of an annotation.
struct Parts<'l> {
    look: &'l DimLook,
    out: Vec<EntityType>,
    measured_width: Option<f64>,
}

impl Parts<'_> {
    fn width(&self, text: &str, height: f64) -> f64 {
        self.measured_width
            .unwrap_or_else(|| text_width(text, height))
    }
    fn line(&mut self, a: P, b: P) {
        if len(sub(a, b)) > 1e-12 {
            self.out
                .push(by_block(EntityType::Line(Line::from_points(v3(a), v3(b)))));
        }
    }

    /// A closed filled arrowhead with its tip at `tip`, its body along `back` (a unit vector).
    fn arrow(&mut self, tip: P, back: P) {
        let size = self.look.arrow;
        if size.is_nan() || size <= 0.0 {
            return;
        }
        let base = add(tip, mul(back, size));
        let side = mul(left(back), size / 6.0);
        let solid = Solid::triangle(v3(tip), v3(add(base, side)), v3(sub(base, side)));
        self.out.push(by_block(EntityType::Solid(solid)));
    }

    /// The text `value` with its middle at `at`, turned `rotation` radians.
    fn text(&mut self, value: &str, at: P, rotation: f64, height: f64) {
        if value.is_empty() || height.is_nan() || height <= 0.0 {
            return;
        }
        let mut t = Text::with_value(value, v3(at));
        t.height = height;
        t.rotation = rotation;
        t.style = self.look.text_style.clone();
        t.horizontal_alignment = TextHorizontalAlignment::Center;
        t.vertical_alignment = TextVerticalAlignment::Middle;
        t.alignment_point = Some(v3(at));
        self.out.push(by_block(EntityType::Text(t)));
    }

    fn arc(&mut self, centre: P, r: f64, start: f64, end: f64) {
        if r > 1e-12 {
            let arc = Arc::from_center_radius_angles(v3(centre), r, start, end);
            self.out.push(by_block(EntityType::Arc(arc)));
        }
    }

    /// A closed rectangle with corners `a`, `b`, `c`, `d`.
    fn frame(&mut self, corners: [P; 4]) {
        let mut pl = LwPolyline::new();
        for p in corners {
            pl.add_point(Vector2::new(p[0], p[1]));
        }
        pl.is_closed = true;
        self.out.push(by_block(EntityType::LwPolyline(pl)));
    }
}

/// What `dim` draws in `doc`, generated from its definition points and its style.
pub fn dimension_entities(doc: &CadDocument, dim: &Dimension) -> DimGeometry {
    dimension_entities_with_width(doc, dim, None)
}

/// Generate dimension geometry with the width measured by the host text shaper.
pub fn dimension_entities_with_width(
    doc: &CadDocument,
    dim: &Dimension,
    width: Option<f64>,
) -> DimGeometry {
    let look = DimLook::of(doc, dim);
    let text = dimension_text(dim, &look);
    let mut parts = Parts {
        look: &look,
        out: Vec::new(),
        measured_width: None,
    };
    parts.measured_width = width;
    let base = dim.base();
    let placed = base
        .text_user_positioned
        .then(|| xy(&base.text_middle_point));
    let text_at = match dim {
        Dimension::Linear(d) => linear(
            &mut parts,
            xy(&d.first_point),
            xy(&d.second_point),
            xy(&d.definition_point),
            polar(d.rotation),
            &text,
            placed,
            [base.flip_arrow1, base.flip_arrow2],
        ),
        Dimension::Aligned(d) => {
            let along = unit(sub(xy(&d.second_point), xy(&d.first_point))).unwrap_or([1.0, 0.0]);
            linear(
                &mut parts,
                xy(&d.first_point),
                xy(&d.second_point),
                xy(&d.definition_point),
                along,
                &text,
                placed,
                [base.flip_arrow1, base.flip_arrow2],
            )
        }
        Dimension::Radius(d) => radial(
            &mut parts,
            xy(&d.angle_vertex),
            xy(&d.definition_point),
            &text,
            placed,
        ),
        Dimension::LargeRadial(d) => radial(
            &mut parts,
            xy(&d.override_center),
            xy(&d.chord_point),
            &text,
            placed,
        ),
        Dimension::Diameter(d) => {
            let (a, b) = (xy(&d.angle_vertex), xy(&d.definition_point));
            parts.line(a, b);
            let mut at = None;
            if let Some(e) = unit(sub(b, a)) {
                parts.arrow(a, e);
                parts.arrow(b, mul(e, -1.0));
                let r = readable(e[1].atan2(e[0]));
                let mid = mul(add(a, b), 0.5);
                let up = left(polar(r));
                let p = placed.unwrap_or(add(mid, mul(up, look.gap + look.text_height / 2.0)));
                parts.text(&text, p, r, look.text_height);
                at = Some(p);
            }
            at
        }
        Dimension::Angular3Pt(d) => angular(
            &mut parts,
            xy(&d.angle_vertex),
            &[xy(&d.first_point), xy(&d.second_point)],
            [
                sub(xy(&d.first_point), xy(&d.angle_vertex)),
                sub(xy(&d.second_point), xy(&d.angle_vertex)),
            ],
            xy(&d.definition_point),
            &text,
            placed,
        ),
        Dimension::Angular2Ln(d) => {
            let (a0, a1) = (xy(&d.first_point), xy(&d.second_point));
            let (b0, b1) = (xy(&d.angle_vertex), xy(&d.definition_point));
            let (da, db) = (sub(a1, a0), sub(b1, b0));
            crossing(a0, da, b0, db).and_then(|v| {
                angular(
                    &mut parts,
                    v,
                    &[a0, a1, b0, b1],
                    [da, db],
                    xy(&d.dimension_arc),
                    &text,
                    placed,
                )
            })
        }
        Dimension::Ordinate(d) => {
            let [start, elbow, landing, end] = d
                .leader_polyline(look.arrow, look.ext_offset, None)
                .map(|p| xy(&p));
            parts.line(start, elbow);
            parts.line(elbow, landing);
            parts.line(landing, end);
            let dir = unit(sub(end, landing)).unwrap_or([1.0, 0.0]);
            let w = parts.width(&text, look.text_height);
            let r = readable(dir[1].atan2(dir[0]));
            let p = placed.unwrap_or(add(end, mul(dir, look.gap + w / 2.0)));
            parts.text(&text, p, r, look.text_height);
            Some(p)
        }
        Dimension::Arc(d) => {
            let c = xy(&d.center_point);
            let (q1, q2) = (xy(&d.first_extension_point), xy(&d.second_extension_point));
            let r = len(sub(xy(&d.definition_point), c));
            let (s, e) = (
                sub(q1, c)[1].atan2(sub(q1, c)[0]),
                sub(q2, c)[1].atan2(sub(q2, c)[0]),
            );
            arc_dimension(&mut parts, c, r, s, e, &[q1, q2], &text, placed)
        }
    };
    DimGeometry {
        entities: parts.out,
        text_at: text_at.filter(|_| !text.is_empty()),
    }
}

/// Where the lines `a0 + t·da` and `b0 + s·db` cross; `None` when they are parallel.
fn crossing(a0: P, da: P, b0: P, db: P) -> Option<P> {
    let den = da[0] * db[1] - da[1] * db[0];
    if den.abs() <= 1e-12 * len(da) * len(db) {
        return None;
    }
    let t = ((b0[0] - a0[0]) * db[1] - (b0[1] - a0[1]) * db[0]) / den;
    Some(add(a0, mul(da, t)))
}

/// A linear or aligned dimension of `p1` to `p2` along `axis`, its line through `d`.
#[allow(clippy::too_many_arguments)]
fn linear(
    parts: &mut Parts,
    p1: P,
    p2: P,
    d: P,
    axis: P,
    text: &str,
    placed: Option<P>,
    flipped: [bool; 2],
) -> Option<P> {
    let look = parts.look;
    let n = left(axis);
    let a1 = add(p1, mul(n, dot(sub(d, p1), n)));
    let a2 = add(p2, mul(n, dot(sub(d, p2), n)));
    for (p, a) in [(p1, a1), (p2, a2)] {
        if let Some(e) = unit(sub(a, p)) {
            parts.line(
                add(p, mul(e, look.ext_offset)),
                add(a, mul(e, look.ext_extension)),
            );
        }
    }
    let along = unit(sub(a2, a1)).unwrap_or(axis);
    let r = readable(along[1].atan2(along[0]));
    let mid = mul(add(a1, a2), 0.5);
    let h = look.text_height;
    let w = parts.width(text, h);
    let at = match placed {
        Some(p) => p,
        None if look.text_above => add(mid, mul(left(polar(r)), look.gap + h / 2.0)),
        None if look.text_below => sub(mid, mul(left(polar(r)), look.gap + h / 2.0)),
        None => mid,
    };
    // Text in the line (DIMTAD 0) breaks it.
    let half = w / 2.0 + look.gap;
    let span = len(sub(a2, a1));
    let in_line = !look.text_above && !look.text_below;
    if placed.is_none() && in_line && !text.is_empty() && 2.0 * half < span {
        parts.line(a1, sub(mid, mul(along, half)));
        parts.line(add(mid, mul(along, half)), a2);
    } else {
        parts.line(a1, a2);
    }
    if span > 1e-12 {
        for (tip, back, flip) in [(a1, along, flipped[0]), (a2, mul(along, -1.0), flipped[1])] {
            if flip {
                // Outside the extension line, pointing in, with a tail.
                let out = mul(back, -1.0);
                parts.arrow(tip, out);
                parts.line(tip, add(tip, mul(out, 2.0 * look.arrow)));
            } else {
                parts.arrow(tip, back);
            }
        }
    }
    // Text moved past an extension line: the dimension line runs on under it.
    if let Some(p) = placed {
        let measured = Segment { start: a1, end: a2 };
        if let Some((under, true)) =
            linear_dimension_leader(measured, d, along, p, [w / 2.0, h / 2.0], r)
        {
            parts.line(under.start, under.end);
        }
    }
    parts.text(text, at, r, h);
    Some(at)
}

/// A radius (or jogged radius) from `c` to the point `p` on the curve: the line from the centre
/// out past the curve to the text, the arrow at `p`.
fn radial(parts: &mut Parts, c: P, p: P, text: &str, placed: Option<P>) -> Option<P> {
    let look = parts.look;
    let e = unit(sub(p, c))?;
    let landing = add(p, mul(e, 2.0 * look.arrow));
    parts.line(c, landing);
    parts.arrow(p, mul(e, -1.0));
    let w = parts.width(text, look.text_height);
    let side = if e[0] < 0.0 { -1.0 } else { 1.0 };
    let at = placed.unwrap_or(add(landing, [side * (look.gap + w / 2.0), 0.0]));
    parts.text(text, at, 0.0, look.text_height);
    Some(at)
}

/// The start and end angles (counter-clockwise) of the sector the rays `dirs` (each both ways
/// for two lines, as the codec measures) bound around `arc`'s direction.
fn sector(dirs: [P; 2], toward: P) -> Option<(f64, f64)> {
    let ang = |d: P| d[1].atan2(d[0]).rem_euclid(TAU);
    unit(dirs[0])?;
    unit(dirs[1])?;
    let mut bounds = [
        ang(dirs[0]),
        (ang(dirs[0]) + PI).rem_euclid(TAU),
        ang(dirs[1]),
        (ang(dirs[1]) + PI).rem_euclid(TAU),
    ];
    bounds.sort_by(f64::total_cmp);
    let m = ang(toward);
    (0..4).find_map(|i| {
        let start = bounds[i];
        let end = if i + 1 < 4 {
            bounds[i + 1]
        } else {
            bounds[0] + TAU
        };
        let m = if m + 1e-12 < start { m + TAU } else { m };
        (m >= start - 1e-12 && m <= end + 1e-12).then_some((start, end))
    })
}

/// An angular dimension around `v` between the rays along `dirs`, its arc through `arc`;
/// `points` are the definition points the extension lines start from.
fn angular(
    parts: &mut Parts,
    v: P,
    points: &[P],
    dirs: [P; 2],
    arc: P,
    text: &str,
    placed: Option<P>,
) -> Option<P> {
    let (s, e) = sector(dirs, sub(arc, v))?;
    let r = len(sub(arc, v));
    arc_dimension(parts, v, r, s, e, points, text, placed)
}

/// A dimension arc around `c`, radius `r`, from `s` to `e` (counter-clockwise), with extension
/// lines out from the `points` lying on its end rays, arrows at its ends and the text outside
/// its middle.
#[allow(clippy::too_many_arguments)]
fn arc_dimension(
    parts: &mut Parts,
    c: P,
    r: f64,
    s: f64,
    e: f64,
    points: &[P],
    text: &str,
    placed: Option<P>,
) -> Option<P> {
    let look = parts.look;
    if r.is_nan() || r <= 1e-12 {
        return None;
    }
    let e = if e <= s { e + TAU } else { e };
    parts.arc(c, r, s, e);
    for a in [s, e] {
        let dir = polar(a);
        let reach = points
            .iter()
            .map(|&p| sub(p, c))
            .filter(|q| {
                let l = len(*q);
                l > 1e-12 && (q[0] * dir[1] - q[1] * dir[0]).abs() <= 1e-6 * l && dot(*q, dir) > 0.0
            })
            .map(|q| dot(q, dir))
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |m| m.max(d))));
        let from = reach.unwrap_or(0.0);
        if from + look.ext_offset < r {
            parts.line(
                add(c, mul(dir, from + look.ext_offset)),
                add(c, mul(dir, r + look.ext_extension)),
            );
        } else if from > r + look.ext_offset {
            // The point lies beyond the arc: the extension line comes back in to it.
            parts.line(
                add(c, mul(dir, from - look.ext_offset)),
                add(c, mul(dir, r - look.ext_extension)),
            );
        }
    }
    let tip = |a: f64| add(c, mul(polar(a), r));
    parts.arrow(tip(s), left(polar(s)));
    parts.arrow(tip(e), mul(left(polar(e)), -1.0));
    let m = (s + e) / 2.0;
    let at = placed.unwrap_or(add(c, mul(polar(m), r + look.gap + look.text_height / 2.0)));
    parts.text(text, at, readable(m - FRAC_PI_2), look.text_height);
    Some(at)
}

/// What a leader draws: its line through its vertices and, when it has one, the arrowhead at its
/// first vertex, sized by its dimension style.
pub fn leader_entities(doc: &CadDocument, leader: &Leader) -> Vec<EntityType> {
    let look = DimLook::named(doc, &leader.dimension_style, &leader.common);
    let mut parts = Parts {
        look: &look,
        out: Vec::new(),
        measured_width: None,
    };
    let points: Vec<P> = leader.vertices.iter().map(xy).collect();
    if points.len() >= 2 {
        let mut pl = LwPolyline::new();
        for p in &points {
            pl.add_point(Vector2::new(p[0], p[1]));
        }
        parts.out.push(by_block(EntityType::LwPolyline(pl)));
        if leader.arrow_enabled {
            if let Some(back) = unit(sub(points[1], points[0])) {
                parts.arrow(points[0], back);
            }
        }
    }
    parts.out
}

/// What a tolerance (feature control frame) draws: a box per field (fields split by `%%v`, rows
/// by `^J`, empty ones left out) holding its text, from its insertion point (the middle of the
/// first row's left side) along its direction. GDT symbols show as their letters.
pub fn tolerance_entities(doc: &CadDocument, tol: &Tolerance) -> Vec<EntityType> {
    let look = DimLook::named(doc, &tol.dimension_style_name, &tol.common);
    let h = look.text_height;
    let mut parts = Parts {
        look: &look,
        out: Vec::new(),
        measured_width: None,
    };
    if h.is_nan() || h <= 0.0 {
        return parts.out;
    }
    let along = unit([tol.direction.x, tol.direction.y]).unwrap_or([1.0, 0.0]);
    let up = left(along);
    let rotation = along[1].atan2(along[0]);
    let origin = xy(&tol.insertion_point);
    let row_height = 2.0 * h;
    let at = |x: f64, y: f64| add(origin, add(mul(along, x), mul(up, y)));
    for (row, line) in tol.text.split("^J").enumerate() {
        let top = row_height / 2.0 - row as f64 * row_height;
        let mut x = 0.0;
        for field in line.split("%%v") {
            let plain = if field.contains(['\\', '{', '}']) {
                parse_mtext(field, true).to_plain_text()
            } else {
                field.to_string()
            };
            let plain = plain.trim();
            if plain.is_empty() {
                continue;
            }
            let w = (text_width(plain, h) + 2.0 * look.gap.max(h / 2.0)).max(row_height);
            parts.frame([
                at(x, top),
                at(x + w, top),
                at(x + w, top - row_height),
                at(x, top - row_height),
            ]);
            parts.text(plain, at(x + w / 2.0, top - row_height / 2.0), rotation, h);
            x += w;
        }
    }
    parts.out
}
