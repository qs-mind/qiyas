//! Plot styles: AutoCAD's colour-dependent (CTB) and named (STB) plot style tables.
//!
//! A file is a 48-byte header (`PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec`, `STBVER1`
//! for an STB), the adler32 of the zlib body, the text's length and the body's length (`u32`
//! little-endian each), then the zlib body: text of `key=value` lines and `name{` ... `}`
//! blocks, strings written with a leading `"` only, LF line ends, the text ending in a NUL the
//! length counts (CR LF, NULs and a description not in UTF-8 are read too). Plain text is read too. A CTB has 255 pens,
//! one per colour (ACI 1 to 255); an STB names its styles ("Normal" first). Screening 0 is
//! AutoCAD's way of not plotting a colour (it has no other flag). Keys Qiyas does not know are
//! kept and written back.

use std::io::{Read, Write};
use std::path::Path;

use opencadcodec::Color;
use serde::{Deserialize, Serialize};

use crate::PlotError;

const CTB_HEADER: &[u8; 48] = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec";
const STB_HEADER: &[u8; 48] = b"PIAFILEVERSION_2.0,STBVER1,compress\r\npmzlibcodec";
/// The most text a plot style file may unpack to.
const MAX_TEXT: u64 = 16 << 20;
/// The most lineweights a table may hold.
const MAX_LINEWEIGHTS: usize = 4096;
/// `color` of a pen that plots with the object's colour.
const OBJECT_COLOR: i32 = -1006632961; // 0xC3FFFFFF
/// Colour method of an explicit RGB pen colour (top byte).
const BY_COLOR: u32 = 0xC2;

/// AutoCAD's lineweights in millimetres; index 0 (0.00) means the object's own.
pub const DEFAULT_LINEWEIGHTS: [f64; 27] = [
    0.0, 0.05, 0.09, 0.10, 0.13, 0.15, 0.18, 0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.53, 0.60,
    0.65, 0.70, 0.80, 0.90, 1.00, 1.06, 1.20, 1.40, 1.58, 2.00, 2.11,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleKind {
    /// Colour-dependent: one pen per colour.
    Ctb,
    /// Named styles.
    Stb,
}

/// One pen (CTB) or named style (STB).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StyleEntry {
    pub name: String,
    pub localized_name: String,
    pub description: String,
    /// The pen's colour; `None` plots with the object's.
    pub color: Option<[u8; 3]>,
    /// Bit 0 dither, bit 1 grayscale.
    pub color_policy: i32,
    pub physical_pen: i32,
    pub virtual_pen: i32,
    /// Screening, 0 to 100: 100 plots the colour as it is, 0 not at all.
    pub screen: u8,
    pub linepattern_size: f64,
    /// 31: the object's linetype (the plot draws linetypes as the screen does).
    pub linetype: i32,
    pub adaptive_linetype: bool,
    /// Index into `PlotStyle::lineweights`; 0 plots the object's lineweight.
    pub lineweight: usize,
    pub fill_style: i32,
    pub end_style: i32,
    pub join_style: i32,
    /// Keys Qiyas does not know, with their values as written.
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// A CTB or STB plot style table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlotStyle {
    pub kind: StyleKind,
    pub description: String,
    pub scale_factor: f64,
    pub apply_factor: bool,
    /// How the editor shows lineweights: 0 millimetres, 1 inches.
    pub display_units: i32,
    pub styles: Vec<StyleEntry>,
    /// Lineweights in millimetres the pens choose from.
    pub lineweights: Vec<f64>,
    /// Top-level keys Qiyas does not know, with their values as written.
    #[serde(default)]
    pub extra: Vec<(String, String)>,
}

/// What a pen draws with: colour, lineweight (`None`: the object's) and whether it plots.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pen {
    pub rgb: [u8; 3],
    pub lineweight_mm: Option<f64>,
    pub plot: bool,
}

/// The pen without a plot style: the object's colour, white plotting black on white paper.
pub fn object_pen(rgb: [u8; 3]) -> Pen {
    Pen {
        rgb: ink(rgb),
        lineweight_mm: None,
        plot: true,
    }
}

