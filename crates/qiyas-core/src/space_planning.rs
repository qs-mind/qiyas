//! Space-planning checks (W5): what a senior sees at a glance in a house plan. They run like
//! `verify` and `brief_check`: read-only, each failing check is a finding with a plain fix, and
//! every threshold is data (`data/drafting-standards.json`, `data/code-register.json`), never code.
//!
//! The input is a plan model: floors, their rooms with areas and sizes in metres, the doors that
//! matter and the stairs. Mizan hands it over from what it designed, or the model is read from
//! the drawing's rooms (`Session::space_check` with `floors`). A check that lacks the data it
//! needs says so in `not_checked`; it never passes silently.
//!
//! Qiyas draws and checks the plan against stated limits. The engineer and the company approve.

use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::EntityType;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::drafting_standards::{
    kind_list, limit_for, room_kind, standards, wardrobe_words, ResolvedLimit,
};
use crate::rooms::{LabelLanguage, RoomsQuery};
use crate::walls::OpeningKind;
use crate::{CoreError, Session};

const MAX_FLOORS: usize = 20;
const MAX_ROOMS: usize = 300;

/// A plan to check: floors with their rooms, doors and stairs. Sizes are in metres and square metres.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanModel {
    #[serde(default)]
    pub floors: Vec<PlanFloor>,
}

/// One floor of the plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanFloor {
    /// For example "Ground floor".
    pub name: String,
    #[serde(default)]
    pub rooms: Vec<PlanRoom>,
    #[serde(default)]
    pub doors: Vec<PlanDoor>,
    #[serde(default)]
    pub stairs: Vec<PlanStair>,
}

/// A room. Explicit classification wins; name matching is only an uncertain suggestion.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanRoom {
    pub name: String,
    /// Any classification supplied by the caller.
    #[serde(default)]
    pub kind: Option<String>,
    /// Explicit designation; a name does not establish design intent.
    #[serde(default)]
    pub master: Option<bool>,
    pub area_m2: f64,
    /// The narrowest clear side, metres.
    #[serde(default)]
    pub width_m: Option<f64>,
    /// The longest side, metres.
    #[serde(default)]
    pub length_m: Option<f64>,
    /// Centre `[x, y]` in metres, for distances between rooms.
    #[serde(default)]
    pub centre_m: Option<[f64; 2]>,
    /// Names of the rooms this room has a door to. A bathroom that opens only to one bedroom is
    /// its en-suite.
    #[serde(default)]
    pub opens_to: Vec<String>,
    /// Names of the rooms it shares a wall with.
    #[serde(default)]
    pub adjacent: Vec<String>,
    /// What is drawn in the room: "wardrobe", "WC", "shower"…
    #[serde(default)]
    pub fixtures: Vec<String>,
}

/// A door that matters to the checks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanDoor {
    #[serde(default)]
    pub name: Option<String>,
    /// The room it opens into, by name.
    #[serde(default)]
    pub to: Option<String>,
    /// Clear leaf width, or both leaves together, metres.
    pub width_m: f64,
    /// 1 or 2.
    #[serde(default)]
    pub leaves: Option<u8>,
    #[serde(default)]
    pub main_entrance: bool,
    /// Position `[x, y]` in metres.
    #[serde(default)]
    pub position_m: Option<[f64; 2]>,
    /// What its swing runs into, in words ("stair landing"), when it does.
    #[serde(default)]
    pub swing_conflict: Option<String>,
}

/// A stair on a floor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanStair {
    #[serde(default)]
    pub name: Option<String>,
    /// Clear width of the flight, metres.
    #[serde(default)]
    pub width_m: Option<f64>,
    /// The room it stands in, by name (a corridor or hall, or its own stair room).
    #[serde(default)]
    pub located_in: Option<String>,
    /// True when walls enclose it.
    #[serde(default)]
    pub enclosed: Option<bool>,
    /// Clear passage left beside the stair in its corridor or hall, metres.
    #[serde(default)]
    pub passage_beside_m: Option<f64>,
    #[serde(default)]
    pub riser_m: Option<f64>,
    #[serde(default)]
    pub going_m: Option<f64>,
    #[serde(default)]
    pub steps_per_flight: Option<u32>,
    #[serde(default)]
    pub headroom_m: Option<f64>,
    /// Distance from the nearest door to the first or last step, metres.
    #[serde(default)]
    pub door_distance_m: Option<f64>,
}

/// A floor to read from the drawing's rooms.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorQuery {
    #[serde(default, flatten)]
    pub boundary_layers: crate::object_regions::BoundaryLayers,
    pub name: String,
    /// `[[min_x, min_y], [max_x, max_y]]` in drawing units: the floor's part of the sheet.
    #[serde(default)]
    pub within: Option<[[f64; 2]; 2]>,
    /// The layer of the room-name texts.
    #[serde(default)]
    pub text_layer: Option<String>,
}

/// One finding: what is wrong, why, and the fix in plain words, in both languages.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpaceFinding {
    pub rule: &'static str,
    /// error, warning or note.
    pub severity: &'static str,
    pub floor: String,
    pub rooms: Vec<String>,
    pub message: String,
    pub message_ar: String,
    pub fix: String,
    pub fix_ar: String,
    pub measured: Option<f64>,
    pub limit: Option<f64>,
    /// code or practice_default.
    pub basis: &'static str,
    /// The code and clause when the limit is a code requirement.
    pub source: Option<String>,
    pub source_url: Option<String>,
    /// Qualifications attached to the cited code limit.
    pub caveat: Option<String>,
}

/// The result of the checks.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpaceReport {
    pub findings: Vec<SpaceFinding>,
    pub checked: Vec<&'static str>,
    pub not_checked: Vec<Value>,
    pub errors: usize,
    pub warnings: usize,
    pub notes: usize,
    pub summary: String,
    pub summary_ar: String,
    pub market: Option<String>,
    pub limits_used: Vec<Value>,
    pub status: &'static str,
    pub responsibility: &'static str,
    pub profile: Option<String>,
    pub building_type: Option<String>,
    pub available_profiles: Vec<String>,
    pub thresholds_applied: Vec<Value>,
    pub status_ar: &'static str,
    pub responsibility_ar: &'static str,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

fn same(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A number to `dp` places, Western digits.
fn e(x: f64, dp: usize) -> String {
    format!("{x:.dp$}")
}

/// A number to `dp` places in Arabic digits with the Arabic decimal separator.
fn a(x: f64, dp: usize) -> String {
    crate::numbers::arabic_digits(&format!("{x:.dp$}"))
}

/// A room with its kind worked out.
struct R<'p> {
    room: &'p PlanRoom,
    kind: Option<&'p str>,
    master: bool,
}

struct Fl<'p> {
    floor: &'p PlanFloor,
    rooms: Vec<R<'p>>,
}

impl<'p> Fl<'p> {
    fn of_kind(&self, kinds: &[&str]) -> Vec<&R<'p>> {
        self.rooms
            .iter()
            .filter(|r| r.kind.is_some_and(|k| kinds.contains(&k)))
            .collect()
    }

    fn find(&self, name: &str) -> Option<&R<'p>> {
        self.rooms.iter().find(|r| same(&r.room.name, name))
    }

    fn kind_of(&self, name: &str) -> Option<&'p str> {
        self.find(name).and_then(|r| r.kind)
    }
}

struct Ctx {
    profile: Option<&'static Value>,
    findings: Vec<SpaceFinding>,
    checked: BTreeSet<&'static str>,
    skipped: Vec<Value>,
    used: BTreeMap<String, Value>,
    thresholds: BTreeMap<String, Value>,
}

impl Ctx {
    fn threshold(&mut self, id: &str) -> Option<f64> {
        let entry = self.profile?["thresholds"].get(id)?;
        let value = entry["value"].as_f64()?;
        self.thresholds
            .insert(id.into(), json!({"id":id,"threshold":entry}));
        Some(value)
    }

    fn limit(&mut self, id: &str) -> Option<ResolvedLimit> {
        let l = limit_for(id, None)?;
        self.used.insert(id.to_string(), l.json());
        Some(l)
    }

