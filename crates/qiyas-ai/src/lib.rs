//! The AI's tools: a catalogue of JSON-schema'd tools over the command bus and the read-only
//! queries, the executor that runs them on a `Session`, and a picture of the drawing for the
//! model to look at. The in-app assistant and the MCP server both call these.
//!
//! The in-app assistant: Claude (`anthropic`) or any OpenAI-compatible service (`openai`) behind
//! one `Provider` trait, the system prompt (`prompt`), and the loop that runs the model's tool
//! calls on the shared session (`agent`), and what the user points it at with a message: the
//! selection, a marked region, the view, pasted pictures (`context`).
//!
//! The agent's log file (`log`).
//!
//! The MCP server for outside AI apps (`mcp`).

mod advice;
pub mod agent;
pub mod anthropic;
mod artifact;
pub mod chatgpt_plan;
pub mod claude_code;
pub mod context;
pub mod conversation;
pub mod errors;
mod execute;
pub mod host;
mod image;
pub mod log;
mod marks;
pub mod mcp;
pub mod models;
pub mod openai;
pub mod prompt;
pub mod provider;
mod result;
mod revision;
pub mod surface;
mod tools;
pub mod window;

pub use agent::{Agent, AgentEvent, AgentSettings, Autonomy, MeasurementMethod};
pub use context::{Region, RegionKind, TurnContext, UserImage, UserMessage};
pub use conversation::{Conversation, TranscriptEntry};
pub use execute::{execute, ToolError, ToolOutput};
pub use host::{is_host_tool, run_host, DrawingHost, Screen};
pub use image::{render_png, render_png_with_text};
pub use log::AgentLog;
pub use provider::{
    from_base64, AiError, ContentBlock, Message, Provider, ResultPart, Role, StopReason,
    StreamEvent, TurnRequest, TurnResult, Usage,
};
pub use surface::tools as catalogue;
pub use tools::ToolSpec;
pub use window::{is_window_tool, run_window};

mod churn;
mod normalise;
mod surface_args;

mod document;
mod history;
pub mod judgement;

pub use provider::RetryBackoff;

pub mod activity;
pub mod endpoint;

pub mod provider_error;

#[cfg(test)]
mod cache_replay;
