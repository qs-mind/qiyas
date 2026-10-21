//! Changing entities already on the sheet on screen (`Command::Modify`): move, copy, rotate,
//! scale, mirror, stretch, align, rectangular and polar arrays, and explode. Each is one undo
//! step. Only the drawing's own entities at the top of the sheet are changed (see
//! `Session::sheet_targets`); a command that can't change every one of them changes nothing.
//!
//! Moves, rotations, scales, mirrors (without keep), stretches and aligns change the entities in
//! place, under their own handles; copies, kept mirrors and arrays add new entities; explode
//! removes the entity and adds its parts.

use std::f64::consts::{PI, TAU};

use opencadcodec::entities::{
    AttachmentPoint, Insert, TextHorizontalAlignment, TextVerticalAlignment,
};
use opencadcodec::types::{Matrix3, Transform};
use opencadcodec::{Circle, EntityType, Handle, Vector3};
use qiyas_geom::{flatten, is_xref_block, Xform};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use std::collections::HashSet;

use crate::commands::insert_point;
use crate::session::{locate, variant_name};
use crate::takeoff::in_metres;
use crate::{Command, CoreError, Kind, Measurement, Outcome, Session, Target};

/// Most entities one command may add.
pub(crate) const MAX_NEW: usize = 100_000;

/// What `Command::Modify` does, to the entities `targets` (as picked, see `Target`). Points are
/// in drawing coordinates, angles in degrees counter-clockwise.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModifySpec {
    /// Moves them by `delta`.
    #[schemars(example = json!({"kind": "move", "targets": [{"handle": 1234}], "delta": [2, 0]}))]
    #[schemars(extend("x-words" = ["MOVE", "M"]))]
    Move {
        targets: Vec<Target>,
        delta: [f64; 2],
    },
    /// Adds `count` copies (1 or more), the first `delta` away, each next one `delta` further.
    #[schemars(example = json!({"kind": "copy", "targets": [{"handle": 1234}], "delta": [0, 3], "count": 4}))]
    #[schemars(extend("x-words" = ["COPY", "CO", "CP"]))]
    Copy {
        targets: Vec<Target>,
        delta: [f64; 2],
        count: u32,
    },
    /// Turns them `angle` about `base`; with `copy`, turns a copy and keeps them.
    #[schemars(example = json!({"kind": "rotate", "targets": [{"handle": 1234}], "base": [0, 0], "angle": 90}))]
    #[schemars(extend("x-words" = ["ROTATE", "RO"]))]
    Rotate {
        targets: Vec<Target>,
        base: [f64; 2],
        angle: f64,
        #[serde(default)]
        copy: bool,
    },
    /// Scales them by `factor` (above 0) about `base`; with `copy`, scales a copy.
    #[schemars(example = json!({"kind": "scale", "targets": [{"handle": 1234}], "base": [0, 0], "factor": 2}))]
    #[schemars(extend("x-words" = ["SCALE", "SC"]))]
    Scale {
        targets: Vec<Target>,
        base: [f64; 2],
        factor: f64,
        #[serde(default)]
        copy: bool,
    },
    /// Mirrors them across the line through `p1` and `p2`; with `keep`, mirrors a copy and keeps
    /// them. Texts still read forwards (AutoCAD's MIRRTEXT 0).
    #[schemars(example = json!({"kind": "mirror", "targets": [{"handle": 1234}], "p1": [5, 0], "p2": [5, 1], "keep": true}))]
    #[schemars(extend("x-words" = ["MIRROR", "MI"]))]
    Mirror {
        targets: Vec<Target>,
        p1: [f64; 2],
        p2: [f64; 2],
        keep: bool,
    },
    /// Moves by `delta` the vertices inside `window` (`[[min_x, min_y], [max_x, max_y]]`) of
    /// lines and polylines; any other entity moves whole when it lies wholly inside the window,
    /// or when its centre or insertion point does (circles, arcs, ellipses, texts, block
    /// references, points).
    #[schemars(example = json!({"kind": "stretch", "targets": [{"handle": 1234}], "window": [[4, -1], [6, 5]], "delta": [1, 0]}))]
    #[schemars(extend("x-words" = ["STRETCH", "S"]))]
    Stretch {
        targets: Vec<Target>,
        window: [[f64; 2]; 2],
        delta: [f64; 2],
    },
    /// Moves them so each pair's source point (the first) lands on its destination (the second):
    /// one pair moves; two pairs also turn them so the line between the sources lies along the
    /// line between the destinations (no scaling).
    #[schemars(example = json!({"kind": "align", "targets": [{"handle": 1234}], "pairs": [[[0, 0], [10, 10]], [[1, 0], [10, 11]]]}))]
    #[schemars(extend("x-words" = ["ALIGN", "AL"]))]
    Align {
        targets: Vec<Target>,
        pairs: Vec<[[f64; 2]; 2]>,
    },
    /// Copies them into `rows` x `cols` (1 or more each, 2 cells at least), `dy` apart up and `dx`
    /// across; they are the first cell.
    #[schemars(example = json!({"kind": "array_rect", "targets": [{"handle": 1234}], "rows": 3, "cols": 4, "dy": 6, "dx": 6}))]
    #[schemars(extend("x-words" = ["ARRAY", "AR"]))]
    ArrayRect {
        targets: Vec<Target>,
        rows: u32,
        cols: u32,
        dy: f64,
        dx: f64,
    },
    /// Copies them round `centre` into `count` items (2 or more, they are the first), each turned
    /// `angle` from the one before (90 with 4 items fills the circle).
    #[schemars(example = json!({"kind": "array_polar", "targets": [{"handle": 1234}], "centre": [0, 0], "count": 8, "angle": 45}))]
    ArrayPolar {
        targets: Vec<Target>,
        centre: [f64; 2],
        count: u32,
        angle: f64,
    },
    /// Breaks block references (one level: a block inside stays a reference; layer "0" and
    /// ByBlock take the reference's layer, colour, linetype and lineweight), polylines and
    /// multilines into their parts. Entities that have no parts are left as they are; nothing to
    /// explode at all is refused.
    #[schemars(example = json!({"kind": "explode", "targets": [{"handle": 1234}]}))]
    #[schemars(extend("x-words" = ["EXPLODE", "X"]))]
    Explode { targets: Vec<Target> },
}

