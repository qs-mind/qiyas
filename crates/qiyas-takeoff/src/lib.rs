//! The comparison engine: turns the BOQ, the take-off, the schedules read off the drawings and
//! the block counts into conflicts and gaps (`compare`), and runs it on a session
//! (`compare_session`).

mod accuracy;
mod compare;
mod matching;

use qiyas_core::{Command, Conflict, CoreError, Session, Source};

pub use accuracy::{accuracy, net_room_accuracy, Accuracy, NetRoomAccuracy};
pub use compare::{compare, CompareInputs};

/// Allowed difference, in percent, for BOQ items with none of their own when the user has set
/// none either. The BOQ panel uses the same.
pub const DEFAULT_TOLERANCE: f64 = 5.0;

/// Compares the session's BOQ and take-off with the schedules and block counts of every sheet,
/// keeps the conflicts and schedules on the session (one undo step, made by `source`) and
/// returns the conflicts. Sheets are named by their layout names. Blocks are counted as the file
/// has them (`Session::block_summary_in_file`), so layers the user has hidden still count.
/// Conflicts found before keep their status and note.
pub fn compare_session(
    session: &mut Session,
    default_tol: f64,
    source: Source,
) -> Result<Vec<Conflict>, CoreError> {
    if !(default_tol >= 0.0 && default_tol.is_finite()) {
        return Err(CoreError::InvalidCommand(
            "The tolerance must be a percentage of 0 or more".to_string(),
        ));
    }
    let sheets = session.drawing.sheets();
    let name_of = |block: &str| {
        sheets
            .iter()
            .find(|s| s.block == block)
            .map_or_else(|| block.to_string(), |s| s.name.clone())
    };
    let mut schedules = Vec::new();
    let mut blocks = Vec::new();
    for sheet in &sheets {
        schedules.extend(session.read_tables(Some(&sheet.block)));
        blocks.push((
            sheet.name.clone(),
            session.block_summary_in_file(Some(&sheet.block)),
        ));
    }
    // The engine sees sheet names, as the user does; the session keeps the schedules as read.
    let mut takeoff = session.takeoff.clone();
    for m in &mut takeoff.items {
        m.sheet = name_of(&m.sheet);
    }
    let mut named = schedules.clone();
    for s in &mut named {
        s.sheet = name_of(&s.sheet);
    }
    let units = session.info().units;
    let inputs = CompareInputs {
        boq: session.boq.as_ref(),
        takeoff: &takeoff,
        schedules: &named,
        blocks: &blocks,
        units_label: &units,
    };
    let conflicts = compare(&inputs, default_tol, &session.conflicts);
    session.run_as(
        Command::SetConflicts {
            conflicts: conflicts.clone(),
            schedules,
        },
        source,
    )?;
    Ok(conflicts)
}
