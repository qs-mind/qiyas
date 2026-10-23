//! Rooms: the smallest closed enclosure round each room name or seed, excluding annotation,
//! grid graphics and opening symbols. Qiyas walls close at their faces through openings. `enclosed_area_m2` is what the line work
//! encloses; `net_area_m2` also takes the islands off (a fountain, a shaft) unless
//! `subtract_islands` is false. Edges bent along arcs are exact. Texts that look like areas or
//! numbers are skipped, so labelling twice does not label the labels.
//!
//! These are candidate areas only: geometry cannot grant professional approval, so
//! every room says `measurement_basis` "unapproved_net" and `net_accuracy_eligible` false, and
//! `Command::LabelSpaces` writes them as "Area 58.28 m²" (Arabic when asked, or
//! when the room's name is Arabic).

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Boundary, CoreError, EntityFilter, Outcome, Session, Source};

/// Most rooms one call looks for.
const MAX_ROOMS: usize = 500;
/// The gap between a room name and its label, in text heights.
const LABEL_GAP: f64 = 1.6;
/// A text's width is about this many text heights per character.
const CHAR_WIDTH: f64 = 0.6;
/// Areas are given to 6 decimals.
const DECIMALS: f64 = 1e6;

pub(crate) fn default_room_decimals() -> usize {
    2
}

/// One point to look round, with the room's name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomSeed {
    #[serde(default)]
    pub name: Option<String>,
    pub point: [f64; 2],
}

/// Where to look for rooms: `seeds`, or the room-name texts of the sheet on screen (on
/// `text_layer`, named in `names`, inside `within`), not both.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RoomsQuery {
    #[serde(default, flatten)]
    pub boundary_layers: crate::object_regions::BoundaryLayers,
    pub seeds: Option<Vec<RoomSeed>>,
    pub text_layer: Option<String>,
    pub names: Option<Vec<String>>,
    pub within: Option<[[f64; 2]; 2]>,
    /// Take islands (a fountain, a shaft) off the net area: true when left out.
    pub subtract_islands: Option<bool>,
}

/// The language of the labels `LabelSpaces` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LabelLanguage {
    Ar,
    En,
}

/// An island inside a room.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IslandRow {
    pub area_m2: f64,
    pub bounds: [[f64; 2]; 2],
}

/// One room found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomRow {
    pub name: Option<String>,
    /// The net candidate area (compatibility name; `measurement_basis` says what it is).
    pub net_area_m2: f64,
    pub candidate_area_m2: f64,
    pub area_origin: &'static str,
    pub region_method: &'static str,
    pub measurement_basis: &'static str,
    pub room_type_status: &'static str,
    pub net_accuracy_eligible: bool,
    pub enclosed_area_m2: f64,
    pub islands: Vec<IslandRow>,
    /// Where the region was found from.
    pub point: [f64; 2],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub also_named: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_point: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_handle: Option<u64>,
    /// Persistent closed boundary used by labels, schedules and take-off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundary_handle: Option<u64>,
}

/// A seed that found no room, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkippedRoom {
    pub text: String,
    pub point: [f64; 2],
    pub reason: &'static str,
}

/// What `rooms` found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomsReport {
    pub rooms: Vec<RoomRow>,
    pub skipped: Vec<SkippedRoom>,
    pub units: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels_written: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_created: Option<bool>,
}

/// One space found, in drawing units.
struct Room {
    name: Option<String>,
    /// The text it was found from: its height and where it starts.
    from_text: Option<(f64, [f64; 2])>,
    point: [f64; 2],
    gross: f64,
    islands: Vec<(f64, [[f64; 2]; 2])>,
    also_named: Vec<String>,
    first: [f64; 2],
    boundary: Boundary,
}

pub(crate) struct RoomLabelDefault {
    pub name: Option<String>,
    pub height_metres: f64,
    pub language: LabelLanguage,
}

impl Room {
    fn net(&self) -> f64 {
        self.gross - self.islands.iter().map(|i| i.0).sum::<f64>()
    }
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
pub(crate) fn ring_area(ring: &[[f64; 2]], bulges: &[f64]) -> f64 {
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

/// A text that is an area or a number, not a room's name.
pub(crate) fn looks_like_a_label(text: &str) -> bool {
    let t = text.trim();
    let lower = t.to_lowercase();
    t.parse::<f64>().is_ok()
        || lower.ends_with("m²")
        || lower.ends_with("m2")
        || t.ends_with("م²")
        || t.ends_with("م2")
}

fn bounds_of(points: &[[f64; 2]]) -> [[f64; 2]; 2] {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        b[0][0] = b[0][0].min(p[0]);
        b[0][1] = b[0][1].min(p[1]);
        b[1][0] = b[1][0].max(p[0]);
        b[1][1] = b[1][1].max(p[1]);
    }
    b
}

fn rounded(x: f64) -> f64 {
    let r = (x * DECIMALS).round() / DECIMALS;
    if r.is_finite() && x.abs() < 1e9 {
        r
    } else {
        x
    }
}

impl Session {
    /// 2.5 mm on paper at the annotation scale, in drawing units.
    pub fn default_text_height(&self) -> f64 {
        let scale = if self.sheet.eq_ignore_ascii_case("*Model_Space") {
            self.annotation_scale().1
        } else {
            1.0
        };
        2.5 * self.paper_unit() * scale
    }