impl ModifySpec {
    fn targets(&self) -> &[Target] {
        match self {
            ModifySpec::Move { targets, .. }
            | ModifySpec::Copy { targets, .. }
            | ModifySpec::Rotate { targets, .. }
            | ModifySpec::Scale { targets, .. }
            | ModifySpec::Mirror { targets, .. }
            | ModifySpec::Stretch { targets, .. }
            | ModifySpec::Align { targets, .. }
            | ModifySpec::ArrayRect { targets, .. }
            | ModifySpec::ArrayPolar { targets, .. }
            | ModifySpec::Explode { targets } => targets,
        }
    }
}

/// A transform of the plan and whether it mirrors (then the mirror's line angle, for texts).
/// `shift` is set for a plain translation: dimensions and tables take only those.
#[derive(Clone, Copy)]
pub(crate) struct Placing {
    transform: Transform,
    mirror_line: Option<f64>,
    shift: Option<[f64; 2]>,
}

impl Placing {
    pub(crate) fn plain(transform: Transform) -> Placing {
        Placing {
            transform,
            mirror_line: None,
            shift: None,
        }
    }

    pub(crate) fn shift(d: [f64; 2]) -> Placing {
        Placing {
            transform: translation(d),
            mirror_line: None,
            shift: Some(d),
        }
    }
}

fn v3([x, y]: [f64; 2]) -> Vector3 {
    Vector3::new(x, y, 0.0)
}

fn translation(d: [f64; 2]) -> Transform {
    Transform::from_translation(v3(d))
}

/// Turns `radians` about `base`.
pub(crate) fn rotation(base: [f64; 2], radians: f64) -> Transform {
    translation([-base[0], -base[1]])
        .then(&Transform::from_rotation(Vector3::UNIT_Z, radians))
        .then(&translation(base))
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn finite(values: &[f64]) -> Result<(), CoreError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid(
            "Points, distances and angles must be finite numbers",
        ))
    }
}

/// Whether `e` can be placed (and with `copying`, copied); why not. Dimensions and tables are
/// drawn from a block of their own, which isn't made again yet: they only move or copy by a
/// `translation` (their block moving with them).
fn check_placeable(e: &EntityType, copying: bool, translation: bool) -> Result<(), CoreError> {
    let h = e.common().handle.value();
    let refuse = |why: &str| Err(invalid(format!("{} {h:X} {why}", variant_name(e))));
    match e {
        EntityType::Dimension(_) | EntityType::Table(_) if !translation => {
            refuse("can only be moved or copied yet: its block is not drawn again")
        }
        EntityType::Viewport(_)
        | EntityType::Light(_)
        | EntityType::SectionSymbol(_)
        | EntityType::ViewBorder(_)
        | EntityType::Extended(_)
        | EntityType::Unknown(_) => refuse("can't be changed"),
        EntityType::RasterImage(_) | EntityType::Underlay(_) if copying => {
            refuse("can't be copied: attach the file again")
        }
        _ => Ok(()),
    }
}

/// Places `e` by `p`, its stored DWG graphics dropped (they would show it where it was).
pub(crate) fn place(e: &mut EntityType, p: &Placing) {
    match p.mirror_line {
        Some(line) => {
            e.apply_mirror(&p.transform);
            keep_readable(e, line);
        }
        None => e.apply_transform(&p.transform),
    }
    e.common_mut().graphic_data = None;
}

/// `place`, keeping a dimension's own block placement (its insertion point): the block is moved
/// with it by the caller. The name of that block, for a dimension.
pub(crate) fn place_keeping_blocks(e: &mut EntityType, p: &Placing) -> Option<String> {
    let kept = match e {
        EntityType::Dimension(d) => Some(d.base().insertion_point),
        _ => None,
    };
    place(e, p);
    match (e, kept) {
        (EntityType::Dimension(d), Some(at)) => {
            d.base_mut().insertion_point = at;
            Some(d.base().block_name.clone())
        }
        _ => None,
    }
}

/// A text's justification by DXF codes (horizontal 0 left, 1 centre, 2 right, 3 aligned,
/// 4 middle, 5 fit; vertical 0 baseline, 1 bottom, 2 middle, 3 top), its angle and its two points.
struct Justified {
    rotation: f64,
    h: u8,
    v: u8,
    insertion: Vector3,
    alignment: Option<Vector3>,
}

/// A mirrored line of text made to read forwards again, as AutoCAD's MIRRTEXT 0 does. When the
/// mirror line runs more across the text than along it, the text turns half round and its
/// justification swaps left and right; otherwise it swaps top and bottom. Either way it covers
/// about what the mirror image covered.
fn readable(t: Justified, line: f64) -> Justified {
    let d = t.rotation - line;
    let across = d.sin().abs() > d.cos().abs();
    let governs = if t.h == 0 && t.v == 0 {
        t.insertion
    } else {
        t.alignment.unwrap_or(t.insertion)
    };
    let two_points = matches!(t.h, 3 | 5);
    let mut out = Justified { ..t };
    if across {
        out.rotation = (t.rotation + PI).rem_euclid(TAU);
        if two_points {
            // The baseline runs the other way: its ends swap.
            if let Some(a) = t.alignment {
                out.insertion = a;
                out.alignment = Some(t.insertion);
            }
            return out;
        }
        out.h = match t.h {
            0 => 2,
            2 => 0,
            h => h,
        };
    } else {
        if two_points || t.h == 4 {
            return out;
        }
        out.v = match t.v {
            0 | 1 => 3,
            3 => 1,
            v => v,
        };
    }
    out.insertion = governs;
    out.alignment = (out.h != 0 || out.v != 0).then_some(governs);
    out
}

