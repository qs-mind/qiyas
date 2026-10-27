//! Compatibility commands and links for retained rooms. Geometry, recipes, quantities and
//! schedules use the shared object engines; old metadata remains readable for existing labels.

use crate::xdata::read_json as get_data;
use opencadcodec::entities::EntityCommon;
use opencadcodec::{AppId, EntityType, Handle};
use serde::{Deserialize, Serialize};

use crate::check::{Finding, Severity};
use crate::object_regions::{region_polyline as polyline, retained_region, RetainedRegion};
use crate::{Boundary, CoreError, Kind, Measurement, Session, Target};

pub(crate) const ROOM_APP: &str = "QIYAS_ROOM";
const LABEL_APP: &str = "QIYAS_ROOM_LABEL";
const SCHEDULE_APP: &str = "QIYAS_ROOM_SCHEDULE";
const BOUNDARY_LAYER: &str = "A-ROOM-BOUNDARY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RoomObject {
    pub(crate) holder: u64,
    pub name: Option<String>,
    pub seed: [f64; 2],
    subtract_islands: bool,
    holes: Vec<Vec<[f64; 2]>>,
    hole_bulges: Vec<Vec<f64>>,
    pub(crate) valid: bool,
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

fn set_data(common: &mut EntityCommon, app: &str, app_handle: u64, value: &impl Serialize) {
    crate::xdata::write_json(common, app, app_handle, value).expect("finite room metadata");
}

pub(crate) fn is_room_boundary(entity: &EntityType) -> bool {
    room_object(entity).is_some()
}

pub(crate) fn room_object(entity: &EntityType) -> Option<RoomObject> {
    let room: RoomObject = get_data(entity.common(), ROOM_APP)?;
    (room.holder == entity.common().handle.value()).then_some(room)
}

/// The canonical room area, in drawing units squared, including its island policy.
pub(crate) fn room_area(entity: &EntityType) -> Option<f64> {
    crate::object_regions::retained_region_area(entity)
}

