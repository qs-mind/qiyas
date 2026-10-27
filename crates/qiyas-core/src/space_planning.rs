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
    kind_list, limit_for, room_kind, threshold, wardrobe_words, ResolvedLimit, KINDS,
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

/// A room. `kind` is read from the name (English or Arabic) when left out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanRoom {
    pub name: String,
    /// One of: stair, guest_wc, bathroom, wc, kitchen, dining, majlis, reception, living,
    /// dressing, bedroom, corridor, lobby, utility, garage, balcony.
    #[serde(default)]
    pub kind: Option<String>,
    /// True for the master bedroom; the name "master" also says it.
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
    kind: Option<&'static str>,
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

    fn kind_of(&self, name: &str) -> Option<&'static str> {
        self.find(name).and_then(|r| r.kind)
    }
}

struct Ctx<'m> {
    market: Option<&'m str>,
    findings: Vec<SpaceFinding>,
    checked: BTreeSet<&'static str>,
    skipped: Vec<Value>,
    used: BTreeMap<String, Value>,
}

impl Ctx<'_> {
    fn limit(&mut self, id: &str) -> Option<ResolvedLimit> {
        let l = limit_for(id, self.market)?;
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
            if let Some(kind) = &room.kind {
                if !KINDS.contains(&kind.as_str()) {
                    return Err(invalid(format!(
                        "Room {}: kind {kind:?} is none of {}",
                        room.name,
                        KINDS.join(", ")
                    )));
                }
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
        .map(|room| {
            let (inferred, named_master) = room_kind(&room.name);
            let kind = room
                .kind
                .as_deref()
                .and_then(|k| KINDS.iter().copied().find(|known| *known == k))
                .or(inferred);
            R {
                room,
                kind,
                master: room.master.unwrap_or(named_master),
            }
        })
        .collect();
    Fl { floor, rooms }
}

/// Run every check on `plan` for `market` (`egypt`, `ksa`, `uae` or none: practice defaults).
///
/// # Errors
/// The plan is not well formed (no floors, a negative area, an unknown kind).
pub fn run_checks(plan: &PlanModel, market: Option<&str>) -> Result<SpaceReport, CoreError> {
    check_model(plan)?;
    let market = market.and_then(crate::drafting_standards::normalise_market);
    let mut cx = Ctx {
        market,
        findings: Vec::new(),
        checked: BTreeSet::new(),
        skipped: Vec::new(),
        used: BTreeMap::new(),
    };
    let floors: Vec<Fl> = plan.floors.iter().map(prepare).collect();
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
    let (errors, warnings, notes) =
        cx.findings
            .iter()
            .fold((0, 0, 0), |(e, w, n), f| match f.severity {
                "error" => (e + 1, w, n),
                "warning" => (e, w + 1, n),
                _ => (e, w, n + 1),
            });
    let summary = if cx.findings.is_empty() {
        format!("{} checks ran; no finding.", cx.checked.len())
    } else {
        format!(
            "{errors} errors, {warnings} warnings, {notes} notes from {} checks.",
            cx.checked.len()
        )
    };
    let summary_ar = crate::numbers::arabic_numerals(&if cx.findings.is_empty() {
        format!("تم تنفيذ {} فحصاً دون ملاحظات.", cx.checked.len())
    } else {
        format!(
            "{errors} أخطاء و{warnings} تحذيرات و{notes} ملاحظات من {} فحصاً.",
            cx.checked.len()
        )
    });
    Ok(SpaceReport {
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
        responsibility:
            "Qiyas draws and checks against stated limits. The engineer and the company approve.",
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
    if fl.rooms.len() < 3 || total <= 0.0 {
        cx.skip(
            "circulation_share",
            format!("{}: fewer than three rooms", fl.floor.name),
        );
        return;
    }
    cx.checked.insert("circulation_share");
    let kinds = kind_list("circulation_kinds");
    let kinds: Vec<&str> = kinds.iter().map(String::as_str).collect();
    let circulation: Vec<&R> = fl.of_kind(&kinds);
    let area: f64 = circulation.iter().map(|r| r.room.area_m2).sum();
    let share = area / total;
    let warn = threshold("circulation_share_warn").unwrap_or(0.15);
    let error = threshold("circulation_share_error").unwrap_or(0.22);
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
                "قلّل حوالي {} م² من الممرات: افتح الغرف على صالة واحدة مضغوطة، واحذف الممر الطويل، وأعطِ السلم حيزاً محاطاً بحوائط.",
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
    let max_len = threshold("corridor_length_max_m").unwrap_or(8.0);
    let min_width = cx.limit("corridor_width_min_m");
    for r in corridors {
        let width = r.room.width_m;
        let length = r
            .room
            .length_m
            .or_else(|| width.filter(|w| *w > 0.0).map(|w| r.room.area_m2 / w));
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
                            "جمّع الغرف حول صالة مركزية بحيث لا يزيد أي ممر عن أمتار قليلة.".into(),
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
                            format!("وسّعه إلى {} م أو أكثر.", a(limit.value(), 2)),
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
                        "كبّرها إلى {} م² على الأقل، أو ادمجها مع المجاور.",
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
        let ratio = threshold("kitchen_to_living_max_ratio").unwrap_or(1.0);
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
                    format!("قلّص المطبخ إلى أقل من {} م² وأعطِ الفرق لمساحة المعيشة.", a(biggest.room.area_m2 * ratio, 0)),
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
                    format!("{ar_first} و{ar_second} غير متجاورتين ولا تفتحان على بعضهما."),
                ),
                (
                    format!("Put the {en_first} and the {en_second} side by side with a door or an opening between them."),
                    format!("ضع {ar_first} و{ar_second} متجاورتين مع باب أو فتحة بينهما."),
                ),
                None,
                None,
                None,
            );
        }
    }
}

