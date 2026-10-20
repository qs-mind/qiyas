//! Every object of the drawing and of the session as a record: the entities of any sheet or
//! block, the drawing's tables (layers, linetypes, text and dimension styles, views, block
//! records), its layouts, the header, and Qiyas's own data (take-off, BOQ, conflicts, comments,
//! constraints). A record is `{"key", "type", …summary, "data"}`: `data` is the object as the
//! drawing library serialises it, so a JSON pointer into it (`/common/layer`, `/radius`) names
//! one property for `where`, `fields` and `Command::SetRecords`.
//!
//! Pages are small (20 by default, 200 at most) and every page stays under `MAX_BYTES`; a long
//! array inside a record shows its first `MAX_ITEMS` items and says how many it left out.

use std::collections::BTreeMap;

use opencadcodec::objects::ObjectType;
use opencadcodec::tables::normalize_name;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::query::Candidate;
use crate::session::variant_name;
use crate::{CoreError, EntityFilter, Session};

/// Records a page holds by default, and at most.
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 200;
/// A page's JSON stays under this many bytes: records past it go to the next page.
pub const MAX_BYTES: usize = 40_000;
/// Items of an array shown inside one record.
const MAX_ITEMS: usize = 100;

/// What `Session::records` reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Collection {
    /// The entities of a sheet (the one on screen by default) or of a block definition.
    #[default]
    Entities,
    Layers,
    Linetypes,
    TextStyles,
    DimStyles,
    /// Block definitions (block records): their entities are `entities` with `block`.
    Blocks,
    Layouts,
    /// The paper-space viewports of every layout (entities of type Viewport).
    Viewports,
    Groups,
    /// Named views.
    Views,
    /// The drawing's header variables, as one record.
    Header,
    LayerStates,
    Xrefs,
    /// Qiyas's take-off rows.
    Takeoff,
    /// The imported BOQ's items.
    Boq,
    Conflicts,
    /// Tables read off the sheets by the comparison.
    Schedules,
    Comments,
    Constraints,
    Parameters,
}

/// How much of each record a page gives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    /// The key, type and a few facts (an entity's layer, length, area, text).
    #[default]
    Summary,
    /// The summary and `data`, the whole object.
    Full,
}

/// One condition on a record's `data`: the value at the JSON pointer `path` compared with
/// `value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Where {
    /// A JSON pointer into `data` (`/common/layer`, `/radius`).
    pub path: String,
    #[serde(default)]
    pub op: WhereOp,
    #[serde(default)]
    pub value: Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WhereOp {
    /// Equal (text in any case).
    #[default]
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Text holding `value` (any case), or an array holding it.
    Contains,
    /// The path is there and not null (`value` true) or not (`value` false).
    Exists,
}

/// Which records of a collection to read, and how much of each.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RecordQuery {
    pub collection: Collection,
    /// Entities by handle.
    pub handles: Vec<u64>,
    /// Records by name or key, any case (layers, styles, blocks, layouts, take-off ids, BOQ codes).
    pub names: Vec<String>,
    /// Entities of this type ("Line", "LwPolyline", "Text", "Insert"), any case.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// Entities on this layer.
    pub layer: Option<String>,
    /// Entities of this sheet (its name or block); the sheet on screen when none.
    pub sheet: Option<String>,
    /// Entities inside this block definition instead of a sheet.
    pub block: Option<String>,
    /// Entities with a point inside `[[min_x, min_y], [max_x, max_y]]` (drawing units).
    pub bounds: Option<[[f64; 2]; 2]>,
    /// Every condition must hold.
    #[serde(rename = "where")]
    pub conditions: Vec<Where>,
    /// JSON pointers into `data`: each record gives the key, type and these only.
    pub fields: Vec<String>,
    pub detail: Detail,
    pub offset: usize,
    /// Records in the page: 20 when none, 200 at most.
    pub limit: Option<usize>,
}

/// One page of records.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecordPage {
    pub collection: Collection,
    /// Records that match, on every page.
    pub total: usize,
    pub offset: usize,
    pub records: Vec<Value>,
    /// Where the next page starts, when there is one.
    pub next_offset: Option<usize>,
    /// The page was cut short to stay under `MAX_BYTES`.
    pub truncated: bool,
}

