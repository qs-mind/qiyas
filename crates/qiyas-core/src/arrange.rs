//! Whole drawing parts share one grouping for arrangement and overlap checks.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ink::{self, Bounds, SheetInk};
use crate::sheet_setup::{SheetGroup, SheetRole, SheetZones, GROUP_APP};
use crate::{CoreError, ModifySpec, Session, Target};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Item {
    Handles {
        handles: Vec<u64>,
        #[serde(default)]
        label: Option<String>,
    },
    Window {
        bounds: Bounds,
        #[serde(default)]
        label: Option<String>,
    },
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Top,
    Center,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Arrange {
    /// Omitted: group plans, tables, notes and legends automatically. Explicit items keep order.
    pub items: Option<Vec<Item>>,
    pub sheet: Option<String>,
    /// Omitted: choose rows whose overall shape is closest to the paper's ratio.
    pub columns: Option<usize>,
    /// Space between parts in paper millimetres, at the sheet's annotation scale (default 15).
    pub gap_mm: Option<f64>,
    /// Top-left corner in drawing units. Omitted: keep the old top-left, or use paper margins.
    pub origin: Option<[f64; 2]>,
    pub align: Align,
}
#[derive(Debug, Clone, Serialize)]
pub struct Move {
    pub handles: Vec<u64>,
    pub label: String,
    /// Old and new top-left corners, in drawing units. Geometry is only translated.
    pub from: [f64; 2],
    pub to: [f64; 2],
}
#[derive(Debug, Clone, Serialize)]
pub struct Arrangement {
    pub sheet: String,
    pub moves: Vec<Move>,
    pub extents: Option<Bounds>,
    /// Remaining collisions or content that cannot fit its designated zone. No content is shrunk.
    pub issues: Vec<ArrangementIssue>,
}
#[derive(Debug, Clone, Serialize)]
pub struct ArrangementIssue {
    pub handles: Vec<u64>,
    pub code: String,
    pub message: String,
    pub message_ar: String,
}
#[derive(Clone)]
pub(crate) struct Part {
    pub handles: Vec<u64>,
    pub bounds: Bounds,
    pub label: String,
    pub title: bool,
    pub frame: bool,
    pub role: Option<SheetRole>,
}
fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}
fn contains(a: Bounds, b: Bounds) -> bool {
    (0..2).all(|i| a[0][i] <= b[0][i] && a[1][i] >= b[1][i])
}
pub(crate) fn overlaps(a: Bounds, b: Bounds) -> bool {
    (0..2).all(|i| a[0][i] < b[1][i] && b[0][i] < a[1][i])
}
fn area(b: Bounds) -> f64 {
    (b[1][0] - b[0][0]) * (b[1][1] - b[0][1])
}
fn mostly_inside(a: Bounds, b: Bounds) -> bool {
    if area(b) <= f64::EPSILON {
        return contains(a, b);
    }
    let intersection = [
        [a[0][0].max(b[0][0]), a[0][1].max(b[0][1])],
        [a[1][0].min(b[1][0]), a[1][1].min(b[1][1])],
    ];
    overlaps(a, b) && area(intersection) >= area(b) * 0.5
}
fn merge(a: &mut Part, b: Part) {
    a.bounds = ink::union([a.bounds, b.bounds].into_iter()).unwrap();
    a.handles.extend(b.handles);
}

