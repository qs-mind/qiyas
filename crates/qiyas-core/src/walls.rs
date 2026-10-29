//! Joined wall outlines, measurable hatches and reusable door and window blocks.

use std::collections::{BTreeMap, BTreeSet};

use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle};
use qiyas_geom::walls::{self, Justification};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CoreError, Current, DrawSpec, Session};

/// The application carrying each wall centreline's thickness (drawing units) and height (m).
pub const WALL_APP: &str = "QIYAS_WALL";

/// A centreline in drawing units. Approximate curves with straight polyline segments.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WallRun {
    pub points: crate::PointRun<true>,
    #[serde(default)]
    pub closed: bool,
    /// Overrides the system thickness, in drawing units.
    #[serde(default)]
    pub thickness: Option<f64>,
    /// Overrides the system height, in metres.
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub justification: Option<WallJustification>,
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
#[derive(Debug, Clone, Serialize, JsonSchema)]
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
    /// Join touching systems on the same layers. False keeps this system separate.
    #[serde(default = "default_join")]
    pub join: bool,
    /// Transient input provenance; persisted on object properties after creation.
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub(crate) defaulted: Vec<String>,
}

#[derive(Deserialize)]
struct WallSpecInput {
    runs: Vec<WallRun>,
    thickness: f64,
    #[serde(default)]
    justification: WallJustification,
    #[serde(default)]
    openings: Vec<WallOpening>,
    #[serde(default)]
    height: Option<f64>,
    #[serde(default)]
    layers: WallLayers,
    #[serde(default = "default_join")]
    join: bool,
    #[serde(default)]
    defaulted: Vec<String>,
}
impl<'de> Deserialize<'de> for WallSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        WallSpecInput::deserialize(deserializer).map(Self::from)
    }
}
impl From<WallSpecInput> for WallSpec {
    fn from(input: WallSpecInput) -> Self {
        let mut defaulted = input.defaulted;
        if input.height.is_none() {
            defaulted.push("height".into());
        }
        Self {
            runs: input.runs,
            thickness: input.thickness,
            justification: input.justification,
            openings: input.openings,
            height: input.height.unwrap_or_else(default_wall_height),
            layers: input.layers,
            join: input.join,
            defaulted,
        }
    }
}

fn default_join() -> bool {
    true
}

fn default_wall_height() -> f64 {
    legacy_default("wall", "wall_height")
        .as_f64()
        .expect("numeric legacy default")
}

fn legacy_default(kind: &str, property: &str) -> serde_json::Value {
    let definitions: serde_json::Value =
        serde_json::from_str(include_str!("../data/legacy-object-types.json"))
            .expect("validated bundled legacy definitions");
    definitions[kind]["properties"][property]["default"].clone()
}

