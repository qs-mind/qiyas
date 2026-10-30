//! Read-only evidence independent of object recipes, recognition rules and BOQ mappings.
//! Counts come from sheet-owned CAD entities, including hidden layers. Block definitions are
//! not counted a second time. Geometry tolerances are numerical, never design limits.
use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::{EntityType, Handle};
use qiyas_geom::{predicates as geom, Path, Xform};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::decisions::{same_number, unit_scale, DecisionSource, Status};
use crate::measurement::{GeneratedItem, MeasurementReport};
use crate::{CoreError, Session};

type Point = [f64; 2];
const FINDING_KINDS: &[&str] = &[
    "raw_recount",
    "quantity_mismatch",
    "unallocated_boq",
    "unlocated_boq_source",
    "outline",
    "outside_outline",
    "dangling_wall",
    "space_without_opening",
    "space_overlap",
    "unmatched_answer_number",
    "decision_dependency",
    "design_decision",
    "not_checked",
];
fn empty_counts() -> BTreeMap<String, usize> {
    FINDING_KINDS
        .iter()
        .map(|kind| ((*kind).into(), 0))
        .collect()
}

/// All sheets by default. Explicit geometry handles supply roles for untagged drawings;
/// they do not alter the raw recount. No building category or rule pack is consulted.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutsideQuery {
    #[serde(default)]
    pub sheet: Option<String>,
    /// Closed boundary on the selected sheet. With several sheets, it applies to its own sheet.
    #[serde(default)]
    pub boundary: Option<u64>,
    #[serde(default)]
    pub wall_handles: Vec<u64>,
    #[serde(default)]
    pub space_handles: Vec<u64>,
    #[serde(default)]
    pub opening_handles: Vec<u64>,
    #[serde(default)]
    pub answer: Option<String>,
    #[serde(default)]
    pub offset: usize,
    /// Default 20, at most 200. Also bounded by the usual 12 KB record budget.
    #[serde(default)]
    pub limit: usize,
}

/// Lossless paging. As with records, one oversized record is allowed so paging advances.
pub(crate) fn page(mut envelope: Value, entries: Vec<Value>, offset: usize, limit: usize) -> Value {
    let total = entries.len();
    let limit = if limit == 0 { 20 } else { limit.min(200) };
    let mut rows = Vec::new();
    let mut bytes = 0;
    let mut next = offset.min(total);
    for entry in entries.into_iter().skip(offset).take(limit) {
        let size = serde_json::to_vec(&entry).expect("JSON value").len();
        if !rows.is_empty() && bytes + size > crate::records::MAX_BYTES {
            break;
        }
        bytes += size;
        rows.push(entry);
        next += 1;
    }
    envelope["entries"] = json!(rows);
    envelope["offset"] = json!(offset);
    envelope["total_entries"] = json!(total);
    envelope["next_offset"] = if next < total {
        json!(next)
    } else {
        Value::Null
    };
    envelope["truncated"] = json!(next < total);
    envelope
}

#[derive(Default)]
struct Recount {
    handles: BTreeSet<u64>,
    count: usize,
    value: f64,
}
struct Shape {
    handle: u64,
    paths: Vec<Path>,
}
struct Space {
    handles: Vec<u64>,
    rings: Vec<Vec<Point>>,
}
fn segments(path: &Path) -> impl Iterator<Item = (Point, Point)> + '_ {
    path.points
        .iter()
        .copied()
        .zip(path.points.iter().copied().cycle().skip(1))
        .take(if path.closed {
            path.points.len()
        } else {
            path.points.len().saturating_sub(1)
        })
}
fn finding(kind: &str, sheet: &str, handles: &[u64], en: &str, ar: &str) -> Value {
    json!({"kind":kind,"sheet":sheet,"handles":handles,"message":en,"message_ar":ar})
}
fn not_checked(sheet: &str, handles: &[u64], check: &str, en: &str, ar: &str) -> Value {
    let mut row = finding("not_checked", sheet, handles, en, ar);
    row["check"] = json!(check);
    row
}

impl Session {
    /// Independent raw recount and geometric observations, paged with totals per finding kind.
    /// This read never edits quantities, decisions, queries, comments or the visible sheet.
    pub fn outside_check(&self, query: &OutsideQuery) -> Result<Value, CoreError> {
        let entries = self.outside_findings(query)?;
        let mut totals = empty_counts();
        let mut per_sheet = BTreeMap::<String, BTreeMap<String, usize>>::new();
        for row in &entries {
            let kind = row["kind"].as_str().unwrap_or("unknown").to_owned();
            *totals.entry(kind.clone()).or_default() += 1;
            *per_sheet
                .entry(row["sheet"].as_str().unwrap_or("").into())
                .or_insert_with(empty_counts)
                .entry(kind)
                .or_default() += 1;
        }
        Ok(page(
            json!({"totals":totals,"sheets":per_sheet,
            "scope":"sheet_owned_entities","scope_ar":"عناصر الرسم التابعة لكل لوحة",
            "curve_chord_error_drawing_units":self.outside_curve_tolerance(),
            "units_known":self.drawing.units != crate::Units::Unknown}),
            entries,
            query.offset,
            query.limit,
        ))
    }

