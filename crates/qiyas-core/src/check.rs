//! Checking a drawing as a reviewer would mark it up: line work that stops short of or runs past
//! another line, repeats, scraps of segments, rooms whose outline is open or whose size label
//! does not match, text that collides or can't be read at the scale, entities on layer 0 or in
//! colours of their own, dimension text that disagrees with what it measures, `\n` written as two
//! characters, entities far off from the rest, and take-off rows that lost their entity.
//! `Command::CheckDrawing` runs it on the sheet on screen (or `scope.sheet`); the AI runs it after
//! its own changes, on what they touched (`scope.handles`).
//!
//! Distances are worked out in metres and compared in drawing units, so a rule means the same in
//! a millimetre plan and a metre one.

use std::collections::{BTreeMap, HashMap, HashSet};

use opencadcodec::{tables::normalize_name, Color, EntityType};
use opencadkernel::geom2d::Curve;
use qiyas_geom::Xform;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Boundary, Session, Units};

/// Every rule `check_drawing` knows, in the order findings are listed.
pub const RULES: [&str; 18] = [
    "literal_newline",
    "duplicate",
    "overlap",
    "dangling_end",
    "overshoot",
    "tiny_segment",
    "room_open",
    "room_size_text_differs",
    "room_area_mismatch",
    "dim_override_mismatch",
    "text_overlaps_text",
    "text_over_lines",
    "text_height_vs_scale",
    "far_off_extents",
    "unlinked_takeoff",
    "layer_zero",
    "colour_not_bylayer",
    "missing_center_mark",
];

/// The pack that owns a drawing check rule.
pub fn rule_pack(rule: &str) -> &'static str {
    match rule {
        "room_open" | "room_size_text_differs" | "room_area_mismatch" | "unlinked_takeoff" => {
            "construction"
        }
        _ => "general",
    }
}

/// Two ends this close touch (metres).
const TOUCH_M: f64 = 0.0005;
/// An end this close to another line without touching it stops short; an end this far past a
/// line it crosses runs over (metres).
const NEAR_M: f64 = 0.2;
/// A segment shorter than this is a scrap (metres).
const TINY_M: f64 = 0.001;
/// Room size labels and rooms may differ by this much (a fraction).
const SIZE_SLACK: f64 = 0.05;
/// Dimension overrides may differ by 5 mm as well as 1% of the measured length.
const DIM_TOL_M: f64 = 0.005;
/// Text on paper smaller than this can't be read, larger than this shouts (millimetres).
const PAPER_MIN_MM: f64 = 1.5;
const PAPER_MAX_MM: f64 = 15.0;
/// Most findings listed (the counts count them all).
const MAX_FINDINGS: usize = 200;
/// Most texts looked at for rooms and collisions.
const MAX_TEXTS: usize = 2_000;
/// Most room texts traced round.
const MAX_ROOMS: usize = 200;

/// What to check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CheckScope {
    /// The sheet (its name or block); the sheet on screen when none.
    pub sheet: Option<String>,
    /// Only findings that involve one of these entities (what a change touched); every finding
    /// when empty.
    pub handles: Vec<u64>,
    /// Packs to check: "general" and "construction". Empty selects general, plus construction
    /// when the drawing has take-off rows or a layer named for rooms, spaces or areas.
    pub packs: Vec<String>,
    /// Only these rules; every rule when empty: literal_newline, duplicate, overlap,
    /// dangling_end, overshoot, tiny_segment, room_open, room_size_text_differs,
    /// dim_override_mismatch, text_overlaps_text, text_over_lines, text_height_vs_scale,
    /// far_off_extents, unlinked_takeoff, layer_zero, colour_not_bylayer, missing_center_mark.
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// One thing to look at.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub pack: &'static str,
    pub severity: Severity,
    pub handles: Vec<u64>,
    /// Where, in drawing coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f64; 2]>,
    pub message: String,
}

/// What `check_drawing` found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckReport {
    /// The sheet checked (its block).
    pub sheet: String,
    /// Entities looked at.
    pub entities: usize,
    /// Findings, errors first, at most 200.
    pub findings: Vec<Finding>,
    /// Every finding by rule, listed or not.
    pub counts: BTreeMap<String, usize>,
    pub errors: usize,
    pub warnings: usize,
    /// One line to say it all (under 200 characters).
    pub line: String,
}

/// A drawn entity's line work.
struct Drawn {
    handle: u64,
    layer: String,
    paths: Vec<qiyas_geom::Path>,
    /// Whether its ends are ends (a line, an open polyline or arc), not a closed outline.
    open: bool,
    /// Actual straight edges, excluding the chords used to display curves.
    straight: Vec<([f64; 2], [f64; 2])>,
    /// Actual edge lengths; a smooth sampled curve contributes its whole path length.
    lengths: Vec<([f64; 2], f64)>,
}

/// One segment of the line work, for the grid.
#[derive(Clone, Copy)]
struct Seg {
    owner: usize,
    a: [f64; 2],
    b: [f64; 2],
}

/// Segments by grid cell.
struct Grid {
    cell: f64,
    cells: HashMap<(i64, i64), Vec<usize>>,
    segs: Vec<Seg>,
}

