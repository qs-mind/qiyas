//! Persistent room boundaries and all the values derived from them. The outline is an ordinary
//! closed polyline; its seed, islands and links are xdata, preserved by both DWG and DXF.

use opencadcodec::entities::EntityCommon;
use opencadcodec::xdata::{ExtendedDataRecord, XDataValue};
use opencadcodec::{AppId, EntityType, Handle, LwPolyline, Vector2};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::check::{Finding, Severity};
use crate::rooms::ring_area;
use crate::{Boundary, CoreError, Kind, Measurement, Session, Target};

pub(crate) const ROOM_APP: &str = "QIYAS_ROOM";
const LABEL_APP: &str = "QIYAS_ROOM_LABEL";
const SCHEDULE_APP: &str = "QIYAS_ROOM_SCHEDULE";
const BOUNDARY_LAYER: &str = "A-ROOM-BOUNDARY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RoomObject {
    holder: u64,
    pub name: Option<String>,
    pub seed: [f64; 2],
    subtract_islands: bool,
    holes: Vec<Vec<[f64; 2]>>,
    hole_bulges: Vec<Vec<f64>>,
    valid: bool,
}

#[derive(Serialize, Deserialize)]
struct RoomLabel {
    boundary: u64,
    decimals: usize,
}

#[derive(Serialize, Deserialize)]
struct RoomSchedule {
    rooms: Vec<u64>,
    decimals: usize,
}

fn invalid(why: impl Into<String>) -> CoreError {
    CoreError::InvalidCommand(why.into())
}

