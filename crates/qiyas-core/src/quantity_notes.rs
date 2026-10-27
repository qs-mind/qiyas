//! Advise when a quantity note has no measured source; keep the model's decision. A sheet shows
//! values, never where Qiyas keeps them: `internal_id` finds the identifiers that must not be
//! printed (take-off ids, row references, handles, unique ids).
use crate::{CoreError, Session, Source};

fn word_start(bytes: &[u8], at: usize) -> bool {
    at == 0 || !bytes[at - 1].is_ascii_alphanumeric()
}

fn is_uuid_at(b: &[u8]) -> bool {
    b.len() >= 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b[..36]
            .iter()
            .enumerate()
            .all(|(i, c)| [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit())
}

/// What internal identifier `text` shows, if any: a take-off id (`takeoff:…`), a BOQ row
/// reference (`boq-row:…`), a unique id, a drawing handle (`handle 1234`, `0x1A2B`) or one of
/// Qiyas's own application names.
pub(crate) fn internal_id(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    for (marker, what) in [
        ("takeoff:", "a take-off id"),
        ("boq-row:", "a BOQ row reference"),
        ("qiyas_", "an internal name"),
    ] {
        if lower.contains(marker) {
            return Some(what);
        }
    }
    let b = lower.as_bytes();
    for at in 0..b.len() {
        if is_uuid_at(&b[at..]) && word_start(b, at) {
            return Some("a unique id");
        }
        if b[at..].starts_with(b"0x")
            && word_start(b, at)
            && b.get(at + 2..at + 4)
                .is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit))
        {
            return Some("a drawing handle");
        }
        if b[at..].starts_with(b"handle") && word_start(b, at) {
            let rest = lower[at + 6..].trim_start_matches([' ', ':', '=', '#']);
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some("a drawing handle");
            }
        }
    }
    None
}

/// What a stated value measures: a note's "17.81 m²" is an area, its "5 m" a length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stated {
    Length,
    Area,
    Volume,
}

impl Stated {
    fn of(kind: crate::Kind) -> Option<Self> {
        match kind {
            crate::Kind::Length => Some(Self::Length),
            crate::Kind::Area => Some(Self::Area),
            crate::Kind::Volume => Some(Self::Volume),
            _ => None,
        }
    }
}

/// The values `text` states with a metre unit, in the order written (Arabic digits read), each
/// with what it measures.
fn stated_values(text: &str) -> Vec<(f64, Stated)> {
    let text = crate::numbers::western(&text.to_lowercase()).into_owned();
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && (chars[j].is_ascii_digit() || matches!(chars[j], '.' | ',')) {
            j += 1;
        }
        let number: String = chars[i..j].iter().filter(|c| **c != ',').collect();
        let mut k = j;
        while k < chars.len() && chars[k].is_whitespace() {
            k += 1;
        }
        let tail: String = chars[k..].iter().collect();
        let starts = |units: &[&str]| units.iter().any(|u| tail.starts_with(u));
        // A word unit ends the word: "m" is a metre, "mm" and "minutes" are not.
        let word = |units: &[&str]| {
            units.iter().any(|u| {
                tail.strip_prefix(u)
                    .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphabetic()))
            })
        };
        let measures = if starts(&["m²", "m2", "m^2", "square m", "sq m", "م²", "م2", "متر مربع"]) {
            Some(Stated::Area)
        } else if starts(&["m³", "m3", "m^3", "cubic m", "م³", "م3", "متر مكعب"]) {
            Some(Stated::Volume)
        } else if word(&["m", "metre", "metres", "meter", "meters", "م", "متر", "أمتار", "امتار"]) {
            Some(Stated::Length)
        } else {
            None
        };
        if let (Some(measures), Ok(n)) = (measures, number.parse::<f64>()) {
            out.push((n, measures));
        }
        i = j.max(i + 1);
    }
    out
}

