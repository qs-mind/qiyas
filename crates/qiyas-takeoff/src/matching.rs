//! Text matching for the comparison: description tokens in English and Arabic, description
//! groups, floor tokens in sheet names, units, numbers in schedule cells.

use std::collections::BTreeSet;

use qiyas_core::BlockCount;

/// Words that say nothing about the work, units and sizes among them. Compared after `fold` and
/// after digits are removed ("50mm" is "mm").
const STOP_WORDS: [&str; 23] = [
    "the",
    "of",
    "and",
    "in",
    "to",
    "for",
    "و",
    "في",
    "من",
    "إلى",
    "mm",
    "cm",
    "m",
    "m2",
    "m3",
    "thk",
    "thick",
    "thickness",
    "nos",
    "no",
    "nr",
    "pcs",
    // A count's default description is "<block> count".
    "count",
];
/// Arabic article and the prepositions fused with it, longest first.
const ARTICLES: [&str; 6] = ["وال", "بال", "كال", "فال", "لل", "ال"];
/// Two descriptions match when more than this share of the shorter one's tokens is in the other.
const MATCH_SHARE: f64 = 0.5;
/// The units the BOQ import knows; any other is the workbook's own text.
pub(crate) const STANDARD_UNITS: [&str; 6] = ["m", "m2", "m3", "pcs", "kg", "t"];

/// Lower case, Arabic diacritics and tatweel removed, alef forms, ى and ة folded (like the BOQ
/// panel's search), so "الأعمدة" and "الاعمده" are the same.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}'))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ى' => 'ي',
            'ة' => 'ه',
            c => c,
        })
        .collect()
}

/// The words of a text, as schedule headers are read: folded, split at anything not a letter or
/// digit, one-letter words dropped, the Arabic article (also fused with و ب ك ف ل) taken off.
/// Stop words stay, so "No." and "Nos" are words.
pub(crate) fn words(text: &str) -> BTreeSet<String> {
    split_words(text, true)
}

/// `words`, with digits kept or removed.
fn split_words(text: &str, keep_digits: bool) -> BTreeSet<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .map(|word| {
            word.chars()
                .filter(|c| keep_digits || !c.is_numeric())
                .collect::<String>()
        })
        .map(|word| {
            ARTICLES
                .iter()
                .find_map(|a| {
                    word.strip_prefix(a)
                        .filter(|rest| rest.chars().count() >= 2)
                })
                .map_or(word.clone(), str::to_string)
        })
        .filter(|word| word.chars().count() >= 2)
        .collect()
}

/// The words of a description with digits and stop words removed and English plurals made
/// singular: what descriptions are matched on ("Screed 50mm" is "screed", "Doors" is "door").
pub(crate) fn tokens(text: &str) -> BTreeSet<String> {
    let stop: Vec<String> = STOP_WORDS.iter().map(|w| fold(w)).collect();
    split_words(text, false)
        .into_iter()
        .map(|w| singular(&w))
        .filter(|w| !stop.contains(w))
        .collect()
}

/// A simple English singular for words of 4 letters or more: "boxes" is "box" (after s, x, z,
/// ch or sh), "doors" is "door"; words ending in "ss" ("glass") stay.
fn singular(word: &str) -> String {
    if word.chars().count() < 4 || !word.is_ascii() || word.ends_with("ss") {
        return word.to_string();
    }
    if let Some(stem) = word.strip_suffix("es") {
        if ["s", "x", "z", "ch", "sh"]
            .iter()
            .any(|e| stem.ends_with(e))
        {
            return stem.to_string();
        }
    }
    word.strip_suffix('s').unwrap_or(word).to_string()
}

/// Two descriptions (`tokens` of each) match when they share 2 tokens or more (or their one
/// token each is the same) and more than half the tokens of the shorter one.
pub(crate) fn overlaps(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    let shared = a.intersection(b).count();
    let shorter = a.len().min(b.len());
    let enough = shared >= 2 || (shared == 1 && a.len() == 1 && b.len() == 1);
    enough && shared as f64 / shorter as f64 > MATCH_SHARE
}

