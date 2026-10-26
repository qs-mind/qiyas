//! Numbers in drawing text can use Western, Arabic or Persian digits, including mixed text.
use std::borrow::Cow;

pub(crate) fn western(text: &str) -> Cow<'_, str> {
    if !text
        .chars()
        .any(|c| matches!(c, '\u{0660}'..='\u{066c}' | '\u{06f0}'..='\u{06f9}' | '\u{2212}'))
    {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter_map(|c| {
                Some(match c {
                    '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x660) as u8),
                    '\u{06f0}'..='\u{06f9}' => char::from(b'0' + (c as u32 - 0x6f0) as u8),
                    '\u{066b}' => '.',
                    '\u{066c}' => return None,
                    '\u{2212}' => '-',
                    other => other,
                })
            })
            .collect(),
    )
}

pub(crate) fn parse(text: &str) -> Option<f64> {
    western(text)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

pub(crate) fn words(text: &str) -> Vec<f64> {
    western(text)
        .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .filter_map(parse)
        .collect()
}

/// Strip the unit before reading the last number: the 2 in m2 is an exponent, not an area.
pub(crate) fn area_text(text: &str) -> Option<&str> {
    let text = text.trim();
    ["m²", "m2", "M²", "M2", "م²", "م2", "م٢", "م۲"]
        .into_iter()
        .find_map(|unit| text.strip_suffix(unit))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_accept_mixed_digits_and_separators_without_reading_unit_exponents() {
        for text in [
            "Area 12.50 m²",
            "المساحة ١٢٫٥٠ م²",
            "Area ۱۲٫۵۰ m2",
            "Area ١2٫5٠ m2",
            "Area ١٢٫٥٠ م٢",
        ] {
            assert_eq!(words(area_text(text).unwrap()).last(), Some(&12.5));
        }
        assert_eq!(parse("١٬٢٣٤٫٥"), Some(1234.5));
        assert_eq!(parse("−۲٫۵"), Some(-2.5));
        assert_eq!(parse("####"), None);
    }
}
