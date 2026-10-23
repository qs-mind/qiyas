//! Drawing-owned identity for exports and replay. This never resolves external resources.
use opencadcodec::{EntityType, Handle};
use qiyas_geom::{flatten, Path, Xform};

use crate::{objects, Drawing};

pub(crate) const REGION_TEXT_APP: &str = "QIYAS_REGION_TEXT";

pub struct ElementMetadata {
    /// World drawing coordinates. PDF drivers already include calibration and insertion.
    pub position: [f64; 2],
    pub label: Option<String>,
    /// Bounding-box diagonal in drawing units, derived only from source geometry.
    pub diagonal: f64,
}

impl Drawing {
    pub fn element_metadata(&self, handle: u64) -> Option<ElementMetadata> {
        let entity = self.doc.get_entity(Handle::from(handle))?;
        let mut paths = flatten(
            &self.doc,
            entity,
            &Xform::identity(),
            0.0001 / self.units.to_metres(),
        )
        .into_paths();
        if let Some(region) = crate::object_regions::retained_region(entity) {
            paths.extend(region.holes.into_iter().map(|points| Path {
                points,
                closed: true,
            }));
        }
        let position = match entity {
            EntityType::Insert(e) => Some(
                qiyas_geom::object_to_world(&e.normal, e.insert_point.z)
                    .apply([e.insert_point.x, e.insert_point.y]),
            ),
            EntityType::Point(e) => Some([e.location.x, e.location.y]),
            EntityType::Text(e) => Some(
                qiyas_geom::object_to_world(&e.normal, e.insertion_point.z)
                    .apply([e.insertion_point.x, e.insertion_point.y]),
            ),
            EntityType::MText(e) => Some([e.insertion_point.x, e.insertion_point.y]),
            _ => centre(&paths),
        }?;
        if !position.iter().all(|v| v.is_finite()) {
            return None;
        }
        // An explicitly owned label takes precedence over inferred enclosure text.
        let owned = self
            .doc
            .entities()
            .filter(|e| {
                crate::xdata::read_json::<objects::ObjectPart>(e.common(), objects::PART_APP)
                    .is_some_and(|p| p.owner == handle && p.role == "label")
            })
            .filter_map(text)
            .collect::<Vec<_>>();
        let label = unique(owned)
            .or_else(|| crate::xdata::read_text(entity.common(), REGION_TEXT_APP))
            .or_else(|| text(entity).map(|(_, text)| text))
            .or_else(|| {
                let object = objects::instance(entity)?;
                ["label", "name", "description"].iter().find_map(|key| {
                    object
                        .props
                        .get(*key)?
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_owned)
                })
            })
            .or_else(|| {
                let rings: Vec<_> = paths.iter().filter(|p| p.closed).collect();
                if rings.is_empty() {
                    return None;
                }
                let same_sheet = || {
                    self.doc
                        .entities()
                        .filter(|e| e.common().owner_handle == entity.common().owner_handle)
                };
                let pdf_text = |e: &&EntityType| {
                    matches!(e, EntityType::Text(_))
                        && crate::xdata::read_text(e.common(), crate::recognition::SOURCE_APP)
                            .is_some()
                };
                // Imported PDF glyph fragments form one label, using the same baseline/gap
                // rules as the calibrated PDF reader. Ordinary drawing notes stay separate.
                let labels = same_sheet()
                    .filter(|e| !pdf_text(e))
                    .filter_map(text)
                    .chain(
                        crate::recognition::text_runs(same_sheet().filter(pdf_text))
                            .into_iter()
                            .map(|r| (r.at, r.text)),
                    );
                let enclosed = labels
                    .filter(|(at, _)| {
                        rings.iter().filter(|p| inside(*at, &p.points)).count() % 2 == 1
                    })
                    .collect();
                if crate::xdata::read_text(entity.common(), crate::recognition::SOURCE_APP)
                    .is_some()
                {
                    nearest_label(position, enclosed)
                } else {
                    unique(enclosed)
                }
            });
        let mut lo = position;
        let mut hi = position;
        for point in paths.iter().flat_map(|p| &p.points) {
            for axis in 0..2 {
                lo[axis] = lo[axis].min(point[axis]);
                hi[axis] = hi[axis].max(point[axis]);
            }
        }
        let diagonal = (hi[0] - lo[0]).hypot(hi[1] - lo[1]);
        Some(ElementMetadata {
            position,
            label,
            diagonal,
        })
    }
}