/// The created sheet entities and physical quantities, in metres, square metres and cubic metres.
#[derive(Debug, Clone, Serialize)]
pub struct WallResult {
    pub group: u64,
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
    /// Every replaced handle and its replacements; an empty list means removed.
    pub handle_map: BTreeMap<u64, Vec<u64>>,
    /// Physical drawing defaults used by the returned elements, in metres.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub defaults_used: Vec<crate::validation::DefaultUsed>,
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
    fn draw_wall_system(
        &mut self,
        mut spec: WallSpec,
        graphics: bool,
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        spec.normalize_runs(self.drawing.units);
        explicit_runs(&mut spec);
        if spec.runs.iter().any(|run| {
            !run.height.unwrap_or(spec.height).is_finite()
                || run.height.unwrap_or(spec.height) <= 0.0
        }) {
            return Err(invalid(
                "The wall height must be a positive number in metres",
            ));
        }
        let factor = self.drawing.units.to_metres();
        let members: Vec<_> = spec
            .runs
            .iter()
            .enumerate()
            .map(|(index, run)| crate::object_bands::BandMember {
                id: index as u64,
                points: run.points.to_vec(),
                closed: run.closed,
                width_m: run.thickness.unwrap_or(spec.thickness) * factor,
                base_m: 0.0,
                height_m: run.height.unwrap_or(spec.height),
                justification: geom_justification(run.justification.unwrap_or(spec.justification)),
            })
            .collect();
        let cuts = spec
            .openings
            .iter()
            .map(|opening| {
                let host = members
                    .get(opening.run)
                    .ok_or_else(|| invalid("The opening run does not exist"))?;
                let sill = opening.sill.expect("resolved sill");
                let head = opening.head.expect("resolved head");
                if !sill.is_finite()
                    || !head.is_finite()
                    || sill < 0.0
                    || head <= sill
                    || head > host.height_m
                {
                    return Err(invalid(
                        "An opening needs 0 <= sill < head <= wall height, in metres",
                    ));
                }
                Ok(crate::object_bands::BandCut {
                    host: host.id,
                    at_m: opening.at * factor,
                    width_m: opening.width * factor,
                    base_m: sill,
                    height_m: head - sill,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        let derived =
            crate::object_bands::derive_band(&members, &cuts, factor, self.measure_tol())?;
        let layers = resolved_layers(&spec.layers);
        for layer in &layers {
            if layer.contains('|') {
                return Err(invalid("Wall entities cannot use an xref's layers"));
            }
            if !self.drawing.doc.layers.contains(layer) {
                self.new_layer(layer)?;
            }
        }
        self.ensure_legacy_type("wall")?;
        if !self.drawing.doc.app_ids.contains(WALL_APP) {
            let mut app = AppId::new(WALL_APP);
            app.handle = self.drawing.doc.allocate_handle();
            self.drawing.doc.app_ids.add(app).map_err(invalid)?;
        }
        let resolved = serde_json::json!(&spec);
        let defaults_used = spec
            .defaulted
            .iter()
            .filter_map(|field| {
                let pointer = format!("/{field}");
                Some(crate::validation::DefaultUsed {
                    field: pointer.clone(),
                    value: resolved.pointer(&pointer)?.clone(),
                    unit: Some("m".into()),
                })
            })
            .collect();
        let mut result = WallResult {
            defaults_used,
            group: 0,
            handles: Vec::new(),
            outlines: Vec::new(),
            hatches: Vec::new(),
            centrelines: Vec::new(),
            inserts: Vec::new(),
            centreline_length: derived.geometry.centreline * factor,
            gross_area: derived.geometry.gross_area * factor * factor,
            net_area: derived.geometry.net_area * factor * factor,
            volume: derived.net_volume,
            handle_map: BTreeMap::new(),
        };
        let current = std::mem::take(&mut self.current);
        let drawn = (|| {
            self.current = Current {
                layer: layers[2].clone(),
                ..Current::default()
            };
            for run in &spec.runs {
                result.centrelines.extend(self.draw(DrawSpec::Polyline {
                    points: run.points.to_vec().into(),
                    closed: run.closed,
                    bulges: Vec::new(),
                })?);
            }
            for (opening, placed) in spec.openings.iter().zip(&derived.geometry.openings) {
                self.current.layer = match opening.kind {
                    OpeningKind::Door => layers[3].clone(),
                    OpeningKind::Window => layers[4].clone(),
                };
                result
                    .inserts
                    .push(self.wall_opening_block(opening, placed, factor)?);
            }
            Ok::<(), CoreError>(())
        })();
        self.current = current;
        drawn?;
        result.group = result.centrelines[0];
        for (role, handles) in [("run", &result.centrelines), ("opening", &result.inserts)] {
            for (index, &handle) in handles.iter().enumerate() {
                self.stamp_wall(
                    handle,
                    &WallTag {
                        group: result.group,
                        role: role.into(),
                        index,
                        spec: spec.clone(),
                    },
                )?;
            }
        }
        self.migrate_legacy_objects()?;
        // Migration imports resolved values. Restore whether each physical input was given.
        for (role, handles, fields) in [
            (
                "runs",
                &result.centrelines,
                &[("height", "wall_height")][..],
            ),
            (
                "openings",
                &result.inserts,
                &[("sill", "sill"), ("head", "head")][..],
            ),
        ] {
            for (i, handle) in handles.iter().enumerate() {
                if let Some(mut object) = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(*handle))
                    .and_then(crate::objects::instance)
                {
                    let kind = if role == "runs" {
                        serde_json::json!("wall")
                    } else {
                        serde_json::json!(spec.openings[i].kind)
                    };
                    object.props.insert("kind".into(), kind);
                    for property in object.props.keys() {
                        object.sources.entry(property.clone()).or_insert(
                            crate::measurement::PropSource {
                                decision: None,
                                kind: crate::objects::given_by(source).kind,
                                handle: None,
                                text: "Explicit command input / قيمة صريحة في الأمر".into(),
                            },
                        );
                    }
                    for (field, property) in fields {
                        let defaulted = spec.defaulted.contains(&format!("{role}/{i}/{field}"));
                        object.sources.insert(
                            (*property).into(),
                            crate::measurement::PropSource {
                                decision: None,
                                kind: if defaulted {
                                    crate::measurement::SourceKind::TypeDefault
                                } else {
                                    crate::objects::given_by(source).kind
                                },
                                text: if defaulted {
                                    "Drawing default / قيمة رسم افتراضية"
                                } else {
                                    "Explicit command input / قيمة صريحة في الأمر"
                                }
                                .into(),
                                handle: None,
                            },
                        );
                    }
                    self.stamp_object(*handle, &object)?;
                }
            }
        }
        result.handles.extend(&result.centrelines);
        result.handles.extend(&result.inserts);
        if !graphics {
            return Ok(result);
        }
        self.refresh_objects()?;
        let mut parts: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let part: crate::objects::ObjectPart =
                    crate::xdata::read_json(entity.common(), crate::objects::PART_APP)?;
                result
                    .centrelines
                    .contains(&part.owner)
                    .then_some((part, entity.common().handle.value()))
            })
            .collect();
        parts.sort_by_key(|(part, handle)| (part.index, *handle));
        for (part, handle) in parts {
            let role_index = match part.role.as_str() {
                "outline" => {
                    result.outlines.push(handle);
                    result.outlines.len() - 1
                }
                "hatch" => {
                    result.hatches.push(handle);
                    result.hatches.len() - 1
                }
                _ => part.index,
            };
            self.stamp_wall(
                handle,
                &WallTag {
                    group: result.group,
                    role: part.role,
                    index: role_index,
                    spec: spec.clone(),
                },
            )?;
            result.handles.push(handle);
        }
        let totals = self.wall_totals(&result.centrelines)?;
        result.gross_area = totals[1];
        result.net_area = totals[2];
        result.volume = totals[3];
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
        let kind = match opening.kind {
            OpeningKind::Door => "door",
            OpeningKind::Window => "window",
        };
        let definition = self.ensure_legacy_type(kind)?;
        let mut props = BTreeMap::from([
            ("width".into(), serde_json::json!(placed.width * factor)),
            (
                "host_width".into(),
                serde_json::json!(placed.thickness * factor),
            ),
            ("distance".into(), serde_json::json!(opening.at * factor)),
            (
                "sill".into(),
                serde_json::json!(opening.sill.unwrap_or(0.0)),
            ),
            (
                "head".into(),
                serde_json::json!(opening.head.unwrap_or(0.0)),
            ),
            ("swing".into(), serde_json::json!(opening.swing)),
        ]);
        props.retain(|name, _| definition.properties.contains_key(name));
        let specs = self.symbol_recipe_for_properties(&definition, &props)?;
        let block = self.legacy_symbol_block(&name, specs.clone())?;
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
            &block,
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
        self.stamp_legacy_symbol_recipe(handle, &name, &specs)?;
        Ok(handle)
    }
}

/// An opening added to a picked centreline. Distances use drawing units; vertical sizes metres.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddedWallOpening {
    pub at: f64,
    pub width: f64,
    pub kind: OpeningKind,
    #[serde(default)]
    pub swing: DoorSwing,
    #[serde(default)]
    pub sill: Option<f64>,
    #[serde(default)]
    pub head: Option<f64>,
}

