//! Text on the sheet on screen: single-line text (`Command::Text`) and MText (`Command::MText`)
//! styled by the current drawing state with a text style, text edits (`Command::EditText`: a
//! text, an MText or one attribute of a block reference), and find and replace
//! (`Command::FindReplace`, `Command::FindText`) over texts, MText (their plain runs only: the
//! formatting codes stay as they are), attributes and dimension text overrides.
//!
//! New and edited text is stored in reading order and marked with `LOGICAL_TEXT_APP` xdata, so
//! "Arabic text is reversed", which fixes the drawing's own old text, never turns it.

use opencadcodec::entities::mtext_format::parse_mtext;
use opencadcodec::entities::{AttachmentPoint, EntityCommon};
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle, MText, Text, Vector3};
use qiyas_geom::LOGICAL_TEXT_APP;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::field::{cell_fields, has_fields, set_text_field, text_field};
use crate::session::variant_name;
use crate::table::is_text_value;
use crate::{CoreError, Session, Target};

/// Where find and replace looks: the sheet on screen, or every sheet of the drawing (model and
/// the paper layouts). Never inside block definitions or xrefs; attributes of block references
/// on those sheets are searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextScope {
    Sheet,
    Drawing,
}

/// An entity holding a match: its handle (a block reference's, for an attribute) and the sheet
/// (block name) it is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextMatch {
    pub handle: u64,
    pub sheet: String,
}

/// What a find looks for.
pub(crate) struct Finder {
    find: Vec<char>,
    match_case: bool,
    whole_word: bool,
}

/// An entity with its matches replaced: how many, and whether the result fits.
struct Hit {
    entity: EntityType,
    count: usize,
    fits: bool,
    /// Table cells with matches left as they were: they hold a field.
    skipped: usize,
}

/// How a string is read: which parts are codes, never searched.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A TEXT or an attribute: `%%d`-style codes.
    Text,
    /// MText formatting codes, braces and escapes too.
    MText,
    /// A dimension text override: as MText, and `<>` (the measurement) too.
    Dimension,
}

impl Finder {
    pub(crate) fn new(find: &str, match_case: bool, whole_word: bool) -> Result<Self, CoreError> {
        if find.is_empty() {
            return Err(invalid("Type what to find"));
        }
        Ok(Finder {
            find: find.chars().collect(),
            match_case,
            whole_word,
        })
    }

    fn same(&self, a: char, b: char) -> bool {
        a == b || (!self.match_case && a.to_lowercase().eq(b.to_lowercase()))
    }

    /// The matches in `run` (plain characters only), as char ranges, left to right without
    /// overlapping. The run's ends count as word boundaries.
    fn matches(&self, run: &[char]) -> Vec<(usize, usize)> {
        let n = self.find.len();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut out = Vec::new();
        let mut i = 0;
        while i + n <= run.len() {
            let hit = run[i..i + n]
                .iter()
                .zip(&self.find)
                .all(|(&a, &b)| self.same(a, b));
            let bounded = !self.whole_word
                || ((i == 0 || !word(run[i - 1])) && (i + n == run.len() || !word(run[i + n])));
            if hit && bounded {
                out.push((i, i + n));
                i += n;
            } else {
                i += 1;
            }
        }
        out
    }

    /// `text` read as `mode`, with every match in its plain runs replaced by `with` (escaped for
    /// MText), and how many there were. With `with` None only counts.
    fn replace(&self, text: &str, mode: Mode, with: Option<&str>) -> (String, usize) {
        let chars: Vec<char> = text.chars().collect();
        let with = with.map(|w| match mode {
            Mode::Text => w.to_string(),
            Mode::MText | Mode::Dimension => escape_mtext(w),
        });
        let mut out = String::new();
        let mut count = 0;
        let mut i = 0;
        while i < chars.len() {
            let code = code_len(&chars[i..], mode);
            if code > 0 {
                out.extend(&chars[i..i + code]);
                i += code;
                continue;
            }
            let start = i;
            while i < chars.len() && code_len(&chars[i..], mode) == 0 {
                i += 1;
            }
            let run = &chars[start..i];
            let mut at = 0;
            for (a, b) in self.matches(run) {
                count += 1;
                out.extend(&run[at..a]);
                if let Some(w) = &with {
                    out.push_str(w);
                } else {
                    out.extend(&run[a..b]);
                }
                at = b;
            }
            out.extend(&run[at..]);
        }
        (out, count)
    }
}