/// White plots black: the paper is white (as the screen's light theme draws it).
fn ink(rgb: [u8; 3]) -> [u8; 3] {
    if rgb == [255, 255, 255] {
        [0, 0, 0]
    } else {
        rgb
    }
}

/// The ACI colour (1 to 255) drawn as `rgb`: the first index with exactly that colour, else the
/// nearest.
pub fn aci_of(rgb: [u8; 3]) -> u8 {
    (1..=255u8)
        .find(|&i| Color::Index(i).rgb() == Some((rgb[0], rgb[1], rgb[2])))
        .unwrap_or_else(|| {
            let i = Color::from_rgb(rgb[0], rgb[1], rgb[2]).approximate_index();
            u8::try_from(i).ok().filter(|i| *i >= 1).unwrap_or(7)
        })
}

fn bad(why: impl Into<String>) -> PlotError {
    PlotError::Style(why.into())
}

impl StyleEntry {
    /// A pen that plots as drawn.
    fn plain(name: &str) -> StyleEntry {
        StyleEntry {
            name: name.to_string(),
            localized_name: name.to_string(),
            description: String::new(),
            color: None,
            color_policy: 1,
            physical_pen: 0,
            virtual_pen: 0,
            screen: 100,
            linepattern_size: 0.5,
            linetype: 31,
            adaptive_linetype: true,
            lineweight: 0,
            fill_style: 73,
            end_style: 4,
            join_style: 5,
            extra: Vec::new(),
        }
    }
}

impl PlotStyle {
    /// A CTB whose 255 pens plot every colour as drawn.
    pub fn default_ctb() -> PlotStyle {
        PlotStyle {
            kind: StyleKind::Ctb,
            description: String::new(),
            scale_factor: 1.0,
            apply_factor: false,
            display_units: 0,
            styles: (1..=255)
                .map(|i| StyleEntry::plain(&format!("Color_{i}")))
                .collect(),
            lineweights: DEFAULT_LINEWEIGHTS.to_vec(),
            extra: Vec::new(),
        }
    }

    /// Reads a CTB or STB file.
    pub fn load(path: &Path) -> Result<PlotStyle, PlotError> {
        let data =
            std::fs::read(path).map_err(|e| bad(format!("Can't read {}: {e}", path.display())))?;
        PlotStyle::parse(&data)
    }

    /// Writes the table to `path` in AutoCAD's compressed form.
    pub fn save(&self, path: &Path) -> Result<(), PlotError> {
        std::fs::write(path, self.to_bytes())
            .map_err(|e| bad(format!("Can't write {}: {e}", path.display())))
    }

    /// Reads a plot style file's bytes: compressed (CTB or STB header) or plain text.
    pub fn parse(data: &[u8]) -> Result<PlotStyle, PlotError> {
        let (kind, text) = if data.starts_with(b"PIAFILEVERSION_") {
            let kind = if data.starts_with(CTB_HEADER) {
                StyleKind::Ctb
            } else if data.starts_with(STB_HEADER) {
                StyleKind::Stb
            } else {
                return Err(bad("Not a CTB or STB plot style file"));
            };
            if data.len() < 60 {
                return Err(bad("The plot style file is cut short"));
            }
            let mut text = Vec::new();
            flate2::read::ZlibDecoder::new(&data[60..])
                .take(MAX_TEXT)
                .read_to_end(&mut text)
                .map_err(|e| bad(format!("The plot style file is damaged: {e}")))?;
            // A description in the computer's code page (not UTF-8) is read as best it can.
            (Some(kind), String::from_utf8_lossy(&text).into_owned())
        } else {
            let text = String::from_utf8_lossy(data).into_owned();
            if !text
                .trim_start_matches('\u{feff}')
                .starts_with("description=")
            {
                return Err(bad("Not a CTB or STB plot style file"));
            }
            (None, text)
        };
        from_text(kind, &text)
    }

