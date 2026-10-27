//! Runs a tool call on the session: parses the input, dispatches to a query or to `run_as`, and
//! keeps the JSON result under 40 KB. Nothing from a tool's input is ever logged.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use qiyas_core::{
    Command, ConflictStatus, CoreError, DimSpec, EntityFilter, Field, Kind, MarkupKind, Outcome,
    Session, Source, Target, COMMENT_LAYER,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use qiyas_render::Batch;

use crate::image::{self, normalized, Inset, Raster, Scene, TextSize, View, MAX_SIDE};
use crate::marks::Mark;

/// Largest JSON result, in bytes. Longer results lose rows (or have their strings cut).
const MAX_OUTPUT: usize = 40_960;
/// `find_entities` rows per page when the model gives no limit.
const DEFAULT_LIMIT: usize = 100;
/// `extract_data` rows per page when the model gives no limit, and the most it may ask for.
const EXTRACT_LIMIT: usize = 200;
const MAX_EXTRACT: usize = 5000;
const DEFAULT_WIDTH: u32 = 800;
const MIN_WIDTH: u32 = 16;
/// Margin around the whole sheet in `view_image`, as a share of its size.
const MARGIN: f64 = 0.02;
/// Most entities marked with numbers in one picture.
const MAX_MARKS: usize = 40;
/// Longest side, in pixels, a picture under the lines is drawn at.
const MAX_RASTER_PX: f64 = 4096.0;
const MODEL: &str = "*Model_Space";
/// Why a tool refuses a path outside the drawing's folder.
const OUTSIDE_FOLDER: &str = "Only files beside the open drawing can be opened by the assistant; \
                              use the BOQ tab's Import for other files";
/// Decimals kept in the numbers of a JSON result.
const DECIMALS: f64 = 1e6;

/// What a tool returns: JSON for the model, or a picture of the drawing.
#[derive(Debug, Clone)]
pub enum ToolOutput {
    Json(Value),
    Image { png: Vec<u8>, caption: String },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("{message}")]
    Structured {
        code: String,
        message: String,
        details: Value,
    },
    #[error("No tool named {0}")]
    UnknownTool(String),
    /// The input is wrong: bad JSON shape, a missing entity, sheet, block or measurement.
    #[error("{0}")]
    BadInput(String),
    /// The tool is in the catalogue but its backing is not built yet.
    #[error("{0}")]
    NotAvailable(String),
    #[error("{0}")]
    Failed(String),
}

impl ToolError {
    pub fn json(&self) -> Value {
        match self {
            Self::Structured {
                code,
                message,
                details,
            } => {
                let mut out = details.clone();
                if !out.is_object() {
                    out = json!({});
                }
                out["code"] = json!(code);
                out["message"] = json!(message);
                out
            }
            other => {
                let mut out = crate::errors::json_error(&self.to_string());
                if out["code"] == "failed" {
                    out["code"] = json!(match other {
                        Self::UnknownTool(_) => "not_found",
                        Self::BadInput(_) => "invalid_value",
                        Self::NotAvailable(_) => "refused",
                        _ => "failed",
                    });
                }
                out
            }
        }
    }
}

impl From<CoreError> for ToolError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::StaleRead { details } => ToolError::Structured { code: "stale_read".into(), message: "The drawing changed since this read. No edit was applied. Read the changed objects and use their current revision.".into(), details },
            CoreError::InputWarnings { error, warnings } => {
                let underlying = ToolError::from(*error);
                let mut details = underlying.json();
                details["warnings"] = json!(warnings);
                ToolError::Structured {
                    code: details["code"].as_str().unwrap_or("failed").into(),
                    message: details["message"].as_str().unwrap_or_default().into(),
                    details,
                }
            }
            CoreError::Step {
                index,
                label,
                error,
            } => {
                let mut details = ToolError::from(*error).json();
                for field in ["errors", "warnings"] {
                    if let Some(issues) = details.get_mut(field).and_then(Value::as_array_mut) {
                        for issue in issues {
                            if let Some(pointer) = issue["pointer"].as_str() {
                                issue["pointer"] = json!(format!("/commands/{index}{pointer}"));
                            }
                        }
                    }
                }
                let message = format!(
                    "Batch step {index} ({label}): {}",
                    details["message"].as_str().unwrap_or_default()
                );
                details["step"] = json!(index);
                details["command"] = json!(label);
                ToolError::Structured {
                    code: details["code"].as_str().unwrap_or("failed").into(),
                    message,
                    details,
                }
            }
            // Handles are integers in the tools' JSON, not hex as in the core's message.
            CoreError::NoSuchEntity(h) => ToolError::BadInput(format!("No entity with handle {h}")),
            CoreError::NoSuchSheet(_) | CoreError::InvalidCommand(_) => {
                ToolError::BadInput(e.to_string())
            }
            other => ToolError::Failed(other.to_string()),
        }
    }
}

/// Runs the tool `name` with `input` on `session` for `source` (new measurements are marked as
/// made by it). Drawing coordinates are in drawing units (`session.drawing.units`); heights
/// are given in metres.
pub fn execute(
    session: &mut Session,
    name: &str,
    input: &Value,
    source: Source,
) -> Result<ToolOutput, ToolError> {
    execute_with_detail(session, name, input, source, false)
}

/// The public surface keeps exact evidence; legacy private tools retain their old display format.
pub(crate) fn execute_full(
    session: &mut Session,
    name: &str,
    input: &Value,
    source: Source,
) -> Result<ToolOutput, ToolError> {
    execute_with_detail(session, name, input, source, true)
}

