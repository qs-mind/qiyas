//! Queries, sheet units and file operations used by the generic surface.

use crate::{ToolError, ToolOutput};
use qiyas_core::{EntityFilter, Session, Source};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

const DEFAULT_LIMIT: usize = 100;
const MAX_OUTPUT: usize = 40_960;
const DECIMALS: f64 = 1e6;
const OUTSIDE_FOLDER: &str = "Only files beside the open drawing can be opened by the assistant; \
                              use the BOQ tab's Import for other files";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindInput {
    sheet: Option<String>,
    layer: Option<String>,
    kind: Option<String>,
    block_name: Option<String>,
    text_contains: Option<String>,
    within: Option<[[f64; 2]; 2]>,
    offset: Option<usize>,
    limit: Option<usize>,
}

pub(crate) fn layers(session: &Session, offset: usize) -> Value {
    let hidden: HashSet<String> = session
        .hidden_layers()
        .iter()
        .map(|n| n.to_uppercase())
        .collect();
    let rows: Vec<Value> = session
        .layers()
        .into_iter()
        .map(|l| {
            let [r, g, b] = l.color;
            json!({
                "name": l.name,
                "color": format!("#{r:02X}{g:02X}{b:02X}"),
                "off": l.off,
                "frozen": l.frozen,
                "locked": l.locked,
                "hidden": hidden.contains(&l.name.to_uppercase()),
            })
        })
        .collect();
    page(rows, offset)
}

pub(crate) fn entities(session: &Session, args: &Value) -> Result<Value, ToolError> {
    let input: FindInput = serde_json::from_value(args.clone()).map_err(|e| bad(&e.to_string()))?;
    let sheet = match input.sheet {
        Some(name) => Some(sheet_block(session, Some(&name))?),
        None => None,
    };
    let coordinates = units_of(session, sheet.as_deref().unwrap_or(&session.sheet));
    let page = session.find_entities(&EntityFilter {
        sheet,
        layer: input.layer,
        kind: input.kind,
        block_name: input.block_name,
        text_contains: input.text_contains,
        within: input.within,
        offset: input.offset.unwrap_or(0),
        limit: input.limit.unwrap_or(DEFAULT_LIMIT),
    });
    Ok(json!({
        "rows": page.rows,
        "total": page.total,
        "next_offset": page.next_offset,
        "units": coordinates,
    }))
}

/// Reads the workbook at `path` and makes it the session's BOQ (an undo step). Returns a summary,
/// not the items: records pages through those.
pub(crate) fn import_boq(
    session: &mut Session,
    path: &str,
    source: Source,
) -> Result<Value, ToolError> {
    let path = resolve_in_drawing_folder(session, path)?;
    // A wrong path or a sheet with no header is the model's to fix, so both are bad input.
    let boq = qiyas_boq::import_boq(&path).map_err(|e| ToolError::BadInput(e.to_string()))?;
    let mut sections: Vec<&str> = Vec::new();
    for item in &boq.items {
        if !item.section.is_empty() && sections.last() != Some(&item.section.as_str()) {
            sections.push(&item.section);
        }
    }
    let mut out = json!({
        "source": boq.source,
        "items": boq.items.len(),
        "skipped_rows": boq.skipped_rows,
        "sections": sections,
    });
    let report = session.run_input(&json!({"type":"set_boq", "boq":boq}), source)?;
    append_metadata(&mut out, &report);
    Ok(out)
}