    /// `Command::Rooms`: the rooms round the seeds or room names `q` gives.
    pub fn rooms(&self, q: &RoomsQuery) -> Result<RoomsReport, CoreError> {
        let (rooms, skipped) = self.find_rooms(q)?;
        Ok(self.report(&rooms, skipped, None))
    }

    fn report(
        &self,
        rooms: &[Room],
        skipped: Vec<SkippedRoom>,
        labels: Option<(&[[f64; 2]], &[u64])>,
    ) -> RoomsReport {
        let to_m = self.drawing.units.to_metres();
        let sq = to_m * to_m;
        let rows = rooms
            .iter()
            .enumerate()
            .map(|(i, room)| RoomRow {
                name: room.name.clone(),
                net_area_m2: rounded(room.net() * sq),
                candidate_area_m2: rounded(room.net() * sq),
                area_origin: "manual_or_entity",
                region_method: "enclosed_drawn_linework",
                measurement_basis: "unapproved_net",
                room_type_status: "unapproved",
                net_accuracy_eligible: false,
                enclosed_area_m2: rounded(room.gross * sq),
                islands: room
                    .islands
                    .iter()
                    .map(|(area, bounds)| IslandRow {
                        area_m2: rounded(area * sq),
                        bounds: *bounds,
                    })
                    .collect(),
                point: room.point,
                also_named: room.also_named.clone(),
                label_point: labels.map(|(points, _)| points[i]),
                label_handle: labels.and_then(|(_, handles)| handles.get(i).copied()),
                boundary_handle: self.room_at(&self.sheet, room.point).map(|(h, _)| h),
            })
            .collect();
        RoomsReport {
            rooms: rows,
            skipped,
            units: self.info().units,
            labels_written: labels.map(|(_, handles)| handles.len()),
            layer: None,
            layer_created: None,
        }
    }

    fn room_label(
        &self,
        room: &Room,
        height: Option<f64>,
        language: Option<LabelLanguage>,
    ) -> (f64, [f64; 2], LabelLanguage) {
        let label_height = height.map(|m| m / self.drawing.units.to_metres());
        let (h, at) = match room.from_text {
            Some((h, at)) => (label_height.unwrap_or(h), [at[0], at[1] - LABEL_GAP * h]),
            None => (
                label_height.unwrap_or_else(|| self.default_text_height()),
                room.point,
            ),
        };
        let language = language.unwrap_or_else(|| {
            let arabic = room.name.as_deref().unwrap_or_default().chars().any(|c| {
                matches!(c, '\u{0600}'..='\u{06ff}' | '\u{0750}'..='\u{077f}' | '\u{08a0}'..='\u{08ff}')
            });
            if arabic { LabelLanguage::Ar } else { LabelLanguage::En }
        });
        (h, at, language)
    }

    /// Label conventions for each found room; height follows the input field's metre units.
    pub(crate) fn room_label_defaults(
        &self,
        query: &RoomsQuery,
    ) -> Result<Vec<RoomLabelDefault>, CoreError> {
        let (rooms, _) = self.find_rooms(query)?;
        Ok(rooms
            .iter()
            .map(|room| {
                let (height, _, language) = self.room_label(room, None, None);
                RoomLabelDefault {
                    name: room.name.clone(),
                    height_metres: height * self.drawing.units.to_metres(),
                    language,
                }
            })
            .collect())
    }

    /// `Command::LabelSpaces`: the rooms of `q`, each with its candidate area written as a text
    /// just under its name (or at its seed point), on `layer` (made when missing; the current
    /// layer when none), `height` metres tall (the name's height, else 2.5 mm at the annotation
    /// scale), to `decimals` places, in `language` (the name's when none). One undo step.
    pub(crate) fn label_spaces(
        &mut self,
        q: &RoomsQuery,
        layer: Option<String>,
        height: Option<f64>,
        decimals: Option<usize>,
        language: Option<LabelLanguage>,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        let decimals = decimals.unwrap_or(2);
        if decimals > 6 {
            return Err(invalid("decimals is 0 to 6"));
        }
        if height.is_some_and(|h| !(h > 0.0 && h.is_finite())) {
            return Err(invalid("The label height must be more than 0"));
        }
        let (rooms, skipped) = self.find_rooms(q)?;
        if rooms.is_empty() {
            return Err(invalid("Nothing was drawn"));
        }
        let layer = layer
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| self.current.layer.clone());
        if layer.contains('|') {
            return Err(invalid("A layer of an xref can't take new entities"));
        }
        let layer = self
            .drawing
            .doc
            .layers
            .get(&layer)
            .map(|l| l.name.clone())
            .unwrap_or(layer);
        let created = self.drawing.doc.layers.get(&layer).is_none();
        self.record_document_change(source, "label_spaces", true, true, |s| {
            if created {
                s.new_layer(&layer)?;
            }
            let previous = s.current.clone();
            let mut points = Vec::new();
            let mut handles = Vec::new();
            for room in &rooms {
                let boundary = s.keep_room(
                    room.name.clone(),
                    room.point,
                    &room.boundary,
                    q.subtract_islands.unwrap_or(true),
                    &q.boundary_layers,
                )?;
                let (h, at, language) = s.room_label(room, height, language);
                points.push(at);
                let template = s.room_label_template(boundary, language)?;
                let code = crate::room_objects::room_area_code(&[boundary], decimals);
                s.current.layer = layer.clone();
                let handle = s.add_text(at, h, 0.0, template.replace("{area}", &code), None)?;
                s.mark_room_label(handle, boundary, decimals);
                handles.push(handle);
            }
            s.current = previous;
            let mut report = s.report(&rooms, skipped, Some((&points, &handles)));
            report.layer = Some(layer);
            report.layer_created = Some(created);
            Ok(Outcome::Rooms {
                report: Box::new(report),
            })
        })
    }

