//! One step from model space to a printable sheet: `Command::MakeSheet` makes a sized layout, a
//! viewport at a standard scale that fits the paper with margins, a captioned title block with
//! its fields filled (status "For review"), a scale bar and the general notes at a readable
//! height. Everything is paper millimetres and generic: no drawing, trade or unit is assumed.

use std::collections::BTreeMap;

use crate::ink::Bounds;
use crate::layouts::SheetRevision;
use opencadcodec::{EntityType, Handle, Vector3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layouts::{caption, LayoutOrientation};
use crate::rooms::LabelLanguage;
use crate::viewports::ViewportScale;
use crate::{CoreError, DrawSpec, Session, Units};

/// The standard drawing scales, as denominators of 1:n.
pub const STANDARD_SCALES: [f64; 18] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 500.0, 1000.0,
    2000.0, 5000.0, 10000.0,
];
/// A plan is drawn at 1:100 or larger when the paper allows it.
const PREFERRED_DENOMINATOR: f64 = 100.0;
const MARGIN_MM: f64 = 10.0;
const BINDING_MM: f64 = 20.0;
const GAP_MM: f64 = 5.0;
/// Paper height of the general notes and the scale bar's labels (2.5 mm is the least readable).
const NOTE_MM: f64 = 3.0;
const LABEL_MM: f64 = 2.5;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetSpec {
    /// The layout's name, such as "A-101 Ground floor".
    pub name: String,
    /// A paper_catalog name. Omitted: the smallest ISO (ANSI in inches and feet) sheet from A3 up
    /// on which the model fits at 1:100 or larger.
    #[serde(default)]
    pub paper: Option<String>,
    /// Omitted: the one that lets the model take the larger standard scale.
    #[serde(default)]
    pub orientation: Option<LayoutOrientation>,
    /// A standard ratio such as "1:50". Omitted: the largest standard scale that fits.
    #[serde(default)]
    pub scale: Option<String>,
    /// The model window to show, in drawing units. Omitted: everything drawn in Model.
    #[serde(default)]
    pub model_bounds: Option<[[f64; 2]; 2]>,
    /// The language of the captions, notes and Arabic digits; the user's language by default.
    #[serde(default)]
    pub language: Option<LabelLanguage>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub drawing_no: Option<String>,
    #[serde(default)]
    pub client: Option<String>,
    #[serde(default)]
    pub consultant: Option<String>,
    #[serde(default)]
    pub project_no: Option<String>,
    #[serde(default)]
    pub checked_by: Option<String>,
    #[serde(default)]
    pub approved_by: Option<String>,
    /// Revision history, in supplied order; no invented revision or date.
    #[serde(default)]
    pub revision_table: Vec<SheetRevision>,
    /// Optional axis-aligned split lines in model units. Otherwise use tagged grid axes.
    #[serde(default)]
    pub split_lines: Vec<Bounds>,
    /// Enclosed core polygons identified by the designer, in model units. Splits avoid their interiors.
    #[serde(default)]
    pub core_regions: Vec<Vec<[f64; 2]>>,
    /// Optional paper-mm zones chosen by the designer; omitted uses the sheet craft layout.
    #[serde(default)]
    pub zones: Option<SheetZones>,
    /// yyyy-MM-dd; empty unless supplied.
    #[serde(default)]
    pub date: Option<String>,
    /// Empty unless supplied.
    #[serde(default)]
    pub drawn_by: Option<String>,
    /// Empty unless supplied.
    #[serde(default)]
    pub revision: Option<String>,
    /// Compatibility input; generated sheets always state "For review" in their language.
    #[serde(default)]
    pub status: Option<String>,
    /// General notes and written assumptions, one per entry, in the sheet's language.
    #[serde(default)]
    pub notes: Vec<String>,
    /// A scale bar with the scale written above it (true by default).
    #[serde(default = "yes")]
    pub scale_bar: bool,
}

fn yes() -> bool {
    true
}

/// What `make_sheet` made.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SheetReport {
    pub layout: String,
    pub paper: String,
    pub orientation: LayoutOrientation,
    /// The drawing scale, such as "1:100".
    pub scale: String,
    pub viewport: u64,
    pub handles: Vec<u64>,
    /// Title block fields left empty because nothing supplied them (never invented).
    pub empty_fields: Vec<String>,
    /// Model text must be at least this tall to read at this scale (2.5 mm on paper), in drawing
    /// units: set the annotation scale to match before writing model text.
    pub min_text_height: f64,
    pub notes_fit: bool,
    /// Notes returned intact when the column cannot hold them, rather than drawn over other zones.
    pub unplaced_notes: Vec<String>,
    pub language: LabelLanguage,
    /// Continuation sheets. The first sheet retains the existing top-level report contract.
    pub continuations: Vec<SheetReport>,
    pub model_bounds: Bounds,
    pub zones: SheetZones,
    /// Geometry and basis for every chosen split, available for the model's decision record.
    pub splits: Vec<SplitDecision>,
    pub core_regions_supplied: bool,
    pub orientation_basis: String,
    pub orientation_basis_ar: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SplitDecision {
    pub line: Bounds,
    pub basis: String,
    pub basis_ar: String,
}