    fn skip(&mut self, rule: &'static str, why: impl Into<String>) {
        let why = why.into();
        if !self
            .skipped
            .iter()
            .any(|s| s["rule"] == rule && s["why"] == why.as_str())
        {
            self.skipped.push(json!({"rule": rule, "why": why}));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        rule: &'static str,
        severity: &'static str,
        floor: &str,
        rooms: Vec<String>,
        say: (String, String),
        fix: (String, String),
        measured: Option<f64>,
        limit: Option<&ResolvedLimit>,
        limit_value: Option<f64>,
    ) {
        let (basis, source, value) = match limit {
            Some(l) => (
                if l.limit.basis == "code" {
                    "code"
                } else {
                    "practice_default"
                },
                (l.limit.basis == "code").then(|| {
                    format!(
                        "{}, {}",
                        l.limit.code.clone().unwrap_or_default(),
                        l.limit.clause.clone().unwrap_or_default()
                    )
                }),
                Some(l.value()),
            ),
            None => ("practice_default", None, limit_value),
        };
        let (severity, say) = if basis == "practice_default" {
            (
                if severity == "error" {
                    "warning"
                } else {
                    severity
                },
                (
                    format!("{} (practice default).", say.0),
                    format!("{} (قيمة ممارسة معتادة).", say.1),
                ),
            )
        } else {
            (severity, say)
        };
        let cited = limit.filter(|l| l.limit.basis == "code");
        self.findings.push(SpaceFinding {
            rule,
            severity,
            floor: floor.to_string(),
            rooms,
            message: say.0,
            message_ar: say.1,
            fix: fix.0,
            fix_ar: fix.1,
            measured,
            limit: value,
            basis,
            source,
            source_url: cited.and_then(|l| l.limit.source_url.clone()),
            caveat: cited.and_then(|l| l.limit.note.clone()),
        });
    }
}

/// Error for a code requirement, warning for a practice default: only a code can be broken.
fn sev(limit: &ResolvedLimit) -> &'static str {
    if limit.limit.basis == "code" {
        "error"
    } else {
        "warning"
    }
}

fn check_model(plan: &PlanModel) -> Result<(), CoreError> {
    if plan.floors.is_empty() || plan.floors.len() > MAX_FLOORS {
        return Err(invalid(format!("A plan has 1 to {MAX_FLOORS} floors")));
    }
    for floor in &plan.floors {
        if floor.name.trim().is_empty() {
            return Err(invalid("Each floor needs a name"));
        }
        if floor.rooms.len() > MAX_ROOMS {
            return Err(invalid(format!(
                "Floor {} has more than {MAX_ROOMS} rooms",
                floor.name
            )));
        }
        for room in &floor.rooms {
            if room.name.trim().is_empty() || !room.area_m2.is_finite() || room.area_m2 < 0.0 {
                return Err(invalid(format!(
                    "Each room needs a name and a non-negative area (floor {})",
                    floor.name
                )));
            }
            for v in [room.width_m, room.length_m].into_iter().flatten() {
                if !v.is_finite() || v < 0.0 {
                    return Err(invalid(format!(
                        "Room {}: sizes are non-negative metres",
                        room.name
                    )));
                }
            }
            if room
                .centre_m
                .is_some_and(|p| p.into_iter().any(|v| !v.is_finite()))
            {
                return Err(invalid("Room centres must be finite metres"));
            }
        }
        for door in &floor.doors {
            if !door.width_m.is_finite() || door.width_m <= 0.0 {
                return Err(invalid("A door needs a positive width in metres"));
            }
            if door.leaves.is_some_and(|n| !matches!(n, 1 | 2))
                || door
                    .position_m
                    .is_some_and(|p| p.into_iter().any(|v| !v.is_finite()))
            {
                return Err(invalid(
                    "A door has one or two leaves and a finite position",
                ));
            }
        }
        for stair in &floor.stairs {
            if [
                stair.width_m,
                stair.riser_m,
                stair.going_m,
                stair.headroom_m,
            ]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v <= 0.0)
                || [stair.passage_beside_m, stair.door_distance_m]
                    .into_iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v < 0.0)
                || stair.steps_per_flight == Some(0)
            {
                return Err(invalid(
                    "Stair sizes must be finite positive metres; clearances may be zero",
                ));
            }
        }
    }
    Ok(())
}

fn prepare(floor: &PlanFloor) -> Fl<'_> {
    let rooms = floor
        .rooms
        .iter()
        .map(|room| R {
            room,
            kind: room.kind.as_deref(),
            master: room.master.unwrap_or(false),
        })
        .collect();
    Fl { floor, rooms }
}

/// Apply an explicitly selected, compatible practice profile. Otherwise report no judgements.
///
/// # Errors
/// The plan is not well formed (for example no floors or a non-finite dimension).
pub fn run_checks(
    plan: &PlanModel,
    market: Option<&str>,
    profile: Option<&str>,
    building_type: Option<&str>,
) -> Result<SpaceReport, CoreError> {
    check_model(plan)?;
    let profiles = &standards()["space_planning"]["profiles"];
    let available_profiles: Vec<String> = profiles
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys().cloned())
        .collect();
    let selected = profile.filter(|name| {
        profiles.get(*name).is_some_and(|p| {
            building_type.is_some_and(|kind| {
                p["applies_to"].as_array().is_some_and(|types| {
                    types
                        .iter()
                        .any(|t| t.as_str().is_some_and(|t| same(t, kind)))
                })
            })
        })
    });
    let mut cx = Ctx {
        profile: selected.and_then(|name| profiles.get(name)),
        findings: Vec::new(),
        checked: BTreeSet::new(),
        skipped: Vec::new(),
        used: BTreeMap::new(),
        thresholds: BTreeMap::new(),
    };
    let floors: Vec<Fl> = plan.floors.iter().map(prepare).collect();
    if selected.is_some() {
        for fl in &floors {
            circulation_share(&mut cx, fl);
            corridors(&mut cx, fl);
            room_sizes(&mut cx, fl);
            kitchen_and_living(&mut cx, fl);
            oversized(&mut cx, fl);
            stairs(&mut cx, fl);
            doors(&mut cx, fl);
            guest_wc(&mut cx, fl);
        }
        master_ensuite(&mut cx, &floors);
        bathroom_ratio(&mut cx, &floors);
        wardrobes(&mut cx, &floors);
    }
    let (errors, warnings, notes) =
        cx.findings
            .iter()
            .fold((0, 0, 0), |(e, w, n), f| match f.severity {
                "error" => (e + 1, w, n),
                "warning" => (e, w + 1, n),
                _ => (e, w, n + 1),
            });
    let summary = if selected.is_none() {
        format!(
            "Facts only. Available profiles: {}.",
            available_profiles.join(", ")
        )
    } else if cx.findings.is_empty() {
        format!("{} checks ran; no finding.", cx.checked.len())
    } else {
        format!(
            "{errors} errors, {warnings} warnings, {notes} notes from {} checks.",
            cx.checked.len()
        )
    };
    let summary_ar = crate::numbers::arabic_numerals(&if selected.is_none() {
        format!(
            "حقائق فقط. الملفات المتاحة: {}.",
            available_profiles.join(", ")
        )
    } else if cx.findings.is_empty() {
        format!("عدد الفحوص المنفذة دون ملاحظات: {}.", cx.checked.len())
    } else {
        format!(
            "نتائج الفحوص: {errors} خطأ و {warnings} تحذير و {notes} ملاحظة. إجمالي الفحوص: {}.",
            cx.checked.len()
        )
    });
    Ok(SpaceReport {
        profile: selected.map(String::from),
        building_type: building_type.map(String::from),
        available_profiles,
        thresholds_applied: cx.thresholds.into_values().collect(),
        status_ar: "للمراجعة",
        responsibility_ar: "القيم المطبقة تفضيلات ممارسة وليست كودا. يقرر المهندس مدى ملاءمتها.",
        errors,
        warnings,
        notes,
        summary,
        summary_ar,
        market: market.map(String::from),
        checked: cx.checked.into_iter().collect(),
        not_checked: cx.skipped,
        limits_used: cx.used.into_values().collect(),
        findings: cx.findings,
        status: "For review",
        responsibility: "Applied thresholds are practice preferences, not code. The engineer decides their suitability.",
    })
}

impl SpaceReport {
    /// The report as JSON.
    pub fn json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    /// True when a finding of this rule exists.
    pub fn has(&self, rule: &str) -> bool {
        self.findings.iter().any(|f| f.rule == rule)
    }
}

// ---- floor checks -----------------------------------------------------------------------