    /// The table in AutoCAD's compressed form.
    pub fn to_bytes(&self) -> Vec<u8> {
        // As AutoCAD writes it: the text ends with a NUL its length counts.
        let mut text = self.to_text();
        text.push('\0');
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(text.as_bytes()).expect("writing to memory");
        let body = z.finish().expect("writing to memory");
        let header = match self.kind {
            StyleKind::Ctb => CTB_HEADER,
            StyleKind::Stb => STB_HEADER,
        };
        let mut out = Vec::with_capacity(60 + body.len());
        out.extend_from_slice(header);
        out.extend(adler2::adler32_slice(&body).to_le_bytes());
        out.extend((text.len() as u32).to_le_bytes());
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    /// The pen for an object drawn `rgb`: a CTB's pen for its colour, an STB's style `named`
    /// (case-insensitive; "Normal" when it has none or the table lacks it). Screening moves the
    /// colour towards white; grayscale greys it; screen 0 does not plot.
    pub fn apply(&self, rgb: [u8; 3], named: Option<&str>) -> Pen {
        let entry = match self.kind {
            StyleKind::Ctb => self.styles.get(usize::from(aci_of(rgb)) - 1),
            StyleKind::Stb => named
                .filter(|n| !n.is_empty())
                .and_then(|n| self.styles.iter().find(|s| s.name.eq_ignore_ascii_case(n)))
                .or_else(|| {
                    self.styles
                        .iter()
                        .find(|s| s.name.eq_ignore_ascii_case("Normal"))
                }),
        };
        let Some(e) = entry else {
            return object_pen(rgb);
        };
        let mut c = e.color.unwrap_or_else(|| ink(rgb));
        if e.color_policy & 2 != 0 {
            let [r, g, b] = c.map(f64::from);
            let grey = (0.299 * r + 0.587 * g + 0.114 * b).round() as u8;
            c = [grey; 3];
        }
        let screen = f64::from(e.screen.min(100)) / 100.0;
        let rgb = c.map(|v| (255.0 - (255.0 - f64::from(v)) * screen).round() as u8);
        let lineweight_mm = (e.lineweight > 0)
            .then(|| self.lineweights.get(e.lineweight).copied())
            .flatten();
        Pen {
            rgb,
            lineweight_mm,
            plot: e.screen > 0,
        }
    }

    fn to_text(&self) -> String {
        let ctb = self.kind == StyleKind::Ctb;
        let mut t = String::new();
        let mut line = |depth: usize, s: &str| {
            t.push_str(&" ".repeat(depth));
            t.push_str(s);
            t.push('\n');
        };
        line(0, &format!("description=\"{}", self.description));
        line(0, &format!("aci_table_available={}", flag(ctb)));
        line(0, &format!("scale_factor={}", number(self.scale_factor)));
        line(0, &format!("apply_factor={}", flag(self.apply_factor)));
        line(
            0,
            &format!("custom_lineweight_display_units={}", self.display_units),
        );
        for (k, v) in &self.extra {
            line(0, &format!("{k}={v}"));
        }
        if ctb {
            line(0, "aci_table{");
            for (i, s) in self.styles.iter().enumerate() {
                line(1, &format!("{i}=\"{}", s.name));
            }
            line(0, "}");
        }
        line(0, "plot_style{");
        for (i, s) in self.styles.iter().enumerate() {
            let color = s.color.map_or(OBJECT_COLOR, |[r, g, b]| {
                (BY_COLOR << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)) as i32
            });
            line(1, &format!("{i}{{"));
            line(2, &format!("name=\"{}", s.name));
            line(2, &format!("localized_name=\"{}", s.localized_name));
            line(2, &format!("description=\"{}", s.description));
            line(2, &format!("color={color}"));
            line(2, &format!("mode_color={color}"));
            line(2, &format!("color_policy={}", s.color_policy));
            line(2, &format!("physical_pen_number={}", s.physical_pen));
            line(2, &format!("virtual_pen_number={}", s.virtual_pen));
            line(2, &format!("screen={}", s.screen));
            line(
                2,
                &format!("linepattern_size={}", number(s.linepattern_size)),
            );
            line(2, &format!("linetype={}", s.linetype));
            line(
                2,
                &format!("adaptive_linetype={}", flag(s.adaptive_linetype)),
            );
            line(2, &format!("lineweight={}", s.lineweight));
            line(2, &format!("fill_style={}", s.fill_style));
            line(2, &format!("end_style={}", s.end_style));
            line(2, &format!("join_style={}", s.join_style));
            for (k, v) in &s.extra {
                line(2, &format!("{k}={v}"));
            }
            line(1, "}");
        }
        line(0, "}");
        line(0, "custom_lineweight_table{");
        for (i, w) in self.lineweights.iter().enumerate() {
            let two = format!("{w:.2}");
            let w = if two.parse::<f64>() == Ok(*w) {
                two
            } else {
                w.to_string()
            };
            line(1, &format!("{i}={w}"));
        }
        line(0, "}");
        t
    }
}

