//! The six rules that turn the BOQ, the take-off, the schedules and the block counts into
//! conflicts.

use std::hash::Hasher;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{
    BlockCount, Boq, BoqItem, Conflict, ConflictKind, ConflictStatus, Kind, Measurement, Schedule,
    Takeoff,
};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::matching::{
    floor_tokens, group_key, is_schedule_sheet, leading_number, names_mark, overlaps, tokens,
    unit_key, words, STANDARD_UNITS,
};

/// What a comparison looks at. Sheets are named as the user sees them (layout names).
pub struct CompareInputs<'a> {
    pub boq: Option<&'a Boq>,
    pub takeoff: &'a Takeoff,
    /// The schedules read off every sheet.
    pub schedules: &'a [Schedule],
    /// The block counts of each sheet, by sheet name.
    pub blocks: &'a [(String, Vec<BlockCount>)],
    /// The drawing's units, as `DrawingInfo::units` says them ("unknown" adds a warning).
    pub units_label: &'a str,
}

/// Schedule header words for the count column; "no" only when none of these is there.
const QTY_WORDS: [&str; 6] = ["qty", "quantity", "count", "nos", "كميه", "عدد"];
/// Schedule header words for the mark column.
const MARK_WORDS: [&str; 7] = ["mark", "type", "ref", "code", "رمز", "نوع", "بند"];
/// Longest description quoted in a reason, in characters.
const QUOTE: usize = 60;

/// The conflicts between the inputs. `default_tol` is the allowed difference in percent for BOQ
/// items with no tolerance of their own. A conflict found before (same id in `previous`) keeps
/// its status, note and creation time.
pub fn compare(inputs: &CompareInputs, default_tol: f64, previous: &[Conflict]) -> Vec<Conflict> {
    let mut found = Vec::new();
    let quantities: Vec<&Measurement> = inputs
        .takeoff
        .items
        .iter()
        .filter(|m| m.available() && is_quantity(m))
        .collect();
    if let Some(boq) = inputs.boq {
        boq_items(
            boq,
            &quantities,
            default_tol,
            inputs.units_label,
            &mut found,
        );
        missing_from_boq(boq, &quantities, &mut found);
        let mut codes = BTreeMap::<String, Vec<&BoqItem>>::new();
        for item in &boq.items {
            if !item.code.trim().is_empty() {
                codes
                    .entry(item.code.trim().to_string())
                    .or_default()
                    .push(item);
            }
        }
        for (code, items) in codes.into_iter().filter(|(_, items)| items.len() > 1) {
            let mut f = Found::new(
                ConflictKind::DuplicateItem,
                format!("BOQ code {code} occurs on {} rows", items.len()),
                format!("رمز البند {code} مكرر في {} صفوف", items.len()),
            );
            f.boq_code = Some(code);
            f.subject = items
                .iter()
                .map(|i| i.row.to_string())
                .collect::<Vec<_>>()
                .join(",");
            found.push(f);
        }
    }
    for schedule in inputs.schedules {
        schedule_vs_plan(schedule, inputs.blocks, &mut found);
    }
    drawing_vs_drawing(inputs.blocks, &mut found);

    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("UTC now formats as RFC 3339");
    let before: HashMap<&str, &Conflict> = previous.iter().map(|c| (c.id.as_str(), c)).collect();
    let mut seen = HashSet::new();
    found
        .into_iter()
        .map(|f| f.into_conflict(&now))
        // Items sharing a code share their links, so their findings can be the same.
        .filter(|c| seen.insert(c.id.clone()))
        .map(|mut c| {
            if let Some(old) = before.get(c.id.as_str()) {
                c.status = old.status;
                c.note.clone_from(&old.note);
                c.created.clone_from(&old.created);
            }
            c
        })
        .collect()
}

/// A take-off line that is a quantity: not a markup, not an item of 0.
fn is_quantity(m: &Measurement) -> bool {
    m.unit != "markup" && !(m.kind == Kind::Item && m.value == 0.0)
}

/// A finding before it has an id.
struct Found {
    kind: ConflictKind,
    boq_code: Option<String>,
    measurements: Vec<String>,
    schedule: Option<String>,
    sheets: Vec<String>,
    /// What else tells this finding apart from others of its kind: a BOQ row, a block name, a
    /// schedule mark, a description group.
    subject: String,
    expected: Option<f64>,
    found: Option<f64>,
    unit: String,
    reason: String,
    reason_ar: String,
}