fn circulation_share(cx: &mut Ctx, fl: &Fl) {
    let total: f64 = fl.rooms.iter().map(|r| r.room.area_m2).sum();
    let Some(min_spaces) = cx.threshold("circulation_min_spaces") else {
        return;
    };
    let known_kinds = crate::drafting_standards::suggested_kinds();
    if (fl.rooms.len() as f64) < min_spaces
        || total <= 0.0
        || fl
            .rooms
            .iter()
            .any(|r| r.kind.is_none_or(|k| !known_kinds.contains(&k)))
    {
        cx.skip(
            "circulation_share",
            format!("{}: insufficient classified spaces for this profile / فراغات مصنفة غير كافية لهذا الملف", fl.floor.name),
        );
        return;
    }
    cx.checked.insert("circulation_share");
    let kinds = kind_list("circulation_kinds");
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let circulation: Vec<&R> = fl.of_kind(&kinds);
    let area: f64 = circulation.iter().map(|r| r.room.area_m2).sum();
    let share = area / total;
    let Some(warn) = cx.threshold("circulation_share_warn") else {
        return;
    };
    let Some(error) = cx.threshold("circulation_share_error") else {
        return;
    };
    if share <= warn {
        return;
    }
    let excess = area - warn * total;
    let names: Vec<String> = circulation.iter().map(|r| r.room.name.clone()).collect();
    cx.add(
        "circulation_share",
        if share > error { "error" } else { "warning" },
        &fl.floor.name,
        names,
        (
            format!(
                "{}: corridors and halls take {} m² of {} m², {}% of the floor (practice limit {}%).",
                fl.floor.name,
                e(area, 1),
                e(total, 1),
                e(share * 100.0, 0),
                e(warn * 100.0, 0)
            ),
            format!(
                "{}: الممرات والصالات تأخذ {} م² من {} م²، أي {}٪ من الدور (الحد المعتاد {}٪).",
                fl.floor.name,
                a(area, 1),
                a(total, 1),
                a(share * 100.0, 0),
                a(warn * 100.0, 0)
            ),
        ),
        (
            format!(
                "Take about {} m² out of the passage: open the rooms onto one compact hall, drop the long corridor, and give the stair its own enclosure.",
                e(excess, 0)
            ),
            format!(
                "قلل حوالي {} م² من الممرات: افتح الغرف على صالة واحدة مضغوطة، واحذف الممر الطويل، وأعط السلم حيزا محاطا بجدران.",
                a(excess, 0)
            ),
        ),
        Some(share),
        None,
        Some(warn),
    );
}

fn corridors(cx: &mut Ctx, fl: &Fl) {
    let corridors = fl.of_kind(&["corridor"]);
    if corridors.is_empty() {
        return;
    }
    let Some(max_len) = cx.threshold("corridor_length_max_m") else {
        return;
    };
    let min_width = cx.limit("corridor_width_min_m");
    for r in corridors {
        let width = r.room.width_m;
        let length = r.room.length_m;
        match length {
            Some(length) => {
                cx.checked.insert("corridor_length");
                if length > max_len {
                    cx.add(
                        "corridor_length",
                        "warning",
                        &fl.floor.name,
                        vec![r.room.name.clone()],
                        (
                            format!("{} is {} m long (practice limit {} m): a corridor plan with rooms on both sides.", r.room.name, e(length, 1), e(max_len, 0)),
                            format!("{} طوله {} م (الحد المعتاد {} م): مسقط ممرات تفتح عليه الغرف من الجانبين.", r.room.name, a(length, 1), a(max_len, 0)),
                        ),
                        (
                            "Group the rooms round a central hall so no passage is longer than a few metres.".into(),
                            "جمع الغرف حول صالة مركزية بحيث لا يزيد أي ممر عن أمتار قليلة.".into(),
                        ),
                        Some(length),
                        None,
                        Some(max_len),
                    );
                }
            }
            None => cx.skip(
                "corridor_length",
                format!("{}: no length or width given", r.room.name),
            ),
        }
        match (width, &min_width) {
            (Some(w), Some(limit)) => {
                cx.checked.insert("corridor_width");
                if w < limit.value() {
                    cx.add(
                        "corridor_width",
                        sev(limit),
                        &fl.floor.name,
                        vec![r.room.name.clone()],
                        (
                            format!(
                                "{} is {} m wide; the least is {} m.",
                                r.room.name,
                                e(w, 2),
                                e(limit.value(), 2)
                            ),
                            format!(
                                "{} عرضه {} م؛ والحد الأدنى {} م.",
                                r.room.name,
                                a(w, 2),
                                a(limit.value(), 2)
                            ),
                        ),
                        (
                            format!("Widen it to {} m or more.", e(limit.value(), 2)),
                            format!("وسعه إلى {} م أو أكثر.", a(limit.value(), 2)),
                        ),
                        Some(w),
                        Some(limit),
                        None,
                    );
                }
            }
            _ => cx.skip("corridor_width", format!("{}: no width given", r.room.name)),
        }
    }
}

/// The limit ids for a room kind: area and narrowest side.
fn size_limits(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "living" | "majlis" | "reception" | "dining" | "bedroom" => {
            Some(("room_area_min_m2", "room_width_min_m"))
        }
        "kitchen" => Some(("kitchen_area_min_m2", "kitchen_width_min_m")),
        "bathroom" => Some(("bathroom_area_min_m2", "bathroom_width_min_m")),
        "wc" | "guest_wc" => Some(("wc_area_min_m2", "wc_width_min_m")),
        _ => None,
    }
}

fn room_sizes(cx: &mut Ctx, fl: &Fl) {
    for r in &fl.rooms {
        let Some((area_id, width_id)) = r.kind.and_then(size_limits) else {
            continue;
        };
        let (Some(area_limit), Some(width_limit)) = (cx.limit(area_id), cx.limit(width_id)) else {
            continue;
        };
        cx.checked.insert("room_size");
        if r.room.area_m2 < area_limit.value() {
            cx.add(
                "room_size",
                sev(&area_limit),
                &fl.floor.name,
                vec![r.room.name.clone()],
                (
                    format!(
                        "{} is {} m²; the least for this room is {} m².",
                        r.room.name,
                        e(r.room.area_m2, 2),
                        e(area_limit.value(), 2)
                    ),
                    format!(
                        "{} مساحته {} م²؛ والحد الأدنى لهذه الغرفة {} م².",
                        r.room.name,
                        a(r.room.area_m2, 2),
                        a(area_limit.value(), 2)
                    ),
                ),
                (
                    format!(
                        "Enlarge it to at least {} m², or merge it with a neighbour.",
                        e(area_limit.value(), 1)
                    ),
                    format!(
                        "كبرها إلى {} م² على الأقل، أو ادمجها مع المجاور.",
                        a(area_limit.value(), 1)
                    ),
                ),
                Some(r.room.area_m2),
                Some(&area_limit),
                None,
            );
        }
        if let Some(w) = r.room.width_m {
            if w < width_limit.value() {
                cx.add(
                    "room_size",
                    sev(&width_limit),
                    &fl.floor.name,
                    vec![r.room.name.clone()],
                    (
                        format!(
                            "{} is {} m across at its narrowest; the least is {} m.",
                            r.room.name,
                            e(w, 2),
                            e(width_limit.value(), 2)
                        ),
                        format!(
                            "{} أضيق عرض فيها {} م؛ والحد الأدنى {} م.",
                            r.room.name,
                            a(w, 2),
                            a(width_limit.value(), 2)
                        ),
                    ),
                    (
                        format!(
                            "Make the room at least {} m wide.",
                            e(width_limit.value(), 1)
                        ),
                        format!("اجعل عرض الغرفة {} م على الأقل.", a(width_limit.value(), 1)),
                    ),
                    Some(w),
                    Some(&width_limit),
                    None,
                );
            }
        }
    }
}

fn kitchen_and_living(cx: &mut Ctx, fl: &Fl) {
    let living_kinds = kind_list("living_kinds");
    let living_kinds: Vec<&str> = living_kinds.iter().map(String::as_str).collect();
    let kitchens = fl.of_kind(&["kitchen"]);
    let living = fl.of_kind(&living_kinds);
    if let (Some(kitchen), Some(biggest)) = (
        kitchens
            .iter()
            .max_by(|x, y| x.room.area_m2.total_cmp(&y.room.area_m2)),
        living
            .iter()
            .max_by(|x, y| x.room.area_m2.total_cmp(&y.room.area_m2)),
    ) {
        cx.checked.insert("kitchen_larger_than_living");
        let Some(ratio) = cx.threshold("kitchen_to_living_max_ratio") else {
            return;
        };
        if kitchen.room.area_m2 > ratio * biggest.room.area_m2 {
            cx.add(
                "kitchen_larger_than_living",
                "warning",
                &fl.floor.name,
                vec![kitchen.room.name.clone(), biggest.room.name.clone()],
                (
                    format!("The kitchen ({} m²) is larger than {} ({} m²).", e(kitchen.room.area_m2, 1), biggest.room.name, e(biggest.room.area_m2, 1)),
                    format!("المطبخ ({} م²) أكبر من {} ({} م²).", a(kitchen.room.area_m2, 1), biggest.room.name, a(biggest.room.area_m2, 1)),
                ),
                (
                    format!("Trim the kitchen to under {} m² and give the difference to the living space.", e(biggest.room.area_m2 * ratio, 0)),
                    format!("قلص المطبخ إلى أقل من {} م² وأعط الفرق لمساحة المعيشة.", a(biggest.room.area_m2 * ratio, 0)),
                ),
                Some(kitchen.room.area_m2),
                None,
                Some(biggest.room.area_m2 * ratio),
            );
        }
    }
    // Kitchen next to the dining room; the living and dining spaces connected.
    for (rule, first, second, severity) in [
        ("kitchen_far_from_dining", "kitchen", "dining", "warning"),
        ("living_dining_apart", "living", "dining", "note"),
    ] {
        let (Some(x), Some(y)) = (
            fl.of_kind(&[first]).into_iter().next(),
            fl.of_kind(&[second]).into_iter().next(),
        ) else {
            continue;
        };
        let has_data = |r: &R| !r.room.adjacent.is_empty() || !r.room.opens_to.is_empty();
        if !has_data(x) && !has_data(y) {
            cx.skip(
                rule,
                format!("{}: no adjacency or door data", fl.floor.name),
            );
            continue;
        }
        cx.checked.insert(rule);
        let linked = |p: &R, q: &R| {
            p.room
                .adjacent
                .iter()
                .chain(&p.room.opens_to)
                .any(|n| same(n, &q.room.name))
        };
        if !linked(x, y) && !linked(y, x) {
            let (en_first, en_second, ar_first, ar_second) = if rule == "kitchen_far_from_dining" {
                ("kitchen", "dining room", "المطبخ", "غرفة السفرة")
            } else {
                ("living room", "dining room", "المعيشة", "السفرة")
            };
            cx.add(
                rule,
                severity,
                &fl.floor.name,
                vec![x.room.name.clone(), y.room.name.clone()],
                (
                    format!("The {en_first} and the {en_second} do not touch or open to each other."),
                    format!("{ar_first} و {ar_second} غير متجاورتين ولا تفتحان على بعضهما."),
                ),
                (
                    format!("Put the {en_first} and the {en_second} side by side with a door or an opening between them."),
                    format!("ضع {ar_first} و {ar_second} متجاورتين مع باب أو فتحة بينهما."),
                ),
                None,
                None,
                None,
            );
        }
    }
}