    pub(crate) fn outside_summary(&self, answer: &str) -> Result<Value, CoreError> {
        let report = self.outside_check(&OutsideQuery {
            answer: (!answer.is_empty()).then(|| answer.to_owned()),
            limit: 1,
            ..Default::default()
        })?;
        Ok(json!({"totals":report["totals"],"sheets":report["sheets"],
            "command":{"type":"outside_check"}}))
    }

    fn outside_curve_tolerance(&self) -> f64 {
        // Flattening accuracy only. Measurements below use analytic geometry where available.
        1e-5 / self.drawing.units.to_metres()
    }

    fn outside_findings(&self, query: &OutsideQuery) -> Result<Vec<Value>, CoreError> {
        let doc = &self.drawing.doc;
        // Read stored generated results only. No evaluation of types, rules or mappings.
        let report: MeasurementReport =
            crate::knowledge::get(doc, crate::knowledge::Kind::Measurement, "result")?
                .map(|entry| serde_json::from_value(entry.data))
                .transpose()
                .map_err(|e| {
                    CoreError::InvalidCommand(format!(
                        "Invalid measurement record: {e} / سجل القياس غير صالح"
                    ))
                })?
                .unwrap_or_default();
        let decisions = self.decisions()?;
        let factor = self.drawing.units.to_metres();
        let tol = self.outside_curve_tolerance();
        let mut rows = Vec::new();
        let mut measured = Vec::<(f64, String)>::new();
        let sheets: Vec<_> = self
            .drawing
            .sheets()
            .into_iter()
            .filter(|s| {
                query.sheet.as_ref().is_none_or(|q| {
                    s.block.eq_ignore_ascii_case(q) || s.name.eq_ignore_ascii_case(q)
                })
            })
            .collect();
        if sheets.is_empty() {
            return Err(CoreError::InvalidCommand(
                "Sheet not found / اللوحة غير موجودة".into(),
            ));
        }
        let selected: BTreeSet<_> = sheets
            .iter()
            .filter_map(|s| doc.block_records.get(&s.block).map(|b| b.handle))
            .collect();
        for h in query
            .boundary
            .iter()
            .chain(&query.wall_handles)
            .chain(&query.space_handles)
            .chain(&query.opening_handles)
        {
            let e = doc
                .get_entity(Handle::new(*h))
                .ok_or(CoreError::NoSuchEntity(*h))?;
            if !selected.contains(&e.common().owner_handle) {
                return Err(CoreError::InvalidCommand("Check handles must belong to a selected sheet / يجب أن تتبع العناصر المحددة لوحة مختارة".into()));
            }
        }
        for sheet in &sheets {
            let name = &sheet.block;
            if self.drawing.units == crate::Units::Unknown {
                rows.push(not_checked(name, &[], "physical_units", "Drawing units are unknown; metre values use the application's millimetre conversion", "وحدات الرسم غير معروفة؛ تستخدم قيم الأمتار تحويل المليمتر الخاص بالتطبيق"));
            }
            let entities: Vec<_> = doc.entities_in_block(name).collect();
            let sheet_handles: BTreeSet<_> =
                entities.iter().map(|e| e.common().handle.value()).collect();
            let mut groups = BTreeMap::<(String, String, String), Recount>::new();
            let mut shapes = Vec::new();
            let mut closed = Vec::<Space>::new();
            let mut spaces = Vec::<Space>::new();
            let mut walls = Vec::<(u64, Path, Option<qiyas_geom::walls::Run>)>::new();
            let mut openings = Vec::<Shape>::new();
            let mut wall_members = BTreeSet::new();
            for e in entities {
                let h = e.common().handle.value();
                let tag = crate::walls::wall_tag(e);
                let wall_graphics = tag
                    .as_ref()
                    .is_some_and(|t| !matches!(t.role.as_str(), "run" | "opening"))
                    || crate::xdata::read_json::<crate::objects::ObjectPart>(
                        e.common(),
                        crate::objects::PART_APP,
                    )
                    .and_then(|p| doc.get_entity(Handle::new(p.owner)))
                    .and_then(crate::walls::wall_tag)
                    .is_some();
                let flattened = qiyas_geom::flatten(doc, e, &Xform::identity(), tol);
                if flattened.truncated {
                    rows.push(not_checked(
                        name,
                        &[h],
                        "geometry",
                        "Geometry expansion was truncated",
                        "لم يكتمل توسيع الهندسة",
                    ));
                }
                let paths = flattened.into_paths();
                if let Some(length) = qiyas_geom::exact_length(e, &Xform::identity()) {
                    measured.push((length * factor, "m".into()));
                }
                if let Some(area) = qiyas_geom::exact_area(e, &Xform::identity()) {
                    measured.push((area * factor * factor, "m2".into()));
                }
                let polyline = matches!(
                    e,
                    EntityType::LwPolyline(_) | EntityType::Polyline(_) | EntityType::Polyline2D(_)
                );
                let area_entity = (polyline && paths.iter().any(|p| p.closed))
                    || matches!(e, EntityType::Hatch(_));
                let mut add = |kind: &str, block: &str, value: f64, count: usize| {
                    let group = groups
                        .entry((kind.into(), e.common().layer.clone(), block.into()))
                        .or_default();
                    group.handles.insert(h);
                    group.count += count;
                    group.value += value;
                };
                if let EntityType::Insert(insert) = e {
                    let count = usize::from(insert.row_count.max(1))
                        * usize::from(insert.column_count.max(1));
                    add("blocks", &insert.block_name, count as f64, count);
                    measured.push((count as f64, "nr".into()));
                }
                if !wall_graphics && (polyline || matches!(e, EntityType::Line(_))) {
                    let value = qiyas_geom::exact_length(e, &Xform::identity())
                        .unwrap_or_else(|| paths.iter().map(Path::length).sum())
                        * factor;
                    add("length", "", value, 1);
                    measured.push((value, "m".into()));
                }
                let rings: Vec<_> = paths
                    .iter()
                    .filter(|p| p.closed && p.points.len() >= 3)
                    .map(|p| p.points.clone())
                    .collect();
                if area_entity {
                    let area = analytic_area(e, &rings).unwrap_or_else(|| ring_set_area(&rings))
                        * factor
                        * factor;
                    // Outlines and hatch are multiple renderings of one wall, not extra quantities.
                    if !wall_graphics && !tag.as_ref().is_some_and(|t| t.role == "run") {
                        add("area", "", area, 1);
                    }
                    measured.push((area, "m2".into()));
                    if !rings.is_empty() {
                        closed.push(Space {
                            handles: vec![h],
                            rings: rings.clone(),
                        });
                    }
                }
                if tag.is_some() || wall_graphics {
                    wall_members.insert(h);
                }
                if tag.as_ref().is_some_and(|t| t.role == "run") || query.wall_handles.contains(&h)
                {
                    if let Some(path) = paths.first().cloned().filter(|p| p.points.len() >= 2) {
                        let run = tag.as_ref().and_then(|t| {
                            t.spec.runs.get(t.index).map(|r| qiyas_geom::walls::Run {
                                points: path.points.clone(),
                                closed: path.closed,
                                thickness: Some(r.thickness.unwrap_or(t.spec.thickness)),
                                justification: Some(crate::walls::geom_justification(
                                    r.justification.unwrap_or(t.spec.justification),
                                )),
                            })
                        });
                        walls.push((h, path, run));
                    } else {
                        rows.push(not_checked(
                            name,
                            &[h],
                            "dangling_wall",
                            "Wall has no measurable path",
                            "لا يوجد مسار قابل للقياس للجدار",
                        ));
                    }
                }
                // Opening identity comes from Qiyas membership, never a layer-name guess.
                // Older exports may retain their generated insert but not its xdata.
                let opening_insert = matches!(e, EntityType::Insert(insert)
                    if insert.block_name.starts_with("QIYAS_DOOR_") || insert.block_name.starts_with("QIYAS_WIN_"));
                if tag.as_ref().is_some_and(|t| t.role == "opening")
                    || opening_insert
                    || query.opening_handles.contains(&h)
                {
                    if paths.is_empty() {
                        rows.push(not_checked(
                            name,
                            &[h],
                            "opening_contact",
                            "Opening has no measurable geometry",
                            "لا توجد هندسة قابلة للقياس للفتحة",
                        ));
                    }
                    let mut opening_paths = paths.clone();
                    if let Some(footprint) = tag.as_ref().and_then(opening_footprint) {
                        opening_paths.push(footprint);
                    }
                    openings.push(Shape {
                        handle: h,
                        paths: opening_paths,
                    });
                }
                if query.space_handles.contains(&h)
                    || crate::room_objects::is_room_boundary(e)
                    || crate::object_regions::retained_region(e).is_some()
                {
                    if rings.is_empty() {
                        rows.push(not_checked(
                            name,
                            &[h],
                            "space_boundary",
                            "Space has no closed boundary",
                            "لا يوجد حد مغلق للفراغ",
                        ));
                    } else {
                        let mut rings = rings.clone();
                        if let Some(region) = crate::object_regions::retained_region(e) {
                            for (index, hole) in region.holes.iter().enumerate() {
                                let poly = crate::object_regions::region_polyline(
                                    hole,
                                    region
                                        .hole_bulges
                                        .get(index)
                                        .map(Vec::as_slice)
                                        .unwrap_or(&[]),
                                );
                                rings.extend(
                                    qiyas_geom::flatten(
                                        doc,
                                        &EntityType::LwPolyline(poly),
                                        &Xform::identity(),
                                        tol,
                                    )
                                    .into_paths()
                                    .into_iter()
                                    .filter(|p| p.closed)
                                    .map(|p| p.points),
                                );
                            }
                        }
                        spaces.push(Space {
                            handles: vec![h],
                            rings,
                        });
                    }
                }
                shapes.push(Shape { handle: h, paths });
            }
            for ((kind, layer, block), raw) in groups {
                let unit = match kind.as_str() {
                    "blocks" => "nr",
                    "area" => "m2",
                    _ => "m",
                };
                measured.push((raw.value, unit.into()));
                let (sum, boq_handles, codes, ambiguous) =
                    compare_group(&report.items, &raw.handles, unit);
                let handles: Vec<_> = raw.handles.iter().copied().collect();
                let mut row = finding(
                    "raw_recount",
                    name,
                    &handles,
                    "Raw geometry and generated BOQ",
                    "الهندسة الخام والمقايسة الناتجة",
                );
                row["group"] = json!({"kind":kind,"layer":layer,"block":block});
                row["raw_count"] = json!(raw.count);
                row["raw_quantity"] = json!(raw.value);
                row["unit"] = json!(unit);
                row["boq_quantity"] = json!(sum);
                if unit == "nr" {
                    row["boq_count"] = json!(sum);
                }
                row["boq_handles"] = json!(boq_handles);
                row["boq_items"] = json!(codes);
                row["unallocated_items"] = json!(ambiguous);
                row["comparison_complete"] = json!(ambiguous.is_empty());
                // A source measured in area or volume is not a length discrepancy.
                // Retain its raw geometry, and state that the BOQ uses a different dimension.
                let other_dimension = codes.is_empty()
                    && raw.handles.iter().all(|h| {
                        report.items.iter().any(|item| {
                            !item.informational
                                && unit_scale(&item.unit).0 != unit
                                && item_handles(item).contains(h)
                        })
                    });
                row["comparison_applicable"] = json!(!other_dimension);
                rows.push(row.clone());
                if other_dimension {
                    continue;
                }
                if !ambiguous.is_empty() {
                    row["kind"] = json!("unallocated_boq");
                    row["message"] =
                        json!("BOQ sources span raw groups or have no attributable dimension line");
                    row["message_ar"] =
                        json!("مصادر المقايسة تمتد عبر مجموعات خام أو تفتقد سطر أبعاد يمكن إسناده");
                    rows.push(row);
                } else if !same_number(raw.value, sum) {
                    row["kind"] = json!("quantity_mismatch");
                    row["difference"] = json!(raw.value - sum);
                    row["message"] = json!("Raw quantity differs from the generated BOQ quantity");
                    row["message_ar"] = json!("الكمية الخام تختلف عن كمية المقايسة الناتجة");
                    rows.push(row);
                }
            }
            let wall_handles: Vec<_> = walls.iter().map(|w| w.0).collect();
            let runs: Vec<_> = walls.iter().filter_map(|w| w.2.clone()).collect();
            let mut union = Vec::new();
            if let Some(thickness) = runs.first().and_then(|r| r.thickness) {
                match qiyas_geom::walls::build_walls(&runs, thickness, &[], qiyas_geom::SAME_POINT)
                {
                    Ok(built) => union = built.regions,
                    Err(_) => rows.push(not_checked(
                        name,
                        &wall_handles,
                        "wall_union",
                        "Wall union could not be measured",
                        "تعذر قياس اتحاد الجدران",
                    )),
                }
            }
            let explicit = query.boundary.filter(|h| sheet_handles.contains(h));
            let (outlines, outline_handles, basis) = if let Some(h) = explicit {
                let boundary = closed.iter().find(|s| s.handles.contains(&h))
                    .ok_or_else(|| CoreError::InvalidCommand("Outline must be a closed polyline or hatch / يجب أن تكون الحدود خطا متعددا مغلقا أو تهشيرا".into()))?;
                (boundary.rings.clone(), vec![h], "caller_boundary")
            } else if !union.is_empty() {
                (
                    union.iter().map(|r| r.outer.clone()).collect(),
                    wall_handles.clone(),
                    "wall_union",
                )
            } else if let Some(boundary) = closed
                .iter()
                .max_by(|a, b| ring_set_area(&a.rings).total_cmp(&ring_set_area(&b.rings)))
            {
                (
                    boundary.rings.clone(),
                    boundary.handles.clone(),
                    "largest_closed_boundary",
                )
            } else {
                (Vec::new(), Vec::new(), "unavailable")
            };
            if outlines.is_empty() {
                rows.push(not_checked(
                    name,
                    &[],
                    "outside_outline",
                    "No building outline is available",
                    "لا يتوفر محيط للمبنى",
                ));
            } else {
                let mut outline = finding(
                    "outline",
                    name,
                    &outline_handles,
                    "Boundary used for containment",
                    "الحد المستخدم لفحص الاحتواء",
                );
                outline["basis"] = json!(basis);
                rows.push(outline);
                for shape in &shapes {
                    if outline_handles.contains(&shape.handle) {
                        continue;
                    }
                    if shape.paths.is_empty() {
                        rows.push(not_checked(
                            name,
                            &[shape.handle],
                            "outside_outline",
                            "Entity has no measurable planar geometry",
                            "لا توجد هندسة مستوية قابلة للقياس للعنصر",
                        ));
                        continue;
                    }
                    let outside = shape.paths.iter().any(|p| {
                        p.points
                            .iter()
                            .any(|&v| !outlines.iter().any(|r| geom::in_ring(v, r)))
                            || segments(p).any(|(a, b)| {
                                !outlines
                                    .iter()
                                    .any(|r| geom::segment_inside(r, a, b, qiyas_geom::SAME_POINT))
                            })
                    });
                    if outside {
                        let mut row = finding(
                            "outside_outline",
                            name,
                            &[shape.handle],
                            "Geometry extends outside the outline",
                            "الهندسة تمتد خارج المحيط",
                        );
                        row["outline_handles"] = json!(outline_handles);
                        rows.push(row);
                    }
                }
            }
            if walls.is_empty() {
                rows.push(not_checked(
                    name,
                    &[],
                    "dangling_wall",
                    "No wall geometry is identified; provide wall_handles for untagged walls",
                    "لم يتم تحديد هندسة الجدران؛ أدخل wall_handles للجدران غير الموسومة",
                ));
            }
            let strips: BTreeMap<_, _> = walls
                .iter()
                .filter_map(|(h, _, run)| {
                    let run = run.as_ref()?;
                    let width = run.thickness?;
                    qiyas_geom::walls::build_walls(
                        std::slice::from_ref(run),
                        width,
                        &[],
                        qiyas_geom::SAME_POINT,
                    )
                    .ok()
                    .map(|wall| (*h, wall.regions))
                })
                .collect();
            for (h, path, run) in &walls {
                if path.closed {
                    continue;
                }
                for (end, point) in [
                    ("start", path.points[0]),
                    ("end", *path.points.last().unwrap()),
                ] {
                    // Contact follows the other wall's actual strip and justification.
                    let touching = walls
                        .iter()
                        .filter(|w| w.0 != *h)
                        .any(|(other_h, other, _)| {
                            strips.get(other_h).is_some_and(|regions| {
                                regions.iter().any(|region| {
                                    geom::in_ring(point, &region.outer)
                                        && !region.holes.iter().any(|r| geom::inside(point, r))
                                })
                            }) || segments(other).any(|(a, b)| geom::on_edge(point, a, b))
                        })
                        || shapes
                            .iter()
                            .filter(|s| s.handle != *h && !wall_members.contains(&s.handle))
                            .flat_map(|s| &s.paths)
                            .any(|p| segments(p).any(|(a, b)| geom::on_edge(point, a, b)));
                    if !touching {
                        let mut row = finding(
                            "dangling_wall",
                            name,
                            &[*h],
                            "Wall endpoint touches no other geometry",
                            "طرف الجدار لا يلامس هندسة أخرى",
                        );
                        row["end"] = json!(end);
                        row["point"] = json!(point);
                        row["thickness"] = json!(run.as_ref().and_then(|r| r.thickness));
                        rows.push(row);
                    }
                }
            }
            // Every uncut wall-union hole is an enclosure, including rooms not yet retained.
            // A retained boundary with the same outer ring represents that enclosure already.
            for region in &union {
                for hole in &region.holes {
                    let area = geom::ring_area(hole).abs();
                    let retained = spaces.iter().any(|space| {
                        space.rings.first().is_some_and(|outer| {
                            same_number(geom::ring_area(outer).abs(), area)
                                && same_number(
                                    intersection_area(
                                        std::slice::from_ref(outer),
                                        std::slice::from_ref(hole),
                                    ),
                                    area,
                                )
                        })
                    });
                    if !retained {
                        spaces.push(Space {
                            handles: wall_handles.clone(),
                            rings: vec![hole.clone()],
                        });
                    }
                }
            }
            if spaces.is_empty() {
                spaces.extend(closed.into_iter().filter(|s| {
                    !s.handles
                        .iter()
                        .any(|h| outline_handles.contains(h) || wall_members.contains(h))
                }));
            }
            if spaces.is_empty() {
                rows.push(not_checked(
                    name,
                    &[],
                    "space_boundary",
                    "No enclosed space boundaries are available",
                    "لا تتوفر حدود مغلقة للفراغات",
                ));
            }
            for space in &spaces {
                let has_opening =
                    openings.iter().any(|opening| {
                        opening.paths.iter().any(|path| {
                            path.points.iter().any(|&point| {
                                space.rings.iter().any(|r| {
                                    geom::edges(r).any(|(a, b)| geom::on_edge(point, a, b))
                                })
                            }) || segments(path).any(|(a, b)| {
                                space.rings.iter().any(|r| {
                                    geom::edges(r).any(|(c, d)| geom::intersects(a, b, c, d))
                                })
                            })
                        })
                    });
                if !has_opening {
                    let mut row = finding(
                        "space_without_opening",
                        name,
                        &space.handles,
                        "No identified opening intersects this enclosed boundary",
                        "لا توجد فتحة محددة تتقاطع مع حدود هذا الفراغ المغلق",
                    );
                    row["centroid"] = json!(space
                        .rings
                        .first()
                        .and_then(|ring| geom::ring_centroid(ring)));
                    row["area_m2"] = json!(ring_set_area(&space.rings) * factor * factor);
                    rows.push(row);
                }
            }
            for (i, a) in spaces.iter().enumerate() {
                for b in spaces.iter().skip(i + 1) {
                    let bounds = |space: &Space| {
                        space.rings.iter().flatten().fold(
                            [
                                f64::INFINITY,
                                f64::INFINITY,
                                f64::NEG_INFINITY,
                                f64::NEG_INFINITY,
                            ],
                            |v, p| {
                                [
                                    v[0].min(p[0]),
                                    v[1].min(p[1]),
                                    v[2].max(p[0]),
                                    v[3].max(p[1]),
                                ]
                            },
                        )
                    };
                    let (aa, bb) = (bounds(a), bounds(b));
                    if aa[0] >= bb[2] || bb[0] >= aa[2] || aa[1] >= bb[3] || bb[1] >= aa[3] {
                        continue;
                    }
                    let overlap = intersection_area(&a.rings, &b.rings) * factor * factor;
                    if !same_number(overlap, 0.0) {
                        let handles: Vec<_> = a.handles.iter().chain(&b.handles).copied().collect();
                        let mut row = finding(
                            "space_overlap",
                            name,
                            &handles,
                            "Enclosed boundaries overlap",
                            "حدود الفراغات المغلقة متداخلة",
                        );
                        row["area_m2"] = json!(overlap);
                        rows.push(row);
                    }
                }
            }
            for decision in &decisions {
                let dependencies =
                    decision_dependencies(&report.items, &decision.name, &sheet_handles);
                if matches!(decision.status, Status::Asked | Status::Provisional)
                    && !dependencies.0.is_empty()
                {
                    let mut row = finding(
                        "decision_dependency",
                        name,
                        &dependencies.0,
                        "BOQ depends on an unresolved decision; review",
                        "المقايسة تعتمد على قرار غير محسوم؛ للمراجعة",
                    );
                    row["decision"] = json!(decision);
                    row["boq_items"] = json!(dependencies.1);
                    row["review_only"] = json!(true);
                    rows.push(row);
                }
            }
        }
        for item in report.items.iter().filter(|i| !i.informational) {
            let handles = item_handles(item);
            let missing: Vec<_> = handles
                .iter()
                .filter(|h| doc.get_entity(Handle::new(**h)).is_none())
                .copied()
                .collect();
            if handles.is_empty() || !missing.is_empty() {
                let mut row = finding(
                    "unlocated_boq_source",
                    "",
                    &missing,
                    "BOQ has missing or absent drawing sources",
                    "المقايسة تحتوي على مصادر رسم مفقودة أو غير محددة",
                );
                row["boq_items"] = json!([item.code]);
                row["boq_handles"] = json!(handles);
                rows.push(row);
            }
        }
        for decision in &decisions {
            if decision.source == DecisionSource::MizanDesign {
                let handles: BTreeSet<_> = report
                    .items
                    .iter()
                    .flat_map(|item| &item.lines)
                    .filter(|line| {
                        line.operands.iter().flat_map(|o| &o.sources).any(|s| {
                            s.source
                                .decision
                                .as_ref()
                                .is_some_and(|n| n.eq_ignore_ascii_case(&decision.name))
                        })
                    })
                    .flat_map(|line| line.sources.iter().map(|s| s.handle))
                    .collect();
                let mut row = finding(
                    "design_decision",
                    "",
                    &handles.into_iter().collect::<Vec<_>>(),
                    "Mizan design decision and engineering reason for review",
                    "قرار ميزان التصميمي وسببه الهندسي للمراجعة",
                );
                row["decision"] = json!(decision);
                row["review_only"] = json!(true);
                rows.push(row);
            }
            if let Some(n) = decision.value.as_f64() {
                measured.push((n, decision.unit.clone()));
            }
        }
        measured.extend(report.items.iter().map(|i| (i.quantity, i.unit.clone())));
        measured.extend(
            report
                .quantities
                .iter()
                .map(|i| (i.quantity, i.unit.clone())),
        );
        measured.extend(
            self.takeoff
                .items
                .iter()
                .filter(|i| i.available())
                .map(|i| (i.shown, i.unit.clone())),
        );
        if let Some(answer) = &query.answer {
            for (text, value, unit, start, end, decimals) in answer_numbers(answer) {
                let (dimension, scale) = unit_scale(&unit);
                if !matches!(
                    dimension.as_str(),
                    "m" | "m2" | "m3" | "nr" | "kg" | "deg" | "l" | "s" | "h" | "n" | "pa" | "%"
                ) && !measured.iter().any(|(_, u)| unit_scale(u).0 == dimension)
                {
                    continue;
                }
                let matches = measured.iter().any(|(n, u)| {
                    let (d, s) = unit_scale(u);
                    // Accept the established printed precision, not a design tolerance.
                    d == dimension
                        && (same_number(value * scale, n * s)
                            || ((n * s / scale) * 10f64.powi(decimals.max(2))).round()
                                / 10f64.powi(decimals.max(2))
                                == value)
                });
                if !matches {
                    let mut row = finding(
                        "unmatched_answer_number",
                        "",
                        &[],
                        "Number with unit has no matching quantity, decision or measurement",
                        "رقم بوحدة لا يقابله حصر أو قرار أو قياس",
                    );
                    row["text"] = json!(text);
                    row["value"] = json!(value);
                    row["unit"] = json!(unit);
                    row["answer_bytes"] = json!([start, end]);
                    rows.push(row);
                }
            }
        }
        Ok(rows)
    }
}

