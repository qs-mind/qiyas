//! The bill of quantities imported from Excel: the items the take-off is checked against.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// An imported BOQ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Boq {
    /// The workbook it was read from.
    pub source: String,
    /// When it was imported, RFC 3339.
    pub imported: String,
    pub items: Vec<BoqItem>,
    /// Item rows left out because their quantity is not a number.
    #[serde(default)]
    pub skipped_rows: u32,
}

impl Boq {
    /// A BOQ imported now from `source`.
    pub fn new(source: String, items: Vec<BoqItem>, skipped_rows: u32) -> Boq {
        Boq {
            source,
            imported: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .expect("UTC now formats as RFC 3339"),
            items,
            skipped_rows,
        }
    }
}

/// One BOQ line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoqItem {
    pub code: String,
    pub description: String,
    /// "m", "m2", "m3", "pcs", "kg", "t", or the workbook's own text when it is none of those.
    pub unit: String,
    pub quantity: f64,
    #[serde(default)]
    pub rate: Option<f64>,
    /// The section heading the item sits under; empty when there is none.
    #[serde(default)]
    pub section: String,
    /// Allowed difference from the take-off, in percent; the user's setting when `None`.
    #[serde(default)]
    pub tolerance: Option<f64>,
    /// The item's row in its sheet, counted from 1 as Excel shows it.
    pub row: u32,
}