fn oversized(cx: &mut Ctx, fl: &Fl) {
    let Some(bedroom) = cx.threshold("bedroom_oversize_m2") else {
        return;
    };
    let Some(master) = cx.threshold("master_oversize_m2") else {
        return;
    };
    let Some(service) = cx.threshold("service_oversize_m2") else {
        return;
    };
    for r in &fl.rooms {
        let (limit, label_en, label_ar) = match r.kind {
            Some("bedroom") if r.master => (master, "master bedroom", "غرفة النوم الرئيسية"),
            Some("bedroom") => (bedroom, "bedroom", "غرفة النوم"),
            Some("utility") => (service, "service room", "غرفة الخدمات"),
            _ => continue,
        };
        cx.checked.insert("room_oversized");
        if r.room.area_m2 > limit {
            cx.add(
                "room_oversized",
                "note",
                &fl.floor.name,
                vec![r.room.name.clone()],
                (
                    format!("{} ({} m²) is large for a {label_en} (practice limit {} m²).", r.room.name, e(r.room.area_m2, 1), e(limit, 0)),
                    format!("{} ({} م²) كبيرة بالنسبة لـ {label_ar} (الحد المعتاد {} م²).", r.room.name, a(r.room.area_m2, 1), a(limit, 0)),
                ),
                (
                    "Give the extra area to the rooms that are short of it, or add a bathroom or dressing room.".into(),
                    "أعط المساحة الزائدة للغرف الناقصة، أو أضف حماما أو غرفة ملابس.".into(),
                ),
                Some(r.room.area_m2),
                None,
                Some(limit),
            );
        }
    }
}

fn stairs(cx: &mut Ctx, fl: &Fl) {
    if fl.floor.stairs.is_empty() {
        return;
    }
    let width_min = cx.limit("stair_width_min_m");
    let riser_max = cx.limit("stair_riser_max_m");
    let going_min = cx.limit("stair_going_min_m");
    let steps_max = cx.limit("stair_steps_per_flight_max");
    let head_min = cx.limit("stair_headroom_min_m");
    let door_min = cx.limit("door_to_stair_min_m");
    let corridor_min = cx.limit("corridor_width_min_m");
    for (i, s) in fl.floor.stairs.iter().enumerate() {
        let label = s.name.clone().unwrap_or_else(|| format!("Stair {}", i + 1));
        let host = s.located_in.as_deref().and_then(|n| fl.kind_of(n));
        let in_passage = matches!(host, Some("corridor" | "lobby"));
        let mut rooms = vec![label.clone()];
        rooms.extend(s.located_in.clone());
        // The stair stands in the passage and leaves too little of it.
        if in_passage {
            match (s.passage_beside_m, &corridor_min) {
                (Some(left), Some(limit)) => {
                    cx.checked.insert("stair_blocks_corridor");
                    if left < limit.value() {
                        cx.add(
                            "stair_blocks_corridor",
                            "error",
                            &fl.floor.name,
                            rooms.clone(),
                            (
                                format!("{label} stands in {} and leaves {} m of passage; at least {} m is needed.", s.located_in.clone().unwrap_or_default(), e(left, 2), e(limit.value(), 2)),
                                format!("{label} واقف في {} ويترك {} م من الممر؛ والمطلوب {} م على الأقل.", s.located_in.clone().unwrap_or_default(), a(left, 2), a(limit.value(), 2)),
                            ),
                            (
                                "Move the stair into its own enclosure off the hall, or widen the passage beside it.".into(),
                                "انقل السلم إلى حيز محاط بجدران على الصالة، أو وسع الممر بجانبه.".into(),
                            ),
                            Some(left),
                            Some(limit),
                            None,
                        );
                    }
                }
                _ => cx.skip(
                    "stair_blocks_corridor",
                    format!("{label}: passage beside the stair not given"),
                ),
            }
        }
        if let Some(enclosed) = s.enclosed {
            cx.checked.insert("stair_not_enclosed");
            if !enclosed {
                cx.add(
                    "stair_not_enclosed",
                    "warning",
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!("{label} is not enclosed by walls."),
                        format!("{label} غير محاط بجدران."),
                    ),
                    (
                        "Enclose the stair in walls on its long sides and give it a door or opening at the foot, so it does not cut the hall or corridor.".into(),
                        "أحط السلم بجدران على جانبيه الطويلين وأعطه باب أو فتحة عند أوله، حتى لا يقطع الصالة أو الممر.".into(),
                    ),
                    None,
                    None,
                    None,
                );
            }
        } else {
            cx.skip("stair_not_enclosed", format!("{label}: enclosed not given"));
        }
        if let (Some(w), Some(limit)) = (s.width_m, &width_min) {
            cx.checked.insert("stair_width");
            if w < limit.value() {
                cx.add(
                    "stair_width",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!(
                            "{label} is {} m wide; the least is {} m.",
                            e(w, 2),
                            e(limit.value(), 2)
                        ),
                        format!(
                            "{label} عرضه {} م؛ والحد الأدنى {} م.",
                            a(w, 2),
                            a(limit.value(), 2)
                        ),
                    ),
                    (
                        format!("Widen the flight to {} m or more.", e(limit.value(), 2)),
                        format!("وسع الدرج إلى {} م أو أكثر.", a(limit.value(), 2)),
                    ),
                    Some(w),
                    Some(limit),
                    None,
                );
            }
        }
        if let (Some(r), Some(limit)) = (s.riser_m, &riser_max) {
            cx.checked.insert("stair_riser");
            if r > limit.value() + 1e-9 {
                cx.add(
                    "stair_riser",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!(
                            "{label} riser is {} m; the most is {} m.",
                            e(r, 3),
                            e(limit.value(), 3)
                        ),
                        format!(
                            "ارتفاع القائمة في {label} {} م؛ والحد الأقصى {} م.",
                            a(r, 3),
                            a(limit.value(), 3)
                        ),
                    ),
                    (
                        "Add a riser to the flight so each one is lower.".into(),
                        "أضف قائمة للسلم لتصبح كل قائمة أقل ارتفاعا.".into(),
                    ),
                    Some(r),
                    Some(limit),
                    None,
                );
            }
        }
        if let (Some(g), Some(limit)) = (s.going_m, &going_min) {
            cx.checked.insert("stair_going");
            if g < limit.value() - 1e-9 {
                cx.add(
                    "stair_going",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!(
                            "{label} tread is {} m; the least is {} m.",
                            e(g, 3),
                            e(limit.value(), 3)
                        ),
                        format!(
                            "عمق النائمة في {label} {} م؛ والحد الأدنى {} م.",
                            a(g, 3),
                            a(limit.value(), 3)
                        ),
                    ),
                    (
                        "Lengthen the flight or turn part of it into a landing.".into(),
                        "أطل الدرج أو حول جزءا منه إلى بسطة.".into(),
                    ),
                    Some(g),
                    Some(limit),
                    None,
                );
            }
        }
        if let (Some(n), Some(limit)) = (s.steps_per_flight, &steps_max) {
            cx.checked.insert("stair_steps_per_flight");
            if f64::from(n) > limit.value() {
                cx.add(
                    "stair_steps_per_flight",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!(
                            "{label} has {n} steps in one flight; the most is {}.",
                            e(limit.value(), 0)
                        ),
                        format!(
                            "{label} فيه {} درجة في مرقاة واحدة؛ والحد الأقصى {}.",
                            a(f64::from(n), 0),
                            a(limit.value(), 0)
                        ),
                    ),
                    (
                        "Break the flight with a landing (an L or U stair).".into(),
                        "اكسر المرقاة ببسطة (سلم L أو U).".into(),
                    ),
                    Some(f64::from(n)),
                    Some(limit),
                    None,
                );
            }
        }
        if let (Some(h), Some(limit)) = (s.headroom_m, &head_min) {
            cx.checked.insert("stair_headroom");
            if h < limit.value() {
                cx.add(
                    "stair_headroom",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!("{label} headroom is {} m; the least is {} m.", e(h, 2), e(limit.value(), 2)),
                        format!("ارتفاع الرأس في {label} {} م؛ والحد الأدنى {} م.", a(h, 2), a(limit.value(), 2)),
                    ),
                    (
                        "Move the slab edge back over the stair, or raise the soffit above the flight.".into(),
                        "أرجع حافة البلاطة فوق السلم، أو ارفع السقف فوق المرقاة.".into(),
                    ),
                    Some(h),
                    Some(limit),
                    None,
                );
            }
        }
        if let (Some(d), Some(limit)) = (s.door_distance_m, &door_min) {
            cx.checked.insert("door_near_stair");
            if d < limit.value() {
                cx.add(
                    "door_near_stair",
                    sev(limit),
                    &fl.floor.name,
                    rooms.clone(),
                    (
                        format!(
                            "A door opens {} m from {label}; at least {} m is needed.",
                            e(d, 2),
                            e(limit.value(), 2)
                        ),
                        format!(
                            "باب يفتح على بعد {} م من {label}؛ والمطلوب {} م على الأقل.",
                            a(d, 2),
                            a(limit.value(), 2)
                        ),
                    ),
                    (
                        "Move the door, or put a landing between it and the first step.".into(),
                        "انقل الباب، أو ضع بسطة بينه وبين أول درجة.".into(),
                    ),
                    Some(d),
                    Some(limit),
                    None,
                );
            }
        }
    }
}