/// Keeps a mirrored text, multiline text, or a block reference's attributes reading forwards.
fn keep_readable(e: &mut EntityType, line: f64) {
    use TextHorizontalAlignment as H;
    use TextVerticalAlignment as V;
    match e {
        EntityType::Text(t) => {
            // The codec slants a mirrored text the other way; a readable one keeps its slant.
            t.oblique_angle = -t.oblique_angle;
            let h = match t.horizontal_alignment {
                H::Left => 0,
                H::Center => 1,
                H::Right => 2,
                H::Aligned => 3,
                H::Middle => 4,
                H::Fit => 5,
            };
            let v = match t.vertical_alignment {
                V::Baseline => 0,
                V::Bottom => 1,
                V::Middle => 2,
                V::Top => 3,
            };
            let out = readable(
                Justified {
                    rotation: t.rotation,
                    h,
                    v,
                    insertion: t.insertion_point,
                    alignment: t.alignment_point,
                },
                line,
            );
            t.rotation = out.rotation;
            t.horizontal_alignment =
                [H::Left, H::Center, H::Right, H::Aligned, H::Middle, H::Fit][usize::from(out.h)];
            t.vertical_alignment = [V::Baseline, V::Bottom, V::Middle, V::Top][usize::from(out.v)];
            t.insertion_point = out.insertion;
            t.alignment_point = out.alignment;
        }
        EntityType::MText(t) => {
            use AttachmentPoint as A;
            let d = t.rotation - line;
            let across = d.sin().abs() > d.cos().abs();
            if across {
                t.rotation = (t.rotation + PI).rem_euclid(TAU);
            }
            let at = t.attachment_point as u8;
            let (row, col) = ((at - 1) / 3, (at - 1) % 3);
            let (row, col) = if across {
                (row, 2 - col)
            } else {
                (2 - row, col)
            };
            t.attachment_point = [
                A::TopLeft,
                A::TopCenter,
                A::TopRight,
                A::MiddleLeft,
                A::MiddleCenter,
                A::MiddleRight,
                A::BottomLeft,
                A::BottomCenter,
                A::BottomRight,
            ][usize::from(row * 3 + col)];
        }
        EntityType::Insert(insert) => {
            for a in &mut insert.attributes {
                let out = readable(
                    Justified {
                        rotation: a.rotation,
                        h: a.horizontal_alignment as u8,
                        v: a.vertical_alignment as u8,
                        insertion: a.insertion_point,
                        alignment: Some(a.alignment_point),
                    },
                    line,
                );
                use opencadcodec::entities::{HorizontalAlignment as AH, VerticalAlignment as AV};
                a.rotation = out.rotation;
                a.horizontal_alignment = [
                    AH::Left,
                    AH::Center,
                    AH::Right,
                    AH::Aligned,
                    AH::Middle,
                    AH::Fit,
                ][usize::from(out.h)];
                a.vertical_alignment =
                    [AV::Baseline, AV::Bottom, AV::Middle, AV::Top][usize::from(out.v)];
                a.insertion_point = out.insertion;
                a.alignment_point = out.alignment.unwrap_or(out.insertion);
            }
        }
        _ => {}
    }
}

/// The world point of `p`, stored in the object coordinates of `normal`.
fn world(normal: Vector3, p: Vector3) -> Vector3 {
    Matrix3::arbitrary_axis(normal) * p
}

/// `delta` (world) in the object coordinates of `normal`.
fn to_object(normal: Vector3, delta: Vector3) -> Vector3 {
    Matrix3::arbitrary_axis(normal).transpose() * delta
}

impl Session {
    /// `Command::Modify`: see `ModifySpec`. `Changed` for a change in place (`Nothing` when a
    /// stretch moved nothing), `Added` with the new handles for copies, arrays and explode.
    pub(crate) fn modify(&mut self, spec: ModifySpec) -> Result<Outcome, CoreError> {
        let handles = self.sheet_targets(spec.targets())?;
        match spec {
            ModifySpec::Move { delta, .. } => {
                finite(&delta)?;
                self.place_all(&handles, Placing::shift(delta))
            }
            ModifySpec::Copy { delta, count, .. } => {
                finite(&delta)?;
                if count == 0 {
                    return Err(invalid("Make 1 copy or more"));
                }
                let copies = (1..=count)
                    .map(|i| Placing::shift([delta[0] * f64::from(i), delta[1] * f64::from(i)]))
                    .collect();
                self.copy_all(&handles, copies)
            }
            ModifySpec::Rotate {
                base, angle, copy, ..
            } => {
                finite(&[base[0], base[1], angle])?;
                let p = Placing::plain(rotation(base, angle.to_radians()));
                self.place_or_copy(&handles, p, copy)
            }
            ModifySpec::Scale {
                base, factor, copy, ..
            } => {
                finite(&[base[0], base[1], factor])?;
                if factor <= 0.0 {
                    return Err(invalid("The scale factor must be more than 0"));
                }
                let t = Transform::from_scaling_with_origin(
                    Vector3::new(factor, factor, factor),
                    v3(base),
                );
                self.place_or_copy(&handles, Placing::plain(t), copy)
            }
            ModifySpec::Mirror { p1, p2, keep, .. } => {
                finite(&[p1[0], p1[1], p2[0], p2[1]])?;
                if p1 == p2 {
                    return Err(invalid("The mirror line needs two different points"));
                }
                let p = Placing {
                    transform: Transform::from_mirror_line(v3(p1), v3(p2)),
                    mirror_line: Some((p2[1] - p1[1]).atan2(p2[0] - p1[0])),
                    shift: None,
                };
                self.place_or_copy(&handles, p, keep)
            }
            ModifySpec::Stretch { window, delta, .. } => {
                let [lo, hi] = window;
                finite(&[lo[0], lo[1], hi[0], hi[1], delta[0], delta[1]])?;
                if !(lo[0] < hi[0] && lo[1] < hi[1]) {
                    return Err(invalid("The window's first corner must be its lower left"));
                }
                self.stretch(&handles, window, delta)
            }
            ModifySpec::Align { pairs, .. } => {
                let flat: Vec<f64> = pairs.iter().flatten().flatten().copied().collect();
                finite(&flat)?;
                let t = match pairs.as_slice() {
                    [[s, d]] => translation([d[0] - s[0], d[1] - s[1]]),
                    [[s1, d1], [s2, d2]] => {
                        if s1 == s2 || d1 == d2 {
                            return Err(invalid("Each pair's points must differ from the other's"));
                        }
                        let turn = (d2[1] - d1[1]).atan2(d2[0] - d1[0])
                            - (s2[1] - s1[1]).atan2(s2[0] - s1[0]);
                        translation([d1[0] - s1[0], d1[1] - s1[1]]).then(&rotation(*d1, turn))
                    }
                    _ => return Err(invalid("Align takes one or two pairs of points")),
                };
                self.place_all(&handles, Placing::plain(t))
            }
            ModifySpec::ArrayRect {
                rows, cols, dy, dx, ..
            } => {
                finite(&[dy, dx])?;
                if rows == 0 || cols == 0 || u64::from(rows) * u64::from(cols) < 2 {
                    return Err(invalid("An array needs 2 items or more"));
                }
                self.check_count(&handles, u64::from(rows) * u64::from(cols) - 1)?;
                let copies = (0..rows)
                    .flat_map(|r| (0..cols).map(move |c| (r, c)))
                    .skip(1)
                    .map(|(r, c)| {
                        Placing::plain(translation([dx * f64::from(c), dy * f64::from(r)]))
                    })
                    .collect();
                self.copy_all(&handles, copies)
            }
            ModifySpec::ArrayPolar {
                centre,
                count,
                angle,
                ..
            } => {
                finite(&[centre[0], centre[1], angle])?;
                if count < 2 {
                    return Err(invalid("An array needs 2 items or more"));
                }
                if angle == 0.0 {
                    return Err(invalid("The angle between items must not be 0"));
                }
                self.check_count(&handles, u64::from(count) - 1)?;
                let copies = (1..count)
                    .map(|i| Placing::plain(rotation(centre, (angle * f64::from(i)).to_radians())))
                    .collect();
                self.copy_all(&handles, copies)
            }
            ModifySpec::Explode { .. } => self.explode(&handles),
        }
    }

