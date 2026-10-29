//! Drafting knowledge as data (W5): the sheet set of a project type, what a senior includes in
//! each kind of view, the limits of a market's codes and the practice defaults that stand in
//! where no code text was read. Both JSON files are editable data, not code:
//! `data/drafting-standards.json` and `data/code-register.json`.
//!
//! Policy: Qiyas draws; the engineer and the company check and approve. Every value that is not
//! a code requirement with its source says `practice_default`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::rooms::LabelLanguage;
use crate::CoreError;

const STANDARDS_JSON: &str = include_str!("../data/drafting-standards.json");
const REGISTER_JSON: &str = include_str!("../data/code-register.json");

/// Jurisdictions for which the packaged register has data.
pub fn markets() -> Vec<&'static str> {
    register()["markets"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys().map(String::as_str))
        .collect()
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

/// The shipped drafting standards. A test parses both files, so the `expect` cannot fire in a build
/// that passed its tests.
pub fn standards() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(STANDARDS_JSON).expect("data/drafting-standards.json is valid JSON")
    })
}

/// The shipped code register.
pub fn register() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(REGISTER_JSON).expect("data/code-register.json is valid JSON")
    })
}

/// A number a code states (`basis` "code", with code, clause and source) or a practice default.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Limit {
    pub id: String,
    pub market: String,
    pub value: f64,
    pub unit: String,
    pub basis: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub clause: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_ar: Option<String>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub evidence_read: bool,
    /// Scope retained from the register, never inferred from the requested country.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Value>,
    /// An unverified reference, never evidence for a code requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_to_confirm: Option<UnconfirmedReference>,
}

/// A translation reference retained so the engineer can confirm the Arabic text.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct UnconfirmedReference {
    pub code: String,
    pub clause: String,
    pub source_url: String,
    pub note: String,
}

/// Every limit of the register.
pub fn limits() -> &'static [Limit] {
    static DATA: OnceLock<Vec<Limit>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_value(register()["limits"].clone())
            .expect("the code register's limits have the Limit shape")
    })
}

/// `egypt`, `ksa` or `uae` for a market as a person writes it; `None` when it is none of them.
pub fn normalise_market(text: &str) -> Option<&'static str> {
    let t = text.trim().to_lowercase();
    register()["markets"]
        .as_object()?
        .iter()
        .find_map(|(key, data)| {
            (key == &t
                || data["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .any(|alias| t == alias || t.split(',').any(|part| part.trim() == alias)))
            .then_some(key.as_str())
        })
}

/// A limit as used for a market, with where it came from.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResolvedLimit {
    pub limit: Limit,
    /// The market that was asked for.
    pub asked: Option<String>,
    /// True when the market has no value of its own and the practice default stands in.
    pub fallback: bool,
}

impl ResolvedLimit {
    /// The number.
    pub fn value(&self) -> f64 {
        self.limit.value
    }

    /// JSON for a result: the value with its basis, source and what to tell the engineer.
    pub fn json(&self) -> Value {
        let mut v = serde_json::to_value(&self.limit).unwrap_or_else(|_| json!({}));
        v["asked"] = json!(self.asked);
        v["fallback"] = json!(self.fallback);
        if self.fallback {
            v["note_market"] =
                json!("No scoped code value is selected; this is an optional practice preference.");
            v["note_market_ar"] =
                json!("لم يتم اختيار قيمة كود محددة النطاق؛ هذه قيمة ممارسة اختيارية.");
        }
        v
    }
}

/// The limit `id` for `market`: that market's own, else the practice default (`any`).
pub fn limit_for(id: &str, market: Option<&str>) -> Option<ResolvedLimit> {
    // An unsupported requested jurisdiction must never borrow another country's values.
    let normalised = match market {
        Some(name) => Some(normalise_market(name)?),
        None => None,
    };
    let candidates: Vec<_> = limits()
        .iter()
        .filter(|l| l.id == id && Some(l.market.as_str()) == normalised)
        .collect();
    // A single-number API cannot choose between building classes or sub-jurisdictions.
    // Scoped entries are exposed together by resolved_limits instead.
    if candidates.len() == 1 && candidates[0].scope.is_none() {
        return Some(ResolvedLimit {
            limit: candidates[0].clone(),
            asked: market.map(String::from),
            fallback: false,
        });
    }
    limits()
        .iter()
        .find(|l| l.id == id && l.market == "any")
        .map(|limit| ResolvedLimit {
            limit: limit.clone(),
            asked: market.map(String::from),
            fallback: market.is_some(),
        })
}

