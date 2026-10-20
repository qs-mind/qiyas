//! Joined wall outlines, measurable hatches and reusable door and window blocks.

use std::collections::BTreeMap;

use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle};
use qiyas_geom::walls::{self, Justification};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CoreError, Current, DrawSpec, HatchPattern, NewEntity, Session};

/// The application carrying each wall centreline's thickness (drawing units) and height (m).
pub const WALL_APP: &str = "QIYAS_WALL";

/// A centreline in drawing units. Approximate curves with straight polyline segments.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WallRun {
    pub points: crate::PointRun<true>,
    #[serde(default)]
    pub closed: bool,
}

/// The side of the directed run occupied by the wall.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WallJustification {
    #[default]
    Centre,
    Left,
    Right,
}

/// The opening's plan symbol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
}

/// The door's hinge at the left or right end, looking along the run.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DoorSwing {
    #[default]
    Left,
    Right,
}

/// A cut centred `at` along a run, with `width` in drawing units. Vertical sizes are metres.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WallOpening {
    pub run: usize,
    pub at: f64,
    pub width: f64,
    pub kind: OpeningKind,
    #[serde(default)]
    pub swing: DoorSwing,
    /// Conventional sill in metres: windows 0.9 m; doors at floor level (0 m).
    #[serde(default)]
    pub sill: Option<f64>,
    /// Conventional head in metres: 2.1 m or the wall height, whichever is lower.
    #[serde(default)]
    pub head: Option<f64>,
}

/// Optional layer names; missing names use the architectural wall, door and glazing layers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WallLayers {
    /// Conventional outline layer: A-WALL.
    pub outline: Option<String>,
    /// Conventional hatch layer: A-WALL-PATT.
    pub hatch: Option<String>,
    /// Conventional centreline layer: A-WALL-CNTR.
    pub centreline: Option<String>,
    /// Conventional door-symbol layer: A-DOOR.
    pub doors: Option<String>,
    /// Conventional window-symbol layer: A-GLAZ.
    pub windows: Option<String>,
}

/// Walls from directed polylines. Plan coordinates, thickness and openings use drawing units;
/// height, sill and head use metres. Openings must fit a straight part clear of junctions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WallSpec {
    pub runs: Vec<WallRun>,
    pub thickness: f64,
    #[serde(default)]
    pub justification: WallJustification,
    #[serde(default)]
    pub openings: Vec<WallOpening>,
    /// Conventional wall height: 3 m when omitted.
    #[serde(default = "default_wall_height")]
    pub height: f64,
    #[serde(default)]
    pub layers: WallLayers,
}

fn default_wall_height() -> f64 {
    3.0
}