    /// The seeds `q` gives: its points, else the room-name texts of the sheet.
    fn room_seeds(&self, q: &RoomsQuery) -> Result<(Vec<Seed>, Vec<SkippedRoom>), CoreError> {
        let mut skipped = Vec::new();
        if let Some(given) = &q.seeds {
            if q.text_layer.is_some() || q.names.is_some() || q.within.is_some() {
                return Err(invalid(
                    "Give seeds, or text_layer / names / within to take the room names from the drawing, not both",
                ));
            }
            let mut out = Vec::new();
            for s in given {
                if !s.point.iter().all(|c| c.is_finite()) {
                    return Err(invalid("A seed's point must have finite coordinates"));
                }
                out.push(Seed {
                    name: s.name.clone().filter(|n| !n.trim().is_empty()),
                    from_text: None,
                    tries: vec![s.point],
                });
            }
            return Ok((out, skipped));
        }
        let on_layer: Option<HashSet<u64>> = match &q.text_layer {
            Some(layer) => {
                let known = self.drawing.doc.layers.get(layer).is_some()
                    || self
                        .layers()
                        .iter()
                        .any(|l| l.name.eq_ignore_ascii_case(layer));
                if !known {
                    return Err(invalid(format!(
                        "No layer named {layer}; the layers list gives the names"
                    )));
                }
                let mut handles = HashSet::new();
                for kind in ["Text", "MText"] {
                    let page = self.find_entities(&EntityFilter {
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
        let names: Option<Vec<String>> = q
            .names
            .as_ref()
            .map(|n| n.iter().map(|s| s.trim().to_lowercase()).collect());
        let mut out = Vec::new();
        for row in self.read_text(None, q.within) {
            if row.xref.is_some()
                || row.inside_block.is_some()
                || self
                    .drawing
                    .doc
                    .get_entity(opencadcodec::Handle::new(row.handle))
                    .is_some_and(crate::draw::room_boundary::is_grid)
            {
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
                skipped.push(SkippedRoom {
                    text,
                    point: row.at,
                    reason: "looks like an area or a number, not a room name",
                });
                continue;
            }
            let h = if row.height > 0.0 {
                row.height
            } else {
                self.default_text_height()
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
                return Err(invalid(format!(
                    "More than {MAX_ROOMS} texts: narrow them with text_layer, names or within"
                )));
            }
        }
        Ok((out, skipped))
    }

    /// The spaces round the seeds, and the seeds that found none.
    fn find_rooms(&self, q: &RoomsQuery) -> Result<(Vec<Room>, Vec<SkippedRoom>), CoreError> {
        let (seeds, mut skipped) = self.room_seeds(q)?;
        if seeds.is_empty() {
            return Err(invalid(
                "No room names found to start from: give seeds, or check text_layer and names (the texts list shows them)",
            ));
        }
        let subtract = q.subtract_islands.unwrap_or(true);
        let mut rooms: Vec<Room> = Vec::new();
        for seed in seeds {
            let mut found = None;
            let mut dense = false;
            for &p in &seed.tries {
                match self.room_boundary_in(&self.sheet, p, &q.boundary_layers) {
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
                skipped.push(SkippedRoom {
                    text: label,
                    point: seed.tries[0],
                    reason: if dense {
                        "too much line work round it to look through"
                    } else {
                        "it sits in no closed space"
                    },
                });
                continue;
            };
            let gross = ring_area(&ring, bulges.first().map_or(&[][..], Vec::as_slice));
            let islands: Vec<(f64, [[f64; 2]; 2])> = holes
                .iter()
                .enumerate()
                .map(|(i, hole)| {
                    (
                        ring_area(hole, bulges.get(i + 1).map_or(&[][..], Vec::as_slice)),
                        bounds_of(hole),
                    )
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
                boundary: Boundary::Ring {
                    ring,
                    holes,
                    bulges,
                },
            });
        }
        Ok((rooms, skipped))
    }
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}