impl Session {
    /// Ink uses the real rendered text bounds; viewport interiors count as occupied too.
    pub(crate) fn drawing_parts(&self, sheet: &str, ink: &SheetInk, tolerance: f64) -> Vec<Part> {
        let mut bounds = BTreeMap::<u64, Bounds>::new();
        for piece in &ink.pieces {
            bounds
                .entry(piece.owner)
                .and_modify(|b| {
                    *b = ink::union([*b, piece.bounds].into_iter()).unwrap();
                })
                .or_insert(piece.bounds);
        }
        let sheet_owner = self.drawing.doc.block_records.get(sheet).map(|b| b.handle);
        let paper_size = self
            .drawing
            .doc
            .objects
            .values()
            .find_map(|object| match object {
                opencadcodec::objects::ObjectType::Layout(layout)
                    if Some(layout.block_record) == sheet_owner =>
                {
                    let (mut w, mut h) = (layout.paper_width, layout.paper_height);
                    if matches!(layout.plot_rotation, 1 | 3) {
                        std::mem::swap(&mut w, &mut h);
                    }
                    Some([w, h])
                }
                _ => None,
            });
        let mut separate = Vec::new();
        let mut lines: Vec<Part> = Vec::new();
        let mut annotations = Vec::new();
        let mut legends: Vec<Part> = Vec::new();
        let mut title: Option<Part> = None;
        let mut groups: BTreeMap<u64, Part> = BTreeMap::new();
        for e in self.drawing.doc.entities_in_block(sheet) {
            let h = e.common().handle.value();
            let b = if let EntityType::Viewport(v) = e {
                if v.id == 1 || !v.status.is_on || v.width <= 0.0 || v.height <= 0.0 {
                    continue;
                }
                [
                    [v.center.x - v.width / 2.0, v.center.y - v.height / 2.0],
                    [v.center.x + v.width / 2.0, v.center.y + v.height / 2.0],
                ]
            } else if let Some(b) = bounds.get(&h) {
                *b
            } else {
                continue;
            };
            let layer = e.common().layer.to_ascii_uppercase();
            let block = match e {
                EntityType::Insert(i) => i.block_name.to_ascii_uppercase(),
                _ => String::new(),
            };
            let is_title =
                layer == "A-ANNO-TTLB" || block.contains("TITLE") || block.contains("TTLB");
            let is_frame = is_title
                && !sheet.eq_ignore_ascii_case("*Model_Space")
                && matches!(e, EntityType::LwPolyline(p) if p.is_closed)
                && paper_size.is_some_and(|size| {
                    size.iter().all(|v| *v > 0.0)
                        && (0..2).all(|i| b[1][i] - b[0][i] >= size[i] * 0.8)
                });
            let label = if is_frame {
                "Sheet frame"
            } else if is_title {
                "Title block"
            } else if matches!(e, EntityType::Table(_)) {
                "Table"
            } else if matches!(e, EntityType::Viewport(_)) {
                "Viewport"
            } else if block.contains("LEGEND") || layer.contains("LEGEND") {
                "Legend"
            } else if matches!(e, EntityType::Insert(_)) {
                "Block"
            } else if matches!(e, EntityType::Text(_) | EntityType::MText(_)) {
                "Notes"
            } else if matches!(e, EntityType::Dimension(_)) {
                "Dimensions"
            } else if matches!(e, EntityType::Leader(_) | EntityType::MultiLeader(_)) {
                "Notes"
            } else {
                "Plan"
            };
            let part = Part {
                handles: vec![h],
                bounds: b,
                label: label.into(),
                title: is_title,
                frame: is_frame,
                role: None,
            };
            if let Some(group) = crate::xdata::read_json::<SheetGroup>(e.common(), GROUP_APP) {
                let mut part = part;
                part.role = Some(group.role);
                part.label = match group.role {
                    SheetRole::Plans => "Viewport",
                    SheetRole::Notes => "Notes",
                    SheetRole::KeyPlan => "Key plan / المخطط المفتاحي",
                    SheetRole::Furniture => "Scale bar / المقياس الخطي",
                }
                .into();
                if let Some(existing) = groups.get_mut(&group.id) {
                    merge(existing, part);
                } else {
                    groups.insert(group.id, part);
                }
                continue;
            }
            if is_frame {
                separate.push(part);
            } else if is_title {
                if matches!(e, EntityType::Insert(_)) || sheet.eq_ignore_ascii_case("*Model_Space")
                {
                    separate.push(part);
                } else if let Some(t) = &mut title {
                    merge(t, part);
                } else {
                    title = Some(part);
                }
            } else if layer.contains("LEGEND")
                && !matches!(e, EntityType::Insert(_) | EntityType::Table(_))
            {
                // A legend drawn as loose symbols and text still moves as one unit.
                let mut part = part;
                let mut i = 0;
                while i < legends.len() {
                    if ink::intersects(ink::expand(part.bounds, tolerance), legends[i].bounds) {
                        merge(&mut part, legends.remove(i));
                        i = 0;
                    } else {
                        i += 1;
                    }
                }
                legends.push(part);
            } else if matches!(e, EntityType::Table(_) | EntityType::Viewport(_))
                || block.contains("LEGEND")
                || layer.contains("LEGEND")
                || matches!(e, EntityType::MText(t) if t.value.contains("\\P") || t.value.contains('\n')
                    || ink.pieces.iter().any(|p| p.owner == h && p.text.is_some()
                        && p.height > 0.0 && p.bounds[1][1] - p.bounds[0][1] > p.height * 1.8))
            {
                separate.push(part);
            } else if matches!(
                e,
                EntityType::Text(_)
                    | EntityType::MText(_)
                    | EntityType::Dimension(_)
                    | EntityType::Leader(_)
                    | EntityType::MultiLeader(_)
            ) {
                annotations.push(part);
            } else {
                lines.push(part);
            }
        }
        // Union nearby line-work extents transitively. Tables and note blocks never bridge plans.
        let mut clusters: Vec<Part> = Vec::new();
        lines.sort_by(|a, b| area(b.bounds).total_cmp(&area(a.bounds)));
        for mut part in lines {
            let mut i = 0;
            while i < clusters.len() {
                if ink::intersects(ink::expand(part.bounds, tolerance), clusters[i].bounds) {
                    merge(&mut part, clusters.remove(i));
                    i = 0;
                } else {
                    i += 1;
                }
            }
            clusters.push(part);
        }
        for part in annotations {
            let cluster = clusters
                .iter()
                .enumerate()
                .filter(|(_, c)| mostly_inside(ink::expand(c.bounds, tolerance), part.bounds))
                .min_by(|(_, a), (_, b)| area(a.bounds).total_cmp(&area(b.bounds)))
                .map(|(i, _)| i);
            // A dimension or label that reaches past its plan's edge still belongs to that plan.
            let cluster = cluster.or_else(|| {
                clusters
                    .iter()
                    .position(|c| ink::intersects(ink::expand(c.bounds, tolerance), part.bounds))
            });
            if let Some(i) = cluster {
                merge(&mut clusters[i], part);
            } else if let Some(near) = separate.iter_mut().find(|p| {
                // Loose notes and dimensions that touch each other move as one group.
                p.label == part.label
                    && ink::intersects(ink::expand(p.bounds, tolerance), part.bounds)
            }) {
                merge(near, part);
            } else {
                separate.push(part);
            }
        }
        separate.extend(groups.into_values());
        separate.extend(clusters);
        separate.extend(legends);
        separate.extend(title);
        separate.sort_by(|a, b| {
            area(b.bounds)
                .total_cmp(&area(a.bounds))
                .then(a.handles[0].cmp(&b.handles[0]))
        });
        separate
    }

