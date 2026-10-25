//! Measurements written into the drawing itself: one entity per measured shape on a `QIYAS-*`
//! layer, carrying the measurement in xdata, so the take-off survives a trip through another CAD
//! program and can be rebuilt when the project file is missing.
//!
//! Xdata record of app `QIYAS`, in order: `String(id)`, `String(kind)` ("length", "area",
//! "count", "volume" or "item"), `Real(value)` in drawing units, `String(description)`,
//! `String(layer)`, `String(sheet)`, `String(block_name or "")`; then, for a volume only,
//! `Real(height)` and, for an item only, `String(unit)`. A measurement of an entity inside blocks
//! adds a 9th value, `String(chain)`: the block-reference handles, outermost first, as decimal
//! numbers joined by commas; its 8th value is then `String("")` for a length, area or count. Readers
//! take legacy records of 7, 8 or 9 values. New records pad to 9 and append a tenth
//! JSON string with area_origin, measurement_basis, count_scope, note, source, validity and
//! an optional retained-room handle. Unavailable quantities use invisible record-only points.
//! Counts without locations use an invisible record-only point, never a counted location. Missing metadata
//! stays unspecified, never certified net. A count writes one point per counted
//! reference, each with the whole count's record. A volume is drawn like an area, on
//! `QIYAS-AREA`; an item with geometry is drawn like a length, on `QIYAS-ITEM` (a markup, an item
//! with unit "markup", on `QIYAS-MARKUP`), and an item without geometry is not written.
//!
//! Xrefs are read-only: a measurement of an xref's entities (`Measurement::xref`) is written from
//! its points only (a count's xref references are kept as points), never from the xref's handles,
//! and QIYAS entities inside an xref drawing are never read back.

use std::collections::HashMap;

use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{
    AppId, CadDocument, Color, EntityType, Handle, Layer, LwPolyline, Point, Vector2, Vector3,
};
use qiyas_geom::{flatten, object_to_world, Path, XrefDocs};

use crate::drawing::short_layer_name;
use crate::session::{effective_block_name, locate, measure_tol_of};
use crate::{Kind, Measurement, Units};

const APP: &str = "QIYAS";
/// The layer markups (items with unit "markup") are drawn on, and its colour index.
const MARKUP_LAYER: (&str, u8) = ("QIYAS-MARKUP", 1);

/// The layer and its colour index for each kind.
fn layer_of(kind: Kind) -> (&'static str, u8) {
    match kind {
        Kind::Length => ("QIYAS-LENGTH", 6),
        Kind::Area | Kind::Volume => ("QIYAS-AREA", 5),
        Kind::Count => ("QIYAS-COUNT", 4),
        Kind::Item => ("QIYAS-ITEM", 2),
    }
}

/// The layer a measurement is drawn on: its kind's, or the markup layer for a markup.
fn layer_for(m: &Measurement) -> &'static str {
    match m.kind {
        Kind::Item if m.unit == "markup" => MARKUP_LAYER.0,
        kind => layer_of(kind).0,
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Length => "length",
        Kind::Area => "area",
        Kind::Count => "count",
        Kind::Volume => "volume",
        Kind::Item => "item",
    }
}