impl Found {
    fn new(kind: ConflictKind, reason: String, reason_ar: String) -> Found {
        Found {
            kind,
            boq_code: None,
            measurements: Vec::new(),
            schedule: None,
            sheets: Vec::new(),
            subject: String::new(),
            expected: None,
            found: None,
            unit: String::new(),
            reason,
            reason_ar,
        }
    }

    /// Takes the ids and sheets of `ms`.
    fn of(mut self, ms: &[&Measurement]) -> Found {
        self.measurements = ms.iter().map(|m| m.id.clone()).collect();
        self.sheets = ms.iter().map(|m| m.sheet.clone()).collect();
        self
    }

    fn into_conflict(self, now: &str) -> Conflict {
        let sorted = |v: Vec<String>| -> Vec<String> {
            v.into_iter().collect::<BTreeSet<_>>().into_iter().collect()
        };
        let measurements = sorted(self.measurements);
        let sheets = sorted(self.sheets);
        let hash_key = [
            self.kind.name(),
            self.boq_code.as_deref().unwrap_or(""),
            &measurements.join(","),
            self.schedule.as_deref().unwrap_or(""),
            &sheets.join(","),
            &self.subject,
        ]
        .join("|");
        Conflict {
            id: {
                let mut hash = fnv::FnvHasher::default();
                hash.write(hash_key.as_bytes());
                format!("{:016x}", hash.finish())
            },
            kind: self.kind,
            boq_code: self.boq_code,
            measurements,
            schedule: self.schedule,
            sheets,
            expected: self.expected,
            found: self.found,
            unit: self.unit,
            reason: self.reason,
            reason_ar: self.reason_ar,
            status: ConflictStatus::Open,
            note: String::new(),
            created: now.to_string(),
            hash_key,
            query: None,
        }
    }
}

/// Quantity mismatches, unit doubts and work not in the drawings, item by item. Only unlinked
/// measurements can show that an item without links is in the drawings: a linked one is evidence
/// for its own item only. Findings carry the item's row, so items sharing a code stay apart.
fn boq_items(
    boq: &Boq,
    quantities: &[&Measurement],
    default_tol: f64,
    units_label: &str,
    out: &mut Vec<Found>,
) {
    let described: Vec<BTreeSet<String>> = quantities
        .iter()
        .filter(|m| !is_linked(m))
        .map(|m| tokens(&m.description))
        .collect();
    let mut push = |f: Found, item: &BoqItem| {
        out.push(Found {
            subject: format!("row {}", item.row),
            ..f
        })
    };
    for item in &boq.items {
        let code = item.code.trim();
        let links: Vec<&Measurement> = quantities
            .iter()
            .filter(|m| !code.is_empty() && m.boq_code.as_deref().map(str::trim) == Some(code))
            .copied()
            .collect();
        if links.is_empty() {
            let words = tokens(&item.description);
            if item.quantity != 0.0 && !described.iter().any(|d| overlaps(&words, d)) {
                push(not_in_drawings(item), item);
            }
            continue;
        }
        let unit = unit_key(&item.unit);
        let (same, other): (Vec<&Measurement>, Vec<&Measurement>) =
            links.iter().partition(|m| unit_key(&m.unit) == unit);
        if !other.is_empty() {
            push(other_unit(item, &other), item);
            // The sum would mix units: no quantity check.
            continue;
        }
        // A unit of the workbook's own is a doubt unless every link is in exactly that unit.
        if !STANDARD_UNITS.contains(&unit.as_str()) && same.iter().any(|m| m.unit != item.unit) {
            push(raw_unit(item, &same), item);
        }
        let sum: f64 = same.iter().map(|m| m.shown).sum();
        let tol = item.tolerance.unwrap_or(default_tol);
        if let Some(f) = mismatch(item, sum, tol, units_label) {
            push(f.of(&same), item);
        }
    }
}

/// Linked to a BOQ code.
fn is_linked(m: &Measurement) -> bool {
    m.boq_code.as_deref().is_some_and(|c| !c.trim().is_empty())
}

