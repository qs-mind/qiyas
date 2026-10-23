//! Task guidance comes from the request and actual operations, never drawing or layer names.
use crate::{prompt::Lang, surface};
use serde_json::Value;

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    #[default]
    Drawing,
    Takeoff,
}

impl Job {
    pub fn request(text: &str, previous: Self) -> Self {
        let text = text.trim().to_lowercase();
        if matches!(
            text.trim_end_matches(['.', '!', '،']),
            "continue"
                | "go on"
                | "approved, continue"
                | "أكمل"
                | "اكمل"
                | "تابع"
                | "موافق، تابع"
                | "موافق، أكمل"
        ) {
            return previous;
        }
        // Explicit negative scope wins; tool use can still establish a later quantity task.
        if [
            "no boq",
            "no take-off",
            "no takeoff",
            "without boq",
            "without take-off",
            "without quantities",
            "drawing only",
            "drafting only",
            "بدون حصر",
            "رسم فقط",
            "دون حصر",
            "لا أريد حصر",
            "بلا حصر",
            "بدون كميات",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            return Self::Drawing;
        }
        if ["why ", "what ", "how ", "لماذا", "كيف ", "ليه "]
            .iter()
            .any(|s| text.starts_with(s))
        {
            return Self::Drawing;
        }
        if [
            "take-off",
            "takeoff",
            "take off",
            "bill of quantities",
            "quantities",
            "boq",
            "حصر",
            "كميات",
            "جدول كميات",
            "جدول الكميات",
        ]
        .iter()
        .any(|s| text.contains(s))
        {
            Self::Takeoff
        } else {
            Self::Drawing
        }
    }

    pub fn observes(name: &str, input: &Value) -> bool {
        let op = input["op"].as_str().unwrap_or_default();
        if (name == surface::READ && matches!(op, "takeoff_kit" | "takeoff_check" | "workflow"))
            || (name == surface::EXECUTE
                && matches!(
                    op,
                    "import_boq" | "compare" | "export_takeoff" | "export_conflicts"
                ))
        {
            return true;
        }
        let Ok(args) = surface::args_of(input) else {
            return false;
        };
        fn quantity_command(command: &Value) -> bool {
            let mut command = match command {
                Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
                other => other.clone(),
            };
            qiyas_core::registry::canonicalize(&mut command);
            let name = command["type"].as_str().unwrap_or_default();
            qiyas_core::registry::registry()
                .get(name)
                .is_some_and(|info| {
                    matches!(info.undo.as_deref(), Some("takeoff" | "boq" | "conflicts"))
                        || (info.category == "takeoff"
                            && info
                                .undo
                                .as_deref()
                                .is_some_and(|undo| undo.ends_with("takeoff")))
                })
                || matches!(name, "measurement" | "recognition")
                || command["commands"]
                    .as_array()
                    .is_some_and(|commands| commands.iter().any(quantity_command))
        }
        match op {
            "run" => quantity_command(&args["command"]),
            "type" => args["line"].as_str().is_some_and(|line| matches!(qiyas_core::cmdline::Machine::default().step(line), Ok(qiyas_core::cmdline::Step::Command(command)) if quantity_command(&serde_json::json!(command)))),
            "batch" => args["commands"].as_array().is_some_and(|commands| commands.iter().any(quantity_command)),
            _ => false,
        }
    }

    pub fn budget(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Drawing, Lang::En) => "[finish now] The working budget is nearly used. Stop exploring. Check the requested drawing changes, fix new warnings, and briefly say what is complete and what remains. Do not save unless asked.",
            (Self::Drawing, Lang::Ar) => "[finish now] قاربت ميزانية العمل على النفاد. أوقف الاستكشاف. تحقّق من تعديلات الرسم المطلوبة وصحّح التحذيرات الجديدة، ثم اذكر باختصار ما اكتمل وما بقي. لا تحفظ إلا بطلب المستخدم.",
            (Self::Takeoff, Lang::En) => "[finish now] The working budget is nearly used. Finish supported BOQ mappings, read measurement.report and takeoff_check, fix measurement errors, and report remaining uncertainty. Stop exploring. Do not save unless asked.",
            (Self::Takeoff, Lang::Ar) => "[finish now] قاربت ميزانية العمل على النفاد. أكمل ربط الكميات المدعومة بالأدلة، واقرأ measurement.report وtakeoff_check وصحّح أخطاء القياس، ثم اذكر ما بقي غير مؤكد. أوقف الاستكشاف. لا تحفظ إلا بطلب المستخدم.",
        }
    }
}

pub fn language(text: &str, fallback: Lang) -> Lang {
    let mut arabic = 0;
    let mut latin = 0;
    let mut code = false;
    for chunk in text.split('`') {
        if !code {
            for word in chunk.split_whitespace() {
                // Match the chat's direction rule: names such as A-WALL are not prose.
                if word.is_ascii()
                    && word.chars().any(|c| c.is_ascii_uppercase())
                    && !word.chars().any(|c| c.is_ascii_lowercase())
                {
                    continue;
                }
                arabic += word.chars().filter(|c| matches!(*c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')).count();
                latin += word.chars().filter(char::is_ascii_alphabetic).count();
            }
        }
        code = !code;
    }
    if arabic > 0 && arabic * 2 >= latin {
        Lang::Ar
    } else if latin > 0 {
        Lang::En
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn scope_uses_requests_and_operations_not_drawing_or_layer_names() {
        for text in [
            "Draw a bracket",
            "ارسم مخطط المكتب",
            "Draw a room schedule",
            "Drawing only, no BOQ",
            "Draw a plate, no take-off",
            "Why is the BOQ panel empty?",
        ] {
            assert_eq!(Job::request(text, Job::Takeoff), Job::Drawing);
        }
        assert_eq!(Job::request("Continue", Job::Takeoff), Job::Takeoff);
        for text in ["Take off quantities", "احصر الكميات", "استخرج كميات المخطط"]
        {
            assert_eq!(Job::request(text, Job::Drawing), Job::Takeoff);
        }
        for name in ["measure_entity", "measure_room", "count_block"] {
            assert!(
                Job::observes(
                    surface::EXECUTE,
                    &json!({"op":"run","args":{"command":{"type":name}}})
                ),
                "{name}"
            );
        }
        for name in [
            "room_schedule",
            "opening_schedule",
            "label_spaces",
            "draw",
            "verify",
        ] {
            assert!(
                !Job::observes(
                    surface::EXECUTE,
                    &json!({"op":"run","args":{"command":{"type":name}}})
                ),
                "{name}"
            );
        }
        assert!(!Job::observes(
            surface::READ,
            &json!({"op":"measure","args":{"handle":42}})
        ));
        for command in [
            json!({"type":"measurement.report"}),
            json!({"type":"measure_entity","handle":42})
                .to_string()
                .into(),
        ] {
            assert!(Job::observes(
                surface::EXECUTE,
                &json!({"op":"run","args":{"command":command}})
            ));
        }
    }
}
