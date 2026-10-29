//! Conventional architectural symbols, composed through the ordinary drawing APIs.
//! Coordinates are drawing units, physical dimensions ending in `_m` are metres, and
//! annotation sizes follow the sheet scale. These are drafting aids, not structural design.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::drafting_standards::standards;
use crate::rooms::LabelLanguage;
use crate::{CoreError, Current, DrawSpec, HatchPattern, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StairFloor {
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaterialView {
    #[default]
    Section,
    Detail,
    Elevation,
}

/// A named level measured from the origin's datum, in metres.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LevelMark {
    pub label: String,
    pub elevation_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SymbolSpec {
    /// Opening from a to b; side_point selects the side it swings towards. The first hinge
    /// is at a. Omitted leaves use the labelled double-leaf threshold in drafting knowledge.
    /// This standalone symbol does not cut a wall; use wall openings for hosted doors.
    #[schemars(example = json!({"kind":"door","a":[0,0],"b":[1.5,0],"side_point":[0,1],"leaves":2}))]
    #[schemars(example = json!({"kind":"door","a":[0,0],"b":[0.9,0],"side_point":[0,-1],"leaves":1}))]
    Door {
        a: [f64; 2],
        b: [f64; 2],
        side_point: [f64; 2],
        #[serde(default)]
        leaves: Option<u8>,
        #[serde(default)]
        layer: Option<String>,
    },
    /// A straight flight, rising along local +Y. Lower: UP, cut and hidden treads above it.
    /// Upper: DN, full descending flight, adjacent void and guarding. width is the flight
    /// width; the upper void occupies another width to its left. Use multiple flights and
    /// draw landings explicitly for an L/U stair. rise_m is this flight's rise, not a default.
    #[schemars(example = json!({"kind":"stair","origin":[0,0],"width":1.2,"length_m":3,"rise_m":1.7,"steps":10,"floor":"lower"}))]
    #[schemars(example = json!({"kind":"stair","origin":[0,0],"width":1.2,"length_m":3,"rise_m":1.7,"steps":10,"floor":"upper"}))]
    Stair {
        origin: [f64; 2],
        width: f64,
        length_m: f64,
        rise_m: f64,
        steps: u32,
        floor: StairFloor,
        #[serde(default)]
        rotation: f64,
        #[serde(default)]
        language: Option<LabelLanguage>,
        #[serde(default)]
        layer: Option<String>,
    },
    /// Elevation markers on one side of a view. origin is the zero datum; elevations are
    /// metres above it. Supply FFLs, roof and parapet from the design, never guessed levels.
    #[schemars(example = json!({"kind":"levels","origin":[0,0],"levels":[{"label":"FFL","elevation_m":0},{"label":"Roof","elevation_m":3.2},{"label":"Parapet","elevation_m":4.3}]}))]
    Levels {
        origin: [f64; 2],
        levels: Vec<LevelMark>,
        #[serde(default)]
        language: Option<LabelLanguage>,
        #[serde(default)]
        layer: Option<String>,
    },
    /// Heavy ground line with soil ticks on the right side of directed a-to-b (below a
    /// left-to-right elevation). Tick spacing and length follow paper scale.
    #[schemars(example = json!({"kind":"ground_line","a":[0,0],"b":[8,0]}))]
    GroundLine {
        a: [f64; 2],
        b: [f64; 2],
        #[serde(default)]
        layer: Option<String>,
    },
    /// Dashed foundation datum at a-to-b, explicitly labelled indicative. elevation_m is
    /// the supplied level annotation; this command does not size or design foundations.
    #[schemars(example = json!({"kind":"foundation_level","a":[0,-1.2],"b":[8,-1.2],"elevation_m":-1.2}))]
    FoundationLevel {
        a: [f64; 2],
        b: [f64; 2],
        elevation_m: f64,
        #[serde(default)]
        language: Option<LabelLanguage>,
        #[serde(default)]
        layer: Option<String>,
    },
    /// Hatch from the material table, heavy cut outline for section/detail and a material
    /// leader from point to at. Holes stay empty. An elevation material with no pattern
    /// gets its leader only. label can include a specified thickness or finish.
    #[schemars(example = json!({"kind":"material_hatch","points":[[0,0],[0.3,0],[0.3,3],[0,3]],"material":"masonry","point":[0.15,1.5],"at":[2,2]}))]
    #[schemars(example = json!({"kind":"material_hatch","points":[[0,0],[3,0],[3,0.3],[0,0.3]],"material":"concrete","view":"detail","point":[1,0.15],"at":[4,1],"label":"Concrete slab"}))]
    MaterialHatch {
        points: crate::PointRun<true>,
        #[serde(default)]
        holes: Vec<crate::PointRun<true>>,
        material: String,
        #[serde(default)]
        view: MaterialView,
        point: [f64; 2],
        at: [f64; 2],
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        language: Option<LabelLanguage>,
        #[serde(default)]
        layer: Option<String>,
    },
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(message.into())
}

fn finite(point: [f64; 2]) -> Result<(), CoreError> {
    if point.into_iter().all(f64::is_finite) {
        Ok(())
    } else {
        Err(invalid("Symbol coordinates must be finite"))
    }
}

fn direction(a: [f64; 2], b: [f64; 2]) -> Result<([f64; 2], f64), CoreError> {
    finite(a)?;
    finite(b)?;
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    if !length.is_finite() || length <= 0.0 {
        return Err(invalid("A symbol needs two distinct finite points"));
    }
    Ok(([(b[0] - a[0]) / length, (b[1] - a[1]) / length], length))
}

fn offset(p: [f64; 2], u: [f64; 2], d: f64) -> [f64; 2] {
    [p[0] + u[0] * d, p[1] + u[1] * d]
}

impl Session {
    /// Keep the caller's drawing state on success and failure; the command transaction owns
    /// geometry, tables and rollback. Never nest commands or create separate undo steps here.
    pub(crate) fn draft_symbol(&mut self, spec: SymbolSpec) -> Result<Vec<u64>, CoreError> {
        let current = self.current.clone();
        let result = self.add_draft_symbol(spec);
        self.current = current;
        result
    }

    fn symbol_layer(&mut self, name: &str, weight: i16) -> Result<(), CoreError> {
        if !self.drawing.doc.layers.contains(name) {
            self.new_layer(name)?;
        }
        let layer = self
            .drawing
            .doc
            .layers
            .get(name)
            .ok_or_else(|| invalid("Missing symbol layer"))?;
        if layer.flags.locked {
            return Err(invalid(format!("Layer {} is locked", layer.name)));
        }
        self.current = Current {
            layer: layer.name.clone(),
            lineweight: weight,
            ..Current::default()
        };
        Ok(())
    }

    fn symbol_line(
        &mut self,
        a: [f64; 2],
        b: [f64; 2],
        out: &mut Vec<u64>,
    ) -> Result<(), CoreError> {
        out.extend(self.draw(DrawSpec::Line { a, b })?);
        Ok(())
    }

    fn symbol_arabic(&self, language: Option<LabelLanguage>) -> bool {
        language.or(self.sheet_language()) == Some(LabelLanguage::Ar)
    }

    fn symbol_metres_per_unit(&self) -> f64 {
        if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            self.drawing.units.to_metres()
        } else {
            0.001 // Paper coordinates are millimetres.
        }
    }

    fn add_draft_symbol(&mut self, spec: SymbolSpec) -> Result<Vec<u64>, CoreError> {
        let data = &standards()["symbols"];
        let k = self.symbol_metres_per_unit();
        let h = self.default_text_height();
        let paper_mm = h / 2.5;
        let mut out = Vec::new();
        match spec {
            SymbolSpec::Door {
                a,
                b,
                side_point,
                leaves,
                layer,
            } => {
                let (u, width) = direction(a, b)?;
                finite(side_point)?;
                let cross = u[0] * (side_point[1] - a[1]) - u[1] * (side_point[0] - a[0]);
                if cross.abs() <= self.measure_tol() {
                    return Err(invalid("side_point must be off the door opening line"));
                }
                let threshold = data["double_leaf_from_m"]["value"].as_f64().unwrap_or(1.2);
                let defaulted_leaves = leaves.is_none();
                let leaves = leaves.unwrap_or(if width * k >= threshold { 2 } else { 1 });
                if !matches!(leaves, 1 | 2) {
                    return Err(invalid("A door has one or two leaves"));
                }
                let sign = cross.signum();
                let leaf = width / f64::from(leaves);
                let mut specs = vec![DrawSpec::Line {
                    a: [0.0, 0.0],
                    b: [0.0, sign * leaf],
                }];
                specs.push(DrawSpec::Arc {
                    centre: [0.0, 0.0],
                    radius: leaf,
                    start: if sign > 0.0 { 0.0 } else { 270.0 },
                    end: if sign > 0.0 { 90.0 } else { 360.0 },
                });
                if leaves == 2 {
                    specs.push(DrawSpec::Line {
                        a: [width, 0.0],
                        b: [width, sign * leaf],
                    });
                    specs.push(DrawSpec::Arc {
                        centre: [width, 0.0],
                        radius: leaf,
                        start: if sign > 0.0 { 90.0 } else { 180.0 },
                        end: if sign > 0.0 { 180.0 } else { 270.0 },
                    });
                }
                self.symbol_layer(
                    layer
                        .as_deref()
                        .unwrap_or(data["door"]["layer"].as_str().unwrap_or("A-DOOR")),
                    -1,
                )?;
                let base = "QIYAS_DRAFT_DOOR";
                let block = self.legacy_symbol_block(base, specs.clone())?;
                let handle = self.insert_block(
                    &block,
                    a,
                    1.0,
                    u[1].atan2(u[0]).to_degrees(),
                    &BTreeMap::new(),
                    None,
                )?;
                self.stamp_legacy_symbol_recipe(handle, base, &specs)?;
                if defaulted_leaves {
                    let app = self.object_app("QIYAS_DEFAULTS")?;
                    let entity = self
                        .drawing
                        .doc
                        .get_entity_mut(opencadcodec::Handle::new(handle))
                        .ok_or(CoreError::NoSuchEntity(handle))?;
                    crate::xdata::write_json(
                        entity.common_mut(),
                        "QIYAS_DEFAULTS",
                        app,
                        &json!({"leaves":{"value":leaves,"kind":"type_default","threshold_m":threshold}}),
                    )?;
                }
                out.push(handle);
            }
            SymbolSpec::Stair {
                origin,
                width,
                length_m,
                rise_m,
                steps,
                floor,
                rotation,
                language,
                layer,
            } => {
                finite(origin)?;
                if [width, length_m, rise_m]
                    .into_iter()
                    .any(|v| !v.is_finite() || v <= 0.0)
                    || !rotation.is_finite()
                    || !(2..=200).contains(&steps)
                {
                    return Err(invalid(
                        "Stair width, length and rise must be positive; steps must be 2 to 200",
                    ));
                }
                let arabic = self.symbol_arabic(language);
                let length = length_m / k;
                let (sin, cos) = rotation.to_radians().sin_cos();
                let p =
                    |x: f64, y: f64| [origin[0] + x * cos - y * sin, origin[1] + x * sin + y * cos];
                let stair = &data["stair"];
                let cut = (stair["cut_height_m"]["value"].as_f64().unwrap_or(1.2) / rise_m)
                    .clamp(0.1, 0.9)
                    * length;
                self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-STRS"), -1)?;
                self.symbol_line(p(0.0, 0.0), p(0.0, length), &mut out)?;
                self.symbol_line(p(width, 0.0), p(width, length), &mut out)?;
                for i in 0..=steps {
                    let y = length * f64::from(i) / f64::from(steps);
                    if floor == StairFloor::Lower && y > cut {
                        self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-STRS-HIDD"), -1)?;
                        self.ensure_convention_linetype("HIDDEN")?;
                        self.current.linetype = "HIDDEN".into();
                    } else {
                        self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-STRS"), -1)?;
                    }
                    self.symbol_line(p(0.0, y), p(width, y), &mut out)?;
                }
                if floor == StairFloor::Lower {
                    self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-STRS"), 50)?;
                    let zig = [
                        p(0.0, cut),
                        p(0.4 * width, cut),
                        p(0.6 * width, cut + 0.15 * width),
                        p(width, cut + 0.15 * width),
                    ];
                    out.extend(self.draw(DrawSpec::Polyline {
                        points: zig.to_vec().into(),
                        bulges: vec![],
                        closed: false,
                    })?);
                } else {
                    // The upper-floor slab has an open well beside the descending flight.
                    self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-STRS"), -1)?;
                    for (a, b) in [
                        (p(-width, 0.0), p(-width, length)),
                        (p(-width, 0.0), p(0.0, 0.0)),
                        (p(-width, length), p(0.0, length)),
                    ] {
                        self.symbol_line(a, b, &mut out)?;
                    }
                    self.symbol_layer(layer.as_deref().unwrap_or("A-FLOR-HRAL"), -1)?;
                    let rail = (stair["handrail_offset_m"]["value"].as_f64().unwrap_or(0.06) / k)
                        .min(width / 4.0);
                    for x in [rail, width - rail] {
                        self.symbol_line(p(x, 0.0), p(x, length), &mut out)?;
                    }
                    // Guard the other three sides of the void as well.
                    out.extend(
                        self.draw(DrawSpec::Polyline {
                            points: vec![
                                p(0.0, rail),
                                p(-width + rail, rail),
                                p(-width + rail, length - rail),
                                p(0.0, length - rail),
                            ]
                            .into(),
                            bulges: vec![],
                            closed: false,
                        })?,
                    );
                    self.symbol_layer(layer.as_deref().unwrap_or("A-ANNO-STRS"), -1)?;
                    out.push(self.add_text(
                        p(-width + h * 0.3, length / 2.0),
                        0.0,
                        rotation,
                        if arabic { "فراغ" } else { "VOID" }.into(),
                        None,
                    )?);
                }
                self.symbol_layer(layer.as_deref().unwrap_or("A-ANNO-STRS"), -1)?;
                let (tip, tail, label) = if floor == StairFloor::Lower {
                    (
                        p(width / 2.0, cut * 0.8),
                        p(width / 2.0, length * 0.1),
                        if arabic { "صعود" } else { "UP" },
                    )
                } else {
                    (
                        p(width / 2.0, length * 0.1),
                        p(width / 2.0, length * 0.85),
                        if arabic { "نزول" } else { "DN" },
                    )
                };
                out.extend(self.add_leader(&[tip, tail], "", false)?);
                out.push(self.add_text(
                    p(width + h, length / 2.0),
                    0.0,
                    rotation,
                    label.into(),
                    None,
                )?);
            }
            SymbolSpec::Levels {
                origin,
                levels,
                language,
                layer,
            } => {
                finite(origin)?;
                if levels.is_empty() || levels.len() > 100 {
                    return Err(invalid("Give 1 to 100 named levels"));
                }
                let arabic = self.symbol_arabic(language);
                self.symbol_layer(layer.as_deref().unwrap_or("A-ANNO-LEVL"), -1)?;
                let size = data["levels"]["marker_size_paper_mm"]
                    .as_f64()
                    .unwrap_or(3.0)
                    * paper_mm;
                for level in levels {
                    if !level.elevation_m.is_finite() || level.label.trim().is_empty() {
                        return Err(invalid("Each level needs a label and finite elevation_m"));
                    }
                    let at = [origin[0], origin[1] + level.elevation_m / k];
                    let ring = vec![
                        at,
                        [at[0] + size, at[1] + size],
                        [at[0] - size, at[1] + size],
                    ];
                    out.extend(self.draw(DrawSpec::Polyline {
                        points: ring.into(),
                        bulges: vec![],
                        closed: true,
                    })?);
                    self.symbol_line(at, [at[0] + 5.0 * size, at[1]], &mut out)?;
                    let text = format!(
                        "{} {:+.2} {}",
                        level.label,
                        level.elevation_m,
                        if arabic { "م" } else { "m" }
                    );
                    let text = if arabic {
                        crate::numbers::arabic_digits(&text)
                    } else {
                        text
                    };
                    out.push(self.add_text(
                        [at[0] + 1.5 * size, at[1] + h * 0.4],
                        0.0,
                        0.0,
                        text,
                        None,
                    )?);
                }
            }
            SymbolSpec::GroundLine { a, b, layer } => {
                let (u, length) = direction(a, b)?;
                let ground = &data["ground_line"];
                let spacing = ground["tick_spacing_paper_mm"].as_f64().unwrap_or(4.0) * paper_mm;
                let tick = ground["tick_length_paper_mm"].as_f64().unwrap_or(3.0) * paper_mm;
                let count = (length / spacing).ceil();
                if count > 10_000.0 {
                    return Err(invalid(
                        "Ground line has too many ticks at this annotation scale",
                    ));
                }
                self.symbol_layer(layer.as_deref().unwrap_or("A-ELEV-GRND"), 70)?;
                self.symbol_line(a, b, &mut out)?;
                self.current.lineweight = -1;
                for i in 0..=count as usize {
                    let p = offset(a, u, i as f64 * length / count);
                    let q = offset(offset(p, [u[1], -u[0]], tick), u, -tick * 0.5);
                    self.symbol_line(p, q, &mut out)?;
                }
            }
            SymbolSpec::FoundationLevel {
                a,
                b,
                elevation_m,
                language,
                layer,
            } => {
                direction(a, b)?;
                if !elevation_m.is_finite() {
                    return Err(invalid("Foundation elevation_m must be finite"));
                }
                let arabic = self.symbol_arabic(language);
                self.symbol_layer(layer.as_deref().unwrap_or("A-SECT-FNDN"), -1)?;
                self.ensure_convention_linetype("HIDDEN")?;
                self.current.linetype = "HIDDEN".into();
                self.symbol_line(a, b, &mut out)?;
                self.current.linetype = "ByLayer".into();
                let key = if arabic { "label_ar" } else { "label_en" };
                let text = format!(
                    "{} {:+.2} {}",
                    data["foundation_level"][key]
                        .as_str()
                        .unwrap_or("Foundation (indicative)"),
                    elevation_m,
                    if arabic { "م" } else { "m" }
                );
                let text = if arabic {
                    crate::numbers::arabic_digits(&text)
                } else {
                    text
                };
                out.push(self.add_text([a[0], a[1] + h], 0.0, 0.0, text, None)?);
            }
            SymbolSpec::MaterialHatch {
                points,
                holes,
                material,
                view,
                point,
                at,
                label,
                language,
                layer,
            } => {
                direction(point, at)?;
                let material = material.trim().to_lowercase();
                let entry = standards()["materials"].get(&material).ok_or_else(|| {
                    invalid("Unknown material: read drafting_knowledge topic materials")
                })?;
                let arabic = self.symbol_arabic(language);
                let pattern_key = if view == MaterialView::Elevation {
                    "elevation_pattern"
                } else {
                    "section_pattern"
                };
                let default_layer = if view == MaterialView::Detail {
                    "A-DETL-PATT"
                } else {
                    "A-SECT-PATT"
                };
                self.symbol_layer(layer.as_deref().unwrap_or(default_layer), -1)?;
                if let Some(name) = entry[pattern_key].as_str() {
                    let view_key = match view {
                        MaterialView::Section => "section",
                        MaterialView::Detail => "detail",
                        MaterialView::Elevation => "elevation",
                    };
                    let spacing = data["hatch"]["spacing_paper_mm"][view_key]
                        .as_f64()
                        .unwrap_or(2.5)
                        * paper_mm;
                    // Built-in base spacing is 3.175 drawing units for ANSI patterns; other
                    // material patterns retain their own proportions through the same API.
                    let pattern = if name == "SOLID" {
                        HatchPattern::Solid
                    } else {
                        HatchPattern::Custom {
                            name: name.into(),
                            angle: 0.0,
                            scale: spacing / 3.175,
                        }
                    };
                    out.extend(self.draw(DrawSpec::Hatch {
                        boundary_handles: vec![],
                        points: points.clone(),
                        holes: holes.clone(),
                        bulges: vec![],
                        pattern,
                    })?);
                }
                if view != MaterialView::Elevation {
                    self.symbol_layer(layer.as_deref().unwrap_or("A-SECT-CUT"), 50)?;
                    for ring in std::iter::once(points).chain(holes) {
                        out.extend(self.draw(DrawSpec::Polyline {
                            points: Vec::<[f64; 2]>::from(ring).into(),
                            bulges: vec![],
                            closed: true,
                        })?);
                    }
                }
                let text = label.unwrap_or_else(|| {
                    entry[if arabic { "title_ar" } else { "title_en" }]
                        .as_str()
                        .unwrap_or(&material)
                        .into()
                });
                if text.trim().is_empty() {
                    return Err(invalid("A material leader needs a label"));
                }
                self.symbol_layer(layer.as_deref().unwrap_or("A-ANNO-NOTE"), -1)?;
                out.extend(self.add_leader(&[point, at], &text, true)?);
            }
        }
        Ok(out)
    }
}