fn analytic_area(entity: &EntityType, rings: &[Vec<Point>]) -> Option<f64> {
    if rings.iter().any(|r| {
        Path {
            points: r.clone(),
            closed: true,
        }
        .self_intersects()
    }) {
        return None;
    }
    if let Some(area) = qiyas_geom::exact_area(entity, &Xform::identity()) {
        return Some(area);
    }
    let areas = qiyas_geom::exact_hatch_areas(entity, &Xform::identity())?;
    if areas.len() != rings.len() {
        return None;
    }
    Some(
        areas
            .iter()
            .enumerate()
            .map(|(i, area)| {
                let depth = rings
                    .iter()
                    .enumerate()
                    .filter(|(j, ring)| {
                        *j != i && rings[i].first().is_some_and(|p| geom::inside(*p, ring))
                    })
                    .count();
                if depth % 2 == 0 {
                    *area
                } else {
                    -*area
                }
            })
            .sum::<f64>()
            .abs(),
    )
}

fn item_handles(item: &GeneratedItem) -> BTreeSet<u64> {
    item.lines
        .iter()
        .filter(|l| l.included)
        .flat_map(|l| l.sources.iter().map(|s| s.handle))
        .collect()
}
fn compare_group(
    items: &[GeneratedItem],
    handles: &BTreeSet<u64>,
    unit: &str,
) -> (f64, BTreeSet<u64>, Vec<String>, Vec<String>) {
    let mut sum = 0.0;
    let mut sources = BTreeSet::new();
    let mut codes = Vec::new();
    let mut ambiguous = Vec::new();
    for item in items.iter().filter(|i| !i.informational) {
        let (dimension, scale) = unit_scale(&item.unit);
        if dimension != unit {
            continue;
        }
        let all = item_handles(item);
        if all.is_disjoint(handles) {
            continue;
        }
        sources.extend(all.iter().copied());
        codes.push(item.code.clone());
        if all.is_subset(handles) {
            sum += item.quantity * scale;
        } else {
            let mut partial = 0.0;
            let mut uncertain = !same_number(
                item.lines
                    .iter()
                    .filter(|l| l.included)
                    .map(|l| {
                        let (dimension, factor) = unit_scale(&l.unit);
                        if dimension == unit {
                            l.value * factor
                        } else {
                            f64::NAN
                        }
                    })
                    .sum(),
                item.quantity * scale,
            );
            for line in item.lines.iter().filter(|l| l.included) {
                let hs: BTreeSet<_> = line.sources.iter().map(|s| s.handle).collect();
                if hs.is_empty() {
                    uncertain = true;
                    continue;
                }
                if hs.is_disjoint(handles) {
                    continue;
                }
                let (dimension, line_scale) = unit_scale(&line.unit);
                if !hs.is_subset(handles) || dimension != unit {
                    uncertain = true;
                    continue;
                }
                partial += line.value * line_scale;
            }
            if uncertain {
                ambiguous.push(item.code.clone());
            } else {
                sum += partial;
            }
        }
    }
    (sum, sources, codes, ambiguous)
}