    pub(crate) fn arrange(&mut self, spec: Arrange) -> Result<Arrangement, CoreError> {
        let sheet = self.sheet_block(spec.sheet.as_deref());
        if !self.drawing.doc.block_records.contains(&sheet) {
            return Err(invalid("The drawing has no such sheet"));
        }
        let mm = spec.gap_mm.unwrap_or(15.0);
        if !mm.is_finite()
            || mm < 0.0
            || spec.columns == Some(0)
            || spec
                .origin
                .is_some_and(|p| !p.iter().all(|v| v.is_finite()))
        {
            return Err(invalid(
                "Use a non-negative gap, at least one column and finite coordinates",
            ));
        }
        let (to_m, scale) = self.check_space(&sheet);
        let gap = mm * 0.001 * scale / to_m;
        if !gap.is_finite() {
            return Err(invalid("The paper gap is too large for this scale"));
        }
        let ink = self.sheet_ink(&sheet);
        let automatic = self.drawing_parts(&sheet, &ink, gap / 2.0);
        let paper = !sheet.eq_ignore_ascii_case("*Model_Space");
        let fixed: Vec<_> = automatic
            .iter()
            .filter(|p| {
                paper
                    && (p.title
                        || p.role == Some(SheetRole::Furniture)
                        || p.role == Some(SheetRole::KeyPlan))
            })
            .cloned()
            .collect();
        let mut parts = if let Some(items) = spec.items {
            let mut seen = BTreeSet::new();
            let mut parts = Vec::new();
            for item in items {
                let (handles, label) = match item {
                    Item::Handles { handles, label } => (handles, label),
                    Item::Window { bounds, label } => {
                        if !bounds.iter().flatten().all(|v| v.is_finite())
                            || (0..2).any(|i| bounds[0][i] > bounds[1][i])
                        {
                            return Err(invalid(
                                "Item bounds need finite lower-left and upper-right corners",
                            ));
                        }
                        let handles = automatic
                            .iter()
                            .flat_map(|p| &p.handles)
                            .copied()
                            .filter(|h| {
                                let b = ink::union(
                                    ink.pieces
                                        .iter()
                                        .filter(|p| p.owner == *h)
                                        .map(|p| p.bounds),
                                )
                                .or_else(|| {
                                    automatic
                                        .iter()
                                        .find(|p| p.handles.contains(h))
                                        .map(|p| p.bounds)
                                });
                                b.is_some_and(|b| contains(bounds, b))
                            })
                            .collect();
                        (handles, label)
                    }
                };
                if handles.is_empty() {
                    return Err(invalid("Each item needs at least one entity"));
                }
                let mut boxes = Vec::new();
                for h in &handles {
                    if self.drawing.doc.get_entity(Handle::new(*h)).is_none() {
                        return Err(invalid(format!("Handle {h} does not exist")));
                    }
                    if !seen.insert(*h) {
                        return Err(invalid(format!("Handle {h} belongs to more than one item")));
                    }
                    if fixed.iter().any(|p| p.handles.contains(h)) {
                        return Err(invalid("Keep the title block and sheet frame in place"));
                    }
                    let b = ink::union(
                        ink.pieces
                            .iter()
                            .filter(|p| p.owner == *h)
                            .map(|p| p.bounds),
                    )
                    .or_else(|| {
                        automatic
                            .iter()
                            .find(|p| p.handles.contains(h))
                            .map(|p| p.bounds)
                    })
                    .ok_or_else(|| {
                        invalid(format!("Handle {h} has no visible geometry on this sheet"))
                    })?;
                    boxes.push(b);
                }
                parts.push(Part {
                    handles,
                    bounds: ink::union(boxes.into_iter()).unwrap(),
                    label: label.unwrap_or_else(|| "Part".into()),
                    title: false,
                    frame: false,
                    role: None,
                });
            }
            parts
        } else {
            automatic
                .iter()
                .filter(|p| {
                    !(paper
                        && (p.title
                            || p.role == Some(SheetRole::Furniture)
                            || p.role == Some(SheetRole::KeyPlan)))
                })
                .cloned()
                .collect()
        };
        // Explicit item labels do not override the geometry's zone, and a viewport's match
        // lines stay attached even when the caller selected just the viewport.
        for part in &mut parts {
            for group in automatic.iter().filter(|p| p.role.is_some()) {
                if part.handles.iter().any(|h| group.handles.contains(h)) {
                    for h in &group.handles {
                        if !part.handles.contains(h) {
                            part.handles.push(*h);
                        }
                    }
                    part.bounds = ink::union([part.bounds, group.bounds].into_iter()).unwrap();
                    part.role = group.role;
                }
            }
        }
        let mut unique = BTreeSet::new();
        if parts
            .iter()
            .flat_map(|p| &p.handles)
            .any(|h| !unique.insert(*h))
        {
            return Err(invalid(
                "Items share a sheet group / العناصر تشترك في مجموعة لوحة واحدة",
            ));
        }
        // Remaining parts are obstacles when only some items were requested.
        let moving: BTreeSet<_> = parts
            .iter()
            .flat_map(|p| p.handles.iter().copied())
            .collect();
        let obstacles: Vec<_> = automatic
            .iter()
            .filter(|p| !p.frame)
            .filter_map(|p| {
                if p.handles.iter().all(|h| !moving.contains(h)) {
                    return Some(p.bounds);
                }
                ink::union(
                    ink.pieces
                        .iter()
                        .filter(|piece| {
                            p.handles.contains(&piece.owner) && !moving.contains(&piece.owner)
                        })
                        .map(|piece| piece.bounds),
                )
            })
            .collect();
        let zones = self.sheet_zones(&sheet);
        let (placed, mut issues) = if let Some(zones) = &zones {
            self.arrange_in_zones(&parts, &obstacles, &fixed, zones, gap)
        } else {
            let zone = if paper {
                Some(self.arrange_zone(&sheet, &fixed, to_m)?)
            } else {
                None
            };
            let extent = ink::union(parts.iter().map(|p| p.bounds));
            let origin = spec
                .origin
                .unwrap_or_else(|| zone.or(extent).map_or([0.0, 0.0], |b| [b[0][0], b[1][1]]));
            let mut origins = vec![origin];
            if spec.origin.is_none() {
                if let Some(b) = ink::union(obstacles.iter().copied()) {
                    // A subset may currently lie over an untouched plan. Try free space without
                    // requiring the caller to guess a new origin.
                    origins.push([b[1][0] + gap, origin[1]]);
                    origins.push([origin[0], b[0][1] - gap]);
                }
                if paper {
                    for b in &obstacles {
                        origins.extend([
                            [b[1][0] + gap, origin[1]],
                            [origin[0], b[0][1] - gap],
                            [b[1][0] + gap, b[1][1]],
                        ]);
                    }
                }
            }
            let ratio = zone.map_or(2.0_f64.sqrt(), |b| {
                (b[1][0] - b[0][0]) / (b[1][1] - b[0][1])
            });
            let candidates: Vec<_> = spec
                .columns
                .map_or_else(|| (1..=parts.len().max(1)).collect(), |n| vec![n]);
            let mut best: Option<(f64, Vec<Bounds>)> = None;
            for origin in origins {
                for &columns in &candidates {
                    let mut placed = Vec::new();
                    let mut y = origin[1];
                    let mut fits = true;
                    for row in parts.chunks(columns) {
                        let height = row
                            .iter()
                            .map(|p| p.bounds[1][1] - p.bounds[0][1])
                            .fold(0.0, f64::max);
                        let mut x = origin[0];
                        for p in row {
                            let w = p.bounds[1][0] - p.bounds[0][0];
                            let h = p.bounds[1][1] - p.bounds[0][1];
                            let top = y - if matches!(spec.align, Align::Center) {
                                (height - h) / 2.0
                            } else {
                                0.0
                            };
                            let b = [[x, top - h], [x + w, top]];
                            if !b.iter().flatten().all(|v| v.is_finite())
                                || zone.is_some_and(|z| !contains(z, b))
                                || obstacles.iter().any(|o| overlaps(b, *o))
                            {
                                fits = false;
                            }
                            placed.push(b);
                            x += w + gap;
                        }
                        y -= height + gap;
                    }
                    let b = ink::union(placed.iter().copied());
                    let score = b.map_or(0.0, |b| {
                        (((b[1][0] - b[0][0]).max(1e-9) / (b[1][1] - b[0][1]).max(1e-9)) / ratio)
                            .ln()
                            .abs()
                    });
                    if fits && best.as_ref().is_none_or(|(s, _)| score < *s) {
                        best = Some((score, placed));
                    }
                }
                if best.is_some() {
                    break;
                }
            }
            let (_, placed) = best.ok_or_else(|| invalid("The parts do not fit without overlap. Use a larger sheet, fewer items or another origin."))?;
            (placed, Vec::new())
        };
        let previous = std::mem::replace(&mut self.sheet, sheet.clone());
        let result = (|| {
            let mut moves = Vec::new();
            for (part, b) in parts.iter_mut().zip(&placed) {
                let targets: Vec<_> = part
                    .handles
                    .iter()
                    .map(|h| Target {
                        handle: *h,
                        chain: vec![],
                    })
                    .collect();
                self.sheet_targets(&targets)?;
                let from = [part.bounds[0][0], part.bounds[1][1]];
                let to = [b[0][0], b[1][1]];
                let delta = [to[0] - from[0], to[1] - from[1]];
                let ordinary: Vec<_> = targets
                    .into_iter()
                    .filter(|t| {
                        !matches!(
                            self.drawing.doc.get_entity(Handle::new(t.handle)),
                            Some(EntityType::Viewport(_))
                        )
                    })
                    .collect();
                if !ordinary.is_empty() {
                    self.modify(ModifySpec::Move {
                        targets: ordinary,
                        delta,
                    })?;
                }
                // modify deliberately excludes viewports; translate only their paper centre,
                // retaining the camera, scale, clipping and all other viewport settings.
                for h in &part.handles {
                    if let Some(EntityType::Viewport(v)) =
                        self.drawing.doc.get_entity_mut(Handle::new(*h))
                    {
                        v.center.x += delta[0];
                        v.center.y += delta[1];
                    }
                }
                moves.push(Move {
                    handles: part.handles.clone(),
                    label: part.label.clone(),
                    from,
                    to,
                });
            }
            // Report all remaining collisions, including untouched content in a subset arrange.
            let final_ink = self.sheet_ink(&sheet);
            let final_parts = self.drawing_parts(&sheet, &final_ink, gap / 2.0);
            for (i, a) in final_parts.iter().enumerate().filter(|(_, p)| !p.frame) {
                for b in final_parts.iter().skip(i + 1).filter(|p| !p.frame) {
                    if overlaps(a.bounds, b.bounds) {
                        issues.push(ArrangementIssue {
                            handles: a.handles.iter().chain(&b.handles).copied().collect(),
                            code: "part_overlap".into(),
                            message: "Sheet content overlaps; review the reported parts".into(),
                            message_ar: "تتداخل محتويات اللوحة؛ راجع الأجزاء المحددة".into(),
                        });
                    }
                }
            }
            let extents = ink::union(
                placed
                    .into_iter()
                    .chain(obstacles.iter().copied())
                    .chain(fixed.iter().filter(|p| p.frame).map(|p| p.bounds)),
            );
            Ok(Arrangement {
                sheet,
                moves,
                extents,
                issues,
            })
        })();
        self.sheet = previous;
        result
    }