pub(crate) fn states_measurement(text: &str) -> bool {
    let text = text.to_lowercase();
    let chars: Vec<char> = text.chars().collect();
    let digit = |c: char| c.is_ascii_digit() || matches!(c, '٠'..='٩' | '۰'..='۹');
    for i in 0..chars.len() {
        if !digit(chars[i]) {
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && (digit(chars[j]) || matches!(chars[j], '.' | ',' | '٫' | '٬')) {
            j += 1;
        }
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        let tail: String = chars[j..].iter().collect();
        for unit in [
            "m",
            "metre",
            "metres",
            "meter",
            "meters",
            "square metres",
            "square meters",
            "cubic metres",
            "cubic meters",
            "م",
            "متر",
            "مترًا",
            "متراً",
            "أمتار",
            "امتار",
        ] {
            if let Some(rest) = tail.strip_prefix(unit) {
                if rest.chars().next().is_none_or(|c| !c.is_alphabetic()) {
                    return true;
                }
            }
        }
    }
    false
}
impl Session {
    /// Advice for text going on a sheet: an internal id is never printed there.
    pub(crate) fn check_sheet_text(&mut self, text: &str, source: Source) {
        if !matches!(source, Source::Ai | Source::Mcp) {
            return;
        }
        if let Some(what) = internal_id(text) {
            let already = self
                .last_advice
                .iter()
                .any(|a| a["code"] == "internal_id_in_text");
            if !already {
                self.last_advice.push(serde_json::json!({"code":"internal_id_in_text","message":format!("This text shows {what}. A client sheet shows values, never internal ids, handles or row references: write the quantity itself (and its source in words), then edit the text."),"message_ar":"يحتوي النص على معرّف داخلي. اللوحة تعرض القيم فقط، لا المعرّفات ولا أرقام الصفوف: اكتب الكمية نفسها ومصدرها بالكلمات."}));
            }
        }
    }

    pub(crate) fn check_quantity_note(
        &mut self,
        text: &str,
        source: Source,
    ) -> Result<(), CoreError> {
        if !matches!(source, Source::Ai | Source::Mcp) {
            return Ok(());
        }
        self.check_sheet_text(text, source);
        if !states_measurement(text) {
            return Ok(());
        }
        // A note is measured when each value it states is a take-off row's value as that row
        // shows it (to the centimetre); a `takeoff:<id>` citation still counts, though it must
        // not be printed.
        let rows: Vec<_> = self
            .takeoff
            .items
            .iter()
            .filter(|row| {
                row.unit != "markup"
                    && row.available()
                    && (!row.handles.is_empty()
                        || row.points.len() >= 2
                        || row.block_name.is_some())
            })
            .collect();
        // Each stated value must be a row's value as that row shows it, in the unit the note
        // gives it: 20 m² is not cited by a 20 m length.
        let values = stated_values(text);
        let by_value = !values.is_empty()
            && values.iter().all(|(n, measures)| {
                rows.iter().any(|row| {
                    Stated::of(row.kind) == Some(*measures) && (row.shown - n).abs() <= 0.0051
                })
            });
        let by_id = rows.iter().any(|row| {
            text.match_indices(&format!("takeoff:{}", row.id))
                .any(|(at, citation)| {
                    text[at + citation.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_')
                })
        });
        if by_value || by_id {
            Ok(())
        } else {
            self.last_advice.push(serde_json::json!({"code":"uncited_quantity","message":"This note states a quantity that matches no measured take-off row. Measure it first (measure_entity, measure_room or count_block; label_spaces labels room areas) and write the value as the row shows it. Never print a take-off id or row number on the sheet. The note does not establish a measured quantity."}));
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::{internal_id, states_measurement};
    #[test]
    fn internal_ids_are_found_and_ordinary_text_is_not() {
        for s in [
            "takeoff:34f348f7-aaaa-bbbb-cccc-1234567890ab",
            "Plan basis: TAKEOFF:abc",
            "see boq-row:7",
            "34f348f7-1234-4abc-8def-0123456789ab",
            "handle 1234",
            "Handle: 77",
            "ref 0x1A2B",
            "QIYAS_OBJECT",
        ] {
            assert!(internal_id(s).is_some(), "{s}");
        }
        for s in [
            "Handle with care",
            "Door D1, 900 x 2100",
            "Area 17.81 m²",
            "Room 0 and 0xyz",
            "Plan basis: 12.0 x 15.0 m, ground floor",
            "المساحة ١٧٫٨١ م²",
        ] {
            assert_eq!(internal_id(s), None, "{s}");
        }
    }
    #[test]
    fn stated_values_are_the_values_with_a_metre_unit_and_what_they_measure() {
        use super::Stated::{Area, Length, Volume};
        assert_eq!(
            super::stated_values("Area 17.81 m² and 3,200.5 m"),
            [(17.81, Area), (3200.5, Length)]
        );
        assert_eq!(super::stated_values("المساحة ١٧٫٨١ م²"), [(17.81, Area)]);
        assert_eq!(super::stated_values("Volume 60 m³"), [(60.0, Volume)]);
        assert!(super::stated_values("Room 2, 1200 mm").is_empty());
    }
    #[test]
    fn detects_english_and_arabic_values_without_confusing_other_units_or_names() {
        for s in [
            "6.70 m × 8.70 m (Area: 58.29 m²)",
            "20m2",
            "2 m³",
            "3 m^3",
            "٢٠٫٥ م²",
            "المساحة ٢٠ متر مربع",
            "۳ متر",
            "2 metres",
            "12 square meters",
        ] {
            assert!(states_measurement(s), "{s}");
        }
        for s in [
            "Room 2",
            "Check door M2",
            "1200 mm",
            "300 cm",
            "2 in",
            "m²",
            "20 minutes",
            "طابق ٢",
        ] {
            assert!(!states_measurement(s), "{s}");
        }
    }
}
