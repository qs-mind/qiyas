//! The tools that write into the drawing: text, room areas and their labels, tables, shapes and
//! block references. Each is one undo step (a `Command::Batch` when it adds several things), and
//! every one only composes commands the window and the command line already have.

use std::collections::{BTreeMap, HashSet};

use qiyas_core::{Boundary, Current, DrawSpec, HatchPattern, TextRow};

use super::*;

/// Most items (texts, shapes, blocks) one call may add.
const MAX_ITEMS: usize = 200;
/// Most rooms one call looks for.
const MAX_ROOMS: usize = 500;
/// The first search radius round a seed, in metres.
const SEARCH_METRES: f64 = 1.0;
/// The gap between a room name and its label, in text heights.
const LABEL_GAP: f64 = 1.6;
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

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LabelLanguage {
    Ar,
    En,
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

/// Text height when none is given: 2.5 mm on paper at the annotation scale, in drawing units.
fn default_text_height(session: &Session) -> f64 {
    0.0025 / session.drawing.units.to_metres() * session.annotation_scale().1
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
    match session.run_as(Command::Batch { commands: all }, source)? {
        Outcome::Added { handles } => Ok((handles, created)),
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
    let default = default_text_height(session);
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
        let multiline = item.text.contains('\n') || width > 0.0;
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

/// One space found: the region round a seed.
struct Room {
    name: Option<String>,
    /// The text it was found from: its height and where it starts.
    from_text: Option<(f64, [f64; 2])>,
    point: [f64; 2],
    gross: f64,
    islands: Vec<Island>,
    also_named: Vec<String>,
    first: [f64; 2],
}

struct Island {
    area: f64,
    bounds: [[f64; 2]; 2],
}

/// A seed to look from: where to try, and what it is called.
struct Seed {
    name: Option<String>,
    from_text: Option<(f64, [f64; 2])>,
    tries: Vec<[f64; 2]>,
}

/// The area a ring encloses, edges bent by `bulges` (tan of a quarter of the arc's angle,
/// positive counter-clockwise, as a polyline keeps them): the shoelace sum plus, for each arc,
/// its segment between the arc and its chord.
fn ring_area(ring: &[[f64; 2]], bulges: &[f64]) -> f64 {
    let n = ring.len();
    let mut twice = 0.0;
    let mut segments = 0.0;
    for i in 0..n {
        let (p, q) = (ring[i], ring[(i + 1) % n]);
        twice += p[0] * q[1] - q[0] * p[1];
        let b = bulges.get(i).copied().unwrap_or(0.0);
        if b != 0.0 {
            let chord = (q[0] - p[0]).hypot(q[1] - p[1]);
            let angle = 4.0 * b.abs().atan();
            let radius = chord * (1.0 + b * b) / (4.0 * b.abs());
            let segment = radius * radius / 2.0 * (angle - angle.sin());
            segments += segment * b.signum();
        }
    }
    (twice / 2.0 + segments).abs()
}

fn bounds_of_ring(ring: &[[f64; 2]]) -> [[f64; 2]; 2] {
    bounds_of(ring)
}

fn looks_like_a_label(text: &str) -> bool {
    let t = text.trim();
    let lower = t.to_lowercase();
    t.parse::<f64>().is_ok()
        || lower.ends_with("m²")
        || lower.ends_with("m2")
        || t.ends_with("م²")
        || t.ends_with("م2")
}

/// The seeds the input gives: its points, else the room-name texts of the sheet.
fn room_seeds(session: &Session, input: &RoomsInput) -> Result<(Vec<Seed>, Vec<Value>), ToolError> {
    let mut skipped = Vec::new();
    if let Some(given) = &input.seeds {
        if input.text_layer.is_some() || input.names.is_some() || input.within.is_some() {
            return Err(bad(
                "Give seeds, or text_layer / names / within to take the room names from the drawing, not both",
            ));
        }
        let mut out = Vec::new();
        for s in given {
            let p = finite_point(s.point, "A seed's point")?;
            out.push(Seed {
                name: s.name.clone().filter(|n| !n.trim().is_empty()),
                from_text: None,
                tries: vec![p],
            });
        }
        return Ok((out, skipped));
    }
    let on_layer: Option<HashSet<u64>> = match &input.text_layer {
        Some(layer) => {
            if !has_layer(session, layer) {
                return Err(bad(&format!(
                    "No layer named {layer}; list_layers gives the names"
                )));
            }
            let mut handles = HashSet::new();
            for kind in ["Text", "MText"] {
                let page = session.find_entities(&EntityFilter {
                    layer: Some(layer.clone()),
                    kind: Some(kind.to_string()),
                    limit: 500,
                    ..Default::default()
                });
                handles.extend(page.rows.iter().map(|r| r.handle));
            }
            Some(handles)
        }
        None => None,
    };
    let names: Option<Vec<String>> = input
        .names
        .as_ref()
        .map(|n| n.iter().map(|s| s.trim().to_lowercase()).collect());
    let rows: Vec<TextRow> = session.read_text(None, input.within);
    let mut out = Vec::new();
    for row in rows {
        if row.xref.is_some() || row.inside_block.is_some() {
            continue;
        }
        let text = row.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if on_layer.as_ref().is_some_and(|h| !h.contains(&row.handle)) {
            continue;
        }
        if names
            .as_ref()
            .is_some_and(|n| !n.contains(&text.to_lowercase()))
        {
            continue;
        }
        if looks_like_a_label(&text) {
            skipped.push(json!({ "text": text, "point": row.at, "reason": "looks like an area or a number, not a room name" }));
            continue;
        }
        let h = if row.height > 0.0 {
            row.height
        } else {
            default_text_height(session)
        };
        let width = text.chars().count() as f64 * CHAR_WIDTH * h;
        out.push(Seed {
            name: Some(text),
            from_text: Some((h, row.at)),
            // The middle of the text first, then its start: a name sits inside its room.
            tries: vec![
                [row.at[0] + width / 2.0, row.at[1] + h / 2.0],
                [row.at[0], row.at[1] + h / 2.0],
                row.at,
            ],
        });
        if out.len() > MAX_ROOMS {
            return Err(bad(&format!(
                "More than {MAX_ROOMS} texts: narrow them with text_layer, names or within"
            )));
        }
    }
    Ok((out, skipped))
}

/// The spaces round the seeds, and the seeds that found none.
fn find_rooms(session: &Session, input: &RoomsInput) -> Result<(Vec<Room>, Vec<Value>), ToolError> {
    let (seeds, mut skipped) = room_seeds(session, input)?;
    if seeds.is_empty() {
        return Err(bad(
            "No room names found to start from: give seeds, or check text_layer and names (read_text shows the texts)",
        ));
    }
    let subtract = input.subtract_islands.unwrap_or(true);
    let start = SEARCH_METRES / session.drawing.units.to_metres();
    let mut rooms: Vec<Room> = Vec::new();
    for seed in seeds {
        let mut found = None;
        let mut dense = false;
        for &p in &seed.tries {
            match session.boundary_at(p, start) {
                Boundary::Ring {
                    ring,
                    holes,
                    bulges,
                } => {
                    found = Some((p, ring, holes, bulges));
                    break;
                }
                Boundary::TooDense => dense = true,
                Boundary::None => {}
            }
        }
        let label = seed.name.clone().unwrap_or_default();
        let Some((point, ring, holes, bulges)) = found else {
            let reason = if dense {
                "too much line work round it to look through"
            } else {
                "it sits in no closed space"
            };
            skipped.push(json!({ "text": label, "point": seed.tries[0], "reason": reason }));
            continue;
        };
        let gross = ring_area(&ring, bulges.first().map_or(&[][..], Vec::as_slice));
        let islands: Vec<Island> = holes
            .iter()
            .enumerate()
            .map(|(i, hole)| Island {
                area: ring_area(hole, bulges.get(i + 1).map_or(&[][..], Vec::as_slice)),
                bounds: bounds_of_ring(hole),
            })
            .collect();
        let first = ring.first().copied().unwrap_or(point);
        // One space with two names in it is one space.
        if let Some(same) = rooms
            .iter_mut()
            .find(|r| (r.gross - gross).abs() < 1e-9 * gross.max(1.0) && r.first == first)
        {
            same.also_named.extend(seed.name);
            continue;
        }
        rooms.push(Room {
            name: seed.name,
            from_text: seed.from_text,
            point,
            gross,
            islands: if subtract { islands } else { Vec::new() },
            also_named: Vec::new(),
            first,
        });
    }
    Ok((rooms, skipped))
}

impl Room {
    fn net(&self) -> f64 {
        self.gross - self.islands.iter().map(|i| i.area).sum::<f64>()
    }
}

pub(super) fn room_areas(
    session: &mut Session,
    input: RoomsInput,
    label: bool,
    source: Source,
) -> Result<Value, ToolError> {
    let to_m = session.drawing.units.to_metres();
    let sq = to_m * to_m;
    let (rooms, skipped) = find_rooms(session, &input)?;
    let decimals = input.decimals.unwrap_or(2);
    if decimals > 6 {
        return Err(bad("decimals is 0 to 6"));
    }
    let default = default_text_height(session);
    let label_height = positive(input.height, "The label height")?.map(|m| m / to_m);

    // Where each label goes, and what it says.
    let mut commands = Vec::new();
    let mut label_points = Vec::new();
    if label {
        for room in &rooms {
            let (height, at) = match room.from_text {
                Some((h, at)) => {
                    let height = label_height.unwrap_or(h);
                    (height, [at[0], at[1] - LABEL_GAP * h])
                }
                None => (label_height.unwrap_or(default), room.point),
            };
            label_points.push(at);
            let language = input.language.unwrap_or_else(|| {
                let has_arabic = room.name.as_deref().unwrap_or_default().chars().any(|c| {
                    matches!(c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')
                });
                if has_arabic {
                    LabelLanguage::Ar
                } else {
                    LabelLanguage::En
                }
            });
            let (prefix, unit) = match language {
                LabelLanguage::Ar => ("مساحة مرشحة غير معتمدة", "م²"),
                LabelLanguage::En => ("Unapproved candidate", "m²"),
            };
            commands.push(Command::Text {
                at,
                height,
                rotation: 0.0,
                // The number the answer reports (rounded as every result is), so the label and
                // the list agree even on a value that ends in 5.
                value: format!(
                    "{} {:.*} {}",
                    prefix,
                    decimals,
                    (room.net() * sq * DECIMALS).round() / DECIMALS,
                    unit
                ),
                style: None,
            });
        }
    }
    let written = if label {
        Some(run_batch(
            session,
            input.layer.as_deref(),
            None,
            commands,
            source,
        )?)
    } else {
        None
    };

    let rows: Vec<Value> = rooms
        .iter()
        .enumerate()
        .map(|(i, room)| {
            let mut row = json!({
                "name": room.name,
                // Compatibility field; the status below controls its meaning.
                "net_area_m2": room.net() * sq,
                "candidate_area_m2": room.net() * sq,
                "area_origin": "manual_or_entity",
                "region_method": "enclosed_drawn_linework",
                "measurement_basis": "unapproved_net",
                "room_type_status": "unapproved",
                "net_accuracy_eligible": false,
                "enclosed_area_m2": room.gross * sq,
                "islands": room.islands.iter().map(|i| json!({
                    "area_m2": i.area * sq,
                    "bounds": i.bounds,
                })).collect::<Vec<_>>(),
                "point": room.point,
            });
            if !room.also_named.is_empty() {
                row["also_named"] = json!(room.also_named);
            }
            if let Some((handles, _)) = &written {
                row["label_point"] = json!(label_points[i]);
                row["label_handle"] = json!(handles.get(i));
            }
            row
        })
        .collect();
    let mut out = json!({
        "rooms": rows,
        "skipped": skipped,
        "units": units(session),
    });
    if let Some((handles, created)) = written {
        out["labels_written"] = json!(handles.len());
        out["layer"] = json!(layer_used(session, &input.layer));
        out["layer_created"] = json!(created);
    }
    Ok(out)
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
        .map_or_else(|| default_text_height(session), |m| m / to_m);
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

fn spec_of(shape: &ShapeItem) -> Result<DrawSpec, ToolError> {
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
                PatternName::Ansi31 => HatchPattern::Ansi31,
                PatternName::Ansi37 => HatchPattern::Ansi37,
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
    let commands = input
        .shapes
        .iter()
        .map(|s| spec_of(s).map(Command::Draw))
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
