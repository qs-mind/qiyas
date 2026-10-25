//! Transactions: any commands, run in order, as ONE undo step, all or nothing. Each command runs
//! through `Session::run_as` as it would alone (its follow-ups too: fields, constraints, the
//! take-off), recording its own step inside an open group of the history; the group closes into
//! one step. When one fails, every step made before it is put back, last first, and the view
//! state (sheet, hidden layers, current state, drafting, UCS, isolation) is as it was: the
//! drawing's own change recorder can't be nested, so the group is made of steps, not one outer
//! recording.

use serde::Serialize;

use crate::undo::{Parts, ViewDelta, ViewRefs};
use crate::{Command, CoreError, Current, Drafting, Outcome, Session, Source, Ucs};

/// What the last command, batch or transaction did to the drawing's entities, by handle: added,
/// changed in place, removed. An entity added and removed again in one transaction is in none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Change {
    pub added: Vec<u64>,
    pub changed: Vec<u64>,
    pub removed: Vec<u64>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    /// This change followed by `later`.
    pub(crate) fn merge(&mut self, later: Change) {
        for h in later.added {
            if !self.added.contains(&h) {
                self.removed.retain(|r| *r != h);
                self.added.push(h);
            }
        }
        for h in later.changed {
            if !self.added.contains(&h) && !self.changed.contains(&h) {
                self.changed.push(h);
            }
        }
        for h in later.removed {
            if self.added.contains(&h) {
                self.added.retain(|a| *a != h);
            } else {
                self.changed.retain(|c| *c != h);
                if !self.removed.contains(&h) {
                    self.removed.push(h);
                }
            }
        }
    }
}

/// One step of the history as the AI and the window list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryStep {
    /// What it was: the command ("draw.line", "set_layer") or the transaction's name ("batch").
    pub label: String,
    pub source: Source,
}

/// The session's view state, which is no undo step but goes back with a failed transaction.
struct TxView {
    sheet: String,
    hidden: std::collections::HashSet<String>,
    arabic_reversed: bool,
    drafting: Drafting,
    current: Current,
    state_hidden: std::collections::BTreeMap<String, Vec<String>>,
    isolated: Option<crate::layers::Isolation>,
    ucs: Ucs,
}

impl TxView {
    fn take(s: &Session) -> TxView {
        TxView {
            sheet: s.sheet.clone(),
            hidden: s.visibility.hidden_layers.clone(),
            arabic_reversed: s.arabic_reversed,
            drafting: s.drafting.clone(),
            current: s.current.clone(),
            state_hidden: s.state_hidden.clone(),
            isolated: s.isolated.clone(),
            ucs: s.ucs,
        }
    }

    fn put_back(self, s: &mut Session) {
        s.sheet = self.sheet;
        s.visibility.hidden_layers = self.hidden;
        s.arabic_reversed = self.arabic_reversed;
        s.drafting = self.drafting;
        s.current = self.current;
        s.state_hidden = self.state_hidden;
        s.isolated = self.isolated;
        s.ucs = self.ucs;
    }
}

