//! Optional nested timing for offline fixtures. Regular application builds omit this module.
use std::{cell::RefCell, collections::BTreeMap, time::Instant};

#[derive(Default)]
struct State {
    enabled: bool,
    stack: Vec<Frame>,
    totals: BTreeMap<&'static str, [u64; 3]>,
}
struct Frame {
    name: &'static str,
    started: Instant,
    children: u64,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Enable timers for one synchronous fixture call on this thread.
pub fn begin() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        assert!(
            state.stack.is_empty(),
            "a previous profile span is still open"
        );
        *state = State {
            enabled: true,
            ..State::default()
        };
    });
}

/// Return call count, inclusive nanoseconds and exclusive nanoseconds by function.
pub fn finish() -> BTreeMap<&'static str, [u64; 3]> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        assert!(state.stack.is_empty(), "a profile span is still open");
        state.enabled = false;
        std::mem::take(&mut state.totals)
    })
}

#[must_use]
pub struct Span(bool);
pub fn span(name: &'static str) -> Span {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if !state.enabled {
            return Span(false);
        }
        state.stack.push(Frame {
            name,
            started: Instant::now(),
            children: 0,
        });
        Span(true)
    })
}
impl Drop for Span {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let frame = state.stack.pop().expect("active profile span");
            let elapsed = frame.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            if let Some(parent) = state.stack.last_mut() {
                parent.children = parent.children.saturating_add(elapsed);
            }
            let row = state.totals.entry(frame.name).or_default();
            row[0] += 1;
            row[1] = row[1].saturating_add(elapsed);
            row[2] = row[2].saturating_add(elapsed.saturating_sub(frame.children));
        });
    }
}