fn flag(b: bool) -> &'static str {
    if b {
        "TRUE"
    } else {
        "FALSE"
    }
}

/// A number as the files write it: at least one decimal.
fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

/// A parsed block: `key=value` lines and nested blocks, in file order.
enum Node {
    Value(String),
    Block(Vec<(String, Node)>),
}

fn parse_block<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    nested: bool,
) -> Result<Vec<(String, Node)>, PlotError> {
    let mut out = Vec::new();
    while let Some(raw) = lines.next() {
        // AutoCAD ends the text with a NUL; lines may end in CR LF.
        let line = raw.trim_end_matches(['\r', '\0']).trim_start();
        if line
            .trim_matches(|c: char| c == '\0' || c.is_whitespace())
            .is_empty()
        {
            continue;
        }
        if line == "}" {
            return if nested {
                Ok(out)
            } else {
                Err(bad("The plot style has a stray }"))
            };
        }
        if let Some(name) = line.strip_suffix('{').filter(|n| !n.contains('=')) {
            out.push((
                name.trim().to_string(),
                Node::Block(parse_block(lines, true)?),
            ));
        } else if let Some((k, v)) = line.split_once('=') {
            out.push((k.trim().to_string(), Node::Value(v.to_string())));
        } else {
            return Err(bad(format!(
                "The plot style has a line it can't read: {line}"
            )));
        }
    }
    if nested {
        Err(bad("The plot style ends inside a block"))
    } else {
        Ok(out)
    }
}

fn string(v: &str) -> String {
    v.strip_prefix('"').unwrap_or(v).to_string()
}

fn num<T: std::str::FromStr>(key: &str, v: &str) -> Result<T, PlotError> {
    v.trim()
        .parse()
        .map_err(|_| bad(format!("{key}={v} is not a number")))
}

fn boolean(key: &str, v: &str) -> Result<bool, PlotError> {
    match v.trim().to_ascii_uppercase().as_str() {
        "TRUE" => Ok(true),
        "FALSE" => Ok(false),
        _ => Err(bad(format!("{key}={v} is not TRUE or FALSE"))),
    }
}

/// A pen colour as the files keep it: the object's, or RGB in the low 24 bits (an ACI colour
/// method 0xC3 with an index in the low byte becomes that index's colour).
fn color(v: i32) -> Option<[u8; 3]> {
    let u = v as u32;
    if v == -1 || v == OBJECT_COLOR {
        return None;
    }
    if u >> 24 == 0xC3 && u & 0xFF_FFFF <= 255 {
        let (r, g, b) = Color::Index((u & 0xFF) as u8).rgb()?;
        return Some([r, g, b]);
    }
    Some([(u >> 16) as u8, (u >> 8) as u8, u as u8])
}