impl Session {
    /// Runs `f` as one undo step named `label` made by `source`: the commands it runs with
    /// `run_as` record their steps into a group that closes into one. When `f` fails, everything
    /// it did is put back (the drawing, take-off, BOQ, conflicts and view state) and its error
    /// returned. Transactions nest. Commands that can't run in a batch (`x-batch` false in the
    /// registry: editing a block in place, writing a block, the clipboard, a batch) are refused
    /// inside one.
    pub fn transaction<T>(
        &mut self,
        source: Source,
        label: &str,
        f: impl FnOnce(&mut Session) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let check_before = self.check_before.clone();
        if self.tx_depth == 0 {
            self.check_before.clear();
        }
        let before_revision = self.revision();
        let advice = self.last_advice.clone();
        if self.tx_depth == 0 {
            self.last_advice.clear();
        }
        let replacements = self.handle_replacements.clone();
        if self.tx_depth == 0 {
            self.handle_replacements.clear();
        }
        let structure = self.drawing.doc.snapshot_structure();
        let view = TxView::take(self);
        let current = self.current.clone();
        let revision = self.document_revision;
        // What an outer transaction did so far; a transaction of its own starts afresh.
        let change = std::mem::take(&mut self.last_change);
        let change = if self.tx_depth == 0 {
            Change::default()
        } else {
            change
        };
        self.tx_depth += 1;
        self.history.begin_group();
        let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(self)))
            .unwrap_or_else(|_| Err(CoreError::InvalidCommand(
                "The edit stopped unexpectedly. Its changes were rolled back. You can keep working.".into(),
            )));
        self.tx_depth -= 1;
        match done {
            Ok(value) => {
                let delta = ViewDelta::current_only(current, self.current.clone()).map(Box::new);
                self.history.commit_group(source, label, delta);
                let inner = std::mem::replace(&mut self.last_change, change);
                if inner.is_empty() && self.document_revision == revision {
                    self.check_before = check_before;
                }
                self.last_change.merge(inner);
                self.record_revision(before_revision);
                Ok(value)
            }
            Err(e) => {
                let mut doc_changed = false;
                self.history.abort_group(Parts {
                    takeoff: &mut self.takeoff,
                    boq: &mut self.boq,
                    conflicts: &mut self.conflicts,
                    schedules: &mut self.schedules,
                    doc: &mut self.drawing.doc,
                    view: ViewRefs {
                        current: &mut self.current,
                        hidden: &mut self.visibility.hidden_layers,
                        state_hidden: &mut self.state_hidden,
                    },
                    doc_changed: &mut doc_changed,
                });
                self.drawing.doc.swap_structure(structure);
                view.put_back(self);
                self.handle_replacements = replacements;
                self.last_change = change;
                self.last_advice = advice;
                self.check_before = check_before;
                // The drawing is as it was at `revision`: drawn again, and no unsaved change
                // left by steps that were put back.
                self.drawing_changed();
                *self.room_cache.get_mut() = Default::default();
                *self.ink_cache.get_mut() = Default::default();
                *self.check_geometry.get_mut() = Default::default();
                self.document_revision = revision;
                Err(e)
            }
        }
    }

    /// Runs `f` in one transaction, on `layer` and with `color` when given. A missing layer is
    /// made inside the transaction. The temporary current state is put back after `f`.
    pub fn batch_with<T>(
        &mut self,
        source: Source,
        label: &str,
        layer: Option<String>,
        color: Option<String>,
        f: impl FnOnce(&mut Session) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        self.transaction(source, label, |s| {
            let before = s.current.clone();
            let mut now = before.clone();
            if let Some(name) = layer.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                match s.drawing.doc.layers.get(name).map(|l| l.name.clone()) {
                    Some(spelled) => now.layer = spelled,
                    None => {
                        s.run_as(
                            Command::NewLayer {
                                name: name.to_string(),
                            },
                            source,
                        )?;
                        now.layer = name.to_string();
                    }
                }
            }
            if let Some(color) = color.filter(|c| !c.trim().is_empty()) {
                now.color = color;
            }
            let restore = now != before;
            if restore {
                s.run_as(Command::SetCurrent { current: now }, source)?;
            }
            let value = f(s)?;
            if restore {
                s.run_as(Command::SetCurrent { current: before }, source)?;
            }
            Ok(value)
        })
    }

    /// `Command::Batch`: `commands` in order as one undo step, all or nothing. A failing
    /// command's error says which one it was (`CoreError::Step`, from 0).
    pub(crate) fn run_batch(
        &mut self,
        commands: Vec<Command>,
        layer: Option<String>,
        color: Option<String>,
        source: Source,
    ) -> Result<Outcome, CoreError> {
        self.batch_with(source, "batch", layer, color, |s| {
            if commands.len() > 1
                && commands
                    .iter()
                    .all(|command| matches!(command, Command::AddWallOpenings { .. }))
            {
                let inputs: Vec<_> = commands
                    .iter()
                    .map(|command| serde_json::json!(command))
                    .collect();
                if let Some(grouped) = s.run_opening_batch(&inputs, source) {
                    return Ok(Outcome::Batch {
                        outcomes: grouped?.into_iter().map(|step| step.outcome).collect(),
                        added: s.last_change.added.clone(),
                    });
                }
            }
            let mut outcomes = Vec::with_capacity(commands.len());
            for (index, command) in commands.into_iter().enumerate() {
                let label = command.label();
                match s.run_as(command, source) {
                    Ok(outcome) => outcomes.push(outcome),
                    Err(error) => {
                        return Err(CoreError::Step {
                            index,
                            label,
                            error: Box::new(error),
                        })
                    }
                }
            }
            Ok(Outcome::Batch {
                added: s.last_change.added.clone(),
                outcomes,
            })
        })
    }

    /// What the last `run`, `run_as` or transaction did to the drawing's entities.
    pub fn last_change(&self) -> &Change {
        &self.last_change
    }

    /// Changes when the drawing changes, including its layers and styles.
    pub fn document_revision(&self) -> u64 {
        self.document_revision
    }

    /// Changes when a step is committed, even when the undo stack is full.
    pub fn history_revision(&self) -> u64 {
        self.history.revision()
    }

    /// Handles added by the caller's newest undo step. Read-only, for automation rebuild checks.
    pub fn undo_added_handles(&self, source: Source) -> Vec<u64> {
        self.history.undo_added_handles(source)
    }

    /// The steps that can be undone (oldest first) and redone (next first).
    pub fn history_steps(&self) -> (Vec<HistoryStep>, Vec<HistoryStep>) {
        self.history.labels()
    }
}