fn execute_with_detail(
    session: &mut Session,
    name: &str,
    input: &Value,
    source: Source,
    full: bool,
) -> Result<ToolOutput, ToolError> {
    let empty = json!({});
    let input = if input.is_null() { &empty } else { input };
    let (value, offset) = match name {
        "list_sheets" => {
            parse::<NoInput>(input)?;
            let info = session.info();
            let out = json!({
                "sheets": info.sheets,
                "current": session.sheet,
                "units": info.units,
            });
            (out, 0)
        }
        "set_sheet" => (set_sheet(session, parse(input)?, source)?, 0),
        "list_layers" => {
            let Paged { offset } = parse(input)?;
            (list_layers(session, offset), offset)
        }
        "show_layers" => (show_layers(session, parse(input)?, source)?, 0),
        "find_entities" => {
            let input: FindInput = parse(input)?;
            let offset = input.offset.unwrap_or(0);
            (find_entities(session, input)?, offset)
        }
        "get_entity" => {
            let Handle { handle, chain } = parse(input)?;
            let entity = session.entity_info(handle, &chain)?;
            (json!({ "entity": entity, "units": units(session) }), 0)
        }
        "read_text" => {
            let ReadInput {
                sheet,
                within,
                layer,
                offset,
            } = parse(input)?;
            let block = sheet_block(session, sheet.as_deref())?;
            let mut rows = session.read_text(Some(&block), within);
            if let Some(layer) = layer {
                rows.retain(|row| row.layer.eq_ignore_ascii_case(&layer));
            }
            (with_units(session, &block, page(rows, offset)), offset)
        }
        "read_tables" => {
            let SheetInput { sheet, offset } = parse(input)?;
            let block = sheet_block(session, sheet.as_deref())?;
            (page(session.read_tables(Some(&block)), offset), offset)
        }
        "read_dimensions" => {
            let SheetInput { sheet, offset } = parse(input)?;
            let block = sheet_block(session, sheet.as_deref())?;
            let rows = session.read_dimensions(Some(&block));
            (with_units(session, &block, page(rows, offset)), offset)
        }
        "block_summary" => {
            let SheetInput { sheet, offset } = parse(input)?;
            let block = sheet_block(session, sheet.as_deref())?;
            let mut out = page(session.block_summary(Some(&block)), offset);
            out["count_scope"] = json!(qiyas_core::INSERT_COUNT_SCOPE);
            out["visibility_policy"] = json!("as_shown");
            out["limitations"] = json!(qiyas_core::INSERT_COUNT_LIMITATIONS);
            (out, offset)
        }
        "measure_length" => (measure(session, parse(input)?, Kind::Length, source)?, 0),
        "measure_area" => (measure(session, parse(input)?, Kind::Area, source)?, 0),
        "count_blocks" => (count_blocks(session, parse(input)?, source)?, 0),
        "add_measurement" => (add_measurement(session, parse(input)?, source)?, 0),
        "set_height" => {
            let SetHeight { id, height } = parse(input)?;
            let height = height / session.drawing.units.to_metres();
            let cmd = Command::SetHeight {
                id: id.clone(),
                height,
            };
            (edit(session, cmd, &id, source)?, 0)
        }
        "list_takeoff" => {
            let Paged { offset } = parse(input)?;
            let mut out = page(session.takeoff.items.clone(), offset);
            out["totals"] = json!(session.takeoff.totals());
            (out, offset)
        }
        "remove_measurement" => {
            let Id { id } = parse(input)?;
            match session.run_as(Command::RemoveMeasurement { id: id.clone() }, source)? {
                Outcome::Removed { id } => (json!({ "removed": id }), 0),
                _ => return Err(no_measurement(&id)),
            }
        }
        "set_description" => {
            let SetText { id, text } = parse(input)?;
            let cmd = Command::SetDescription {
                id: id.clone(),
                text,
            };
            (edit(session, cmd, &id, source)?, 0)
        }
        "link_boq" => {
            let LinkBoq { id, code } = parse(input)?;
            let code = Some(code.trim().to_string()).filter(|c| !c.is_empty());
            let cmd = Command::LinkBoq {
                id: id.clone(),
                code,
            };
            (edit(session, cmd, &id, source)?, 0)
        }
        "import_boq" => (import_boq(session, parse(input)?, source)?, 0),
        "list_boq" => {
            let Paged { offset } = parse(input)?;
            let boq = session.boq.as_ref().ok_or_else(|| {
                bad("No BOQ is imported yet; import_boq imports one from an Excel file")
            })?;
            let mut out = page(boq.items.clone(), offset);
            out["source"] = json!(boq.source);
            out["skipped_rows"] = json!(boq.skipped_rows);
            (out, offset)
        }
        "compare_boq" => {
            let Tolerance { tolerance } = parse(input)?;
            let tolerance = tolerance.unwrap_or(qiyas_takeoff::DEFAULT_TOLERANCE);
            let conflicts = qiyas_takeoff::compare_session(session, tolerance, source)?;
            (conflicts_page(&conflicts, 0), 0)
        }
        "list_conflicts" => {
            let Paged { offset } = parse(input)?;
            (conflicts_page(&session.conflicts, offset), offset)
        }
        "resolve_conflict" => {
            let Resolve { id, status, note } = parse(input)?;
            let cmd = Command::ResolveConflict {
                id: id.clone(),
                status,
                note,
            };
            match session.run_as(cmd, source)? {
                Outcome::Nothing => {
                    return Err(bad(&format!(
                        "No conflict with id {id}; qiyas_read records with collection conflicts gives the ids"
                    )))
                }
                _ => {
                    let conflict = session.conflicts.iter().find(|c| c.id == id);
                    (json!({ "conflict": conflict }), 0)
                }
            }
        }
        "add_markup" => (add_markup(session, parse(input)?, source)?, 0),
        "add_dimension" => (add_dimension(session, parse(input)?, source)?, 0),
        "set_annotation_scale" => {
            let ScaleInput { scale } = parse(input)?;
            session.run_as(Command::AnnotationScale { name: scale }, source)?;
            let (name, factor) = session.annotation_scale();
            (json!({ "annotation_scale": name, "factor": factor }), 0)
        }
        "add_comment" => {
            let CommentInput {
                points,
                text,
                arc_length,
            } = parse(input)?;
            let cmd = Command::Comment {
                points: points.into(),
                text,
                arc_len: arc_length,
            };
            match session.run_as(cmd, source)? {
                Outcome::Added { handles } => (
                    json!({ "cloud": handles.first(), "label": handles.get(1), "layer": COMMENT_LAYER }),
                    0,
                ),
                _ => return Err(ToolError::Failed("No comment was drawn".to_string())),
            }
        }
        "list_comments" => match session.run_as(Command::ListComments, source)? {
            Outcome::Comments { comments } => (json!({ "comments": comments }), 0),
            _ => return Err(ToolError::Failed("No comments were listed".to_string())),
        },
        "measure_geometry" => {
            let GeometryInput { handle, chain, at } = parse(input)?;
            let cmd = Command::MeasureGeometry { handle, chain, at };
            (info(session.run_as(cmd, source)?)?, 0)
        }
        "divide_along" => {
            let DivideInput {
                handle,
                chain,
                count,
            } = parse(input)?;
            let cmd = Command::DivideAlong {
                handle,
                chain,
                count,
            };
            (placed(session.run_as(cmd, source)?)?, 0)
        }
        "measure_along" => {
            let AlongInput {
                handle,
                chain,
                step,
            } = parse(input)?;
            let step = step / session.drawing.units.to_metres();
            let cmd = Command::MeasureAlong {
                handle,
                chain,
                step,
            };
            (placed(session.run_as(cmd, source)?)?, 0)
        }
        "id_point" => {
            let PointInput { point } = parse(input)?;
            let cmd = Command::IdPoint {
                p: point,
                kind: None,
            };
            (info(session.run_as(cmd, source)?)?, 0)
        }
        "extract_data" => {
            let input: ExtractInput = parse(input)?;
            let offset = input.offset.unwrap_or(0);
            (extract_data(session, input, source)?, offset)
        }
        "zoom_to" => (zoom_to(session, parse(input)?)?, 0),
        "view_image" => return view_image(session, parse(input)?),
        _ => return Err(ToolError::UnknownTool(name.to_string())),
    };
    Ok(ToolOutput::Json(if full {
        value
    } else {
        capped(rounded(value), offset)
    }))
}