/// Changes to one opening, picked by its insert. Omitted properties stay as they are.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WallOpeningEdit {
    /// Custom opening ID; empty uses the size mark.
    #[serde(default)]
    pub mark: Option<String>,
    #[serde(default)]
    pub at: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub kind: Option<OpeningKind>,
    #[serde(default)]
    pub swing: Option<DoorSwing>,
    #[serde(default)]
    pub sill: Option<f64>,
    #[serde(default)]
    pub head: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WallGroupInfo {
    pub group: u64,
    pub handles: Vec<u64>,
    pub layers: WallLayers,
    pub runs: Vec<WallRunInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WallRunInfo {
    pub centreline: u64,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    /// Length and thickness use drawing units; height uses metres.
    pub length: f64,
    pub thickness: f64,
    pub height: f64,
    pub justification: WallJustification,
    pub openings: Vec<WallOpeningInfo>,
    /// Accepted centre-distance intervals for the requested width, in drawing units.
    pub free_ranges: Option<Vec<[f64; 2]>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WallOpeningInfo {
    pub insert: u64,
    pub mark: Option<String>,
    #[serde(flatten)]
    pub opening: WallOpening,
}

/// Every member holds the definition: neither a sidecar nor an application session is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WallTag {
    pub(crate) group: u64,
    pub(crate) role: String,
    pub(crate) index: usize,
    pub(crate) spec: WallSpec,
}

#[derive(Clone)]
pub(crate) struct WallGroup {
    pub(crate) id: u64,
    pub(crate) spec: WallSpec,
    pub(crate) members: Vec<(u64, String, usize)>,
}

pub(crate) fn geom_justification(value: WallJustification) -> Justification {
    match value {
        WallJustification::Centre => Justification::Centre,
        WallJustification::Left => Justification::Left,
        WallJustification::Right => Justification::Right,
    }
}

pub(crate) fn geom_runs(spec: &WallSpec) -> Vec<walls::Run> {
    spec.runs
        .iter()
        .map(|r| walls::Run {
            points: r.points.to_vec(),
            closed: r.closed,
            thickness: r.thickness,
            justification: r.justification.map(geom_justification),
        })
        .collect()
}

pub(crate) fn geom_openings(spec: &WallSpec) -> Vec<walls::Opening> {
    spec.openings
        .iter()
        .map(|o| walls::Opening {
            run: o.run,
            at: o.at,
            width: o.width,
        })
        .collect()
}

pub(crate) fn wall_tag(entity: &EntityType) -> Option<WallTag> {
    crate::xdata::read_json(entity.common(), WALL_APP)
}

pub(crate) fn resolved_layers(layers: &WallLayers) -> [String; 5] {
    [
        (&layers.outline, "outline_layer"),
        (&layers.hatch, "hatch_layer"),
        (&layers.centreline, "centreline_layer"),
        (&layers.doors, "doors_layer"),
        (&layers.windows, "windows_layer"),
    ]
    .map(|(supplied, property)| {
        supplied
            .clone()
            .unwrap_or_else(|| {
                legacy_default("wall", property)
                    .as_str()
                    .expect("layer default")
                    .to_owned()
            })
            .trim()
            .to_owned()
    })
}

fn layer_names(layers: &WallLayers) -> [String; 5] {
    resolved_layers(layers).map(|name| name.to_uppercase())
}

pub(crate) fn explicit_runs(spec: &mut WallSpec) {
    for (i, run) in spec.runs.iter_mut().enumerate() {
        if run.height.is_none() && spec.defaulted.iter().any(|p| p == "height") {
            let field = format!("runs/{i}/height");
            if !spec.defaulted.contains(&field) {
                spec.defaulted.push(field);
            }
        }
        run.thickness = Some(run.thickness.unwrap_or(spec.thickness));
        run.height = Some(run.height.unwrap_or(spec.height));
        run.justification = Some(run.justification.unwrap_or(spec.justification));
    }
    for (i, opening) in spec.openings.iter_mut().enumerate() {
        for (field, missing) in [
            ("sill", opening.sill.is_none()),
            ("head", opening.head.is_none()),
        ] {
            let field = format!("openings/{i}/{field}");
            if missing && !spec.defaulted.contains(&field) {
                spec.defaulted.push(field);
            }
        }
        if let Some(run) = spec.runs.get(opening.run) {
            let kind = match opening.kind {
                OpeningKind::Door => "door",
                OpeningKind::Window => "window",
            };
            opening.sill =
                Some(opening.sill.unwrap_or_else(|| {
                    legacy_default(kind, "sill").as_f64().expect("sill default")
                }));
            opening.head = Some(opening.head.unwrap_or_else(|| {
                run.height
                    .unwrap_or(spec.height)
                    .min(legacy_default(kind, "head").as_f64().expect("head default"))
            }));
        }
    }
}

fn run_segments(run: &WallRun) -> Vec<([f64; 2], [f64; 2])> {
    let mut segments: Vec<_> = run.points.windows(2).map(|p| (p[0], p[1])).collect();
    if run.closed && run.points.len() > 1 {
        segments.push((*run.points.last().unwrap(), run.points[0]));
    }
    segments
}

fn systems_touch(a: &WallSpec, b: &WallSpec, tolerance: f64) -> bool {
    fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
        a[0] * b[1] - a[1] * b[0]
    }
    fn minus(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
        [a[0] - b[0], a[1] - b[1]]
    }
    fn near(p: [f64; 2], a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
        let d = minus(b, a);
        let length2 = d[0] * d[0] + d[1] * d[1];
        if length2 == 0.0 {
            return false;
        }
        let q = minus(p, a);
        let t = ((q[0] * d[0] + q[1] * d[1]) / length2).clamp(0.0, 1.0);
        (q[0] - t * d[0]).hypot(q[1] - t * d[1]) <= tolerance
    }
    a.runs.iter().flat_map(run_segments).any(|(p, q)| {
        b.runs.iter().flat_map(run_segments).any(|(r, s)| {
            let size = p
                .iter()
                .chain(&q)
                .chain(&r)
                .chain(&s)
                .fold(0.0_f64, |a, v| a.max(v.abs()));
            let tolerance = qiyas_geom::tol_near(size, tolerance);
            if near(p, r, s, tolerance)
                || near(q, r, s, tolerance)
                || near(r, p, q, tolerance)
                || near(s, p, q, tolerance)
            {
                return true;
            }
            let d = minus(q, p);
            let e = minus(s, r);
            let denominator = cross(d, e);
            if denominator.abs() <= f64::EPSILON * d[0].hypot(d[1]) * e[0].hypot(e[1]) {
                return false;
            }
            let t = cross(minus(r, p), e) / denominator;
            let u = cross(minus(r, p), d) / denominator;
            (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)
        })
    })
}

impl Session {
    /// Keep the historical wall API's metadata as an output projection of ordinary objects.
    /// This does not create geometry; generic regeneration owns every generated part.
    pub(crate) fn sync_legacy_wall_tags(&mut self) -> Result<(), CoreError> {
        if !self.drawing.doc.app_ids.contains(WALL_APP) {
            return Ok(());
        }
        let sheets: Vec<_> = self
            .drawing
            .doc
            .block_records
            .iter()
            .filter(|block| {
                self.drawing
                    .doc
                    .entities_in_block(&block.name)
                    .any(|entity| wall_tag(entity).is_some_and(|tag| tag.role == "run"))
            })
            .map(|block| block.name.clone())
            .collect();
        let original = self.sheet.clone();
        let result = (|| {
            for sheet in sheets {
                self.sheet = sheet;
                for group in self.stored_wall_groups()? {
                    for (handle, role, index) in group.members {
                        self.stamp_wall(
                            handle,
                            &WallTag {
                                group: group.id,
                                role,
                                index,
                                spec: group.spec.clone(),
                            },
                        )?;
                    }
                }
            }
            Ok(())
        })();
        self.sheet = original;
        result
    }

    pub(crate) fn stamp_wall(&mut self, handle: u64, tag: &WallTag) -> Result<(), CoreError> {
        let app_handle = crate::xdata::app_handle(&self.drawing.doc, WALL_APP);
        let entity = self
            .drawing
            .doc
            .get_entity_mut(Handle::new(handle))
            .ok_or_else(|| invalid("A wall member is missing"))?;
        let mut record = ExtendedDataRecord::new(WALL_APP);
        if tag.role == "run" {
            let run = &tag.spec.runs[tag.index];
            record.add_value(XDataValue::Real(
                run.thickness.unwrap_or(tag.spec.thickness),
            ));
            record.add_value(XDataValue::Real(run.height.unwrap_or(tag.spec.height)));
        }
        for chunk in
            crate::field::chunks(&serde_json::to_string(tag).map_err(|e| invalid(e.to_string()))?)
        {
            record.add_value(chunk);
        }
        crate::xdata::write_values(
            entity.common_mut(),
            WALL_APP,
            app_handle,
            Some(record.values),
        );
        Ok(())
    }

