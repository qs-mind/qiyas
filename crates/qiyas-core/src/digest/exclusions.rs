//! Coverage exclusions are explicit records, never inferred from prose or type names.
use crate::decisions::TakeoffExclusion;
use crate::objects::ObjectRecord;
use crate::{CoreError, Session};
use std::collections::BTreeMap;

pub(super) fn recorded(
    session: &Session,
    objects: &[ObjectRecord],
    report: &str,
) -> Result<BTreeMap<u64, String>, CoreError> {
    let mut excluded = BTreeMap::new();
    for decision in session.decisions()? {
        match decision.takeoff_exclusion {
            Some(TakeoffExclusion::Handles { handles }) => {
                for h in handles {
                    excluded.insert(h, decision.reason.clone());
                }
            }
            Some(TakeoffExclusion::ObjectType { object_type }) => {
                for object in objects
                    .iter()
                    .filter(|o| o.instance.type_name.eq_ignore_ascii_case(&object_type))
                {
                    excluded.insert(object.instance.id, decision.reason.clone());
                }
            }
            None => {}
        }
    }
    // The optional legacy note citation remains part of the reason shown to the engineer.
    static LINE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?im)^\s*(?:-\s*)?exclude\s+(0x[0-9a-f]+|[0-9]+)[ \t]*:[ \t]*([^\r\n]+)$",
        )
        .unwrap()
    });
    for c in LINE.captures_iter(report) {
        let reason = c[2].trim();
        if reason.is_empty() || reason.to_ascii_lowercase().starts_with("(note ") {
            continue;
        }
        let h = if c[1].starts_with("0x") || c[1].starts_with("0X") {
            u64::from_str_radix(&c[1][2..], 16)
        } else {
            c[1].parse()
        };
        if let Ok(h) = h {
            excluded.entry(h).or_insert_with(|| reason.to_owned());
        }
    }
    excluded.retain(|h, _| {
        session
            .drawing
            .doc
            .get_entity(opencadcodec::Handle::new(*h))
            .is_some()
    });
    Ok(excluded)
}