/// How many characters at the start of `s` are a code (none for a plain character).
fn code_len(s: &[char], mode: Mode) -> usize {
    let starts = |a: char, b: char| s.first() == Some(&a) && s.get(1) == Some(&b);
    if starts('%', '%') {
        return s.len().min(3);
    }
    if mode == Mode::Text {
        return 0;
    }
    if mode == Mode::Dimension && starts('<', '>') {
        return 2;
    }
    match s[0] {
        '{' | '}' => 1,
        '\\' => match s.get(1) {
            None => 1,
            Some('U') if s.get(2) == Some(&'+') => s.len().min(7),
            Some('M') if s.get(2) == Some(&'+') => s.len().min(8),
            // Codes with a value up to ';': font, height, width, slant, tracking, alignment,
            // colours, paragraph, stacking.
            Some('f' | 'F' | 'H' | 'W' | 'Q' | 'T' | 'A' | 'C' | 'c' | 'p' | 'S') => s
                .iter()
                .position(|&c| c == ';')
                .map_or(s.len(), |end| end + 1),
            Some(_) => 2,
        },
        _ => 0,
    }
}

/// Plain text as MText stores it: backslashes and braces escaped, line breaks as `\P`.
pub(crate) fn escape_mtext(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    out.push_str("\\P");
                }
            }
            '\n' => out.push_str("\\P"),
            c => out.push(c),
        }
    }
    out
}