impl Session {
    /// The records of `q.collection` that match `q`, one page.
    pub fn records(&self, q: &RecordQuery) -> Result<RecordPage, CoreError> {
        let detail = if q.fields.is_empty() {
            q.detail
        } else {
            Detail::Full
        };
        let all = match q.collection {
            Collection::Entities => self.entity_records(q, detail, None)?,
            Collection::Viewports => self.viewport_records(q, detail)?,
            other => self.other_records(other, detail)?,
        };
        let names: Vec<String> = q.names.iter().map(|n| normalize_name(n)).collect();
        let matching: Vec<Value> = all
            .into_iter()
            .filter(|r| names.is_empty() || key_matches(r, &names))
            .filter(|r| q.conditions.iter().all(|w| holds(r.get("data"), w)))
            .collect();
        let total = matching.len();
        let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let mut records = Vec::new();
        let mut bytes = 0;
        let mut truncated = false;
        for record in matching.into_iter().skip(q.offset).take(limit) {
            let record = shape(record, &q.fields, detail);
            let size = record.to_string().len();
            if bytes + size > MAX_BYTES && !records.is_empty() {
                truncated = true;
                break;
            }
            bytes += size;
            records.push(record);
        }
        let end = q.offset + records.len();
        Ok(RecordPage {
            collection: q.collection,
            total,
            offset: q.offset,
            next_offset: (end < total && !records.is_empty()).then_some(end),
            records,
            truncated,
        })
    }

    /// The entities of a sheet (or a block definition) as records; `only_type` keeps one type.
    fn entity_records(
        &self,
        q: &RecordQuery,
        detail: Detail,
        only_type: Option<&str>,
    ) -> Result<Vec<Value>, CoreError> {
        let kind = only_type.map(str::to_string).or_else(|| q.kind.clone());
        if let Some(block) = &q.block {
            let record = self
                .drawing
                .doc
                .block_records
                .get(block)
                .ok_or_else(|| invalid(format!("The drawing has no block {block}")))?;
            let name = record.name.clone();
            let layer = q.layer.as_deref().map(normalize_name);
            return Ok(self
                .drawing
                .entities_in(&name)
                .filter(|e| {
                    kind.as_deref()
                        .is_none_or(|k| variant_name(e).eq_ignore_ascii_case(k))
                })
                .filter(|e| {
                    layer
                        .as_ref()
                        .is_none_or(|l| *l == normalize_name(&e.common().layer))
                })
                .filter(|e| q.handles.is_empty() || q.handles.contains(&e.common().handle.value()))
                .map(|e| {
                    let mut r = json!({
                        "key": e.common().handle.value(),
                        "type": variant_name(e),
                        "layer": e.common().layer,
                        "block": name,
                    });
                    if detail == Detail::Full {
                        r["data"] = entity_data(e);
                    }
                    r
                })
                .collect());
        }
        let filter = EntityFilter {
            sheet: q.sheet.clone(),
            layer: q.layer.clone(),
            kind,
            within: q.bounds,
            limit: usize::MAX,
            ..EntityFilter::default()
        };
        let xrefs = self.xref_docs();
        let sheet = self.sheet_block(q.sheet.as_deref());
        Ok(self
            .matching(&filter, &xrefs)
            .into_iter()
            .filter(|c| {
                q.handles.is_empty() || q.handles.contains(&c.entity.common().handle.value())
            })
            .map(|c: Candidate| {
                let data = (detail == Detail::Full).then(|| entity_data(c.entity));
                let xref = c.xref.map(str::to_string);
                let chain = c.chain.clone();
                let row = self.entity_row(&xrefs, c);
                let mut r = serde_json::to_value(&row).unwrap_or_default();
                if let Some(obj) = r.as_object_mut() {
                    let handle = obj.remove("handle").unwrap_or_default();
                    obj.insert("key".into(), handle);
                    obj.insert("type".into(), obj.get("kind").cloned().unwrap_or_default());
                    obj.remove("kind");
                    obj.insert("sheet".into(), json!(sheet));
                    if let Some(xref) = xref {
                        obj.insert("xref".into(), json!(xref));
                        obj.insert("chain".into(), json!(chain));
                    }
                    if let Some(data) = data {
                        obj.insert("data".into(), data);
                    }
                }
                r
            })
            .collect())
    }

    /// The viewports of every layout (or of `q.sheet`), as entity records.
    fn viewport_records(&self, q: &RecordQuery, detail: Detail) -> Result<Vec<Value>, CoreError> {
        let sheets: Vec<String> = match &q.sheet {
            Some(sheet) => vec![sheet.clone()],
            None => self
                .drawing
                .sheets()
                .into_iter()
                .filter(|s| !s.is_model)
                .map(|s| s.block)
                .collect(),
        };
        let mut out = Vec::new();
        for sheet in sheets {
            let q = RecordQuery {
                sheet: Some(sheet),
                ..q.clone()
            };
            out.extend(self.entity_records(&q, detail, Some("Viewport"))?);
        }
        Ok(out)
    }