/// The created sheet entities and physical quantities, in metres, square metres and cubic metres.
#[derive(Debug, Clone, Serialize)]
pub struct WallResult {
    pub handles: Vec<u64>,
    pub outlines: Vec<u64>,
    pub hatches: Vec<u64>,
    pub centrelines: Vec<u64>,
    pub inserts: Vec<u64>,
    pub centreline_length: f64,
    pub gross_area: f64,
    pub net_area: f64,
    /// Gross area times height less the opening voids; sill walls and lintels remain.
    pub volume: f64,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

/// Millimetres in block names: 900 becomes 0900, a fractional size keeps its decimal part.
fn size_name(mm: f64) -> String {
    let whole = mm.round();
    if (whole - mm).abs() < 1e-6 {
        format!("{whole:04.0}")
    } else {
        format!("{mm:.6}").trim_end_matches('0').replace('.', "_")
    }
}

impl Session {
    /// `Command::Walls`: one recorded change including the layers, blocks and xdata application.
    pub(crate) fn walls(&mut self, mut spec: WallSpec) -> Result<WallResult, CoreError> {
        spec.normalize_runs(self.drawing.units);
        if !spec.height.is_finite() || spec.height <= 0.0 {
            return Err(invalid(
                "The wall height must be a positive number in metres",
            ));
        }
        let vertical: Vec<_> = spec
            .openings
            .iter()
            .map(|opening| {
                let (sill, head) = match opening.kind {
                    OpeningKind::Door => (
                        opening.sill.unwrap_or(0.0),
                        opening.head.unwrap_or(spec.height.min(2.1)),
                    ),
                    OpeningKind::Window => (
                        opening.sill.unwrap_or(0.9),
                        opening.head.unwrap_or(spec.height.min(2.1)),
                    ),
                };
                if !sill.is_finite()
                    || !head.is_finite()
                    || sill < 0.0
                    || head <= sill
                    || head > spec.height
                {
                    return Err(invalid(
                        "An opening needs 0 <= sill < head <= wall height, in metres",
                    ));
                }
                Ok(head - sill)
            })
            .collect::<Result<_, CoreError>>()?;
        let runs: Vec<_> = spec
            .runs
            .iter()
            .map(|r| walls::Run {
                points: r.points.to_vec(),
                closed: r.closed,
            })
            .collect();
        let openings: Vec<_> = spec
            .openings
            .iter()
            .map(|o| walls::Opening {
                run: o.run,
                at: o.at,
                width: o.width,
            })
            .collect();
        let justification = match spec.justification {
            WallJustification::Centre => Justification::Centre,
            WallJustification::Left => Justification::Left,
            WallJustification::Right => Justification::Right,
        };
        let built = walls::build_walls_justified(
            &runs,
            spec.thickness,
            &openings,
            self.measure_tol(),
            justification,
        )
        .map_err(|e| invalid(e.to_string()))?;
        let factor = self.drawing.units.to_metres();
        let gross_area = built.gross_area * factor * factor;
        let net_area = built.net_area * factor * factor;
        let voids: f64 = spec
            .openings
            .iter()
            .zip(vertical)
            .map(|(o, height)| o.width * spec.thickness * factor * factor * height)
            .sum();
        let mut result = WallResult {
            handles: Vec::new(),
            outlines: Vec::new(),
            hatches: Vec::new(),
            centrelines: Vec::new(),
            inserts: Vec::new(),
            centreline_length: built.centreline * factor,
            gross_area,
            net_area,
            volume: gross_area * spec.height - voids,
        };
        let layers = [
            spec.layers.outline.as_deref().unwrap_or("A-WALL"),
            spec.layers.hatch.as_deref().unwrap_or("A-WALL-PATT"),
            spec.layers.centreline.as_deref().unwrap_or("A-WALL-CNTR"),
            spec.layers.doors.as_deref().unwrap_or("A-DOOR"),
            spec.layers.windows.as_deref().unwrap_or("A-GLAZ"),
        ]
        .map(str::trim);
        for layer in layers {
            if layer.contains('|') {
                return Err(invalid("Wall entities cannot use an xref's layers"));
            }
            if !self.drawing.doc.layers.contains(layer) {
                self.new_layer(layer)?;
            }
        }
        let layers = layers.map(|name| {
            self.drawing
                .doc
                .layers
                .get(name)
                .expect("wall layer")
                .name
                .clone()
        });
        if !self.drawing.doc.app_ids.contains(WALL_APP) {
            let mut app = AppId::new(WALL_APP);
            app.handle = self.drawing.doc.allocate_handle();
            self.drawing.doc.app_ids.add(app).map_err(invalid)?;
        }
        let current = std::mem::take(&mut self.current);
        let drawn = (|| {
            self.current = Current {
                layer: layers[0].to_string(),
                ..Current::default()
            };
            for region in &built.regions {
                for ring in std::iter::once(&region.outer).chain(&region.holes) {
                    result.outlines.extend(self.draw(DrawSpec::Polyline {
                        points: ring.clone().into(),
                        closed: true,
                        bulges: Vec::new(),
                    })?);
                }
            }
            self.current.layer = layers[1].to_string();
            for region in &built.regions {
                result.hatches.extend(self.draw(DrawSpec::Hatch {
                    points: region.outer.clone().into(),
                    holes: region.holes.iter().cloned().map(Into::into).collect(),
                    boundary_handles: Vec::new(),
                    bulges: Vec::new(),
                    pattern: HatchPattern::Solid,
                })?);
            }
            self.current.layer = layers[2].to_string();
            for run in &spec.runs {
                let mut entities = self.draw_entities(DrawSpec::Polyline {
                    points: run.points.to_vec().into(),
                    closed: run.closed,
                    bulges: Vec::new(),
                })?;
                let mut record = ExtendedDataRecord::new(WALL_APP);
                record.add_value(XDataValue::Real(spec.thickness));
                record.add_value(XDataValue::Real(spec.height));
                entities[0].common_mut().extended_data.upsert_record(record);
                result.centrelines.extend(self.add_styled(entities)?);
            }
            for (opening, placed) in spec.openings.iter().zip(&built.openings) {
                self.current.layer = match opening.kind {
                    OpeningKind::Door => &layers[3],
                    OpeningKind::Window => &layers[4],
                }
                .to_string();
                result
                    .inserts
                    .push(self.wall_opening_block(opening, placed, factor)?);
            }
            Ok::<(), CoreError>(())
        })();
        self.current = current;
        drawn?;
        result.handles.extend(&result.outlines);
        result.handles.extend(&result.hatches);
        result.handles.extend(&result.centrelines);
        result.handles.extend(&result.inserts);
        Ok(result)
    }

