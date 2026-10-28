//! Text in the PDF. Each font a text uses becomes an embedded TrueType subset (Type0 over a
//! CIDFontType2, Identity-H, the subset's glyph ids as CIDs) with a ToUnicode map built from the
//! text the glyphs were shaped from, so the PDF's text can be searched and copied. A text whose
//! glyphs don't all map back to its characters, or whose font can't be embedded (CFF outlines,
//! embedding restricted, a subset that fails), is drawn as its glyphs' outlines instead.

use std::collections::{BTreeMap, HashMap};

use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use qiyas_geom::Xform;
use qiyas_render::{LaidText, CAP_EM};
use subsetter::GlyphRemapper;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::content::{fmt, Content};

/// Where a font's file comes from: its id in the session's text engine.
pub type FontBytes<'a> = dyn Fn(u16) -> Option<(&'static [u8], u32)> + 'a;

struct Font {
    face: Option<Face<'static>>,
    data: &'static [u8],
    index: u32,
    /// TrueType outlines and embedding allowed.
    embeddable: bool,
    remapper: GlyphRemapper,
    /// Characters per original glyph id, for ToUnicode (the first seen).
    unicode: HashMap<u16, String>,
    subset: Option<Vec<u8>>,
}

#[derive(Default)]
pub struct FontSet {
    fonts: BTreeMap<u16, Font>,
}

/// Per glyph of `text`, the characters it stands for: the first glyph of each cluster with an
/// advance (else its first) gets the cluster's characters, the others (marks drawn apart, such
/// as Arabic dots) none. `None` when a glyph can't be mapped back: the missing glyph, or a
/// cluster that isn't a character of the text.
pub fn glyph_chars(
    text: &LaidText,
    advance: impl Fn(u16, u16) -> u16,
) -> Option<Vec<Option<String>>> {
    let s = text.text.as_str();
    let mut starts: Vec<usize> = Vec::with_capacity(text.glyphs.len());
    let mut owner: HashMap<usize, usize> = HashMap::new();
    for (i, g) in text.glyphs.iter().enumerate() {
        let c = g.cluster as usize;
        if g.glyph == 0 || c >= s.len() || !s.is_char_boundary(c) {
            return None;
        }
        starts.push(c);
        match owner.get(&c) {
            None => {
                owner.insert(c, i);
            }
            Some(&j) => {
                let first = &text.glyphs[j];
                if advance(first.font, first.glyph) == 0 && advance(g.font, g.glyph) > 0 {
                    owner.insert(c, i);
                }
            }
        }
    }
    starts.sort_unstable();
    starts.dedup();
    let mut out = vec![None; text.glyphs.len()];
    for (c, i) in owner {
        let next = starts.partition_point(|&x| x <= c);
        let end = starts.get(next).copied().unwrap_or(s.len());
        let chars: String = s[c..end].chars().filter(|ch| !ch.is_control()).collect();
        if !chars.is_empty() {
            out[i] = Some(chars);
        }
    }
    Some(out)
}

impl Font {
    fn load(data: &'static [u8], index: u32) -> Font {
        let face = Face::parse(data, index).ok();
        let embeddable = face.as_ref().is_some_and(|f| {
            f.tables().glyf.is_some()
                && f.permissions() != Some(ttf_parser::Permissions::Restricted)
                && f.is_subsetting_allowed()
        });
        Font {
            face,
            data,
            index,
            embeddable,
            remapper: GlyphRemapper::new(),
            unicode: HashMap::new(),
            subset: None,
        }
    }

    fn advance(&self, glyph: u16) -> u16 {
        self.face
            .as_ref()
            .and_then(|f| f.glyph_hor_advance(GlyphId(glyph)))
            .unwrap_or(0)
    }
}

impl FontSet {
    fn load(&mut self, id: u16, bytes: &FontBytes) -> bool {
        if let std::collections::btree_map::Entry::Vacant(slot) = self.fonts.entry(id) {
            let Some((data, index)) = bytes(id) else {
                return false;
            };
            slot.insert(Font::load(data, index));
        }
        true
    }

    fn advance(&self, font: u16, glyph: u16) -> u16 {
        self.fonts.get(&font).map_or(0, |f| f.advance(glyph))
    }