    /// Every record of a collection other than the entities.
    fn other_records(
        &self,
        collection: Collection,
        detail: Detail,
    ) -> Result<Vec<Value>, CoreError> {
        let doc = &self.drawing.doc;
        let full = detail == Detail::Full;
        let table = |key: &str, kind: &str, data: Value, summary: Value| -> Value {
            let mut r = json!({ "key": key, "type": kind });
            if let (Some(obj), Some(more)) = (r.as_object_mut(), summary.as_object()) {
                for (k, v) in more {
                    obj.insert(k.clone(), v.clone());
                }
            }
            if full {
                r["data"] = data;
            }
            r
        };
        let info = || self.info();
        Ok(match collection {
            Collection::Entities | Collection::Viewports => Vec::new(),
            Collection::Layers => doc
                .layers
                .iter()
                .map(|l| {
                    let hidden = self
                        .visibility
                        .hidden_layers
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(&l.name));
                    table(
                        &l.name,
                        "Layer",
                        to_value(l),
                        json!({
                            "color": l.color,
                            "linetype": l.line_type,
                            "lineweight": l.line_weight,
                            "on": !l.flags.off,
                            "frozen": l.flags.frozen,
                            "locked": l.flags.locked,
                            "plot": l.is_plottable,
                            "hidden_on_screen": hidden,
                        }),
                    )
                })
                .collect(),
            Collection::Linetypes => doc
                .line_types
                .iter()
                .map(|t| table(&t.name, "LineType", to_value(t), json!({})))
                .collect(),
            Collection::TextStyles => doc
                .text_styles
                .iter()
                .map(|t| table(&t.name, "TextStyle", to_value(t), json!({})))
                .collect(),
            Collection::DimStyles => doc
                .dim_styles
                .iter()
                .map(|t| table(&t.name, "DimStyle", to_value(t), json!({})))
                .collect(),
            Collection::Views => doc
                .views
                .iter()
                .map(|t| table(&t.name, "View", to_value(t), json!({})))
                .collect(),
            Collection::Blocks => doc
                .block_records
                .iter()
                .map(|b| {
                    let count = self.drawing.entities_in(&b.name).count();
                    table(
                        &b.name,
                        "BlockRecord",
                        to_value(b),
                        json!({ "entities": count, "layout": b.name.starts_with("*Paper_Space") || b.name.eq_ignore_ascii_case("*Model_Space") }),
                    )
                })
                .collect(),
            Collection::Layouts => {
                let mut layouts: Vec<Value> = doc
                    .objects
                    .values()
                    .filter_map(|o| match o {
                        ObjectType::Layout(l) => Some(table(
                            &l.name,
                            "Layout",
                            to_value(l),
                            json!({ "tab_order": l.tab_order }),
                        )),
                        _ => None,
                    })
                    .collect();
                layouts.sort_by_key(|r| r["tab_order"].as_i64().unwrap_or(0));
                layouts
            }
            Collection::Header => vec![table("header", "Header", to_value(&doc.header), json!({}))],
            Collection::Groups => list(info().groups, "Group", "name"),
            Collection::LayerStates => info()
                .layer_states
                .into_iter()
                .map(|name| {
                    let hidden = self.state_hidden.get(&name).cloned().unwrap_or_default();
                    json!({ "key": name, "type": "LayerState", "hidden_on_screen": hidden })
                })
                .collect(),
            Collection::Xrefs => list(info().xrefs, "Xref", "name"),
            Collection::Takeoff => list(self.takeoff.items.clone(), "Measurement", "id"),
            Collection::Boq => list(
                self.boq.as_ref().map(|b| b.items.clone()).unwrap_or_default(),
                "BoqItem",
                "code",
            ),
            Collection::Conflicts => list(self.conflicts.clone(), "Conflict", "id"),
            Collection::Schedules => list(self.schedules.clone(), "Schedule", "name"),
            Collection::Comments => list(self.list_comments(), "Comment", "id"),
            Collection::Constraints => list(self.list_constraints(), "Constraint", "id"),
            Collection::Parameters => list(self.list_parameters(), "Parameter", "name"),
        })
    }
}