    pub(crate) fn stored_wall_groups(&self) -> Result<Vec<WallGroup>, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("walls.stored_wall_groups");
        let mut groups: BTreeMap<u64, WallGroup> = BTreeMap::new();
        for entity in self.drawing.doc.entities_in_block(&self.sheet) {
            if let Some(tag) = wall_tag(entity) {
                groups
                    .entry(tag.group)
                    .or_insert_with(|| WallGroup {
                        id: tag.group,
                        spec: tag.spec.clone(),
                        members: Vec::new(),
                    })
                    .members
                    .push((entity.common().handle.value(), tag.role, tag.index));
            }
        }
        for group in groups.values_mut() {
            let mut present = BTreeSet::new();
            let mut driver_indices = BTreeMap::new();
            for &(handle, ref role, index) in &group.members {
                if role != "run" {
                    continue;
                }
                if !present.insert(index) {
                    return Err(invalid(
                        "A copied wall system has duplicate run identities; copy the complete wall group",
                    ));
                }
                driver_indices.insert(handle, index);
                let Some(run) = group.spec.runs.get_mut(index) else {
                    return Err(invalid("The stored wall run is invalid"));
                };
                if let Some(EntityType::LwPolyline(poly)) =
                    self.drawing.doc.get_entity(Handle::new(handle))
                {
                    run.points = poly
                        .vertices
                        .iter()
                        .map(|v| [v.location.x, v.location.y])
                        .collect();
                    run.closed = poly.is_closed;
                    if let Some(object) =
                        crate::objects::instance(&EntityType::LwPolyline(poly.clone()))
                    {
                        for (role, slot) in [
                            ("outline", &mut group.spec.layers.outline),
                            ("hatch", &mut group.spec.layers.hatch),
                            ("centreline", &mut group.spec.layers.centreline),
                            ("doors", &mut group.spec.layers.doors),
                            ("windows", &mut group.spec.layers.windows),
                        ] {
                            if let Some(layer) = object
                                .props
                                .get(&format!("{role}_layer"))
                                .and_then(serde_json::Value::as_str)
                            {
                                *slot = Some(layer.to_string());
                            }
                        }
                        let factor = self.drawing.units.to_metres();
                        if let Some(width) = object
                            .props
                            .get("width")
                            .and_then(serde_json::Value::as_f64)
                        {
                            run.thickness = Some(width / factor);
                        }
                        if let Some(height) = object
                            .props
                            .get("wall_height")
                            .and_then(serde_json::Value::as_f64)
                        {
                            run.height = Some(height);
                        }
                        if let Some(value) = object.props.get("justification") {
                            run.justification = serde_json::from_value(value.clone()).ok();
                        }
                    }
                }
            }
            if present.len() != group.spec.runs.len() {
                return Err(invalid(
                    "A wall centreline was erased; undo the erase before rebuilding its wall system",
                ));
            }
            for &(handle, ref role, index) in &group.members {
                if role != "opening" {
                    continue;
                }
                let Some(object) = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(handle))
                    .and_then(crate::objects::instance)
                else {
                    continue;
                };
                let Some(opening) = group.spec.openings.get_mut(index) else {
                    continue;
                };
                let factor = self.drawing.units.to_metres();
                if let Some(host) = object.host.and_then(|host| driver_indices.get(&host)) {
                    opening.run = *host;
                }
                if let Some(value) = object
                    .props
                    .get("width")
                    .and_then(serde_json::Value::as_f64)
                {
                    opening.width = value / factor;
                }
                if let Some(value) = object
                    .props
                    .get("distance")
                    .and_then(serde_json::Value::as_f64)
                {
                    opening.at = value / factor;
                }
                if let Some(value) = object.props.get("sill").and_then(serde_json::Value::as_f64) {
                    opening.sill = Some(value);
                }
                if let Some(value) = object.props.get("head").and_then(serde_json::Value::as_f64) {
                    opening.head = Some(value);
                }
                if let Some(value) = object.props.get("swing") {
                    opening.swing = serde_json::from_value(value.clone())
                        .map_err(|error| invalid(error.to_string()))?;
                }
            }
            let mut parts: Vec<_> = self
                .drawing
                .doc
                .entities()
                .filter_map(|entity| {
                    let part: crate::objects::ObjectPart =
                        crate::xdata::read_json(entity.common(), crate::objects::PART_APP)?;
                    driver_indices
                        .contains_key(&part.owner)
                        .then_some((part, entity.common().handle.value()))
                })
                .collect();
            if driver_indices.keys().any(|handle| {
                self.drawing
                    .doc
                    .get_entity(Handle::new(*handle))
                    .and_then(crate::objects::instance)
                    .is_some()
            }) {
                group
                    .members
                    .retain(|(_, role, _)| matches!(role.as_str(), "run" | "opening"));
                parts.sort_by_key(|(part, handle)| (part.index, *handle));
                let mut indices = BTreeMap::new();
                for (part, handle) in parts {
                    let index = indices.entry(part.role.clone()).or_insert(0);
                    group.members.push((handle, part.role, *index));
                    *index += 1;
                }
            }
        }
        Ok(groups.into_values().collect())
    }

    /// Reads the editable systems on the current sheet, optionally with opening placement ranges.
    pub fn wall_groups(&self, width: Option<f64>) -> Result<Vec<WallGroupInfo>, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("walls.wall_groups");
        let _read = self.evaluation_read()?;
        self.stored_wall_groups()?
            .into_iter()
            .map(|group| {
                let spec = &group.spec;
                let runs = spec
                    .runs
                    .iter()
                    .enumerate()
                    .map(|(index, run)| {
                        let centreline = group
                            .members
                            .iter()
                            .find(|(_, role, i)| role == "run" && *i == index)
                            .map(|(h, _, _)| *h)
                            .unwrap_or(0);
                        let openings = spec
                            .openings
                            .iter()
                            .enumerate()
                            .filter(|(_, o)| o.run == index)
                            .map(|(i, o)| WallOpeningInfo {
                                mark: group
                                    .members
                                    .iter()
                                    .find(|(_, role, n)| role == "opening" && *n == i)
                                    .and_then(|(h, _, _)| {
                                        self.drawing.doc.get_entity(Handle::new(*h))
                                    })
                                    .and_then(crate::opening_schedule::mark),
                                insert: group
                                    .members
                                    .iter()
                                    .find(|(_, role, n)| role == "opening" && *n == i)
                                    .map(|(h, _, _)| *h)
                                    .unwrap_or(0),
                                opening: o.clone(),
                            })
                            .collect();
                        let free_ranges = width
                            .map(|width| {
                                walls::where_it_fits(
                                    &geom_runs(spec),
                                    spec.thickness,
                                    &geom_openings(spec),
                                    index,
                                    width,
                                    self.measure_tol(),
                                    geom_justification(spec.justification),
                                )
                                .map_err(|e| invalid(e.to_string()))
                            })
                            .transpose()?;
                        Ok(WallRunInfo {
                            centreline,
                            points: run.points.to_vec(),
                            closed: run.closed,
                            length: run_segments(run)
                                .iter()
                                .map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
                                .sum(),
                            thickness: run.thickness.unwrap_or(spec.thickness),
                            height: run.height.unwrap_or(spec.height),
                            justification: run.justification.unwrap_or(spec.justification),
                            openings,
                            free_ranges,
                        })
                    })
                    .collect::<Result<_, CoreError>>()?;
                Ok(WallGroupInfo {
                    group: group.id,
                    handles: group.members.iter().map(|(h, _, _)| *h).collect(),
                    layers: spec.layers.clone(),
                    runs,
                })
            })
            .collect()
    }

    pub(crate) fn walls(
        &mut self,
        mut spec: WallSpec,
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        spec.normalize_runs(self.drawing.units);
        explicit_runs(&mut spec);
        let mut joined = Vec::new();
        if spec.join {
            let mut candidates = self.stored_wall_groups()?;
            while let Some(index) = candidates.iter().position(|g| {
                g.spec.join
                    && layer_names(&g.spec.layers) == layer_names(&spec.layers)
                    && systems_touch(&spec, &g.spec, self.measure_tol())
            }) {
                let group = candidates.remove(index);
                let mut old = group.spec.clone();
                explicit_runs(&mut old);
                // Existing runs retain their order, making handle-to-run selection predictable.
                let offset = old.runs.len();
                let opening_offset = old.openings.len();
                spec.defaulted = spec
                    .defaulted
                    .iter()
                    .filter_map(|path| {
                        let mut parts = path.split('/');
                        let role = parts.next()?;
                        let index = parts.next()?.parse::<usize>().ok()?;
                        let field = parts.next()?;
                        let shift = if role == "runs" {
                            offset
                        } else {
                            opening_offset
                        };
                        Some(format!("{role}/{}/{field}", index + shift))
                    })
                    .collect();
                for opening in &mut spec.openings {
                    opening.run += offset;
                }
                old.runs.extend(spec.runs);
                old.openings.extend(spec.openings);
                spec.runs = old.runs;
                spec.openings = old.openings;
                joined.push(group);
            }
        }
        self.rebuild_walls(spec, &joined, source)
    }

    pub(crate) fn rebuild_walls(
        &mut self,
        spec: WallSpec,
        old: &[WallGroup],
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("walls.rebuild_walls");
        let targets: Vec<_> = old
            .iter()
            .flat_map(|g| g.members.iter())
            .map(|(h, _, _)| crate::Target {
                handle: *h,
                chain: Vec::new(),
            })
            .collect();
        if !targets.is_empty() {
            self.sheet_targets(&targets)?;
        }
        // Allocate only source drivers for an edit. Generated parts belong to the object
        // engine, which runs after surviving identities and hosts are in their final places.
        let mut result = self.draw_wall_system(spec.clone(), old.is_empty(), source)?;
        if old.is_empty() {
            return Ok(result);
        }
        let old_parts: Vec<_> = old
            .iter()
            .flat_map(|g| &g.members)
            .filter(|(_, role, _)| !matches!(role.as_str(), "run" | "opening"))
            .filter_map(|(h, role, _)| {
                self.drawing
                    .doc
                    .get_entity(Handle::new(*h))
                    .map(|e| (*h, role.clone(), self.object_part_region(e)))
            })
            .collect();
        for group in old {
            for &(handle, ref role, index) in &group.members {
                let replacements = match role.as_str() {
                    "run" => group
                        .spec
                        .runs
                        .get(index)
                        .and_then(|run| {
                            spec.runs
                                .iter()
                                .position(|r| r.points == run.points && r.closed == run.closed)
                        })
                        .map(|i| vec![result.centrelines[i]])
                        .unwrap_or_default(),
                    "opening" => group
                        .spec
                        .openings
                        .get(index)
                        .and_then(|opening| {
                            let old_run = group.spec.runs.get(opening.run)?;
                            spec.openings.iter().position(|o| {
                                spec.runs[o.run].points == old_run.points
                                    && o.at == opening.at
                                    && o.width == opening.width
                            })
                        })
                        .or_else(|| {
                            (old.len() == 1 && group.spec.openings.len() == spec.openings.len())
                                .then_some(index)
                        })
                        .map(|i| vec![result.inserts[i]])
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                result.handle_map.insert(handle, replacements);
            }
        }
        // Compatibility rebuilds retain data supplied through the common object commands,
        // including levels and additional properties from a redefined type.
        let objects: Vec<_> = result
            .handle_map
            .iter()
            .filter_map(|(old, replacements)| {
                self.drawing
                    .doc
                    .get_entity(Handle::new(*old))
                    .and_then(crate::objects::instance)
                    .map(|object| (object, replacements.clone()))
            })
            .collect();
        for (mut object, replacements) in objects {
            if let Some(host) = object.host {
                object.host = result
                    .handle_map
                    .get(&host)
                    .and_then(|r| r.first())
                    .copied()
                    .or(Some(host));
            }
            for handle in replacements {
                if let Some(new) = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(handle))
                    .and_then(crate::objects::instance)
                {
                    object.assembly = new.assembly;
                    object.layers = new.layers;
                    object.type_name = new.type_name;
                    for key in [
                        "width",
                        "wall_height",
                        "justification",
                        "distance",
                        "sill",
                        "head",
                        "swing",
                        "host_width",
                    ] {
                        if let Some(value) = new.props.get(key) {
                            if object.props.get(key) != Some(value) {
                                if let Some(source) = new.sources.get(key) {
                                    object.sources.insert(key.into(), source.clone());
                                }
                            }
                            object.props.insert(key.into(), value.clone());
                        }
                    }
                }
                object.id = handle;
                self.stamp_object(handle, &object)?;
            }
        }
        // Reuse surviving identities before publishing the rebuild. Geometry may change,
        // but a wall run and an opening remain the same selected object.
        let mut reuse = BTreeMap::new();
        for group in old {
            for &(handle, ref role, _) in &group.members {
                // Only a member still in the drawing can lend its handle to its replacement.
                if self.drawing.doc.get_entity(Handle::new(handle)).is_none() {
                    continue;
                }
                let Some(candidates) = result.handle_map.get(&handle) else {
                    continue;
                };
                if matches!(role.as_str(), "run" | "opening") && candidates.len() == 1 {
                    reuse.entry(candidates[0]).or_insert(handle);
                }
            }
        }
        let remap = |h: u64| reuse.get(&h).copied().unwrap_or(h);
        let app = self.object_app(crate::objects::PART_APP)?;
        let snapshots: Vec<_> = result
            .handles
            .iter()
            .filter_map(|&h| {
                self.drawing
                    .doc
                    .get_entity(Handle::new(h))
                    .cloned()
                    .map(|e| (h, e))
            })
            .collect();
        for (new, mut entity) in snapshots {
            let handle = remap(new);
            if new != handle {
                let previous = self
                    .drawing
                    .doc
                    .get_entity(Handle::new(handle))
                    .expect("old member");
                let data = entity.common().extended_data.clone();
                *entity.common_mut() = previous.common().clone();
                for record in data.records() {
                    crate::xdata::write_values(
                        entity.common_mut(),
                        &record.application_name,
                        crate::xdata::app_handle(&self.drawing.doc, &record.application_name),
                        Some(record.values.clone()),
                    );
                }
                entity.common_mut().raw_record = None;
                entity.common_mut().graphic_data = None;
            }
            if let Some(mut object) = crate::objects::instance(&entity) {
                object.id = handle;
                object.host = object.host.map(remap);
                crate::xdata::write_json(
                    entity.common_mut(),
                    crate::objects::OBJECT_APP,
                    crate::xdata::app_handle(&self.drawing.doc, crate::objects::OBJECT_APP),
                    &object,
                )?;
            }
            if let Some(mut part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                entity.common(),
                crate::objects::PART_APP,
            ) {
                part.owner = remap(part.owner);
                crate::xdata::write_json(
                    entity.common_mut(),
                    crate::objects::PART_APP,
                    app,
                    &part,
                )?;
            }
            let tag = wall_tag(&entity).map(|mut tag| {
                tag.group = remap(tag.group);
                tag
            });
            self.drawing
                .doc
                .replace_entity_arc(Handle::new(handle), std::sync::Arc::new(entity));
            if let Some(tag) = tag {
                self.stamp_wall(handle, &tag)?;
            }
            if new != handle {
                self.drawing.doc.remove_entity_arc(Handle::new(new));
            }
        }
        result.group = remap(result.group);
        for handles in [
            &mut result.handles,
            &mut result.centrelines,
            &mut result.inserts,
            &mut result.outlines,
            &mut result.hatches,
        ] {
            handles.iter_mut().for_each(|h| *h = remap(*h));
        }
        for handles in result.handle_map.values_mut() {
            handles.iter_mut().for_each(|h| *h = remap(*h));
        }
        let owner = *result.centrelines.iter().min().expect("wall has a run");
        let wall_app = crate::xdata::app_handle(&self.drawing.doc, WALL_APP);
        for (handle, _, _) in &old_parts {
            if let Some(entity) = self.drawing.doc.get_entity_mut(Handle::new(*handle)) {
                if let Some(mut part) = crate::xdata::read_json::<crate::objects::ObjectPart>(
                    entity.common(),
                    crate::objects::PART_APP,
                ) {
                    part.owner = owner;
                    crate::xdata::write_values(entity.common_mut(), WALL_APP, wall_app, None);
                    crate::xdata::write_json(
                        entity.common_mut(),
                        crate::objects::PART_APP,
                        app,
                        &part,
                    )?;
                }
            }
        }
        let removed: Vec<_> = result
            .handle_map
            .keys()
            .filter(|h| {
                !result.handles.contains(h) && !old_parts.iter().any(|(part, _, _)| part == *h)
            })
            .copied()
            .map(Handle::new)
            .collect();
        for &handle in &removed {
            self.drawing.doc.remove_entity_arc(handle);
        }
        let scope = result
            .centrelines
            .iter()
            .chain(&result.inserts)
            .copied()
            .collect();
        self.refresh_objects_scoped(Some(&scope))?;
        self.sync_legacy_wall_tags()?;
        let group = self
            .stored_wall_groups()?
            .into_iter()
            .find(|g| g.id == result.group)
            .ok_or_else(|| invalid("The rebuilt wall is missing"))?;
        let mut final_result = self.wall_result(&group)?;
        let old_outline_count = old_parts
            .iter()
            .filter(|(_, role, _)| role == "outline")
            .count();
        for (old, role, region) in old_parts {
            let candidates = if role == "hatch" {
                &final_result.hatches
            } else {
                &final_result.outlines
            };
            let replacements = if role == "hatch" {
                let overlapping: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|h| {
                        let after = self
                            .drawing
                            .doc
                            .get_entity(Handle::new(*h))
                            .and_then(|e| self.object_part_region(e));
                        region
                            .as_ref()
                            .zip(after.as_ref())
                            .is_some_and(|(a, b)| walls::regions_overlap(a, b, self.measure_tol()))
                    })
                    .collect();
                if overlapping.is_empty() && candidates.contains(&old) {
                    vec![old]
                } else {
                    overlapping
                }
            } else if candidates.contains(&old) && old_outline_count == candidates.len() {
                vec![old]
            } else {
                candidates.clone()
            };
            result.handle_map.insert(old, replacements);
        }
        let pieces: Vec<_> = result
            .handle_map
            .iter()
            .filter(|(h, into)| into.as_slice() != [**h])
            .map(|(&h, into)| (Handle::new(h), into.clone()))
            .collect();
        self.follow_edit(&[], &removed, &pieces, &[]);
        self.remap_wall_fields(&result.handle_map)?;
        self.remember_replacements(&result.handle_map)?;
        final_result.handle_map = result.handle_map;
        Ok(final_result)
    }

    /// Whole assemblies need one union. Summing every member's prefix difference would
    /// solve the same union twice per run, even though those differences telescope.
    fn wall_totals(&self, centrelines: &[u64]) -> Result<[f64; 4], CoreError> {
        #[cfg(any(test, feature = "fixtures"))]
        let _profile = crate::fixtures::profile::span("walls.wall_totals");
        let _read = self.evaluation_read()?;
        let mut seen = BTreeSet::new();
        let mut total = [0.0; 4];
        let factor = self.drawing.units.to_metres();
        for &handle in centrelines {
            if seen.contains(&handle) {
                continue;
            }
            let object = self
                .drawing
                .doc
                .get_entity(Handle::new(handle))
                .and_then(crate::objects::instance)
                .ok_or_else(|| invalid("Missing wall driver"))?;
            let definition = self.object_type(&object.type_name)?;
            let members = self.band_members(&object, &definition)?;
            if members.iter().any(|m| !centrelines.contains(&m.id))
                || definition.plan.band_faces.is_none()
            {
                // A retyped or partial assembly retains the general per-object contract.
                let records = self.objects_for(centrelines)?;
                return Ok(["length", "gross_area", "net_area", "volume"].map(|name| {
                    records
                        .iter()
                        .filter(|r| centrelines.contains(&r.instance.id))
                        .filter_map(|r| r.facts.get(name))
                        .sum()
                }));
            }
            seen.extend(members.iter().map(|m| m.id));
            if let Some(band) = self.band_geometry(&object, &definition, None)? {
                for (value, add) in total.iter_mut().zip([
                    band.geometry.centreline * factor,
                    band.geometry.gross_area * factor * factor,
                    band.geometry.net_area * factor * factor,
                    band.net_volume,
                ]) {
                    *value += add;
                }
            }
        }
        Ok(total)
    }

    fn wall_result(&self, group: &WallGroup) -> Result<WallResult, CoreError> {
        let role = |name: &str| {
            group
                .members
                .iter()
                .filter(|(_, r, _)| r == name)
                .map(|(h, _, _)| *h)
                .collect::<Vec<_>>()
        };
        let centrelines = role("run");
        let totals = self.wall_totals(&centrelines)?;
        let mut defaults_used = Vec::new();
        for (handle, role, index) in &group.members {
            let Some(object) = self
                .drawing
                .doc
                .get_entity(Handle::new(*handle))
                .and_then(crate::objects::instance)
            else {
                continue;
            };
            let fields: &[(&str, &str)] = match role.as_str() {
                "run" => &[("wall_height", "height")],
                "opening" => &[("sill", "sill"), ("head", "head")],
                _ => &[],
            };
            for (property, field) in fields {
                if object
                    .sources
                    .get(*property)
                    .is_some_and(|s| s.kind == crate::measurement::SourceKind::TypeDefault)
                {
                    if let Some(value) = object.props.get(*property) {
                        defaults_used.push(crate::validation::DefaultUsed {
                            field: format!(
                                "/{}/{index}/{field}",
                                if role == "run" { "runs" } else { "openings" }
                            ),
                            value: value.clone(),
                            unit: Some("m".into()),
                        });
                    }
                }
            }
        }
        Ok(WallResult {
            defaults_used,
            group: group.id,
            handles: group.members.iter().map(|(h, _, _)| *h).collect(),
            outlines: role("outline"),
            hatches: role("hatch"),
            inserts: role("opening"),
            centreline_length: totals[0],
            gross_area: totals[1],
            net_area: totals[2],
            volume: totals[3],
            centrelines,
            handle_map: BTreeMap::new(),
        })
    }

    fn wall_group_for(&self, handle: u64, role: &str) -> Result<(WallGroup, usize), CoreError> {
        self.sheet_targets(&[crate::Target {
            handle,
            chain: Vec::new(),
        }])?;
        for group in self.stored_wall_groups()? {
            if let Some((_, _, index)) = group
                .members
                .iter()
                .find(|(h, r, _)| *h == handle && r == role)
            {
                let index = *index;
                return Ok((group, index));
            }
        }
        Err(invalid(format!(
            "Handle {handle} is not a wall {role}; use wall_groups to read the system"
        )))
    }

    /// Independent additions can validate in input order and regenerate each assembly once.
    /// The returned wall results describe the final shared geometry for each input's group.
    pub(crate) fn add_wall_openings_many(
        &mut self,
        commands: &[(u64, Vec<AddedWallOpening>)],
        source: crate::Source,
    ) -> Result<Vec<WallResult>, CoreError> {
        let groups = self.stored_wall_groups()?;
        let mut pending = BTreeMap::<u64, (WallGroup, WallSpec, usize)>::new();
        let mut order = Vec::new();
        for (index, (handle, openings)) in commands.iter().enumerate() {
            let step = |error| CoreError::Step {
                index,
                label: "add_wall_openings".into(),
                error: Box::new(error),
            };
            if openings.is_empty() {
                return Err(step(invalid("Add at least one wall opening")));
            }
            let (group, run) = groups
                .iter()
                .find_map(|g| {
                    g.members
                        .iter()
                        .find(|(h, role, _)| h == handle && role == "run")
                        .map(|(_, _, run)| (g, *run))
                })
                .ok_or_else(|| {
                    step(invalid(format!(
                        "Handle {handle} is not a wall run; use wall_groups to read the system"
                    )))
                })?;
            let (_, spec, _) = pending
                .entry(group.id)
                .or_insert_with(|| (group.clone(), group.spec.clone(), index));
            spec.openings.extend(openings.iter().map(|o| WallOpening {
                run,
                at: o.at,
                width: o.width,
                kind: o.kind,
                swing: o.swing,
                sill: o.sill,
                head: o.head,
            }));
            explicit_runs(spec);
            for opening in &spec.openings {
                let sill = opening.sill.expect("resolved sill");
                let head = opening.head.expect("resolved head");
                if !sill.is_finite()
                    || !head.is_finite()
                    || sill < 0.0
                    || head <= sill
                    || head > spec.runs[opening.run].height.unwrap_or(spec.height)
                {
                    return Err(step(invalid(
                        "An opening needs 0 <= sill < head <= wall height, in metres",
                    )));
                }
            }
            // Validate each prefix so an error still names the first failing input and its
            // fitting ranges. This is geometry only: no temporary objects or full derivation.
            walls::build_walls_justified(
                &geom_runs(spec),
                spec.thickness,
                &geom_openings(spec),
                self.measure_tol(),
                geom_justification(spec.justification),
            )
            .map_err(|error| step(invalid(error.to_string())))?;
            order.push(group.id);
        }
        let mut results = BTreeMap::new();
        // Preserve first-use order even when group handles have been reused or copied.
        let mut groups: Vec<_> = pending.into_iter().collect();
        groups.sort_by_key(|(_, (_, _, index))| *index);
        for (id, (group, spec, index)) in groups {
            let result = self
                .rebuild_walls(spec, &[group], source)
                .map_err(|error| CoreError::Step {
                    index,
                    label: "add_wall_openings".into(),
                    error: Box::new(error),
                })?;
            results.insert(id, result);
        }
        Ok(order.into_iter().map(|id| results[&id].clone()).collect())
    }

    pub(crate) fn add_wall_openings(
        &mut self,
        centreline: u64,
        openings: Vec<AddedWallOpening>,
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        if openings.is_empty() {
            return Err(invalid("Add at least one wall opening"));
        }
        let (group, run) = self.wall_group_for(centreline, "run")?;
        let mut spec = group.spec.clone();
        spec.openings
            .extend(openings.into_iter().map(|o| WallOpening {
                run,
                at: o.at,
                width: o.width,
                kind: o.kind,
                swing: o.swing,
                sill: o.sill,
                head: o.head,
            }));
        explicit_runs(&mut spec);
        self.rebuild_walls(spec, &[group], source)
    }

    pub(crate) fn remove_wall_openings(
        &mut self,
        inserts: &[u64],
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        if inserts.is_empty() {
            return Err(invalid("Pick at least one opening insert"));
        }
        let (group, _) = self.wall_group_for(inserts[0], "opening")?;
        let mut indices = BTreeSet::new();
        for &handle in inserts {
            let (other, index) = self.wall_group_for(handle, "opening")?;
            if other.id != group.id {
                return Err(invalid("Remove openings from one wall group per command"));
            }
            indices.insert(index);
        }
        let mut spec = group.spec.clone();
        spec.openings = spec
            .openings
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !indices.contains(i))
            .map(|(_, o)| o)
            .collect();
        self.rebuild_walls(spec, &[group], source)
    }

    pub(crate) fn edit_wall_opening(
        &mut self,
        insert: u64,
        edit: WallOpeningEdit,
        source: crate::Source,
    ) -> Result<WallResult, CoreError> {
        let (group, index) = self.wall_group_for(insert, "opening")?;
        let mut spec = group.spec.clone();
        let opening = &mut spec.openings[index];
        if let Some(v) = edit.at {
            opening.at = v;
        }
        if let Some(v) = edit.width {
            opening.width = v;
        }
        if let Some(v) = edit.kind {
            opening.kind = v;
        }
        if let Some(v) = edit.swing {
            opening.swing = v;
        }
        if let Some(v) = edit.sill {
            opening.sill = Some(v);
        }
        if let Some(v) = edit.head {
            opening.head = Some(v);
        }
        let supplied: Vec<_> = [("sill", edit.sill), ("head", edit.head)]
            .into_iter()
            .filter_map(|(key, v)| v.map(|v| (key, v)))
            .collect();
        for (key, _) in &supplied {
            spec.defaulted
                .retain(|p| p != &format!("openings/{index}/{key}"));
        }
        if !supplied.is_empty() {
            if let Some(mut object) = self
                .drawing
                .doc
                .get_entity(Handle::new(insert))
                .and_then(crate::objects::instance)
            {
                for (key, _) in &supplied {
                    object.sources.insert(
                        (*key).into(),
                        crate::measurement::PropSource {
                            decision: None,
                            kind: crate::objects::given_by(source).kind,
                            handle: None,
                            text: "Explicit command input / قيمة صريحة في الأمر".into(),
                        },
                    );
                }
                self.stamp_object(insert, &object)?;
            }
        }
        if let Some(mark) = edit.mark {
            self.set_object_props(
                &[insert],
                &BTreeMap::from([("mark".into(), serde_json::json!(mark))]),
                None,
                &BTreeMap::new(),
                crate::measurement::PropSource::default(),
            )?;
        }
        if crate::net_effect::same_value(&serde_json::json!(&spec), &serde_json::json!(&group.spec))
        {
            return self.wall_result(&group);
        }
        self.rebuild_walls(spec, &[group], source)
    }

    pub(crate) fn refuse_wall_insert_erase(&self, handles: &[Handle]) -> Result<(), CoreError> {
        for &handle in handles {
            if self
                .drawing
                .doc
                .get_entity(handle)
                .and_then(wall_tag)
                .is_some_and(|tag| tag.role == "opening")
            {
                return Err(invalid(format!(
                    "Opening insert {} belongs to a wall system; use remove_wall_openings to close the wall in the same undo step",
                    handle.value()
                )));
            }
        }
        Ok(())
    }

    /// Copies have independent identities and definitions derived from their actual centrelines.
    pub(crate) fn copied_wall_groups(
        &mut self,
        handles: &[u64],
        mirrored: bool,
    ) -> Result<(), CoreError> {
        let mut groups: BTreeMap<u64, Vec<(u64, WallTag)>> = BTreeMap::new();
        for &handle in handles {
            if let Some(tag) = self
                .drawing
                .doc
                .get_entity(Handle::new(handle))
                .and_then(wall_tag)
            {
                groups.entry(tag.group).or_default().push((handle, tag));
            }
        }
        for members in groups.values_mut() {
            let mut spec = members[0].1.spec.clone();
            let mut run_count = 0;
            let mut scale = 1.0;
            for (handle, tag) in members.iter() {
                if tag.role == "run" {
                    run_count += 1;
                    if let Some(EntityType::LwPolyline(poly)) =
                        self.drawing.doc.get_entity(Handle::new(*handle))
                    {
                        let old = &spec.runs[tag.index].points;
                        if old.len() >= 2 && poly.vertices.len() >= 2 {
                            let a = poly.vertices[0].location;
                            let b = poly.vertices[1].location;
                            scale = (b.x - a.x).hypot(b.y - a.y)
                                / (old[1][0] - old[0][0]).hypot(old[1][1] - old[0][1]);
                        }
                        spec.runs[tag.index].points = poly
                            .vertices
                            .iter()
                            .map(|v| [v.location.x, v.location.y])
                            .collect();
                    }
                }
            }
            if run_count != spec.runs.len()
                || members
                    .iter()
                    .filter(|(_, tag)| tag.role == "opening")
                    .count()
                    != spec.openings.len()
            {
                return Err(invalid(
                    "Copy the complete wall group, including every centreline and opening insert",
                ));
            }
            let typed = members
                .iter()
                .filter(|(_, tag)| matches!(tag.role.as_str(), "run" | "opening"))
                .all(|(handle, _)| {
                    self.drawing
                        .doc
                        .get_entity(Handle::new(*handle))
                        .and_then(crate::objects::instance)
                        .is_some()
                });
            if !typed {
                let built = walls::build_walls_justified(
                    &geom_runs(&members[0].1.spec),
                    members[0].1.spec.thickness,
                    &geom_openings(&members[0].1.spec),
                    self.measure_tol(),
                    geom_justification(members[0].1.spec.justification),
                )
                .map_err(|e| invalid(e.to_string()))?;
                let outlines: usize = built.regions.iter().map(|r| 1 + r.holes.len()).sum();
                if members
                    .iter()
                    .filter(|(_, tag)| tag.role == "hatch")
                    .count()
                    != built.regions.len()
                    || members
                        .iter()
                        .filter(|(_, tag)| tag.role == "outline")
                        .count()
                        != outlines
                {
                    return Err(invalid(
                        "Copy the complete wall group, including its outlines and hatches",
                    ));
                }
            }
            spec.thickness *= scale;
            // A reflection changes the opening side. Reverse the directed centreline so
            // its left/right convention continues to describe the mirrored symbols.
            if mirrored {
                for (handle, tag) in members.iter().filter(|(_, tag)| tag.role == "run") {
                    if let Some(EntityType::LwPolyline(poly)) =
                        self.drawing.doc.get_entity_mut(Handle::new(*handle))
                    {
                        if poly.is_closed {
                            poly.vertices[1..].reverse();
                        } else {
                            poly.vertices.reverse();
                        }
                        spec.runs[tag.index].points = poly
                            .vertices
                            .iter()
                            .map(|v| [v.location.x, v.location.y])
                            .collect();
                    }
                }
            }
            for run in &mut spec.runs {
                run.thickness = run.thickness.map(|value| value * scale);
            }
            for opening in &mut spec.openings {
                opening.at *= scale;
                opening.width *= scale;
                if mirrored {
                    let length: f64 = run_segments(&spec.runs[opening.run])
                        .iter()
                        .map(|(a, b)| (b[0] - a[0]).hypot(b[1] - a[1]))
                        .sum();
                    opening.at = length - opening.at;
                    opening.swing = match opening.swing {
                        DoorSwing::Left => DoorSwing::Right,
                        DoorSwing::Right => DoorSwing::Left,
                    };
                }
            }
            let group = members.iter().map(|(h, _)| *h).min().unwrap();
            for (handle, tag) in members {
                tag.group = group;
                tag.spec = spec.clone();
                self.stamp_wall(*handle, tag)?;
                if matches!(tag.role.as_str(), "run" | "opening") {
                    if let Some(mut object) = self
                        .drawing
                        .doc
                        .get_entity(Handle::new(*handle))
                        .and_then(crate::objects::instance)
                    {
                        // The old copy API scales its physical dimensions too. Re-import its
                        // translated data once; subsequent regeneration uses the typed instance.
                        object.compatibility = Some("copy".into());
                        self.stamp_object(*handle, &object)?;
                    }
                }
            }
        }
        Ok(())
    }
}