fn entry(nodes: Vec<(String, Node)>) -> Result<StyleEntry, PlotError> {
    let mut e = StyleEntry::plain("");
    for (k, node) in nodes {
        let Node::Value(v) = node else {
            continue;
        };
        match k.as_str() {
            "name" => e.name = string(&v),
            "localized_name" => e.localized_name = string(&v),
            "description" => e.description = string(&v),
            "color" => e.color = color(num(&k, &v)?),
            "mode_color" => {}
            "color_policy" => e.color_policy = num(&k, &v)?,
            "physical_pen_number" => e.physical_pen = num(&k, &v)?,
            "virtual_pen_number" => e.virtual_pen = num(&k, &v)?,
            "screen" => e.screen = num::<i64>(&k, &v)?.clamp(0, 100) as u8,
            "linepattern_size" => e.linepattern_size = num(&k, &v)?,
            "linetype" => e.linetype = num(&k, &v)?,
            "adaptive_linetype" => e.adaptive_linetype = boolean(&k, &v)?,
            "lineweight" => e.lineweight = num::<i64>(&k, &v)?.max(0) as usize,
            "fill_style" => e.fill_style = num(&k, &v)?,
            "end_style" => e.end_style = num(&k, &v)?,
            "join_style" => e.join_style = num(&k, &v)?,
            _ => e.extra.push((k, v)),
        }
    }
    Ok(e)
}

fn from_text(kind: Option<StyleKind>, text: &str) -> Result<PlotStyle, PlotError> {
    let text = text.trim_start_matches('\u{feff}');
    let nodes = parse_block(&mut text.split('\n'), false)?;
    let mut style = PlotStyle::default_ctb();
    style.styles.clear();
    style.lineweights.clear();
    let mut aci = None;
    let mut weights: Vec<(usize, f64)> = Vec::new();
    let mut seen_description = false;
    for (k, node) in nodes {
        match (k.as_str(), node) {
            ("description", Node::Value(v)) => {
                style.description = string(&v);
                seen_description = true;
            }
            ("aci_table_available", Node::Value(v)) => aci = Some(boolean(&k, &v)?),
            ("scale_factor", Node::Value(v)) => style.scale_factor = num(&k, &v)?,
            ("apply_factor", Node::Value(v)) => style.apply_factor = boolean(&k, &v)?,
            ("custom_lineweight_display_units", Node::Value(v)) => {
                style.display_units = num(&k, &v)?;
            }
            ("aci_table", Node::Block(_)) => {}
            ("plot_style", Node::Block(entries)) => {
                let mut numbered = Vec::new();
                for (i, node) in entries {
                    let Node::Block(fields) = node else {
                        continue;
                    };
                    numbered.push((num::<usize>("plot_style", &i)?, entry(fields)?));
                }
                numbered.sort_by_key(|(i, _)| *i);
                style.styles = numbered.into_iter().map(|(_, e)| e).collect();
            }
            ("custom_lineweight_table", Node::Block(entries)) => {
                for (i, node) in entries {
                    if let Node::Value(v) = node {
                        weights.push((num(&k, &i)?, num(&k, &v)?));
                    }
                }
            }
            (_, Node::Value(v)) => style.extra.push((k, v)),
            // A block Qiyas does not know is left out.
            (_, Node::Block(_)) => {}
        }
    }
    if !seen_description {
        return Err(bad("Not a CTB or STB plot style file"));
    }
    style.kind = kind.unwrap_or(if aci.unwrap_or(false) {
        StyleKind::Ctb
    } else {
        StyleKind::Stb
    });
    let count = weights
        .iter()
        .map(|(i, _)| i + 1)
        .filter(|c| *c <= MAX_LINEWEIGHTS)
        .max()
        .unwrap_or(0);
    style.lineweights = (0..count.max(DEFAULT_LINEWEIGHTS.len()))
        .map(|i| DEFAULT_LINEWEIGHTS.get(i).copied().unwrap_or(0.0))
        .collect();
    for (i, w) in weights {
        if let Some(slot) = style.lineweights.get_mut(i) {
            *slot = w;
        }
    }
    if style.kind == StyleKind::Ctb {
        if style.styles.len() > 255 {
            return Err(bad("A CTB has 255 pens"));
        }
        for i in style.styles.len()..255 {
            style
                .styles
                .push(StyleEntry::plain(&format!("Color_{}", i + 1)));
        }
    } else if style.styles.is_empty() {
        style.styles.push(StyleEntry::plain("Normal"));
    }
    Ok(style)
}