    pub(crate) fn part_zone(&self, part: &Part, zones: &SheetZones) -> Bounds {
        if part.role == Some(SheetRole::KeyPlan) {
            return zones.key_plan;
        }
        if part.role == Some(SheetRole::Notes)
            || (part.role.is_none()
                && part.handles.iter().all(|h| {
                    matches!(
                        self.drawing.doc.get_entity(Handle::new(*h)),
                        Some(
                            EntityType::Text(_)
                                | EntityType::MText(_)
                                | EntityType::Leader(_)
                                | EntityType::MultiLeader(_)
                        )
                    )
                }))
        {
            return zones.notes;
        }
        if part.handles.iter().any(|h| {
            matches!(
                self.drawing.doc.get_entity(Handle::new(*h)),
                Some(EntityType::Table(_))
            )
        }) {
            return zones.schedules;
        }
        zones.plans
    }

    fn arrange_in_zones(
        &self,
        parts: &[Part],
        obstacles: &[Bounds],
        fixed: &[Part],
        zones: &SheetZones,
        gap: f64,
    ) -> (Vec<Bounds>, Vec<ArrangementIssue>) {
        let mut placed = Vec::new();
        let mut issues = Vec::new();
        let mut occupied = obstacles.to_vec();
        for p in parts {
            let mut zone = self.part_zone(p, zones);
            let notes = zone == zones.notes;
            if notes {
                // Keep the key plan and scale bar at the foot of the notes column.
                for f in fixed
                    .iter()
                    .filter(|f| matches!(f.role, Some(SheetRole::KeyPlan | SheetRole::Furniture)))
                {
                    zone[0][1] = zone[0][1].max(f.bounds[1][1] + 5.0);
                }
            }
            let w = p.bounds[1][0] - p.bounds[0][0];
            let h = p.bounds[1][1] - p.bounds[0][1];
            let mut xs = vec![zone[0][0]];
            let mut ys = vec![zone[1][1]];
            let obstacles = &occupied;
            for b in obstacles {
                if !notes {
                    xs.push(b[1][0] + gap);
                }
                ys.push(b[0][1] - gap);
            }
            xs.sort_by(f64::total_cmp);
            ys.sort_by(|a, b| b.total_cmp(a));
            let mut found = None;
            'rows: for y in ys {
                for &x in &xs {
                    let b = [[x, y - h], [x + w, y]];
                    if contains(zone, b) && !obstacles.iter().any(|o| overlaps(b, *o)) {
                        found = Some(b);
                        break 'rows;
                    }
                }
            }
            let b = found.unwrap_or(p.bounds);
            if found.is_none() {
                issues.push(ArrangementIssue {
                    handles: p.handles.clone(),
                    code: "sheet_zone_overflow".into(),
                    message: "Content does not fit its sheet zone; its position was retained"
                        .into(),
                    message_ar: "المحتوى لا يتسع في منطقته باللوحة؛ احتُفظ بموضعه".into(),
                });
            }
            occupied.push(b);
            placed.push(b);
        }
        (placed, issues)
    }

    fn arrange_zone(&self, sheet: &str, fixed: &[Part], to_m: f64) -> Result<Bounds, CoreError> {
        let owner = self.drawing.doc.block_records.get(sheet).map(|b| b.handle);
        let layout = self
            .drawing
            .doc
            .objects
            .values()
            .find_map(|o| match o {
                opencadcodec::objects::ObjectType::Layout(l) if Some(l.block_record) == owner => {
                    Some(l)
                }
                _ => None,
            })
            .ok_or_else(|| invalid("Arrange paper content on a sized layout"))?;
        let (mut w, mut h) = (layout.paper_width, layout.paper_height);
        if matches!(layout.plot_rotation, 1 | 3) {
            std::mem::swap(&mut w, &mut h);
        }
        let mm = 0.001 / to_m;
        let mut zone = [[20.0 * mm, 10.0 * mm], [w - 10.0 * mm, h - 10.0 * mm]];
        if let Some(frame) = fixed.iter().find(|p| p.frame) {
            zone = frame.bounds;
        }
        // Keep rows above the title strip, including custom/imported title blocks.
        for title in fixed.iter().filter(|p| !p.frame) {
            zone[0][1] = zone[0][1].max(title.bounds[1][1] + 5.0 * mm);
        }
        if !zone.iter().flatten().all(|v| v.is_finite()) || (0..2).any(|i| zone[0][i] >= zone[1][i])
        {
            return Err(invalid(
                "The paper has no room inside its margins and title block",
            ));
        }
        Ok(zone)
    }
}
