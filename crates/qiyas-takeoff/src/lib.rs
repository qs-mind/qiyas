//! The comparison engine: turns the BOQ, the take-off, the schedules read off the drawings and
//! the block counts into conflicts and gaps (`compare`), and runs it on a session
//! (`compare_session`).

mod accuracy;
pub use accuracy::{accuracy, net_room_accuracy, Accuracy, NetRoomAccuracy};
pub use qiyas_core::discrepancy::{compare, CompareInputs};
use qiyas_core::{Conflict, CoreError, Session, Source};
pub const DEFAULT_TOLERANCE: f64 = 5.0;

/// Run the shared discrepancy engine and raise drawing queries in one undo step.
pub fn compare_session(
    session: &mut Session,
    default_tol: f64,
    source: Source,
) -> Result<Vec<Conflict>, CoreError> {
    session.run_as(
        qiyas_core::Command::Check {
            tolerance: Some(default_tol),
        },
        source,
    )?;
    Ok(session.conflicts.clone())
}