impl Grid {
    fn new(segs: Vec<Seg>, cell: f64) -> Grid {
        let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, s) in segs.iter().enumerate() {
            let (x0, x1) = (s.a[0].min(s.b[0]), s.a[0].max(s.b[0]));
            let (y0, y1) = (s.a[1].min(s.b[1]), s.a[1].max(s.b[1]));
            let (cx0, cy0) = ((x0 / cell).floor() as i64, (y0 / cell).floor() as i64);
            let (cx1, cy1) = ((x1 / cell).floor() as i64, (y1 / cell).floor() as i64);
            // A very long segment in a fine grid: cap the cells it spans.
            if cx1
                .saturating_sub(cx0)
                .saturating_add(1)
                .saturating_mul(cy1.saturating_sub(cy0).saturating_add(1))
                > 4_096
            {
                cells.entry((i64::MIN, i64::MIN)).or_default().push(i);
                continue;
            }
            for cx in cx0..=cx1 {
                for cy in cy0..=cy1 {
                    cells.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        Grid { cell, cells, segs }
    }

    /// Segments in cells touched by a box, each once.
    fn within(&self, bounds: [[f64; 2]; 2]) -> Vec<usize> {
        let cell_at = |p: [f64; 2]| {
            [
                (p[0] / self.cell).floor() as i64,
                (p[1] / self.cell).floor() as i64,
            ]
        };
        let (lo, hi) = (cell_at(bounds[0]), cell_at(bounds[1]));
        let count = hi[0]
            .saturating_sub(lo[0])
            .saturating_add(1)
            .saturating_mul(hi[1].saturating_sub(lo[1]).saturating_add(1));
        if count > 4_096 {
            return (0..self.segs.len()).collect();
        }
        let mut out = Vec::new();
        for cx in lo[0]..=hi[0] {
            for cy in lo[1]..=hi[1] {
                if let Some(list) = self.cells.get(&(cx, cy)) {
                    out.extend(list);
                }
            }
        }
        if let Some(list) = self.cells.get(&(i64::MIN, i64::MIN)) {
            out.extend(list);
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The segments near `p` (within one cell), each once.
    fn near(&self, p: [f64; 2]) -> Vec<usize> {
        let (cx, cy) = (
            (p[0] / self.cell).floor() as i64,
            (p[1] / self.cell).floor() as i64,
        );
        let mut out: Vec<usize> = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self
                    .cells
                    .get(&(cx.saturating_add(dx), cy.saturating_add(dy)))
                {
                    out.extend(list);
                }
            }
        }
        if let Some(list) = self.cells.get(&(i64::MIN, i64::MIN)) {
            out.extend(list);
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

impl Session {
    /// Definitions inherit the space of their reference. A dimension/table block is not a
    /// paper layout merely because its name differs from Model_Space.
    fn check_space(&self, block: &str) -> (f64, f64) {
        let doc = &self.drawing.doc;
        let mut name = block.to_string();
        let mut seen = HashSet::new();
        while seen.insert(name.to_uppercase()) {
            if name.eq_ignore_ascii_case("*Model_Space") {
                break;
            }
            let handle = doc.block_records.get(&name).map(|b| b.handle);
            if let Some(layout) = doc.objects.values().find_map(|object| match object {
                opencadcodec::objects::ObjectType::Layout(layout)
                    if Some(layout.block_record) == handle =>
                {
                    Some(layout)
                }
                _ => None,
            }) {
                let inches = layout.plot_paper_units == 0
                    && layout.paper_width > 0.0
                    && layout.paper_height > 0.0;
                return (if inches { 0.0254 } else { 0.001 }, 1.0);
            }
            let parent = doc.entities().find_map(|entity| {
                let reference = match entity {
                    EntityType::Insert(insert) => &insert.block_name,
                    EntityType::Dimension(dimension) => &dimension.base().block_name,
                    EntityType::Table(table) => &table.block_name,
                    _ => return None,
                };
                reference
                    .eq_ignore_ascii_case(&name)
                    .then_some(entity.common().owner_handle)
            });
            let Some(owner) = parent else {
                break;
            };
            let Some(parent) = doc.block_records.iter().find(|b| b.handle == owner) else {
                break;
            };
            name = parent.name.clone();
        }
        (self.drawing.units.to_metres(), self.annotation_scale().1)
    }

    /// Checks the sheet `scope.sheet` (the one on screen by default) and lists what it finds.
    pub fn check_drawing(&self, scope: &CheckScope) -> CheckReport {
        self.check_drawing_with_limit(scope, MAX_FINDINGS)
    }

    /// The same check without truncating its findings. Approval compares complete error sets
    /// so an old error beyond the display limit cannot be mistaken for a newly introduced one.
    pub fn check_drawing_all(&self, scope: &CheckScope) -> CheckReport {
        self.check_drawing_with_limit(scope, usize::MAX)
    }

    fn check_drawing_with_limit(&self, scope: &CheckScope, limit: usize) -> CheckReport {
        let sheet = self.sheet_block(scope.sheet.as_deref());
        let (to_m, paper_scale) = self.check_space(&sheet);
        let unit = |metres: f64| metres / to_m;
        let doc = &self.drawing.doc;
        let construction = !self.takeoff.items.is_empty()
            || doc.layers.iter().any(|layer| {
                let name = layer.name.to_lowercase();
                ["room", "space", "area"]
                    .iter()
                    .any(|word| name.contains(word))
            });
        let wanted = |rule: &str| {
            let pack = rule_pack(rule);
            let enabled = if scope.packs.is_empty() {
                pack == "general" || construction
            } else {
                scope.packs.iter().any(|p| p == pack)
            };
            enabled && (scope.rules.is_empty() || scope.rules.iter().any(|r| r == rule))
        };
        let tol = unit(0.0002);
        let dimension = doc.entities().find_map(|entity| match entity {
            EntityType::Dimension(dimension)
                if dimension.base().block_name.eq_ignore_ascii_case(&sheet) =>
            {
                Some(dimension)
            }
            _ => None,
        });
        let extension = dimension.map(|d| qiyas_geom::DimLook::of(doc, d).ext_extension);
        let dimension_ends: Vec<_> = dimension
            .into_iter()
            .flat_map(|d| qiyas_geom::dimension_entities(doc, d).entities)
            .filter_map(|entity| match entity {
                EntityType::Line(line) => {
                    Some([[line.start.x, line.start.y], [line.end.x, line.end.y]])
                }
                _ => None,
            })
            .flatten()
            .collect();

        let entities: Vec<&EntityType> = self.drawing.entities_in(&sheet).collect();
        let mut drawn: Vec<Drawn> = Vec::new();
        for e in &entities {
            let open = match e {
                EntityType::Line(_) | EntityType::Arc(_) => true,
                EntityType::LwPolyline(p) => !p.is_closed,
                EntityType::Polyline2D(_) | EntityType::Spline(_) => true,
                EntityType::Circle(_) | EntityType::Ellipse(_) => false,
                _ => continue,
            };
            let paths = qiyas_geom::flatten(doc, e, &Xform::identity(), tol).into_paths();
            let open = open && paths.iter().all(|p| !p.closed);
            let mut straight = Vec::new();
            let mut lengths = Vec::new();
            for shape in qiyas_geom::snap_shapes(doc, e, &Xform::identity(), tol) {
                if shape.exact {
                    for piece in shape.pieces {
                        if let Curve::Line(line) = &piece {
                            straight.push((line.start, line.end));
                        }
                        lengths.push((piece.point_at(0.0), piece.length()));
                    }
                    // Snapping omits zero-length edges. They are still drawing faults.
                    for pair in shape.vertices.windows(2) {
                        if pair[0] == pair[1] {
                            lengths.push((pair[0], 0.0));
                        }
                    }
                } else {
                    lengths.extend(
                        paths
                            .iter()
                            .filter_map(|path| path.points.first().map(|&at| (at, path.length()))),
                    );
                }
            }
            drawn.push(Drawn {
                handle: e.common().handle.value(),
                layer: normalize_name(&e.common().layer),
                paths,
                open,
                straight,
                lengths,
            });
        }
        let mut segs = Vec::new();
        for (owner, d) in drawn.iter().enumerate() {
            for path in &d.paths {
                for (a, b) in segments(path) {
                    segs.push(Seg { owner, a, b });
                }
            }
        }
        let grid = Grid::new(segs, unit(NEAR_M).max(tol * 10.0));
        let mut found: Vec<Finding> = Vec::new();
        let mut push = |f: Finding| found.push(f);
        let fmt_m = |d: f64| format!("{:.3} m", d * to_m);

        // Line work.
        if wanted("duplicate") || wanted("overlap") {
            self.check_repeats(&drawn, &grid, unit(TOUCH_M), &mut push);
        }
        if wanted("dangling_end") || wanted("overshoot") {
            for (i, d) in drawn.iter().enumerate() {
                if !d.open {
                    continue;
                }
                for path in &d.paths {
                    let n = path.points.len();
                    if n < 2 {
                        continue;
                    }
                    for (end, prev) in [
                        (path.points[0], path.points[1]),
                        (path.points[n - 1], path.points[n - 2]),
                    ] {
                        end_findings(
                            &grid,
                            i,
                            end,
                            prev,
                            &drawn,
                            unit(TOUCH_M),
                            unit(NEAR_M),
                            extension.filter(|_| {
                                dimension_ends
                                    .iter()
                                    .any(|&p| dist(p, end) <= unit(TOUCH_M))
                            }),
                            &fmt_m,
                            &mut push,
                        );
                    }
                }
            }
        }
        if wanted("tiny_segment") {
            for d in &drawn {
                let tiny = d
                    .lengths
                    .iter()
                    .copied()
                    .find(|(_, len)| *len < unit(TINY_M));
                if let Some((a, len)) = tiny {
                    push(Finding {
                        rule: "tiny_segment",
                        pack: "general",
                        severity: Severity::Warning,
                        handles: vec![d.handle],
                        at: Some(a),
                        message: if len <= tol {
                            "a segment of no length".to_string()
                        } else {
                            format!("a segment only {} long", fmt_m(len))
                        },
                    });
                }
            }
        }
        if wanted("missing_center_mark") {
            missing_center_marks(&entities, doc, &drawn, &grid, unit(TOUCH_M), &mut push);
        }

        // Text.
        let xrefs = self.xref_docs();
        let texts: Vec<_> = entities
            .iter()
            .filter(|e| matches!(e, EntityType::Text(_) | EntityType::MText(_)))
            .take(MAX_TEXTS)
            .map(|e| {
                self.entity_row(
                    &xrefs,
                    crate::query::Candidate {
                        entity: e,
                        doc,
                        layer: std::borrow::Cow::Borrowed(&e.common().layer),
                        xf: Xform::identity(),
                        chain: Vec::new(),
                        xref: None,
                    },
                )
            })
            .collect();
        if wanted("literal_newline") {
            for e in &entities {
                let raw = match e {
                    EntityType::Text(t) => &t.value,
                    EntityType::MText(t) => &t.value,
                    _ => continue,
                };
                if raw.contains("\\n") {
                    push(Finding {
                        rule: "literal_newline",
                        pack: "general",
                        severity: Severity::Error,
                        handles: vec![e.common().handle.value()],
                        at: None,
                        message: "the text shows \\n as two characters instead of a new line"
                            .into(),
                    });
                }
            }
        }
        if wanted("text_overlaps_text") {
            for (i, a) in texts.iter().enumerate() {
                for b in &texts[i + 1..] {
                    let (ia, ib) = (area(a.bounds), area(b.bounds));
                    let shared = area_of(intersect(a.bounds, b.bounds));
                    if shared > 0.1 * ia.min(ib) && ia > 0.0 && ib > 0.0 {
                        push(Finding {
                            rule: "text_overlaps_text",
                            pack: "general",
                            severity: Severity::Warning,
                            handles: vec![a.handle, b.handle],
                            at: Some(centre(intersect(a.bounds, b.bounds).unwrap_or(a.bounds))),
                            message: format!(
                                "\"{}\" and \"{}\" overlap",
                                clip(a.text.as_deref()),
                                clip(b.text.as_deref())
                            ),
                        });
                    }
                }
            }
        }
        if wanted("text_over_lines") {
            for t in &texts {
                let Some(entity) = doc.get_entity(opencadcodec::Handle::new(t.handle)) else {
                    continue;
                };
                let paths = qiyas_geom::flatten(doc, entity, &Xform::identity(), tol).into_paths();
                let Some(outline) = paths.first().filter(|p| p.points.len() >= 4) else {
                    continue;
                };
                let crossing = grid.within(t.bounds).into_iter().find(|&s| {
                    let s = grid.segs[s];
                    segment_hits_text(s.a, s.b, &outline.points)
                });
                if let Some(s) = crossing {
                    push(Finding {
                        rule: "text_over_lines",
                        pack: "general",
                        severity: Severity::Warning,
                        handles: vec![t.handle, drawn[grid.segs[s].owner].handle],
                        at: Some(centre(t.bounds)),
                        message: format!("a line runs through \"{}\"", clip(t.text.as_deref())),
                    });
                }
            }
        }
        if wanted("text_height_vs_scale") {
            let (scale_name, _) = self.annotation_scale();
            for e in &entities {
                let height = match e {
                    EntityType::Text(t) => t.height,
                    EntityType::MText(t) => t.height,
                    _ => continue,
                };
                let paper_mm = height * to_m * 1000.0 / paper_scale;
                if paper_mm > 0.0 && !(PAPER_MIN_MM..=PAPER_MAX_MM).contains(&paper_mm) {
                    let small = paper_mm < PAPER_MIN_MM;
                    push(Finding {
                        rule: "text_height_vs_scale",
                        pack: "general",
                        severity: Severity::Warning,
                        handles: vec![e.common().handle.value()],
                        at: None,
                        message: format!(
                            "text {} on paper at {scale_name}: {}",
                            format_args!("{paper_mm:.1} mm"),
                            if small {
                                "too small to read"
                            } else {
                                "far larger than notes and labels"
                            }
                        ),
                    });
                }
            }
        }

        if wanted("room_area_mismatch") {
            self.check_room_areas(&sheet, &mut push);
        }

        // Rooms.
        if wanted("room_open") || wanted("room_size_text_differs") {
            self.check_rooms(&texts, &sheet, to_m, &mut push);
        }

        // Dimensions.
        if wanted("dim_override_mismatch") {
            for row in self.read_dimensions(Some(&sheet)) {
                let (Some(text), Some(measured)) = (&row.text_override, row.measurement) else {
                    continue;
                };
                if row.kind.contains("Angular") || text.contains("<>") {
                    continue;
                }
                let Some((written, unit)) = dimension_number(text) else {
                    continue;
                };
                let measured_m = measured * to_m;
                let agrees_in = |factor: f64| {
                    (written * factor - measured_m).abs() <= 0.01 * measured_m.abs() + DIM_TOL_M
                };
                // A stated unit is binding. With none, allow drawing units or a metric size.
                let agrees = unit.map_or_else(
                    || {
                        [
                            to_m,
                            Units::Metres.to_metres(),
                            Units::Centimetres.to_metres(),
                            Units::Millimetres.to_metres(),
                        ]
                        .into_iter()
                        .any(agrees_in)
                    },
                    agrees_in,
                );
                if !agrees {
                    push(Finding {
                        rule: "dim_override_mismatch",
                        pack: "general",
                        severity: Severity::Warning,
                        handles: vec![row.handle],
                        at: Some(row.at),
                        message: format!(
                            "the dimension says \"{text}\" but measures {}",
                            fmt_m(measured)
                        ),
                    });
                }
            }
        }

        // The drawing as a whole.
        if wanted("far_off_extents") {
            far_off(&entities, doc, tol, &mut push);
        }
        if wanted("unlinked_takeoff") {
            for m in self
                .takeoff
                .items
                .iter()
                .filter(|m| m.sheet.eq_ignore_ascii_case(&sheet))
            {
                let gone: Vec<u64> = m
                    .handles
                    .iter()
                    .copied()
                    .filter(|h| crate::session::locate(doc, &xrefs, *h, &m.chain).is_err())
                    .collect();
                if !gone.is_empty() {
                    push(Finding {
                        rule: "unlinked_takeoff",
                        pack: "construction",
                        severity: Severity::Warning,
                        handles: gone,
                        at: None,
                        message: format!(
                            "take-off row {} ({}) measures an entity that is gone",
                            m.id, m.description
                        ),
                    });
                }
            }
        }
        if wanted("layer_zero") || wanted("colour_not_bylayer") {
            // Layer 0 is right inside a block definition (its contents take the layer of the
            // reference), so only entities drawn on a sheet itself count.
            let on_sheet = self
                .drawing
                .sheets()
                .iter()
                .any(|s| s.block.eq_ignore_ascii_case(&sheet));
            let mut on_zero = Vec::new();
            let mut coloured = Vec::new();
            for e in &entities {
                let c = e.common();
                if matches!(e, EntityType::Viewport(_)) {
                    continue;
                }
                if on_sheet && c.layer == "0" {
                    on_zero.push(c.handle.value());
                }
                if !matches!(c.color, Color::ByLayer | Color::ByBlock) {
                    coloured.push(c.handle.value());
                }
            }
            if wanted("layer_zero") && !on_zero.is_empty() {
                push(Finding {
                    rule: "layer_zero",
                    pack: "general",
                    severity: Severity::Warning,
                    message: format!(
                        "{} entities on layer 0: put them on layers named for what they are",
                        on_zero.len()
                    ),
                    handles: on_zero,
                    at: None,
                });
            }
            if wanted("colour_not_bylayer") && !coloured.is_empty() {
                push(Finding {
                    rule: "colour_not_bylayer",
                    pack: "general",
                    severity: Severity::Info,
                    message: format!(
                        "{} entities have a colour of their own instead of their layer's",
                        coloured.len()
                    ),
                    handles: coloured,
                    at: None,
                });
            }
        }

        // Only what the scope asks about, errors first.
        found.retain(|f| wanted(f.rule));
        if !scope.handles.is_empty() {
            let keep: HashSet<u64> = scope.handles.iter().copied().collect();
            found.retain(|f| f.handles.iter().any(|h| keep.contains(h)));
        }
        let rank = |rule: &str| RULES.iter().position(|r| *r == rule).unwrap_or(RULES.len());
        found.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then(rank(a.rule).cmp(&rank(b.rule)))
        });
        let mut counts = BTreeMap::new();
        for f in &found {
            *counts.entry(f.rule.to_string()).or_insert(0) += 1;
        }
        let errors = found
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count();
        let warnings = found
            .iter()
            .filter(|f| f.severity == Severity::Warning)
            .count();
        found.truncate(limit);
        let line = check_line(errors, warnings, found.len(), &counts);
        CheckReport {
            sheet,
            entities: entities.len(),
            findings: found,
            counts,
            errors,
            warnings,
            line,
        }
    }

    /// Exact repeats and collinear overlaps require the same layer, a matching property in
    /// AutoCAD OVERKILL by default. Coincidence across layers is intentional: grid axes on wall
    /// centrelines, hatch boundaries on outline layers, or viewport frames on title lines.
    fn check_repeats(
        &self,
        drawn: &[Drawn],
        grid: &Grid,
        touch: f64,
        push: &mut impl FnMut(Finding),
    ) {
        // Repeats: the same rounded points, in either direction.
        let key = |d: &Drawn| {
            let q = |v: f64| (v / touch).round() as i64;
            let mut paths = Vec::new();
            for path in &d.paths {
                let mut pts: Vec<_> = path.points.iter().map(|p| (q(p[0]), q(p[1]))).collect();
                if path.closed && pts.first() == pts.last() {
                    pts.pop();
                }
                let mut rev = pts.clone();
                rev.reverse();
                if path.closed {
                    for points in [&mut pts, &mut rev] {
                        if let Some((first, _)) = points.iter().enumerate().min_by_key(|(_, p)| **p)
                        {
                            points.rotate_left(first);
                        }
                    }
                }
                paths.push((path.closed, pts.min(rev)));
            }
            paths.sort();
            paths
        };
        let mut seen = HashMap::new();
        let mut repeated: HashMap<u64, u64> = HashMap::new();
        for d in drawn {
            let paths = key(d);
            if paths.is_empty() {
                continue;
            }
            let k = (&d.layer, paths);
            if let Some(&first) = seen.get(&k) {
                repeated.insert(d.handle, first);
                repeated.insert(first, first);
                push(Finding {
                    rule: "duplicate",
                    pack: "general",
                    severity: Severity::Error,
                    handles: vec![first, d.handle],
                    at: d.paths.first().and_then(|p| p.points.first().copied()),
                    message: "drawn twice in the same place".into(),
                });
            } else {
                seen.insert(k, d.handle);
            }
        }
        // Overlaps require actual straight CAD edges. A tangent to a displayed circle can
        // lie within tolerance of its sampled chords without sharing any of the curve.
        let straight = drawn
            .iter()
            .enumerate()
            .flat_map(|(owner, d)| d.straight.iter().map(move |&(a, b)| Seg { owner, a, b }))
            .collect();
        let grid = Grid::new(straight, grid.cell);
        // Overlaps: collinear segments of two entities sharing a stretch.
        let mut pairs: HashSet<(usize, usize)> = HashSet::new();
        for (i, s) in grid.segs.iter().enumerate() {
            let len = dist(s.a, s.b);
            if len <= touch {
                continue;
            }
            for j in grid.within(bounds_of(&[s.a, s.b])) {
                if j <= i {
                    continue;
                }
                let t = grid.segs[j];
                if t.owner == s.owner || drawn[s.owner].layer != drawn[t.owner].layer {
                    continue;
                }
                let (h1, h2) = (drawn[s.owner].handle, drawn[t.owner].handle);
                if repeated
                    .get(&h1)
                    .is_some_and(|first| repeated.get(&h2) == Some(first))
                {
                    continue;
                }
                let key = (s.owner.min(t.owner), s.owner.max(t.owner));
                if pairs.contains(&key) {
                    continue;
                }
                if let Some(shared) = collinear_overlap(*s, t, touch) {
                    if shared > touch * 10.0 {
                        pairs.insert(key);
                        push(Finding {
                            rule: "overlap",
                            pack: "general",
                            severity: Severity::Warning,
                            handles: vec![h1, h2],
                            at: Some(midpoint(t.a, t.b)),
                            message: format!(
                                "two entities lie on each other for {:.3} drawing units",
                                shared
                            ),
                        });
                    }
                }
            }
        }
    }

    /// Room names and size labels: each traced to the space round it.
    fn check_rooms(
        &self,
        texts: &[crate::EntityRow],
        sheet: &str,
        to_m: f64,
        push: &mut impl FnMut(Finding),
    ) {
        let roomish = |r: &crate::EntityRow| {
            let layer = r.layer.to_lowercase();
            let text = r.text.as_deref().unwrap_or("");
            !layer.starts_with("qiyas")
                && !crate::rooms::looks_like_a_label(text)
                && (["room", "space", "name", "area"]
                    .iter()
                    .any(|w| layer.contains(w))
                    || size_label(text).is_some())
        };
        let start = 3.0 / to_m;
        for row in texts.iter().filter(|r| roomish(r)).take(MAX_ROOMS) {
            let text = row.text.as_deref().unwrap_or("");
            let at = centre(row.bounds);
            match self.boundary_at_in(sheet, at, start) {
                Boundary::Ring { ring, .. } => {
                    let Some((w, h)) = size_label(text) else {
                        continue;
                    };
                    let b = bounds_of(&ring);
                    let (bw, bh) = ((b[1][0] - b[0][0]) * to_m, (b[1][1] - b[0][1]) * to_m);
                    let (lo, hi) = (w.min(h), w.max(h));
                    let (blo, bhi) = (bw.min(bh), bw.max(bh));
                    let off = |a: f64, b: f64| (a - b).abs() > SIZE_SLACK * b.max(1e-9);
                    if off(lo, blo) || off(hi, bhi) {
                        push(Finding {
                            rule: "room_size_text_differs",
                            pack: "construction",
                            severity: Severity::Warning,
                            handles: vec![row.handle],
                            at: Some(at),
                            message: format!(
                                "the label says {w:.2} × {h:.2} m but the room drawn is {bw:.2} × {bh:.2} m"
                            ),
                        });
                    }
                }
                Boundary::None => push(Finding {
                    rule: "room_open",
                    pack: "construction",
                    severity: Severity::Warning,
                    handles: vec![row.handle],
                    at: Some(at),
                    message: format!(
                        "\"{}\" sits in no closed space: its walls have a gap",
                        clip(Some(text))
                    ),
                }),
                Boundary::TooDense => {}
            }
        }
    }
}

/// A CENTER line through the centre also serves as a centre mark. Resolve ByLayer so imported
/// drawings and hand-drawn marks work like the drafting commands.
fn missing_center_marks(
    entities: &[&EntityType],
    doc: &opencadcodec::CadDocument,
    drawn: &[Drawn],
    grid: &Grid,
    touch: f64,
    push: &mut impl FnMut(Finding),
) {
    // Labels may be on a different layer from the bubble that surrounds them.
    let labels: Vec<_> = doc
        .entities()
        .filter_map(|entity| {
            let point = match entity {
                EntityType::Text(text) if !text.value.trim().is_empty() => text.insertion_point,
                EntityType::MText(text) if !text.value.trim().is_empty() => text.insertion_point,
                _ => return None,
            };
            Some((entity.common().owner_handle, [point.x, point.y]))
        })
        .collect();
    let marked: HashSet<u64> = entities
        .iter()
        .filter(|e| {
            if !matches!(
                e,
                EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Polyline2D(_)
            ) {
                return false;
            }
            let common = e.common();
            let linetype =
                if common.linetype.is_empty() || common.linetype.eq_ignore_ascii_case("ByLayer") {
                    doc.layers
                        .get(&common.layer)
                        .map(|layer| layer.line_type.as_str())
                        .unwrap_or("")
                } else {
                    &common.linetype
                };
            ["CENTER", "CENTER2", "CENTERX2"]
                .iter()
                .any(|name| linetype.eq_ignore_ascii_case(name))
        })
        .map(|e| e.common().handle.value())
        .collect();
    for e in entities {
        let (c, normal, radius) = match e {
            EntityType::Circle(c) => (c.center, c.normal, c.radius),
            EntityType::Arc(a)
                if (a.end_angle - a.start_angle)
                    .rem_euclid(std::f64::consts::TAU)
                    .abs()
                    < 1e-9 =>
            {
                (a.center, a.normal, a.radius)
            }
            _ => continue,
        };
        let at = qiyas_geom::object_to_world(&normal, c.z).apply([c.x, c.y]);
        if e.common()
            .extended_data
            .get_record(crate::conventions::GRID_APP)
            .is_some()
            || labels.iter().any(|(owner, point)| {
                *owner == e.common().owner_handle && dist(at, *point) <= radius + touch
            })
        {
            continue;
        }
        let has_mark = grid.near(at).into_iter().any(|i| {
            let seg = grid.segs[i];
            marked.contains(&drawn[seg.owner].handle)
                && dist(seg.a, seg.b) > touch
                && point_segment(at, seg.a, seg.b) <= touch
        });
        if !has_mark {
            push(Finding {
                rule: "missing_center_mark",
                pack: "general",
                severity: Severity::Info,
                handles: vec![e.common().handle.value()],
                at: Some(at),
                message: "a circle or full arc has no centre mark or centre line".into(),
            });
        }
    }
}

/// An end that stops short of a line, or runs a little past one it crosses.
#[allow(clippy::too_many_arguments)]
fn end_findings(
    grid: &Grid,
    own: usize,
    end: [f64; 2],
    prev: [f64; 2],
    drawn: &[Drawn],
    touch: f64,
    near: f64,
    dimension_extension: Option<f64>,
    fmt_m: &dyn Fn(f64) -> String,
    push: &mut impl FnMut(Finding),
) {
    let mut nearest: Option<(f64, usize)> = None;
    let mut touching = false;
    let mut over: Option<(f64, usize, [f64; 2])> = None;
    let length = dist(prev, end);
    if length == 0.0 {
        return;
    }
    let ahead = [
        end[0] + (end[0] - prev[0]) * near / length,
        end[1] + (end[1] - prev[1]) * near / length,
    ];
    for j in grid.near(end) {
        let s = grid.segs[j];
        if s.owner == own {
            continue;
        }
        let d = point_segment(end, s.a, s.b);
        if d <= touch {
            touching = true;
            break;
        }
        // A gap is along the endpoint's continuation, not merely beside a nearby line.
        // Collinear facing endpoints also meet, although `crossing` excludes parallel lines.
        let meeting = crossing(end, ahead, s.a, s.b).or_else(|| {
            [s.a, s.b]
                .into_iter()
                .filter(|&p| point_segment(p, end, ahead) <= touch)
                .min_by(|a, b| dist(end, *a).total_cmp(&dist(end, *b)))
        });
        if let Some(meeting) = meeting {
            let gap = dist(end, meeting);
            if gap > touch && nearest.is_none_or(|(best, _)| gap < best) {
                nearest = Some((gap, s.owner));
            }
        }
        if let Some(c) = crossing(prev, end, s.a, s.b) {
            let past = dist(c, end);
            if past > touch && past < near && over.is_none_or(|(p, _, _)| past < p) {
                over = Some((past, s.owner, c));
            }
        }
    }
    if touching {
        return;
    }
    if let Some((past, other, c)) = over {
        // A dimension's exact generated endpoint is allowed the style's DIMEXE extension.
        // A moved endpoint or any different overrun still gets the ordinary finding.
        if dimension_extension.is_some_and(|extension| (past - extension).abs() <= touch) {
            return;
        }
        push(Finding {
            rule: "overshoot",
            pack: "general",
            severity: Severity::Warning,
            handles: vec![drawn[own].handle, drawn[other].handle],
            at: Some(c),
            message: format!("runs {} past the line it meets", fmt_m(past)),
        });
    } else if let Some((d, other)) = nearest.filter(|(d, _)| *d < near) {
        push(Finding {
            rule: "dangling_end",
            pack: "general",
            severity: Severity::Warning,
            handles: vec![drawn[own].handle, drawn[other].handle],
            at: Some(end),
            message: format!("stops {} short of the line next to it", fmt_m(d)),
        });
    }
}

/// Entities whose middle lies far outside where most of the drawing is.
fn far_off(
    entities: &[&EntityType],
    doc: &opencadcodec::CadDocument,
    tol: f64,
    push: &mut impl FnMut(Finding),
) {
    let mut centres: Vec<(u64, [f64; 2])> = Vec::new();
    for e in entities.iter().take(20_000) {
        if matches!(
            e,
            EntityType::Viewport(_) | EntityType::Ray(_) | EntityType::XLine(_)
        ) {
            continue;
        }
        let paths =
            qiyas_geom::flatten(doc, e, &Xform::identity(), tol.max(1e-9) * 100.0).into_paths();
        let pts: Vec<[f64; 2]> = paths
            .iter()
            .flat_map(|p| p.points.iter().copied())
            .collect();
        if pts.is_empty() {
            continue;
        }
        centres.push((e.common().handle.value(), centre(bounds_of(&pts))));
    }
    if centres.len() < 10 {
        return;
    }
    let pick = |axis: usize, q: f64| {
        let mut v: Vec<f64> = centres.iter().map(|c| c.1[axis]).collect();
        v.sort_by(f64::total_cmp);
        v[((v.len() - 1) as f64 * q) as usize]
    };
    let core = [
        [pick(0, 0.05), pick(1, 0.05)],
        [pick(0, 0.95), pick(1, 0.95)],
    ];
    let size = dist(core[0], core[1]).max(tol * 1e6);
    for (h, c) in centres {
        let outside = [
            core[0][0] - c[0],
            c[0] - core[1][0],
            core[0][1] - c[1],
            c[1] - core[1][1],
        ]
        .into_iter()
        .fold(0.0_f64, f64::max);
        if outside > 3.0 * size {
            push(Finding {
                rule: "far_off_extents",
                pack: "general",
                severity: Severity::Warning,
                handles: vec![h],
                at: Some(c),
                message:
                    "far from the rest of the drawing (a stray entity makes zoom extents useless)"
                        .into(),
            });
        }
    }
}

/// "3 errors, 5 warnings: 2 dangling_end, 1 duplicate, …" (under 200 characters).
fn check_line(
    errors: usize,
    warnings: usize,
    listed: usize,
    counts: &BTreeMap<String, usize>,
) -> String {
    if counts.is_empty() {
        return "No findings".to_string();
    }
    let mut parts: Vec<(usize, &String)> = counts.iter().map(|(k, v)| (*v, k)).collect();
    parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    let mut line = format!("{errors} errors, {warnings} warnings:");
    for (n, rule) in parts {
        let next = format!(" {n} {rule},");
        if line.len() + next.len() > 180 {
            line.push_str(" …");
            break;
        }
        line.push_str(&next);
    }
    if line.ends_with(',') {
        line.pop();
    }
    let total: usize = counts.values().sum();
    if listed < total {
        line.push_str(&format!(" ({listed} listed)"));
    }
    line
}

/// "5.25m x 3.25m", "5.25 x 3.25", "525 x 325 cm", "5250x3250 mm" (and ×, X): the two sizes in
/// metres. Numbers past 100 with no unit are taken as centimetres past 100 and millimetres past
/// 1000.
pub(crate) fn size_label(text: &str) -> Option<(f64, f64)> {
    fn number(s: &str) -> Option<(f64, Option<f64>)> {
        let s = s.trim();
        let digits: String = s
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let rest = s[digits.len()..].trim();
        let value: f64 = digits.parse().ok()?;
        let unit = match rest {
            "mm" => Some(0.001),
            "cm" => Some(0.01),
            "m" => Some(1.0),
            "" => None,
            _ => return None,
        };
        Some((value, unit))
    }

    let lower = text.to_lowercase().replace(['×', '*'], "x");
    for line in lower.lines() {
        for (at, _) in line.match_indices('x') {
            let (left, right) = (&line[..at], &line[at + 1..]);
            // A room name may come before the sizes, including an x of its own.
            let mut words = left.split_whitespace().rev();
            let Some(last) = words.next() else { continue };
            let first = if matches!(last, "mm" | "cm" | "m") {
                let Some(value) = words.next() else { continue };
                format!("{value} {last}")
            } else {
                last.to_string()
            };
            let (Some((a, ua)), Some((b, ub))) = (number(&first), number(right)) else {
                continue;
            };
            let unit = ub.or(ua).unwrap_or(if a.max(b) > 1000.0 {
                0.001
            } else if a.max(b) > 100.0 {
                0.01
            } else {
                1.0
            });
            let (ua, ub) = (ua.unwrap_or(unit), ub.unwrap_or(unit));
            if a > 0.0 && b > 0.0 {
                return Some((a * ua, b * ub));
            }
        }
    }
    None
}

/// The first number of a dimension override and its stated unit, when it has one.
fn dimension_number(text: &str) -> Option<(f64, Option<f64>)> {
    let at = text.find(|c: char| c.is_ascii_digit())?;
    let value = &text[at..];
    let digits: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let suffix = value[digits.len()..].trim_start().to_ascii_lowercase();
    let word = suffix
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    let unit = match word {
        "mm" => Some(Units::Millimetres),
        "cm" => Some(Units::Centimetres),
        "m" => Some(Units::Metres),
        "in" => Some(Units::Inches),
        "ft" => Some(Units::Feet),
        _ => None,
    };
    Some((digits.parse().ok()?, unit.map(|u| u.to_metres())))
}

fn clip(text: Option<&str>) -> String {
    let t = text.unwrap_or("").replace('\n', " ");
    if t.chars().count() > 30 {
        format!("{}…", t.chars().take(30).collect::<String>())
    } else {
        t
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn point_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

/// Where the segment `pq` crosses the segment `ab`, if it does.
fn crossing(p: [f64; 2], q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Option<[f64; 2]> {
    let r = [q[0] - p[0], q[1] - p[1]];
    let s = [b[0] - a[0], b[1] - a[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-15 {
        return None;
    }
    let t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / den;
    let u = ((a[0] - p[0]) * r[1] - (a[1] - p[1]) * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then(|| [p[0] + t * r[0], p[1] + t * r[1]])
}

/// How long two collinear segments share, when they are collinear within `tol`.
fn collinear_overlap(s: Seg, t: Seg, tol: f64) -> Option<f64> {
    let len = dist(s.a, s.b);
    if point_segment_line(t.a, s.a, s.b) > tol || point_segment_line(t.b, s.a, s.b) > tol {
        return None;
    }
    let dir = [(s.b[0] - s.a[0]) / len, (s.b[1] - s.a[1]) / len];
    let proj = |p: [f64; 2]| (p[0] - s.a[0]) * dir[0] + (p[1] - s.a[1]) * dir[1];
    let (t0, t1) = (proj(t.a).min(proj(t.b)), proj(t.a).max(proj(t.b)));
    let shared = t1.min(len) - t0.max(0.0);
    (shared > 0.0).then_some(shared)
}

/// Distance from `p` to the infinite line through `a` and `b`.
fn point_segment_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len == 0.0 {
        return dist(p, a);
    }
    ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / len
}

fn bounds_of(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        b[0][0] = b[0][0].min(p[0]);
        b[0][1] = b[0][1].min(p[1]);
        b[1][0] = b[1][0].max(p[0]);
        b[1][1] = b[1][1].max(p[1]);
    }
    b
}

fn centre(b: [[f64; 2]; 2]) -> [f64; 2] {
    midpoint(b[0], b[1])
}

fn area(b: [[f64; 2]; 2]) -> f64 {
    (b[1][0] - b[0][0]).max(0.0) * (b[1][1] - b[0][1]).max(0.0)
}

fn intersect(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> Option<[[f64; 2]; 2]> {
    let lo = [a[0][0].max(b[0][0]), a[0][1].max(b[0][1])];
    let hi = [a[1][0].min(b[1][0]), a[1][1].min(b[1][1])];
    (lo[0] < hi[0] && lo[1] < hi[1]).then_some([lo, hi])
}

fn area_of(b: Option<[[f64; 2]; 2]>) -> f64 {
    b.map_or(0.0, area)
}

/// Work in the text's own rotated box. Its axis-aligned bounds include empty corners, which
/// would falsely collide with a dimension line parallel to the text's baseline.
fn segment_hits_text(a: [f64; 2], b: [f64; 2], corners: &[[f64; 2]]) -> bool {
    let origin = corners[0];
    let x = [corners[1][0] - origin[0], corners[1][1] - origin[1]];
    let y = [corners[3][0] - origin[0], corners[3][1] - origin[1]];
    let determinant = x[0] * y[1] - x[1] * y[0];
    if determinant.abs() <= f64::MIN_POSITIVE {
        return false;
    }
    let local = |p: [f64; 2]| {
        let p = [p[0] - origin[0], p[1] - origin[1]];
        [
            (p[0] * y[1] - p[1] * y[0]) / determinant,
            (x[0] * p[1] - x[1] * p[0]) / determinant,
        ]
    };
    segment_hits_box(local(a), local(b), [[0.2, 0.2], [0.8, 0.8]])
}

/// Whether the segment crosses into the box.
fn segment_hits_box(a: [f64; 2], b: [f64; 2], bx: [[f64; 2]; 2]) -> bool {
    let inside =
        |p: [f64; 2]| p[0] > bx[0][0] && p[0] < bx[1][0] && p[1] > bx[0][1] && p[1] < bx[1][1];
    if inside(a) || inside(b) {
        return true;
    }
    let corners = [bx[0], [bx[1][0], bx[0][1]], bx[1], [bx[0][0], bx[1][1]]];
    (0..4).any(|i| crossing(a, b, corners[i], corners[(i + 1) % 4]).is_some())
}

/// The straight segments of a flattened path, including its closing edge.
fn segments(path: &qiyas_geom::Path) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    let count = if path.closed && path.points.len() > 2 {
        path.points.len()
    } else {
        path.points.len().saturating_sub(1)
    };
    (0..count).map(|i| (path.points[i], path.points[(i + 1) % path.points.len()]))
}
