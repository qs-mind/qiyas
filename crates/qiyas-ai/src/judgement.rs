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
        if wants_to_continue(text) {
            return previous;
        }
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

    /// Efficiency advice, sent once in a turn. It is never a reason to end a job the user asked for.
    pub fn budget(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Self::Drawing, Lang::En) => "[efficiency note] This turn has used many calls. Spend the rest well: batch edits, do not re-read what you already have, read a long result by pointer instead of paging it. This is not a reason to stop: finish the work the user asked for, phase by phase.",
            (Self::Drawing, Lang::Ar) => "[efficiency note] استُخدمت خطوات كثيرة في هذه الرسالة. وفّر في الباقي: اجمع التعديلات في دفعات، ولا تعد قراءة ما لديك، واقرأ النتيجة الطويلة بمؤشر لا بصفحات. هذا ليس سببًا للتوقف: أنهِ ما طلبه المستخدم مرحلة بعد مرحلة.",
            (Self::Takeoff, Lang::En) => "[efficiency note] This turn has used many calls. Spend the rest well: batch the mappings, do not re-read what you already have, read a long report by pointer instead of paging it. This is not a reason to stop: finish the quantity work the user asked for, and explain what stays uncertain.",
            (Self::Takeoff, Lang::Ar) => "[efficiency note] استُخدمت خطوات كثيرة في هذه الرسالة. وفّر في الباقي: اجمع الربط في دفعات، ولا تعد قراءة ما لديك، واقرأ التقرير الطويل بمؤشر لا بصفحات. هذا ليس سببًا للتوقف: أنهِ عمل الكميات الذي طلبه المستخدم واشرح ما بقي غير مؤكد.",
        }
    }
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A short message that tells Mizan to carry on ("continue", "complete everything", "أكمل"), or to
/// decide for itself ("apply all your assumptions"). It resumes the pending plan; it is not a
/// new request.
pub fn wants_to_continue(text: &str) -> bool {
    let words = words(text);
    if words.is_empty() || words.len() > 12 {
        return false;
    }
    let has = |list: &[&str]| words.iter().any(|w| list.contains(&w.as_str()));
    // "Continue", "resume", "أكمل": a short message that is only that.
    let carry_on = has(&[
        "continue",
        "resume",
        "proceed",
        "أكمل",
        "اكمل",
        "كمل",
        "تابع",
        "استمر",
        "واصل",
        "أتمم",
        "اتمم",
    ]) && words.len() <= 6;
    // "Complete everything", "finish the rest".
    let everything = has(&["complete", "finish"])
        && has(&["everything", "all", "rest", "remaining", "كل", "الباقي"]);
    carry_on
        || everything
        || (has(&["assumptions", "assumption", "افتراضاتك", "افتراضات"])
            && has(&["apply", "use", "your", "all", "طبق", "اعتمد", "كل"]))
        || (has(&["go"]) && has(&["ahead", "on"]))
        || (has(&["decide"]) && has(&["you", "yourself"]))
}

/// The user told Mizan to stop: the one case where an unfinished plan may be left as it is.
pub fn wants_to_stop(text: &str) -> bool {
    let words = words(text);
    if words.len() > 14 {
        return false;
    }
    let lower = text.to_lowercase();
    [
        "that's enough",
        "that is enough",
        "never mind",
        "do not continue",
        "don't continue",
        "no need to continue",
        "لا تكمل",
        "لا تتابع",
        "بلا داعي",
        "مش محتاج",
    ]
    .iter()
    .any(|p| lower.contains(p))
        || words.iter().any(|w| {
            matches!(
                w.as_str(),
                "stop"
                    | "cancel"
                    | "enough"
                    | "كفاية"
                    | "كفايه"
                    | "توقف"
                    | "قف"
                    | "وقف"
                    | "أوقف"
                    | "اوقف"
                    | "كفى"
            )
        })
}