fn oversized(cx: &mut Ctx, fl: &Fl) {
    let bedroom = threshold("bedroom_oversize_m2").unwrap_or(24.0);
    let master = threshold("master_oversize_m2").unwrap_or(36.0);
    let service = threshold("service_oversize_m2").unwrap_or(12.0);
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
                    format!("{} ({} م²) كبيرة بالنسبة لـ{label_ar} (الحد المعتاد {} م²).", r.room.name, a(r.room.area_m2, 1), a(limit, 0)),
                ),
                (
                    "Give the extra area to the rooms that are short of it, or add a bathroom or dressing room.".into(),
                    "أعطِ المساحة الزائدة للغرف الناقصة، أو أضف حماماً أو غرفة ملابس.".into(),
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
                                "انقل السلم إلى حيز محاط بحوائط على الصالة، أو وسّع الممر بجانبه.".into(),
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
                        format!("{label} غير محاط بحوائط."),
                    ),
                    (
                        "Enclose the stair in walls on its long sides and give it a door or opening at the foot, so it does not cut the hall or corridor.".into(),
                        "أحِط السلم بحوائط على جانبيه الطويلين وأعطه باب أو فتحة عند أوله، حتى لا يقطع الصالة أو الممر.".into(),
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
                        format!("وسّع الدرج إلى {} م أو أكثر.", a(limit.value(), 2)),
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
                        "أضف قائمة للسلم لتصبح كل قائمة أقل ارتفاعاً.".into(),
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
                        "أطِل الدرج أو حوّل جزءاً منه إلى بسطة.".into(),
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
                    let leaves = d.leaves.unwrap_or(1);
                    cx.add(
                        "entrance_door_narrow",
                        "warning",
                        &fl.floor.name,
                        rooms.clone(),
                        (
                            format!(
                                "The main entrance is {} m ({} leaf); a villa entrance is at least {} m.",
                                e(d.width_m, 2),
                                if leaves >= 2 { "double" } else { "single" },
                                e(limit.value(), 2)
                            ),
                            format!(
                                "باب المدخل الرئيسي {} م ({}); ومدخل الفيلا لا يقل عن {} م.",
                                a(d.width_m, 2),
                                if leaves >= 2 { "ضلفتان" } else { "ضلفة واحدة" },
                                a(limit.value(), 2)
                            ),
                        ),
                        (
                            "Make the entrance a double-leaf door about 1.50 m wide, or a single leaf of 1.20 m.".into(),
                            "اجعل المدخل بابًا بضلفتين عرضه حوالي 1.50 م، أو ضلفة واحدة 1.20 م.".into(),
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
                            format!("وسّع الباب إلى {} م أو أكثر.", a(limit.value(), 2)),
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
                    "اعكس اتجاه فتح الباب، أو اجعله منزلقاً، أو انقله بعيداً عن مجال الفتح.".into(),
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
                "أضف دورة مياه للضيوف بجوار مدخل الصالة، يُوصل إليها دون المرور بمنطقة العائلة.".into(),
            ),
            None,
            None,
            None,
        );
        return;
    }
    let max = threshold("guest_wc_distance_max_m").unwrap_or(10.0);
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
    let min_bedrooms = threshold("master_rule_min_bedrooms").unwrap_or(3.0);
    let bedrooms: Vec<(&Fl, &R)> = floors
        .iter()
        .flat_map(|fl| fl.of_kind(&["bedroom"]).into_iter().map(move |r| (fl, r)))
        .collect();
    if (bedrooms.len() as f64) < min_bedrooms {
        return;
    }
    // The master is the one named so, else the largest bedroom.
    let master = bedrooms.iter().find(|(_, r)| r.master).or_else(|| {
        bedrooms
            .iter()
            .max_by(|x, y| x.1.room.area_m2.total_cmp(&y.1.room.area_m2))
    });
    let Some((fl, master)) = master.copied() else {
        return;
    };
    let circulation = kind_list("circulation_kinds");
    let is_ensuite = |bath: &R| -> Option<bool> {
        if !bath.room.opens_to.is_empty() {
            return Some(
                bath.room.opens_to.len() == 1 && same(&bath.room.opens_to[0], &master.room.name),
            );
        }
        if bath.room.adjacent.is_empty() {
            return None;
        }
        let touches_master = bath
            .room
            .adjacent
            .iter()
            .any(|n| same(n, &master.room.name));
        let public = bath.room.adjacent.iter().any(|n| {
            fl.kind_of(n).is_some_and(|k| {
                circulation.iter().any(|c| c == k)
                    || matches!(k, "kitchen" | "living" | "majlis" | "reception" | "dining")
            })
        });
        Some(touches_master && !public)
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
            "some bathrooms have no door or adjacency data",
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
            "أضف حماماً يفتح على غرفة النوم الرئيسية فقط، مع منطقة ملابس إن كانت الغرفة كبيرة.".into(),
        ),
        None,
        None,
        None,
    );
}