fn decision_dependencies(
    items: &[GeneratedItem],
    name: &str,
    sheet: &BTreeSet<u64>,
) -> (Vec<u64>, Vec<String>) {
    let mut handles = BTreeSet::new();
    let mut codes = BTreeSet::new();
    for item in items.iter().filter(|i| !i.informational) {
        for line in item.lines.iter().filter(|l| l.included) {
            if line.operands.iter().flat_map(|o| &o.sources).any(|s| {
                s.source
                    .decision
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            }) {
                for source in &line.sources {
                    if sheet.contains(&source.handle) {
                        handles.insert(source.handle);
                        codes.insert(item.code.clone());
                    }
                }
            }
        }
    }
    (handles.into_iter().collect(), codes.into_iter().collect())
}

// Even-odd scanline integration works for concave boundaries and holes. All vertex and
// crossing ordinates partition the plane into slabs with linear interval widths. The midpoint
// rule is exact for straight segments in each slab; curved boundaries are flattened above.
fn intervals(rings: &[Vec<Point>], y: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<_> = rings
        .iter()
        .flat_map(|r| geom::edges(r))
        .filter(|(a, b)| (a[1] > y) != (b[1] > y))
        .map(|(a, b)| a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1]))
        .collect();
    xs.sort_by(f64::total_cmp);
    xs.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect()
}
fn integrated_area(a: &[Vec<Point>], b: Option<&[Vec<Point>]>) -> f64 {
    let edges: Vec<_> = a
        .iter()
        .chain(b.into_iter().flatten())
        .flat_map(|r| geom::edges(r))
        .collect();
    let mut ys: Vec<_> = edges.iter().flat_map(|(a, b)| [a[1], b[1]]).collect();
    for (i, (a, b)) in edges.iter().enumerate() {
        for (c, d) in edges.iter().skip(i + 1) {
            if let Some(p) = geom::crossing(*a, *b, *c, *d) {
                ys.push(p[1]);
            }
        }
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    ys.windows(2)
        .map(|y| {
            let at = y[0] + (y[1] - y[0]) / 2.0;
            let first = intervals(a, at);
            let width = match b {
                None => first.iter().map(|(a, b)| b - a).sum(),
                Some(b) => {
                    let second = intervals(b, at);
                    first
                        .iter()
                        .flat_map(|(a, b)| {
                            second
                                .iter()
                                .map(move |(c, d)| (b.min(*d) - a.max(*c)).max(0.0))
                        })
                        .sum::<f64>()
                }
            };
            width * (y[1] - y[0])
        })
        .sum::<f64>()
        .max(0.0)
}
fn ring_set_area(rings: &[Vec<Point>]) -> f64 {
    integrated_area(rings, None)
}
fn intersection_area(a: &[Vec<Point>], b: &[Vec<Point>]) -> f64 {
    integrated_area(a, Some(b))
}

fn answer_numbers(answer: &str) -> Vec<(String, f64, String, usize, usize, i32)> {
    // Keep an index map so Arabic normalization still returns original UTF-8 byte locations.
    let mut normalized = String::new();
    let mut positions = Vec::new();
    for (index, c) in answer.char_indices() {
        let c = match c {
            '٠'..='٩' => char::from_u32('0' as u32 + c as u32 - '٠' as u32).unwrap(),
            '۰'..='۹' => char::from_u32('0' as u32 + c as u32 - '۰' as u32).unwrap(),
            '٫' => '.',
            '٬' => ',',
            _ => c,
        };
        positions.extend(std::iter::repeat_n(index, c.len_utf8()));
        normalized.push(c);
    }
    positions.push(answer.len());
    let pattern = regex::Regex::new(r"(?i)([-+]?\d+(?:[,]\d{3})*(?:\.\d+)?(?:[eE][-+]?\d+)?)\s*(متر\s+(?:مربع|مكعب)|(?:square|cubic)\s+met(?:re|er)s?|(?:sq|cu)\s+m|[\p{L}°%]+(?:\^?[23²³])?)").expect("static numeric-unit pattern");
    pattern
        .captures_iter(&normalized)
        .filter_map(|c| {
            let whole = c.get(0)?;
            let number = c.get(1)?.as_str().replace(',', "");
            let unit = c.get(2)?.as_str().to_owned();
            let value = number.parse::<f64>().ok().filter(|n| n.is_finite())?;
            let decimals = number
                .split('.')
                .nth(1)
                .map_or(0, |s| {
                    s.chars().take_while(char::is_ascii_digit).count() as i32
                })
                .min(12);
            let start = positions[whole.start()];
            let end = positions[whole.end()];
            Some((answer[start..end].into(), value, unit, start, end, decimals))
        })
        .collect()
}

/// The cut crosses both wall faces even when a symbolic leaf or glazing line does not.
/// Read only the wall command's geometric source; no object recipe or measurement rule.
fn opening_footprint(tag: &crate::walls::WallTag) -> Option<Path> {
    if tag.role != "opening" {
        return None;
    }
    let opening = tag.spec.openings.get(tag.index)?;
    let runs = crate::walls::geom_runs(&tag.spec);
    let run = runs.get(opening.run)?;
    let built = qiyas_geom::walls::build_walls_justified(
        std::slice::from_ref(run),
        tag.spec.thickness,
        &[qiyas_geom::walls::Opening {
            run: 0,
            at: opening.at,
            width: opening.width,
        }],
        qiyas_geom::SAME_POINT,
        crate::walls::geom_justification(tag.spec.justification),
    )
    .ok()?;
    let placed = built.openings.first()?;
    let [dx, dy] = placed.dir;
    let half_width = placed.width / 2.;
    let half_depth = placed.thickness / 2.;
    Some(Path {
        closed: true,
        points: [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
            .map(|(along, across)| {
                [
                    placed.centre[0] + along * half_width * dx - across * half_depth * dy,
                    placed.centre[1] + along * half_width * dy + across * half_depth * dx,
                ]
            })
            .to_vec(),
    })
}
