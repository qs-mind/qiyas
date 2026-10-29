//! Centre marks, centre lines, symmetry axes and labelled reference grids.

use opencadcodec::entities::{TextHorizontalAlignment, TextVerticalAlignment};
use opencadcodec::tables::{LineType, LineTypeElement};
use opencadcodec::{EntityType, Handle};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CoreError, Current, DrawSpec, Session, Target};

/// Grid bubble provenance survives moving labels, layer changes and DWG/DXF saves.
pub(crate) const GRID_APP: &str = "QIYAS_GRID";

/// A labelled axis at a coordinate in drawing units.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GridAxis {
    pub label: String,
    pub at: f64,
}

/// The rectangle spanned by the grid, in drawing units.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub struct GridExtent {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn finite(point: [f64; 2]) -> bool {
    point.into_iter().all(f64::is_finite)
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn along(a: [f64; 2], direction: [f64; 2], distance: f64) -> [f64; 2] {
    [
        a[0] + direction[0] * distance,
        a[1] + direction[1] * distance,
    ]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn unit(a: [f64; 2]) -> Result<[f64; 2], CoreError> {
    let length = a[0].hypot(a[1]);
    if !length.is_finite() || length <= 0.0 {
        return Err(invalid("A centre line needs finite, non-zero lines"));
    }
    Ok([a[0] / length, a[1] / length])
}

fn target(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

impl Session {
    /// Installs a standard centre or hidden pattern only when missing. Pattern lengths use
    /// paper millimetres, converted through the sheet's units and the header's LTSCALE.
    pub(crate) fn ensure_convention_linetype(&mut self, name: &str) -> Result<bool, CoreError> {
        let (name, pattern, description): (&str, &[f64], &str) =
            if name.eq_ignore_ascii_case("CENTER") {
                ("CENTER", &[12.5, -2.5, 2.5, -2.5], "Centre line")
            } else if name.eq_ignore_ascii_case("HIDDEN") {
                ("HIDDEN", &[6.25, -3.125], "Hidden line")
            } else {
                return Ok(false);
            };
        if self.drawing.doc.line_types.contains(name) {
            return Ok(true);
        }
        let global = self.drawing.doc.header.linetype_scale;
        if !global.is_finite() || global <= 0.0 {
            return Err(invalid("The drawing's linetype scale must be positive"));
        }
        let scale = self.default_text_height() / 2.5 / global;
        let mut linetype = LineType::new(name);
        linetype.handle = self.drawing.doc.allocate_handle();
        linetype.description = description.into();
        linetype.elements = pattern
            .iter()
            .map(|length| LineTypeElement {
                length: length * scale,
                complex: None,
            })
            .collect();
        linetype.pattern_length = pattern.iter().map(|length| length.abs() * scale).sum();
        self.drawing.doc.line_types.add(linetype).map_err(invalid)?;
        Ok(true)
    }

    /// `Command::CenterMark`: two centre lines through each circle or arc centre. `extension`
    /// is the distance past the radius, defaulting to 2.5 mm on paper.
    pub(crate) fn center_mark(
        &mut self,
        targets: &[u64],
        extension: Option<f64>,
    ) -> Result<Vec<u64>, CoreError> {
        let extension = extension.unwrap_or_else(|| self.default_text_height());
        if !extension.is_finite() || extension < 0.0 {
            return Err(invalid(
                "The centre mark extension must be a non-negative distance",
            ));
        }
        let handles =
            self.sheet_targets_any(&targets.iter().map(|&h| target(h)).collect::<Vec<_>>())?;
        let mut entities = Vec::with_capacity(handles.len() * 2);
        for handle in handles {
            let entity = self.drawing.doc.get_entity(handle).expect("checked target");
            let (center, radius, normal) = match entity {
                EntityType::Circle(circle) => (circle.center, circle.radius, circle.normal),
                EntityType::Arc(arc) => (arc.center, arc.radius, arc.normal),
                _ => {
                    return Err(invalid(format!(
                        "Entity {} is not a circle or arc",
                        handle.value()
                    )));
                }
            };
            if !radius.is_finite()
                || radius <= 0.0
                || normal.x.abs() > 1e-10
                || normal.y.abs() > 1e-10
            {
                return Err(invalid(
                    "A centre mark needs a circle or arc in the drawing plane",
                ));
            }
            let center = qiyas_geom::object_to_world(&normal, center.z).apply([center.x, center.y]);
            let reach = radius + extension;
            for direction in [[1.0, 0.0], [0.0, 1.0]] {
                entities.extend(self.draw_entities(DrawSpec::Line {
                    a: along(center, direction, -reach),
                    b: along(center, direction, reach),
                })?);
            }
        }
        self.add_convention_lines("CENTER", entities)
    }

    /// `Command::CenterLine`: the common span of parallel faces or the acute angle bisector.
    /// Line directions are canonical, so reversing a source line does not change the result.
    pub(crate) fn center_line(&mut self, a: u64, b: u64) -> Result<Vec<u64>, CoreError> {
        if a == b {
            return Err(invalid("Pick two different lines for a centre line"));
        }
        self.sheet_targets_any(&[target(a), target(b)])?;
        let endpoints = |handle| match self.drawing.doc.get_entity(Handle::new(handle)) {
            Some(EntityType::Line(line)) => {
                Ok(([line.start.x, line.start.y], [line.end.x, line.end.y]))
            }
            _ => Err(invalid(format!("Entity {handle} is not a line"))),
        };
        let (a0, a1) = endpoints(a)?;
        let (b0, b1) = endpoints(b)?;
        if ![a0, a1, b0, b1].into_iter().all(finite) {
            return Err(invalid("A centre line needs finite endpoints"));
        }
        let canonical = |direction| -> Result<[f64; 2], CoreError> {
            let u = unit(direction)?;
            Ok(if u[0] < 0.0 || (u[0] == 0.0 && u[1] < 0.0) {
                [-u[0], -u[1]]
            } else {
                u
            })
        };
        let u = canonical(sub(a1, a0))?;
        let mut v = canonical(sub(b1, b0))?;
        let extension = self.default_text_height();
        let determinant = cross(u, v);
        let (start, end) = if determinant.abs() < 1e-10 {
            let projections = [
                0.0,
                dot(sub(a1, a0), u),
                dot(sub(b0, a0), u),
                dot(sub(b1, a0), u),
            ];
            let low = projections[0]
                .min(projections[1])
                .max(projections[2].min(projections[3]));
            let high = projections[0]
                .max(projections[1])
                .min(projections[2].max(projections[3]));
            if high <= low {
                return Err(invalid(
                    "Parallel faces need a shared length for a centre line",
                ));
            }
            let normal = [-u[1], u[0]];
            let base = along(a0, normal, dot(sub(b0, a0), normal) * 0.5);
            (
                along(base, u, low - extension),
                along(base, u, high + extension),
            )
        } else {
            let origin = along(a0, u, cross(sub(b0, a0), v) / determinant);
            if dot(u, v) < 0.0 {
                v = [-v[0], -v[1]];
            }
            let bisector = unit([u[0] + v[0], u[1] + v[1]])?;
            let projected = [a0, a1, b0, b1].map(|p| dot(sub(p, origin), bisector));
            let low = projected.into_iter().fold(0.0, f64::min);
            let high = projected.into_iter().fold(0.0, f64::max);
            (
                along(origin, bisector, low - extension),
                along(origin, bisector, high + extension),
            )
        };
        let entities = self.draw_entities(DrawSpec::Line { a: start, b: end })?;
        self.add_convention_lines("CENTER", entities)
    }

    /// `Command::SymmetryAxis`: an explicit axis between two points, without changing its span.
    pub(crate) fn symmetry_axis(
        &mut self,
        p1: [f64; 2],
        p2: [f64; 2],
    ) -> Result<Vec<u64>, CoreError> {
        let entities = self.draw_entities(DrawSpec::Line { a: p1, b: p2 })?;
        self.add_convention_lines("CENTER", entities)
    }

    fn convention_layer(&mut self, name: &str) -> Result<String, CoreError> {
        if let Some(layer) = self.drawing.doc.layers.get(name) {
            if layer.flags.locked {
                return Err(invalid(format!(
                    "Layer {} is locked: unlock it first",
                    layer.name
                )));
            }
            return Ok(layer.name.clone());
        }
        self.new_layer(name)?;
        Ok(self
            .drawing
            .doc
            .layers
            .get(name)
            .expect("new layer")
            .name
            .clone())
    }

    fn add_convention_lines(
        &mut self,
        layer: &str,
        entities: Vec<EntityType>,
    ) -> Result<Vec<u64>, CoreError> {
        let layer = self.convention_layer(layer)?;
        self.ensure_convention_linetype("CENTER")?;
        let current = std::mem::replace(
            &mut self.current,
            Current {
                layer,
                linetype: self
                    .drawing
                    .doc
                    .line_types
                    .get("CENTER")
                    .expect("centre linetype")
                    .name
                    .clone(),
                ..Current::default()
            },
        );
        let added = self.add_styled(entities);
        self.current = current;
        added
    }

    /// Automatic grid bounds include the sheet and all axes, with a paper-sized lone span.
    pub(crate) fn grid_extent_default(&self, x: &[GridAxis], y: &[GridAxis]) -> GridExtent {
        let paper = self.default_text_height() / 2.5;
        let [mut min, mut max] = self.sheet_bounds();
        for (i, axes) in [x, y].into_iter().enumerate() {
            for axis in axes {
                min[i] = min[i].min(axis.at);
                max[i] = max[i].max(axis.at);
            }
            if max[i] <= min[i] {
                min[i] -= 10.0 * paper;
                max[i] += 10.0 * paper;
            }
        }
        GridExtent { min, max }
    }

    /// `Command::Grid`: axes extend to the given bounds, with a labelled bubble at each end.
    /// Omitted bounds enclose the current sheet and axis coordinates. A lone axis gets a span
    /// of 20 mm on paper. `bubble` is a radius, defaulting to 3.5 mm on paper.
    pub(crate) fn reference_grid(
        &mut self,
        x: &[GridAxis],
        y: &[GridAxis],
        extent: Option<GridExtent>,
        bubble: Option<f64>,
        layer: Option<&str>,
    ) -> Result<Vec<u64>, CoreError> {
        if x.is_empty() && y.is_empty() {
            return Err(invalid("A grid needs at least one labelled axis"));
        }
        for axis in x.iter().chain(y) {
            if !axis.at.is_finite()
                || axis.label.trim().is_empty()
                || axis.label.chars().any(char::is_control)
            {
                return Err(invalid(
                    "Each grid axis needs a finite coordinate and a non-empty single-line label",
                ));
            }
        }
        let paper = self.default_text_height() / 2.5;
        let radius = bubble.unwrap_or(3.5 * paper);
        if !radius.is_finite() || radius <= 0.0 {
            return Err(invalid(
                "The grid bubble radius must be a positive distance",
            ));
        }
        let extent = extent.unwrap_or_else(|| self.grid_extent_default(x, y));
        if !finite(extent.min)
            || !finite(extent.max)
            || extent.min[0] >= extent.max[0]
            || extent.min[1] >= extent.max[1]
        {
            return Err(invalid(
                "Grid bounds need finite minimum and maximum corners with positive width and height",
            ));
        }
        let mut lines = Vec::with_capacity(x.len() + y.len());
        let mut bubbles = Vec::with_capacity(2 * (x.len() + y.len()));
        for (dimension, axes) in [x, y].into_iter().enumerate() {
            for axis in axes {
                if axis.at < extent.min[dimension] || axis.at > extent.max[dimension] {
                    return Err(invalid(format!(
                        "Grid axis {} lies outside the given bounds",
                        axis.label
                    )));
                }
                let (a, b) = if dimension == 0 {
                    ([axis.at, extent.min[1]], [axis.at, extent.max[1]])
                } else {
                    ([extent.min[0], axis.at], [extent.max[0], axis.at])
                };
                let direction = unit(sub(b, a))?;
                // Leave one bubble radius outside the bounds before each bubble. Adjacent
                // perpendicular axes then have separate bubbles at a boundary corner.
                let (a, b) = (along(a, direction, -radius), along(b, direction, radius));
                lines.extend(self.draw_entities(DrawSpec::Line { a, b })?);
                for center in [along(a, direction, -radius), along(b, direction, radius)] {
                    let circle = self.draw_entities(DrawSpec::Circle {
                        centre: center,
                        radius,
                    })?;
                    bubbles.push((circle, center, axis.label.clone()));
                }
            }
        }
        let layer = layer.unwrap_or("GRID");
        let mut handles = self.add_convention_lines(layer, lines)?;
        let layer = self
            .drawing
            .doc
            .layers
            .get(layer)
            .expect("grid layer")
            .name
            .clone();
        let current = std::mem::replace(
            &mut self.current,
            Current {
                layer,
                linetype: "Continuous".into(),
                ..Current::default()
            },
        );
        let added = (|| {
            if !self.drawing.doc.app_ids.contains(GRID_APP) {
                let mut app = opencadcodec::AppId::new(GRID_APP);
                app.handle = self.drawing.doc.allocate_handle();
                self.drawing.doc.app_ids.add(app).map_err(invalid)?;
            }
            for (circle, center, label) in bubbles {
                let circles = self.add_styled(circle)?;
                for handle in &circles {
                    if let Some(entity) = self.drawing.doc.get_entity_mut(Handle::new(*handle)) {
                        let mut record = opencadcodec::xdata::ExtendedDataRecord::new(GRID_APP);
                        record.add_value(opencadcodec::xdata::XDataValue::String("bubble".into()));
                        entity.common_mut().extended_data.upsert_record(record);
                    }
                }
                handles.extend(circles);
                let text_height =
                    (2.5 * paper).min(2.0 * radius / (label.chars().count() as f64 + 1.0));
                let handle = self.add_text(center, text_height, 0.0, label, None)?;
                if let Some(EntityType::Text(text)) =
                    self.drawing.doc.get_entity_mut(Handle::new(handle))
                {
                    text.horizontal_alignment = TextHorizontalAlignment::Center;
                    text.vertical_alignment = TextVerticalAlignment::Middle;
                    text.alignment_point = Some(text.insertion_point);
                }
                handles.push(handle);
            }
            // Every grid part carries provenance, including axes and text. Layer names and
            // later formatting changes do not determine whether geometry bounds a room.
            for handle in &handles {
                if let Some(entity) = self.drawing.doc.get_entity_mut(Handle::new(*handle)) {
                    let mut record = opencadcodec::xdata::ExtendedDataRecord::new(GRID_APP);
                    record.add_value(opencadcodec::xdata::XDataValue::String("grid".into()));
                    entity.common_mut().extended_data.upsert_record(record);
                    entity.common_mut().raw_record = None;
                }
            }
            Ok(handles)
        })();
        self.current = current;
        added
    }
}

impl Session {
    /// Earlier grids stamped only their bubbles. Recover connected axes and centred letters
    /// from that provenance, without guessing from a layer name or from ordinary circles.
    pub(crate) fn migrate_grid_provenance(&mut self) {
        let Some(app) = self.drawing.doc.app_ids.get(GRID_APP) else {
            return;
        };
        let app_handle = app.handle.value();
        let tolerance = self.measure_tol();
        let circles: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let EntityType::Circle(c) = entity else {
                    return None;
                };
                crate::draw::room_boundary::is_grid(entity).then_some((
                    c.common.owner_handle,
                    [c.center.x, c.center.y],
                    c.radius,
                ))
            })
            .collect();
        if circles.is_empty() {
            return;
        }
        let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
        let handles: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter(|entity| {
                if crate::draw::room_boundary::is_grid(entity) {
                    return false;
                }
                let owner = entity.common().owner_handle;
                match entity {
                    EntityType::Line(line) => {
                        let a = [line.start.x, line.start.y];
                        let b = [line.end.x, line.end.y];
                        let Ok(direction) = unit(sub(b, a)) else {
                            return false;
                        };
                        let at_start = circles.iter().any(|(sheet, c, r)| {
                            *sheet == owner && close(*c, along(a, direction, -*r))
                        });
                        let at_end = circles.iter().any(|(sheet, c, r)| {
                            *sheet == owner && close(*c, along(b, direction, *r))
                        });
                        at_start && at_end
                    }
                    EntityType::Text(text) => circles.iter().any(|(sheet, c, _)| {
                        *sheet == owner
                            && close(*c, [text.insertion_point.x, text.insertion_point.y])
                    }),
                    _ => false,
                }
            })
            .map(|e| e.common().handle)
            .collect();
        for handle in handles {
            if let Some(entity) = self.drawing.doc.get_entity_mut(handle) {
                crate::xdata::write_text(entity.common_mut(), GRID_APP, app_handle, Some("grid"));
            }
        }
    }
}