fn doors(cx: &mut Ctx, fl: &Fl) {
    let entrance_min = cx.limit("door_width_entrance_min_m");
    let room_min = cx.limit("door_width_min_room_m");
    let service_min = cx.limit("door_width_min_service_m");
    let mut saw_entrance = false;
    for (i, d) in fl.floor.doors.iter().enumerate() {
        let label = d.name.clone().unwrap_or_else(|| format!("Door {}", i + 1));
        let mut rooms = vec![label.clone()];
        rooms.extend(d.to.clone());
        if d.main_entrance {
            saw_entrance = true;
            if let Some(limit) = &entrance_min {
                cx.checked.insert("entrance_door_narrow");
                if d.width_m < limit.value() - 1e-9 {
                    let leaves = d.leaves;
                    cx.add(
                        "entrance_door_narrow",
                        "warning",
                        &fl.floor.name,
                        rooms.clone(),
                        (
                            format!(
                                "The main entrance is {} m ({} leaf); a villa entrance is at least {} m.",
                                e(d.width_m, 2),
                                match leaves { Some(2) => "double", Some(1) => "single", _ => "unspecified" },
                                e(limit.value(), 2)
                            ),
                            format!(
                                "باب المدخل الرئيسي {} م ({}); ومدخل الفيلا لا يقل عن {} م.",
                                a(d.width_m, 2),
                                match leaves { Some(2) => "ضلفتان", Some(1) => "ضلفة واحدة", _ => "غير محدد" },
                                a(limit.value(), 2)
                            ),
                        ),
                        (
                            "Make the entrance a double-leaf door about 1.50 m wide, or a single leaf of 1.20 m.".into(),
                            "اجعل المدخل بابا بضلفتين عرضه حوالي 1.50 م، أو ضلفة واحدة 1.20 م.".into(),
                        ),
                        Some(d.width_m),
                        Some(limit),
                        None,
                    );
                }
            }
        } else if let Some(to) = d.to.as_deref() {
            let service = matches!(
                fl.kind_of(to),
                Some("bathroom" | "wc" | "guest_wc" | "kitchen")
            );
            let limit = if service { &service_min } else { &room_min };
            if let Some(limit) = limit {
                cx.checked.insert("door_width");
                if d.width_m < limit.value() - 1e-9 {
                    cx.add(
                        "door_width",
                        sev(limit),
                        &fl.floor.name,
                        rooms.clone(),
                        (
                            format!(
                                "{label} to {to} is {} m wide; the least is {} m.",
                                e(d.width_m, 2),
                                e(limit.value(), 2)
                            ),
                            format!(
                                "{label} إلى {to} عرضه {} م؛ والحد الأدنى {} م.",
                                a(d.width_m, 2),
                                a(limit.value(), 2)
                            ),
                        ),
                        (
                            format!("Widen the door to {} m or more.", e(limit.value(), 2)),
                            format!("وسع الباب إلى {} م أو أكثر.", a(limit.value(), 2)),
                        ),
                        Some(d.width_m),
                        Some(limit),
                        None,
                    );
                }
            }
        }
        if let Some(what) = &d.swing_conflict {
            cx.checked.insert("door_swing_conflict");
            cx.add(
                "door_swing_conflict",
                "warning",
                &fl.floor.name,
                rooms,
                (
                    format!("{label} swings onto {what}."),
                    format!("{label} يفتح على {what}."),
                ),
                (
                    "Hang the door the other way, make it sliding, or move it clear of the swing."
                        .into(),
                    "اعكس اتجاه فتح الباب، أو اجعله منزلقا، أو انقله بعيدا عن مجال الفتح.".into(),
                ),
                None,
                None,
                None,
            );
        }
    }
    if !saw_entrance {
        let ground = fl.floor.name.to_lowercase();
        if ground.contains("ground") || ground.contains("أرضي") || ground.starts_with("gf") {
            cx.skip(
                "entrance_door_narrow",
                format!("{}: no door is marked main_entrance", fl.floor.name),
            );
        }
    }
}

fn guest_wc(cx: &mut Ctx, fl: &Fl) {
    let guest_facing = fl.of_kind(&["majlis", "reception"]);
    if guest_facing.is_empty() {
        return;
    }
    let wcs = fl.of_kind(&["guest_wc"]);
    cx.checked.insert("guest_wc");
    if wcs.is_empty() {
        let names: Vec<String> = guest_facing.iter().map(|r| r.room.name.clone()).collect();
        cx.add(
            "guest_wc",
            "warning",
            &fl.floor.name,
            names,
            (
                format!("{} has a majlis or reception but no guest WC.", fl.floor.name),
                format!("{} فيه مجلس أو استقبال دون دورة مياه للضيوف.", fl.floor.name),
            ),
            (
                "Add a guest WC next to the entrance hall, reached without crossing the family area.".into(),
                "أضف دورة مياه للضيوف بجوار مدخل الصالة، يوصل إليها دون المرور بمنطقة العائلة.".into(),
            ),
            None,
            None,
            None,
        );
        return;
    }
    let Some(max) = cx.threshold("guest_wc_distance_max_m") else {
        return;
    };
    let guest_names: Vec<&str> = fl
        .rooms
        .iter()
        .filter(|r| matches!(r.kind, Some("majlis" | "reception" | "lobby")))
        .map(|r| r.room.name.as_str())
        .collect();
    let mut points: Vec<[f64; 2]> = fl
        .floor
        .doors
        .iter()
        .filter(|d| d.main_entrance)
        .filter_map(|d| d.position_m)
        .collect();
    points.extend(guest_facing.iter().filter_map(|r| r.room.centre_m));
    // The nearest guest WC decides.
    let mut nearest: Option<(f64, &R)> = None;
    let mut unknown = false;
    for wc in wcs.iter().copied() {
        let linked = wc
            .room
            .adjacent
            .iter()
            .chain(&wc.room.opens_to)
            .any(|n| guest_names.iter().any(|g| same(g, n)));
        if linked {
            nearest = Some((0.0, wc));
            break;
        }
        match (wc.room.centre_m, points.is_empty()) {
            (Some(c), false) => {
                let d = points
                    .iter()
                    .map(|p| (p[0] - c[0]).hypot(p[1] - c[1]))
                    .fold(f64::INFINITY, f64::min);
                if nearest.is_none_or(|(best, _)| d < best) {
                    nearest = Some((d, wc));
                }
            }
            _ => unknown = true,
        }
    }
    match nearest {
        Some((d, wc)) if d > max => cx.add(
            "guest_wc_far",
            "warning",
            &fl.floor.name,
            vec![wc.room.name.clone()],
            (
                format!(
                    "{} is {} m from the entrance and the majlis (practice limit {} m).",
                    wc.room.name,
                    e(d, 1),
                    e(max, 0)
                ),
                format!(
                    "{} يبعد {} م عن المدخل والمجلس (الحد المعتاد {} م).",
                    wc.room.name,
                    a(d, 1),
                    a(max, 0)
                ),
            ),
            (
                "Move the guest WC next to the entrance hall or the majlis.".into(),
                "انقل دورة مياه الضيوف بجوار صالة المدخل أو المجلس.".into(),
            ),
            Some(d),
            None,
            Some(max),
        ),
        Some(_) => {}
        None if unknown => cx.skip(
            "guest_wc_far",
            format!(
                "{}: the guest WC has no centre, or the entrance no position",
                fl.floor.name
            ),
        ),
        None => {}
    }
}

