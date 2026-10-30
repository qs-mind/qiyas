//! Observational batch progress on the executing thread. It never changes execution or undo.
use std::cell::{Cell, RefCell};

#[derive(Debug, Clone)]
pub struct Step {
    pub done: usize,
    pub total: usize,
    /// Wire operation name; the window translates it through its activity mapping.
    pub label: String,
}

type Observer = Box<dyn FnMut(Step)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None);
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Installs a callback only for this operation, restoring it even on panic.
pub fn observe<T>(observer: impl FnMut(Step) + 'static, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<Observer>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OBSERVER.with(|slot| slot.replace(self.0.take()));
        }
    }
    let _restore = Restore(OBSERVER.with(|slot| slot.replace(Some(Box::new(observer)))));
    run()
}

/// Nested batches count as one step of their enclosing batch.
pub struct Batch {
    total: usize,
    outer: bool,
}
impl Batch {
    pub fn new(total: usize) -> Self {
        let outer = DEPTH.with(|depth| {
            let old = depth.get();
            depth.set(old + 1);
            old == 0
        });
        Self { total, outer }
    }

    /// Called only after a successful step, even when a later failure rolls the batch back.
    pub fn completed(&self, done: usize, input: &serde_json::Value) {
        if !self.outer {
            return;
        }
        OBSERVER.with(|slot| {
            if let Some(observer) = slot.borrow_mut().as_mut() {
                let name = input["type"].as_str().unwrap_or("command");
                let label = input["kind"]
                    .as_str()
                    .map_or_else(|| name.to_owned(), |kind| format!("{name}.{kind}"));
                observer(Step {
                    done,
                    total: self.total,
                    label,
                });
            }
        });
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}
