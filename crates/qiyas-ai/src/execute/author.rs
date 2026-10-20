//! The tools that write into the drawing: text, room areas and their labels, tables, shapes and
//! block references. Each is one undo step (a `Command::Batch` when it adds several things), and
//! every one only composes commands the window and the command line already have.

use std::collections::BTreeMap;

use qiyas_core::rooms::{LabelLanguage, RoomSeed, RoomsQuery};
use qiyas_core::{Current, DrawSpec, HatchPattern};

use super::*;

/// Most items (texts, shapes, blocks) one call may add.
const MAX_ITEMS: usize = 200;
/// A text's width is about this many text heights per character.
const CHAR_WIDTH: f64 = 0.6;

// ---- Inputs ----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TextsInput {
    texts: Vec<TextItem>,
    layer: Option<String>,
    style: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextItem {
    text: String,
    point: [f64; 2],
    height: Option<f64>,
    rotation: Option<f64>,
    width: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RoomsInput {
    seeds: Option<Vec<SeedInput>>,
    text_layer: Option<String>,
    names: Option<Vec<String>>,
    within: Option<[[f64; 2]; 2]>,
    subtract_islands: Option<bool>,
    // label_spaces only.
    layer: Option<String>,
    height: Option<f64>,
    decimals: Option<usize>,
    language: Option<LabelLanguage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedInput {
    name: Option<String>,
    point: [f64; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TableInput {
    point: [f64; 2],
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    text_height: Option<f64>,
    row_height: Option<f64>,
    col_width: Option<f64>,
    layer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CellsInput {
    handle: u64,
    cells: Vec<CellItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellItem {
    row: usize,
    col: usize,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DrawInput {
    shapes: Vec<ShapeItem>,
    layer: Option<String>,
    color: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum ShapeKind {
    Line,
    Polyline,
    Rectangle,
    Circle,
    Arc,
    Hatch,
    Revcloud,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum PatternName {
    Solid,
    Ansi31,
    Ansi37,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShapeItem {
    kind: ShapeKind,
    a: Option<[f64; 2]>,
    b: Option<[f64; 2]>,
    points: Option<Vec<[f64; 2]>>,
    bulges: Option<Vec<f64>>,
    closed: Option<bool>,
    centre: Option<[f64; 2]>,
    radius: Option<f64>,
    start: Option<f64>,
    end: Option<f64>,
    pattern: Option<PatternName>,
    arc_length: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BlocksInput {
    blocks: Vec<BlockItem>,
    layer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockItem {
    name: String,
    point: [f64; 2],
    scale: Option<f64>,
    rotation: Option<f64>,
    #[serde(default)]
    attributes: Vec<AttributeItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeItem {
    tag: String,
    value: String,
}

// ---- Shared ----

fn finite_point(p: [f64; 2], what: &str) -> Result<[f64; 2], ToolError> {
    if p.iter().all(|v| v.is_finite()) {
        Ok(p)
    } else {
        Err(bad(&format!("{what} is not a number")))
    }
}

fn positive(v: Option<f64>, what: &str) -> Result<Option<f64>, ToolError> {
    match v {
        Some(v) if !(v.is_finite() && v > 0.0) => Err(bad(&format!("{what} must be above 0"))),
        other => Ok(other),
    }
}

fn at_most(n: usize, what: &str) -> Result<(), ToolError> {
    if n == 0 {
        return Err(bad(&format!("Give at least one of {what}")));
    }
    if n > MAX_ITEMS {
        return Err(bad(&format!("At most {MAX_ITEMS} {what} in one call")));
    }
    Ok(())
}

/// The layer `name` as the drawing spells it, if the drawing has it.
fn own_layer(session: &Session, name: &str) -> Option<String> {
    session
        .drawing
        .doc
        .layers
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(name))
        .map(|l| l.name.clone())
}

/// Runs `commands` as one undo step on `layer` and `color` (a layer the drawing lacks is made,
/// white), the current layer and colour put back after. Returns the entities added and whether
/// the layer was made.
fn run_batch(
    session: &mut Session,
    layer: Option<&str>,
    color: Option<&str>,
    commands: Vec<Command>,
    source: Source,
) -> Result<(Vec<u64>, bool), ToolError> {
    let before = session.current.clone();
    let mut now: Current = before.clone();
    let mut all = Vec::new();
    let mut created = false;
    if let Some(name) = layer.map(str::trim).filter(|n| !n.is_empty()) {
        match own_layer(session, name) {
            Some(spelled) => now.layer = spelled,
            None => {
                if name.contains('|') {
                    return Err(bad("A layer of an xref can't take new entities"));
                }
                all.push(Command::NewLayer {
                    name: name.to_string(),
                });
                now.layer = name.to_string();
                created = true;
            }
        }
    }
    if let Some(color) = color {
        now.color = color.to_string();
    }
    let changed = now != before;
    if changed {
        all.push(Command::SetCurrent { current: now });
    }
    all.extend(commands);
    if changed {
        all.push(Command::SetCurrent { current: before });
    }
    let batch = Command::Batch {
        commands: all,
        layer: None,
        color: None,
    };
    match session.run_as(batch, source)? {
        Outcome::Batch { added, .. } if !added.is_empty() => Ok((added, created)),
        _ => Err(ToolError::Failed("Nothing was drawn".to_string())),
    }
}

// ---- add_text ----

pub(super) fn add_text(
    session: &mut Session,
    input: TextsInput,
    source: Source,
) -> Result<Value, ToolError> {
    at_most(input.texts.len(), "texts")?;
    let to_m = session.drawing.units.to_metres();
    let default = session.default_text_height();
    let mut commands = Vec::new();
    let mut shown = Vec::new();
    for item in &input.texts {
        if item.text.trim().is_empty() {
            return Err(bad("A text can't be empty"));
        }
        let at = finite_point(item.point, "A text's point")?;
        let height = positive(item.height, "A text's height")?.map_or(default, |m| m / to_m);
        let width = match item.width {
            Some(w) if !(w.is_finite() && w >= 0.0) => {
                return Err(bad("A text's width must be 0 or more"))
            }
            Some(w) => w / to_m,
            None => 0.0,
        };
        let rotation = item.rotation.unwrap_or(0.0);
        if !rotation.is_finite() {
            return Err(bad("A text's rotation is not a number"));
        }
        let multiline =
            item.text.contains(['\n', '\r']) || item.text.contains("\\n") || width > 0.0;
        if multiline && rotation != 0.0 {
            return Err(bad("Only single-line text can be rotated"));
        }
        commands.push(if multiline {
            Command::MText {
                at,
                width,
                height,
                value: item.text.clone(),
                style: input.style.clone(),
            }
        } else {
            Command::Text {
                at,
                height,
                rotation,
                value: item.text.clone(),
                style: input.style.clone(),
            }
        });
        shown.push(
            json!({ "kind": if multiline { "MText" } else { "Text" }, "height_m": height * to_m }),
        );
    }
    let (handles, created) = run_batch(session, input.layer.as_deref(), None, commands, source)?;
    for (row, handle) in shown.iter_mut().zip(&handles) {
        row["handle"] = json!(handle);
    }
    Ok(json!({
        "texts": shown,
        "layer": layer_used(session, &input.layer),
        "layer_created": created,
    }))
}

/// The layer the entities of a call went on: the one asked for, else the current one.
fn layer_used(session: &Session, asked: &Option<String>) -> String {
    asked
        .as_deref()
        .and_then(|n| own_layer(session, n.trim()))
        .unwrap_or_else(|| session.current.layer.clone())
}

// ---- room_areas, label_spaces ----

pub(super) fn room_areas(
    session: &mut Session,
    input: RoomsInput,
    label: bool,
    source: Source,
) -> Result<Value, ToolError> {
    let query = RoomsQuery {
        seeds: input.seeds.map(|seeds| {
            seeds
                .into_iter()
                .map(|seed| RoomSeed {
                    name: seed.name,
                    point: seed.point,
                })
                .collect()
        }),
        text_layer: input.text_layer,
        names: input.names,
        within: input.within,
        subtract_islands: input.subtract_islands,
    };
    let command = if label {
        Command::LabelSpaces {
            query,
            layer: input.layer,
            height: input.height,
            decimals: input.decimals,
            language: input.language,
        }
    } else {
        // These fields belonged to both legacy tools, including the read-only one.
        if input.decimals.unwrap_or(2) > 6 {
            return Err(bad("decimals is 0 to 6"));
        }
        positive(input.height, "The label height")?;
        Command::Rooms { query }
    };
    match session.run_as(command, source) {
        Ok(Outcome::Rooms { report }) => Ok(json!(report)),
        Err(CoreError::InvalidCommand(message)) if label && message == "Nothing was drawn" => {
            Err(ToolError::Failed(message))
        }
        Err(error) => Err(error.into()),
        _ => Err(ToolError::Failed(
            "No rooms report was returned".to_string(),
        )),
    }
}

// ---- add_table, edit_table_cell ----

pub(super) fn add_table(
    session: &mut Session,
    input: TableInput,
    source: Source,
) -> Result<Value, ToolError> {
    if input.columns.is_empty() {
        return Err(bad("A table needs at least one column"));
    }
    if input.rows.iter().any(|r| r.len() > input.columns.len()) {
        return Err(bad("A row has more cells than there are columns"));
    }
    let at = finite_point(input.point, "The table's point")?;
    let to_m = session.drawing.units.to_metres();
    let text = positive(input.text_height, "The text height")?
        .map_or_else(|| session.default_text_height(), |m| m / to_m);
    let row_height = positive(input.row_height, "The row height")?
        .map(|m| m / to_m)
        .unwrap_or(2.0 * text);
    let longest = input
        .columns
        .iter()
        .chain(input.rows.iter().flatten())
        .map(|c| c.chars().count())
        .max()
        .unwrap_or(1) as f64;
    // The table's text is half its row height.
    let col_width = positive(input.col_width, "The column width")?
        .map(|m| m / to_m)
        .unwrap_or((longest * CHAR_WIDTH + 1.0) * row_height / 2.0);
    let (rows, cols) = (input.rows.len() + 1, input.columns.len());
    let mut data = vec![input.columns];
    data.extend(input.rows);
    let cmd = Command::Table {
        at,
        rows,
        cols,
        col_width,
        row_height,
        data,
    };
    let (handles, created) = run_batch(session, input.layer.as_deref(), None, vec![cmd], source)?;
    let table = handles.first().copied();
    Ok(json!({
        "table": table,
        "rows": rows,
        "columns": cols,
        "col_width_m": col_width * to_m,
        "row_height_m": row_height * to_m,
        "bounds": [[at[0], at[1] - rows as f64 * row_height], [at[0] + cols as f64 * col_width, at[1]]],
        "layer_created": created,
        "units": units(session),
    }))
}

pub(super) fn edit_table_cell(
    session: &mut Session,
    input: CellsInput,
    source: Source,
) -> Result<Value, ToolError> {
    at_most(input.cells.len(), "cells")?;
    let target = Target {
        handle: input.handle,
        chain: Vec::new(),
    };
    let commands: Vec<Command> = input
        .cells
        .into_iter()
        .map(|c| Command::EditCell {
            target: target.clone(),
            row: c.row,
            col: c.col,
            text: c.text,
        })
        .collect();
    let n = commands.len();
    run_batch(session, None, None, commands, source)?;
    Ok(json!({ "table": input.handle, "cells_changed": n }))
}

// ---- draw ----

fn need<T>(v: Option<T>, what: &str, kind: &str) -> Result<T, ToolError> {
    v.ok_or_else(|| bad(&format!("A {kind} needs {what}")))
}

/// The draw spec of one shape; patterns are scaled by `hatch_scale` (the Hatch tool's default,
/// 25 in millimetres, 0.025 in metres).
fn spec_of(shape: &ShapeItem, hatch_scale: f64) -> Result<DrawSpec, ToolError> {
    let point = |p: Option<[f64; 2]>, what: &str, kind: &str| -> Result<[f64; 2], ToolError> {
        finite_point(need(p, what, kind)?, what)
    };
    let ring = |kind: &str, least: usize| -> Result<Vec<[f64; 2]>, ToolError> {
        let points = need(shape.points.clone(), "points", kind)?;
        if points.len() < least {
            return Err(bad(&format!("A {kind} needs at least {least} points")));
        }
        for p in &points {
            finite_point(*p, "A point")?;
        }
        Ok(points)
    };
    Ok(match shape.kind {
        ShapeKind::Line => DrawSpec::Line {
            a: point(shape.a, "a", "line")?,
            b: point(shape.b, "b", "line")?,
        },
        ShapeKind::Rectangle => DrawSpec::Rectangle {
            a: point(shape.a, "a", "rectangle")?,
            b: point(shape.b, "b", "rectangle")?,
        },
        ShapeKind::Polyline => {
            let points = ring("polyline", 2)?;
            let bulges = shape.bulges.clone().unwrap_or_default();
            if !bulges.is_empty() && bulges.len() != points.len() {
                return Err(bad("bulges has one number per point, or none"));
            }
            DrawSpec::Polyline {
                points,
                bulges,
                closed: shape.closed.unwrap_or(false),
            }
        }
        ShapeKind::Circle => DrawSpec::Circle {
            centre: point(shape.centre, "centre", "circle")?,
            radius: positive(Some(need(shape.radius, "radius", "circle")?), "The radius")?
                .unwrap_or_default(),
        },
        ShapeKind::Arc => DrawSpec::Arc {
            centre: point(shape.centre, "centre", "arc")?,
            radius: positive(Some(need(shape.radius, "radius", "arc")?), "The radius")?
                .unwrap_or_default(),
            start: need(shape.start, "start", "arc")?,
            end: need(shape.end, "end", "arc")?,
        },
        ShapeKind::Hatch => DrawSpec::Hatch {
            boundary_handles: Vec::new(),
            points: ring("hatch", 3)?,
            holes: Vec::new(),
            bulges: Vec::new(),
            pattern: match shape.pattern.unwrap_or(PatternName::Solid) {
                PatternName::Solid => HatchPattern::Solid,
                // The pattern table is in millimetres: at scale 1 a metre plan would get lines
                // 3 m apart, a millimetre one lines that plot as solid.
                PatternName::Ansi31 => HatchPattern::Custom {
                    name: "ANSI31".into(),
                    angle: 0.0,
                    scale: hatch_scale,
                },
                PatternName::Ansi37 => HatchPattern::Custom {
                    name: "ANSI37".into(),
                    angle: 0.0,
                    scale: hatch_scale,
                },
            },
        },
        ShapeKind::Revcloud => DrawSpec::RevCloud {
            points: ring("revcloud", 3)?,
            arc_len: positive(
                Some(need(shape.arc_length, "arc_length", "revcloud")?),
                "The arc length",
            )?
            .unwrap_or_default(),
        },
    })
}

pub(super) fn draw(
    session: &mut Session,
    input: DrawInput,
    source: Source,
) -> Result<Value, ToolError> {
    at_most(input.shapes.len(), "shapes")?;
    let hatch_scale = 0.025 / session.drawing.units.to_metres();
    let commands = input
        .shapes
        .iter()
        .map(|s| spec_of(s, hatch_scale).map(Command::Draw))
        .collect::<Result<Vec<_>, _>>()?;
    let (handles, created) = run_batch(
        session,
        input.layer.as_deref(),
        input.color.as_deref(),
        commands,
        source,
    )?;
    Ok(json!({ "shapes": handles, "layer_created": created, "units": units(session) }))
}

// ---- insert_block ----

pub(super) fn insert_block(
    session: &mut Session,
    input: BlocksInput,
    source: Source,
) -> Result<Value, ToolError> {
    at_most(input.blocks.len(), "blocks")?;
    let mut commands = Vec::new();
    for item in &input.blocks {
        if session.drawing.doc.block_records.get(&item.name).is_none() {
            return Err(bad(&format!(
                "No block named {}; block_summary lists the blocks",
                item.name
            )));
        }
        let scale = positive(item.scale, "The scale")?.unwrap_or(1.0);
        let rotation = item.rotation.unwrap_or(0.0);
        if !rotation.is_finite() {
            return Err(bad("The rotation is not a number"));
        }
        commands.push(Command::InsertBlock {
            name: item.name.clone(),
            at: finite_point(item.point, "The insertion point")?,
            scale,
            rotation,
            attribute_values: item
                .attributes
                .iter()
                .map(|a| (a.tag.clone(), a.value.clone()))
                .collect::<BTreeMap<_, _>>(),
            from_path: None,
        });
    }
    let (handles, created) = run_batch(session, input.layer.as_deref(), None, commands, source)?;
    Ok(json!({ "blocks": handles, "layer_created": created }))
}