fn get_data<T: DeserializeOwned>(common: &EntityCommon, app: &str) -> Option<T> {
    let text: String = common
        .extended_data
        .get_record(app)?
        .values
        .iter()
        .filter_map(|v| {
            if let XDataValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    serde_json::from_str(&text).ok()
}

fn set_data(common: &mut EntityCommon, app: &str, app_handle: u64, value: &impl Serialize) {
    let text = serde_json::to_string(value).expect("finite room metadata");
    let mut record = ExtendedDataRecord::new(app);
    for chunk in crate::field::chunks(&text) {
        record.add_value(chunk);
    }
    if common.extended_data.get_record(app) != Some(&record) {
        common.raw_record = None;
        // DWG prefers its original raw EED blob; discard only this application's stale copy.
        common
            .extended_data
            .raw_dwg_eed
            .retain(|(handle, _)| *handle != app_handle);
        common.extended_data.upsert_record(record);
    }
}

pub(crate) fn is_room_boundary(entity: &EntityType) -> bool {
    room_object(entity).is_some()
}

fn room_object(entity: &EntityType) -> Option<RoomObject> {
    let room: RoomObject = get_data(entity.common(), ROOM_APP)?;
    (room.holder == entity.common().handle.value()).then_some(room)
}

fn polyline(ring: &[[f64; 2]], bulges: &[f64]) -> LwPolyline {
    let mut polyline = LwPolyline::new();
    polyline.is_closed = true;
    for (i, p) in ring.iter().enumerate() {
        polyline.add_point_with_bulge(
            Vector2::new(p[0], p[1]),
            bulges.get(i).copied().unwrap_or(0.0),
        );
    }
    polyline
}

/// The canonical room area, in drawing units squared, including its island policy.
pub(crate) fn room_area(entity: &EntityType) -> Option<f64> {
    let room = room_object(entity)?;
    if !room.valid {
        return None;
    }
    let EntityType::LwPolyline(p) = entity else {
        return None;
    };
    if !p.is_closed {
        return None;
    }
    let ring: Vec<_> = p
        .vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect();
    let bulges: Vec<_> = p.vertices.iter().map(|v| v.bulge).collect();
    let area = ring_area(&ring, &bulges);
    let islands = if room.subtract_islands {
        room.holes
            .iter()
            .enumerate()
            .map(|(i, hole)| ring_area(hole, room.hole_bulges.get(i).map_or(&[], Vec::as_slice)))
            .sum::<f64>()
    } else {
        0.0
    };
    Some((area - islands).max(0.0))
}

pub(crate) fn room_area_code(handles: &[u64], decimals: usize) -> String {
    let ids = handles
        .iter()
        .map(|h| format!("{h:X}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"%<\AcVar QiyasRoomArea "{ids}" {decimals}>%"#)
}

impl Session {
    fn register_room_app(&mut self, name: &str) -> u64 {
        if !self.drawing.doc.app_ids.contains(name) {
            let mut app = AppId::new(name);
            app.handle = self.drawing.doc.allocate_handle();
            self.drawing.doc.app_ids.add(app).expect("new application");
        }
        self.drawing
            .doc
            .app_ids
            .get(name)
            .expect("registered application")
            .handle
            .value()
    }

    pub(crate) fn has_rooms(&self) -> bool {
        self.drawing.doc.app_ids.contains(ROOM_APP)
    }

    /// The persistent room containing a seed. Smallest first, with the same island policy.
    pub(crate) fn room_at(&self, sheet: &str, point: [f64; 2]) -> Option<(u64, RoomObject)> {
        self.drawing
            .doc
            .entities_in_block(sheet)
            .filter_map(|entity| {
                let room = room_object(entity)?;
                let EntityType::LwPolyline(p) = entity else {
                    return None;
                };
                let ring = qiyas_geom::Path {
                    points: p
                        .vertices
                        .iter()
                        .map(|v| [v.location.x, v.location.y])
                        .collect(),
                    closed: true,
                };
                // The seed is retained even when its former enclosure has been opened.
                let seed_matches =
                    (room.seed[0] - point[0]).hypot(room.seed[1] - point[1]) < self.measure_tol();
                if !seed_matches
                    && (!inside(&ring.points, point)
                        || room.holes.iter().any(|hole| inside(hole, point)))
                {
                    return None;
                }
                Some((ring.area(), entity.common().handle.value(), room))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, h, room)| (h, room))
    }

    /// Keeps one actual region as a reusable object; repeated labelling reuses its handle.
    pub(crate) fn keep_room(
        &mut self,
        name: Option<String>,
        seed: [f64; 2],
        boundary: &Boundary,
        subtract_islands: bool,
    ) -> Result<u64, CoreError> {
        let Boundary::Ring {
            ring,
            holes,
            bulges,
        } = boundary
        else {
            return Err(invalid("The room seed is not inside a closed drawn region"));
        };
        if let Some((h, old)) = self.room_at(&self.sheet, seed) {
            if old.subtract_islands != subtract_islands {
                return Err(invalid("This room already has an island policy; use its existing boundary consistently"));
            }
            return Ok(h);
        }
        if !self.drawing.doc.layers.contains(BOUNDARY_LAYER) {
            self.new_layer(BOUNDARY_LAYER)?;
            self.drawing
                .doc
                .layers
                .get_mut(BOUNDARY_LAYER)
                .expect("created layer")
                .is_plottable = false;
        }
        let app_handle = self.register_room_app(ROOM_APP);
        let mut p = polyline(ring, bulges.first().map_or(&[], Vec::as_slice));
        p.common.layer = BOUNDARY_LAYER.to_string();
        p.common.owner_handle = self
            .sheet_owner()
            .ok_or_else(|| invalid("Choose a drawing sheet first"))?;
        let h = self
            .drawing
            .doc
            .add_entity(EntityType::LwPolyline(p))
            .map_err(|e| invalid(e.to_string()))?;
        let room = RoomObject {
            holder: h.value(),
            name,
            seed,
            subtract_islands,
            holes: holes.clone(),
            hole_bulges: bulges.iter().skip(1).cloned().collect(),
            valid: true,
        };
        set_data(
            self.drawing
                .doc
                .get_entity_mut(h)
                .expect("added boundary")
                .common_mut(),
            ROOM_APP,
            app_handle,
            &room,
        );
        Ok(h.value())
    }

    pub(crate) fn mark_room_label(&mut self, label: u64, boundary: u64, decimals: usize) {
        let app_handle = self.register_room_app(LABEL_APP);
        set_data(
            self.drawing
                .doc
                .get_entity_mut(Handle::new(label))
                .expect("added label")
                .common_mut(),
            LABEL_APP,
            app_handle,
            &RoomLabel { boundary, decimals },
        );
    }

    pub(crate) fn room_field_value(&self, handles: &[u64], decimals: usize) -> Option<String> {
        let area: f64 = handles
            .iter()
            .map(|h| room_area(self.drawing.doc.get_entity(Handle::new(*h))?))
            .sum::<Option<f64>>()?;
        let area = area * self.drawing.units.to_metres().powi(2);
        // Match Rooms' six-place report before applying the label's displayed precision.
        Some(format!(
            "{:.*}",
            decimals.min(6),
            (area * 1e6).round() / 1e6
        ))
    }

    /// Follows changed wall geometry while the command's undo recording is still open.
    pub(crate) fn refresh_rooms(&mut self) -> Result<(), CoreError> {
        if !self.has_rooms() {
            return Ok(());
        }
        let rooms: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let room = room_object(entity)?;
                let sheet = self
                    .drawing
                    .doc
                    .block_records
                    .iter()
                    .find(|b| b.handle == entity.common().owner_handle)?
                    .name
                    .clone();
                Some((entity.common().handle, room, sheet))
            })
            .collect();
        let start = 1.0 / self.drawing.units.to_metres();
        let app_handle = self.register_room_app(ROOM_APP);
        for (handle, mut room, sheet) in rooms {
            let found = self.boundary_at_in(&sheet, room.seed, start);
            match found {
                Boundary::Ring {
                    ring,
                    holes,
                    bulges,
                } => {
                    room.holes = holes;
                    room.hole_bulges = bulges.iter().skip(1).cloned().collect();
                    room.valid = true;
                    let mut replacement =
                        polyline(&ring, bulges.first().map_or(&[], Vec::as_slice));
                    replacement.common = self
                        .drawing
                        .doc
                        .get_entity(handle)
                        .expect("listed boundary")
                        .common()
                        .clone();
                    // A cloned common can restore the original whole DWG record after
                    // get_entity_mut clears it. The replacement geometry must be encoded.
                    replacement.common.raw_record = None;
                    replacement.common.graphic_data = None;
                    set_data(&mut replacement.common, ROOM_APP, app_handle, &room);
                    let entity = EntityType::LwPolyline(replacement);
                    let before = self
                        .drawing
                        .doc
                        .get_entity(handle)
                        .expect("listed boundary");
                    if serde_json::to_value(before).ok() != serde_json::to_value(&entity).ok() {
                        *self
                            .drawing
                            .doc
                            .get_entity_mut(handle)
                            .expect("listed boundary") = entity;
                    }
                }
                Boundary::None | Boundary::TooDense => {
                    if room.valid {
                        room.valid = false;
                        set_data(
                            self.drawing
                                .doc
                                .get_entity_mut(handle)
                                .expect("listed boundary")
                                .common_mut(),
                            ROOM_APP,
                            app_handle,
                            &room,
                        );
                    }
                }
            }
        }
        let quantities: Vec<_> = self
            .takeoff
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                if row.kind != Kind::Area || row.handles.len() != 1 {
                    return None;
                }
                let entity = self.drawing.doc.get_entity(Handle::new(row.handles[0]))?;
                if !is_room_boundary(entity) {
                    return None;
                }
                Some((i, room_area(entity)))
            })
            .collect();
        for (i, area) in quantities {
            let row = &mut self.takeoff.items[i];
            if let Some(area) = area {
                row.value = area;
                row.shown = area * self.drawing.units.to_metres().powi(2);
                row.note =
                    "Unapproved candidate room area; linked to the retained boundary".to_string();
            } else {
                row.note =
                    "Room boundary is no longer closed; quantity is stale and must not be used"
                        .to_string();
            }
        }
        self.follow_fields()
    }

    pub(crate) fn room_schedule(
        &mut self,
        at: [f64; 2],
        source_sheet: Option<String>,
        labels: &[Target],
        decimals: usize,
    ) -> Result<u64, CoreError> {
        if decimals > 6 {
            return Err(invalid("decimals is 0 to 6"));
        }
        let sheet = self.sheet_block(Some(source_sheet.as_deref().unwrap_or("*Model_Space")));
        let mut rooms: Vec<u64> = if labels.is_empty() {
            self.drawing
                .doc
                .entities_in_block(&sheet)
                .filter(|e| is_room_boundary(e))
                .map(|e| e.common().handle.value())
                .collect()
        } else {
            labels
                .iter()
                .map(|target| self.room_from_label(target))
                .collect::<Result<_, _>>()?
        };
        rooms.sort_unstable();
        rooms.dedup();
        if rooms.is_empty() {
            return Err(invalid(
                "No retained rooms: run label_spaces first, or measure_room with a seed point",
            ));
        }
        let mut data = vec![vec!["Room".to_string(), "Candidate area (m2)".to_string()]];
        for h in &rooms {
            let entity = self
                .drawing
                .doc
                .get_entity(Handle::new(*h))
                .expect("resolved room");
            let room = room_object(entity)
                .ok_or_else(|| invalid("A selected room boundary no longer exists"))?;
            data.push(vec![
                room.name.unwrap_or_else(|| format!("Room {h:X}")),
                room_area_code(&[*h], decimals),
            ]);
        }
        data.push(vec![
            "Total candidate area".to_string(),
            room_area_code(&rooms, decimals),
        ]);
        let saved = self.current.clone();
        let result = (|| {
            if self.current.layer == "0" {
                if !self.drawing.doc.layers.contains("A-ANNO-TABL") {
                    self.new_layer("A-ANNO-TABL")?;
                }
                self.current.layer = "A-ANNO-TABL".to_string();
            }
            // Size from rendered values rather than the internal field code.
            let owner = self
                .sheet_owner()
                .ok_or_else(|| invalid("Choose a sheet first"))?;
            let shown = data
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| self.evaluate_fields(cell, owner))
                        .collect()
                })
                .collect::<Vec<Vec<_>>>();
            let (width, height) = self.table_sizes(0.0, 0.0, &shown);
            let h = self.add_table(at, data.len(), 2, width, height, &data)?;
            let app_handle = self.register_room_app(SCHEDULE_APP);
            set_data(
                self.drawing
                    .doc
                    .get_entity_mut(Handle::new(h))
                    .expect("added schedule")
                    .common_mut(),
                SCHEDULE_APP,
                app_handle,
                &RoomSchedule { rooms, decimals },
            );
            Ok(h)
        })();
        self.current = saved;
        result
    }

    fn room_from_label(&self, label: &Target) -> Result<u64, CoreError> {
        if !label.chain.is_empty() {
            return Err(invalid("Pick a room label at the top of its sheet"));
        }
        let entity = self
            .drawing
            .doc
            .get_entity(Handle::new(label.handle))
            .ok_or_else(|| invalid("The room label no longer exists"))?;
        if is_room_boundary(entity) {
            return Ok(label.handle);
        }
        let boundary = get_data::<RoomLabel>(entity.common(), LABEL_APP)
            .map(|r| r.boundary)
            .ok_or_else(|| {
                invalid("Pick a live room label made by label_spaces, or give a seed point")
            })?;
        if !self
            .drawing
            .doc
            .get_entity(Handle::new(boundary))
            .is_some_and(is_room_boundary)
        {
            return Err(invalid(
                "The room label's retained boundary no longer exists",
            ));
        }
        Ok(boundary)
    }

    pub(crate) fn measure_room(
        &mut self,
        label: Option<Target>,
        point: Option<[f64; 2]>,
    ) -> Result<Measurement, CoreError> {
        let boundary = match (label, point) {
            (Some(label), None) => self.room_from_label(&label)?,
            (None, Some(point)) if point.iter().all(|v| v.is_finite()) => {
                if let Some((h, _)) = self.room_at(&self.sheet, point) {
                    h
                } else {
                    let boundary = self.boundary_at(point, 1.0 / self.drawing.units.to_metres());
                    self.keep_room(None, point, &boundary, true)?
                }
            }
            _ => {
                return Err(invalid(
                    "Give exactly one of label: {handle: N} or point: [x, y]",
                ))
            }
        };
        let entity = self
            .drawing
            .doc
            .get_entity(Handle::new(boundary))
            .ok_or_else(|| invalid("The room boundary no longer exists"))?;
        let room = room_object(entity)
            .ok_or_else(|| invalid("The selected boundary is not a retained room"))?;
        let area = room_area(entity).ok_or_else(|| {
            invalid("The room is no longer closed: fix its walls before measuring")
        })?;
        let mut measurement = Measurement::new(
            Kind::Area,
            area,
            self.drawing.units,
            room.name.clone().unwrap_or_else(|| "Room".to_string()),
            entity.common().layer.clone(),
            self.sheet.clone(),
        );
        if let Some(sheet) = self
            .drawing
            .doc
            .block_records
            .iter()
            .find(|b| b.handle == entity.common().owner_handle)
        {
            measurement.sheet = sheet.name.clone();
        }
        measurement.handles = vec![boundary];
        measurement.measurement_basis = crate::MeasurementBasis::UnapprovedNet;
        measurement.area_origin = crate::AreaOrigin::ExistingBoundary;
        measurement.note =
            "Unapproved candidate room area; linked to the retained boundary".to_string();
        Ok(measurement)
    }

    pub(crate) fn check_room_areas(&self, sheet: &str, push: &mut impl FnMut(Finding)) {
        let mismatch = |handles: Vec<u64>, message: String, push: &mut dyn FnMut(Finding)| {
            push(Finding {
                rule: "room_area_mismatch",
                pack: "construction",
                severity: Severity::Warning,
                handles,
                at: None,
                message,
            });
        };
        for entity in self.drawing.doc.entities_in_block(sheet) {
            if let Some(label) = get_data::<RoomLabel>(entity.common(), LABEL_APP) {
                let text = match entity {
                    EntityType::Text(t) => t.value.as_str(),
                    EntityType::MText(t) => t.value.as_str(),
                    _ => continue,
                };
                let expected = self.room_field_value(&[label.boundary], label.decimals);
                let actual = numeric_words(text).last().copied();
                if !same_number(actual, expected.as_deref(), label.decimals) {
                    mismatch(
                        vec![entity.common().handle.value(), label.boundary],
                        format!(
                            "Room label area differs from its retained room: expected {} m2",
                            expected.unwrap_or_else(|| "a closed boundary".into())
                        ),
                        push,
                    );
                }
            }
            if get_data::<RoomLabel>(entity.common(), LABEL_APP).is_none() {
                let literal = match entity {
                    EntityType::Text(t) => {
                        Some((t.value.as_str(), [t.insertion_point.x, t.insertion_point.y]))
                    }
                    EntityType::MText(t) => {
                        Some((t.value.as_str(), [t.insertion_point.x, t.insertion_point.y]))
                    }
                    _ => None,
                };
                if let Some((text, at)) = literal.filter(|(text, _)| text.trim().ends_with("m²")) {
                    if let Some((boundary, _)) = self.room_at(sheet, at) {
                        let expected = self.room_field_value(&[boundary], 2);
                        if !same_number(numeric_words(text).last().copied(), expected.as_deref(), 2)
                        {
                            mismatch(vec![entity.common().handle.value(), boundary], "Literal room area label differs from its retained room; use label_spaces for a live field".into(), push);
                        }
                    }
                }
            }
            if let (EntityType::Table(table), Some(schedule)) = (
                entity,
                get_data::<RoomSchedule>(entity.common(), SCHEDULE_APP),
            ) {
                for index in 0..=schedule.rooms.len() {
                    let handles = if index == schedule.rooms.len() {
                        schedule.rooms.clone()
                    } else {
                        vec![schedule.rooms[index]]
                    };
                    let expected = self.room_field_value(&handles, schedule.decimals);
                    let actual = table
                        .cell_text(index + 1, 1)
                        .and_then(|s| s.parse::<f64>().ok());
                    if !same_number(actual, expected.as_deref(), schedule.decimals) {
                        let mut linked = vec![entity.common().handle.value()];
                        linked.extend(handles);
                        mismatch(
                            linked,
                            format!(
                                "Room schedule row {} area differs from its retained room",
                                index + 1
                            ),
                            push,
                        );
                    }
                }
            }
        }
        for row in self
            .takeoff
            .items
            .iter()
            .filter(|r| r.sheet.eq_ignore_ascii_case(sheet) && r.kind == Kind::Area)
        {
            let Some(entity) = row
                .handles
                .first()
                .and_then(|h| self.drawing.doc.get_entity(Handle::new(*h)))
            else {
                continue;
            };
            if !is_room_boundary(entity) {
                continue;
            }
            let expected = room_area(entity).map(|a| a * self.drawing.units.to_metres().powi(2));
            if expected.is_none_or(|area| (row.shown - area).abs() > 0.000001) {
                mismatch(
                    vec![entity.common().handle.value()],
                    format!(
                        "Take-off row {} area differs from its retained room",
                        row.id
                    ),
                    push,
                );
            }
        }
    }
}

fn inside(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn numeric_words(text: &str) -> Vec<f64> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn same_number(actual: Option<f64>, expected: Option<&str>, decimals: usize) -> bool {
    actual
        .zip(expected.and_then(|s| s.parse::<f64>().ok()))
        .is_some_and(|(a, b)| (a - b).abs() < 0.5 * 10f64.powi(-(decimals as i32)))
}
