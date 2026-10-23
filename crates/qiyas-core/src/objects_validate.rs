//! Damaged object metadata must be reported before it can disappear from quantities or make
//! a rebuild erase its graphics. Loading keeps the drawing available for explicit repair.

use opencadcodec::entities::EntityCommon;
use opencadcodec::xdata::XDataValue;
use serde::de::DeserializeOwned;

use crate::object_regions::{RetainedRegion, REGION_APP};
use crate::objects::{
    ObjectInstance, ObjectPart, ObjectSchedule, OBJECT_APP, PART_APP, SCHEDULE_APP,
};
use crate::{CoreError, Session};

fn metadata<T: DeserializeOwned>(common: &EntityCommon, app: &str) -> Result<Option<T>, CoreError> {
    let Some(record) = common.extended_data.get_record(app) else {
        return Ok(None);
    };
    let invalid = |reason: &str| {
        CoreError::InvalidCommand(format!(
            "Invalid {app} metadata on entity {}: {reason}",
            common.handle.value()
        ))
    };
    let mut text = String::new();
    for value in &record.values {
        let XDataValue::String(chunk) = value else {
            return Err(invalid("expected JSON string chunks"));
        };
        text.push_str(chunk);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| invalid(&error.to_string()))
}

impl Session {
    pub(crate) fn validate_object_metadata(&self) -> Result<(), CoreError> {
        if ![
            OBJECT_APP,
            PART_APP,
            SCHEDULE_APP,
            REGION_APP,
            crate::recognition::APP,
        ]
        .iter()
        .any(|app| self.drawing.doc.app_ids.contains(app))
        {
            return Ok(());
        }
        for entity in self.drawing.doc.entities() {
            let common = entity.common();
            metadata::<crate::recognition::Candidate>(common, crate::recognition::APP)?;
            metadata::<ObjectInstance>(common, OBJECT_APP)?;
            metadata::<ObjectPart>(common, PART_APP)?;
            metadata::<ObjectSchedule>(common, SCHEDULE_APP)?;
            if let Some(region) = metadata::<RetainedRegion>(common, REGION_APP)? {
                if !crate::object_regions::valid_region_metadata(&region) {
                    return Err(CoreError::InvalidCommand(format!(
                        "Invalid {REGION_APP} geometry on entity {}",
                        common.handle.value(),
                    )));
                }
            }
        }
        Ok(())
    }
}