    /// Refuses `copies` copies of `handles` when that is more than one command may add.
    fn check_count(&self, handles: &[Handle], copies: u64) -> Result<(), CoreError> {
        if copies.saturating_mul(handles.len() as u64) > MAX_NEW as u64 {
            return Err(invalid(format!(
                "That makes more than {MAX_NEW} copies: use fewer items"
            )));
        }
        Ok(())
    }

    pub(crate) fn entity(&self, h: Handle) -> &EntityType {
        self.drawing
            .doc
            .get_entity(h)
            .expect("sheet_targets found it")
    }

    fn place_or_copy(
        &mut self,
        handles: &[Handle],
        p: Placing,
        copy: bool,
    ) -> Result<Outcome, CoreError> {
        if copy {
            self.copy_all(handles, vec![p])
        } else {
            self.place_all(handles, p)
        }
    }

    /// Places every entity `handles` by `p`, in place; the take-off rows measuring them follow.
    fn place_all(&mut self, handles: &[Handle], p: Placing) -> Result<Outcome, CoreError> {
        for &h in handles {
            check_placeable(self.entity(h), false, p.shift.is_some())?;
        }
        for &h in handles {
            let e = self.drawing.doc.get_entity_mut(h).expect("checked above");
            let dimension_block = place_keeping_blocks(e, &p);
            if let (Some(name), Some(d)) = (dimension_block, p.shift) {
                self.shift_block(&name, d);
            }
        }
        let unlinked = self.follow_takeoff(handles, &[]);
        Ok(Self::changed_unlinking(unlinked))
    }

    /// Moves every entity of the block `name` by `d`.
    pub(crate) fn shift_block(&mut self, name: &str, d: [f64; 2]) {
        let inside: Vec<Handle> = self
            .drawing
            .doc
            .block_records
            .get(name)
            .map(|b| b.entity_handles.clone())
            .unwrap_or_default();
        for h in inside {
            if let Some(e) = self.drawing.doc.get_entity_mut(h) {
                place(e, &Placing::shift(d));
            }
        }
    }

    /// Adds a copy of every entity `handles` placed by each of `copies`.
    fn copy_all(&mut self, handles: &[Handle], copies: Vec<Placing>) -> Result<Outcome, CoreError> {
        self.check_count(handles, copies.len() as u64)?;
        for &h in handles {
            let translations = copies.iter().all(|p| p.shift.is_some());
            check_placeable(self.entity(h), true, translations)?;
        }
        let owner = self.sheet_owner();
        let mut added = Vec::with_capacity(handles.len() * copies.len());
        for p in &copies {
            let start = added.len();
            for &h in handles {
                let mut copy = self.entity(h).clone();
                let dimension_block = place_keeping_blocks(&mut copy, p);
                // A dimension or a table gets a block of its own: a dimension's is drawn in world
                // coordinates and moves with it; a table's sits where the table refers to it.
                match &mut copy {
                    EntityType::Dimension(d) => {
                        let name = dimension_block.unwrap_or_default();
                        if let Some((name, _)) = self.copy_block(&name, "*D", p.shift)? {
                            d.base_mut().block_name = name;
                        }
                    }
                    EntityType::Table(t) => {
                        // A DWG reader names the block by its record handle only.
                        let name = qiyas_geom::table_block(&self.drawing.doc, t)
                            .map(|b| b.name.clone())
                            .unwrap_or_default();
                        if let Some((name, handle)) = self.copy_block(&name, "*T", None)? {
                            t.block_name = name;
                            t.block_record_handle = Some(handle);
                        }
                    }
                    _ => {}
                }
                added.push(self.add_fresh(copy, owner)?);
            }
            self.copied_wall_groups(&added[start..], p.mirror_line.is_some())?;
        }
        Ok(Outcome::Added { handles: added })
    }

    /// A copy of the anonymous block `name` under the first free `prefix`N name, its entities
    /// moved by `shift`; its name and record handle. `None` when there is no such block.
    fn copy_block(
        &mut self,
        name: &str,
        prefix: &str,
        shift: Option<[f64; 2]>,
    ) -> Result<Option<(String, Handle)>, CoreError> {
        let Some(source) = self.drawing.doc.block_records.get(name).cloned() else {
            return Ok(None);
        };
        let doc = &mut self.drawing.doc;
        let fresh = (1..)
            .map(|n| format!("{prefix}{n}"))
            .find(|n| doc.block_records.get(n).is_none())
            .expect("a free name");
        let mut record = opencadcodec::BlockRecord::new(fresh.clone());
        record.handle = doc.allocate_handle();
        record.block_entity_handle = doc.allocate_handle();
        record.block_end_handle = doc.allocate_handle();
        record.flags.anonymous = true;
        record.base_point = source.base_point;
        let handle = record.handle;
        doc.block_records
            .add(record)
            .map_err(|e| invalid(e.to_string()))?;
        for h in source.entity_handles {
            let Some(e) = self.drawing.doc.get_entity(h) else {
                continue;
            };
            let mut e = e.clone();
            if let Some(d) = shift {
                place(&mut e, &Placing::shift(d));
            }
            self.add_fresh(e, Some(handle))?;
        }
        Ok(Some((fresh, handle)))
    }

