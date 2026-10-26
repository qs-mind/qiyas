//! Check the edited fields after all automatic drawing updates have finished.
use std::{collections::BTreeMap, sync::Arc};

use opencadcodec::{CadDocument, EntityType, Handle};
use serde_json::Value;

pub(crate) fn same_entity(a: &EntityType, b: &EntityType) -> bool {
    if a == b {
        return true;
    }
    let mut a = a.clone();
    let mut b = b.clone();
    for entity in [&mut a, &mut b] {
        entity.common_mut().raw_record = None;
        entity.common_mut().graphic_data = None;
    }
    a == b
}

fn editable_state(entity: &EntityType) -> Value {
    entity_state(entity, false)
}

pub(crate) fn geometry_state(entity: &EntityType) -> Value {
    entity_state(entity, true)
}

fn entity_state(entity: &EntityType, geometry_only: bool) -> Value {
    let mut state = serde_json::to_value(entity).expect("entity serialization");
    // Provenance and generated display caches are maintained by the derivation passes.
    // Object source properties are compared separately, as structured values.
    fn strip(value: &mut Value, geometry_only: bool) {
        match value {
            Value::Object(map) => {
                for key in ["raw_record", "graphic_data", "extended_data"] {
                    map.remove(key);
                }
                if geometry_only {
                    map.remove("common");
                }
                map.values_mut()
                    .for_each(|value| strip(value, geometry_only));
            }
            Value::Array(items) => items
                .iter_mut()
                .for_each(|value| strip(value, geometry_only)),
            _ => {}
        }
    }
    strip(&mut state, geometry_only);
    if let EntityType::LwPolyline(poly) = entity {
        if poly.is_closed
            && poly.vertices.len() >= 2
            && (geometry_only
                || crate::object_regions::retained_region(entity).is_some()
                || crate::xdata::read_json::<crate::objects::ObjectPart>(
                    entity.common(),
                    crate::objects::PART_APP,
                )
                .is_some())
        {
            // Computed closed rings may start at another vertex or run in the opposite
            // direction after retracing. Compare their complete edges, including arcs and
            // widths, while leaving ordinary editable polylines' vertex order significant.
            let mut edges = Vec::with_capacity(poly.vertices.len());
            for (i, a) in poly.vertices.iter().enumerate() {
                let b = &poly.vertices[(i + 1) % poly.vertices.len()];
                let start = [a.location.x, a.location.y];
                let end = [b.location.x, b.location.y];
                edges.push([
                    start[0],
                    start[1],
                    end[0],
                    end[1],
                    a.bulge,
                    a.start_width,
                    a.end_width,
                ]);
            }
            state["LwPolyline"]["vertices"] = serde_json::json!(edges);
        }
    }
    if let EntityType::Hatch(hatch) = entity {
        if geometry_only
            || crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            )
            .is_some()
        {
            for (i, path) in hatch.paths.iter().enumerate() {
                for (j, edge) in path.edges.iter().enumerate() {
                    let opencadcodec::entities::hatch::BoundaryEdge::Polyline(poly) = edge else {
                        continue;
                    };
                    if !poly.is_closed || poly.vertices.len() < 2 {
                        continue;
                    }
                    let edges: Vec<_> = poly
                        .vertices
                        .iter()
                        .enumerate()
                        .map(|(k, a)| {
                            let b = &poly.vertices[(k + 1) % poly.vertices.len()];
                            // Hatch polyline Z stores the arc bulge, not elevation.
                            [a.x, a.y, b.x, b.y, a.z, 0.0, 0.0]
                        })
                        .collect();
                    state["Hatch"]["paths"][i]["edges"][j]["Polyline"]["vertices"] =
                        serde_json::json!(edges);
                }
            }
        }
    }
    // Hosted frames store an angle in [0, 2π); ordinary transforms may store a negative
    // angle for exactly the same orientation. Compare those two representations equally.
    let frame = match entity {
        EntityType::Insert(insert) => Some(("Insert", "rotation", insert.rotation)),
        EntityType::Point(point) => Some(("Point", "x_axis_angle", point.x_axis_angle)),
        _ => None,
    };
    if let Some((kind, field, angle)) = frame {
        let angle = angle.rem_euclid(std::f64::consts::TAU);
        let angle = if angle < 1e-12 || std::f64::consts::TAU - angle < 1e-12 {
            0.0
        } else {
            angle
        };
        state[kind][field] = serde_json::json!(angle);
    }
    if let Some(object) = crate::objects::instance(entity).filter(|_| !geometry_only) {
        state["object_props"] = serde_json::json!(object.props);
        state["object_layers"] = serde_json::json!(object.layers);
        state["object_level"] = serde_json::json!(object.level);
    }
    state
}