fn text(entity: &EntityType) -> Option<([f64; 2], String)> {
    let (at, value) = match entity {
        EntityType::Text(e) => (
            qiyas_geom::object_to_world(&e.normal, e.insertion_point.z)
                .apply([e.insertion_point.x, e.insertion_point.y]),
            e.value.clone(),
        ),
        EntityType::MText(e) => (
            [e.insertion_point.x, e.insertion_point.y],
            opencadcodec::entities::mtext_format::parse_mtext(&e.value, true).to_plain_text(),
        ),
        _ => return None,
    };
    (!value.trim().is_empty()).then_some((at, value))
}

fn unique(labels: Vec<([f64; 2], String)>) -> Option<String> {
    // Several enclosed notes are ambiguous; do not select the nearest or invent a name.
    if labels.len() == 1 {
        labels.into_iter().next().map(|(_, label)| label)
    } else {
        None
    }
}

/// Select the nearest enclosed text. Equal-distance alternatives remain ambiguous.
pub(crate) fn nearest_label(at: [f64; 2], mut labels: Vec<([f64; 2], String)>) -> Option<String> {
    let distance = |p: [f64; 2]| (p[0] - at[0]).hypot(p[1] - at[1]);
    labels.sort_by(|a, b| distance(a.0).total_cmp(&distance(b.0)));
    labels.dedup();
    let first = labels.first()?;
    if labels
        .get(1)
        .is_some_and(|second| (distance(first.0) - distance(second.0)).abs() < 1e-9)
    {
        return None;
    }
    Some(first.1.clone())
}

pub(crate) fn inside(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    let mut result = false;
    for (a, b) in ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
    {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            result = !result;
        }
    }
    result
}

fn centre(paths: &[Path]) -> Option<[f64; 2]> {
    let mut area = 0.0;
    let mut moment = [0.0; 2];
    for path in paths.iter().filter(|p| p.closed) {
        let mut twice = 0.0;
        let mut sum = [0.0; 2];
        // Subtracting a local origin preserves precision at survey coordinates.
        let origin = *path.points.first()?;
        for (a, b) in path
            .points
            .iter()
            .zip(path.points.iter().cycle().skip(1))
            .take(path.points.len())
        {
            let a = [a[0] - origin[0], a[1] - origin[1]];
            let b = [b[0] - origin[0], b[1] - origin[1]];
            let cross = a[0] * b[1] - b[0] * a[1];
            twice += cross;
            for axis in 0..2 {
                sum[axis] += (a[axis] + b[axis]) * cross;
            }
        }
        if twice.abs() > f64::EPSILON {
            let depth = paths
                .iter()
                .filter(|other| {
                    other.closed
                        && !std::ptr::eq(*other, path)
                        && inside(path.points[0], &other.points)
                })
                .count();
            let weight = twice.abs() * if depth % 2 == 0 { 1.0 } else { -1.0 };
            for axis in 0..2 {
                moment[axis] += (origin[axis] + sum[axis] / (3.0 * twice)) * weight;
            }
            area += weight;
        }
    }
    if area > 0.0 {
        return Some(moment.map(|v| v / area));
    }
    let segments: Vec<_> = paths.iter().flat_map(|p| p.points.windows(2)).collect();
    let distance = |p: &&[[f64; 2]]| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]);
    let mut remaining = segments.iter().map(distance).sum::<f64>() * 0.5;
    for pair in segments {
        let length = distance(&pair);
        if length > 0.0 && remaining <= length {
            return Some(std::array::from_fn(|axis| {
                pair[0][axis] + (pair[1][axis] - pair[0][axis]) * remaining / length
            }));
        }
        remaining -= length;
    }
    paths.iter().find_map(|p| p.points.first().copied())
}