/// `path` (relative to the open drawing's folder, or absolute) as a real path inside that folder
/// or one of its subfolders, links followed. Every tool that takes a path goes through this, so
/// the model, or an MCP client, can't read files elsewhere on the computer.
pub(crate) fn resolve_in_drawing_folder(
    session: &Session,
    path: &str,
) -> Result<PathBuf, ToolError> {
    // The drawing's folder, or Documents for an untitled drawing: one place for every export.
    let folder = session
        .export_folder()
        .canonicalize()
        .map_err(|e| ToolError::Failed(format!("The drawing's folder can't be read: {e}")))?;
    let full = folder.join(path);
    // Resolve the existing parent too: a new export inside a symlink must stay in the folder.
    let mut ancestor = full.as_path();
    let mut suffix = Vec::new();
    let real = loop {
        if let Ok(mut real) = ancestor.canonicalize() {
            for part in suffix.iter().rev() {
                real.push(part);
            }
            break lexical(&real);
        }
        let name = ancestor.file_name().ok_or_else(|| bad(OUTSIDE_FOLDER))?;
        suffix.push(name);
        ancestor = ancestor.parent().ok_or_else(|| bad(OUTSIDE_FOLDER))?;
    };
    if real.starts_with(&folder) {
        Ok(real)
    } else {
        Err(bad(OUTSIDE_FOLDER))
    }
}

/// `path` with `.` and `..` worked out from its text alone.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `value` with every non-integer number rounded to 6 decimals, so the model reads 22.09, not
/// 22.089999999999996. Integers (handles, counts) are left as they are.
fn rounded(value: Value) -> Value {
    match value {
        Value::Number(n) if n.is_f64() => {
            let x = n.as_f64().unwrap_or_default();
            let r = (x * DECIMALS).round() / DECIMALS;
            // Too large to scale safely: as it is.
            if !r.is_finite() || x.abs() >= 1e9 {
                return Value::Number(n);
            }
            serde_json::Number::from_f64(r).map_or(Value::Number(n), Value::Number)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(rounded).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, rounded(v))).collect())
        }
        other => other,
    }
}

/// The block of the sheet named by its name or block name (any case); the current sheet when
/// `None`.
pub(crate) fn sheet_block(session: &Session, name: Option<&str>) -> Result<String, ToolError> {
    let Some(name) = name else {
        return Ok(session.sheet.clone());
    };
    session
        .drawing
        .resolve_sheet(name)
        .map(|s| s.block)
        .map_err(|e| bad(&e.to_string()))
}

pub(crate) fn sheet_units(session: &Session, sheet: &str) -> String {
    if sheet.eq_ignore_ascii_case("*Model_Space") {
        format!("drawing units ({})", session.info().units)
    } else {
        "paper millimetres".into()
    }
}

pub(crate) fn units(session: &Session) -> String {
    units_of(session, &session.sheet)
}

pub(crate) fn units_of(session: &Session, sheet: &str) -> String {
    if sheet.eq_ignore_ascii_case("*Model_Space") {
        session.info().units
    } else {
        "paper millimetres".into()
    }
}

pub(crate) fn sheet_metres_per_unit(session: &Session) -> f64 {
    if session.sheet.eq_ignore_ascii_case("*Model_Space") {
        session.drawing.units.to_metres()
    } else {
        0.001
    }
}

pub(crate) fn with_units(session: &Session, sheet: &str, mut out: Value) -> Value {
    out["units"] = json!(units_of(session, sheet));
    out
}

/// `rows` from `offset` on, with the total before paging.
pub(crate) fn page<T: serde::Serialize>(rows: Vec<T>, offset: usize) -> Value {
    let total = rows.len();
    let rows: Vec<T> = rows.into_iter().skip(offset).collect();
    json!({ "rows": rows, "total": total })
}

/// Spatial filtering happens before grouping/paging, with the same result metadata as a full read.
pub(super) fn filtered_query_page(
    session: &Session,
    kind: &str,
    sheet: Option<&str>,
    rows: Vec<Value>,
    offset: usize,
) -> Result<ToolOutput, ToolError> {
    let block = sheet_block(session, sheet)?;
    let mut out = page(rows, offset);
    if matches!(kind, "dimensions" | "near" | "window") {
        out = with_units(session, &block, out);
    }
    if kind == "blocks" {
        out["count_scope"] = json!(qiyas_core::INSERT_COUNT_SCOPE);
        out["visibility_policy"] = json!("as_shown");
        out["limitations"] = json!(qiyas_core::INSERT_COUNT_LIMITATIONS);
    }
    Ok(ToolOutput::Json(capped(rounded(out), offset)))
}

