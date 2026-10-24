//! Advise when a quantity note has no measured source; keep the model's decision.
use crate::{CoreError, Session, Source};

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
    pub(crate) fn check_quantity_note(
        &mut self,
        text: &str,
        source: Source,
    ) -> Result<(), CoreError> {
        if !matches!(source, Source::Ai | Source::Mcp) || !states_measurement(text) {
            return Ok(());
        }
        let cited = self.takeoff.items.iter().any(|row| {
            row.unit != "markup"
                && row.quantity_valid
                && (!row.handles.is_empty() || row.points.len() >= 2 || row.block_name.is_some())
                && text
                    .match_indices(&format!("takeoff:{}", row.id))
                    .any(|(at, citation)| {
                        text[at + citation.len()..]
                            .chars()
                            .next()
                            .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_')
                    })
        });
        if cited {
            Ok(())
        } else {
            self.last_advice.push(serde_json::json!({"code":"uncited_quantity","message":"This note states a quantity without a measured take-off citation. Consider measure_entity, measure_room or count_block and cite takeoff:<row id>; label_spaces can label room areas. The note does not establish a measured quantity."}));
            Ok(())
        }
    }
}
#[cfg(test)]
mod tests {
    use super::states_measurement;
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