/// All applicable register alternatives, retaining their jurisdiction and building-class scopes.
pub fn resolved_limits(market: Option<&str>) -> Vec<Value> {
    let normalised = match market {
        Some(name) => match normalise_market(name) {
            Some(m) => Some(m),
            None => return Vec::new(),
        },
        None => None,
    };
    limit_ids()
        .iter()
        .flat_map(|id| {
            let own: Vec<_> = limits()
                .iter()
                .filter(|l| l.id == *id && Some(l.market.as_str()) == normalised)
                .collect();
            let selected = if own.is_empty() {
                limits()
                    .iter()
                    .filter(|l| l.id == *id && l.market == "any")
                    .collect()
            } else {
                own
            };
            selected
                .into_iter()
                .map(|limit| {
                    ResolvedLimit {
                        limit: limit.clone(),
                        asked: market.map(String::from),
                        fallback: normalised.is_some() && limit.market == "any",
                    }
                    .json()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The ids of all limits, once each.
pub fn limit_ids() -> Vec<String> {
    let mut ids: Vec<String> = limits().iter().map(|l| l.id.clone()).collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Every problem in the register: a value without its source or without the practice-default
/// label, a code without an official URL. Empty when the register is sound. A test runs it.
pub fn check_register() -> Vec<String> {
    let mut problems = Vec::new();
    let https = |s: &Option<String>| s.as_deref().is_some_and(|u| u.starts_with("https://"));
    let filled = |s: &Option<String>| s.as_deref().is_some_and(|t| !t.trim().is_empty());
    for l in limits() {
        let at = format!("limit {} ({})", l.id, l.market);
        if l.id.trim().is_empty() || l.unit.trim().is_empty() || !l.value.is_finite() {
            problems.push(format!("{at}: needs an id, a unit and a finite value"));
        }
        if !markets().contains(&l.market.as_str()) && l.market != "any" {
            problems.push(format!("{at}: unknown market"));
        }
        match l.basis.as_str() {
            "code" => {
                if !filled(&l.code) || !filled(&l.clause) || !https(&l.source_url) {
                    problems.push(format!(
                        "{at}: a code value needs the code, the clause and an https source"
                    ));
                }
                if l.market == "any" {
                    problems.push(format!("{at}: a code value belongs to a market"));
                }
            }
            "practice_default" => {
                if !filled(&l.rationale) {
                    problems.push(format!("{at}: a practice default needs its rationale"));
                }
                if l.clause.is_some() || l.source_url.is_some() || l.code.is_some() {
                    problems.push(format!(
                        "{at}: a practice default must not carry a code, clause or source"
                    ));
                }
            }
            other => problems.push(format!(
                "{at}: basis {other:?} is neither code nor practice_default"
            )),
        }
    }
    for id in limit_ids() {
        if !limits().iter().any(|l| l.id == id && l.market == "any") {
            problems.push(format!(
                "limit {id}: no practice default for markets without a value"
            ));
        }
    }
    if let Some(markets) = register()["markets"].as_object() {
        for (market, body) in markets {
            for code in body["codes"].as_array().into_iter().flatten() {
                let id = code["id"].as_str().unwrap_or("?");
                let at = format!("code {id} ({market})");
                for field in ["title_en", "title_ar", "publisher", "evidence_note", "use"] {
                    if code[field].as_str().is_none_or(|t| t.trim().is_empty()) {
                        problems.push(format!("{at}: {field} is empty"));
                    }
                }
                for field in ["official_url", "evidence_url"] {
                    if !code[field]
                        .as_str()
                        .is_some_and(|u| u.starts_with("https://"))
                    {
                        problems.push(format!("{at}: {field} must be an https address"));
                    }
                }
                if !matches!(code["status"].as_str(), Some("read" | "named_in_source")) {
                    problems.push(format!("{at}: status must be read or named_in_source"));
                }
            }
        }
    } else {
        problems.push("the register has no markets".into());
    }
    problems
}

/// A project type's key (`villa`) for a name or alias as a person writes it ("villa",
/// "فيلا"); `None` when the data has no such type.
pub fn project_type_key(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase();
    let types = standards()["project_types"].as_object()?;
    if types.contains_key(&t) {
        return Some(t);
    }
    types.iter().find_map(|(key, body)| {
        body["aliases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|alias| t == alias.to_lowercase())
            .then(|| key.clone())
    })
}

/// The sheet set of a project type: numbered sheets in discipline order, each with what it
/// shows and the standard of its view type. `storeys` drops the sheets that need more floors.
pub fn sheet_set(project_type: &str, storeys: Option<u32>) -> Result<Value, CoreError> {
    let Some(key) = project_type_key(project_type) else {
        return Ok(json!({
            "project_type": project_type, "sheets": [], "sheet_count": 0,
            "available_project_types": project_type_names(),
            "note": format!("No packaged template for {project_type}; build the package from the brief."),
            "note_ar": format!("لا توجد حزمة جاهزة لنوع {project_type}؛ كوّن الحزمة من متطلبات المشروع.")
        }));
    };
    let body = &standards()["project_types"][&key];
    let order: Vec<&str> = body["order"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let views = &standards()["views"];
    let mut sheets: Vec<Value> = body["sheets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| sheet_applies(s["when"].as_str(), storeys))
        .cloned()
        .collect();
    sheets.sort_by_key(|s| {
        let rank = order
            .iter()
            .position(|d| Some(*d) == s["discipline"].as_str())
            .unwrap_or(order.len());
        (rank, s["number"].as_str().unwrap_or("").to_string())
    });
    for sheet in &mut sheets {
        let view = sheet["view"].as_str().unwrap_or("").to_string();
        let include: Vec<Value> = views[&view]["include"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["id"].as_str().map(|id| json!(id)))
            .collect();
        sheet["view_standard"] = json!(include);
        sheet["status"] = json!("For review");
    }
    Ok(json!({
        "project_type": key,
        "title_en": body["title_en"],
        "title_ar": body["title_ar"],
        "storeys": storeys,
        "conditional_sheets": if storeys.is_none() { body["sheets"].as_array().into_iter().flatten().filter(|s| s.get("when").is_some()).cloned().collect::<Vec<_>>() } else { Vec::new() },
        "sheet_count": sheets.len(),
        "sheets": sheets,
        "numbering": "G general, A architectural, S structural, M mechanical, E electrical, P plumbing; 001 for notes and indexes, 1xx plans, 2xx elevations, 3xx sections, 4xx enlarged plans, 5xx details, 6xx schedules.",
        "by_engineer": "Sheets marked by_engineer are indicative layouts: sizes, loads and ratings are the discipline engineer's.",
        "view_standards": "Read topic views for what each view type includes.",
        "status": "For review",
    }))
}

/// Names of the project types, for messages.
pub fn project_type_names() -> Vec<String> {
    standards()["project_types"]
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// A condition of the form `storeys >= 2`. Unknown storeys leave conditional sheets unselected.
fn sheet_applies(when: Option<&str>, storeys: Option<u32>) -> bool {
    let Some(when) = when else { return true };
    match when
        .strip_prefix("storeys >= ")
        .and_then(|n| n.trim().parse::<u32>().ok())
    {
        Some(n) => storeys.is_some_and(|storeys| storeys >= n),
        None => true,
    }
}

/// What the data says to include in a view type (`plan`, `elevation`, `section`, `detail`,
/// `schedule`, `site`), or in all of them.
pub fn view_standards(view: Option<&str>) -> Result<Value, CoreError> {
    let views = &standards()["views"];
    match view {
        None => Ok(views.clone()),
        Some(name) => {
            let key = name.trim().to_lowercase();
            let body = views.get(&key).ok_or_else(|| {
                invalid(format!(
                    "No view standard {name:?}: plan, elevation, section, detail, schedule or site"
                ))
            })?;
            let mut one = serde_json::Map::new();
            one.insert(key, body.clone());
            Ok(Value::Object(one))
        }
    }
}

/// An uncertain room-kind suggestion for a name, from the data's keyword rules (first match), and whether the name
/// says master. Names in English and Arabic.
pub fn room_kind(name: &str) -> (Option<&'static str>, bool) {
    let sp = &standards()["space_planning"];
    let lower = name.to_lowercase();
    let master = sp["master_words"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|w| lower.contains(w));
    let kind = sp["kind_rules"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|rule| {
            rule["any"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|w| lower.contains(w))
        })
        .and_then(|rule| rule["kind"].as_str());
    (kind, master)
}

/// Kind names present in the optional suggestion data, without restricting caller classifications.
pub fn suggested_kinds() -> Vec<&'static str> {
    standards()["space_planning"]["kind_rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["kind"].as_str())
        .collect()
}

/// A space-planning threshold (practice default) by id.
pub fn threshold(id: &str) -> Option<f64> {
    standards()["space_planning"]["thresholds"][id]["value"].as_f64()
}

/// The list of kinds in `space_planning.<field>` (circulation_kinds, living_kinds, wet_kinds).
pub fn kind_list(field: &str) -> Vec<String> {
    standards()["space_planning"][field]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}

/// Words in a name that mean a wardrobe.
pub fn wardrobe_words() -> Vec<String> {
    kind_list("wardrobe_words")
}

/// The market's codes, as the register states them.
pub fn codes_of(market: &str) -> Vec<Value> {
    normalise_market(market)
        .and_then(|m| register()["markets"][m]["codes"].as_array().cloned())
        .unwrap_or_default()
}

/// The lines Mizan puts in a sheet's general notes: the status, the responsibility notice, the
/// codes applied for the market, the practice-default line and the numbered assumptions.
/// Never mentions anything but the data. Returns English or Arabic lines.
pub fn general_notes(
    market: Option<&str>,
    language: LabelLanguage,
    assumptions: &[String],
) -> Vec<String> {
    let notes = &standards()["general_notes"];
    let pick = |field: &str| -> String {
        let key = if language == LabelLanguage::Ar {
            "ar"
        } else {
            "en"
        };
        notes[field][key].as_str().unwrap_or("").to_string()
    };
    let digits = |text: String| {
        if language == LabelLanguage::Ar {
            crate::numbers::arabic_numerals(&text)
        } else {
            text
        }
    };
    let mut lines = vec![pick("status"), pick("responsibility")];
    let codes = market.map(codes_of).unwrap_or_default();
    if let Some(name) = market.filter(|name| normalise_market(name).is_none()) {
        lines.push(if language == LabelLanguage::Ar {
            format!("الاختصاص {name} غير مدعوم في السجل المرفق.")
        } else {
            format!("Jurisdiction {name} is unsupported by the packaged register.")
        });
    }
    if !codes.is_empty() {
        let title = if language == LabelLanguage::Ar {
            "title_ar"
        } else {
            "title_en"
        };
        let names: Vec<&str> = codes.iter().filter_map(|c| c[title].as_str()).collect();
        lines.push(format!("{} {}", pick("codes_prefix"), names.join("; ")));
    }
    lines.push(pick("practice_line"));
    if !assumptions.is_empty() {
        lines.push(pick("assumptions_prefix"));
        for (i, text) in assumptions.iter().enumerate() {
            lines.push(digits(format!("A{}. {}", i + 1, text.trim())));
        }
    }
    lines
}

/// What `drafting_knowledge` can answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeTopic {
    /// The topics, project types and markets.
    Overview,
    /// The numbered sheet list of a project type.
    SheetSet,
    /// What each view type includes.
    Views,
    /// Drawing symbol sizes, layers and defaults.
    Symbols,
    /// Material hatches for sections and details.
    Materials,
    /// The space-planning thresholds and room-kind words.
    SpacePlanning,
    /// The code register of a market, with each code's source.
    Codes,
    /// The numeric limits of a market with their sources or practice-default labels.
    Limits,
    /// The lines for a sheet's general notes: status, responsibility, codes, assumptions.
    GeneralNotes,
}

/// `Command::DraftingKnowledge`.
pub fn knowledge(
    topic: KnowledgeTopic,
    project_type: Option<&str>,
    market: Option<&str>,
    storeys: Option<u32>,
    view: Option<&str>,
    language: LabelLanguage,
    assumptions: &[String],
) -> Result<Value, CoreError> {
    let std = standards();
    match topic {
        KnowledgeTopic::Overview => Ok(json!({
            "notice": std["notice"],
            "topics": ["overview", "sheet_set", "views", "symbols", "materials", "space_planning", "codes", "limits", "general_notes"],
            "project_types": project_type_names(),
            "markets": markets(),
            "how_to_use": "Packaged entries are optional references. The brief determines the drawing package and applicable profiles.",
            "how_to_use_ar": "المحتوى المرفق مراجع اختيارية. متطلبات المشروع تحدد حزمة الرسومات والملفات المناسبة.",
        })),
        KnowledgeTopic::SheetSet => {
            sheet_set(project_type.unwrap_or("unspecified / غير محدد"), storeys)
        }
        KnowledgeTopic::Views => view_standards(view),
        KnowledgeTopic::Symbols => Ok(std["symbols"].clone()),
        KnowledgeTopic::Materials => Ok(std["materials"].clone()),
        KnowledgeTopic::SpacePlanning => Ok(json!({
            "profiles": std["space_planning"]["profiles"],
            "thresholds": std["space_planning"]["thresholds"],
            "circulation_kinds": std["space_planning"]["circulation_kinds"],
            "living_kinds": std["space_planning"]["living_kinds"],
            "suggested_kinds": suggested_kinds(),
            "market": market,
            "supported": market.is_none_or(|m| normalise_market(m).is_some()),
            "note_market": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("Unsupported jurisdiction: {m}.")),
            "note_market_ar": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("اختصاص غير مدعوم: {m}.")),
            "limits_available": resolved_limits(market),
            "note": "Profiles are optional practice preferences, not code. Select a compatible profile explicitly.",
            "note_ar": "الملفات قيم ممارسة اختيارية وليست كوداً. اختر ملفاً متوافقاً بشكل صريح.",
        })),
        KnowledgeTopic::Codes => {
            let markets: BTreeMap<String, Value> = match market.and_then(normalise_market) {
                Some(m) => [(m.to_string(), register()["markets"][m].clone())].into(),
                None if market.is_some() => BTreeMap::new(),
                None => register()["markets"]
                    .as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default(),
            };
            Ok(
                json!({ "market": market, "supported": market.is_none_or(|m| normalise_market(m).is_some()), "policy": register()["policy"], "markets": markets,
                "note": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("Unsupported jurisdiction: {m}.")),
                "note_ar": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("اختصاص غير مدعوم: {m}.")) }),
            )
        }
        KnowledgeTopic::Limits => {
            let resolved = resolved_limits(market);
            Ok(json!({
                "market": market,
                "supported": market.is_none_or(|m| normalise_market(m).is_some()),
                "note": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("Unsupported jurisdiction: {m}.")),
                "note_ar": market.filter(|m| normalise_market(m).is_none()).map(|m| format!("اختصاص غير مدعوم: {m}.")),
                "policy": register()["policy"],
                "limits": resolved,
            }))
        }
        KnowledgeTopic::GeneralNotes => Ok(json!({
            "market": market,
            "language": if language == LabelLanguage::Ar { "ar" } else { "en" },
            "lines": general_notes(market, language, assumptions),
            "use": "Optional sheet-note text; listed assumptions are supplied by the caller.",
            "use_ar": "نص اختياري لملاحظات اللوحة؛ الافتراضات المدرجة مقدمة من المستدعي.",
        })),
    }
}

/// The limit ids the space checks read.
pub fn space_limit_ids() -> [&'static str; 10] {
    [
        "room_area_min_m2",
        "room_width_min_m",
        "kitchen_area_min_m2",
        "kitchen_width_min_m",
        "bathroom_area_min_m2",
        "bathroom_width_min_m",
        "wc_area_min_m2",
        "wc_width_min_m",
        "corridor_width_min_m",
        "door_width_entrance_min_m",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_register_is_sound() {
        let problems = check_register();
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn markets_are_read_from_how_people_write_them() {
        assert_eq!(normalise_market("Egypt"), Some("egypt"));
        assert_eq!(normalise_market("مصر"), Some("egypt"));
        assert_eq!(normalise_market("Riyadh, KSA"), Some("ksa"));
        assert_eq!(normalise_market("Dubai"), Some("uae"));
        assert_eq!(normalise_market("Mars"), None);
    }

    #[test]
    fn a_market_without_its_own_value_gets_the_labelled_default() {
        let egypt = limit_for("stair_riser_max_m", Some("Egypt")).unwrap();
        assert_eq!(egypt.limit.basis, "code");
        assert!(egypt.limit.evidence_read);
        assert!(!egypt.fallback);
        assert_eq!(
            egypt.limit.clause.as_deref(),
            Some("Article 100 (text substituted by decree 109/2013)")
        );
        assert!((egypt.value() - 0.17).abs() < 1e-12);
        let ksa = limit_for("stair_riser_max_m", Some("ksa")).unwrap();
        assert_eq!(ksa.limit.basis, "practice_default");
        assert!(ksa.fallback);
    }

    #[test]
    fn room_kinds_come_from_the_data_in_both_languages() {
        assert_eq!(room_kind("Guest WC").0, Some("guest_wc"));
        assert_eq!(room_kind("Master Bedroom"), (Some("bedroom"), true));
        assert_eq!(room_kind("غرفة نوم رئيسية"), (Some("bedroom"), true));
        assert_eq!(room_kind("مطبخ").0, Some("kitchen"));
        assert_eq!(room_kind("Entrance Hall").0, Some("lobby"));
        assert_eq!(room_kind("Hallway").0, Some("corridor"));
    }
}