/// Arabic and Latin letters in the prose of `text` (code spans and identifiers like A-WALL left out).
fn letters(text: &str) -> (usize, usize) {
    let mut arabic = 0;
    let mut latin = 0;
    let mut code = false;
    for chunk in text.split('`') {
        if !code {
            for word in chunk.split_whitespace() {
                // Match the chat's direction rule: names such as A-WALL are not prose.
                if word.is_ascii()
                    && word.chars().any(|c| c.is_ascii_uppercase())
                    && word
                        .chars()
                        .any(|c| c == '-' || c == '_' || c.is_ascii_digit())
                {
                    continue;
                }
                arabic += word.chars().filter(|c| c.is_alphabetic() && matches!(*c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')).count();
                latin += word.chars().filter(char::is_ascii_alphabetic).count();
            }
        }
        code = !code;
    }
    (arabic, latin)
}

/// An answer clearly written in the other language than the user's: enough prose, and one
/// language well ahead of the other (names, units and file names in the user's language may
/// appear inside an answer without making it the wrong language).
pub fn language_mismatch(answer: &str, expected: Lang) -> bool {
    let (arabic, latin) = letters(answer);
    match expected {
        Lang::En => arabic >= 60 && arabic > latin,
        Lang::Ar => latin >= 60 && latin > arabic * 4,
    }
}

pub fn language(text: &str, fallback: Lang) -> Lang {
    let (arabic, latin) = letters(text);
    if arabic > 0 && arabic * 2 >= latin {
        Lang::Ar
    } else if latin > 0 {
        Lang::En
    } else {
        fallback
    }
}

/// A new design with no stated destination needs the user's choice on a populated drawing.
/// This deliberately avoids inferring a destination from drawing or layer names.
pub fn new_design(text: &str) -> bool {
    let text: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(*c, '\u{064b}'..='\u{065f}' | '\u{0670}'))
        .collect();
    if [
        "this drawing",
        "current drawing",
        "existing drawing",
        "new drawing",
        "هذا الرسم",
        "الرسم الحالي",
        "رسم جديد",
    ]
    .iter()
    .any(|s| text.contains(s))
        || text.split_whitespace().any(|word| {
            matches!(
                word.trim_matches(|c: char| !c.is_alphabetic()),
                "here" | "هنا"
            )
        })
    {
        return false;
    }
    let request = ["draw a ", "draw an ", "design ", "ارسم ", "صمم ", "صمّم "]
        .iter()
        .any(|s| text.contains(s));
    let small_edit = [
        "line",
        "circle",
        "rectangle",
        "dimension",
        "label",
        "text",
        "خط",
        "خطا",
        "دائرة",
        "مستطيل",
        "مستطيلا",
        "تسمية",
        "نص",
        "نصا",
        "بعدا",
    ]
    .iter()
    .any(|s| {
        text.split_whitespace()
            .any(|word| word.trim_matches(|c: char| !c.is_alphabetic()) == *s)
    });
    request && !small_edit
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn continue_and_stop_are_read_from_short_messages_in_both_languages() {
        for text in [
            "Continue and complete everything !",
            "continue",
            "Apply all assumptions from your side",
            "أكمل كل شيء",
            "كمل",
            "go ahead",
            "تابع من فضلك",
            "Finish the rest",
        ] {
            assert!(wants_to_continue(text), "{text}");
        }
        for text in [
            "Draw a villa",
            "ارسم فيلا دورين وطلّع مقايستها",
            "Cairo",
            "Draw a villa, keep it simple",
            "Complete the kitchen layout",
            "What does the continue command do in this long sentence about something unrelated to resuming work at all today",
        ] {
            assert!(!wants_to_continue(text), "{text}");
        }
        for text in [
            "stop",
            "That's enough, thanks",
            "كفاية كده",
            "Please stop here",
            "وقف",
        ] {
            assert!(wants_to_stop(text), "{text}");
        }
        for text in ["Continue and complete everything !", "Egypt", "Build a bus stop shelter drawing with the full detailed dimension set for every wall and window"] {
            assert!(!wants_to_stop(text), "{text}");
        }
        // Continue keeps the job the conversation was doing.
        assert_eq!(
            Job::request("Continue and complete everything !", Job::Takeoff),
            Job::Takeoff
        );
    }

    #[test]
    fn an_answer_in_the_wrong_language_is_found_but_names_do_not_count() {
        let arabic = "توقفت هنا لتجنب زيادة التكلفة. الموجود حاليا حزمة تطوير معماري مبدئية وليست حزمة تنفيذ كاملة أو معتمدة.";
        let english = "I stopped here to avoid more cost. What exists now is a preliminary architectural package, not a complete or approved construction set.";
        assert!(language_mismatch(arabic, Lang::En));
        assert!(language_mismatch(english, Lang::Ar));
        assert!(!language_mismatch(arabic, Lang::Ar));
        assert!(!language_mismatch(english, Lang::En));
        assert!(!language_mismatch(
            "Cairo_Villa_P02_A501_Review.pdf",
            Lang::Ar
        ));
        let mixed = "الملفات: Cairo_Villa_Plans.pdf و A-WALL و Villa_Quantities.xlsx جاهزة للمراجعة وليست معتمدة بعد من المهندس المسؤول";
        assert!(!language_mismatch(mixed, Lang::Ar));
    }

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