    /// Adds `e` (a copy of an entity, or a part of one) to the block `owner` (the sheet on
    /// screen) as a new entity: new handles for it and its attributes, none of its parts' old
    /// handles (vertices, faces, the sequence end), and nothing that ties it to the one it came
    /// from (reactors, extension dictionary, hatch boundary links).
    pub(crate) fn add_fresh(
        &mut self,
        e: EntityType,
        owner: Option<Handle>,
    ) -> Result<u64, CoreError> {
        add_fresh_to(&mut self.drawing.doc, e, owner)
    }

    /// `ModifySpec::Stretch`.
    fn stretch(
        &mut self,
        handles: &[Handle],
        window: [[f64; 2]; 2],
        delta: [f64; 2],
    ) -> Result<Outcome, CoreError> {
        for &h in handles {
            check_placeable(self.entity(h), false, false)?;
        }
        let [lo, hi] = window;
        let inside = |p: Vector3| lo[0] <= p.x && p.x <= hi[0] && lo[1] <= p.y && p.y <= hi[1];
        let d = v3(delta);
        let whole = Placing::plain(translation(delta));
        let mut moved = false;
        for &h in handles {
            let e = self.entity(h);
            // Vertices inside, by index; `None` for an entity without vertices to stretch.
            let vertices: Option<Vec<usize>> = match e {
                EntityType::Line(l) => Some(
                    [l.start, l.end]
                        .into_iter()
                        .enumerate()
                        .filter(|(_, p)| inside(*p))
                        .map(|(i, _)| i)
                        .collect(),
                ),
                EntityType::LwPolyline(pl) => Some(
                    (0..pl.vertices.len())
                        .filter(|&i| {
                            let at = pl.vertices[i].location;
                            inside(world(pl.normal, Vector3::new(at.x, at.y, pl.elevation)))
                        })
                        .collect(),
                ),
                EntityType::Polyline2D(pl) => Some(
                    (0..pl.vertices.len())
                        .filter(|&i| inside(world(pl.normal, pl.vertices[i].location)))
                        .collect(),
                ),
                EntityType::Polyline(pl) => Some(
                    (0..pl.vertices.len())
                        .filter(|&i| inside(pl.vertices[i].location))
                        .collect(),
                ),
                EntityType::Polyline3D(pl) => Some(
                    (0..pl.vertices.len())
                        .filter(|&i| inside(pl.vertices[i].position))
                        .collect(),
                ),
                _ => None,
            };
            match vertices {
                Some(at) if at.is_empty() => {}
                Some(at) => {
                    moved = true;
                    let e = self.drawing.doc.get_entity_mut(h).expect("found above");
                    e.common_mut().graphic_data = None;
                    match e {
                        EntityType::Line(l) => {
                            for i in at {
                                if i == 0 {
                                    l.start = l.start + d;
                                } else {
                                    l.end = l.end + d;
                                }
                            }
                        }
                        EntityType::LwPolyline(pl) => {
                            let o = to_object(pl.normal, d);
                            for i in at {
                                let v = &mut pl.vertices[i].location;
                                v.x += o.x;
                                v.y += o.y;
                            }
                        }
                        EntityType::Polyline2D(pl) => {
                            let o = to_object(pl.normal, d);
                            for i in at {
                                let v = &mut pl.vertices[i].location;
                                v.x += o.x;
                                v.y += o.y;
                            }
                        }
                        EntityType::Polyline(pl) => {
                            for i in at {
                                pl.vertices[i].location = pl.vertices[i].location + d;
                            }
                        }
                        EntityType::Polyline3D(pl) => {
                            for i in at {
                                pl.vertices[i].position = pl.vertices[i].position + d;
                            }
                        }
                        _ => unreachable!("only entities with vertices"),
                    }
                }
                None => {
                    let anchor = match e {
                        EntityType::Circle(c) => Some(world(c.normal, c.center)),
                        EntityType::Arc(a) => Some(world(a.normal, a.center)),
                        EntityType::Ellipse(el) => Some(el.center),
                        EntityType::Text(t) => Some(world(t.normal, t.insertion_point)),
                        EntityType::MText(t) => Some(t.insertion_point),
                        EntityType::Insert(i) => Some(world(i.normal, i.insert_point)),
                        EntityType::Point(p) => Some(p.location),
                        _ => None,
                    };
                    let goes = match anchor {
                        Some(p) => inside(p),
                        None => self.wholly_inside(e, window),
                    };
                    if goes {
                        moved = true;
                        let e = self.drawing.doc.get_entity_mut(h).expect("found above");
                        place(e, &whole);
                    }
                }
            }
        }
        if !moved {
            return Ok(Outcome::Nothing);
        }
        let unlinked = self.follow_takeoff(handles, &[]);
        Ok(Self::changed_unlinking(unlinked))
    }

    /// Every point of `e` as drawn lies in `window`.
    fn wholly_inside(&self, e: &EntityType, [lo, hi]: [[f64; 2]; 2]) -> bool {
        let paths =
            flatten(&self.drawing.doc, e, &Xform::identity(), self.batch_tol()).into_paths();
        !paths.is_empty()
            && paths
                .iter()
                .flat_map(|p| &p.points)
                .all(|p| lo[0] <= p[0] && p[0] <= hi[0] && lo[1] <= p[1] && p[1] <= hi[1])
    }