/// Qiyas's own rows as records: the whole row is the summary and `data`.
fn list<T: Serialize>(rows: Vec<T>, kind: &str, key: &str) -> Vec<Value> {
    rows.into_iter()
        .map(|row| {
            let data = to_value(&row);
            let key_value = data.get(key).cloned().unwrap_or(Value::Null);
            let mut r = json!({ "key": key_value, "type": kind });
            if let (Some(obj), Some(fields)) = (r.as_object_mut(), data.as_object()) {
                for (k, v) in fields {
                    obj.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            r["data"] = data;
            r
        })
        .collect()
}

fn to_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// An entity's `data`: its own fields, without the variant around them (`{"Line": {…}}` gives
/// `{…}`).
pub fn entity_data(e: &opencadcodec::EntityType) -> Value {
    match to_value(e) {
        Value::Object(map) if map.len() == 1 => map
            .into_iter()
            .next()
            .map(|(_, v)| v)
            .unwrap_or(Value::Null),
        other => other,
    }
}

fn key_matches(record: &Value, names: &[String]) -> bool {
    let key = match record.get("key") {
        Some(Value::String(s)) => normalize_name(s),
        Some(other) => other.to_string(),
        None => return false,
    };
    names.contains(&key)
}

/// The record as the page gives it: with `fields`, the key, type and those values only; with
/// the summary, no `data`; long arrays cut to `MAX_ITEMS`.
fn shape(mut record: Value, fields: &[String], detail: Detail) -> Value {
    if !fields.is_empty() {
        let data = record.get("data").cloned().unwrap_or(Value::Null);
        let mut picked = BTreeMap::new();
        for f in fields {
            picked.insert(f.clone(), data.pointer(f).cloned().unwrap_or(Value::Null));
        }
        let mut out = json!({ "key": record["key"].clone(), "type": record["type"].clone() });
        for k in ["xref", "chain"] {
            if let Some(v) = record.get(k) {
                out[k] = v.clone();
            }
        }
        out["fields"] = json!(picked);
        return out;
    }
    if detail == Detail::Summary {
        if let Some(obj) = record.as_object_mut() {
            obj.remove("data");
        }
    }
    cut_arrays(&mut record);
    record
}

/// Arrays longer than `MAX_ITEMS` keep their first items and say how many they left out.
fn cut_arrays(value: &mut Value) {
    match value {
        Value::Array(items) => {
            if items.len() > MAX_ITEMS {
                let left = items.len() - MAX_ITEMS;
                items.truncate(MAX_ITEMS);
                items.push(json!(format!(
                    "… {left} more: read one with fields, e.g. \"/…/{MAX_ITEMS}\""
                )));
            }
            for v in items {
                cut_arrays(v);
            }
        }
        Value::Object(map) => {
            for v in map.values_mut() {
                cut_arrays(v);
            }
        }
        _ => {}
    }
}

/// Whether `data` meets the condition.
fn holds(data: Option<&Value>, w: &Where) -> bool {
    let found = data.and_then(|d| d.pointer(&w.path));
    if w.op == WhereOp::Exists {
        let there = found.is_some_and(|v| !v.is_null());
        return there == w.value.as_bool().unwrap_or(true);
    }
    let Some(found) = found else {
        return w.op == WhereOp::Ne;
    };
    match w.op {
        WhereOp::Eq => same(found, &w.value),
        WhereOp::Ne => !same(found, &w.value),
        WhereOp::Contains => match (found, &w.value) {
            (Value::String(s), Value::String(n)) => s.to_lowercase().contains(&n.to_lowercase()),
            (Value::Array(items), v) => items.iter().any(|i| same(i, v)),
            _ => false,
        },
        WhereOp::Lt | WhereOp::Le | WhereOp::Gt | WhereOp::Ge => {
            let ord = match (found, &w.value) {
                (Value::Number(a), Value::Number(b)) => a
                    .as_f64()
                    .zip(b.as_f64())
                    .and_then(|(a, b)| a.partial_cmp(&b)),
                (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
                _ => None,
            };
            ord.is_some_and(|o| match w.op {
                WhereOp::Lt => o.is_lt(),
                WhereOp::Le => o.is_le(),
                WhereOp::Gt => o.is_gt(),
                _ => o.is_ge(),
            })
        }
        WhereOp::Exists => unreachable!("handled above"),
    }
}

/// Equal, text in any case and numbers by value.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(x), Value::String(y)) => x.eq_ignore_ascii_case(y),
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}