/// The quantity mismatch of `item` against the linked `sum`, or `None` within `tol` percent.
fn mismatch(item: &BoqItem, sum: f64, tol: f64, units_label: &str) -> Option<Found> {
    let q = item.quantity;
    let percent = (q != 0.0).then(|| (sum - q) / q.abs() * 100.0);
    let within = match percent {
        Some(p) => p.abs() <= tol + 1e-9,
        None => sum.abs() < 1e-9,
    };
    if within {
        return None;
    }
    let u = &item.unit;
    let pct = percent.map_or(String::new(), |p| format!(" ({})", signed(p)));
    let (mut reason, mut reason_ar) = (
        format!(
            "BOQ says {} {}, drawings measure {} {}{pct}",
            qty(q, u),
            unit_en(u),
            qty(sum, u),
            unit_en(u)
        ),
        format!(
            "جدول الكميات يذكر {} {}، والمقاس من الرسومات {} {}{pct}",
            qty(q, u),
            unit_ar(u),
            qty(sum, u),
            unit_ar(u)
        ),
    );
    if units_label == "unknown" {
        reason.push_str("; the drawing's units are unknown and were read as millimetres");
        reason_ar.push_str("؛ وحدات الرسم غير معروفة فقُرئت بالمليمتر");
    }
    let mut f = Found::new(ConflictKind::QuantityMismatch, reason, reason_ar);
    f.boq_code = Some(item.code.trim().to_string());
    f.expected = Some(q);
    f.found = Some(sum);
    f.unit.clone_from(u);
    Some(f)
}

/// Linked measurements in another unit than the item's.
fn other_unit(item: &BoqItem, other: &[&Measurement]) -> Found {
    let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
    for m in other {
        *sums.entry(m.unit.as_str()).or_default() += m.shown;
    }
    let list = |ar: bool| -> String {
        sums.iter()
            .map(|(u, s)| {
                format!(
                    "{} {}",
                    qty(*s, u),
                    if ar { unit_ar(u) } else { unit_en(u) }
                )
            })
            .collect::<Vec<_>>()
            .join(" + ")
    };
    let n = other.len();
    let code = item.code.trim();
    let mut f = Found::new(
        ConflictKind::UnitDoubt,
        format!(
            "BOQ item {code} is in {}, but {} in another unit: {}",
            unit_en(&item.unit),
            plural(n, "linked measurement is", "linked measurements are"),
            list(false)
        ),
        format!(
            "البند {code} بوحدة {}، لكن {} بوحدة أخرى: {}",
            unit_ar(&item.unit),
            count_ar(
                n,
                [
                    "قياسًا مرتبطًا",
                    "قياسين مرتبطين",
                    "قياسات مرتبطة",
                    "قياسًا مرتبطًا"
                ]
            ),
            list(true)
        ),
    )
    .of(other);
    f.boq_code = Some(code.to_string());
    f.expected = Some(item.quantity);
    f.unit.clone_from(&item.unit);
    f
}

/// An item whose unit is the workbook's own text, with links.
fn raw_unit(item: &BoqItem, links: &[&Measurement]) -> Found {
    let n = links.len();
    let code = item.code.trim();
    let mut f = Found::new(
        ConflictKind::UnitDoubt,
        format!(
            "BOQ item {code} is in \"{}\", not a standard unit (m, m², m³, pcs, kg, t); check its {}",
            item.unit,
            plural(n, "linked measurement", "linked measurements")
        ),
        format!(
            "وحدة البند {code} هي «{}» وليست وحدة قياسية (م، م²، م³، قطعة، كجم، طن)؛ راجع القياسات المرتبطة به ({n})",
            item.unit,
        ),
    )
    .of(links);
    f.boq_code = Some(code.to_string());
    f.expected = Some(item.quantity);
    f.found = Some(links.iter().map(|m| m.shown).sum());
    f.unit.clone_from(&item.unit);
    f
}

fn not_in_drawings(item: &BoqItem) -> Found {
    let (code, u) = (item.code.trim(), &item.unit);
    let desc = quote(&item.description);
    let mut f = Found::new(
        ConflictKind::NotInDrawings,
        format!(
            "BOQ item {code} \"{desc}\" ({} {}) has no linked measurement and nothing measured matches it",
            qty(item.quantity, u),
            unit_en(u)
        ),
        format!(
            "البند {code} «{desc}» ({} {}) بلا قياس مرتبط، ولا شيء مقاس يطابقه",
            qty(item.quantity, u),
            unit_ar(u)
        ),
    );
    f.boq_code = (!code.is_empty()).then(|| code.to_string());
    f.expected = Some(item.quantity);
    f.unit.clone_from(u);
    f
}

