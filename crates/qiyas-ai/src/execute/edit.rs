//! Mizan's edit tools: `modify` (move, copy, rotate, scale, mirror, array), `edit` (offset, trim,
//! extend, fillet, chamfer), `erase` and `undo_last`. Each is one undo step of the core's own
//! commands (`Command::Modify`, `Command::Edit`, `Command::DeleteEntities`), so they keep its rules:
//! the drawing's own entities at the top of the sheet only, locked layers refused, the take-off
//! following the edit. Handles come from find_entities; points and lengths are drawing units,
//! angles degrees.

use qiyas_core::{EditSpec, EntityProp, ModifySpec, Outcome, Target};

use super::*;

fn targets(handles: &[u64]) -> Vec<Target> {
    handles
        .iter()
        .map(|&handle| Target {
            handle,
            chain: Vec::new(),
        })
        .collect()
}

fn target(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

fn need<T>(value: Option<T>, field: &str, kind: &str) -> Result<T, ToolError> {
    value.ok_or_else(|| bad(&format!("{kind} needs {field}")))
}

/// What a drawing edit answers: what it did, and the handles it made or took away.
fn answer(outcome: Outcome) -> Value {
    let message = match &outcome {
        Outcome::Edited { added, removed, .. } => {
            format!("{} added, {} removed", added.len(), removed.len())
        }
        Outcome::Added { handles } => format!("{} added", handles.len()),
        _ => "Done".to_string(),
    };
    json!({ "message": message, "outcome": outcome })
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum ModifyKind {
    Move,
    Copy,
    Rotate,
    Scale,
    Mirror,
    Array,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModifyInput {
    kind: ModifyKind,
    handles: Vec<u64>,
    delta: Option<[f64; 2]>,
    count: Option<u32>,
    base: Option<[f64; 2]>,
    angle: Option<f64>,
    factor: Option<f64>,
    copy: Option<bool>,
    p1: Option<[f64; 2]>,
    p2: Option<[f64; 2]>,
    rows: Option<u32>,
    cols: Option<u32>,
    dx: Option<f64>,
    dy: Option<f64>,
}

pub(super) fn modify(
    session: &mut Session,
    input: ModifyInput,
    source: Source,
) -> Result<Value, ToolError> {
    if input.handles.is_empty() {
        return Err(bad(
            "handles: name at least one entity (find_entities gives them)",
        ));
    }
    let t = targets(&input.handles);
    let copy = input.copy.unwrap_or(false);
    let spec = match input.kind {
        ModifyKind::Move => ModifySpec::Move {
            targets: t,
            delta: need(input.delta, "delta", "move")?,
        },
        ModifyKind::Copy => ModifySpec::Copy {
            targets: t,
            delta: need(input.delta, "delta", "copy")?,
            count: input.count.unwrap_or(1),
        },
        ModifyKind::Rotate => ModifySpec::Rotate {
            targets: t,
            base: need(input.base, "base", "rotate")?,
            angle: need(input.angle, "angle", "rotate")?,
            copy,
        },
        ModifyKind::Scale => ModifySpec::Scale {
            targets: t,
            base: need(input.base, "base", "scale")?,
            factor: need(input.factor, "factor", "scale")?,
            copy,
        },
        ModifyKind::Mirror => ModifySpec::Mirror {
            targets: t,
            p1: need(input.p1, "p1", "mirror")?,
            p2: need(input.p2, "p2", "mirror")?,
            keep: copy,
        },
        ModifyKind::Array => ModifySpec::ArrayRect {
            targets: t,
            rows: input.rows.unwrap_or(1),
            cols: input.cols.unwrap_or(1),
            dy: input.dy.unwrap_or(0.0),
            dx: input.dx.unwrap_or(0.0),
        },
    };
    Ok(answer(session.run_as(Command::Modify(spec), source)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum EditKind {
    Offset,
    Trim,
    Extend,
    Fillet,
    Chamfer,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EditInput {
    kind: EditKind,
    handle: u64,
    /// The second line of a fillet or chamfer.
    other: Option<u64>,
    /// Offset: the distance; chamfer: back along the first line.
    distance: Option<f64>,
    /// Chamfer: back along the second line (the same as `distance` when left out).
    distance2: Option<f64>,
    /// Fillet: the arc's radius (0 meets the lines at their corner).
    radius: Option<f64>,
    /// Offset: a point on the side to offset to; trim: a point on the piece to cut away; extend:
    /// a point near the end to extend; fillet and chamfer: a point on the first line's kept side.
    point: [f64; 2],
    /// Fillet and chamfer: a point on the second line's kept side.
    point2: Option<[f64; 2]>,
    /// Trim's cutters or extend's boundaries; left out, everything drawn on the sheet.
    edges: Option<Vec<u64>>,
}

pub(super) fn edit(
    session: &mut Session,
    input: EditInput,
    source: Source,
) -> Result<Value, ToolError> {
    let edges = targets(input.edges.as_deref().unwrap_or(&[]));
    let spec = match input.kind {
        EditKind::Offset => EditSpec::Offset {
            target: target(input.handle),
            distance: need(input.distance, "distance", "offset")?,
            side_point: input.point,
        },
        EditKind::Trim => EditSpec::Trim {
            cutters: edges,
            target: target(input.handle),
            at_point: input.point,
        },
        EditKind::Extend => EditSpec::Extend {
            boundaries: edges,
            target: target(input.handle),
            at_end: input.point,
        },
        EditKind::Fillet => EditSpec::Fillet {
            a: target(input.handle),
            b: target(need(input.other, "other", "fillet")?),
            at_a: input.point,
            at_b: need(input.point2, "point2", "fillet")?,
            radius: input.radius.unwrap_or(0.0),
        },
        EditKind::Chamfer => {
            let d1 = need(input.distance, "distance", "chamfer")?;
            EditSpec::Chamfer {
                a: target(input.handle),
                b: target(need(input.other, "other", "chamfer")?),
                at_a: input.point,
                at_b: need(input.point2, "point2", "chamfer")?,
                d1,
                d2: input.distance2.unwrap_or(d1),
            }
        }
    };
    Ok(answer(session.run_as(Command::Edit(spec), source)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EraseInput {
    handles: Vec<u64>,
}

pub(super) fn erase(
    session: &mut Session,
    input: EraseInput,
    source: Source,
) -> Result<Value, ToolError> {
    if input.handles.is_empty() {
        return Err(bad(
            "handles: name at least one entity (find_entities gives them)",
        ));
    }
    let outcome = session.run_as(
        Command::DeleteEntities {
            targets: targets(&input.handles),
        },
        source,
    )?;
    let mut out = answer(outcome);
    out["message"] = json!(format!("Erased {}", input.handles.len()));
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetTextInput {
    handle: u64,
    text: Option<String>,
    /// Metres, as add_text takes it.
    height: Option<f64>,
    rotation: Option<f64>,
    layer: Option<String>,
}

/// Changes a text or MText in place (its words, height in metres, rotation in degrees, layer):
/// fixing a label is changing it, never writing another one over it. Each change is a step.
pub(super) fn set_text(
    session: &mut Session,
    input: SetTextInput,
    source: Source,
) -> Result<Value, ToolError> {
    let t = target(input.handle);
    let mut changed = Vec::new();
    if let Some(text) = input.text {
        session.run_as(
            Command::EditText {
                target: t.clone(),
                value: text,
                tag: None,
            },
            source,
        )?;
        changed.push("text");
    }
    let per_metre = 1.0 / session.drawing.units.to_metres();
    let props = [
        (
            EntityProp::TextHeight,
            input.height.map(|h| json!(h * per_metre)),
            "height",
        ),
        (
            EntityProp::TextRotation,
            input.rotation.map(|r| json!(r)),
            "rotation",
        ),
        (EntityProp::Layer, input.layer.map(|l| json!(l)), "layer"),
    ];
    for (prop, value, name) in props {
        if let Some(value) = value {
            session.run_as(
                Command::SetProperty {
                    target: t.clone(),
                    prop,
                    value,
                },
                source,
            )?;
            changed.push(name);
        }
    }
    if changed.is_empty() {
        return Err(bad("Give text, height, rotation or layer to change"));
    }
    Ok(json!({ "message": format!("Changed {}", changed.join(", ")), "handle": input.handle }))
}

/// Undoes the newest step when it is the assistant's (`Session::undo_if`): never the user's.
pub(super) fn undo_last(session: &mut Session, source: Source) -> Result<Value, ToolError> {
    if session.undo_if(source) {
        Ok(json!({ "message": "Undone", "undone": true, "ui": { "action": "redraw" } }))
    } else {
        Err(bad(
            "The newest step is not yours (or there is none): only the user can undo it",
        ))
    }
}