// ---- Inputs. Unknown fields are refused, so a misspelt filter is never silently ignored. ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoInput {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paged {
    #[serde(default)]
    offset: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetName {
    sheet: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowLayers {
    names: Vec<String>,
    visible: bool,
}

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    handle: u64,
    #[serde(default)]
    chain: Vec<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadInput {
    sheet: Option<String>,
    layer: Option<String>,
    #[serde(alias = "bounds")]
    within: Option<[[f64; 2]; 2]>,
    #[serde(default)]
    offset: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SheetInput {
    sheet: Option<String>,
    #[serde(default)]
    offset: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureInput {
    handle: Option<u64>,
    #[serde(default)]
    chain: Vec<u64>,
    points: Option<Vec<[f64; 2]>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CountInput {
    block_name: String,
    layer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddInput {
    kind: Kind,
    description: String,
    #[serde(default)]
    points: Vec<[f64; 2]>,
    #[serde(default)]
    handles: Vec<u64>,
    height: Option<f64>,
    value: Option<f64>,
    unit: Option<String>,
    #[serde(default)]
    note: String,
    /// True when the user explicitly asked for an empty item.
    #[serde(default)]
    empty_item: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetHeight {
    id: String,
    height: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetText {
    id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportBoq {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkBoq {
    id: String,
    code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tolerance {
    tolerance: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolve {
    id: String,
    status: ConflictStatus,
    note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarkupInput {
    points: Vec<[f64; 2]>,
    text: String,
    kind: MarkupKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleInput {
    scale: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentInput {
    points: Vec<[f64; 2]>,
    text: String,
    arc_length: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum DimKind {
    Linear,
    Aligned,
    Angular,
    Radius,
    Diameter,
    ArcLength,
    Ordinate,
    Baseline,
    Continue,
    Quick,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DimensionInput {
    kind: DimKind,
    p1: Option<[f64; 2]>,
    p2: Option<[f64; 2]>,
    line_point: Option<[f64; 2]>,
    rotation: Option<f64>,
    offset: Option<f64>,
    vertex: Option<[f64; 2]>,
    arc_point: Option<[f64; 2]>,
    point: Option<[f64; 2]>,
    leader_end: Option<[f64; 2]>,
    handle: Option<u64>,
    handles: Option<Vec<u64>>,
    points: Option<Vec<[f64; 2]>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZoomInput {
    handles: Option<Vec<u64>>,
    bounds: Option<[[f64; 2]; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeometryInput {
    handle: u64,
    #[serde(default)]
    chain: Vec<u64>,
    at: Option<[f64; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DivideInput {
    handle: u64,
    #[serde(default)]
    chain: Vec<u64>,
    count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlongInput {
    handle: u64,
    #[serde(default)]
    chain: Vec<u64>,
    /// Metres.
    step: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointInput {
    point: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtractInput {
    sheet: Option<String>,
    layer: Option<String>,
    kind: Option<String>,
    block_name: Option<String>,
    text_contains: Option<String>,
    within: Option<[[f64; 2]; 2]>,
    fields: Vec<Field>,
    #[serde(default)]
    attributes: Vec<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageInput {
    bounds: Option<[[f64; 2]; 2]>,
    width: Option<u32>,
    sheet: Option<String>,
    #[serde(default)]
    highlight: Vec<u64>,
    #[serde(default)]
    marks: bool,
}

fn parse<T: DeserializeOwned>(input: &Value) -> Result<T, ToolError> {
    T::deserialize(input).map_err(|e| ToolError::BadInput(format!("Bad input: {e}")))
}

// ---- Tools ----

fn set_sheet(session: &mut Session, input: SheetName, source: Source) -> Result<Value, ToolError> {
    let block = sheet_block(session, Some(&input.sheet))?;
    session.run_as(
        Command::SetSheet {
            block: block.clone(),
        },
        source,
    )?;
    Ok(json!({ "sheet": block, "ui": { "action": "show_sheet", "sheet": block } }))
}

/// True when the drawing has the layer, or one of its xrefs adds it (`XREF|LAYER`); any case.
fn has_layer(session: &Session, name: &str) -> bool {
    session.drawing.doc.layers.get(name).is_some()
        || session
            .layers()
            .iter()
            .any(|l| l.name.eq_ignore_ascii_case(name))
}

fn list_layers(session: &Session, offset: usize) -> Value {
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

fn show_layers(
    session: &mut Session,
    input: ShowLayers,
    source: Source,
) -> Result<Value, ToolError> {
    let unknown: Vec<&str> = input
        .names
        .iter()
        .filter(|n| !has_layer(session, n))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(ToolError::BadInput(format!(
            "No layer named {}; qiyas_read query with kind layers gives the names",
            unknown.join(", ")
        )));
    }
    for name in input.names {
        session.run_as(
            Command::SetLayerHidden {
                name,
                hidden: !input.visible,
            },
            source,
        )?;
    }
    let hidden = session.hidden_layers();
    Ok(json!({ "hidden": hidden, "ui": { "action": "redraw", "hidden": hidden } }))
}

fn find_entities(session: &Session, input: FindInput) -> Result<Value, ToolError> {
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

/// `measure_length` or `measure_area`: an entity by handle, or a shape by points.
fn measure(
    session: &mut Session,
    input: MeasureInput,
    kind: Kind,
    source: Source,
) -> Result<Value, ToolError> {
    if !input.chain.is_empty() && input.handle.is_none() {
        return Err(bad("A chain goes with a handle"));
    }
    let cmd = match (input.handle, input.points) {
        (Some(handle), None) => {
            let entity = session.entity_info(handle, &input.chain)?;
            Command::AddMeasurement {
                kind,
                value: None,
                unit: None,
                height: None,
                points: Vec::new().into(),
                handles: vec![handle],
                chain: input.chain,
                description: format!("{} on {}", entity.kind, entity.layer_short),
                note: String::new(),
                empty_item: false,
            }
        }
        (None, Some(points)) if kind == Kind::Length => Command::MeasureDistance {
            points: points.into(),
        },
        (None, Some(points)) => Command::MeasureArea {
            points: points.into(),
        },
        (Some(_), Some(_)) => return Err(bad("Give handle or points, not both")),
        (None, None) => return Err(bad("Give handle or points")),
    };
    let default_name = matches!(cmd, Command::AddMeasurement { .. });
    let by_points = match &cmd {
        Command::MeasureDistance { points } => Some(points.to_vec()),
        Command::MeasureArea { points } => Some(points.to_vec()),
        _ => None,
    };
    let mut outcome = session.run_as(cmd, source)?;
    // "LwPolyline on A-ROOM" is the default name MeasureEntity would give: mark it so, in the
    // take-off (the step just recorded) and in the answer.
    if let (true, Outcome::Measured { measurement }) = (default_name, &mut outcome) {
        measurement.auto_description = true;
        if let Some(m) = session
            .takeoff
            .items
            .iter_mut()
            .find(|m| m.id == measurement.id)
        {
            m.auto_description = true;
        }
    }
    let mut out = measured(outcome)?;
    // Points that trace one drawn entity only approximate it: say so, and where it is.
    if let Some(points) = by_points {
        let candidates = traced_entities(session, &points);
        if !candidates.is_empty() {
            let what = if kind == Kind::Length {
                "length"
            } else {
                "area"
            };
            out["hint"] = json!(format!(
                "These points follow a drawn entity, so they only approximate its shape. \
                 Remove this measurement (remove_measurement) and call measure_{what} with the \
                 handle of the entity meant (candidates), which measures the drawn geometry \
                 exactly; if several could be meant, ask the user which."
            ));
            out["candidates"] = json!(candidates);
        }
    }
    Ok(out)
}

/// The drawn entities whose extent is that of `points` (within a tenth of it): what a user
/// clicking along one entity most likely meant.
fn traced_entities(session: &Session, points: &[[f64; 2]]) -> Vec<Value> {
    const KINDS: [&str; 10] = [
        "Line",
        "Arc",
        "Circle",
        "Ellipse",
        "Spline",
        "LwPolyline",
        "Polyline",
        "Polyline2D",
        "Polyline3D",
        "Hatch",
    ];
    if points.is_empty() {
        return Vec::new();
    }
    let [lo, hi] = bounds_of(points);
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(extent.is_finite() && extent > 0.0) {
        return Vec::new();
    }
    let tol = extent * 0.1;
    let page = session.find_entities(&EntityFilter {
        within: Some([[lo[0] - tol, lo[1] - tol], [hi[0] + tol, hi[1] + tol]]),
        limit: 500,
        ..Default::default()
    });
    page.rows
        .into_iter()
        .filter(|r| KINDS.contains(&r.kind.as_str()))
        .filter(|r| {
            let [rl, rh] = r.bounds;
            [rl[0] - lo[0], rl[1] - lo[1], rh[0] - hi[0], rh[1] - hi[1]]
                .iter()
                .all(|d| d.abs() <= tol)
        })
        .take(5)
        .map(|r| {
            json!({
                "handle": r.handle,
                "kind": r.kind,
                "layer": r.layer,
                "length": r.length,
                "area": r.area,
            })
        })
        .collect()
}

fn count_blocks(
    session: &mut Session,
    input: CountInput,
    source: Source,
) -> Result<Value, ToolError> {
    // The block may be the drawing's or one of its xrefs'.
    let known_block = std::iter::once(&session.drawing.doc)
        .chain(
            session
                .xrefs
                .entries
                .iter()
                .filter_map(|x| Some(&x.drawing.as_ref()?.doc)),
        )
        .any(|doc| doc.block_records.get(&input.block_name).is_some());
    if !known_block {
        return Err(bad(&format!(
            "No block named {}; block_summary lists the blocks",
            input.block_name
        )));
    }
    if let Some(layer) = &input.layer {
        if !has_layer(session, layer) {
            return Err(bad(&format!("No layer named {layer}")));
        }
    }
    let cmd = Command::CountBlock {
        block_name: input.block_name,
        layer: input.layer,
    };
    measured(session.run_as(cmd, source)?)
}

fn add_measurement(
    session: &mut Session,
    input: AddInput,
    source: Source,
) -> Result<Value, ToolError> {
    if input.unit.as_deref() == Some("markup") {
        return Err(bad("Use add_markup for markups"));
    }
    let mut seen = HashSet::new();
    for &h in &input.handles {
        if !seen.insert(h) {
            return Err(bad(&format!("Handle {h} is given twice")));
        }
        session.entity_info(h, &[])?;
    }
    let to_drawing_units = session.drawing.units.to_metres();
    let cmd = Command::AddMeasurement {
        kind: input.kind,
        // An explicitly empty item is a quantity of 0.
        value: input.value.or(input.empty_item.then_some(0.0)),
        unit: input.unit,
        height: input.height.map(|h| h / to_drawing_units),
        points: input.points.into(),
        handles: input.handles,
        chain: Vec::new(),
        description: input.description,
        note: input.note,
        empty_item: input.empty_item,
    };
    measured(session.run_as(cmd, source)?)
}

/// Reads the workbook at `path` and makes it the session's BOQ (an undo step). Returns a summary,
/// not the items: list_boq pages through those.
fn import_boq(session: &mut Session, input: ImportBoq, source: Source) -> Result<Value, ToolError> {
    let path = resolve_in_drawing_folder(session, &input.path)?;
    // A wrong path or a sheet with no header is the model's to fix, so both are bad input.
    let boq = qiyas_boq::import_boq(&path).map_err(|e| ToolError::BadInput(e.to_string()))?;
    let mut sections: Vec<&str> = Vec::new();
    for item in &boq.items {
        if !item.section.is_empty() && sections.last() != Some(&item.section.as_str()) {
            sections.push(&item.section);
        }
    }
    let out = json!({
        "source": boq.source,
        "items": boq.items.len(),
        "skipped_rows": boq.skipped_rows,
        "sections": sections,
    });
    session.run_as(Command::SetBoq { boq }, source)?;
    Ok(out)
}

fn add_markup(
    session: &mut Session,
    input: MarkupInput,
    source: Source,
) -> Result<Value, ToolError> {
    let bounds = bounds_of(&input.points);
    let cmd = Command::AddMarkup {
        kind: input.kind,
        points: input.points.into(),
        text: input.text,
    };
    let mut out = measured(session.run_as(cmd, source)?)?;
    let id = out["measurement"]["id"].clone();
    out["ui"] = json!({ "action": "show_markup", "id": id, "bounds": bounds });
    Ok(out)
}

/// Draws the dimension `input` describes; the handles added.
fn add_dimension(
    session: &mut Session,
    input: DimensionInput,
    source: Source,
) -> Result<Value, ToolError> {
    fn need<T>(v: Option<T>, what: &str) -> Result<T, ToolError> {
        v.ok_or_else(|| bad(&format!("This kind of dimension needs {what}")))
    }
    let target = |h: Option<u64>| -> Result<Target, ToolError> {
        Ok(Target {
            handle: need(h, "handle")?,
            chain: Vec::new(),
        })
    };
    let spec = match input.kind {
        DimKind::Linear => DimSpec::Linear {
            p1: need(input.p1, "p1")?,
            p2: need(input.p2, "p2")?,
            line_point: need(input.line_point, "line_point")?,
            rotation: input.rotation,
        },
        DimKind::Aligned => DimSpec::Aligned {
            p1: need(input.p1, "p1")?,
            p2: need(input.p2, "p2")?,
            offset: need(input.offset, "offset")?,
        },
        DimKind::Angular => DimSpec::Angular {
            vertex: need(input.vertex, "vertex")?,
            p1: need(input.p1, "p1")?,
            p2: need(input.p2, "p2")?,
            arc_point: need(input.arc_point, "arc_point")?,
        },
        DimKind::Radius => DimSpec::Radius {
            target: target(input.handle)?,
            point: need(input.point, "point")?,
        },
        DimKind::Diameter => DimSpec::Diameter {
            target: target(input.handle)?,
            point: need(input.point, "point")?,
        },
        DimKind::ArcLength => DimSpec::ArcLength {
            target: target(input.handle)?,
            point: need(input.point, "point")?,
        },
        DimKind::Ordinate => DimSpec::Ordinate {
            feature: need(input.point, "point")?,
            leader_end: need(input.leader_end, "leader_end")?,
        },
        DimKind::Baseline => DimSpec::Baseline {
            from: target(input.handle)?,
            points: need(input.points, "points")?.into(),
        },
        DimKind::Continue => DimSpec::Continue {
            from: target(input.handle)?,
            points: need(input.points, "points")?.into(),
        },
        DimKind::Quick => DimSpec::Quick {
            targets: need(input.handles, "handles")?
                .into_iter()
                .map(|handle| Target {
                    handle,
                    chain: Vec::new(),
                })
                .collect(),
            line_point: need(input.line_point, "line_point")?,
        },
    };
    match session.run_as(Command::Dimension(spec), source)? {
        Outcome::Added { handles } => Ok(json!({ "dimensions": handles })),
        _ => Err(ToolError::Failed("No dimension was drawn".to_string())),
    }
}

/// One page of the data table; `total` counts every entity the filters find.
fn extract_data(
    session: &mut Session,
    input: ExtractInput,
    source: Source,
) -> Result<Value, ToolError> {
    if input
        .fields
        .iter()
        .any(|f| matches!(f, Field::Attribute(_)))
    {
        return Err(bad("Give attribute tags in attributes, not in fields"));
    }
    let sheet = match input.sheet {
        Some(name) => Some(sheet_block(session, Some(&name))?),
        None => None,
    };
    let offset = input.offset.unwrap_or(0);
    let filter = EntityFilter {
        sheet,
        layer: input.layer,
        kind: input.kind,
        block_name: input.block_name,
        text_contains: input.text_contains,
        within: input.within,
        offset,
        limit: input.limit.unwrap_or(EXTRACT_LIMIT).min(MAX_EXTRACT),
    };
    let total = session
        .find_entities(&EntityFilter {
            limit: 0,
            ..filter.clone()
        })
        .total;
    let mut fields = input.fields;
    fields.extend(input.attributes.into_iter().map(Field::Attribute));
    match session.run_as(Command::ExtractData { filter, fields }, source)? {
        Outcome::Table { columns, rows } => {
            let end = offset.saturating_add(rows.len());
            Ok(json!({
                "columns": columns,
                "rows": rows,
                "total": total,
                "next_offset": (end < total && !rows.is_empty()).then_some(end),
            }))
        }
        _ => Err(ToolError::Failed("No table was made".to_string())),
    }
}

fn zoom_to(session: &Session, input: ZoomInput) -> Result<Value, ToolError> {
    let (bounds, handles) = match (input.handles, input.bounds) {
        (None, Some(bounds)) => (normalized(bounds), Vec::new()),
        (Some(handles), None) if !handles.is_empty() => (drawn_bounds(session, &handles)?, handles),
        (Some(_), None) => return Err(bad("Give at least one handle")),
        (Some(_), Some(_)) => return Err(bad("Give handles or bounds, not both")),
        (None, None) => return Err(bad("Give handles or bounds")),
    };
    Ok(json!({
        "bounds": bounds,
        "ui": { "action": "zoom", "bounds": bounds, "handles": handles },
    }))
}

fn view_image(session: &Session, input: ImageInput) -> Result<ToolOutput, ToolError> {
    let look = Look {
        sheet: input.sheet.as_deref(),
        bounds: input.bounds,
        width: input.width.unwrap_or(DEFAULT_WIDTH),
        highlight: &input.highlight,
        region: &[],
        marks: input.marks,
    };
    let (png, caption) = picture(session, &look)?;
    Ok(ToolOutput::Image { png, caption })
}

/// What a picture of the drawing shows: `view_image`'s, and the one sent with what the user
/// selected or marked.
pub(crate) struct Look<'a> {
    /// A sheet by layout or block name; the one on screen when `None`.
    pub sheet: Option<&'a str>,
    /// The part shown, drawing coordinates; the whole sheet when `None`.
    pub bounds: Option<[[f64; 2]; 2]>,
    pub width: u32,
    /// Entities at the top of the sheet drawn thick violet and marked first.
    pub highlight: &'a [u64],
    /// A region the user marked, dashed.
    pub region: &'a [[f64; 2]],
    /// Numbered marks on the entities in view, listed in the caption.
    pub marks: bool,
}

/// A PNG of the drawing as `look` asks, drawn as the engineer sees it (pictures and PDF
/// underlays under the lines, model space in a layout's viewports, the text as letters), and
/// its caption: where it is, how big a pixel is, how tall the text is, the marks' handles and
/// what could not be drawn.
pub(crate) fn picture(session: &Session, look: &Look) -> Result<(Vec<u8>, String), ToolError> {
    let width = look.width.clamp(MIN_WIDTH, MAX_SIDE);
    let sheet = sheet_block(session, look.sheet)?;
    let bounds = match look.bounds {
        Some(b) => normalized(b),
        None => {
            let drawn = if sheet == session.sheet {
                session.batch()
            } else {
                session.sheet_batch(&sheet)
            };
            let (pictures, problems) = session.plot_pictures(&sheet, 1.0, MAX_RASTER_PX);
            let geometry = !drawn.segments.is_empty() || !drawn.fills.is_empty();
            if !geometry && pictures.is_empty() {
                if !problems.is_empty() {
                    return Err(bad(&format!(
                        "Pictures unavailable: {}",
                        problems.join("; ")
                    )));
                }
                return Err(bad("Nothing is drawn on that sheet"));
            }
            let mut bounds = if geometry {
                drawn.bounds
            } else {
                [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]]
            };
            for (_, placement, _) in pictures {
                for point in placement.corners {
                    for axis in 0..2 {
                        bounds[0][axis] = bounds[0][axis].min(point[axis]);
                        bounds[1][axis] = bounds[1][axis].max(point[axis]);
                    }
                }
            }
            with_margin(bounds)
        }
    };
    let bounds = fit(bounds, width);
    let view = View::new(bounds, width);
    // Drawn afresh for this picture's zoom, its text as glyphs: what the engineer sees.
    let px = view.px_per_unit();
    let (batch, pages) = session.picture_batch_of(&sheet, px);

    // Pictures in view, under the lines.
    let (placed, mut left_out) = session.plot_pictures(&sheet, px, MAX_RASTER_PX);
    let mut rasters = Vec::new();
    let mut shown = Vec::new();
    for (_, placement, image) in placed {
        if !overlaps(&placement.corners, bounds) {
            continue;
        }
        let file = image.path.file_name().map_or_else(
            || image.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        match qiyas_core::draw_picture(&image, |_| {}) {
            Ok(drawn) => {
                let (w, h) = (drawn.width(), drawn.height());
                if let Some(r) = Raster::from_rgba(placement.corners, w, h, drawn.into_raw()) {
                    rasters.push(r);
                    shown.push(file);
                }
            }
            Err(e) => left_out.push(format!("{file}: {e}")),
        }
    }

    // Model space in the layout's viewports in view, drawn for the closest of their zooms.
    let frames = session.paper_views_against(&sheet, &Batch::default());
    let scale = frames
        .iter()
        .filter(|v| boxes_overlap(v.rect, bounds))
        .map(|v| v.scale)
        .fold(0.0, f64::max);
    let (model, model_pages, views) = if scale > 0.0 {
        let (model, pages) = session.picture_batch_of(MODEL, px * scale);
        let views = session.paper_views_against(&sheet, &model);
        (model, pages, views)
    } else {
        (Batch::default(), Vec::new(), Vec::new())
    };
    let insets: Vec<Inset> = views
        .iter()
        .map(|view| Inset {
            batch: &model,
            pages: &model_pages,
            view,
        })
        .collect();

    let measurements: Vec<_> = session
        .takeoff
        .items
        .iter()
        .filter(|m| m.sheet == sheet)
        .cloned()
        .collect();
    let drawn = image::draw(
        &Scene {
            batch: &batch,
            pages: &pages,
            rasters: &rasters,
            insets: &insets,
            measurements: &measurements,
            highlight: look.highlight,
            region: look.region,
            marks: if look.marks { MAX_MARKS } else { 0 },
        },
        view,
    );
    let mut caption = format!(
        "Sheet {}, bounds [[{:.3}, {:.3}], [{:.3}, {:.3}]] in {}, {} x {} px, {}.",
        sheet_label(session, &sheet),
        bounds[0][0],
        bounds[0][1],
        bounds[1][0],
        bounds[1][1],
        sheet_units(session, &sheet),
        drawn.width,
        drawn.height,
        pixel_size(session, &sheet, px),
    );
    if !shown.is_empty() {
        caption.push_str(&format!(" Pictures: {}.", shown.join(", ")));
    }
    if !look.highlight.is_empty() {
        caption.push_str(" Highlighted entities are thick violet.");
    }
    if !look.region.is_empty() {
        caption.push_str(" The region the user marked is dashed blue.");
    }
    caption.push_str(&text_note(drawn.text));
    caption.push_str(&marks_legend(session, &drawn.marks));
    if !left_out.is_empty() {
        caption.push_str(&format!(" Left out: {}.", left_out.join("; ")));
    }
    Ok((drawn.png, caption))
}

/// How big a pixel is: drawing units, and millimetres.
fn pixel_size(session: &Session, sheet: &str, px_per_unit: f64) -> String {
    let per_px = 1.0 / px_per_unit.max(f64::MIN_POSITIVE);
    if !sheet.eq_ignore_ascii_case("*Model_Space") {
        return format!("1 px = {per_px:.4} paper millimetres");
    }
    let mm = per_px * session.drawing.units.to_metres() * 1000.0;
    format!("1 px = {per_px:.4} drawing units ({mm:.1} mm)")
}

/// What the caption says of the text in view.
pub(crate) fn text_note(text: Option<TextSize>) -> String {
    match text {
        Some(t) => format!(
            " Text: the smallest text is about {:.1} px tall, most text about {:.1} px; letters \
             read from about 8 px, so zoom in with bounds to read smaller text.",
            t.smallest, t.typical
        ),
        None => " No CAD text in view; pictures may contain text.".to_string(),
    }
}

/// The caption's list of marks: number, handle, kind, layer, and a text's words.
pub(crate) fn marks_legend(session: &Session, marks: &[Mark]) -> String {
    if marks.is_empty() {
        return String::new();
    }
    let rows: Vec<String> = marks
        .iter()
        .map(|m| match session.entity_info(m.top, &[]) {
            Ok(info) => {
                let mut row = format!(
                    "#{} handle {} {} {}",
                    m.n, m.top, info.kind, info.layer_short
                );
                if let Some(text) = info.plain_text.as_ref().or(info.text.as_ref()) {
                    let short: String = text.chars().take(40).collect();
                    row.push_str(&format!(" \"{short}\""));
                }
                row
            }
            Err(_) => format!("#{} handle {}", m.n, m.top),
        })
        .collect();
    format!(
        " Marks (orange badges; use the handle, never guess one): {}.",
        rows.join("; ")
    )
}

/// The sheet's layout name, with its block name when they differ.
fn sheet_label(session: &Session, block: &str) -> String {
    session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.block.eq_ignore_ascii_case(block))
        .map_or_else(
            || block.to_string(),
            |s| {
                if s.name == s.block {
                    s.name
                } else {
                    format!("{} ({})", s.name, s.block)
                }
            },
        )
}

/// Whether the corners' box overlaps `bounds`.
fn overlaps(corners: &[[f64; 2]; 4], bounds: [[f64; 2]; 2]) -> bool {
    boxes_overlap(bounds_of(corners), bounds)
}

fn boxes_overlap(a: [[f64; 2]; 2], b: [[f64; 2]; 2]) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

// ---- Helpers ----

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
fn sheet_block(session: &Session, name: Option<&str>) -> Result<String, ToolError> {
    let Some(name) = name else {
        return Ok(session.sheet.clone());
    };
    session
        .drawing
        .sheets()
        .into_iter()
        .find(|s| s.block.eq_ignore_ascii_case(name) || s.name.eq_ignore_ascii_case(name))
        .map(|s| s.block)
        .ok_or_else(|| {
            bad(&format!(
                "No sheet named {name}; qiyas_read state gives the sheets"
            ))
        })
}

pub(crate) fn sheet_units(session: &Session, sheet: &str) -> String {
    if sheet.eq_ignore_ascii_case("*Model_Space") {
        format!("drawing units ({})", session.info().units)
    } else {
        "paper millimetres".into()
    }
}

fn units(session: &Session) -> String {
    units_of(session, &session.sheet)
}

fn units_of(session: &Session, sheet: &str) -> String {
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

fn with_units(session: &Session, sheet: &str, mut out: Value) -> Value {
    out["units"] = json!(units_of(session, sheet));
    out
}

/// `rows` from `offset` on, with the total before paging.
fn page<T: serde::Serialize>(rows: Vec<T>, offset: usize) -> Value {
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

fn measured(outcome: Outcome) -> Result<Value, ToolError> {
    match outcome {
        Outcome::Measured { measurement } => Ok(json!({ "measurement": measurement })),
        _ => Err(ToolError::Failed("Nothing was measured".to_string())),
    }
}

fn info(outcome: Outcome) -> Result<Value, ToolError> {
    match outcome {
        Outcome::Info { title, rows } => Ok(json!({ "title": title, "rows": rows })),
        _ => Err(ToolError::Failed("Nothing was found".to_string())),
    }
}

/// The count of points just placed, with a redraw for the window (the drawing changed).
fn placed(outcome: Outcome) -> Result<Value, ToolError> {
    let mut out = measured(outcome)?;
    out["ui"] = json!({ "action": "redraw" });
    Ok(out)
}

/// Runs an edit of the measurement `id` and returns it as it is now.
fn edit(session: &mut Session, cmd: Command, id: &str, source: Source) -> Result<Value, ToolError> {
    match session.run_as(cmd, source)? {
        Outcome::Nothing => Err(no_measurement(id)),
        _ => Ok(json!({ "measurement": session.takeoff.get(id) })),
    }
}

fn no_measurement(id: &str) -> ToolError {
    bad(&format!(
        "No measurement with id {id}; qiyas_read records with collection takeoff gives the ids"
    ))
}

fn bad(why: &str) -> ToolError {
    ToolError::BadInput(why.to_string())
}

/// The box around the drawn segments of `handles` (entities on the sheet) on the current sheet.
fn drawn_bounds(session: &Session, handles: &[u64]) -> Result<[[f64; 2]; 2], ToolError> {
    let batch = session.batch();
    // A block reference has one range per entity drawn inside it.
    let mut ranges: HashMap<u64, Vec<(u32, u32)>> = HashMap::new();
    for r in &batch.ranges {
        ranges.entry(r.top).or_default().push((r.first, r.count));
    }
    let mut points = Vec::new();
    for &h in handles {
        session.entity_info(h, &[])?;
        let drawn = ranges
            .get(&h)
            .ok_or_else(|| bad(&format!("Entity {h} is not drawn on the sheet shown")))?;
        let [ox, oy] = batch.origin;
        for &(first, count) in drawn {
            let segments = &batch.segments[first as usize * 4..(first + count) as usize * 4];
            points.extend(
                segments
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|[x, y]| [f64::from(*x) + ox, f64::from(*y) + oy]),
            );
        }
    }
    Ok(bounds_of(&points))
}

fn bounds_of(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let first = points.first().copied().unwrap_or_default();
    points.iter().fold([first, first], |[lo, hi], p| {
        [
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        ]
    })
}

fn with_margin([lo, hi]: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let m = (hi[0] - lo[0]).max(hi[1] - lo[1]) * MARGIN;
    [[lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m]]
}

/// Widens or heightens `bounds` about its centre so a picture `width` pixels wide shows all of
/// it undistorted within 1600 pixels of height. A box with no size becomes one drawing unit.
fn fit([lo, hi]: [[f64; 2]; 2], width: u32) -> [[f64; 2]; 2] {
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (mut dx, mut dy) = (hi[0] - lo[0], hi[1] - lo[1]);
    if dx <= 0.0 && dy <= 0.0 {
        (dx, dy) = (1.0, 1.0);
    }
    let w = f64::from(width);
    // Height = width * dy / dx must stay within 1 and 1600 pixels.
    dx = dx.max(dy * w / f64::from(MAX_SIDE));
    dy = dy.max(dx / w);
    [
        [centre[0] - dx / 2.0, centre[1] - dy / 2.0],
        [centre[0] + dx / 2.0, centre[1] + dy / 2.0],
    ]
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