    /// Takes in a text to draw (before `finish`): its glyphs and characters go into its fonts'
    /// subsets when it can be drawn as text.
    pub fn prepare(&mut self, text: &LaidText, bytes: &FontBytes) {
        for g in &text.glyphs {
            if !self.load(g.font, bytes) {
                return;
            }
        }
        if !text
            .glyphs
            .iter()
            .all(|g| self.fonts.get(&g.font).is_some_and(|f| f.embeddable))
        {
            return;
        }
        let Some(chars) = glyph_chars(text, |f, g| self.advance(f, g)) else {
            return;
        };
        for (g, c) in text.glyphs.iter().zip(chars) {
            let font = self.fonts.get_mut(&g.font).expect("loaded above");
            font.remapper.remap(g.glyph);
            if let Some(c) = c {
                font.unicode.entry(g.glyph).or_insert(c);
            }
        }
    }

    /// Makes the subsets of the fonts the prepared texts use.
    pub fn finish(&mut self) {
        for font in self.fonts.values_mut() {
            if font.embeddable && font.remapper.num_gids() > 1 {
                font.subset = subsetter::subset(font.data, font.index, &font.remapper).ok();
            }
        }
    }

    /// `text` draws as text (after `finish`): every glyph maps back and every font is embedded.
    pub fn as_text(&self, text: &LaidText) -> bool {
        text.glyphs.iter().all(|g| {
            self.fonts
                .get(&g.font)
                .is_some_and(|f| f.subset.is_some() && f.remapper.get(g.glyph).is_some())
        }) && glyph_chars(text, |f, g| self.advance(f, g)).is_some()
    }

    /// Per glyph of `text` (one `as_text` accepts), the characters the ToUnicode map gives
    /// wrong for it, else `None`. The map keeps the first characters seen per glyph, and
    /// Arabic fonts draw letters that differ only by their dots (ج ح, ب ت ن ي) with one glyph
    /// plus a dot mark, so a later letter on that glyph reads back as the first.
    pub fn actual_text(&self, text: &LaidText) -> Vec<Option<String>> {
        let chars = glyph_chars(text, |f, g| self.advance(f, g)).unwrap_or_default();
        text.glyphs
            .iter()
            .zip(chars)
            .map(|(g, c)| {
                // A separately drawn dot/mark is already represented by its cluster's
                // base glyph. Explicit empty text prevents extractors inventing a
                // replacement character for that unmapped decorative glyph.
                let Some(c) = c else {
                    return Some(String::new());
                };
                let mapped = self
                    .fonts
                    .get(&g.font)
                    .and_then(|f| f.unicode.get(&g.glyph));
                (mapped != Some(&c)).then_some(c)
            })
            .collect()
    }

    /// The glyph's id in its font's subset.
    pub fn subset_gid(&self, font: u16, glyph: u16) -> u16 {
        self.fonts
            .get(&font)
            .and_then(|f| f.remapper.get(glyph))
            .unwrap_or(0)
    }

    /// Appends the outline of glyph `glyph` of font `font` to `out` as a path, through `xf` (em
    /// font units of the glyph to page points). Nothing for a glyph without an outline.
    pub fn outline(
        &mut self,
        font: u16,
        glyph: u16,
        xf: &Xform,
        bytes: &FontBytes,
        out: &mut Content,
    ) -> bool {
        if !self.load(font, bytes) {
            return false;
        }
        let Some(face) = self.fonts.get(&font).and_then(|f| f.face.as_ref()) else {
            return false;
        };
        let mut path = PathOut {
            xf,
            out,
            start: [0.0; 2],
            last: [0.0; 2],
            drew: false,
        };
        face.outline_glyph(GlyphId(glyph), &mut path).is_some() && path.drew
    }

    /// Font units per em of `font` (1000 when unknown).
    pub fn units_per_em(&self, font: u16) -> f64 {
        self.fonts
            .get(&font)
            .and_then(|f| f.face.as_ref())
            .map_or(1000.0, |f| f64::from(f.units_per_em()))
    }