pub(crate) fn room_area_code(handles: &[u64], decimals: usize) -> String {
    crate::object_schedules::object_value_code(handles, "net_floor_area", decimals, false)
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

    /// Convert only the former room API's identity and source. Geometry and quantities then
    /// follow the same engine as every other region type.
    pub(crate) fn migrate_legacy_rooms(&mut self) -> Result<(), CoreError> {
        if !self.has_rooms() {
            return Ok(());
        }
        self.validate_object_metadata()?;
        let rooms: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                room_object(entity).map(|room| {
                    (
                        entity.common().handle.value(),
                        room,
                        retained_region(entity),
                        crate::objects::instance(entity),
                    )
                })
            })
            .collect();
        if rooms.is_empty() {
            return Ok(());
        }
        let definition = self.ensure_legacy_type("room")?;
        for (handle, room, region, old) in rooms {
            if region.is_none() {
                self.stamp_retained_region(
                    handle,
                    &RetainedRegion {
                        seed: room.seed,
                        boundary_layers: Default::default(),
                        subtract_islands: room.subtract_islands,
                        holes: room.holes,
                        hole_bulges: room.hole_bulges,
                        valid: room.valid,
                        app_handle: 0,
                    },
                )?;
            }
            let mut object = old.unwrap_or_else(|| crate::objects::ObjectInstance {
                id: handle,
                type_name: definition.name.clone(),
                props: std::collections::BTreeMap::from([(
                    "name".into(),
                    serde_json::json!(room.name.unwrap_or_default()),
                )]),
                level: None,
                host: None,
                assembly: None,
                layers: std::collections::BTreeMap::new(),
                compatibility: None,
            });
            object.id = handle;
            object.compatibility = None;
            object.props = self
                .object_type(&object.type_name)?
                .properties(&object.props)?;
            self.stamp_object(handle, &object)?;
        }
        self.sync_legacy_room_regions()
    }

    /// Keep old label and checking links readable without letting their metadata drive geometry.
    pub(crate) fn sync_legacy_room_regions(&mut self) -> Result<(), CoreError> {
        let rooms: Vec<_> = self
            .drawing
            .doc
            .entities()
            .filter_map(|entity| {
                let mut room: RoomObject = get_data(entity.common(), ROOM_APP)?;
                let region = retained_region(entity)?;
                let object = crate::objects::instance(entity)?;
                room.holder = entity.common().handle.value();
                room.seed = region.seed;
                room.subtract_islands = region.subtract_islands;
                room.holes = region.holes;
                room.hole_bulges = region.hole_bulges;
                room.valid = region.valid;
                if let Some(name) = object.props.get("name").and_then(serde_json::Value::as_str) {
                    room.name = Some(name.into());
                }
                Some((room.holder, room))
            })
            .collect();
        if rooms.is_empty() {
            return Ok(());
        }
        let app = self.register_room_app(ROOM_APP);
        for (handle, room) in rooms {
            crate::xdata::write_json(
                self.drawing
                    .doc
                    .get_entity_mut(Handle::new(handle))
                    .expect("listed room")
                    .common_mut(),
                ROOM_APP,
                app,
                &room,
            )?;
        }
        Ok(())
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

    /// The retained room on `sheet` whose boundary is exactly `ring` (the same corners).
    fn room_with_ring(&self, sheet: &str, ring: &[[f64; 2]]) -> Option<u64> {
        let tol = self.measure_tol() * 100.0;
        self.drawing
            .doc
            .entities_in_block(sheet)
            .find_map(|entity| {
                room_object(entity)?;
                let EntityType::LwPolyline(p) = entity else {
                    return None;
                };
                let same = p.vertices.len() == ring.len()
                    && ring.iter().all(|c| {
                        p.vertices
                            .iter()
                            .any(|v| (v.location.x - c[0]).hypot(v.location.y - c[1]) <= tol)
                    });
                same.then(|| entity.common().handle.value())
            })
    }

    /// Keeps one actual region as a reusable object; repeated labelling reuses its handle.
    pub(crate) fn keep_room(
        &mut self,
        name: Option<String>,
        seed: [f64; 2],
        boundary: &Boundary,
        subtract_islands: bool,
        policy: &crate::object_regions::BoundaryLayers,
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
            if old.subtract_islands != subtract_islands
                || self
                    .drawing
                    .doc
                    .get_entity(Handle::new(h))
                    .and_then(retained_region)
                    .is_some_and(|r| r.boundary_layers != *policy)
            {
                return Err(invalid("This room already has a boundary or island policy; use its existing policy consistently"));
            }
            return Ok(h);
        }
        // The same enclosure seeded elsewhere (another name, another point) is the same room:
        // one retained boundary per region, never two over each other.
        if let Some(h) = self.room_with_ring(&self.sheet, ring) {
            return Ok(h);
        }
        if !self.drawing.doc.layers.contains(BOUNDARY_LAYER) {
            self.new_layer(BOUNDARY_LAYER)?;
        }
        // Retained boundaries are records, not drawing: they never plot, even when the layer
        // was made beforehand.
        self.drawing
            .doc
            .layers
            .get_mut(BOUNDARY_LAYER)
            .expect("boundary layer")
            .is_plottable = false;
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
        self.migrate_legacy_rooms()?;
        let mut region = retained_region(self.drawing.doc.get_entity(h).expect("new room"))
            .expect("migrated room");
        region.boundary_layers = policy.clone();
        self.stamp_retained_region(h.value(), &region)?;
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
        self.object_field_value(handles, "net_floor_area", decimals)
    }

    pub(crate) fn room_label_template(
        &self,
        boundary: u64,
        language: crate::rooms::LabelLanguage,
    ) -> Result<String, CoreError> {
        let property = match language {
            crate::rooms::LabelLanguage::En => "label_en",
            crate::rooms::LabelLanguage::Ar => "label_ar",
        };
        if let Some(template) = self
            .drawing
            .doc
            .get_entity(Handle::new(boundary))
            .and_then(crate::objects::instance)
            .and_then(|object| {
                object
                    .props
                    .get(property)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
        {
            return Ok(template
                .replace("Unapproved candidate", "Area")
                .replace("مساحة مرشحة غير معتمدة", "المساحة"));
        }
        // Older stored definitions lack the annotation defaults; the alias reads the same
        // compatibility data rather than introducing a second rendering rule.
        let defaults: serde_json::Value =
            serde_json::from_str(include_str!("../data/legacy-object-types.json"))
                .map_err(|error| invalid(error.to_string()))?;
        defaults["room"]["properties"][property]["default"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| invalid("Room label template is missing from its type data"))
    }

    pub(crate) fn room_schedule(
        &mut self,
        at: [f64; 2],
        source_sheet: Option<String>,
        labels: &[Target],
        decimals: usize,
        language: Option<crate::rooms::LabelLanguage>,
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
        let mut spec = crate::object_schedules::legacy_recipe("room")?;
        if language.or(self.sheet_language()) == Some(crate::rooms::LabelLanguage::Ar) {
            spec.headers = vec!["الغرفة".into(), "المساحة (م²)".into()];
            spec.totals.insert("name".into(), "إجمالي المساحة".into());
            spec.language = Some("ar".into());
        }
        spec.at = at;
        spec.ids = Some(rooms.clone());
        spec.decimals.insert("net_floor_area".into(), decimals);
        let handle = self.add_object_schedule(spec)?;
        // Keep the old link metadata so existing discrepancy checks read the same cells.
        let app_handle = self.register_room_app(SCHEDULE_APP);
        set_data(
            self.drawing
                .doc
                .get_entity_mut(Handle::new(handle))
                .expect("added schedule")
                .common_mut(),
            SCHEDULE_APP,
            app_handle,
            &RoomSchedule { rooms, decimals },
        );
        Ok(handle)
    }

    pub(crate) fn room_from_label(&self, label: &Target) -> Result<u64, CoreError> {
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
        policy: &crate::object_regions::BoundaryLayers,
    ) -> Result<Measurement, CoreError> {
        let boundary = match (label, point) {
            (Some(label), None) => self.room_from_label(&label)?,
            (None, Some(point)) if point.iter().all(|v| v.is_finite()) => {
                if let Some((h, _)) = self.room_at(&self.sheet, point) {
                    h
                } else {
                    let boundary = self.room_boundary_in(&self.sheet, point, policy);
                    self.keep_room(None, point, &boundary, true, policy)?
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
        if (!policy.boundary_layers.is_empty() || !policy.exclude_layers.is_empty())
            && retained_region(entity).is_some_and(|region| region.boundary_layers != *policy)
        {
            return Err(invalid(
                "The selected room has a different saved boundary policy",
            ));
        }
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

    pub(crate) fn check_room_areas(
        &self,
        sheet: &str,
        active: &impl Fn(u64) -> bool,
        push: &mut impl FnMut(Finding),
    ) {
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
            if !active(entity.common().handle.value())
                && !get_data::<RoomLabel>(entity.common(), LABEL_APP)
                    .is_some_and(|label| active(label.boundary))
                && !get_data::<RoomSchedule>(entity.common(), SCHEDULE_APP)
                    .is_some_and(|schedule| schedule.rooms.iter().any(|id| active(*id)))
            {
                continue;
            }

            if let Some(label) = get_data::<RoomLabel>(entity.common(), LABEL_APP) {
                let text = match entity {
                    EntityType::Text(t) => t.value.clone(),
                    EntityType::MText(t) => {
                        opencadcodec::entities::mtext_format::parse_mtext(&t.value, true)
                            .to_plain_text()
                    }
                    _ => continue,
                };
                let expected = self.room_field_value(&[label.boundary], label.decimals);
                let actual = numeric_words(&text).last().copied();
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
                        Some((t.value.clone(), [t.insertion_point.x, t.insertion_point.y]))
                    }
                    EntityType::MText(t) => Some((
                        opencadcodec::entities::mtext_format::parse_mtext(&t.value, true)
                            .to_plain_text(),
                        [t.insertion_point.x, t.insertion_point.y],
                    )),
                    _ => None,
                };
                if let Some((text, at)) =
                    literal.filter(|(text, _)| crate::numbers::area_text(text).is_some())
                {
                    if let Some((boundary, _)) = self.room_at(sheet, at) {
                        let expected = self.room_field_value(&[boundary], 2);
                        if !same_number(
                            numeric_words(&text).last().copied(),
                            expected.as_deref(),
                            2,
                        ) {
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
                        .and_then(crate::numbers::parse);
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
    crate::numbers::words(crate::numbers::area_text(text).unwrap_or(text))
}

fn same_number(actual: Option<f64>, expected: Option<&str>, decimals: usize) -> bool {
    actual
        .zip(expected.and_then(crate::numbers::parse))
        .is_some_and(|(a, b)| (a - b).abs() < 0.5 * 10f64.powi(-(decimals as i32)))
}