/// Replaces the measurement entities in `doc` with `items`: removes every entity carrying `QIYAS`
/// xdata, then adds one polyline per path of a length, area, volume or item and one point per counted reference
/// and per stored point, on the measurement's sheet. A count reference that is not a block
/// reference is skipped, and so are the handles of a measurement of an xref's entities.
pub fn embed_measurements(doc: &mut CadDocument, items: &[Measurement]) {
    // `remove_entity` leaves the handle in its block's list; readers and writers skip such handles.
    let old: Vec<Handle> = doc
        .entities()
        .filter(|e| e.common().extended_data.get_record(APP).is_some())
        .map(|e| e.common().handle)
        .collect();
    for handle in old {
        doc.remove_entity(handle);
    }

    if !doc.app_ids.contains(APP) {
        let mut app = AppId::new(APP);
        app.handle = doc.allocate_handle();
        doc.app_ids.add(app).expect("checked it is missing");
    }
    // Volumes share the area layer.
    let layers = [Kind::Length, Kind::Area, Kind::Count, Kind::Item].map(layer_of);
    for (name, index) in layers.into_iter().chain([MARKUP_LAYER]) {
        if !doc.layers.contains(name) {
            let mut layer = Layer::new(name);
            layer.handle = doc.allocate_handle();
            layer.color = Color::Index(index);
            doc.layers.add(layer).expect("checked it is missing");
        }
    }

    // Shapes are worked out first: they read the document that is then added to.
    let mut shapes: Vec<(EntityType, &Measurement)> = Vec::new();
    for m in items {
        if !m.available() {
            let mut point = Point::new();
            point.common.invisible = true;
            shapes.push((EntityType::Point(point), m));
            continue;
        }
        match m.kind {
            Kind::Length | Kind::Area | Kind::Volume | Kind::Item => {
                for points in shape_paths(doc, m) {
                    let mut line = LwPolyline::from_points(
                        points.iter().map(|p| Vector2::new(p[0], p[1])).collect(),
                    );
                    line.is_closed = closed(m.kind);
                    shapes.push((EntityType::LwPolyline(line), m));
                }
            }
            Kind::Count => {
                let handles = if m.xref.is_some() {
                    &[][..]
                } else {
                    &m.handles[..]
                };
                let locations: Vec<Vector3> = handles
                    .iter()
                    .filter_map(|h| {
                        let located = locate(doc, &XrefDocs::new(), *h, &m.chain).ok()?;
                        let EntityType::Insert(insert) = located.entity else {
                            return None;
                        };
                        // The insert point is in the insert's own object coordinates.
                        let p = insert.insert_point;
                        let on_plan = object_to_world(&insert.normal, p.z).apply([p.x, p.y]);
                        let [x, y] = located.xf.apply(on_plan);
                        Some(Vector3::new(x, y, p.z))
                    })
                    .chain(m.points.iter().map(|p| Vector3::new(p[0], p[1], 0.0)))
                    .collect();
                if locations.is_empty() {
                    let mut point = Point::new();
                    point.common.invisible = true;
                    shapes.push((EntityType::Point(point), m));
                }
                for location in locations {
                    let mut point = Point::new();
                    point.location = location;
                    shapes.push((EntityType::Point(point), m));
                }
            }
        }
    }
    for (mut entity, m) in shapes {
        let room_boundary = m.handles.first().copied().filter(|h| {
            m.handles.len() == 1
                && m.chain.is_empty()
                && m.xref.is_none()
                && doc
                    .get_entity(Handle::new(*h))
                    .and_then(crate::object_regions::retained_region)
                    .is_some()
        });
        let owner = doc.block_records.get(&m.sheet).map(|b| b.handle);
        let common = entity.common_mut();
        common.layer = layer_for(m).to_string();
        if let Some(owner) = owner {
            common.owner_handle = owner;
        }
        let record_only = common.invisible;
        common
            .extended_data
            .add_record(record(m, record_only, room_boundary));
        doc.add_entity(entity)
            .expect("a new entity is always added");
    }
}

/// Whether the kind is drawn as closed rings: areas and volumes.
fn closed(kind: Kind) -> bool {
    matches!(kind, Kind::Area | Kind::Volume)
}

/// The picked points as one path, or for a measured entity the vertices of each of its paths
/// (each closed path, for an area or volume), placed by its chain of block references. Empty
/// when there is nothing to draw, and for an xref's entity without points.
fn shape_paths(doc: &CadDocument, m: &Measurement) -> Vec<Vec<[f64; 2]>> {
    if !m.points.is_empty() {
        return vec![m.points.clone()];
    }
    if m.xref.is_some() {
        return Vec::new();
    }
    let Some(located) = m
        .handles
        .first()
        .and_then(|h| locate(doc, &XrefDocs::new(), *h, &m.chain).ok())
    else {
        return Vec::new();
    };
    flatten(doc, located.entity, &located.xf, measure_tol_of(doc))
        .into_paths()
        .into_iter()
        .filter(|p| !closed(m.kind) || p.closed)
        .map(|p| p.points)
        .collect()
}

