//! Coalesce completed steps to at most five intermediate updates per second.
use qiyas_core::batch_progress::Step;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Throttle {
    last: Option<Duration>,
}
impl Throttle {
    /// A terminal update is always delivered before the tool result, including fast batches.
    pub fn accept(&mut self, step: &Step, elapsed: Duration) -> bool {
        if self
            .last
            .is_none_or(|last| elapsed.saturating_sub(last) >= Duration::from_millis(200))
            || step.done == step.total
        {
            self.last = Some(elapsed);
            true
        } else {
            false
        }
    }
}

pub fn observe<T>(mut send: impl FnMut(Step) + 'static, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let mut throttle = Throttle::default();
    qiyas_core::batch_progress::observe(
        move |step| {
            if throttle.accept(&step, start.elapsed()) {
                send(step);
            }
        },
        run,
    )
}

/// Match the private MCP operation with the CLI's tool-use id, regardless of encoded args.
pub(crate) fn key(name: &str, input: &serde_json::Value) -> String {
    let normalized = crate::normalise::envelope(name, input)
        .map(|(value, _)| value)
        .unwrap_or_else(|_| input.clone());
    crate::artifact::canonical(&serde_json::json!({"name":name, "op":normalized["op"], "args":crate::surface::args_of(&normalized).ok()})).to_string()
}

#[derive(Default)]
pub struct Bridge {
    active: std::collections::HashMap<String, String>,
    waiting: std::collections::HashMap<String, Vec<Step>>,
    closed: std::collections::HashSet<String>,
}
impl Bridge {
    pub fn step(&mut self, key: String, step: Step) -> Option<crate::AgentEvent> {
        let ids: Vec<_> = self
            .active
            .iter()
            .filter(|(_, value)| **value == key)
            .map(|(id, _)| id.clone())
            .collect();
        match ids.as_slice() {
            [id] => Some(crate::AgentEvent::ToolProgress {
                id: id.clone(),
                done: step.done,
                total: step.total,
                label: step.label,
            }),
            [] if !self.closed.contains(&key) => {
                self.waiting.entry(key).or_default().push(step);
                None
            }
            // Concurrent identical calls cannot be attributed reliably. Show no counter.
            _ => None,
        }
    }

    pub fn event(&mut self, event: &crate::AgentEvent) -> Vec<crate::AgentEvent> {
        match event {
            crate::AgentEvent::ToolCall {
                id, name, input, ..
            } => {
                let key = key(name, input);
                self.closed.remove(&key);
                self.active.insert(id.clone(), key.clone());
                self.waiting
                    .remove(&key)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|step| self.step(key.clone(), step))
                    .collect()
            }
            crate::AgentEvent::ToolResult { id, .. } => {
                if let Some(key) = self.active.remove(id) {
                    self.waiting.remove(&key);
                    self.closed.insert(key);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}
