//! A conflict or gap found by comparing the BOQ, the take-off, the schedules on the drawings and
//! the block counts (the engine is `qiyas_takeoff::compare`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The linked measurements add up to more or less than the BOQ quantity, beyond tolerance.
    QuantityMismatch,
    DuplicateItem,
    RelationMismatch,
    DocumentMismatch,
    /// Measured work that no BOQ item describes.
    MissingFromBoq,
    /// A BOQ item with nothing linked and nothing measured that matches its description.
    NotInDrawings,
    /// A schedule's count for a mark differs from the blocks counted on the plans.
    ScheduleVsPlan,
    /// One block counted differently on two sheets of the same floor.
    DrawingVsDrawing,
    /// The linked measurements are in another unit than the BOQ item, or its unit is not a
    /// standard one.
    UnitDoubt,
}

impl ConflictKind {
    pub const ALL: [ConflictKind; 9] = [
        ConflictKind::QuantityMismatch,
        ConflictKind::DuplicateItem,
        ConflictKind::RelationMismatch,
        ConflictKind::DocumentMismatch,
        ConflictKind::MissingFromBoq,
        ConflictKind::NotInDrawings,
        ConflictKind::ScheduleVsPlan,
        ConflictKind::DrawingVsDrawing,
        ConflictKind::UnitDoubt,
    ];

    /// As stored, e.g. "quantity_mismatch".
    pub fn name(self) -> &'static str {
        match self {
            ConflictKind::DuplicateItem => "duplicate_item",
            ConflictKind::RelationMismatch => "relation_mismatch",
            ConflictKind::DocumentMismatch => "document_mismatch",
            ConflictKind::QuantityMismatch => "quantity_mismatch",
            ConflictKind::MissingFromBoq => "missing_from_boq",
            ConflictKind::NotInDrawings => "not_in_drawings",
            ConflictKind::ScheduleVsPlan => "schedule_vs_plan",
            ConflictKind::DrawingVsDrawing => "drawing_vs_drawing",
            ConflictKind::UnitDoubt => "unit_doubt",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictStatus {
    #[default]
    Open,
    Resolved,
    Ignored,
}

impl ConflictStatus {
    pub const ALL: [ConflictStatus; 3] = [
        ConflictStatus::Open,
        ConflictStatus::Resolved,
        ConflictStatus::Ignored,
    ];
}

/// One conflict. Quantities are in `unit` ("m", "m2", "m3", "pcs", or a BOQ item's own unit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Conflict {
    /// 16 hex digits, the same on every comparison for the same finding (see `hash_key`).
    pub id: String,
    pub kind: ConflictKind,
    pub boq_code: Option<String>,
    /// Ids of the measurements involved, sorted.
    pub measurements: Vec<String>,
    /// Name of the schedule involved.
    pub schedule: Option<String>,
    /// Names of the sheets involved, sorted.
    pub sheets: Vec<String>,
    /// What the BOQ or the schedule (or the first sheet) says.
    pub expected: Option<f64>,
    /// What the drawings measure (or the second sheet counts).
    pub found: Option<f64>,
    pub unit: String,
    /// A short English sentence with the numbers in it.
    pub reason: String,
    /// The same in Arabic.
    #[serde(default)]
    pub reason_ar: String,
    #[serde(default)]
    pub status: ConflictStatus,
    #[serde(default)]
    pub note: String,
    /// When it was first found, RFC 3339.
    #[serde(default)]
    pub created: String,
    /// What `id` is hashed from, for debugging.
    #[serde(default)]
    pub hash_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<crate::discrepancy::Query>,
}