fn record(m: &Measurement, record_only: bool, room_boundary: Option<u64>) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(APP);
    for value in [
        XDataValue::String(m.id.clone()),
        XDataValue::String(kind_name(m.kind).to_string()),
        XDataValue::Real(if m.available() { m.value } else { 0.0 }),
        XDataValue::String(m.description.clone()),
        XDataValue::String(m.layer.clone()),
        XDataValue::String(m.sheet.clone()),
        XDataValue::String(m.block_name.clone().unwrap_or_default()),
    ] {
        record.add_value(value);
    }
    match m.kind {
        Kind::Volume => record.add_value(XDataValue::Real(m.height.unwrap_or_default())),
        Kind::Item => record.add_value(XDataValue::String(m.unit.clone())),
        Kind::Length | Kind::Area | Kind::Count if !m.chain.is_empty() => {
            record.add_value(XDataValue::String(String::new()))
        }
        Kind::Length | Kind::Area | Kind::Count => {}
    }
    if !m.chain.is_empty() {
        let chain: Vec<String> = m.chain.iter().map(u64::to_string).collect();
        record.add_value(XDataValue::String(chain.join(",")));
    }
    while record.values.len() < 9 {
        record.add_value(XDataValue::String(String::new()));
    }
    record.add_value(XDataValue::String(
        serde_json::json!({
            "area_origin": m.area_origin,
            "measurement_basis": m.measurement_basis,
            "count_scope": m.count_scope,
            "note": m.note,
            "source": m.source,
            "record_only": record_only,
            "quantity_valid": m.available(),
            // An excluded row retains its source and value so unmarking works after reopen.
            "view_only_row": m.view_only.then_some(m),
            "room_boundary": room_boundary,
        })
        .to_string(),
    ));
    record
}

/// Rebuilds the take-off from the entities carrying `QIYAS` xdata, in the order they were written
/// (by handle). Entities with the same id merge into one measurement with the stored value: a
/// length or area takes its points from its first polyline, a count gets one point per `Point`.
/// A measured entity with several paths therefore comes back with the first path only, and is
/// written back as that one polyline on the next save. Records that don't match the layout are
/// skipped. A description that is one of the defaults Qiyas gives ("Distance", "Area",
/// "<Kind> on <layer>", "<block> count") is marked as such.
pub fn read_embedded(doc: &CadDocument) -> Vec<Measurement> {
    let units = Units::from_header(doc.header.insertion_units);
    let mut entities: Vec<(&EntityType, &ExtendedDataRecord)> = doc
        .entities()
        .filter_map(|e| Some((e, e.common().extended_data.get_record(APP)?)))
        .collect();
    entities.sort_by_key(|(e, _)| e.common().handle.value());

    let mut items: Vec<Measurement> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (entity, record) in entities {
        let Some(m) = parse(record, units) else {
            continue;
        };
        let i = *index.entry(m.id.clone()).or_insert_with(|| {
            items.push(m);
            items.len() - 1
        });
        let m = &mut items[i];
        match entity {
            EntityType::Point(p) if m.kind == Kind::Count && !is_record_only(record) => {
                m.points.push([p.location.x, p.location.y]);
            }
            EntityType::LwPolyline(line) if m.kind != Kind::Count && m.points.is_empty() => {
                m.points = line
                    .vertices
                    .iter()
                    .map(|v| [v.location.x, v.location.y])
                    .collect();
                m.self_intersecting = closed(m.kind)
                    && Path {
                        points: m.points.clone(),
                        closed: true,
                    }
                    .self_intersects();
            }
            _ => {}
        }
    }
    for m in &mut items {
        m.auto_description = is_default_description(m);
        m.block_path = m
            .chain
            .iter()
            .filter_map(|h| match doc.get_entity(Handle::new(*h)) {
                Some(EntityType::Insert(insert)) => Some(effective_block_name(doc, insert)),
                _ => None,
            })
            .collect();
    }
    items
}

fn is_record_only(record: &ExtendedDataRecord) -> bool {
    let Some(XDataValue::String(text)) = record.values.get(9) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|meta| meta.get("record_only").and_then(|v| v.as_bool()))
        .unwrap_or(false)
}

/// True when the description is one Qiyas makes up for a new measurement.
fn is_default_description(m: &Measurement) -> bool {
    let d = m.description.as_str();
    // Older measurements carry the whole bound-xref layer name, newer ones the short one.
    let kind_on_layer = [m.layer.as_str(), short_layer_name(&m.layer)]
        .iter()
        .any(|layer| {
            d.strip_suffix(&format!(" on {layer}")).is_some_and(|kind| {
                !kind.is_empty() && kind.chars().all(|c| c.is_ascii_alphanumeric())
            })
        });
    let block_count = m
        .block_name
        .as_ref()
        .is_some_and(|b| d == format!("{b} count"));
    matches!(d, "Distance" | "Area") || kind_on_layer || block_count
}