/// Unlinked measurements that no BOQ description matches, one finding per description group.
/// Lines with a default description ("LwPolyline on A-ROOM") say nothing about the work, so they
/// are left out: link or name them to have them checked.
fn missing_from_boq(boq: &Boq, quantities: &[&Measurement], out: &mut Vec<Found>) {
    let items: Vec<BTreeSet<String>> = boq.items.iter().map(|i| tokens(&i.description)).collect();
    let mut groups: BTreeMap<String, Vec<&Measurement>> = BTreeMap::new();
    for m in quantities {
        if (is_linked(m)
            && boq
                .items
                .iter()
                .any(|i| Some(i.code.trim()) == m.boq_code.as_deref().map(str::trim)))
            || m.auto_description
        {
            continue;
        }
        let words = tokens(&m.description);
        if is_linked(m) || !items.iter().any(|i| overlaps(&words, i)) {
            groups
                .entry(
                    m.boq_code
                        .clone()
                        .unwrap_or_else(|| group_key(&m.description)),
                )
                .or_default()
                .push(m);
        }
    }
    for (key, ms) in groups {
        let mut sums: BTreeMap<&str, f64> = BTreeMap::new();
        for m in &ms {
            *sums.entry(m.unit.as_str()).or_default() += m.shown;
        }
        let total = |ar: bool| -> String {
            sums.iter()
                .map(|(u, s)| {
                    format!(
                        "{} {}",
                        qty(*s, u),
                        if ar { unit_ar(u) } else { unit_en(u) }
                    )
                })
                .collect::<Vec<_>>()
                .join(" + ")
        };
        let desc = match ms[0].description.trim() {
            "" => "(no description)".to_string(),
            d => quote(d),
        };
        let n = ms.len();
        let mut f = Found::new(
            ConflictKind::MissingFromBoq,
            format!(
                "{} \"{desc}\" ({}) {} no BOQ item",
                plural(n, "measurement", "measurements"),
                total(false),
                if n == 1 { "matches" } else { "match" }
            ),
            format!(
                "«{desc}»: {} ({}) بلا بند يقابله في جدول الكميات",
                count_ar(n, ["قياس واحد", "قياسان", "قياسات", "قياسًا"]),
                total(true)
            ),
        )
        .of(&ms);
        if let [(u, s)] = sums.iter().collect::<Vec<_>>()[..] {
            f.unit = u.to_string();
            f.found = Some(*s);
        }
        f.boq_code = ms[0].boq_code.clone();
        f.subject = key;
        out.push(f);
    }
}

/// For a schedule with a count column and a mark column: each row whose mark names blocks on
/// the plans (`names_mark`), when the plans count another number.
fn schedule_vs_plan(
    schedule: &Schedule,
    blocks: &[(String, Vec<BlockCount>)],
    out: &mut Vec<Found>,
) {
    let Some((mark_col, qty_col)) = schedule_columns(&schedule.columns) else {
        return;
    };
    let plans: Vec<&(String, Vec<BlockCount>)> = blocks
        .iter()
        .filter(|(s, _)| !is_schedule_sheet(s))
        .collect();
    for row in &schedule.rows {
        let mark = row.get(mark_col).map_or("", |m| m.trim());
        let Some(listed) = row.get(qty_col).and_then(|c| leading_number(c)) else {
            continue;
        };
        if mark.is_empty() {
            continue;
        }
        let mut per_sheet: Vec<(&str, usize)> = Vec::new();
        // Sheets of one floor show the same rooms: per floor, the most any of its sheets counts.
        // A sheet that names no floor is a floor of its own.
        let mut per_floor: BTreeMap<String, usize> = BTreeMap::new();
        for (i, (sheet, counts)) in plans.iter().enumerate() {
            let n: usize = counts
                .iter()
                .filter(|b| names_mark(b, mark))
                .map(|b| b.count)
                .sum();
            if n > 0 {
                per_sheet.push((sheet, n));
            }
            let floor = floor_tokens(sheet)
                .into_iter()
                .next()
                .unwrap_or_else(|| format!("#{i}"));
            let most = per_floor.entry(floor).or_default();
            *most = (*most).max(n);
        }
        let shown: usize = per_floor.values().sum();
        if per_sheet.is_empty() || (shown as f64 - listed).abs() < 1e-9 {
            continue;
        }
        let where_ = per_sheet
            .iter()
            .map(|(s, n)| format!("{s} {n}"))
            .collect::<Vec<_>>()
            .join(", ");
        let name = quote(&schedule.name);
        let listed_text = qty(listed, "pcs");
        let mut f = Found::new(
            ConflictKind::ScheduleVsPlan,
            format!("Schedule \"{name}\" lists {listed_text} of {mark}; the plans show {shown} ({where_})"),
            format!("الجدول «{name}» يذكر {listed_text} من {mark}، والمخططات تُظهر {shown} ({where_})"),
        );
        f.schedule = Some(schedule.name.clone());
        f.sheets = std::iter::once(schedule.sheet.clone())
            .chain(per_sheet.iter().map(|(s, _)| s.to_string()))
            .collect();
        f.expected = Some(listed);
        f.found = Some(shown as f64);
        f.unit = "pcs".to_string();
        f.subject = mark.to_lowercase();
        out.push(f);
    }
}