// ---- house checks -------------------------------------------------------------------------

fn master_ensuite(cx: &mut Ctx, floors: &[Fl]) {
    let Some(min_bedrooms) = cx.threshold("master_rule_min_bedrooms") else {
        return;
    };
    let bedrooms: Vec<(&Fl, &R)> = floors
        .iter()
        .flat_map(|fl| fl.of_kind(&["bedroom"]).into_iter().map(move |r| (fl, r)))
        .collect();
    if (bedrooms.len() as f64) < min_bedrooms {
        return;
    }
    let master = bedrooms.iter().find(|(_, r)| r.master);
    let Some((fl, master)) = master.copied() else {
        return;
    };
    let is_ensuite = |bath: &R| -> Option<bool> {
        if bath.room.opens_to.is_empty() {
            return None;
        }
        Some(bath.room.opens_to.len() == 1 && same(&bath.room.opens_to[0], &master.room.name))
    };
    let baths = fl.of_kind(&["bathroom", "wc"]);
    let mut verdicts: Vec<Option<bool>> = Vec::new();
    for bath in baths.iter().copied() {
        verdicts.push(is_ensuite(bath));
    }
    if verdicts.contains(&Some(true)) {
        cx.checked.insert("master_without_ensuite");
        return;
    }
    if verdicts.iter().any(Option::is_none) {
        cx.skip(
            "master_without_ensuite",
            "some bathrooms have no door-access data / بعض الحمامات بلا بيانات وصول عبر الأبواب",
        );
        return;
    }
    cx.checked.insert("master_without_ensuite");
    cx.add(
        "master_without_ensuite",
        "warning",
        &fl.floor.name,
        vec![master.room.name.clone()],
        (
            format!("{} has no en-suite bathroom of its own.", master.room.name),
            format!("{} ليس له حمام خاص.", master.room.name),
        ),
        (
            "Add a bathroom that opens only to the master bedroom, with a dressing area if the room is large.".into(),
            "أضف حماما يفتح على غرفة النوم الرئيسية فقط، مع منطقة ملابس إن كانت الغرفة كبيرة.".into(),
        ),
        None,
        None,
        None,
    );
}

fn bathroom_ratio(cx: &mut Ctx, floors: &[Fl]) {
    let Some(ratio) = cx.threshold("baths_per_bedroom_min") else {
        return;
    };
    let count = |kind: &str| -> usize { floors.iter().map(|f| f.of_kind(&[kind]).len()).sum() };
    let bedrooms = count("bedroom");
    let Some(min_bedrooms) = cx.threshold("bathroom_ratio_min_bedrooms") else {
        return;
    };
    if (bedrooms as f64) < min_bedrooms {
        return;
    }
    cx.checked.insert("bathroom_ratio");
    let baths = count("bathroom");
    let needed = (bedrooms as f64 * ratio - 1e-9).ceil() as usize;
    if baths < needed {
        let names: Vec<String> = floors
            .iter()
            .flat_map(|f| {
                f.of_kind(&["bathroom"])
                    .into_iter()
                    .map(|r| r.room.name.clone())
            })
            .collect();
        cx.add(
            "bathroom_ratio",
            "warning",
            &floors[0].floor.name,
            names,
            (
                format!(
                    "{bedrooms} bedrooms share {baths} bathrooms; {needed} or more are expected."
                ),
                format!(
                    "{} غرف نوم تتشارك {} حمامات؛ والمتوقع {} أو أكثر.",
                    a(bedrooms as f64, 0),
                    a(baths as f64, 0),
                    a(needed as f64, 0)
                ),
            ),
            (
                format!(
                    "Add {} bathroom(s), the first one for the master bedroom.",
                    needed - baths
                ),
                format!(
                    "أضف {} حمام، الأول لغرفة النوم الرئيسية.",
                    a((needed - baths) as f64, 0)
                ),
            ),
            Some(baths as f64),
            None,
            Some(needed as f64),
        );
    }
}

fn wardrobes(cx: &mut Ctx, floors: &[Fl]) {
    let words = wardrobe_words();
    let has_word = |text: &str| {
        let t = text.to_lowercase();
        words.iter().any(|w| t.contains(w.as_str()))
    };
    for fl in floors {
        let dressing: Vec<&str> = fl
            .of_kind(&["dressing"])
            .into_iter()
            .map(|r| r.room.name.as_str())
            .collect();
        for r in fl.of_kind(&["bedroom"]) {
            cx.checked.insert("bedroom_without_wardrobe");
            let drawn = r.room.fixtures.iter().any(|f| has_word(f));
            let dressing_next = r
                .room
                .adjacent
                .iter()
                .chain(&r.room.opens_to)
                .any(|n| dressing.iter().any(|d| same(d, n)));
            if !drawn && !dressing_next {
                cx.add(
                    "bedroom_without_wardrobe",
                    "warning",
                    &fl.floor.name,
                    vec![r.room.name.clone()],
                    (
                        format!("{} has no wardrobe drawn.", r.room.name),
                        format!("{} بلا خزانة ملابس مرسومة.", r.room.name),
                    ),
                    (
                        "Draw a wardrobe along a wall (about 0.60 m deep), or add a dressing room."
                            .into(),
                        "ارسم خزانة ملابس على أحد الجدران (عمقها حوالي 0.60 م)، أو أضف غرفة ملابس."
                            .into(),
                    ),
                    None,
                    None,
                    None,
                );
            }
        }
    }
}

// ---- reading the plan from the drawing ---------------------------------------------------------

type Point = [f64; 2];

fn edges(ring: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(ring.len())
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn on_edge(p: Point, a: Point, b: Point, tol: f64) -> bool {
    let ab = sub(b, a);
    let len = ab[0].hypot(ab[1]);
    len > tol
        && cross(ab, sub(p, a)).abs() <= tol * len
        && dot(sub(p, a), ab) >= -tol * len
        && dot(sub(p, b), ab) <= tol * len
}
fn inside(ring: &[Point], p: Point, tol: f64) -> bool {
    if edges(ring).any(|(a, b)| on_edge(p, a, b, tol)) {
        return true;
    }
    let mut hit = false;
    for (a, b) in edges(ring) {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            hit = !hit;
        }
    }
    hit
}
fn shared_segments(a: &[Point], b: &[Point], tol: f64) -> Vec<[Point; 2]> {
    let mut shared = Vec::new();
    for (p, q) in edges(a) {
        let d = sub(q, p);
        let len = d[0].hypot(d[1]);
        if len <= tol {
            continue;
        }
        let u = [d[0] / len, d[1] / len];
        for (r, s) in edges(b) {
            if cross(u, sub(r, p)).abs() > tol || cross(u, sub(s, p)).abs() > tol {
                continue;
            }
            let x = dot(sub(r, p), u);
            let y = dot(sub(s, p), u);
            let lo = x.min(y).max(0.0);
            let hi = x.max(y).min(len);
            if hi - lo > tol {
                shared.push([
                    [p[0] + u[0] * lo, p[1] + u[1] * lo],
                    [p[0] + u[0] * hi, p[1] + u[1] * hi],
                ]);
            }
        }
    }
    shared
}

/// Interior chords sampled between projected vertices in every polygon-edge direction.
/// Unlike an enclosing box these stop at concave boundaries and island edges.
fn clear_widths(rings: &[Vec<Point>], tol: f64) -> Vec<Value> {
    let Some(outer) = rings.first() else {
        return Vec::new();
    };
    let mut directions: Vec<Point> = Vec::new();
    for (a, b) in edges(outer) {
        let d = sub(b, a);
        let len = d[0].hypot(d[1]);
        if len <= tol {
            continue;
        }
        let mut u = [d[0] / len, d[1] / len];
        if u[0] < -tol || (u[0].abs() <= tol && u[1] < 0.0) {
            u = [-u[0], -u[1]];
        }
        if !directions.iter().any(|v| cross(*v, u).abs() <= 1e-9) {
            directions.push(u);
        }
    }
    let mut result = Vec::new();
    for u in directions {
        let n = [-u[1], u[0]];
        let mut offsets: Vec<_> = rings.iter().flatten().map(|p| dot(*p, n)).collect();
        offsets.sort_by(f64::total_cmp);
        offsets.dedup_by(|a, b| (*a - *b).abs() <= tol);
        for interval in offsets.windows(2) {
            let t = (interval[0] + interval[1]) / 2.0;
            let mut hits = Vec::new();
            for ring in rings {
                for (a, b) in edges(ring) {
                    let (ta, tb) = (dot(a, n), dot(b, n));
                    if (ta > t) != (tb > t) {
                        let f = (t - ta) / (tb - ta);
                        hits.push(dot([a[0] + f * (b[0] - a[0]), a[1] + f * (b[1] - a[1])], u));
                    }
                }
            }
            hits.sort_by(f64::total_cmp);
            for pair in hits.as_chunks::<2>().0 {
                if pair[1] - pair[0] > tol {
                    result.push(json!({"width_m":pair[1]-pair[0],"direction":u,
                        "from_m":[u[0]*pair[0]+n[0]*t,u[1]*pair[0]+n[1]*t],
                        "to_m":[u[0]*pair[1]+n[0]*t,u[1]*pair[1]+n[1]*t]}));
                }
            }
        }
    }
    result
}