fn bathroom_ratio(cx: &mut Ctx, floors: &[Fl]) {
    let ratio = threshold("baths_per_bedroom_min").unwrap_or(0.75);
    let count = |kind: &str| -> usize { floors.iter().map(|f| f.of_kind(&[kind]).len()).sum() };
    let bedrooms = count("bedroom");
    if bedrooms < 2 {
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
                        "ارسم خزانة ملابس على أحد الحوائط (عمقها حوالي 0.60 م)، أو أضف غرفة ملابس."
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

/// Bounding box `[[min_x, min_y], [max_x, max_y]]` in metres.
type BoxM = [[f64; 2]; 2];

fn gap_x(a: &BoxM, b: &BoxM) -> f64 {
    (a[0][0] - b[1][0]).max(b[0][0] - a[1][0])
}

fn gap_y(a: &BoxM, b: &BoxM) -> f64 {
    (a[0][1] - b[1][1]).max(b[0][1] - a[1][1])
}

/// Two rooms touch when their boxes meet along one axis (within a wall) and overlap along the
/// other for at least `OVERLAP` metres. Boxes are an approximation for rooms that are not rectangular.
fn touching(a: &BoxM, b: &BoxM) -> bool {
    const WALL: f64 = 0.4;
    const OVERLAP: f64 = 0.6;
    let (gx, gy) = (gap_x(a, b), gap_y(a, b));
    (gx.abs() <= WALL && gy <= -OVERLAP) || (gy.abs() <= WALL && gx <= -OVERLAP)
}

fn inside_box(b: &BoxM, p: [f64; 2]) -> bool {
    p[0] >= b[0][0] && p[0] <= b[1][0] && p[1] >= b[0][1] && p[1] <= b[1][1]
}

fn point_along(points: &[[f64; 2]], at: f64) -> Option<[f64; 2]> {
    let mut left = at;
    for w in points.windows(2) {
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
    /// The floor `query` names, read from the drawing: the rooms the sheet on screen has (name,
    /// net area, size from the room's boundary), what touches what, the wardrobes and fixtures
    /// named inside each room, and the walls' doors. Sizes come from each room's bounding box, so
    /// a room that is not rectangular reports its box. Stairs and main entrances cannot be known
    /// from the drawing alone: pass them in the plan.
    pub(crate) fn plan_floor_from_drawing(
        &self,
        query: &FloorQuery,
    ) -> Result<PlanFloor, CoreError> {
        let k = self.drawing.units.to_metres();
        let report = self.rooms(&RoomsQuery {
            text_layer: query.text_layer.clone(),
            within: query.within,
            ..RoomsQuery::default()
        })?;
        // Every text and block name on the sheet, with its place in metres.
        let owner = self.sheet_owner();
        let mut items: Vec<(String, [f64; 2])> = Vec::new();
        for entity in self.drawing.doc.entities() {
            if owner.is_some_and(|o| entity.common().owner_handle != o) {
                continue;
            }
            match entity {
                EntityType::Text(t) => items.push((
                    t.value.clone(),
                    [t.insertion_point.x * k, t.insertion_point.y * k],
                )),
                EntityType::MText(t) => items.push((
                    crate::query::plain_text(&t.value),
                    [t.insertion_point.x * k, t.insertion_point.y * k],
                )),
                EntityType::Insert(i) => items.push((
                    i.block_name.clone(),
                    [i.insert_point.x * k, i.insert_point.y * k],
                )),
                _ => {}
            }
        }
        let mut rooms: Vec<(PlanRoom, BoxM)> = Vec::new();
        for (index, row) in report.rooms.iter().enumerate() {
            let name = row
                .name
                .clone()
                .unwrap_or_else(|| format!("Room {}", index + 1));
            let ring: Vec<[f64; 2]> = row
                .boundary_handle
                .and_then(|h| self.drawing.doc.get_entity(opencadcodec::Handle::new(h)))
                .and_then(|e| match e {
                    EntityType::LwPolyline(p) => Some(
                        p.vertices
                            .iter()
                            .map(|v| [v.location.x * k, v.location.y * k])
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            let point = [row.point[0] * k, row.point[1] * k];
            let bounds: BoxM = if ring.is_empty() {
                [point, point]
            } else {
                let xs = ring.iter().map(|p| p[0]);
                let ys = ring.iter().map(|p| p[1]);
                [
                    [
                        xs.clone().fold(f64::INFINITY, f64::min),
                        ys.clone().fold(f64::INFINITY, f64::min),
                    ],
                    [
                        xs.fold(f64::NEG_INFINITY, f64::max),
                        ys.fold(f64::NEG_INFINITY, f64::max),
                    ],
                ]
            };
            let (w, h) = (bounds[1][0] - bounds[0][0], bounds[1][1] - bounds[0][1]);
            rooms.push((
                PlanRoom {
                    name,
                    area_m2: row.net_area_m2,
                    width_m: (!ring.is_empty()).then_some(w.min(h)),
                    length_m: (!ring.is_empty()).then_some(w.max(h)),
                    centre_m: Some([
                        (bounds[0][0] + bounds[1][0]) / 2.0,
                        (bounds[0][1] + bounds[1][1]) / 2.0,
                    ]),
                    fixtures: items
                        .iter()
                        .filter(|(_, p)| !ring.is_empty() && inside_box(&bounds, *p))
                        .map(|(t, _)| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect(),
                    ..PlanRoom::default()
                },
                bounds,
            ));
        }
        let touches: Vec<Vec<String>> = (0..rooms.len())
            .map(|i| {
                (0..rooms.len())
                    .filter(|&j| j != i && touching(&rooms[i].1, &rooms[j].1))
                    .map(|j| rooms[j].0.name.clone())
                    .collect()
            })
            .collect();
        let mut rooms: Vec<PlanRoom> = rooms
            .into_iter()
            .zip(touches)
            .map(|((mut room, _), near)| {
                room.adjacent = near;
                room
            })
            .collect();
        // Doors of the wall systems that stand on this floor: their widths.
        let mut doors = Vec::new();
        for group in self.wall_groups(None)? {
            for run in &group.runs {
                for o in &run.openings {
                    if !matches!(o.opening.kind, OpeningKind::Door) {
                        continue;
                    }
                    let Some(p) = point_along(&run.points, o.opening.at) else {
                        continue;
                    };
                    if let Some([lo, hi]) = query.within {
                        if p[0] < lo[0] || p[0] > hi[0] || p[1] < lo[1] || p[1] > hi[1] {
                            continue;
                        }
                    }
                    doors.push(PlanDoor {
                        name: o.mark.clone(),
                        width_m: o.opening.width * k,
                        position_m: Some([p[0] * k, p[1] * k]),
                        ..PlanDoor::default()
                    });
                }
            }
        }
        rooms.sort_by(|x, y| x.name.cmp(&y.name));
        Ok(PlanFloor {
            name: query.name.clone(),
            rooms,
            doors,
            stairs: Vec::new(),
        })
    }

    /// `Command::SpaceCheck`: the plan model (floors read from the drawing, then the explicit
    /// plan laid over them: its rooms replace the drawing's of the same name, its doors and
    /// stairs are added) checked against the limits of `market`. Reads only.
    pub(crate) fn space_check(
        &self,
        plan: Option<PlanModel>,
        floors: &[FloorQuery],
        market: Option<&str>,
        language: Option<LabelLanguage>,
    ) -> Result<Value, CoreError> {
        let mut model = PlanModel::default();
        for query in floors {
            model.floors.push(self.plan_floor_from_drawing(query)?);
        }
        if let Some(plan) = plan {
            for extra in plan.floors {
                match model.floors.iter_mut().find(|f| same(&f.name, &extra.name)) {
                    Some(floor) => {
                        for room in extra.rooms {
                            match floor.rooms.iter_mut().find(|r| same(&r.name, &room.name)) {
                                Some(old) => *old = room,
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
        let report = run_checks(&model, market)?;
        let mut value = report.json();
        value["language"] = json!(
            if language.or(self.sheet_language()) == Some(LabelLanguage::Ar) {
                "ar"
            } else {
                "en"
            }
        );
        value["floors"] = json!(model.floors.iter().map(|f| json!({
            "name": f.name, "rooms": f.rooms.len(), "doors": f.doors.len(), "stairs": f.stairs.len()
        })).collect::<Vec<_>>());
        Ok(value)
    }
}