pub(crate) fn same_geometry_state(a: &Value, b: &Value) -> bool {
    fn equal(a: &Value, b: &Value, path: &str) -> bool {
        if same_field(path, a, b) {
            return true;
        }
        match (a.as_object(), b.as_object()) {
            (Some(a), Some(b)) if a.len() == b.len() => a.iter().all(|(key, value)| {
                b.get(key)
                    .is_some_and(|other| equal(value, other, &format!("{path}/{key}")))
            }),
            _ => match (a.as_array(), b.as_array()) {
                (Some(a), Some(b)) if a.len() == b.len() => a
                    .iter()
                    .zip(b)
                    .enumerate()
                    .all(|(i, (a, b))| equal(a, b, &format!("{path}/{i}"))),
                _ => false,
            },
        }
    }
    equal(a, b, "")
}

pub(crate) fn same_value(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| same_value(value, other)))
        }
        _ => {
            (a.is_f64() || b.is_f64())
                && a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| {
                    (a - b).abs() <= 1e-10 + 16.0 * f64::EPSILON * a.abs().max(b.abs())
                })
        }
    }
}

fn same_field(path: &str, a: &Value, b: &Value) -> bool {
    if same_value(a, b) {
        return true;
    }
    if path == "/Hatch/paths" {
        let (Some(a), Some(b)) = (a.as_array(), b.as_array()) else {
            return false;
        };
        let computed = a.iter().any(|path| {
            path["edges"].as_array().is_some_and(|edges| {
                edges
                    .iter()
                    .any(|edge| edge["Polyline"]["vertices"][0].is_array())
            })
        });
        if !computed || a.len() != b.len() {
            return false;
        }
        let mut used = std::collections::BTreeSet::new();
        return a.iter().all(|before| {
            b.iter()
                .enumerate()
                .find(|(i, after)| !used.contains(i) && same_geometry_state(before, after))
                .is_some_and(|(i, _)| used.insert(i))
        });
    }
    if path != "/LwPolyline/vertices" && !path.ends_with("/Polyline/vertices") {
        return false;
    }
    let (Some(a), Some(b)) = (a.as_array(), b.as_array()) else {
        return false;
    };
    if a.len() != b.len() || a.is_empty() || !a[0].is_array() || !b[0].is_array() {
        return false;
    }
    let same_edge = |a: &Value, b: &Value, reverse: bool| {
        if !reverse {
            return same_value(a, b);
        }
        let Some(b) = b.as_array().filter(|edge| edge.len() == 7) else {
            return false;
        };
        let Some(bulge) = b[4].as_f64() else {
            return false;
        };
        same_value(
            a,
            &serde_json::json!([b[2], b[3], b[0], b[1], -bulge, b[6], b[5]]),
        )
    };
    // Cyclic order avoids sorting coordinates whose last few bits can change during
    // regeneration. Ordinary vertex objects never enter this comparison.
    (0..b.len()).any(|start| {
        [false, true].into_iter().any(|reverse| {
            same_edge(&a[0], &b[start], reverse)
                && a.iter().enumerate().all(|(i, edge)| {
                    let index = if reverse {
                        (start + b.len() - i) % b.len()
                    } else {
                        (start + i) % b.len()
                    };
                    same_edge(edge, &b[index], reverse)
                })
        })
    })
}

fn changes(before: &Value, after: &Value, path: &str, out: &mut Vec<(String, Value)>) {
    if same_field(path, before, after) {
        return;
    }
    if let (Some(before), Some(after)) = (before.as_object(), after.as_object()) {
        let keys: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
        for key in keys {
            let path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
            changes(
                before.get(key).unwrap_or(&Value::Null),
                after.get(key).unwrap_or(&Value::Null),
                &path,
                out,
            );
        }
    } else {
        out.push((path.into(), after.clone()));
    }
}