fn point_along(points: &[[f64; 2]], closed: bool, at: f64) -> Option<[f64; 2]> {
    let mut left = at;
    for (a, b) in points.iter().zip(
        points
            .iter()
            .skip(1)
            .chain(points.first().filter(|_| closed)),
    ) {
        let w = [a, b];
        let len = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if len > 0.0 && left <= len {
            let t = left / len;
            return Some([
                w[0][0] + t * (w[1][0] - w[0][0]),
                w[0][1] + t * (w[1][1] - w[0][1]),
            ]);
        }
        left -= len;
    }
    points.last().copied()
}

impl Session {
    /// Read real room polygons, including holes. Curves use the drawing's geometric tolerance.
    fn plan_floor_from_drawing(
        &self,
        query: &FloorQuery,
    ) -> Result<(PlanFloor, Vec<Value>), CoreError> {
        let k = self.drawing.units.to_metres();
        let tol = self.measure_tol() * k;
        let report = self.rooms(&RoomsQuery {
            boundary_layers: query.boundary_layers.clone(),
            text_layer: query.text_layer.clone(),
            within: query.within,
            ..Default::default()
        })?;
        let mut rooms = Vec::new();
        let mut polygons = Vec::new();
        let mut facts = Vec::new();
        let owner = self.sheet_owner();
        let items: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter(|e| owner.is_none_or(|o| e.common().owner_handle == o))
            .filter_map(|e| match e {
                EntityType::Insert(i) => Some((
                    e.common().handle.value(),
                    i.block_name.clone(),
                    [i.insert_point.x * k, i.insert_point.y * k],
                )),
                EntityType::Point(p) => Some((
                    e.common().handle.value(),
                    String::new(),
                    [p.location.x * k, p.location.y * k],
                )),
                _ => None,
            })
            .collect();
        for (index, row) in report.rooms.iter().enumerate() {
            let name = row
                .name
                .clone()
                .unwrap_or_else(|| format!("Room / فراغ {}", index + 1));
            let mut rings = Vec::new();
            let mut perimeter = 0.0;
            if let crate::Boundary::Ring {
                ring,
                holes,
                bulges,
            } = self.room_boundary_in(&self.sheet, row.point, &query.boundary_layers)
            {
                for (i, ring) in std::iter::once(ring).chain(holes).enumerate() {
                    let entity = EntityType::LwPolyline(crate::object_regions::region_polyline(
                        &ring,
                        bulges.get(i).map_or(&[], Vec::as_slice),
                    ));
                    perimeter += qiyas_geom::exact_length(&entity, &qiyas_geom::Xform::identity())
                        .unwrap_or(0.0)
                        * k;
                    let paths = qiyas_geom::flatten(
                        &self.drawing.doc,
                        &entity,
                        &qiyas_geom::Xform::identity(),
                        self.measure_tol(),
                    )
                    .into_paths();
                    if let Some(path) = paths.first() {
                        rings.push(
                            path.points
                                .iter()
                                .map(|p| [p[0] * k, p[1] * k])
                                .collect::<Vec<_>>(),
                        );
                    }
                }
            }
            let widths = clear_widths(&rings, tol);
            let fixtures: Vec<_> = items
                .iter()
                .filter(|(_, _, p)| {
                    rings.first().is_some_and(|r| inside(r, *p, tol))
                        && !rings.iter().skip(1).any(|r| inside(r, *p, tol))
                })
                .map(|(h, n, p)| json!({"handle":h,"name":n,"position_m":p}))
                .collect();
            let kind = row
                .boundary_handle
                .and_then(|h| self.drawing.doc.get_entity(opencadcodec::Handle::new(h)))
                .and_then(crate::objects::instance)
                .and_then(|o| {
                    o.props
                        .get("room_kind")
                        .or_else(|| o.props.get("space_kind"))
                        .or_else(|| {
                            o.props
                                .get("kind")
                                .filter(|kind| kind.as_str() != Some("room"))
                        })
                        .and_then(Value::as_str)
                        .map(String::from)
                });
            rooms.push(PlanRoom {
                name: name.clone(),
                kind,
                area_m2: row.net_area_m2,
                width_m: widths
                    .iter()
                    .filter_map(|w| w["width_m"].as_f64())
                    .reduce(f64::min),
                centre_m: Some([row.point[0] * k, row.point[1] * k]),
                fixtures: fixtures
                    .iter()
                    .filter_map(|f| f["name"].as_str().map(String::from))
                    .collect(),
                ..Default::default()
            });
            facts.push(json!({"name":name,"area_m2":row.net_area_m2,"perimeter_m":if rings.is_empty(){None}else{Some(perimeter)},
                "clear_widths_m":widths,"width_method":"interior_chords_at_projected_vertex_interval_midpoints",
                "polygon_tolerance_m":tol,"boundary_m":rings,"fixtures":fixtures,"shared_boundaries":[],"openings":[]}));
            polygons.push(rings);
        }
        for i in 0..rooms.len() {
            let mut shared = Vec::new();
            for j in 0..rooms.len() {
                if i == j {
                    continue;
                }
                let segments: Vec<_> = polygons[i]
                    .iter()
                    .flat_map(|a| {
                        polygons[j]
                            .iter()
                            .flat_map(move |b| shared_segments(a, b, tol))
                    })
                    .collect();
                if !segments.is_empty() {
                    let other_name = rooms[j].name.clone();
                    rooms[i].adjacent.push(other_name);
                    let length: f64 = segments
                        .iter()
                        .map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1]))
                        .sum();
                    shared.push(
                        json!({"space":rooms[j].name,"length_m":length,"segments_m":segments}),
                    );
                }
            }
            facts[i]["shared_boundaries"] = json!(shared);
        }
        let mut doors = Vec::new();
        let mut seen_openings = BTreeSet::new();
        for group in self.wall_groups(None)? {
            for run in &group.runs {
                for opening in &run.openings {
                    seen_openings.insert(opening.insert);
                    let o = &opening.opening;
                    let Some(p) = point_along(&run.points, run.closed, o.at) else {
                        continue;
                    };
                    if query.within.is_some_and(|[lo, hi]| {
                        p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1]
                    }) {
                        continue;
                    }
                    let p = [p[0] * k, p[1] * k];
                    // The physical opening spans the host's measured thickness, not an arbitrary box tolerance.
                    let distance = run.thickness * k / 2.0 + tol;
                    for (i, rings) in polygons.iter().enumerate() {
                        if rings
                            .iter()
                            .any(|r| edges(r).any(|(a, b)| on_edge(p, a, b, distance)))
                        {
                            facts[i]["openings"].as_array_mut().expect("array").push(json!({"handle":opening.insert,"kind":o.kind,"width_m":o.width*k,"position_m":p,"sill_m":o.sill,"head_m":o.head}));
                        }
                    }
                    if matches!(o.kind, OpeningKind::Door) {
                        doors.push(PlanDoor {
                            name: opening.mark.clone(),
                            width_m: o.width * k,
                            position_m: Some(p),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        // Typed openings can come from supplied object definitions as well as wall commands.
        for object in self.objects(&crate::objects::ObjectFilter {
            category: Some("opening".into()),
            sheet: Some(self.sheet.clone()),
            ..Default::default()
        })? {
            let id = object.instance.id;
            if !seen_openings.insert(id) {
                continue;
            }
            let Some(entity) = self.drawing.doc.get_entity(opencadcodec::Handle::new(id)) else {
                continue;
            };
            let p = match entity {
                EntityType::Point(p) => [p.location.x * k, p.location.y * k],
                EntityType::Insert(p) => [p.insert_point.x * k, p.insert_point.y * k],
                _ => continue,
            };
            let props = &object.instance.props;
            let kind = props.get("kind").cloned().unwrap_or(Value::Null);
            let width = props.get("width").and_then(Value::as_f64);
            let distance = props
                .get("host_width")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                / 2.0
                + tol;
            for (i, rings) in polygons.iter().enumerate() {
                if rings
                    .iter()
                    .any(|r| edges(r).any(|(a, b)| on_edge(p, a, b, distance)))
                {
                    facts[i]["openings"]
                        .as_array_mut()
                        .expect("array")
                        .push(json!({
                            "handle":id,"kind":kind,"width_m":width,"position_m":p,
                            "sill_m":props.get("sill"),"head_m":props.get("head")
                        }));
                }
            }
        }
        for fact in &mut facts {
            fact["fixtures"].as_array_mut().expect("array").retain(|f| {
                f["handle"]
                    .as_u64()
                    .is_none_or(|h| !seen_openings.contains(&h))
            });
        }
        Ok((
            PlanFloor {
                name: query.name.clone(),
                rooms,
                doors,
                stairs: Vec::new(),
            },
            facts,
        ))
    }

    /// Read measured floor facts and overlay explicit classifications. Practice checks run only
    /// for a selected profile compatible with the caller's building type. Reads only.
    pub(crate) fn space_check(
        &self,
        plan: Option<PlanModel>,
        floors: &[FloorQuery],
        market: Option<&str>,
        language: Option<LabelLanguage>,
        profile: Option<&str>,
        building_type: Option<&str>,
    ) -> Result<Value, CoreError> {
        let mut model = PlanModel::default();
        let mut geometry = BTreeMap::new();
        for query in floors {
            let (floor, facts) = self.plan_floor_from_drawing(query)?;
            geometry.insert(query.name.clone(), facts);
            model.floors.push(floor);
        }
        if let Some(plan) = plan {
            for extra in plan.floors {
                match model.floors.iter_mut().find(|f| same(&f.name, &extra.name)) {
                    Some(floor) => {
                        for room in extra.rooms {
                            match floor.rooms.iter_mut().find(|r| same(&r.name, &room.name)) {
                                Some(old) => {
                                    if room.kind.is_some() {
                                        old.kind = room.kind;
                                    }
                                    if room.master.is_some() {
                                        old.master = room.master;
                                    }
                                    old.opens_to = room.opens_to;
                                }
                                None => floor.rooms.push(room),
                            }
                        }
                        floor.doors.extend(extra.doors);
                        floor.stairs.extend(extra.stairs);
                    }
                    None => model.floors.push(extra),
                }
            }
        }
        if model.floors.is_empty() {
            return Err(invalid(
                "Give a plan (floors with rooms) or the floors to read from the drawing",
            ));
        }
        let report = run_checks(&model, market, profile, building_type)?;
        let mut value = report.json();
        value["language"] = json!(
            if language.or(self.sheet_language()) == Some(LabelLanguage::Ar) {
                "ar"
            } else {
                "en"
            }
        );
        value["profile_requested"] = json!(profile);
        value["floors"] = json!(model.floors.iter().map(|f| {
            let mut spaces = geometry.get(&f.name).cloned().unwrap_or_else(|| f.rooms.iter().map(|r| json!({
                "name":r.name,"area_m2":r.area_m2,"perimeter_m":null,"clear_widths_m":[],
                "supplied_width_m":r.width_m,"supplied_length_m":r.length_m,
                "adjacent_supplied":r.adjacent,"fixtures_supplied":r.fixtures,
                "shared_boundaries":[],"openings":[],"source":"caller"
            })).collect());
            for room in &f.rooms {
                if !spaces.iter().any(|s|s["name"].as_str()==Some(room.name.as_str())) {
                    spaces.push(json!({"name":room.name,"area_m2":room.area_m2,"perimeter_m":null,
                        "clear_widths_m":[],"shared_boundaries":[],"openings":[],"source":"caller"}));
                }
            }
            for space in &mut spaces {
                if let Some(room) = f.rooms.iter().find(|r| r.name == space["name"].as_str().unwrap_or("")) {
                    space["kind"] = json!(room.kind);
                    if room.kind.is_none() {
                        space["suggested_kind"] = json!(room_kind(&room.name).0);
                        space["uncertain"] = json!(true);
                    }
                }
            }
            let kinds = kind_list("circulation_kinds");
            let total: f64 = f.rooms.iter().map(|r|r.area_m2).sum();
            let known: f64 = f.rooms.iter().filter(|r| r.kind.as_ref().is_some_and(|k| kinds.contains(k))).map(|r|r.area_m2).sum();
            let known_kinds = crate::drafting_standards::suggested_kinds();
            let classified = f.rooms.iter().all(|r|r.kind.as_deref().is_some_and(|kind|known_kinds.contains(&kind)));
            json!({"name":f.name,"spaces":spaces,"doors":f.doors,"stairs":f.stairs,
                "area_m2":total,"classified_circulation_area_m2":known,
                "circulation_share":if classified && total>0.0 {Some(known/total)} else {None},
                "circulation_classification_complete":classified,"circulation_kinds":kinds,
                "circulation_note":if classified {None} else {Some("Circulation share is unknown until explicit space classifications establish their circulation roles.")},
                "circulation_note_ar":if classified {None} else {Some("نسبة الحركة غير معروفة حتى تحدد تصنيفات الفراغات الصريحة أدوارها في الحركة.")}})
        }).collect::<Vec<_>>());
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polygon_facts_exclude_notches_and_corner_contacts() {
        let l = vec![
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 1.0],
            [1.0, 1.0],
            [1.0, 4.0],
            [0.0, 4.0],
        ];
        let notch = vec![[2.0, 2.0], [3.0, 2.0], [3.0, 3.0], [2.0, 3.0]];
        let adjoining = vec![[-2.0, 0.0], [0.0, 0.0], [0.0, 4.0], [-2.0, 4.0]];
        let corner = vec![[4.0, 1.0], [5.0, 1.0], [5.0, 2.0], [4.0, 2.0]];
        assert!(!inside(&l, [2.5, 2.5], 1e-9));
        assert!(inside(&l, [0.5, 2.5], 1e-9));
        assert!(shared_segments(&l, &notch, 1e-9).is_empty());
        assert!(shared_segments(&l, &corner, 1e-9).is_empty());
        let shared = shared_segments(&l, &adjoining, 1e-9);
        assert_eq!(shared.len(), 1);
        assert!(
            ((shared[0][1][0] - shared[0][0][0]).hypot(shared[0][1][1] - shared[0][0][1]) - 4.0)
                .abs()
                < 1e-9
        );
        let widths = clear_widths(&[l], 1e-9);
        assert!(widths.iter().any(|w| w["width_m"] == 1.0));
    }

    #[test]
    fn clear_chords_stop_at_islands_and_follow_rotated_edges() {
        let outer = vec![[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]];
        let hole = vec![[2.0, 1.0], [4.0, 1.0], [4.0, 3.0], [2.0, 3.0]];
        let widths = clear_widths(&[outer, hole], 1e-9);
        assert!(widths.iter().any(|w| w["width_m"] == 2.0));
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let rotated: Vec<_> = [[0.0, 0.0], [4.0, 0.0], [4.0, 1.0], [0.0, 1.0]]
            .into_iter()
            .map(|[x, y]| [(x - y) * k, (x + y) * k])
            .collect();
        let widths = clear_widths(&[rotated], 1e-9);
        assert!(widths
            .iter()
            .any(|w| (w["width_m"].as_f64().unwrap() - 1.0).abs() < 1e-8));
    }

    #[test]
    fn code_findings_keep_the_citation_url_and_caveat() {
        // A synthetic code limit tests the citation path without promoting unverified data.
        let limit = ResolvedLimit {
            limit: serde_json::from_value(json!({
                "id":"test_width", "market":"egypt", "value":1.0, "unit":"m",
                "basis":"code", "code":"Test code", "clause":"Test clause",
                "source_url":"https://example.com/test-code", "evidence_read":true,
                "note":"Test caveat: applies only to the stated room type."
            }))
            .unwrap(),
            asked: Some("egypt".into()),
            fallback: false,
        };
        let mut cx = Ctx {
            profile: None,
            findings: Vec::new(),
            checked: BTreeSet::new(),
            skipped: Vec::new(),
            used: BTreeMap::new(),
            thresholds: BTreeMap::new(),
        };
        cx.add(
            "test_width",
            "error",
            "Ground floor",
            vec!["Test room".into()],
            ("Too narrow.".into(), "العرض لا يكفي.".into()),
            ("Widen the room.".into(), "زد عرض الغرفة.".into()),
            Some(0.8),
            Some(&limit),
            None,
        );
        let finding = serde_json::to_value(&cx.findings[0]).unwrap();
        assert_eq!(finding["basis"], "code");
        assert_eq!(finding["severity"], "error");
        assert_eq!(finding["source"], "Test code, Test clause");
        assert_eq!(finding["source_url"], "https://example.com/test-code");
        assert_eq!(finding["caveat"], limit.limit.note.unwrap());
        assert_eq!(finding["message"], "Too narrow.");
    }
}