/// Reserved paper-mm rectangles, each [lower left, upper right].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SheetZones {
    pub plans: Bounds,
    pub notes: Bounds,
    pub schedules: Bounds,
    pub key_plan: Bounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SheetRole {
    Plans,
    Notes,
    KeyPlan,
    Furniture,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SheetGroup {
    pub id: u64,
    pub role: SheetRole,
}

pub(crate) const ZONES_APP: &str = "QIYAS_SHEET_ZONES";
pub(crate) const GROUP_APP: &str = "QIYAS_SHEET_GROUP";

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn ratio(text: &str) -> Result<f64, CoreError> {
    let (paper, model) = text
        .split_once(':')
        .ok_or_else(|| invalid("Write the scale as 1:100"))?;
    let (paper, model) = (paper.trim().parse::<f64>(), model.trim().parse::<f64>());
    match (paper, model) {
        (Ok(p), Ok(m))
            if p.is_finite()
                && m.is_finite()
                && p > 0.0
                && m > 0.0
                && (m / p).is_finite()
                && m / p > 0.0 =>
        {
            Ok(m / p)
        }
        _ => Err(invalid("A scale is two positive numbers, such as 1:100")),
    }
}

fn trimmed(n: f64) -> String {
    n.to_string()
}

/// The paper rectangle the drawing may use: inside the frame, above the title strip.
struct Zone {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

/// A title block row stack: 8 mm rows and a 14 mm title row.
fn strip_height() -> f64 {
    8.0 * (crate::layouts::SHEET_FIELDS.len() - 1) as f64 + 14.0
}

fn zones_of(w: f64, h: f64, revisions: &[SheetRevision]) -> SheetZones {
    let strip_top = MARGIN_MM + strip_height() + crate::layouts::revision_height(revisions);
    let block_left = w - MARGIN_MM - 180.0_f64.min(w - MARGIN_MM - BINDING_MM);
    SheetZones {
        plans: [
            [BINDING_MM + GAP_MM, strip_top + GAP_MM],
            [w - MARGIN_MM - GAP_MM, h - 60.0],
        ],
        notes: [
            [BINDING_MM + GAP_MM, MARGIN_MM],
            [block_left - GAP_MM, strip_top],
        ],
        schedules: [
            // Use the full strip above the plan field, up to the frame margin.
            // Placement adds clearance on every side; extra zone gaps left too little
            // height for a normal opening schedule with numbered tags and total rows.
            [BINDING_MM + GAP_MM, h - 60.0],
            [w - MARGIN_MM - GAP_MM, h - MARGIN_MM],
        ],
        // On split sheets, notes reserve this lower portion of their column for the key plan.
        key_plan: [
            [BINDING_MM + GAP_MM, MARGIN_MM + 23.0],
            [
                (BINDING_MM + 105.0).min(block_left - GAP_MM),
                MARGIN_MM + 73.0,
            ],
        ],
    }
}

fn plan_zone(zones: &SheetZones) -> Zone {
    Zone {
        left: zones.plans[0][0],
        right: zones.plans[1][0],
        bottom: zones.plans[0][1],
        top: zones.plans[1][1],
    }
}

fn valid_bounds(b: Bounds) -> bool {
    b.iter().flatten().all(|v| v.is_finite()) && (0..2).all(|i| b[0][i] < b[1][i])
}

#[derive(Clone)]
struct Cut {
    axis: usize,
    at: f64,
    given: bool,
}

/// Partition only at actual supplied or grid coordinates. No arbitrary midpoint fallback.
fn partition(
    bounds: Bounds,
    capacity: [f64; 2],
    cuts: &[Cut],
    cores: &[Vec<[f64; 2]>],
) -> Option<(Vec<Bounds>, Vec<SplitDecision>)> {
    partition_inner(
        bounds,
        capacity,
        cuts,
        cores,
        &mut std::collections::BTreeSet::new(),
    )
}

fn partition_inner(
    bounds: Bounds,
    capacity: [f64; 2],
    cuts: &[Cut],
    cores: &[Vec<[f64; 2]>],
    failed: &mut std::collections::BTreeSet<[u64; 4]>,
) -> Option<(Vec<Bounds>, Vec<SplitDecision>)> {
    let key = [
        bounds[0][0].to_bits(),
        bounds[0][1].to_bits(),
        bounds[1][0].to_bits(),
        bounds[1][1].to_bits(),
    ];
    if failed.contains(&key) {
        return None;
    }
    if (0..2).all(|i| bounds[1][i] - bounds[0][i] <= capacity[i] + capacity[i] * 1e-9) {
        return Some((vec![bounds], vec![]));
    }
    let mut candidates: Vec<_> = cuts
        .iter()
        .filter(|c| {
            bounds[1][c.axis] - bounds[0][c.axis] > capacity[c.axis]
                && c.at > bounds[0][c.axis]
                && c.at < bounds[1][c.axis]
        })
        .collect();
    // Avoid every supplied core interior; a boundary tangent is allowed. Polygon projection
    // works for concave enclosures too, and does not infer uses or building types.
    candidates.retain(|c| {
        !cores.iter().any(|ring| {
            let lo = ring.iter().map(|p| p[c.axis]).fold(f64::INFINITY, f64::min);
            let hi = ring
                .iter()
                .map(|p| p[c.axis])
                .fold(f64::NEG_INFINITY, f64::max);
            let other = 1 - c.axis;
            let low = ring.iter().map(|p| p[other]).fold(f64::INFINITY, f64::min);
            let high = ring
                .iter()
                .map(|p| p[other])
                .fold(f64::NEG_INFINITY, f64::max);
            c.at > lo && c.at < hi && high > bounds[0][other] && low < bounds[1][other]
        })
    });
    candidates.sort_by(|a, b| {
        b.given.cmp(&a.given).then_with(|| {
            let distance = |c: &Cut| (c.at - (bounds[0][c.axis] + bounds[1][c.axis]) / 2.0).abs();
            distance(a).total_cmp(&distance(b))
        })
    });
    for c in candidates {
        let mut a = bounds;
        let mut b = bounds;
        a[1][c.axis] = c.at;
        b[0][c.axis] = c.at;
        if let (Some((mut left, mut decisions)), Some((right, more))) = (
            partition_inner(a, capacity, cuts, cores, failed),
            partition_inner(b, capacity, cuts, cores, failed),
        ) {
            left.extend(right);
            decisions.extend(more);
            let mut line = bounds;
            line[0][c.axis] = c.at;
            line[1][c.axis] = c.at;
            decisions.push(SplitDecision {
                line,
                basis: if c.given { "Supplied split line; requested scale retained and supplied core interiors avoided" } else { "Tagged grid axis; requested scale retained and supplied core interiors avoided" }.into(),
                basis_ar: if c.given { "خط فصل محدد؛ حُفظ المقياس المطلوب وتُجنبت المناطق الداخلية للنوى المحددة" } else { "محور شبكة معلّم؛ حُفظ المقياس المطلوب وتُجنبت المناطق الداخلية للنوى المحددة" }.into(),
            });
            return Some((left, decisions));
        }
    }
    failed.insert(key);
    None
}

/// The smallest standard scale denominator at which a model `w_m` x `h_m` metres fits `zone`
/// with an inner margin on every side; `None` when even 1:10000 does not.
fn best_denominator(w_m: f64, h_m: f64, zone: &Zone) -> Option<f64> {
    let (avail_w, avail_h) = (
        zone.right - zone.left - 2.0 * GAP_MM,
        zone.top - zone.bottom - 2.0 * GAP_MM,
    );
    if avail_w <= 0.0 || avail_h <= 0.0 {
        return None;
    }
    STANDARD_SCALES
        .into_iter()
        .find(|den| w_m * 1000.0 / den <= avail_w && h_m * 1000.0 / den <= avail_h)
}

impl Session {
    fn model_window(&self) -> Option<[[f64; 2]; 2]> {
        let (bounds, empty) = self.with_model(None, |m| {
            (
                m.bounds,
                m.ranges.is_empty() && m.fill_ranges.is_empty() && m.text_ranges.is_empty(),
            )
        });
        let [[x0, y0], [x1, y1]] = bounds;
        (!empty && x1 >= x0 && y1 >= y0 && [x0, y0, x1, y1].iter().all(|v| v.is_finite()))
            .then_some(bounds)
    }

    pub(crate) fn sheet_zones(&self, sheet: &str) -> Option<SheetZones> {
        self.drawing
            .doc
            .entities_in_block(sheet)
            .find_map(|e| crate::xdata::read_json(e.common(), ZONES_APP))
    }

    pub(crate) fn make_sheet(&mut self, spec: SheetSpec) -> Result<SheetReport, CoreError> {
        let language = spec
            .language
            .or(self.sheet_language())
            .unwrap_or(LabelLanguage::En);
        let bounds = spec.model_bounds.or_else(|| self.model_window()).ok_or_else(|| {
            invalid("Model has nothing to put on a sheet: draw first, or give model_bounds / لا توجد عناصر في النموذج؛ ارسم أو حدد حدود النموذج")
        })?;
        if !valid_bounds(bounds)
            || spec
                .core_regions
                .iter()
                .any(|r| r.len() < 3 || !r.iter().flatten().all(|v| v.is_finite()))
        {
            return Err(invalid("Use finite ordered model bounds and core polygons / استخدم حدود نموذج مرتبة ومضلعات نوى ذات إحداثيات صالحة"));
        }
        let to_m = self.drawing.units.to_metres();
        let (w_m, h_m) = (
            (bounds[1][0] - bounds[0][0]) * to_m,
            (bounds[1][1] - bounds[0][1]) * to_m,
        );
        let forced = spec.scale.as_deref().map(ratio).transpose()?;
        let mut cuts = Vec::new();
        for line in &spec.split_lines {
            if !line.iter().flatten().all(|v| v.is_finite())
                || (line[0][0] == line[1][0]) == (line[0][1] == line[1][1])
            {
                return Err(invalid("Split lines must be horizontal or vertical / خطوط الفصل يجب أن تكون أفقية أو رأسية"));
            }
            let axis = usize::from(line[0][1] == line[1][1]);
            cuts.push(Cut {
                axis,
                at: line[0][axis],
                given: true,
            });
        }
        for e in self.drawing.doc.entities_in_block("*Model_Space") {
            if e.common()
                .extended_data
                .get_record(crate::conventions::GRID_APP)
                .is_none()
            {
                continue;
            }
            if let EntityType::Line(line) = e {
                let a = [line.start.x, line.start.y];
                let b = [line.end.x, line.end.y];
                for axis in 0..2 {
                    if (a[axis] - b[axis]).abs() < 1e-9 && (a[1 - axis] - b[1 - axis]).abs() > 1e-9
                    {
                        cuts.push(Cut {
                            axis,
                            at: a[axis],
                            given: false,
                        });
                    }
                }
            }
        }
        let imperial = matches!(self.drawing.units, Units::Inches | Units::Feet);
        let candidates: Vec<String> = match &spec.paper {
            Some(name) => vec![name.clone()],
            None if imperial => ["ANSI B", "ANSI C", "ANSI D", "ANSI E"]
                .map(String::from)
                .to_vec(),
            None => ["ISO A3", "ISO A2", "ISO A1", "ISO A0"]
                .map(String::from)
                .to_vec(),
        };
        let aspect = if w_m >= h_m {
            LayoutOrientation::Landscape
        } else {
            LayoutOrientation::Portrait
        };
        let other = if aspect == LayoutOrientation::Landscape {
            LayoutOrientation::Portrait
        } else {
            LayoutOrientation::Landscape
        };
        let orientations = spec
            .orientation
            .map_or_else(|| vec![aspect, other], |o| vec![o]);
        let mut chosen = None;
        for (i, name) in candidates.iter().enumerate() {
            let paper = crate::layouts::lookup_paper(name)?;
            let mut best = None;
            for &orientation in &orientations {
                let (w, h) = match orientation {
                    LayoutOrientation::Portrait => (paper.w_mm, paper.h_mm),
                    LayoutOrientation::Landscape => (paper.h_mm, paper.w_mm),
                };
                let zones = spec
                    .zones
                    .clone()
                    .unwrap_or_else(|| zones_of(w, h, &spec.revision_table));
                if spec.zones.is_some()
                    && [zones.plans, zones.notes, zones.schedules, zones.key_plan]
                        .iter()
                        .any(|b| !valid_bounds(*b))
                {
                    return Err(invalid("Sheet zones need finite ordered rectangles / مناطق اللوحة تحتاج مستطيلات مرتبة بإحداثيات صالحة"));
                }
                let zone = plan_zone(&zones);
                let den = forced.or_else(|| best_denominator(w_m, h_m, &zone));
                let Some(den) = den else {
                    continue;
                };
                let capacity = [
                    (zone.right - zone.left - 2.0 * GAP_MM) * den / (1000.0 * to_m),
                    (zone.top - zone.bottom - 2.0 * GAP_MM) * den / (1000.0 * to_m),
                ];
                if capacity.iter().any(|n| !n.is_finite() || *n <= 0.0) {
                    continue;
                }
                let Some((parts, splits)) = partition(bounds, capacity, &cuts, &spec.core_regions)
                else {
                    continue;
                };
                if parts.len() > 1
                    && (!valid_bounds(zones.key_plan)
                        || zones.key_plan[1][0] - zones.key_plan[0][0] <= 10.0)
                {
                    continue;
                }
                let score = (parts.len(), den);
                if best
                    .as_ref()
                    .is_none_or(|(old, _, _, _, _, _)| score < *old)
                {
                    best = Some((score, orientation, den, zones, parts, splits));
                }
            }
            if let Some((_, orientation, den, zones, parts, splits)) = best {
                let good = if forced.is_some() {
                    parts.len() == 1
                } else {
                    den <= PREFERRED_DENOMINATOR
                };
                chosen = Some((paper, orientation, den, zones, parts, splits));
                if good || i + 1 == candidates.len() {
                    break;
                }
            }
        }
        let (paper, orientation, den, zones, parts, splits) = chosen.ok_or_else(|| invalid(
            "The requested scale needs fitting split lines outside the supplied cores; give split_lines, another paper or scale / المقياس المطلوب يحتاج خطوط فصل مناسبة خارج النوى المحددة؛ حدد خطوط الفصل أو ورقًا أو مقياسًا آخر"
        ))?;
        let scale_text = format!("1:{den}");
        let names: Vec<_> = (0..parts.len())
            .map(|i| {
                if i == 0 {
                    spec.name.trim().to_string()
                } else {
                    format!("{}-{}", spec.name.trim(), i + 1)
                }
            })
            .collect();
        for name in &names {
            if self
                .drawing
                .sheets()
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(name))
            {
                return Err(invalid(format!(
                    "The drawing already has a sheet named {name} / توجد لوحة بهذا الاسم: {name}"
                )));
            }
        }
        let shown = self.sheet.clone();
        let made = (|| {
            let mut reports = Vec::new();
            for (i, part) in parts.iter().enumerate() {
                // Only the initial empty layout of a drawing created in this session can be reused.
                // An engineer-opened drawing is never eligible, even if its Layout1 is empty.
                let reuse = self
                    .created_here
                    .then(|| {
                        self.drawing.sheets().into_iter().find(|s| {
                            s.name == "Layout1"
                                && self
                                    .drawing
                                    .doc
                                    .entities_in_block(&s.block)
                                    .all(|e| matches!(e, EntityType::Viewport(v) if v.id == 1))
                        })
                    })
                    .flatten();
                if let Some(layout) = reuse {
                    self.rename_layout(&layout.name, &names[i])?;
                    self.set_layout_paper(&names[i], &paper.name, orientation)?;
                } else {
                    self.new_layout(&names[i], &paper.name, orientation)?;
                }
                let block = self
                    .drawing
                    .sheets()
                    .into_iter()
                    .find(|s| s.name == names[i])
                    .map(|s| s.block)
                    .ok_or_else(|| {
                        invalid(
                            "The new layout could not be found / تعذر العثور على اللوحة الجديدة",
                        )
                    })?;
                self.sheet = block;
                let mut page_spec = spec.clone();
                page_spec.name = names[i].clone();
                if parts.len() > 1 {
                    if let Some(number) = spec.drawing_no.as_ref().filter(|s| !s.trim().is_empty())
                    {
                        page_spec.drawing_no = Some(format!("{}-{}", number.trim(), i + 1));
                    }
                }
                let (viewport, mut handles, mut empty_fields, notes_fit) = self.sheet_contents(
                    &page_spec,
                    language,
                    den,
                    &scale_text,
                    *part,
                    &zones,
                    parts.len() > 1,
                )?;
                let app = crate::xdata::register(&mut self.drawing.doc, ZONES_APP);
                crate::xdata::write_json(
                    self.drawing
                        .doc
                        .get_entity_mut(Handle::new(viewport))
                        .expect("new viewport")
                        .common_mut(),
                    ZONES_APP,
                    app,
                    &zones,
                )?;
                if spec.revision_table.is_empty() {
                    empty_fields.push("revision_table".into());
                }
                for (row, rev) in spec.revision_table.iter().enumerate() {
                    for (key, value) in [
                        ("rev", &rev.rev),
                        ("date", &rev.date),
                        ("description", &rev.description),
                    ] {
                        if value.trim().is_empty() {
                            empty_fields.push(format!("revision_table[{row}].{key}"));
                        }
                    }
                }
                if parts.len() > 1 {
                    handles.extend(self.sheet_navigation(
                        viewport, i, &parts, &names, &spec, bounds, &zones, language, den,
                    )?);
                }
                reports.push(SheetReport {
                    layout: names[i].clone(),
                    paper: paper.name.clone(),
                    orientation,
                    scale: scale_text.clone(),
                    viewport,
                    handles,
                    empty_fields,
                    min_text_height: LABEL_MM * den / 1000.0 / to_m,
                    notes_fit,
                    unplaced_notes: if notes_fit {
                        vec![]
                    } else {
                        spec.notes.clone()
                    },
                    language,
                    continuations: vec![],
                    model_bounds: *part,
                    zones: zones.clone(),
                    splits: splits.clone(),
                    core_regions_supplied: !spec.core_regions.is_empty(),
                    orientation_basis: if spec.orientation.is_some() {
                        "Explicit orientation retained"
                    } else {
                        "Compared fitting sheet counts and scales; plan aspect breaks ties"
                    }
                    .into(),
                    orientation_basis_ar: if spec.orientation.is_some() {
                        "حُفظ الاتجاه المحدد"
                    } else {
                        "قورنت أعداد اللوحات الملائمة والمقاييس؛ وعند التعادل تُتبع نسب المسقط"
                    }
                    .into(),
                });
            }
            let mut first = reports.remove(0);
            first.continuations = reports;
            Ok(first)
        })();
        self.sheet = shown;
        if made.is_ok() {
            if let Some(block) = self
                .drawing
                .sheets()
                .into_iter()
                .find(|s| s.name == names[0])
                .map(|s| s.block)
            {
                self.set_made_sheet(&block);
            }
        }
        made
    }

    pub(crate) fn mark_sheet_group(
        &mut self,
        handles: &[u64],
        id: u64,
        role: SheetRole,
    ) -> Result<(), CoreError> {
        let app = crate::xdata::register(&mut self.drawing.doc, GROUP_APP);
        for h in handles {
            let entity = self
                .drawing
                .doc
                .get_entity_mut(Handle::new(*h))
                .expect("new sheet entity");
            crate::xdata::write_json(
                entity.common_mut(),
                GROUP_APP,
                app,
                &SheetGroup { id, role },
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn sheet_navigation(
        &mut self,
        viewport: u64,
        index: usize,
        parts: &[Bounds],
        names: &[String],
        spec: &SheetSpec,
        whole: Bounds,
        zones: &SheetZones,
        language: LabelLanguage,
        den: f64,
    ) -> Result<Vec<u64>, CoreError> {
        let arabic = language == LabelLanguage::Ar;
        let part = parts[index];
        let scale = 1000.0 * self.drawing.units.to_metres() / den;
        let centre = [
            (zones.plans[0][0] + zones.plans[1][0]) / 2.0,
            (zones.plans[0][1] + zones.plans[1][1]) / 2.0,
        ];
        let map = |p: [f64; 2]| {
            [
                centre[0] + (p[0] - (part[0][0] + part[1][0]) / 2.0) * scale,
                centre[1] + (p[1] - (part[0][1] + part[1][1]) / 2.0) * scale,
            ]
        };
        let mut entities = Vec::new();
        for (j, other) in parts.iter().enumerate().filter(|(j, _)| *j != index) {
            for axis in 0..2 {
                for edge in 0..2 {
                    if part[edge][axis] != other[1 - edge][axis] {
                        continue;
                    }
                    let cross = 1 - axis;
                    let lo = part[0][cross].max(other[0][cross]);
                    let hi = part[1][cross].min(other[1][cross]);
                    if hi <= lo {
                        continue;
                    }
                    let mut a = part[edge];
                    let mut b = a;
                    a[cross] = lo;
                    b[cross] = hi;
                    let (a, b) = (map(a), map(b));
                    // Dashed paper-space match line and an adjacent reference label.
                    let length = (b[cross] - a[cross]).abs();
                    let mut offset = 0.0;
                    while offset < length {
                        let mut start = a;
                        let mut end = a;
                        start[cross] += offset;
                        end[cross] += (offset + 4.0).min(length);
                        entities.extend(self.draw_entities(DrawSpec::Line { a: start, b: end })?);
                        offset += 6.0;
                    }
                    let number = spec
                        .drawing_no
                        .as_ref()
                        .filter(|s| !s.trim().is_empty())
                        .map_or_else(|| names[j].clone(), |n| format!("{}-{}", n.trim(), j + 1));
                    let label = if arabic {
                        format!("خط المطابقة - انظر اللوحة {number}")
                    } else {
                        format!("Match line - see sheet {number}")
                    };
                    let mut at = a;
                    at[cross] = (a[cross] + b[cross]) / 2.0;
                    at[axis] += if edge == 0 { -2.0 } else { 2.0 };
                    let mut text = self.mtext_entity(
                        Vector3::new(at[0], at[1], 0.0),
                        0.0,
                        LABEL_MM,
                        &label,
                        None,
                    )?;
                    if let EntityType::MText(t) = &mut text {
                        t.attachment_point = opencadcodec::entities::AttachmentPoint::BottomCenter;
                        if axis == 0 {
                            t.rotation = if edge == 0 {
                                std::f64::consts::FRAC_PI_2
                            } else {
                                -std::f64::consts::FRAC_PI_2
                            };
                        } else if edge == 0 {
                            t.attachment_point = opencadcodec::entities::AttachmentPoint::TopCenter;
                        }
                    }
                    entities.push(text);
                }
            }
        }
        let mut handles = self.add_on_annotation_layer("A-ANNO-MATCH", entities, false)?;
        let mut plan_group = handles.clone();
        plan_group.push(viewport);
        self.mark_sheet_group(&plan_group, viewport, SheetRole::Plans)?;

        // A real miniature viewport shows the whole outline, including irregular boundaries.
        // The bold rectangle marks the extent shown on this sheet, with a matching sheet label.
        let [[kx0, ky0], [kx1, ky1]] = zones.key_plan;
        if kx1 <= kx0 || ky1 - ky0 <= 8.0 {
            return Err(invalid(
                "The key plan zone has no room / لا تتسع منطقة المخطط المفتاحي",
            ));
        }
        let key_scale = ((kx1 - kx0 - 4.0) / (whole[1][0] - whole[0][0]))
            .min((ky1 - ky0 - 10.0) / (whole[1][1] - whole[0][1]));
        let key_centre = [(kx0 + kx1) / 2.0, (ky0 + ky1 - 6.0) / 2.0];
        let key_frame = self.viewport_frame(
            [0.0, 0.0],
            [0.0, 0.0],
            Some(&ViewportScale::Number(key_scale)),
            None,
            Some(whole),
            Some(key_centre),
        )?;
        let key = self.mview(key_frame)?;
        let key_map = |p: [f64; 2]| {
            [
                key_centre[0] + (p[0] - (whole[0][0] + whole[1][0]) / 2.0) * key_scale,
                key_centre[1] + (p[1] - (whole[0][1] + whole[1][1]) / 2.0) * key_scale,
            ]
        };
        let mut outline = self.draw_entities(DrawSpec::Rectangle {
            a: key_map(part[0]),
            b: key_map(part[1]),
        })?;
        let label = if arabic {
            "المخطط المفتاحي - الجزء في هذه اللوحة"
        } else {
            "Key plan - part on this sheet"
        };
        outline.push(self.mtext_entity(
            Vector3::new(kx0, ky1, 0.0),
            kx1 - kx0,
            LABEL_MM,
            label,
            None,
        )?);
        let mut key_handles = self.add_on_annotation_layer("A-ANNO-KEY", outline, false)?;
        for h in &key_handles {
            if let Some(EntityType::LwPolyline(p)) =
                self.drawing.doc.get_entity_mut(Handle::new(*h))
            {
                p.common.line_weight = opencadcodec::types::LineWeight::W0_50;
            }
        }
        key_handles.push(key);
        self.mark_sheet_group(&key_handles, key, SheetRole::KeyPlan)?;
        handles.extend(key_handles);
        Ok(handles)
    }

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn sheet_contents(
        &mut self,
        spec: &SheetSpec,
        language: LabelLanguage,
        den: f64,
        scale_text: &str,
        bounds: [[f64; 2]; 2],
        zones: &SheetZones,
        split: bool,
    ) -> Result<(u64, Vec<u64>, Vec<String>, bool), CoreError> {
        let arabic = language == LabelLanguage::Ar;
        let digits = |text: &str| {
            if arabic {
                crate::numbers::arabic_numerals(text)
            } else {
                text.to_string()
            }
        };
        let zone = plan_zone(zones);
        let mut handles = Vec::new();

        // The viewport: the window itself (its extents already hold its text), centred in the
        // zone, at the scale; the zone keeps an inner margin round it.
        let [[x0, y0], [x1, y1]] = bounds;
        let model = [[x0, y0], [x1, y1]];
        let centre = [
            (zone.left + zone.right) / 2.0,
            (zone.bottom + zone.top) / 2.0,
        ];
        let frame = self.viewport_frame(
            [0.0, 0.0],
            [0.0, 0.0],
            Some(&ViewportScale::Ratio(scale_text.to_string())),
            None,
            Some(model),
            Some(centre),
        )?;
        let viewport = self.mview(frame)?;
        handles.push(viewport);

        // The title block, captioned, with every field that has a value.
        let mut fields = BTreeMap::new();
        let mut put = |key: &str, value: Option<&str>| {
            if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
                fields.insert(key.to_string(), v.to_string());
            }
        };
        put("project", spec.project.as_deref());
        put("title", spec.title.as_deref());
        put("drawing_no", spec.drawing_no.as_deref());
        put("scale", Some(&digits(scale_text)));
        put("date", spec.date.as_ref().map(|d| digits(d)).as_deref());
        put("drawn_by", spec.drawn_by.as_deref());
        put("revision", spec.revision.as_deref());
        put("client", spec.client.as_deref());
        put("consultant", spec.consultant.as_deref());
        put("project_no", spec.project_no.as_deref());
        put("checked_by", spec.checked_by.as_deref());
        put("approved_by", spec.approved_by.as_deref());
        put(
            "status",
            Some(if arabic {
                "للمراجعة"
            } else {
                "For review"
            }),
        );
        put("sheet", Some(spec.name.trim()));
        let empty_fields: Vec<String> = crate::layouts::SHEET_FIELDS
            .iter()
            .filter(|k| !fields.contains_key(**k))
            .map(|k| (*k).to_string())
            .collect();
        handles.extend(self.title_block_with(
            &fields,
            MARGIN_MM,
            BINDING_MM,
            &crate::layouts::default_title_layer(),
            Some(language),
        )?);

        handles.extend(self.sheet_revision_table(
            &spec.revision_table,
            language,
            MARGIN_MM + strip_height(),
            MARGIN_MM,
            BINDING_MM,
            &crate::layouts::default_title_layer(),
        )?);

        // One notes column beside the title block, with the scale bar below.
        let [[strip_left, strip_bottom], [strip_right, strip_top]] = zones.notes;
        let mut entities: Vec<EntityType> = Vec::new();
        let width = strip_right - strip_left;
        let right_aligned = arabic;
        let anchor_x = if arabic { strip_right } else { strip_left };
        let style_note = |this: &mut Session, y: f64, text: &str, size: f64, wrap: f64| {
            let mut e =
                this.mtext_entity(Vector3::new(anchor_x, y, 0.0), wrap, size, text, None)?;
            if let EntityType::MText(t) = &mut e {
                if right_aligned {
                    t.attachment_point = opencadcodec::entities::AttachmentPoint::TopRight;
                }
            }
            Ok::<_, CoreError>(e)
        };
        let mut notes_fit = true;
        let bar_reserve = if split {
            (zones.key_plan[1][1] - strip_bottom + GAP_MM).max(22.0)
        } else if spec.scale_bar {
            22.0
        } else {
            0.0
        };
        if !spec.notes.is_empty() && width < 40.0 {
            // No room beside the title block on this paper: say so rather than overlap it.
            notes_fit = false;
        }
        if !spec.notes.is_empty() && width >= 40.0 {
            let heading = if arabic {
                "ملاحظات عامة"
            } else {
                "General notes"
            };
            entities.push(style_note(self, strip_top - 2.0, heading, 3.5, width)?);
            let numbered: Vec<String> = spec
                .notes
                .iter()
                .enumerate()
                .map(|(i, n)| format!("{}. {}", digits(&(i + 1).to_string()), n.trim()))
                .collect();
            // Characters per line at this width (a text of height h is about 0.6 h per character).
            let per_line = (width / (NOTE_MM * 0.6)).floor().max(10.0);
            let lines: f64 = numbered
                .iter()
                .map(|n| {
                    n.lines()
                        .map(|line| (line.chars().count() as f64 / per_line).ceil().max(1.0))
                        .sum::<f64>()
                })
                .sum();
            let needed = lines * NOTE_MM * 1.5 + 8.0;
            notes_fit = needed <= strip_top - strip_bottom - bar_reserve - 4.0;
            if notes_fit {
                entities.push(style_note(
                    self,
                    strip_top - 9.0,
                    &numbered.join("\n"),
                    NOTE_MM,
                    width,
                )?);
            }
        }
        if !entities.is_empty() {
            let notes = self.add_on_annotation_layer("A-ANNO-NOTE", entities, false)?;
            if let Some(&id) = notes.first() {
                self.mark_sheet_group(&notes, id, SheetRole::Notes)?;
            }
            handles.extend(notes);
        }
        if spec.scale_bar && width >= 40.0 {
            let bar = self.scale_bar(
                den,
                language,
                strip_left,
                strip_bottom + 9.0,
                scale_text,
                (width - 10.0).min(90.0),
            )?;
            let bar = self.add_on_annotation_layer("A-ANNO-SCALE", bar, false)?;
            if let Some(&id) = bar.first() {
                self.mark_sheet_group(&bar, id, SheetRole::Furniture)?;
            }
            handles.extend(bar);
        }
        Ok((viewport, handles, empty_fields, notes_fit))
    }

    /// A scale bar with 4 alternate segments of the longest round model length that fits `max_len_mm`
    /// on paper (90 at most), labelled 0, half and full length in metres, the scale written above it.
    fn scale_bar(
        &mut self,
        den: f64,
        language: LabelLanguage,
        x: f64,
        y: f64,
        scale_text: &str,
        max_len_mm: f64,
    ) -> Result<Vec<EntityType>, CoreError> {
        let arabic = language == LabelLanguage::Ar;
        let digits = |text: &str| {
            if arabic {
                crate::numbers::arabic_digits(text)
            } else {
                text.to_string()
            }
        };
        let length_m = [
            0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0,
            100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0,
        ]
        .into_iter()
        .rev()
        .find(|m| m * 1000.0 / den <= max_len_mm)
        .unwrap_or(1.0);
        let len = length_m * 1000.0 / den;
        let segment = len / 4.0;
        let bar_h = 2.0;
        let mut out = Vec::new();
        out.extend(self.draw_entities(DrawSpec::Rectangle {
            a: [x, y],
            b: [x + len, y + bar_h],
        })?);
        for i in [0.0, 2.0] {
            out.extend(
                self.draw_entities(DrawSpec::Solid2D {
                    // A 2D solid takes its corners in the order 1, 2, 4, 3.
                    points: vec![
                        [x + i * segment, y],
                        [x + (i + 1.0) * segment, y],
                        [x + i * segment, y + bar_h],
                        [x + (i + 1.0) * segment, y + bar_h],
                    ]
                    .into(),
                })?,
            );
        }
        let unit = if arabic { "م" } else { "m" };
        for (at, label) in [
            (0.0, "0".to_string()),
            (len / 2.0, trimmed(length_m / 2.0)),
            (len, format!("{} {unit}", trimmed(length_m))),
        ] {
            let label = self.mtext_entity(
                Vector3::new(x + at - 1.0, y - 1.5, 0.0),
                0.0,
                LABEL_MM,
                &digits(&label),
                None,
            )?;
            out.push(label);
        }
        let heading = format!("{} {}", caption("scale", language), digits(scale_text));
        out.push(self.mtext_entity(
            Vector3::new(x, y + bar_h + 5.0, 0.0),
            0.0,
            LABEL_MM,
            &heading,
            None,
        )?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_the_largest_standard_one_that_fits() {
        // Reserved title, notes and schedule zones determine the remaining plan field.
        let zone = plan_zone(&zones_of(420.0, 297.0, &[]));
        assert_eq!(best_denominator(12.0, 15.0, &zone), Some(200.0));
        let zone = plan_zone(&zones_of(297.0, 420.0, &[]));
        assert_eq!(best_denominator(12.0, 15.0, &zone), Some(75.0));
        assert_eq!(best_denominator(0.5, 0.5, &zone), Some(5.0));
        assert_eq!(best_denominator(1.0e6, 1.0e6, &zone), None);
    }

    #[test]
    fn ratios_read_and_numbers_trim() {
        assert_eq!(ratio("1:50").unwrap(), 50.0);
        assert!(ratio("0:5").is_err());
        assert!(ratio("fit").is_err());
        assert_eq!(trimmed(2.5), "2.5");
        assert_eq!(trimmed(5.0), "5");
    }
}