#[derive(Default)]
pub(crate) struct RequestedChanges {
    fields: BTreeMap<Handle, Vec<(String, Value)>>,
    existence: BTreeMap<Handle, bool>,
    overwritten_by: BTreeMap<(Handle, String), &'static str>,
}

impl RequestedChanges {
    pub(crate) fn capture(doc: &CadDocument, before: &[(Handle, Option<Arc<EntityType>>)]) -> Self {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("net_effect.capture");
        let mut requested = Self::default();
        for (handle, image) in before {
            let current = doc.get_entity(*handle);
            if image.is_some() != current.is_some() {
                let entity = current.or(image.as_deref()).expect("one state exists");
                let sheet_entity = doc.block_records.iter().any(|block| {
                    block.handle == entity.common().owner_handle
                        && (block.name.eq_ignore_ascii_case("*Model_Space")
                            || block.name.to_ascii_lowercase().starts_with("*paper_space"))
                });
                if sheet_entity {
                    requested.existence.insert(*handle, current.is_some());
                }
            }
            let (Some(before), Some(after)) = (image, current) else {
                continue;
            };
            if same_entity(before, after) {
                continue;
            }
            let mut fields = Vec::new();
            changes(
                &editable_state(before),
                &editable_state(after),
                "",
                &mut fields,
            );
            if !fields.is_empty() {
                requested.fields.insert(*handle, fields);
            }
        }
        requested
    }

    pub(crate) fn observe(&mut self, doc: &CadDocument, pass: &'static str) {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("net_effect.observe");
        for (&handle, &exists) in &self.existence {
            if doc.get_entity(handle).is_some() != exists {
                self.overwritten_by
                    .entry((handle, String::new()))
                    .or_insert(pass);
            } else {
                self.overwritten_by.remove(&(handle, String::new()));
            }
        }
        for (&handle, fields) in &self.fields {
            let state = doc
                .get_entity(handle)
                .map(editable_state)
                .unwrap_or(Value::Null);
            for (path, value) in fields {
                if !same_field(path, state.pointer(path).unwrap_or(&Value::Null), value) {
                    self.overwritten_by
                        .entry((handle, path.clone()))
                        .or_insert(pass);
                } else {
                    self.overwritten_by.remove(&(handle, path.clone()));
                }
            }
        }
    }

    pub(crate) fn advice(&self, doc: &CadDocument) -> Vec<Value> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("net_effect.advice");
        let mut advice = Vec::new();
        for (&handle, &exists) in &self.existence {
            if doc.get_entity(handle).is_some() != exists {
                let pass = self
                    .overwritten_by
                    .get(&(handle, String::new()))
                    .copied()
                    .unwrap_or("automatic drawing updates");
                advice.push(serde_json::json!({"code":"derived_value_changed","handle":handle.value(),"path":"/exists","requested":exists,"actual":doc.get_entity(handle).is_some(),"reason":pass,"message":format!("Entity {} was {} by {pass}. Edit its owning object to change this result.",handle.value(), if exists {"removed"} else {"restored"})}));
            }
        }
        for (&handle, fields) in &self.fields {
            let state = doc
                .get_entity(handle)
                .map(editable_state)
                .unwrap_or(Value::Null);
            for (path, value) in fields {
                if !same_field(path, state.pointer(path).unwrap_or(&Value::Null), value) {
                    let pass = self
                        .overwritten_by
                        .get(&(handle, path.clone()))
                        .copied()
                        .unwrap_or("automatic drawing updates");
                    advice.push(serde_json::json!({"code":"derived_value_changed","handle":handle.value(),"path":path,"requested":value,"actual":state.pointer(path).unwrap_or(&Value::Null),"reason":pass,"message":format!("Automatic updates changed entity {} ({path}): {pass}. Edit the owning object's properties, enclosing walls or schedule source to change this result.",handle.value())}));
                }
            }
        }
        advice
    }
}
