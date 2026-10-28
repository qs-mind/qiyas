//! Fonts for drawing text: Noto Sans and Noto Sans Arabic (OFL, bundled) behind every drawing
//! font, and the drawing's own TrueType files when the computer has them.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use ab_glyph::FontRef;

/// Noto Sans Regular, unhinted (OFL, `fonts/NotoSans-OFL.txt`).
static NOTO_SANS_TTF: &[u8] = include_bytes!("../fonts/NotoSans-Regular.ttf");
/// Noto Sans Arabic Regular, unhinted (OFL, `fonts/NotoSansArabic-OFL.txt`).
static NOTO_ARABIC_TTF: &[u8] = include_bytes!("../fonts/NotoSansArabic-Regular.ttf");

/// Font ids of the bundled fonts, always the first two.
pub const NOTO_SANS: u16 = 0;
pub const NOTO_ARABIC: u16 = 1;
/// The fallback chain when the drawing's font is an SHX font or is not on this computer.
pub const DEFAULT_CHAIN: [u16; 2] = [NOTO_SANS, NOTO_ARABIC];

/// One loaded font: the shaping face and the outline face over the same bytes.
pub struct Font {
    pub face: rustybuzz::Face<'static>,
    pub raster: FontRef<'static>,
    /// Font units per em.
    pub upem: f32,
    pub index: u32,
}

impl Font {
    fn parse(data: &'static [u8], index: u32) -> Option<Font> {
        let face = rustybuzz::Face::from_slice(data, index)?;
        let raster = FontRef::try_from_slice_and_index(data, index).ok()?;
        let upem = face.units_per_em() as f32;
        Some(Font {
            face,
            raster,
            upem,
            index,
        })
    }

    /// Whether the font draws `c` (not the missing-glyph box).
    pub fn has(&self, c: char) -> bool {
        self.face.glyph_index(c).is_some_and(|g| g.0 != 0)
    }
}

/// The fonts of one session. Chains are per font file name of a text style.
pub struct Fonts {
    list: Vec<Font>,
    /// Font file name (lower case) to its font id, or `None` when not found or not a TrueType.
    by_file: HashMap<String, Option<u16>>,
    /// Look in the computer's font folders; off in tests, so they don't depend on the machine.
    system: bool,
}

impl Fonts {
    /// The bundled fonts, and the computer's when `system` is true.
    pub fn new(system: bool) -> Fonts {
        let list = [NOTO_SANS_TTF, NOTO_ARABIC_TTF]
            .map(|data| Font::parse(data, 0).expect("bundled fonts parse"))
            .into();
        Fonts {
            list,
            by_file: HashMap::new(),
            system,
        }
    }

    pub fn get(&self, id: u16) -> &Font {
        &self.list[usize::from(id)]
    }

    /// The whole font file of font `id`; `None` for an unknown id.
    pub fn bytes(&self, id: u16) -> Option<&'static [u8]> {
        self.list
            .get(usize::from(id))
            .map(|f| f.face.raw_face().data)
    }

    /// The fonts to try for a style whose font file is `file` (e.g. `arial.ttf`, `romans.shx`,
    /// `txt`): that file when it is a TrueType or OpenType font found in the computer's font
    /// folders, then Noto Sans, then Noto Sans Arabic. SHX fonts map to the Noto pair.
    pub fn chain(&mut self, file: &str) -> Vec<u16> {
        let mut chain = Vec::with_capacity(3);
        if let Some(id) = self.load(file) {
            chain.push(id);
        }
        chain.extend(DEFAULT_CHAIN);
        chain
    }

    fn load(&mut self, file: &str) -> Option<u16> {
        if !self.system {
            return None;
        }
        let name = file_name(file)?;
        if let Some(found) = self.by_file.get(&name) {
            return *found;
        }
        let found = system_font(&name)
            .and_then(|(data, index)| Font::parse(data, index))
            .map(|font| {
                self.list.push(font);
                (self.list.len() - 1) as u16
            });
        self.by_file.insert(name, found);
        found
    }
}

/// The lower-case file name of a TrueType or OpenType font file, without any folder (Windows or
/// not); `None` for SHX fonts (which include names without an extension, as AutoCAD reads them).
fn file_name(file: &str) -> Option<String> {
    let name = file.trim().rsplit(['/', '\\']).next()?.to_lowercase();
    [".ttf", ".otf", ".ttc"]
        .iter()
        .any(|ext| name.ends_with(ext))
        .then_some(name)
}

/// A loaded face: its bytes, kept for the process lifetime, and its index in a collection.
type Face = (&'static [u8], u32);

/// System discovery and collection faces belong to fontdb. CAD filenames still win over
/// family matching. Immutable bytes are kept once per selected face for the process lifetime.
fn system_font(name: &str) -> Option<Face> {
    static DB: OnceLock<fontdb::Database> = OnceLock::new();
    static LOADED: OnceLock<Mutex<HashMap<fontdb::ID, Face>>> = OnceLock::new();
    let db = DB.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        db
    });
    let file_face = db
        .faces()
        .filter(|face| {
            let path = match &face.source {
                fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path,
                _ => return false,
            };
            path.to_str().and_then(file_name).as_deref() == Some(name)
        })
        .min_by_key(|face| {
            (
                face.style != fontdb::Style::Normal,
                face.weight.0.abs_diff(fontdb::Weight::NORMAL.0),
                face.index,
            )
        });
    let id = file_face.map(|face| face.id).or_else(|| {
        let stem = name.rsplit_once('.')?.0;
        let family = db
            .faces()
            .flat_map(|face| &face.families)
            .find(|(family, _)| family.eq_ignore_ascii_case(stem))?;
        db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(&family.0)],
            ..Default::default()
        })
    })?;
    let mut loaded = LOADED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(face) = loaded.get(&id) {
        return Some(*face);
    }
    let face = db.with_face_data(id, |data, index| {
        let data: &'static [u8] = Box::leak(data.to_vec().into_boxed_slice());
        (data, index)
    })?;
    loaded.insert(id, face);
    Some(face)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shx_and_missing_fonts_fall_back_to_noto() {
        let mut fonts = Fonts::new(true);
        assert_eq!(fonts.chain("romans.shx"), DEFAULT_CHAIN);
        assert_eq!(fonts.chain("txt"), DEFAULT_CHAIN);
        assert_eq!(fonts.chain(""), DEFAULT_CHAIN);
        assert_eq!(fonts.chain("no-such-font-anywhere.ttf"), DEFAULT_CHAIN);
    }

    #[test]
    fn bundled_fonts_cover_latin_and_arabic() {
        let fonts = Fonts::new(false);
        assert!(fonts.get(NOTO_SANS).has('A'));
        assert!(!fonts.get(NOTO_SANS).has('ق'));
        assert!(fonts.get(NOTO_ARABIC).has('ق'));
        assert_eq!(
            file_name("C:\\Fonts\\Arial.TTF").as_deref(),
            Some("arial.ttf")
        );
        assert_eq!(file_name("Arial.TTF").as_deref(), Some("arial.ttf"));
    }
}
