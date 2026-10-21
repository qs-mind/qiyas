//! Today's automatic measurements. This module has no access to the gold quantities.
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use qiyas_core::{Command, EntityFilter, Outcome, Session};

use super::model::{Item, Submission};
use super::submission::from_measurement;

pub fn measure(drawing: &Path) -> Result<Submission, String> {
    let started = Instant::now();
    let mut session = Session::open_standalone(drawing).map_err(|e| e.to_string())?;
    // Embedded/manual take-off is not automatic baseline work.
    session.takeoff.items.clear();
    let factor = session.drawing.units.to_metres();
    let mut entities = Vec::new();
    let mut query = EntityFilter {
        limit: 500,
        ..Default::default()
    };
    loop {
        let page = session.find_entities(&query);
        entities.extend(page.rows);
        let Some(next) = page.next_offset else { break };
        query.offset = next;
    }
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let blocks: BTreeSet<_> = entities
        .iter()
        .filter_map(|r| r.block_name.clone())
        .collect();
    for name in blocks {
        match session.run(Command::CountBlock {
            block_name: name,
            layer: None,
        }) {
            Ok(Outcome::Measured { measurement }) => {
                items.push(from_measurement(&measurement, factor))
            }
            Err(e) => warnings.push(e.to_string()),
            _ => {}
        }
    }
    for row in &entities {
        if row.block_name.is_some()
            || row.area.is_none()
            || matches!(row.kind.as_str(), "Text" | "MText" | "Dimension" | "Table")
        {
            continue;
        }
        match session.run(Command::MeasureEntity {
            handle: row.handle,
            chain: row.chain.clone(),
        }) {
            Ok(Outcome::Measured { measurement }) => {
                items.push(from_measurement(&measurement, factor))
            }
            Err(e) => warnings.push(e.to_string()),
            _ => {}
        }
    }
    match session.rooms(&Default::default()) {
        Ok(rooms) => {
            for room in rooms.rooms {
                if room
                    .boundary_handle
                    .is_some_and(|h| items.iter().any(|i| i.source_objects == [format!("{h:X}")]))
                {
                    continue;
                }
                items.push(Item {
                    id: String::new(),
                    description: room.name.unwrap_or_else(|| "Unnamed room".into()),
                    kind: "area".into(),
                    unit: "m2".into(),
                    quantity: room.candidate_area_m2,
                    source_objects: room
                        .boundary_handle
                        .map(|h| vec![format!("{h:X}")])
                        .unwrap_or_default(),
                    position: Some([room.point[0] * factor, room.point[1] * factor]),
                    basis: "geometric".into(),
                    ..Default::default()
                });
            }
            if !rooms.skipped.is_empty() {
                warnings.push(format!(
                    "{} room labels could not be enclosed",
                    rooms.skipped.len()
                ));
            }
        }
        Err(e) => warnings.push(e.to_string()),
    }
    // Native measurement UUIDs change each run; repeatability compares quantities and citations.
    for item in &mut items {
        item.id = String::new();
    }
    Ok(Submission {
        items,
        warnings,
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
        ..Default::default()
    })
}