    /// `ModifySpec::Explode`. `Exploded` with the parts' handles and how many entities picked had
    /// no parts and were left as they are.
    fn explode(&mut self, handles: &[Handle]) -> Result<Outcome, CoreError> {
        let doc = &self.drawing.doc;
        let too_many = || {
            invalid(format!(
                "That makes more than {MAX_NEW} entities: explode fewer at a time"
            ))
        };
        let mut parts: Vec<(Handle, Vec<EntityType>)> = Vec::new();
        // Array cells times the block's entities, counted before any part is made.
        let mut estimate = 0usize;
        for &h in handles {
            let e = self.entity(h);
            let pieces = match e {
                EntityType::Insert(insert) => {
                    if is_xref_block(doc, &insert.block_name) {
                        return Err(invalid(format!(
                            "Entity {:X} is the xref {}: an xref can't be exploded",
                            h.value(),
                            insert.block_name
                        )));
                    }
                    let cells = usize::from(insert.column_count.max(1))
                        * usize::from(insert.row_count.max(1));
                    let children = doc.entities_in_block(&insert.block_name).count();
                    estimate = estimate.saturating_add(cells.saturating_mul(children));
                    if estimate > MAX_NEW {
                        return Err(too_many());
                    }
                    explode_insert(doc, insert)
                }
                EntityType::LwPolyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline(_)
                | EntityType::Polyline3D(_)
                | EntityType::MLine(_) => e.explode(),
                _ => Vec::new(),
            };
            if !pieces.is_empty() {
                parts.push((h, pieces));
            }
        }
        if parts.is_empty() {
            return Err(invalid(
                "Nothing picked can explode: only block references, polylines and multilines do",
            ));
        }
        if parts.iter().map(|(_, p)| p.len()).sum::<usize>() > MAX_NEW {
            return Err(too_many());
        }
        let left_alone = handles.len() - parts.len();
        let owner = self.sheet_owner();
        let mut added = Vec::new();
        let mut removed = Vec::with_capacity(parts.len());
        for (h, pieces) in parts {
            self.drawing.doc.remove_entity_arc(h);
            removed.push(h);
            for piece in pieces {
                added.push(self.add_fresh(piece, owner)?);
            }
        }
        let (remeasured, unlinked) = self.follow_erase(&removed);
        Ok(Outcome::Exploded {
            handles: added,
            left_alone,
            remeasured,
            unlinked,
        })
    }
}

/// The entities of `insert`'s block placed where it draws them (every cell of an array, its
/// offsets turned with the reference, from the block's base point), one level deep, layer "0"
/// and ByBlock resolved to the reference's; its visible attributes as texts (or multiline texts)
/// holding their values. A circle stays a circle under an even scale (the codec makes every
/// circle an ellipse).
fn explode_insert(doc: &opencadcodec::CadDocument, insert: &Insert) -> Vec<EntityType> {
    let even = (insert.x_scale().abs() - insert.y_scale().abs()).abs() < 1e-10;
    let base = doc
        .block_records
        .get(&insert.block_name)
        .map_or(Vector3::ZERO, |b| b.base_point);
    let from_base = Transform::from_translation(Vector3::new(-base.x, -base.y, -base.z));
    // One reference per array cell: the codec's own array offsets don't turn with the reference.
    let (sin, cos) = insert.rotation.sin_cos();
    let mut cells = Vec::new();
    for col in 0..insert.column_count.max(1) {
        for row in 0..insert.row_count.max(1) {
            let (x, y) = (
                f64::from(col) * insert.column_spacing,
                f64::from(row) * insert.row_spacing,
            );
            let mut cell = insert.clone();
            cell.column_count = 1;
            cell.row_count = 1;
            cell.column_spacing = 0.0;
            cell.row_spacing = 0.0;
            cell.insert_point.x += x * cos - y * sin;
            cell.insert_point.y += x * sin + y * cos;
            cells.push(cell);
        }
    }
    let mut out = Vec::new();
    for child in doc.entities_in_block(&insert.block_name) {
        let mut moved = child.clone();
        moved.apply_transform(&from_base);
        for cell in &cells {
            for mut piece in cell.explode(std::slice::from_ref(&moved)) {
                if let (true, EntityType::Circle(c), EntityType::Ellipse(el)) =
                    (even, child, &piece)
                {
                    let mut circle =
                        Circle::from_center_radius(Vector3::ZERO, el.major_axis.length());
                    circle.common = el.common.clone();
                    circle.normal = el.normal;
                    circle.center = to_object(el.normal, el.center);
                    circle.thickness = c.thickness;
                    piece = EntityType::Circle(circle);
                }
                by_block_linetype(piece.common_mut(), insert);
                out.push(piece);
            }
        }
    }
    for a in &insert.attributes {
        if a.common.invisible || a.flags.invisible || a.value.is_empty() {
            continue;
        }
        let mut common = a.common.clone();
        if common.layer == "0" {
            common.layer = insert.common.layer.clone();
        }
        if common.color == opencadcodec::Color::ByBlock {
            common.color = insert.common.color;
        }
        if common.line_weight == opencadcodec::LineWeight::ByBlock {
            common.line_weight = insert.common.line_weight;
        }
        by_block_linetype(&mut common, insert);
        if let Some(mtext) = a.embedded_mtext.as_deref().filter(|_| a.is_multiline) {
            let mut m = mtext.clone();
            m.common = common;
            out.push(EntityType::MText(m));
            continue;
        }
        use TextHorizontalAlignment as H;
        use TextVerticalAlignment as V;
        let (h, v) = (a.horizontal_alignment as u8, a.vertical_alignment as u8);
        out.push(EntityType::Text(opencadcodec::Text {
            common,
            value: a.value.clone(),
            insertion_point: a.insertion_point,
            alignment_point: (h != 0 || v != 0).then_some(a.alignment_point),
            height: a.height,
            rotation: a.rotation,
            width_factor: a.width_factor,
            oblique_angle: a.oblique_angle,
            style: a.text_style.clone(),
            horizontal_alignment: [H::Left, H::Center, H::Right, H::Aligned, H::Middle, H::Fit]
                [usize::from(h.min(5))],
            vertical_alignment: [V::Baseline, V::Bottom, V::Middle, V::Top][usize::from(v.min(3))],
            normal: a.normal,
            thickness: 0.0,
            generation_flags: a.text_generation_flags,
        }));
    }
    out
}

