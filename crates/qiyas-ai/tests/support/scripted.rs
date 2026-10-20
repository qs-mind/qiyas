//! Offline provider shared by surface and drafting evaluations.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use qiyas_ai::{AiError, ContentBlock, Provider, StopReason, TurnRequest, TurnResult};
use serde_json::Value;

pub enum ScriptCall {
    Fixed(&'static str, Value),
    /// Build a deterministic call from the conversation's tool results, with no session access.
    FromHistory(fn(&TurnRequest) -> (&'static str, Value)),
}

impl From<(&'static str, Value)> for ScriptCall {
    fn from((name, input): (&'static str, Value)) -> Self {
        Self::Fixed(name, input)
    }
}

/// Sends one tool call per answer, then ends the turn. Requests are kept to check the budget.
pub struct Script {
    pub calls: Mutex<VecDeque<ScriptCall>>,
    pub requests: Arc<Mutex<Vec<TurnRequest>>>,
}

impl Script {
    pub fn new(calls: Vec<(&'static str, Value)>, requests: Arc<Mutex<Vec<TurnRequest>>>) -> Self {
        Self::steps(calls.into_iter().map(ScriptCall::from).collect(), requests)
    }

    pub fn steps(calls: Vec<ScriptCall>, requests: Arc<Mutex<Vec<TurnRequest>>>) -> Self {
        Self {
            calls: Mutex::new(calls.into()),
            requests,
        }
    }
}

impl Provider for Script {
    fn stream_turn<'a>(
        &'a self,
        request: TurnRequest,
        _: qiyas_ai::provider::Sink<'a>,
    ) -> qiyas_ai::provider::BoxFuture<'a, Result<TurnResult, AiError>> {
        let next = self
            .calls
            .lock()
            .unwrap()
            .pop_front()
            .map(|call| match call {
                ScriptCall::Fixed(name, input) => (name, input),
                ScriptCall::FromHistory(build) => build(&request),
            });
        let id = {
            let mut requests = self.requests.lock().unwrap();
            let id = requests.len();
            requests.push(request);
            id
        };
        Box::pin(async move {
            let (content, stop_reason) = match next {
                Some((name, input)) => (
                    vec![ContentBlock::ToolUse {
                        id: format!("call_{id}"),
                        name: name.into(),
                        input,
                    }],
                    StopReason::ToolUse,
                ),
                None => (
                    vec![ContentBlock::Text {
                        text: "Done".into(),
                    }],
                    StopReason::EndTurn,
                ),
            };
            Ok(TurnResult {
                content,
                stop_reason,
                usage: Default::default(),
            })
        })
    }
}
