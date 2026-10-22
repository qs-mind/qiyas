//! Drawing comparison and quantity-only variations. Recognition and measurement remain data.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use opencadcodec::{EntityType, Handle};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    knowledge::{self, Kind},
    measurement::GeneratedItem,
    CoreError, Session,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub handle: u64,
    pub sheet: String,
    pub kind: String,
    pub layer: String,
    pub block: Option<String>,
    pub bounds: Option<[[f64; 2]; 2]>,
    pub data: Value,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Matched,
    Added,
    Removed,
    Changed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityChange {
    /// Revision identity remains A's source handle even when B renumbers its entities.
    pub identity: String,
    pub kind: ChangeKind,
    pub before: Option<EntitySnapshot>,
    pub after: Option<EntitySnapshot>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetChanges {
    pub sheet: String,
    pub bounds: Option<[[f64; 2]; 2]>,
    pub entities: Vec<EntityChange>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub tolerance: f64,
    pub sheets: Vec<SheetChanges>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementDelta {
    pub identity: String,
    pub kind: ChangeKind,
    pub a_handles: Vec<u64>,
    pub b_handles: Vec<u64>,
    pub sheet: String,
    pub a_quantity: f64,
    pub b_quantity: f64,
    pub delta: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariationItem {
    pub code: String,
    pub description_en: String,
    pub description_ar: String,
    pub unit: String,
    pub a_quantity: f64,
    pub b_quantity: f64,
    pub delta: f64,
    pub elements: Vec<ElementDelta>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VariationReport {
    pub from_path: String,
    pub to_path: String,
    pub items: Vec<VariationItem>,
    pub comparison: Comparison,
    pub clouds: Vec<u64>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Baseline {
    path: String,
    entities: Vec<EntitySnapshot>,
    items: Vec<GeneratedItem>,
    warnings: Vec<String>,
    tolerance: f64,
}
fn invalid(s: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(s.into())
}

// Codec ownership, caches and Qiyas annotations are not drawing geometry. Keep all other
// entity fields, including attributes, so edits unsupported by the renderer still compare.
fn clean(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|k, _| {
                !k.contains("handle")
                    && !k.starts_with("raw_")
                    && !matches!(k.as_str(), "extended_data" | "reactors" | "graphics_cache")
            });
            fields.values_mut().for_each(clean);
        }
        Value::Array(values) => values.iter_mut().for_each(clean),
        _ => {}
    }
}
fn entity_data(s: &Session, e: &EntityType, depth: usize) -> Value {
    let mut data = serde_json::to_value(e).expect("entity serializes");
    clean(&mut data);
    if depth < 32 {
        if let EntityType::Insert(i) = e {
            let name = crate::session::effective_block_name(&s.drawing.doc, i);
            let children: Vec<_> = s
                .drawing
                .entities_in(&name)
                .map(|e| entity_data(s, e, depth + 1))
                .collect();
            data = json!({"entity":data,"definition":children});
        }
    }
    if e.common()
        .extended_data
        .get_record(crate::recognition::APP)
        .is_none()
    {
        if let Some(object) = crate::objects::instance(e) {
            data = json!({"entity":data,"object_type":object.type_name,"props":object.props,"level":object.level});
        }
    }
    data
}
fn equivalent(a: &Value, b: &Value, tolerance: f64) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            (a.as_f64().unwrap_or(0.) - b.as_f64().unwrap_or(0.)).abs() <= tolerance
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b, tolerance))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, a)| {
                    // Only coordinates and linear sizes use a drawing-unit tolerance. In
                    // particular, colour indices, flags, angles and scale factors are exact.
                    let linear = matches!(
                        key.as_str(),
                        "x" | "y"
                            | "z"
                            | "radius"
                            | "height"
                            | "width"
                            | "thickness"
                            | "elevation"
                            | "length"
                            | "offset"
                            | "dashes"
                    ) || [
                        "_radius",
                        "_height",
                        "_width",
                        "_length",
                        "_spacing",
                        "_distance",
                        "_offset",
                        "_elevation",
                    ]
                    .iter()
                    .any(|suffix| key.ends_with(suffix));
                    let numeric = a.is_number()
                        || a.as_array().is_some_and(|v| v.iter().any(Value::is_number));
                    let exact = (numeric && !linear)
                        || matches!(
                            key.as_str(),
                            "normal"
                                | "extrusion"
                                | "extrusion_direction"
                                | "direction"
                                | "common"
                                | "props"
                        );
                    b.get(key)
                        .is_some_and(|b| equivalent(a, b, if exact { 0. } else { tolerance }))
                })
        }
        _ => a == b,
    }
}
fn union(a: Option<[[f64; 2]; 2]>, b: Option<[[f64; 2]; 2]>) -> Option<[[f64; 2]; 2]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            [a[0][0].min(b[0][0]), a[0][1].min(b[0][1])],
            [a[1][0].max(b[1][0]), a[1][1].max(b[1][1])],
        ]),
        (a, b) => a.or(b),
    }
}
fn compatible(a: &EntitySnapshot, b: &EntitySnapshot) -> bool {
    a.sheet == b.sheet && a.kind == b.kind
}
fn proximity(a: &EntitySnapshot, b: &EntitySnapshot, tolerance: f64) -> Option<f64> {
    if !compatible(a, b) || a.layer != b.layer || a.block != b.block {
        return None;
    }
    let (a, b) = (a.bounds?, b.bounds?);
    if (0..2).any(|i| a[1][i] + tolerance < b[0][i] || b[1][i] + tolerance < a[0][i]) {
        return None;
    }
    Some(
        (0..2)
            .flat_map(|i| (0..2).map(move |j| (a[i][j] - b[i][j]).abs()))
            .sum(),
    )
}
pub fn compare(a: &[EntitySnapshot], b: &[EntitySnapshot], tolerance: f64) -> Comparison {
    let mut pairs = BTreeMap::new();
    let mut used = BTreeSet::new();
    // Reserve exact geometry before trusting changed handles: exports can reuse A's handles.
    for (i, left) in a.iter().enumerate() {
        let right = b
            .iter()
            .enumerate()
            .filter(|(j, r)| {
                !used.contains(j)
                    && compatible(left, r)
                    && equivalent(&left.data, &r.data, tolerance)
            })
            .min_by_key(|(j, r)| (r.handle != left.handle, *j));
        if let Some((j, _)) = right {
            pairs.insert(i, j);
            used.insert(j);
        }
    }
    let renumbered = pairs.iter().any(|(i, j)| a[*i].handle != b[*j].handle);
    if !renumbered {
        for (i, left) in a
            .iter()
            .enumerate()
            .filter(|(i, _)| !pairs.contains_key(i))
            .collect::<Vec<_>>()
        {
            if let Some((j, _)) = b
                .iter()
                .enumerate()
                .find(|(j, r)| !used.contains(j) && left.handle == r.handle && compatible(left, r))
            {
                pairs.insert(i, j);
                used.insert(j);
            }
        }
    }
    // Unmatched exports use unique mutual nearest geometry in the same layer/block. Ties
    // remain additions/removals; inventing identity would conceal ambiguous duplicated work.
    let remaining: Vec<_> = (0..a.len()).filter(|i| !pairs.contains_key(i)).collect();
    for i in remaining {
        let mut candidates: Vec<_> = b
            .iter()
            .enumerate()
            .filter(|(j, _)| !used.contains(j))
            .filter_map(|(j, r)| proximity(&a[i], r, tolerance).map(|d| (j, d)))
            .collect();
        candidates.sort_by(|x, y| x.1.total_cmp(&y.1));
        let Some(&(j, d)) = candidates.first() else {
            continue;
        };
        if candidates
            .get(1)
            .is_some_and(|x| (x.1 - d).abs() <= tolerance)
        {
            continue;
        }
        if a.iter().enumerate().any(|(k, l)| {
            k != i
                && !pairs.contains_key(&k)
                && proximity(l, &b[j], tolerance).is_some_and(|other| other <= d + tolerance)
        }) {
            continue;
        }
        pairs.insert(i, j);
        used.insert(j);
    }
    let mut sheets = BTreeMap::<String, SheetChanges>::new();
    let mut push = |before: Option<EntitySnapshot>, after: Option<EntitySnapshot>| {
        let entity = after.as_ref().or(before.as_ref()).expect("one side");
        let kind = match (&before, &after) {
            (Some(a), Some(b)) if equivalent(&a.data, &b.data, tolerance) => ChangeKind::Matched,
            (Some(_), Some(_)) => ChangeKind::Changed,
            (Some(_), None) => ChangeKind::Removed,
            _ => ChangeKind::Added,
        };
        let sheet = sheets
            .entry(entity.sheet.clone())
            .or_insert_with(|| SheetChanges {
                sheet: entity.sheet.clone(),
                bounds: None,
                entities: vec![],
            });
        if kind != ChangeKind::Matched {
            sheet.bounds = union(
                sheet.bounds,
                union(
                    before.as_ref().and_then(|e| e.bounds),
                    after.as_ref().and_then(|e| e.bounds),
                ),
            );
        }
        let identity = before
            .as_ref()
            .map(|e| format!("A:{:X}", e.handle))
            .unwrap_or_else(|| format!("B:{:X}", entity.handle));
        sheet.entities.push(EntityChange {
            identity,
            kind,
            before,
            after,
        });
    };
    for (i, e) in a.iter().enumerate() {
        push(Some(e.clone()), pairs.get(&i).map(|j| b[*j].clone()));
    }
    for (j, e) in b.iter().enumerate().filter(|(j, _)| !used.contains(j)) {
        let _ = j;
        push(None, Some(e.clone()));
    }
    Comparison {
        tolerance,
        sheets: sheets.into_values().collect(),
    }
}
impl Session {
    pub fn revision_identity(&self, handle: u64) -> String {
        knowledge::get(&self.drawing.doc, Kind::Revisions, "identities")
            .ok()
            .flatten()
            .and_then(|e| {
                e.data
                    .get(handle.to_string())
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("{handle:X}"))
    }
    pub fn revision_snapshot(&self) -> Vec<EntitySnapshot> {
        let xrefs = self.xref_docs();
        let mut out = vec![];
        for sheet in self.drawing.sheets() {
            for e in self.drawing.entities_in(&sheet.block) {
                if [crate::comment::COMMENT_APP, crate::objects::PART_APP]
                    .iter()
                    .any(|app| e.common().extended_data.get_record(app).is_some())
                {
                    continue;
                }
                let c = crate::query::Candidate {
                    entity: e,
                    doc: &self.drawing.doc,
                    layer: std::borrow::Cow::Borrowed(&e.common().layer),
                    xf: qiyas_geom::Xform::identity(),
                    chain: vec![],
                    xref: None,
                };
                let points = self.points_of(&xrefs, &c);
                out.push(EntitySnapshot {
                    handle: e.common().handle.value(),
                    sheet: sheet.name.clone(),
                    kind: crate::session::variant_name(e),
                    layer: e.common().layer.clone(),
                    block: if let EntityType::Insert(i) = e {
                        Some(crate::session::effective_block_name(&self.drawing.doc, i))
                    } else {
                        None
                    },
                    bounds: (!points.is_empty()).then(|| crate::query::bounds(&points)),
                    data: entity_data(self, e, 0),
                });
            }
        }
        out.sort_by_key(|e| e.handle);
        out
    }
    fn revision_source(&self, path: &str) -> Result<Session, CoreError> {
        let path = Path::new(path);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.drawing
                .path
                .parent()
                .unwrap_or(Path::new("."))
                .join(path)
        };
        let other = Session::open_standalone(&path)?;
        if other.drawing.units != self.drawing.units {
            return Err(invalid(
                "Revision drawings must use the same drawing units; convert the revision first",
            ));
        }
        Ok(other)
    }
    fn revision_tolerance(&self, value: Option<f64>) -> Result<f64, CoreError> {
        let t = value.unwrap_or_else(|| self.measure_tol());
        if !t.is_finite() || t < 0. {
            return Err(invalid(
                "Revision tolerance must be finite and nonnegative, in drawing units",
            ));
        }
        Ok(t)
    }
    pub fn compare_revisions(
        &self,
        path: &str,
        tolerance: Option<f64>,
    ) -> Result<Comparison, CoreError> {
        let other = self.revision_source(path)?;
        Ok(compare(
            &other.revision_snapshot(),
            &self.revision_snapshot(),
            self.revision_tolerance(tolerance)?,
        ))
    }
    pub(crate) fn carry_knowledge(
        &mut self,
        path: &str,
        tolerance: Option<f64>,
    ) -> Result<Value, CoreError> {
        let mut a = self.revision_source(path)?;
        a.refresh_measurement()?;
        let tolerance = self.revision_tolerance(tolerance)?;
        let entities = a.revision_snapshot();
        let comparison = compare(&entities, &self.revision_snapshot(), tolerance);
        let handle_map: BTreeMap<_, _> = comparison
            .sheets
            .iter()
            .flat_map(|s| &s.entities)
            .filter_map(|c| Some((c.before.as_ref()?.handle, c.after.as_ref()?.handle)))
            .collect();
        for kind in Kind::ALL
            .into_iter()
            .filter(|k| !matches!(k, Kind::Queries | Kind::Revisions))
        {
            for entry in knowledge::list(&a.drawing.doc, kind)? {
                if kind == Kind::Measurement && entry.name == "result" {
                    continue;
                }
                let mut data = entry.data;
                remap(&mut data, "", &handle_map);
                knowledge::put(&mut self.drawing.doc, kind, &entry.name, &data)?;
            }
        }
        // Discard previous recognition decisions, then extract properties again from B.
        let recognized: Vec<_> = self
            .recognition_evidence()?
            .iter()
            .map(|c| c.handle)
            .collect();
        for h in recognized {
            for app in [crate::recognition::APP, crate::objects::OBJECT_APP] {
                let app_handle = crate::xdata::app_handle(&self.drawing.doc, app);
                if let Some(e) = self.drawing.doc.get_entity_mut(Handle::new(h)) {
                    crate::xdata::write_values(e.common_mut(), app, app_handle, None);
                }
            }
        }
        let original = self.sheet.clone();
        let result = (|| {
            for sheet in self.drawing.sheets() {
                self.set_sheet(&sheet.block)?;
                self.run_recognition(crate::recognition::RecognitionCommand::Apply {
                    rules: vec![],
                })?;
            }
            // Authored instances have no recognition decision. Carry their supplied properties
            // only when B does not already provide an authored or newly recognised instance.
            for (old, new) in &handle_map {
                let Some(mut object) = a
                    .drawing
                    .doc
                    .get_entity(Handle::new(*old))
                    .and_then(crate::objects::instance)
                else {
                    continue;
                };
                if a.drawing
                    .doc
                    .get_entity(Handle::new(*old))
                    .is_some_and(|e| {
                        e.common()
                            .extended_data
                            .get_record(crate::recognition::APP)
                            .is_some()
                    })
                {
                    continue;
                }
                if self
                    .drawing
                    .doc
                    .get_entity(Handle::new(*new))
                    .and_then(crate::objects::instance)
                    .is_none()
                {
                    object.id = *new;
                    object.host = object.host.and_then(|h| handle_map.get(&h).copied());
                    self.stamp_object(*new, &object)?;
                }
            }
            let report = a.measurement_report()?;
            let baseline = Baseline {
                path: a.drawing.path.to_string_lossy().into(),
                entities,
                items: report.items,
                warnings: report.warnings,
                tolerance,
            };
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Revisions,
                "baseline",
                &json!(baseline),
            )?;
            let identities: BTreeMap<_, _> = self
                .revision_snapshot()
                .iter()
                .map(|e| {
                    let identity = handle_map
                        .iter()
                        .find(|(_, new)| **new == e.handle)
                        .map(|(old, _)| a.revision_identity(*old))
                        .unwrap_or_else(|| format!("B:{:X}", e.handle));
                    (e.handle.to_string(), identity)
                })
                .collect();
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Revisions,
                "identities",
                &json!(identities),
            )?;
            Ok(json!({"comparison":comparison,"matched_elements":handle_map.len()}))
        })();
        self.set_sheet(&original)?;
        result
    }
    pub fn variation(&self) -> Result<Option<VariationReport>, CoreError> {
        knowledge::get(&self.drawing.doc, Kind::Revisions, "result")?
            .map(|e| serde_json::from_value(e.data).map_err(|e| invalid(e.to_string())))
            .transpose()
    }
    pub(crate) fn variation_report(
        &mut self,
        layer: Option<String>,
    ) -> Result<VariationReport, CoreError> {
        let entry = knowledge::get(&self.drawing.doc, Kind::Revisions, "baseline")?
            .ok_or_else(|| invalid("Run carry_knowledge from revision A first"))?;
        let base: Baseline =
            serde_json::from_value(entry.data).map_err(|e| invalid(e.to_string()))?;
        self.refresh_measurement()?;
        let current = self.measurement_report()?;
        let comparison = compare(&base.entities, &self.revision_snapshot(), base.tolerance);
        let mut report = build_variation(&base.items, &current.items, comparison);
        report.from_path = base.path;
        report.to_path = self.drawing.path.to_string_lossy().into();
        report.warnings = base.warnings;
        report.warnings.extend(current.warnings);
        let layer = layer
            .or(
                knowledge::get(&self.drawing.doc, Kind::Revisions, "cloud_layer")?
                    .and_then(|e| e.data.as_str().map(str::to_owned)),
            )
            .unwrap_or_else(|| "VARIATION".into());
        if layer.trim().is_empty() {
            return Err(invalid("Give a variation cloud layer name"));
        }
        let old = self.variation()?.map(|r| r.clouds).unwrap_or_default();
        for c in self
            .list_comments()
            .into_iter()
            .filter(|c| old.contains(&c.handle))
        {
            for h in std::iter::once(c.handle).chain(c.label) {
                self.drawing.doc.remove_entity(Handle::new(h));
            }
        }
        self.ensure_layer(&layer, opencadcodec::Color::Index(1));
        let original = self.sheet.clone();
        let result = (|| {
            for sheet in &report.comparison.sheets {
                let Some(target) = self
                    .drawing
                    .sheets()
                    .into_iter()
                    .find(|s| s.name == sheet.sheet)
                else {
                    report.warnings.push(format!(
                        "Removed sheet {} has no sheet in B for clouds",
                        sheet.sheet
                    ));
                    continue;
                };
                self.set_sheet(&target.block)?;
                for change in sheet
                    .entities
                    .iter()
                    .filter(|c| c.kind != ChangeKind::Matched)
                {
                    let Some([[x0, y0], [x1, y1]]) = union(
                        change.before.as_ref().and_then(|e| e.bounds),
                        change.after.as_ref().and_then(|e| e.bounds),
                    ) else {
                        continue;
                    };
                    let pad = self.default_text_height().max(base.tolerance).max(1e-6);
                    let text = format!("Variation / التغيير {} {:?}", change.identity, change.kind);
                    let handles = self.add_comment(
                        vec![
                            [x0 - pad, y0 - pad],
                            [x1 + pad, y0 - pad],
                            [x1 + pad, y1 + pad],
                            [x0 - pad, y1 + pad],
                        ],
                        &text,
                        None,
                    )?;
                    for h in &handles {
                        let e = self
                            .drawing
                            .doc
                            .get_entity_mut(Handle::new(*h))
                            .expect("comment added");
                        e.common_mut().layer = layer.clone();
                    }
                    report.clouds.push(handles[0]);
                }
            }
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Revisions,
                "cloud_layer",
                &json!(layer),
            )?;
            knowledge::put(
                &mut self.drawing.doc,
                Kind::Revisions,
                "result",
                &json!(report),
            )?;
            Ok(report)
        })();
        self.set_sheet(&original)?;
        result
    }
}
fn remap(v: &mut Value, key: &str, map: &BTreeMap<u64, u64>) {
    match v {
        Value::Object(fields) => {
            for (k, v) in fields {
                remap(v, k, map)
            }
        }
        Value::Array(values) => {
            for v in values {
                remap(v, key, map)
            }
        }
        Value::Number(n) if matches!(key, "handle" | "handles" | "driver" | "host" | "targets") => {
            if let Some(h) = n.as_u64() {
                *v = json!(map.get(&h).copied().unwrap_or(0));
            }
        }
        _ => {}
    }
}
fn contributions(item: Option<&GeneratedItem>) -> BTreeMap<(String, Vec<u64>), f64> {
    let mut out = BTreeMap::new();
    if let Some(item) = item {
        for line in item.lines.iter().filter(|l| l.included) {
            let sheet = line
                .sources
                .first()
                .map(|s| s.sheet.clone())
                .unwrap_or_default();
            let mut handles: Vec<_> = line.sources.iter().map(|s| s.handle).collect();
            handles.sort();
            handles.dedup();
            *out.entry((sheet, handles)).or_default() += line.value;
        }
    }
    out
}
/// Pair dimension contributions by source identity, retaining all handles for deductions.
pub fn build_variation(
    a: &[GeneratedItem],
    b: &[GeneratedItem],
    comparison: Comparison,
) -> VariationReport {
    let changes: Vec<_> = comparison.sheets.iter().flat_map(|s| &s.entities).collect();
    let map: BTreeMap<_, _> = changes
        .iter()
        .filter_map(|c| Some((c.before.as_ref()?.handle, c.after.as_ref()?.handle)))
        .collect();
    let keys: BTreeSet<_> = a.iter().chain(b).map(|i| (&i.code, &i.unit)).collect();
    let mut items = vec![];
    for (code, unit) in keys {
        let left = a.iter().find(|i| i.code == *code && i.unit == *unit);
        let right = b.iter().find(|i| i.code == *code && i.unit == *unit);
        let item = right.or(left).expect("item key");
        let (aq, bq) = (
            left.map_or(0., |i| i.quantity),
            right.map_or(0., |i| i.quantity),
        );
        let mut after = contributions(right);
        let mut elements = vec![];
        for ((sheet, handles), qty) in contributions(left) {
            let mapped: Option<Vec<_>> = handles.iter().map(|h| map.get(h).copied()).collect();
            let mut mapped = mapped.unwrap_or_default();
            mapped.sort();
            // Sheet references are block names in dimension lines; identity is per handle.
            let key = after
                .keys()
                .find(|(_, h)| !mapped.is_empty() && *h == mapped)
                .cloned();
            let bqty = key.as_ref().and_then(|k| after.remove(k)).unwrap_or(0.);
            if qty != bqty {
                elements.push(ElementDelta {
                    identity: format!(
                        "A:{}",
                        handles
                            .iter()
                            .map(|h| format!("{h:X}"))
                            .collect::<Vec<_>>()
                            .join("+")
                    ),
                    kind: if key.is_some() {
                        ChangeKind::Changed
                    } else {
                        ChangeKind::Removed
                    },
                    a_handles: handles,
                    b_handles: if key.is_some() { mapped } else { vec![] },
                    sheet,
                    a_quantity: qty,
                    b_quantity: bqty,
                    delta: bqty - qty,
                });
            }
        }
        for ((sheet, handles), qty) in after {
            if qty != 0. {
                elements.push(ElementDelta {
                    identity: format!(
                        "B:{}",
                        handles
                            .iter()
                            .map(|h| format!("{h:X}"))
                            .collect::<Vec<_>>()
                            .join("+")
                    ),
                    kind: ChangeKind::Added,
                    a_handles: vec![],
                    b_handles: handles,
                    sheet,
                    a_quantity: 0.,
                    b_quantity: qty,
                    delta: qty,
                });
            }
        }
        items.push(VariationItem {
            code: code.clone(),
            description_en: item.description_en.clone(),
            description_ar: item.description_ar.clone(),
            unit: unit.clone(),
            a_quantity: aq,
            b_quantity: bq,
            delta: bq - aq,
            elements,
        });
    }
    VariationReport {
        items,
        comparison,
        ..Default::default()
    }
}