/// `Session::add_fresh` into `doc`, which may be another drawing than the session's.
pub(crate) fn add_fresh_to(
    doc: &mut opencadcodec::CadDocument,
    mut e: EntityType,
    owner: Option<Handle>,
) -> Result<u64, CoreError> {
    let c = e.common_mut();
    c.handle = Handle::NULL;
    c.reactors.clear();
    c.xdictionary_handle = None;
    c.graphic_data = None;
    if let Some(owner) = owner {
        c.owner_handle = owner;
    }
    match &mut e {
        EntityType::Insert(insert) => {
            let h = doc.allocate_handle();
            insert.common.handle = h;
            insert.seqend_handle = None;
            for a in &mut insert.attributes {
                a.common.handle = doc.allocate_handle();
                a.common.owner_handle = h;
                a.common.reactors.clear();
                a.common.xdictionary_handle = None;
            }
        }
        EntityType::Hatch(hatch) => {
            hatch.is_associative = false;
            for path in &mut hatch.paths {
                path.boundary_handles.clear();
            }
        }
        // The writers give a null handle a new one, and write any other as it is.
        EntityType::Polyline3D(pl) => {
            for v in &mut pl.vertices {
                v.handle = Handle::NULL;
            }
        }
        EntityType::PolyfaceMesh(mesh) => {
            for v in &mut mesh.vertices {
                v.common.handle = Handle::NULL;
            }
            for f in &mut mesh.faces {
                f.common.handle = Handle::NULL;
            }
            mesh.seqend_handle = None;
        }
        EntityType::PolygonMesh(mesh) => {
            for v in &mut mesh.vertices {
                v.common.handle = Handle::NULL;
            }
        }
        _ => {}
    }
    let h = doc.add_entity(e).map_err(|e| invalid(e.to_string()))?;
    Ok(h.value())
}

/// A ByBlock linetype takes the reference's.
fn by_block_linetype(c: &mut opencadcodec::entities::EntityCommon, insert: &Insert) {
    if c.linetype.eq_ignore_ascii_case("ByBlock") {
        c.linetype = insert.common.linetype.clone();
    }
}

/// `m` no longer measures any entity: it keeps its value and points.
fn unlink(m: &mut Measurement) {
    m.handles.clear();
    m.chain.clear();
    m.block_path.clear();
    m.xref = None;
}

/// The take-off row `m` measures the entity `h` at the top of the sheet: one of its handles, or
/// the block (or xref) reference its chain starts with.
pub(crate) fn measures(m: &Measurement, h: u64) -> bool {
    match m.chain.first() {
        Some(&top) => top == h,
        None => m.handles.contains(&h),
    }
}

impl Session {
    /// Besides the entities, what `cmd` changes and so keeps in its undo step: the drawing's
    /// tables (a copied dimension's or table's block; an erased image's or underlay's definition
    /// link) and the take-off (rows measuring a target).
    pub(crate) fn document_extras(&self, cmd: &Command) -> (bool, bool) {
        let edited;
        let (targets, copying, erasing) = match cmd {
            Command::Walls(_)
            | Command::AddWallOpenings { .. }
            | Command::RemoveWallOpenings { .. }
            | Command::EditWallOpening { .. } => return (true, true),
            Command::DeleteEntities { targets } => (targets.as_slice(), false, true),
            Command::Modify(spec) => (
                spec.targets(),
                matches!(spec, ModifySpec::Copy { .. }),
                false,
            ),
            Command::Edit(spec) => {
                edited = spec.targets();
                (edited.as_slice(), false, false)
            }
            Command::Text { value, .. }
            | Command::MText { value, .. }
            | Command::EditText { value, .. } => {
                let field = crate::field::has_fields(value) && self.needs_field_app();
                return (self.needs_text_app() || field, false);
            }
            // Typed text is marked; a block copied from another drawing is a new record.
            Command::InsertBlock {
                attribute_values,
                from_path,
                ..
            } => {
                let typed = !attribute_values.is_empty() && self.needs_text_app();
                return (typed || from_path.is_some(), false);
            }
            Command::AttDef { .. }
            | Command::EditAttributes { .. }
            | Command::EditAttDef { .. } => {
                return (self.needs_text_app(), false);
            }
            Command::CreateBlock { targets, .. } => (targets.as_slice(), false, false),
            // Rows measuring a reference to the block are measured again.
            Command::RedefineBlock { .. } => return (false, true),
            // A dimension's or a table's block drawn again, typed text marked; rows on a target
            // follow its layer and shape.
            Command::SetProperty { target, prop, .. } => {
                let typed = *prop == crate::EntityProp::Text && self.needs_text_app();
                let (tables, takeoff) = self.annotation_extras(std::slice::from_ref(target));
                return (tables || typed, takeoff);
            }
            Command::MatchProperties { targets, .. } => return self.annotation_extras(targets),
            _ => return (false, false),
        };
        let doc = &self.drawing.doc;
        let tables = targets
            .iter()
            .any(|t| match doc.get_entity(Handle::new(t.handle)) {
                Some(EntityType::Dimension(_) | EntityType::Table(_)) => copying,
                Some(EntityType::RasterImage(_) | EntityType::Underlay(_)) => erasing,
                _ => false,
            })
            // A table's field reading an entity draws the table's block again.
            || self.tables_follow_fields();
        let takeoff = targets
            .iter()
            .any(|t| self.takeoff.items.iter().any(|m| measures(m, t.handle)));
        (tables, takeoff)
    }

    /// For an edit of `targets`' properties: the tables when one is a dimension or a table
    /// (its block is drawn again) or a table's field reads an entity, the take-off when a row
    /// measures one.
    fn annotation_extras(&self, targets: &[Target]) -> (bool, bool) {
        let doc = &self.drawing.doc;
        let tables = self.tables_follow_fields()
            || targets.iter().any(|t| {
                matches!(
                    doc.get_entity(Handle::new(t.handle)),
                    Some(EntityType::Dimension(_) | EntityType::Table(_))
                )
            });
        let takeoff = targets
            .iter()
            .any(|t| self.takeoff.items.iter().any(|m| measures(m, t.handle)));
        (tables, takeoff)
    }