    /// Writes the embedded fonts into `doc`; their resource dictionary (`F<id>`).
    pub fn write(&self, doc: &mut Document) -> Dictionary {
        let mut resources = Dictionary::new();
        for (&id, font) in &self.fonts {
            let (Some(subset), Some(face)) = (&font.subset, &font.face) else {
                continue;
            };
            let upem = f64::from(face.units_per_em());
            let k = 1000.0 / upem;
            let name = format!("{}+{}", subset_tag(id), postscript_name(face));
            let file = doc.add_object(flate_stream(
                dictionary! { "Length1" => subset.len() as i64 },
                subset,
            ));
            let bbox = face.global_bounding_box();
            let cap = face.capital_height().unwrap_or(face.ascender());
            let descriptor = doc.add_object(dictionary! {
                "Type" => "FontDescriptor",
                "FontName" => Object::Name(name.clone().into_bytes()),
                "Flags" => 4,
                "FontBBox" => vec![
                    real(f64::from(bbox.x_min) * k),
                    real(f64::from(bbox.y_min) * k),
                    real(f64::from(bbox.x_max) * k),
                    real(f64::from(bbox.y_max) * k),
                ],
                "ItalicAngle" => real(f64::from(face.italic_angle())),
                "Ascent" => real(f64::from(face.ascender()) * k),
                "Descent" => real(f64::from(face.descender()) * k),
                "CapHeight" => real(f64::from(cap) * k),
                "StemV" => 80,
                "FontFile2" => file,
            });
            let mut widths: Vec<Object> = Vec::new();
            for (new, old) in font.remapper.remapped_gids().enumerate() {
                widths.push(Object::Integer(new as i64));
                widths.push(Object::Array(vec![real(f64::from(font.advance(old)) * k)]));
            }
            let cid = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "CIDFontType2",
                "BaseFont" => Object::Name(name.clone().into_bytes()),
                "CIDSystemInfo" => dictionary! {
                    "Registry" => Object::string_literal("Adobe"),
                    "Ordering" => Object::string_literal("Identity"),
                    "Supplement" => 0,
                },
                "FontDescriptor" => descriptor,
                "DW" => 1000,
                "W" => widths,
                "CIDToGIDMap" => "Identity",
            });
            let to_unicode = doc.add_object(flate_stream(Dictionary::new(), &to_unicode(font)));
            let type0 = doc.add_object(dictionary! {
                "Type" => "Font",
                "Subtype" => "Type0",
                "BaseFont" => Object::Name(name.into_bytes()),
                "Encoding" => "Identity-H",
                "DescendantFonts" => vec![Object::Reference(cid)],
                "ToUnicode" => to_unicode,
            });
            resources.set(format!("F{id}"), type0);
        }
        resources
    }
}

/// A real number for a PDF dictionary.
pub fn real(v: f64) -> Object {
    Object::Real(v as f32)
}

/// A stream of `data`, zlib-compressed.
pub fn flate_stream(mut dict: Dictionary, data: &[u8]) -> Stream {
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(data).expect("writing to memory");
    dict.set("Filter", "FlateDecode");
    let mut stream = Stream::new(dict, z.finish().expect("writing to memory"));
    stream.allows_compression = false;
    stream
}

/// Six capital letters naming the subset of font `id` (PDF 9.6.4).
fn subset_tag(id: u16) -> String {
    let mut n = u32::from(id);
    let mut tag = *b"QIYAAA";
    for slot in tag[3..].iter_mut().rev() {
        *slot = b'A' + (n % 26) as u8;
        n /= 26;
    }
    String::from_utf8(tag.to_vec()).expect("ASCII")
}

/// The font's PostScript name (else its family), letters, digits and `-` only.
fn postscript_name(face: &Face) -> String {
    let name = |id: u16| {
        face.names()
            .into_iter()
            .filter(|n| n.name_id == id)
            .find_map(|n| n.to_string())
    };
    let raw = name(ttf_parser::name_id::POST_SCRIPT_NAME)
        .or_else(|| name(ttf_parser::name_id::FAMILY))
        .unwrap_or_else(|| "Font".to_string());
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(60)
        .collect();
    if clean.is_empty() {
        "Font".to_string()
    } else {
        clean
    }
}

/// The ToUnicode CMap of a subset: new glyph id to the characters it was shaped from.
fn to_unicode(font: &Font) -> Vec<u8> {
    let mut pairs: Vec<(u16, &str)> = font
        .remapper
        .remapped_gids()
        .enumerate()
        .filter_map(|(new, old)| font.unicode.get(&old).map(|s| (new as u16, s.as_str())))
        .collect();
    pairs.sort_by_key(|(g, _)| *g);
    let mut t = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    for chunk in pairs.chunks(100) {
        t.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (g, s) in chunk {
            let hex: String = s.encode_utf16().map(|u| format!("{u:04X}")).collect();
            t.push_str(&format!("<{g:04X}> <{hex}>\n"));
        }
        t.push_str("endbfchar\n");
    }
    t.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    t.into_bytes()
}

