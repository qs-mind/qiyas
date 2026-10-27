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

/// What a measured number is: client text shows areas and lengths to two decimals with their
/// unit (the take-off keeps full precision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dim {
    Area,
    Length,
}

/// The kind of quantity a property or column name stands for, by the names the engine itself
/// uses (`area`, `net_floor_area`, `perimeter`, `boundary_length`).
pub(crate) fn dim_of_name(name: &str) -> Option<Dim> {
    let n = name.to_ascii_lowercase();
    if n.ends_with("area") {
        Some(Dim::Area)
    } else if matches!(n.as_str(), "length" | "perimeter") || n.ends_with("_length") {
        Some(Dim::Length)
    } else {
        None
    }
}

/// Decimals a client sees for `name` when nothing says otherwise.
pub(crate) fn default_decimals(name: &str) -> usize {
    if dim_of_name(name).is_some() {
        2
    } else {
        3
    }
}

pub(crate) fn has_arabic(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')
    })
}

/// Western digits and the decimal point as Arabic digits and the Arabic decimal separator.
pub(crate) fn arabic_digits(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('٠' as u32 + c as u32 - '0' as u32).unwrap(),
            '.' => '٫',
            _ => c,
        })
        .collect()
}

/// A heading that already gives the unit (`Area (m²)`) makes the unit in every cell redundant.
pub(crate) fn heading_gives_unit(heading: &str) -> bool {
    ["m²", "م²", "(m)", "(م)", "m2"]
        .iter()
        .any(|unit| heading.contains(unit))
}

/// Digits only (a code such as `D-900` keeps its point and letters).
pub(crate) fn arabic_numerals(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0'..='9' => char::from_u32('٠' as u32 + c as u32 - '0' as u32).unwrap(),
            _ => c,
        })
        .collect()
}

pub(crate) fn unit_text(dim: Dim, arabic: bool) -> &'static str {
    match (dim, arabic) {
        (Dim::Area, false) => "m²",
        (Dim::Area, true) => "م²",
        (Dim::Length, false) => "m",
        (Dim::Length, true) => "م",
    }
}

/// True when the text right after a number already starts with a unit (`m²`, `م²`, `sqm`, `m`).
pub(crate) fn unit_follows(rest: &str) -> bool {
    let t = rest.trim_start();
    t.starts_with("sq")
        || t.starts_with("متر")
        || ["m", "M", "م"].iter().any(|u| {
            t.strip_prefix(u).is_some_and(|after| {
                after
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphabetic() || matches!(c, 'm' | 'M'))
            })
        })
}

/// A measured number as a client reads it: `value` already has its decimals; the unit goes
/// after it unless the text that follows gives one.
pub(crate) fn with_unit(value: &str, dim: Dim, arabic: bool, following: &str) -> String {
    let shown = if arabic {
        arabic_digits(value)
    } else {
        value.to_string()
    };
    if unit_follows(following) {
        shown
    } else {
        format!("{shown} {}", unit_text(dim, arabic))
    }
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
