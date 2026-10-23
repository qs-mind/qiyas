//! Without recognition data there is no automatic BOQ; raw geometry remains candidates.
use super::model::Submission;
use std::path::Path;
pub fn measure(drawing: &Path) -> Result<Submission, String> {
    let session = qiyas_core::Session::open_standalone(drawing).map_err(|e| e.to_string())?;
    super::submission::from_session(&session)
}