/// Marks the text `common` belongs to as written by Qiyas in reading order.
pub(crate) fn mark_logical(common: &mut EntityCommon) {
    let mut record = ExtendedDataRecord::new(LOGICAL_TEXT_APP);
    record.add_value(XDataValue::Integer16(1));
    common.extended_data.upsert_record(record);
    // Proxy graphics a reader kept show the old text.
    common.graphic_data = None;
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn check_point(p: [f64; 2]) -> Result<Vector3, CoreError> {
    if p.iter().all(|c| c.is_finite()) {
        Ok(Vector3::new(p[0], p[1], 0.0))
    } else {
        Err(invalid("The point must have finite coordinates"))
    }
}

fn check_height(height: f64) -> Result<(), CoreError> {
    if height > 0.0 && height.is_finite() {
        Ok(())
    } else {
        Err(invalid("The text height must be more than 0"))
    }
}

fn check_value(value: &str) -> Result<(), CoreError> {
    if value.trim().is_empty() {
        Err(invalid("Type the text first"))
    } else {
        Ok(())
    }
}

impl Session {
    /// True when the drawing has no `LOGICAL_TEXT_APP` application yet: writing or editing a
    /// text adds it to the drawing's tables.
    pub(crate) fn needs_text_app(&self) -> bool {
        !self.drawing.doc.app_ids.contains(LOGICAL_TEXT_APP)
    }

    pub(crate) fn register_text_app(&mut self) {
        if self.needs_text_app() {
            let doc = &mut self.drawing.doc;
            let mut app = AppId::new(LOGICAL_TEXT_APP);
            app.handle = doc.allocate_handle();
            doc.app_ids.add(app).expect("checked it is missing");
        }
    }

    /// The drawing's own text styles (not an xref's), sorted by name.
    pub fn text_styles(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .drawing
            .doc
            .text_styles
            .iter()
            .filter(|s| !s.name.contains('|') && !s.name.is_empty())
            .map(|s| s.name.clone())
            .collect();
        names.sort();
        names
    }

    /// The style new text takes when none is chosen: the drawing's current one (TEXTSTYLE), or
    /// "Standard" when the drawing has no such style.
    pub fn text_style(&self) -> String {
        let doc = &self.drawing.doc;
        doc.text_styles
            .get(&doc.header.current_text_style_name)
            .map_or_else(|| "Standard".to_string(), |s| s.name.clone())
    }

    /// The text style `name` as the drawing spells it, or the current one when `None`.
    fn style_named(&self, name: Option<&str>) -> Result<String, CoreError> {
        match name.map(str::trim) {
            None | Some("") => Ok(self.text_style()),
            Some(name) => self
                .drawing
                .doc
                .text_styles
                .get(name)
                .map(|s| s.name.clone())
                .ok_or_else(|| invalid(format!("The drawing has no text style {name}"))),
        }
    }

    /// `Command::Text`: one line of text at `at`, `height` drawing units tall, turned `rotation`
    /// degrees, in `style` (the drawing's current one when `None`).
    pub(crate) fn add_text(
        &mut self,
        at: [f64; 2],
        height: f64,
        rotation: f64,
        value: String,
        style: Option<&str>,
    ) -> Result<u64, CoreError> {
        let at = check_point(at)?;
        let height = if height == 0.0 {
            self.default_text_height()
        } else {
            height
        };
        check_height(height)?;
        if !rotation.is_finite() {
            return Err(invalid("The rotation must be a number"));
        }
        let value = value.replace("\\n", "\n");
        check_value(&value)?;
        if value.contains(['\n', '\r']) {
            // The top of the first line: one height up along the text's own rotation.
            let (sin, cos) = rotation.to_radians().sin_cos();
            let top = check_point([at.x - sin * height, at.y + cos * height])?;
            let mut text = self.mtext_entity(top, 0.0, height, &value, style)?;
            if let EntityType::MText(t) = &mut text {
                t.rotation = rotation.to_radians();
            }
            return Ok(self.add_styled(vec![text])?[0]);
        }
        let style = self.style_named(style)?;
        let (shown, field) = self.with_fields(value);
        let mut text = Text::with_value(shown, at).with_height(height);
        text.rotation = rotation.to_radians();
        text.style = style;
        self.register_text_app();
        mark_logical(&mut text.common);
        set_text_field(
            &mut text.common,
            field.as_deref(),
            crate::xdata::app_handle(&self.drawing.doc, crate::field::FIELD_APP),
        );
        Ok(self.add_styled(vec![EntityType::Text(text)])?[0])
    }

    /// `Command::MText`: plain text (lines split by `\n`) in a box `width` drawing units wide
    /// (0: no wrapping) whose top left corner is `at`.
    pub(crate) fn add_mtext(
        &mut self,
        at: [f64; 2],
        width: f64,
        height: f64,
        value: &str,
        style: Option<&str>,
    ) -> Result<u64, CoreError> {
        let at = check_point(at)?;
        let height = if height == 0.0 {
            self.default_text_height()
        } else {
            height
        };
        check_height(height)?;
        if !(width >= 0.0 && width.is_finite()) {
            return Err(invalid("The width must be 0 or more"));
        }
        let value = value.replace("\\n", "\n");
        check_value(&value)?;
        let text = self.mtext_entity(at, width, height, &value, style)?;
        Ok(self.add_styled(vec![text])?[0])
    }

    /// The MText `Command::MText` adds, not added yet (its text application registered).
    pub(crate) fn mtext_entity(
        &mut self,
        at: Vector3,
        width: f64,
        height: f64,
        value: &str,
        style: Option<&str>,
    ) -> Result<EntityType, CoreError> {
        let style = self.style_named(style)?;
        let (shown, field) = self.with_fields(value.to_string());
        let mut text = MText::with_value(escape_mtext(&shown), at).with_height(height);
        text.rectangle_width = width;
        text.attachment_point = AttachmentPoint::TopLeft;
        text.style = style;
        self.register_text_app();
        mark_logical(&mut text.common);
        set_text_field(
            &mut text.common,
            field.as_deref(),
            crate::xdata::app_handle(&self.drawing.doc, crate::field::FIELD_APP),
        );
        Ok(EntityType::MText(text))
    }

    /// `Command::EditText`: the text of `target` becomes `value` (plain text; an MText's
    /// formatting goes). A block reference changes its attribute `tag`.
    pub(crate) fn edit_text(
        &mut self,
        target: &Target,
        value: &str,
        tag: Option<&str>,
    ) -> Result<(), CoreError> {
        let value = value.replace("\\n", "\n");
        let value = value.as_str();
        let h = self.sheet_targets(std::slice::from_ref(target))?[0];
        self.refuse_generated_edit(&[h], false)?;
        let entity = self.drawing.doc.get_entity(h).expect("a sheet target");
        // Check first, so a refused edit changes nothing.
        let attribute = match (entity, tag) {
            (EntityType::Text(_) | EntityType::MText(_), None) => {
                check_value(value)?;
                None
            }
            (EntityType::Insert(insert), Some(tag)) => {
                let i = insert
                    .attributes
                    .iter()
                    .position(|a| a.tag.eq_ignore_ascii_case(tag.trim()))
                    .ok_or_else(|| {
                        invalid(format!("This block reference has no attribute {tag}"))
                    })?;
                // A QS may clear an attribute.
                if !insert.attributes[i].is_multiline && value.contains(['\n', '\r']) {
                    return Err(invalid("This attribute takes one line"));
                }
                Some(i)
            }
            (EntityType::Insert(_), None) => {
                return Err(invalid("Name the attribute to change"));
            }
            (EntityType::Text(_) | EntityType::MText(_), Some(_)) => {
                return Err(invalid("Only a block reference has attributes"));
            }
            (other, _) => {
                return Err(invalid(format!("A {} holds no text", variant_name(other))));
            }
        };
        self.register_text_app();
        let (shown, field) = if attribute.is_none() {
            self.with_fields(value.to_string())
        } else {
            (value.to_string(), None)
        };
        let field_app = crate::xdata::app_handle(&self.drawing.doc, crate::field::FIELD_APP);
        let entity = self.drawing.doc.get_entity_mut(h).expect("a sheet target");
        match entity {
            EntityType::Text(t) if value.contains(['\n', '\r']) => {
                let mut at = t.insertion_point;
                at.y += t.height;
                let mut text = MText::with_value(escape_mtext(&shown), at).with_height(t.height);
                text.common = t.common.clone();
                text.common.raw_record = None;
                text.style.clone_from(&t.style);
                text.rotation = t.rotation;
                text.normal = t.normal;
                text.attachment_point = AttachmentPoint::TopLeft;
                text.rectangle_width = 0.0;
                mark_logical(&mut text.common);
                set_text_field(&mut text.common, field.as_deref(), field_app);
                *entity = EntityType::MText(text);
            }
            EntityType::Text(t) => {
                t.value = shown;
                mark_logical(&mut t.common);
                set_text_field(&mut t.common, field.as_deref(), field_app);
            }
            EntityType::MText(t) => {
                t.value = escape_mtext(&shown);
                mark_logical(&mut t.common);
                set_text_field(&mut t.common, field.as_deref(), field_app);
            }
            EntityType::Insert(insert) => {
                let a = &mut insert.attributes[attribute.expect("checked above")];
                match a.embedded_mtext.as_deref_mut().filter(|_| a.is_multiline) {
                    Some(mtext) => {
                        // The attribute's own value carries the first line only.
                        mtext.value = escape_mtext(value);
                        a.value = value.lines().next().unwrap_or_default().to_string();
                    }
                    None => a.value = value.to_string(),
                }
                mark_logical(&mut a.common);
                insert.common.graphic_data = None;
            }
            _ => unreachable!("checked above"),
        }
        Ok(())
    }

    /// What a text typed as `value` shows, and the field code it keeps when it holds one (worked
    /// out on the sheet on screen).
    fn with_fields(&mut self, value: String) -> (String, Option<String>) {
        if !has_fields(&value) {
            return (value, None);
        }
        self.register_field_app();
        let sheet = self.sheet_owner().unwrap_or(Handle::NULL);
        (self.evaluate_fields(&value, sheet), Some(value))
    }

    /// The handles of the entities `scope` searches, with the sheet (block name) of each.
    fn text_candidates(&self, scope: TextScope) -> Vec<(Handle, String)> {
        let sheets = match scope {
            TextScope::Sheet => vec![self.sheet.clone()],
            TextScope::Drawing => self.drawing.sheets().into_iter().map(|s| s.block).collect(),
        };
        sheets
            .into_iter()
            .flat_map(|sheet| {
                self.drawing
                    .entities_in(&sheet)
                    .filter(|e| {
                        matches!(
                            e,
                            EntityType::Text(_)
                                | EntityType::MText(_)
                                | EntityType::Insert(_)
                                | EntityType::Dimension(_)
                                | EntityType::Table(_)
                        )
                    })
                    .map(|e| (e.common().handle, sheet.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The text of `e` with the matches replaced by `with` (counted only when `None`); `None`
    /// when there is no match. It does not fit when a text would be left blank or a single line
    /// would take a line break, or when it is a field (the next update would put the field's value
    /// back); a table's field cells are left as they were and counted in `skipped`.
    fn replaced(finder: &Finder, e: &EntityType, with: Option<&str>) -> Option<Hit> {
        let mut e = e.clone();
        let one_line = |v: &str| !v.contains(['\n', '\r']);
        let field = text_field(e.common()).is_some();
        let mut fits = true;
        let mut skipped = 0;
        let count = match &mut e {
            EntityType::Text(t) => {
                let (v, n) = finder.replace(&t.value, Mode::Text, with);
                fits = one_line(&v) && !v.trim().is_empty() && !field;
                t.value = v;
                n
            }
            EntityType::MText(t) => {
                let (v, n) = finder.replace(&t.value, Mode::MText, with);
                fits = !field;
                t.value = v;
                n
            }
            EntityType::Insert(insert) => {
                let mut total = 0;
                for a in &mut insert.attributes {
                    let n = match a.embedded_mtext.as_deref_mut().filter(|_| a.is_multiline) {
                        // Every line is in the embedded MText; the attribute's own value is its
                        // first line.
                        Some(mtext) => {
                            let (v, n) = finder.replace(&mtext.value, Mode::MText, with);
                            if n > 0 {
                                let plain = parse_mtext(&v, true).to_plain_text();
                                a.value = plain.lines().next().unwrap_or_default().to_string();
                                mtext.value = v;
                            }
                            n
                        }
                        None => {
                            let (v, n) = finder.replace(&a.value, Mode::Text, with);
                            fits &= n == 0 || one_line(&v);
                            a.value = v;
                            n
                        }
                    };
                    if n > 0 {
                        total += n;
                        a.common.graphic_data = None;
                    }
                }
                total
            }
            EntityType::Dimension(d) => {
                let base = d.base_mut();
                let text = base.text_override().map(str::to_string)?;
                let (v, n) = finder.replace(&text, Mode::Dimension, with);
                base.set_text_override(Some(v));
                n
            }
            // Every cell's text, read as MText; a field's cell is never replaced.
            EntityType::Table(t) => {
                let fields: Vec<(usize, usize)> =
                    cell_fields(t).into_iter().map(|(r, c, _)| (r, c)).collect();
                let mut total = 0;
                for (r, row) in t.rows.iter_mut().enumerate() {
                    for (c, cell) in row.cells.iter_mut().enumerate() {
                        let field = fields.contains(&(r, c));
                        let contents = cell.contents.iter_mut();
                        for content in contents.filter(|x| is_text_value(&x.value)) {
                            let (v, n) = finder.replace(content.value.display(), Mode::MText, with);
                            if n == 0 {
                                continue;
                            }
                            if with.is_some() {
                                if field {
                                    skipped += 1;
                                    continue;
                                }
                                content.value.text = v.clone();
                                content.value.formatted_value = v;
                            }
                            total += n;
                        }
                    }
                }
                total
            }
            _ => 0,
        };
        e.common_mut().graphic_data = None;
        (count > 0 || skipped > 0).then_some(Hit {
            entity: e,
            count,
            fits,
            skipped,
        })
    }

    /// `Command::FindText`: how many matches `finder` finds in `scope`, and the entities holding
    /// them in drawing order (for the whole drawing: model, then the layouts in tab order).
    pub(crate) fn find_text(&self, finder: &Finder, scope: TextScope) -> (usize, Vec<TextMatch>) {
        let doc = &self.drawing.doc;
        let mut count = 0;
        let mut matches = Vec::new();
        for (h, sheet) in self.text_candidates(scope) {
            let e = doc.get_entity(h).expect("listed from the drawing");
            if let Some(hit) = Self::replaced(finder, e, None) {
                count += hit.count;
                matches.push(TextMatch {
                    handle: h.value(),
                    sheet,
                });
            }
        }
        (count, matches)
    }

    /// `Command::FindReplace`: replaces every match of `finder` in `scope` with `with`; how
    /// many, the entities changed, and how many entities were left as they were because the
    /// result would not fit (a blank text, a line break in one line). A dimension's block is
    /// rebuilt with its new text.
    pub(crate) fn find_replace(
        &mut self,
        finder: &Finder,
        with: &str,
        scope: TextScope,
    ) -> Result<(usize, Vec<u64>, usize), CoreError> {
        let mut count = 0;
        let mut handles = Vec::new();
        let mut skipped = 0;
        for (h, _) in self.text_candidates(scope) {
            let e = self
                .drawing
                .doc
                .get_entity(h)
                .expect("listed from the drawing");
            match Self::replaced(finder, e, Some(with)) {
                // Locked: left as it is, counted.
                Some(hit) if hit.count > 0 && self.is_locked(h) => skipped += 1,
                Some(hit) if hit.fits => {
                    skipped += hit.skipped;
                    if hit.count == 0 {
                        continue;
                    }
                    let dimension = matches!(hit.entity, EntityType::Dimension(_));
                    let table = matches!(hit.entity, EntityType::Table(_));
                    *self.drawing.doc.get_entity_mut(h).expect("listed") = hit.entity;
                    // Its block shows the new text.
                    if dimension {
                        self.rebuild_dimension(h)?;
                    }
                    if table {
                        self.rebuild_table(h)?;
                    }
                    count += hit.count;
                    handles.push(h.value());
                }
                Some(_) => skipped += 1,
                None => {}
            }
        }
        Ok((count, handles, skipped))
    }
}

#[cfg(test)]
mod tests {
    use opencadcodec::entities::{AttributeEntity, Insert};

    use super::*;

    fn run(find: &str, text: &str, mode: Mode, case: bool, word: bool) -> (String, usize) {
        Finder::new(find, case, word)
            .unwrap()
            .replace(text, mode, Some("X"))
    }

    #[test]
    fn codes_are_never_searched() {
        let note = "{\\fArial|b1;\\C1;FIRE}\\PDOORS \\U+0041";
        assert_eq!(run("C1", note, Mode::MText, false, false).1, 0);
        assert_eq!(run("Arial", note, Mode::MText, false, false).1, 0);
        assert_eq!(run("0041", note, Mode::MText, false, false).1, 0);
        assert_eq!(
            run("fire", note, Mode::MText, false, false).0,
            "{\\fArial|b1;\\C1;X}\\PDOORS \\U+0041"
        );
        assert_eq!(
            run("d", "90%%d d", Mode::Text, true, false),
            ("90%%d X".into(), 1)
        );
        assert_eq!(
            run(">", "<> >", Mode::Dimension, true, false),
            ("<> X".into(), 1)
        );
    }

    #[test]
    fn whole_words_and_case() {
        assert_eq!(run("door", "Door doors DOOR", Mode::Text, false, true).1, 2);
        assert_eq!(run("door", "Door doors DOOR", Mode::Text, true, false).1, 1);
        // Arabic letters are letters: a word inside another is no whole word.
        assert_eq!(
            run("باب", "باب الباب", Mode::Text, true, true),
            ("X الباب".into(), 1)
        );
        // Matches don't overlap.
        assert_eq!(run("aa", "aaa", Mode::Text, true, false), ("Xa".into(), 1));
    }

    fn multiline_attribute(value: &str, mtext: &str) -> EntityType {
        let mut a = AttributeEntity::new("NOTE".to_string(), value.to_string());
        a.is_multiline = true;
        a.embedded_mtext = Some(Box::new(MText::with_value(mtext, Vector3::ZERO)));
        let mut insert = Insert::new("TAG", Vector3::ZERO);
        insert.attributes.push(a);
        EntityType::Insert(insert)
    }

    #[test]
    fn a_multiline_attribute_is_searched_on_every_line() {
        let finder = Finder::new("DOOR", true, false).unwrap();
        let e = multiline_attribute("FIRE", "{\\C1;FIRE}\\PRATED DOOR");
        let hit = Session::replaced(&finder, &e, Some("GATE")).unwrap();
        assert_eq!(hit.count, 1);
        let EntityType::Insert(insert) = hit.entity else {
            unreachable!()
        };
        let a = &insert.attributes[0];
        assert_eq!(
            a.embedded_mtext.as_ref().unwrap().value,
            "{\\C1;FIRE}\\PRATED GATE"
        );
        assert_eq!(
            a.value, "FIRE",
            "the attribute's own value is the first line"
        );
        // The first line changes it too.
        let finder = Finder::new("FIRE", true, false).unwrap();
        let hit = Session::replaced(&finder, &e, Some("SMOKE")).unwrap();
        let EntityType::Insert(insert) = hit.entity else {
            unreachable!()
        };
        assert_eq!(insert.attributes[0].value, "SMOKE");
        assert!(hit.fits);
    }

    #[test]
    fn plain_text_is_escaped_for_mtext() {
        assert_eq!(escape_mtext("a\\b {c}\r\nd\ne"), "a\\\\b \\{c\\}\\Pd\\Pe");
    }
}