/// Glyph outlines into a content stream: quadratic curves become cubic ones.
struct PathOut<'a> {
    xf: &'a Xform,
    out: &'a mut Content,
    start: [f64; 2],
    last: [f64; 2],
    drew: bool,
}

impl PathOut<'_> {
    fn at(&self, x: f32, y: f32) -> [f64; 2] {
        [f64::from(x), f64::from(y)]
    }
}

impl OutlineBuilder for PathOut<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.start = p;
        self.last = p;
        self.out.point(self.xf.apply(p), "m");
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.last = p;
        self.drew = true;
        self.out.point(self.xf.apply(p), "l");
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (q, p) = (self.at(x1, y1), self.at(x, y));
        let l = self.last;
        let c1 = [
            l[0] + 2.0 / 3.0 * (q[0] - l[0]),
            l[1] + 2.0 / 3.0 * (q[1] - l[1]),
        ];
        let c2 = [
            p[0] + 2.0 / 3.0 * (q[0] - p[0]),
            p[1] + 2.0 / 3.0 * (q[1] - p[1]),
        ];
        self.curve(c1, c2, p);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.curve(c1, c2, p);
    }

    fn close(&mut self) {
        self.last = self.start;
        self.out.op("h");
    }
}

impl PathOut<'_> {
    fn curve(&mut self, c1: [f64; 2], c2: [f64; 2], p: [f64; 2]) {
        self.last = p;
        self.drew = true;
        let [a, b, c] = [c1, c2, p].map(|q| self.xf.apply(q));
        self.out.op(&format!(
            "{} {} {} {} {} {} c",
            fmt(a[0], 3),
            fmt(a[1], 3),
            fmt(b[0], 3),
            fmt(b[1], 3),
            fmt(c[0], 3),
            fmt(c[1], 3)
        ));
    }
}

/// Font units of a glyph of `font` to text units (one text unit = the text height).
pub fn em_to_text(upem: f64) -> Xform {
    let k = 1.0 / (upem * f64::from(CAP_EM));
    Xform::scale(k, k)
}

#[cfg(test)]
mod tests {
    use qiyas_render::{LaidText, ShapedGlyph};

    use super::*;

    fn laid(text: &str, glyphs: &[(u16, u32)]) -> LaidText {
        LaidText {
            top: 1,
            entity: 1,
            chain: u32::MAX,
            color: 0,
            xf: Xform::identity(),
            world_size: 1.0,
            glyphs: glyphs
                .iter()
                .map(|&(glyph, cluster)| ShapedGlyph {
                    font: 0,
                    glyph,
                    x: 0.0,
                    y: 0.0,
                    cluster,
                })
                .collect(),
            text: text.to_string(),
        }
    }

    #[test]
    fn each_cluster_maps_to_its_characters_once() {
        // "fi" as one ligature glyph, then "x".
        let t = laid("fix", &[(10, 0), (11, 2)]);
        let chars = glyph_chars(&t, |_, _| 500).unwrap();
        assert_eq!(chars, [Some("fi".to_string()), Some("x".to_string())]);
        // A mark (no advance) drawn before its base in the same cluster: the base gets it.
        let t = laid("بx", &[(20, 0), (21, 0), (22, 2)]);
        let chars = glyph_chars(&t, |_, g| if g == 20 { 0 } else { 500 }).unwrap();
        assert_eq!(chars, [None, Some("ب".to_string()), Some("x".to_string())]);
        // MText lines: the line break maps to nothing.
        let t = laid("a\nb", &[(1, 0), (2, 2)]);
        let chars = glyph_chars(&t, |_, _| 500).unwrap();
        assert_eq!(chars, [Some("a".to_string()), Some("b".to_string())]);
    }

    #[test]
    fn a_glyph_that_does_not_map_back_draws_the_text_as_outlines() {
        // The missing glyph, a cluster past the end, a cluster inside a character.
        assert!(glyph_chars(&laid("a", &[(0, 0)]), |_, _| 1).is_none());
        assert!(glyph_chars(&laid("a", &[(5, 3)]), |_, _| 1).is_none());
        assert!(glyph_chars(&laid("ب", &[(5, 1)]), |_, _| 1).is_none());
    }

    #[test]
    fn subset_tags_are_six_capitals() {
        assert_eq!(subset_tag(0), "QIYAAA");
        assert_eq!(subset_tag(27), "QIYABB");
        assert!(subset_tag(u16::MAX).bytes().all(|b| b.is_ascii_uppercase()));
    }
}