    /// The take-off follows a drawing edit: a row measuring an entity `removed` and others too
    /// is measured again without it (a count counts one less); one measuring only removed ones
    /// keeps its value and points but loses the link; rows measuring an entity `changed` are
    /// measured again. How many rows lost their link because they could no longer be measured
    /// (see `follow_edit`).
    pub(crate) fn follow_takeoff(&mut self, changed: &[Handle], removed: &[Handle]) -> usize {
        self.follow_edit(changed, removed, &[], &[])
    }

    /// `follow_takeoff` for entities `removed` (erased or exploded), and what it did to the rows:
    /// how many were measured again and how many lost their link (`Outcome::TakeoffFollowed`).
    pub(crate) fn follow_erase(&mut self, removed: &[Handle]) -> (usize, usize) {
        let gone: HashSet<u64> = removed.iter().map(|h| h.value()).collect();
        let (mut partly, mut whole) = (0, 0);
        for m in &self.takeoff.items {
            if !gone.iter().any(|&h| measures(m, h)) {
                continue;
            }
            if m.chain.is_empty() && m.handles.iter().any(|h| !gone.contains(h)) {
                partly += 1;
            } else {
                whole += 1;
            }
        }
        // Only a row measured again can fail to measure.
        let failed = self.follow_takeoff(&[], removed);
        (partly - failed, whole + failed)
    }

    /// `Outcome::Changed`, or `TakeoffFollowed` when `unlinked` take-off rows lost their link.
    fn changed_unlinking(unlinked: usize) -> Outcome {
        if unlinked == 0 {
            Outcome::Changed
        } else {
            Outcome::TakeoffFollowed {
                remeasured: 0,
                unlinked,
            }
        }
    }

    /// `follow_takeoff`, for edits that cut entities into pieces or join them: a row measuring
    /// an entity of `pieces` (one cut into pieces, with the new pieces' handles) measures every
    /// piece too; a row measuring a `(gone, into)` of `merged` (joined into another) measures
    /// `into` instead. Every row measuring a changed, cut or joined entity is then measured
    /// again, as the sum of its entities; one that can't be any more (an area whose outline was
    /// opened) loses its link like a row on an erased entity, keeping its value and points. How
    /// many rows lost their link that way.
    pub(crate) fn follow_edit(
        &mut self,
        changed: &[Handle],
        removed: &[Handle],
        pieces: &[(Handle, Vec<u64>)],
        merged: &[(Handle, Handle)],
    ) -> usize {
        for m in &mut self.takeoff.items {
            if !m.chain.is_empty() {
                continue;
            }
            for (cut, new) in pieces {
                if m.handles.contains(&cut.value()) {
                    for &n in new {
                        if !m.handles.contains(&n) {
                            m.handles.push(n);
                        }
                    }
                }
            }
            for &(gone, into) in merged {
                if m.handles.contains(&gone.value()) {
                    m.handles.retain(|&h| h != gone.value());
                    if !m.handles.contains(&into.value()) {
                        m.handles.push(into.value());
                    }
                }
            }
        }
        let removed: HashSet<u64> = removed.iter().map(|h| h.value()).collect();
        // Rows that measured an erased entity and others too: measured again without it.
        let mut partly: Vec<usize> = Vec::new();
        for (i, m) in self.takeoff.items.iter_mut().enumerate() {
            if !removed.iter().any(|&h| measures(m, h)) {
                continue;
            }
            if m.chain.is_empty() {
                let before = m.handles.len();
                m.handles.retain(|h| !removed.contains(h));
                if !m.handles.is_empty() {
                    if m.kind == Kind::Count {
                        // Each reference counts one.
                        m.value -= (before - m.handles.len()) as f64;
                        m.shown = m.value;
                    }
                    partly.push(i);
                }
            } else {
                unlink(m);
            }
        }
        let touched: HashSet<u64> = changed
            .iter()
            .chain(pieces.iter().map(|(cut, _)| cut))
            .chain(merged.iter().map(|(_, into)| into))
            .map(|h| h.value())
            .collect();
        let again: Vec<usize> = (0..self.takeoff.items.len())
            .filter(|&i| {
                let m = &self.takeoff.items[i];
                partly.contains(&i) || touched.iter().any(|&h| measures(m, h))
            })
            .collect();
        let mut unlinked = 0;
        for i in again {
            match self.remeasured(&self.takeoff.items[i]) {
                Some(m) => self.takeoff.items[i] = m,
                // An item's row is not measured from its entities: it stays as it is.
                None if self.takeoff.items[i].kind == Kind::Item => {}
                None => {
                    unlink(&mut self.takeoff.items[i]);
                    unlinked += 1;
                }
            }
        }
        unlinked
    }

    /// How many take-off rows measure the entity `h`.
    pub(crate) fn rows_measuring(&self, h: Handle) -> usize {
        self.takeoff
            .items
            .iter()
            .filter(|m| measures(m, h.value()))
            .count()
    }

    /// `m` measured again from its entities as they are now, as a new measurement of them would
    /// be; `None` when it can't be (an entity gone, an item).
    fn remeasured(&self, m: &Measurement) -> Option<Measurement> {
        let xrefs = self.xref_docs();
        let located = m
            .handles
            .iter()
            .map(|&h| locate(&self.drawing.doc, &xrefs, h, &m.chain))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let mut out = m.clone();
        match m.kind {
            Kind::Length => {
                out.value = located
                    .iter()
                    .map(|e| self.extent(e.doc, e.entity, &e.xf).length)
                    .sum::<Option<f64>>()?;
            }
            Kind::Area | Kind::Volume => {
                let mut crossing = false;
                let area = located
                    .iter()
                    .map(|e| {
                        let extent = self.extent(e.doc, e.entity, &e.xf);
                        crossing |= extent.self_intersecting;
                        extent.area
                    })
                    .sum::<Option<f64>>()?;
                out.self_intersecting = crossing;
                out.value = area * m.height.unwrap_or(1.0);
            }
            // The count stays; an xref's references are kept as their places.
            Kind::Count => {
                if m.xref.is_some() {
                    out.points = located.iter().filter_map(insert_point).collect();
                }
                return Some(out);
            }
            Kind::Item => return None,
        }
        let (unit, shown) = in_metres(out.kind, out.value, self.drawing.units);
        out.unit = unit.to_string();
        out.shown = shown;
        Some(out)
    }
}
