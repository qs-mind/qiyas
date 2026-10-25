//! Admission is separate from drawing locks. Queued Claude turns do not occupy ordinary slots.
use crate::state::lock;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Queue {
    next: u64,
    waiting: VecDeque<(u64, bool)>,
    running: usize,
    claude: bool,
}

pub struct TurnQueue {
    queue: Mutex<Queue>,
    changed: Notify,
}
impl Default for TurnQueue {
    fn default() -> Self {
        Self {
            queue: Mutex::default(),
            changed: Notify::new(),
        }
    }
}

/// A permit lasts until the provider task returns. Claude also holds its process semaphore
/// until the child is reaped, so cancellation cannot overlap two executable processes.
pub struct Permit {
    queue: Arc<TurnQueue>,
    claude: bool,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = lock(&self.queue.queue);
        state.running -= 1;
        if self.claude {
            state.claude = false;
        }
        drop(state);
        self.queue.changed.notify_waiters();
    }
}
struct Waiting {
    queue: Arc<TurnQueue>,
    ticket: u64,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        lock(&self.queue.queue)
            .waiting
            .retain(|(id, _)| *id != self.ticket);
        self.queue.changed.notify_waiters();
    }
}

impl TurnQueue {
    pub async fn acquire(
        self: &Arc<Self>,
        claude: bool,
        cancel: &CancellationToken,
        limit: impl Fn() -> usize,
    ) -> Option<Permit> {
        let ticket = {
            let mut state = lock(&self.queue);
            let id = state.next;
            state.next += 1;
            state.waiting.push_back((id, claude));
            id
        };
        let _waiting = Waiting {
            queue: self.clone(),
            ticket,
        };
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if cancel.is_cancelled() {
                return None;
            }
            {
                let mut state = lock(&self.queue);
                let first = state
                    .waiting
                    .iter()
                    .find(|(_, wants_claude)| !*wants_claude || !state.claude);
                if first.is_some_and(|(id, _)| *id == ticket) && state.running < limit().max(1) {
                    state.waiting.retain(|(id, _)| *id != ticket);
                    state.running += 1;
                    if claude {
                        state.claude = true;
                    }
                    return Some(Permit {
                        queue: self.clone(),
                        claude,
                    });
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => return None,
                _ = &mut changed => {},
                _ = tokio::time::sleep(Duration::from_secs(1)) => {},
            }
        }
    }
}

/// Keep at least 1 GiB available for the desktop, render buffers and the operating system.
/// Existing work finishes; pressure changes admission only. Unknown OS readings use Settings.
pub fn effective_limit(configured: usize, available_kib: Option<u64>) -> usize {
    if available_kib.is_some_and(|free| free < 1024 * 1024) {
        1
    } else {
        configured.clamp(1, 8)
    }
}
#[cfg(not(test))]
pub fn available_memory_kib() -> Option<u64> {
    let mem = std::fs::read_to_string("/proc/meminfo").ok()?;
    mem.lines().find_map(|line| {
        line.strip_prefix("MemAvailable:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fourth_turn_waits_for_one_of_the_three_running_turns() {
        let queue = Arc::new(TurnQueue::default());
        let cancel = CancellationToken::new();
        let a = queue.acquire(false, &cancel, || 3).await.unwrap();
        let b = queue.acquire(false, &cancel, || 3).await.unwrap();
        let c = queue.acquire(false, &cancel, || 3).await.unwrap();
        let waiting = queue.acquire(false, &cancel, || 3);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut waiting)
                .await
                .is_err()
        );
        assert_eq!(lock(&queue.queue).running, 3);
        drop(b);
        let d = waiting.await.unwrap();
        assert_eq!(lock(&queue.queue).running, 3);
        drop((a, c, d));
        assert_eq!(lock(&queue.queue).running, 0);
    }
    #[tokio::test]
    async fn claude_waits_without_blocking_other_providers_and_cancels_in_queue() {
        let queue = Arc::new(TurnQueue::default());
        let cancel = CancellationToken::new();
        let first = queue.acquire(true, &cancel, || 3).await.unwrap();
        let waiting_cancel = cancel.child_token();
        let task = {
            let queue = queue.clone();
            let token = waiting_cancel.clone();
            tokio::spawn(async move { queue.acquire(true, &token, || 3).await })
        };
        tokio::task::yield_now().await;
        let ordinary = tokio::time::timeout(
            Duration::from_millis(100),
            queue.acquire(false, &cancel, || 3),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!task.is_finished());
        waiting_cancel.cancel();
        assert!(task.await.unwrap().is_none());
        drop(first);
        drop(ordinary);
        assert_eq!(lock(&queue.queue).running, 0);
        assert!(lock(&queue.queue).waiting.is_empty());
    }
    #[tokio::test]
    async fn lower_limit_waits_for_running_work_without_cancelling_it() {
        let queue = Arc::new(TurnQueue::default());
        let token = CancellationToken::new();
        let a = queue.acquire(false, &token, || 3).await.unwrap();
        let b = queue.acquire(false, &token, || 3).await.unwrap();
        let next = queue.acquire(false, &token, || effective_limit(3, Some(512_000)));
        tokio::pin!(next);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut next)
            .await
            .is_err());
        drop(a);
        assert!(tokio::time::timeout(Duration::from_millis(20), &mut next)
            .await
            .is_err());
        drop(b);
        assert!(next.await.is_some());
        assert_eq!(effective_limit(3, None), 3);
    }
}
