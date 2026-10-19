//! The AI's tools: a catalogue of JSON-schema'd tools over the command bus and the read-only
//! queries, the executor that runs them on a `Session`, and a picture of the drawing for the
//! model to look at. The in-app assistant and the MCP server both call these.
//!
//! The in-app assistant: Claude (`anthropic`) or any OpenAI-compatible service (`openai`) behind
//! one `Provider` trait, the system prompt (`prompt`), and the loop that runs the model's tool
//! calls on the shared session (`agent`).
//!
//! The agent's log file (`log`).
//!
//! The MCP server for outside AI apps (`mcp`).

pub mod agent;
pub mod anthropic;
mod execute;
pub mod host;
mod image;
pub mod log;
pub mod mcp;
pub mod openai;
pub mod prompt;
pub mod provider;
mod tools;

pub use agent::{Agent, AgentEvent, AgentSettings};
pub use execute::{execute, ToolError, ToolOutput};
pub use host::{is_host_tool, run_host, DrawingHost};
pub use image::{render_png, render_png_with_text};
pub use log::AgentLog;
pub use provider::{
    AiError, ContentBlock, Message, Provider, ResultPart, Role, StopReason, StreamEvent,
    TurnRequest, TurnResult, Usage,
};
pub use tools::{catalogue, ToolSpec};
