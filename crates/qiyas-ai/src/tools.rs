//! The provider-neutral tool specification. The catalogue lives in `surface`.
//!
//! Every schema is an object with `additionalProperties: false` and a `required` list, and uses
//! only what the API's strict tool mode accepts (no numeric, length or item-count limits); the
//! executor checks those limits itself.

use serde_json::Value;

/// One tool the model can call.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema of the input: an object, `additionalProperties: false`, with `required`.
    pub input_schema: Value,
    /// Whether the tool can change the drawing or take-off. The surface refines this per call:
    /// execute operations mutate except selecting and viewing.
    pub mutating: bool,
}