    fn wall_opening_block(
        &mut self,
        opening: &WallOpening,
        placed: &walls::Placed,
        factor: f64,
    ) -> Result<u64, CoreError> {
        let mm = 0.001 / factor;
        let width_mm = placed.width / mm;
        let thickness_mm = placed.thickness / mm;
        let name = match opening.kind {
            OpeningKind::Door => format!("QIYAS_DOOR_{}", size_name(width_mm)),
            OpeningKind::Window => format!(
                "QIYAS_WIN_{}_{}",
                size_name(width_mm),
                size_name(thickness_mm).trim_start_matches('0')
            ),
        };
        if !self.drawing.doc.block_records.contains(&name) {
            // Build the library in millimetres, then convert it to this drawing's units.
            let w = width_mm * mm;
            let t = thickness_mm * mm;
            let specs = match opening.kind {
                OpeningKind::Door => vec![
                    DrawSpec::Line {
                        a: [0.0, 0.0],
                        b: [0.0, w],
                    },
                    DrawSpec::Arc {
                        centre: [0.0, 0.0],
                        radius: w,
                        start: 0.0,
                        end: 90.0,
                    },
                ],
                OpeningKind::Window => vec![
                    DrawSpec::Rectangle {
                        a: [-w / 2.0, -t / 2.0],
                        b: [w / 2.0, t / 2.0],
                    },
                    DrawSpec::Line {
                        a: [-w / 2.0, 0.0],
                        b: [w / 2.0, 0.0],
                    },
                ],
            };
            let mut entities = Vec::new();
            for spec in specs {
                for entity in self.draw_entities(spec)? {
                    let mut data = crate::records::entity_data(&entity);
                    // Layer 0 inherits the reference's door or glazing layer.
                    data["common"] = serde_json::json!({"layer": "0"});
                    entities.push(NewEntity {
                        kind: crate::session::variant_name(&entity).to_string(),
                        data,
                    });
                }
            }
            self.define_block_entities(&name, [0.0, 0.0], entities)?;
        }
        let [dx, dy] = placed.dir;
        let mirror =
            matches!(opening.kind, OpeningKind::Door) && matches!(opening.swing, DoorSwing::Right);
        let at = match opening.kind {
            OpeningKind::Window => placed.centre,
            OpeningKind::Door => {
                let along = if mirror {
                    placed.width / 2.0
                } else {
                    -placed.width / 2.0
                };
                [
                    placed.centre[0] + dx * along + dy * placed.thickness / 2.0,
                    placed.centre[1] + dy * along - dx * placed.thickness / 2.0,
                ]
            }
        };
        let handle = self.insert_block(
            &name,
            at,
            1.0,
            dy.atan2(dx).to_degrees(),
            &BTreeMap::new(),
            None,
        )?;
        if mirror {
            if let Some(EntityType::Insert(insert)) =
                self.drawing.doc.get_entity_mut(Handle::new(handle))
            {
                insert.set_x_scale(-insert.x_scale());
            }
        }
        Ok(handle)
    }
}