/// `conflicts` from `offset` on, with the counts of all of them per kind and per status.
fn conflicts_page(conflicts: &[qiyas_core::Conflict], offset: usize) -> Value {
    let mut by_kind = serde_json::Map::new();
    let mut by_status = serde_json::Map::new();
    for c in conflicts {
        for (map, key) in [
            (&mut by_kind, json!(c.kind)),
            (&mut by_status, json!(c.status)),
        ] {
            let key = key.as_str().unwrap_or_default().to_string();
            let n = map.get(&key).and_then(Value::as_u64).unwrap_or(0);
            map.insert(key, json!(n + 1));
        }
    }
    let mut out = page(conflicts.to_vec(), offset);
    out["by_kind"] = Value::Object(by_kind);
    out["by_status"] = Value::Object(by_status);
    out
}

fn bad(why: &str) -> ToolError {
    ToolError::BadInput(why.to_string())
}

fn size(v: &Value) -> usize {
    serde_json::to_vec(v).map_or(usize::MAX, |b| b.len())
}

/// `value` cut to fit `MAX_OUTPUT` bytes: a `rows` array loses its last rows, with
/// `"truncated": true` and `next_offset` (`offset` plus the rows kept); otherwise strings are
/// shortened. As a last resort the result is replaced by a note saying it was too large.
fn capped(mut value: Value, offset: usize) -> Value {
    if size(&value) <= MAX_OUTPUT {
        return value;
    }
    if let Some(Value::Array(rows)) = value.get_mut("rows").map(Value::take) {
        // Size of everything but the rows, with the widest next_offset it could get.
        value["truncated"] = json!(true);
        value["next_offset"] = json!(offset + rows.len());
        value["rows"] = json!([]);
        let base = size(&value);
        let mut used = base;
        let mut kept = 0;
        for row in &rows {
            // A comma before every row but the first.
            let add = size(row) + usize::from(kept > 0);
            if used + add > MAX_OUTPUT {
                break;
            }
            used += add;
            kept += 1;
        }
        if kept > 0 {
            value["next_offset"] = json!(offset + kept);
            value["rows"] = Value::Array(rows.into_iter().take(kept).collect());
            return value;
        }
        value["rows"] = Value::Array(rows);
    }
    let mut limit = 1024;
    while limit >= 16 {
        let mut cut = value.clone();
        cut_strings(&mut cut, limit);
        if let Some(map) = cut.as_object_mut() {
            map.insert("truncated".to_string(), json!(true));
        }
        if size(&cut) <= MAX_OUTPUT {
            return cut;
        }
        limit /= 2;
    }
    json!({ "truncated": true, "note": "The result was too large to return; ask for less." })
}
fn cut_strings(v: &mut Value, limit: usize) {
    match v {
        Value::String(s) if s.chars().count() > limit => {
            *s = s.chars().take(limit).chain(['…']).collect();
        }
        Value::Array(items) => items.iter_mut().for_each(|v| cut_strings(v, limit)),
        Value::Object(map) => map.values_mut().for_each(|v| cut_strings(v, limit)),
        _ => {}
    }
}

/// Host operations keep their saved result fields and add command input metadata beside them.
pub(crate) fn append_metadata(out: &mut Value, report: &qiyas_core::Reported) {
    let metadata = json!(report);
    for key in ["defaults_used", "warnings", "advice", "handle_map"] {
        if let Some(value) = metadata.get(key) {
            // Drawing checks already use warnings for the finding count.
            if key == "warnings"
                && out[key].is_number()
                && value.as_array().is_some_and(Vec::is_empty)
            {
                continue;
            }
            out[key] = value.clone();
        }
    }
}

pub(crate) fn compare(
    session: &mut Session,
    args: &Value,
    source: Source,
) -> Result<Value, ToolError> {
    let mut input = args.clone();
    input["type"] = json!("discrepancy_check");
    let report = session.run_input(&input, source)?;
    let mut out = conflicts_page(&session.conflicts, 0);
    append_metadata(&mut out, &report);
    Ok(out)
}