/// The key measurements are grouped by: folded, digits and punctuation removed, spaces
/// collapsed, so "Wall 1" and "wall-2" group together.
pub(crate) fn group_key(description: &str) -> String {
    fold(description)
        .chars()
        .map(|c| if c.is_alphabetic() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Units compare without case or spaces, so "M2" is "m2".
pub(crate) fn unit_key(unit: &str) -> String {
    unit.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// The floors a sheet name names, as whole words in any case, each as one canonical name so
/// that "Level 01" and "L1" are the same floor:
/// - GF, G, ground, الأرضي → GF; RF, roof, السطح → RF; mezzanine, الميزانين → MZ;
/// - FF, first, الأول → L1; SF, second, الثاني → L2; TF, third, الثالث → L3; fourth, الرابع → L4;
/// - L1 to L99, 1F to 99F, and a number after level, floor, lvl, طابق or دور → L and the number
///   (level 0 → GF);
/// - B1 to B9 → B and the number; basement, البدروم → B.
///
/// A bare sheet number ("A-01") is not a floor.
pub(crate) fn floor_tokens(sheet: &str) -> BTreeSet<String> {
    let words: Vec<String> = sheet
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(fold)
        .collect();
    let mut floors = BTreeSet::new();
    for (i, word) in words.iter().enumerate() {
        if let Some(floor) = named_floor(word) {
            floors.insert(floor);
        } else if LEVEL_WORDS.iter().any(|w| fold(w) == *word) {
            let number = words.get(i + 1).and_then(|n| floor_number(n, 2));
            if let Some(n) = number {
                floors.insert(level(n));
            }
        }
    }
    floors
}

/// Words a floor number follows.
const LEVEL_WORDS: [&str; 7] = ["level", "floor", "lvl", "طابق", "الطابق", "دور", "الدور"];

/// The canonical floor of one folded word, if it names one by itself.
fn named_floor(word: &str) -> Option<String> {
    const NAMED: [(&str, &str); 22] = [
        ("gf", "GF"),
        ("g", "GF"),
        ("ground", "GF"),
        ("الأرضي", "GF"),
        ("rf", "RF"),
        ("roof", "RF"),
        ("السطح", "RF"),
        ("mezzanine", "MZ"),
        ("الميزانين", "MZ"),
        ("ff", "L1"),
        ("first", "L1"),
        ("الأول", "L1"),
        ("sf", "L2"),
        ("second", "L2"),
        ("الثاني", "L2"),
        ("tf", "L3"),
        ("third", "L3"),
        ("الثالث", "L3"),
        ("fourth", "L4"),
        ("الرابع", "L4"),
        ("basement", "B"),
        ("البدروم", "B"),
    ];
    if let Some((_, floor)) = NAMED.iter().find(|(w, _)| fold(w) == word) {
        return Some(floor.to_string());
    }
    if let Some(n) = word.strip_prefix('b').and_then(|d| floor_number(d, 1)) {
        return Some(format!("B{n}"));
    }
    word.strip_prefix('l')
        .or_else(|| word.strip_suffix('f'))
        .and_then(|d| floor_number(d, 2))
        .map(level)
}

/// Level `n` by its canonical name: level 0 is the ground floor.
fn level(n: u32) -> String {
    if n == 0 {
        "GF".to_string()
    } else {
        format!("L{n}")
    }
}

/// `text` as a floor number of 1 to `most` digits (Arabic-Indic too), without leading zeros.
fn floor_number(text: &str, most: usize) -> Option<u32> {
    let digits: String = text.chars().map(ascii_digit).collect();
    ((1..=most).contains(&digits.chars().count()) && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Arabic-Indic digits as ASCII digits; other characters as they are.
fn ascii_digit(c: char) -> char {
    match c {
        '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
        '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
        c => c,
    }
}

/// A sheet of schedules: its name says "schedule" or "جدول".
pub(crate) fn is_schedule_sheet(sheet: &str) -> bool {
    sheet.to_lowercase().contains("schedule") || sheet.contains("جدول")
}

/// The number a schedule cell starts with ("4", "4 nos", "٤"); `None` when it starts with none.
pub(crate) fn leading_number(cell: &str) -> Option<f64> {
    let digits: String = cell
        .trim()
        .chars()
        .map(|c| if c == '٫' { '.' } else { ascii_digit(c) })
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok()
}

/// The block references of `b` are the ones a schedule mark names when one of their attribute
/// values is the mark, or the block's name is the mark, or (for a mark of 2 characters or more
/// with a letter and a digit, like "D1") the name has the mark as one of its words ("DOOR-D1").
/// All in any case.
pub(crate) fn names_mark(b: &BlockCount, mark: &str) -> bool {
    let mark = mark.trim().to_lowercase();
    if mark.is_empty() {
        return false;
    }
    let name = b.block_name.to_lowercase();
    let as_word = mark.chars().count() >= 2
        && mark.chars().any(char::is_alphabetic)
        && mark.chars().any(char::is_numeric);
    name == mark
        || (as_word
            && name
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w == mark))
        || b.attributes.iter().any(|a| a.trim().to_lowercase() == mark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arabic_article_and_stop_words_go() {
        let t = tokens("خرسانة مسلحة للأعمدة في المبنى");
        assert!(t.contains("اعمده"), "{t:?}");
        assert!(t.contains("مبني"), "{t:?}");
        assert!(!t.iter().any(|w| w == "في"));
        assert_eq!(
            tokens("The walls of the 2nd floor"),
            tokens("walls floor nd")
        );
    }

    #[test]
    fn descriptions_need_two_shared_tokens() {
        let m = |a: &str, b: &str| overlaps(&tokens(a), &tokens(b));
        assert!(m("خرسانة مسلحة للأعمدة", "خرسانة الأعمدة"));
        assert!(m("Concrete slab GF", "Reinforced concrete slab"));
        assert!(m("Plaster", "plaster"));
        assert!(!m("Concrete columns", "Reinforced concrete slabs"));
        assert!(!m("Screed 50mm", "Plaster 20mm thick"));
        // Half is not more than half.
        assert!(!m("concrete slab beam column", "concrete slab stair wall"));
    }

    #[test]
    fn english_plurals_are_singular() {
        assert_eq!(tokens("Doors"), tokens("door"));
        assert_eq!(tokens("Boxes"), tokens("box"));
        assert_eq!(tokens("Walls"), tokens("wall"));
        assert_eq!(tokens("Floor tiles"), tokens("floor tile"));
        assert_eq!(tokens("Glass"), ["glass".to_string()].into());
        assert_eq!(tokens("gas"), ["gas".to_string()].into());
        assert!(overlaps(&tokens("Doors 900 mm"), &tokens("DOOR count")));
    }

    #[test]
    fn floors_are_whole_words() {
        let f = |name: &str| floor_tokens(name).into_iter().collect::<Vec<_>>();
        assert_eq!(f("GF Plan"), ["GF"]);
        assert_eq!(f("L02 - Lighting"), ["L2"]);
        assert_eq!(f("1f furniture"), ["L1"]);
        assert_eq!(f("Level 01 Plan"), f("Level 01 RCP"));
        assert_eq!(f("Level 01 Plan"), ["L1"]);
        assert_eq!(f("Ground Floor Plan"), ["GF"]);
        assert_eq!(f("Ground Floor Furniture"), ["GF"]);
        assert_eq!(f("Floor 3"), ["L3"]);
        assert_eq!(f("Level 0 Plan"), ["GF"]);
        assert_eq!(f("L00 Lighting"), ["GF"]);
        assert_eq!(f("مخطط الدور ٢"), ["L2"]);
        assert_eq!(f("مخطط الطابق الأرضي"), ["GF"]);
        assert_eq!(f("Basement B2"), ["B", "B2"]);
        assert!(f("A-01").is_empty());
        assert!(f("GF_PLAN").is_empty());
        assert!(f("Gfx Plan").is_empty());
        assert!(f("Floor Plan").is_empty());
    }

    fn block(name: &str, attributes: &[&str]) -> BlockCount {
        BlockCount {
            count_scope: qiyas_core::INSERT_COUNT_SCOPE,
            visibility_policy: "in_file",
            limitations: qiyas_core::INSERT_COUNT_LIMITATIONS,
            block_name: name.to_string(),
            layer: "0".to_string(),
            count: 1,
            attributes: attributes.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn schedule_cells_and_marks() {
        assert_eq!(leading_number(" 4 nos"), Some(4.0));
        assert_eq!(leading_number("٤"), Some(4.0));
        assert_eq!(leading_number("-"), None);
        assert!(names_mark(&block("DOOR-D1", &[]), "d1"));
        assert!(names_mark(&block("D1", &[]), "D1"));
        assert!(!names_mark(&block("D10", &[]), "D1"));
        assert!(!names_mark(&block("A-DOOR", &[]), "A"));
        assert!(names_mark(&block("A", &[]), "a"));
        assert!(names_mark(&block("DOOR", &["D1"]), "d1"));
        assert!(!names_mark(&block("DOOR", &["D2"]), "D1"));
    }
}
