//! Ordered derivation within the command's existing entity recorder and table snapshot.
use crate::{CoreError, Session};

type Pass = fn(&mut Session) -> Result<(), CoreError>;
const PASSES: &[Pass] = &[objects, views, schedules, boq_rows, fields];
fn objects(s: &mut Session) -> Result<(), CoreError> {
    s.migrate_legacy_objects()?;
    s.refresh_objects()?;
    if s.has_object_regions() {
        // Enclosures follow the regenerated source geometry, then their own recipes follow
        // the new regions. Both passes remain inside the command's existing undo snapshot.
        s.park();
        s.refresh_object_regions()?;
        s.sync_legacy_room_regions()?;
        s.refresh_objects()?;
    }
    s.sync_legacy_wall_tags()
}
fn views(_: &mut Session) -> Result<(), CoreError> {
    Ok(())
}
fn schedules(s: &mut Session) -> Result<(), CoreError> {
    s.refresh_opening_schedules()?;
    s.migrate_object_schedules()?;
    s.refresh_object_schedules()
}
fn boq_rows(s: &mut Session) -> Result<(), CoreError> {
    s.refresh_measurement()
}
fn fields(s: &mut Session) -> Result<(), CoreError> {
    s.follow_fields()
}
impl Session {
    pub(crate) fn derived_extras(&self) -> (bool, bool) {
        let exists = crate::knowledge::get(
            &self.drawing.doc,
            crate::knowledge::Kind::Measurement,
            "selection",
        )
        .is_ok_and(|entry| entry.is_some())
            || self.has_rooms()
            || self.has_object_regions()
            || self.has_opening_schedules()
            || self
                .drawing
                .doc
                .app_ids
                .contains(crate::objects::OBJECT_APP)
            || self.drawing.doc.app_ids.contains(crate::WALL_APP);
        (exists, exists)
    }
    pub(crate) fn rederive(&mut self) -> Result<(), CoreError> {
        for pass in PASSES {
            pass(self)?;
        }
        Ok(())
    }
}