/// The mark column and the count column of a schedule header, or `None` when it lacks either.
fn schedule_columns(columns: &[String]) -> Option<(usize, usize)> {
    let words: Vec<BTreeSet<String>> = columns.iter().map(|c| words(c)).collect();
    let has = |i: usize, set: &[&str]| set.iter().any(|w| words[i].contains(*w));
    let qty = (0..columns.len())
        .find(|&i| has(i, &QTY_WORDS))
        .or_else(|| (0..columns.len()).find(|&i| has(i, &["no"])))?;
    let mark = (0..columns.len()).find(|&i| i != qty && has(i, &MARK_WORDS))?;
    Some((mark, qty))
}

/// One block counted differently on two sheets of the same floor: one finding per block and
/// pair of sheets. Anonymous blocks (names starting with `*`) are left out.
fn drawing_vs_drawing(blocks: &[(String, Vec<BlockCount>)], out: &mut Vec<Found>) {
    let mut sheets: Vec<(&str, BTreeSet<String>, BTreeMap<&str, usize>)> = blocks
        .iter()
        .map(|(sheet, counts)| {
            let mut per_block: BTreeMap<&str, usize> = BTreeMap::new();
            for b in counts.iter().filter(|b| !b.block_name.starts_with('*')) {
                *per_block.entry(b.block_name.as_str()).or_default() += b.count;
            }
            (sheet.as_str(), floor_tokens(sheet), per_block)
        })
        .collect();
    sheets.sort_by(|a, b| a.0.cmp(b.0));
    for (i, (a, floors_a, counts_a)) in sheets.iter().enumerate() {
        for (b, floors_b, counts_b) in &sheets[i + 1..] {
            if a == b || floors_a.is_disjoint(floors_b) {
                continue;
            }
            for (block, &ca) in counts_a {
                let Some(&cb) = counts_b.get(block) else {
                    continue;
                };
                if ca == cb {
                    continue;
                }
                let mut f = Found::new(
                    ConflictKind::DrawingVsDrawing,
                    format!("{block} is counted {ca} on {a} and {cb} on {b}"),
                    format!("{block} عدده {ca} في {a} و{cb} في {b}"),
                );
                f.sheets = vec![a.to_string(), b.to_string()];
                f.expected = Some(ca as f64);
                f.found = Some(cb as f64);
                f.unit = "pcs".to_string();
                f.subject = block.to_string();
                out.push(f);
            }
        }
    }
}

/// A quantity as the BOQ panel shows it: whole pieces as whole numbers, else three decimals.
fn qty(n: f64, unit: &str) -> String {
    if unit_key(unit) == "pcs" && n.fract() == 0.0 {
        format!("{n:.0}")
    } else {
        format!("{n:.3}")
    }
}

/// A percentage with its sign: "−18.0 %", "+5.2 %".
fn signed(p: f64) -> String {
    let sign = if p < 0.0 { '−' } else { '+' };
    format!("{sign}{:.1} %", p.abs())
}

/// A unit as English text shows it: "m²" and "m³" for the standard area and volume units.
fn unit_en(unit: &str) -> &str {
    match unit_key(unit).as_str() {
        "m2" => "m²",
        "m3" => "m³",
        _ => unit,
    }
}

fn unit_ar(unit: &str) -> &str {
    match unit_key(unit).as_str() {
        "m" => "م",
        "m2" => "م²",
        "m3" => "م³",
        "pcs" => "قطعة",
        "kg" => "كجم",
        "t" => "طن",
        _ => unit,
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// An Arabic counted noun from its forms for one, two, 3 to 10 and 11 or more.
fn count_ar(n: usize, [one, two, few, many]: [&str; 4]) -> String {
    match n {
        1 => one.to_string(),
        2 => two.to_string(),
        3..=10 => format!("{n} {few}"),
        _ => format!("{n} {many}"),
    }
}

/// `text` cut to `QUOTE` characters.
fn quote(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= QUOTE {
        text.to_string()
    } else {
        text.chars().take(QUOTE).chain(['…']).collect()
    }
}