/// A measurement from one record, made now by the user, with no points or handles yet.
fn parse(record: &ExtendedDataRecord, units: Units) -> Option<Measurement> {
    let values = record.values.as_slice();
    let (values, metadata) = if values.len() == 10 {
        let XDataValue::String(text) = &values[9] else {
            return None;
        };
        (
            &values[..9],
            Some(serde_json::from_str::<serde_json::Value>(text).ok()?),
        )
    } else {
        (values, None)
    };
    let (head, extra, chain) = match values.len() {
        7 => (values, None, None),
        8 => (&values[..7], Some(&values[7]), None),
        9 => (&values[..7], Some(&values[7]), Some(&values[8])),
        _ => return None,
    };
    let chain: Vec<u64> = match chain {
        None => Vec::new(),
        Some(XDataValue::String(list)) if list.is_empty() => Vec::new(),
        Some(XDataValue::String(list)) => list
            .split(',')
            .map(|h| h.trim().parse().ok())
            .collect::<Option<_>>()?,
        Some(_) => return None,
    };
    // A chain record pads the 8th value of kinds that have none.
    let extra = match extra {
        Some(XDataValue::String(s)) if s.is_empty() => None,
        other => other,
    };
    let [XDataValue::String(id), XDataValue::String(kind), XDataValue::Real(value), XDataValue::String(description), XDataValue::String(layer), XDataValue::String(sheet), XDataValue::String(block_name)] =
        head
    else {
        return None;
    };
    let kind = match (kind.as_str(), extra) {
        ("length", None) => Kind::Length,
        ("area", None) => Kind::Area,
        ("count", None) => Kind::Count,
        ("volume", Some(XDataValue::Real(_))) => Kind::Volume,
        ("item", Some(XDataValue::String(_))) => Kind::Item,
        _ => return None,
    };
    if let Some(row) = metadata
        .as_ref()
        .and_then(|meta| meta.get("view_only_row"))
        .filter(|row| !row.is_null())
    {
        return serde_json::from_value(row.clone()).ok();
    }
    let mut m = Measurement::new(
        kind,
        *value,
        units,
        description.clone(),
        layer.clone(),
        sheet.clone(),
    );
    m.id = id.clone();
    m.block_name = (!block_name.is_empty()).then(|| block_name.clone());
    if let Some(meta) = metadata {
        m.area_origin = serde_json::from_value(meta.get("area_origin")?.clone()).ok()?;
        m.measurement_basis =
            serde_json::from_value(meta.get("measurement_basis")?.clone()).ok()?;
        m.count_scope = serde_json::from_value(meta.get("count_scope")?.clone()).ok()?;
        if meta.get("quantity_valid").and_then(|v| v.as_bool()) == Some(false) {
            m.quantity_valid = false;
            m.value = f64::NAN;
            m.shown = f64::NAN;
        }
        if let Some(handle) = meta.get("room_boundary").and_then(|v| v.as_u64()) {
            m.handles = vec![handle];
        }
        if let Some(note) = meta.get("note") {
            m.note = serde_json::from_value(note.clone()).ok()?;
        }
        if let Some(source) = meta.get("source") {
            m.source = serde_json::from_value(source.clone()).ok()?;
        }
    }
    m.chain = chain;
    match extra {
        Some(XDataValue::Real(height)) => m.height = Some(*height),
        Some(XDataValue::String(unit)) => m.unit = unit.clone(),
        _ => {}
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_xref_description_is_a_default_whole_or_short() {
        let layer = "BASE-B$0$A-WALL".to_string();
        let m = |d: &str| {
            Measurement::new(
                Kind::Length,
                1.0,
                Units::Metres,
                d.into(),
                layer.clone(),
                "*Model_Space".into(),
            )
        };
        assert!(is_default_description(&m("Line on A-WALL")));
        assert!(is_default_description(&m("Line on BASE-B$0$A-WALL")));
        assert!(!is_default_description(&m("Wall on level 1")));
    }
}
