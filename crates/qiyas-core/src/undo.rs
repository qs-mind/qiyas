//! Undo and redo. The take-off, the BOQ and the conflicts are kept whole: small enough at
//! take-off sizes, and a step keeps only the part its command changed, so editing the take-off
//! never copies a long BOQ. A drawing change keeps only what it touched: the before images of
//! the entities it added, removed or changed (opencadcodec's entity change recorder), and the
//! drawing's tables when the command changes them too. Each step remembers who made it, so the AI
//! can undo only its own last change.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use opencadcodec::classes::DxfClassCollection;
use opencadcodec::objects::ObjectType;
use opencadcodec::tables::{
    AppId, BlockRecord, DimStyle, Layer, LineType, Table, TextStyle, Ucs, VPort, View,
};
use opencadcodec::{CadDocument, EntityType, Handle};

use crate::{Boq, Conflict, Current, HistoryStep, Schedule, Source, Takeoff};

/// Most undo steps kept; the oldest is dropped first.
const MAX_STEPS: usize = 200;

/// The part of the session a command changed, as it was before (or after, on the redo stack).
#[derive(Debug)]
pub(crate) enum Step {
    ProjectFile(crate::projects::FileChange),
    Takeoff(Takeoff),
    Boq(Option<Boq>),
    Conflicts(Vec<Conflict>, Vec<Schedule>),
    /// A change to the drawing (and to the take-off with it, for divide and measure along).
    Document(DocChange),
    /// Several steps made as one (a batch, a transaction), held in the order they are put back
    /// (the last one made first), and what the group did to the current drawing state.
    Group(Vec<Step>, Option<Box<ViewDelta>>),
}

/// One drawing change, held as what puts it back: undoing removes `added`, restores `removed`
/// and puts back `replaced`, and swaps `tables` and `takeoff` in. Doing that gives the change
/// that redoes it, and so on.
#[derive(Debug, Default)]
pub(crate) struct DocChange {
    /// Entities the change added, by handle.
    pub added: Vec<Handle>,
    /// Entities the change removed, as they were.
    pub removed: Vec<Arc<EntityType>>,
    /// Entities the change edited, as they were before.
    pub replaced: Vec<(Handle, Arc<EntityType>)>,
    /// The drawing's tables and objects before the change, for a command that changes them
    /// (layers, views, block records, underlay and image definitions).
    pub tables: Option<Box<TablesSnapshot>>,
    /// The take-off before the change, for a command that changes it too.
    pub takeoff: Option<Takeoff>,
    /// What the change did to the session's view of the drawing's tables, when it moved it (a
    /// renamed layer is the current or a hidden one; a layer state's hidden layers). Only that
    /// difference is undone, so layers hidden or shown since are left as they are.
    pub view: Option<Box<ViewDelta>>,
}

/// What one drawing change did to the session's `ViewState`: the current state before and
/// after, the layers it hid and showed, and each layer state's hidden layers before and after.
/// Undo applies it backwards, redo forwards; what changed since in the same place is kept.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ViewDelta {
    pub current: Option<(Current, Current)>,
    pub hidden_added: Vec<String>,
    pub hidden_removed: Vec<String>,
    pub state_hidden: Vec<StateHidden>,
}

/// A layer state's name, its hidden layers before and after (`None`: not there).
pub(crate) type StateHidden = (String, Option<Vec<String>>, Option<Vec<String>>);

impl ViewDelta {
    /// The current drawing state moved from `before` to `after` (a batch that ran `SetCurrent`).
    pub(crate) fn current_only(before: Current, after: Current) -> Option<ViewDelta> {
        (before != after).then(|| ViewDelta {
            current: Some((before, after)),
            ..ViewDelta::default()
        })
    }

    /// The difference from `before` to `after`; `None` when they are the same.
    pub(crate) fn between(before: &ViewState, after: &ViewState) -> Option<ViewDelta> {
        let mut delta = ViewDelta::default();
        if before.current != after.current {
            delta.current = Some((before.current.clone(), after.current.clone()));
        }
        let mut added: Vec<String> = after.hidden.difference(&before.hidden).cloned().collect();
        let mut removed: Vec<String> = before.hidden.difference(&after.hidden).cloned().collect();
        added.sort();
        removed.sort();
        delta.hidden_added = added;
        delta.hidden_removed = removed;
        let names: std::collections::BTreeSet<&String> = before
            .state_hidden
            .keys()
            .chain(after.state_hidden.keys())
            .collect();
        for name in names {
            let (b, a) = (before.state_hidden.get(name), after.state_hidden.get(name));
            if b != a {
                delta
                    .state_hidden
                    .push((name.clone(), b.cloned(), a.cloned()));
            }
        }
        (delta != ViewDelta::default()).then_some(delta)
    }

    /// Takes the change back in `view` and returns the delta that does it again. A current state
    /// or a layer state's hidden layers changed since is left as it is; the hidden layers lose
    /// only what the change hid and get back only what it showed (see `still_hidden`).
    fn undo(self, view: &mut ViewRefs) -> ViewDelta {
        if let Some((before, after)) = &self.current {
            if *view.current == *after {
                *view.current = before.clone();
            }
        }
        let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        // A renamed hidden layer is hidden again under its old name only while it is still
        // hidden under the new one: the user may have shown it since. A deleted one (nothing
        // hidden in its place) comes back hidden.
        let still_hidden = self.hidden_added.is_empty()
            || self
                .hidden_added
                .iter()
                .any(|name| view.hidden.iter().any(|h| same(h, name)));
        for name in &self.hidden_added {
            view.hidden.retain(|h| !same(h, name));
        }
        if still_hidden {
            for name in &self.hidden_removed {
                if !view.hidden.iter().any(|h| same(h, name)) {
                    view.hidden.insert(name.clone());
                }
            }
        }
        for (state, before, after) in &self.state_hidden {
            if view.state_hidden.get(state) == after.as_ref() {
                match before {
                    Some(hidden) => {
                        view.state_hidden.insert(state.clone(), hidden.clone());
                    }
                    None => {
                        view.state_hidden.remove(state);
                    }
                }
            }
        }
        ViewDelta {
            current: self.current.map(|(before, after)| (after, before)),
            hidden_added: self.hidden_removed,
            hidden_removed: self.hidden_added,
            state_hidden: self
                .state_hidden
                .into_iter()
                .map(|(state, before, after)| (state, after, before))
                .collect(),
        }
    }
}

/// What the session keeps about the drawing's layers outside the drawing: the current drawing
/// state, the hidden layers and the hidden layers kept with each layer state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ViewState {
    pub current: Current,
    pub hidden: HashSet<String>,
    pub state_hidden: BTreeMap<String, Vec<String>>,
}

/// The session's live `ViewState`, to put one back.
pub(crate) struct ViewRefs<'a> {
    pub current: &'a mut Current,
    pub hidden: &'a mut HashSet<String>,
    pub state_hidden: &'a mut BTreeMap<String, Vec<String>>,
}

impl ViewRefs<'_> {
    /// As it is now.
    pub(crate) fn get(&self) -> ViewState {
        ViewState {
            current: self.current.clone(),
            hidden: self.hidden.clone(),
            state_hidden: self.state_hidden.clone(),
        }
    }
}

/// The parts of a drawing outside its entities that commands change, with the header variables
/// they set.
#[derive(Debug)]
pub(crate) struct TablesSnapshot {
    layers: Table<Layer>,
    line_types: Table<LineType>,
    text_styles: Table<TextStyle>,
    block_records: Table<BlockRecord>,
    dim_styles: Table<DimStyle>,
    app_ids: Table<AppId>,
    views: Table<View>,
    vports: Table<VPort>,
    ucss: Table<Ucs>,
    classes: DxfClassCollection,
    objects: std::collections::HashMap<Handle, ObjectType>,
    header: HeaderBits,
}

/// The header variables commands change: the annotation scale and the current dimension style
/// with the dimension variables it sets. Never the whole header, whose handle seed must only
/// grow.
#[derive(Debug, Clone, PartialEq)]
struct HeaderBits {
    named_objects_dict_handle: Handle,
    annotation_scale: String,
    annotation_scale_value: f64,
    dimstyle_name: String,
    dimstyle_handle: Handle,
    dim: [f64; 7],
    dim_decimal_places: i16,
    // The variables `SetRecords` may set (`record_edit::HEADER_WRITABLE`).
    linetype_scale: f64,
    point_display_mode: i16,
    point_display_size: f64,
    text_height: f64,
    fill_mode: bool,
    mirror_text: bool,
    lineweight_display: bool,
    linear_unit_format: i16,
    linear_unit_precision: i16,
    angular_unit_format: i16,
    angular_unit_precision: i16,
    attribute_visibility: i16,
    project_name: String,
}

impl HeaderBits {
    fn take(doc: &CadDocument) -> HeaderBits {
        let h = &doc.header;
        HeaderBits {
            named_objects_dict_handle: h.named_objects_dict_handle,
            annotation_scale: h.current_annotation_scale.clone(),
            annotation_scale_value: h.annotation_scale_value,
            dimstyle_name: h.current_dimstyle_name.clone(),
            dimstyle_handle: h.current_dimstyle_handle,
            dim: [
                h.dim_scale,
                h.dim_arrow_size,
                h.dim_text_height,
                h.dim_ext_line_offset,
                h.dim_ext_line_extension,
                h.dim_line_gap,
                h.dim_linear_scale,
            ],
            dim_decimal_places: h.dim_decimal_places,
            linetype_scale: h.linetype_scale,
            point_display_mode: h.point_display_mode,
            point_display_size: h.point_display_size,
            text_height: h.text_height,
            fill_mode: h.fill_mode,
            mirror_text: h.mirror_text,
            lineweight_display: h.lineweight_display,
            linear_unit_format: h.linear_unit_format,
            linear_unit_precision: h.linear_unit_precision,
            angular_unit_format: h.angular_unit_format,
            angular_unit_precision: h.angular_unit_precision,
            attribute_visibility: h.attribute_visibility,
            project_name: h.project_name.clone(),
        }
    }

    fn put(self, doc: &mut CadDocument) {
        let h = &mut doc.header;
        h.named_objects_dict_handle = self.named_objects_dict_handle;
        h.current_annotation_scale = self.annotation_scale;
        h.annotation_scale_value = self.annotation_scale_value;
        h.current_dimstyle_name = self.dimstyle_name;
        h.current_dimstyle_handle = self.dimstyle_handle;
        [
            h.dim_scale,
            h.dim_arrow_size,
            h.dim_text_height,
            h.dim_ext_line_offset,
            h.dim_ext_line_extension,
            h.dim_line_gap,
            h.dim_linear_scale,
        ] = self.dim;
        h.dim_decimal_places = self.dim_decimal_places;
        h.linetype_scale = self.linetype_scale;
        h.point_display_mode = self.point_display_mode;
        h.point_display_size = self.point_display_size;
        h.text_height = self.text_height;
        h.fill_mode = self.fill_mode;
        h.mirror_text = self.mirror_text;
        h.lineweight_display = self.lineweight_display;
        h.linear_unit_format = self.linear_unit_format;
        h.linear_unit_precision = self.linear_unit_precision;
        h.angular_unit_format = self.angular_unit_format;
        h.angular_unit_precision = self.angular_unit_precision;
        h.attribute_visibility = self.attribute_visibility;
        h.project_name = self.project_name;
    }
}

impl TablesSnapshot {
    pub(crate) fn take(doc: &CadDocument) -> Box<TablesSnapshot> {
        Box::new(TablesSnapshot {
            layers: doc.layers.clone(),
            line_types: doc.line_types.clone(),
            text_styles: doc.text_styles.clone(),
            block_records: doc.block_records.clone(),
            dim_styles: doc.dim_styles.clone(),
            app_ids: doc.app_ids.clone(),
            views: doc.views.clone(),
            vports: doc.vports.clone(),
            ucss: doc.ucss.clone(),
            classes: doc.classes.clone(),
            objects: doc.objects.clone(),
            header: HeaderBits::take(doc),
        })
    }

    /// True when `doc` holds these tables still.
    fn same_as(&self, doc: &CadDocument) -> bool {
        self.layers == doc.layers
            && self.line_types == doc.line_types
            && self.text_styles == doc.text_styles
            && self.block_records == doc.block_records
            && self.dim_styles == doc.dim_styles
            && self.app_ids == doc.app_ids
            && self.views == doc.views
            && self.vports == doc.vports
            && self.ucss == doc.ucss
            && self.classes == doc.classes
            && self.objects == doc.objects
            && self.header == HeaderBits::take(doc)
    }

    /// Puts these tables in `doc` and returns the ones they replaced.
    fn swap(mut self: Box<Self>, doc: &mut CadDocument) -> Box<TablesSnapshot> {
        use std::mem::swap;
        swap(&mut self.layers, &mut doc.layers);
        swap(&mut self.line_types, &mut doc.line_types);
        swap(&mut self.text_styles, &mut doc.text_styles);
        swap(&mut self.block_records, &mut doc.block_records);
        swap(&mut self.dim_styles, &mut doc.dim_styles);
        swap(&mut self.app_ids, &mut doc.app_ids);
        swap(&mut self.views, &mut doc.views);
        swap(&mut self.vports, &mut doc.vports);
        swap(&mut self.ucss, &mut doc.ucss);
        swap(&mut self.classes, &mut doc.classes);
        swap(&mut self.objects, &mut doc.objects);
        let now = HeaderBits::take(doc);
        std::mem::replace(&mut self.header, now).put(doc);
        self
    }
}

impl DocChange {
    /// The change a command made, from the recorder's before images (`None`: added while
    /// recording) and the drawing after it. An entity added and removed again is left out.
    pub(crate) fn from_before_images(
        doc: &CadDocument,
        before: Vec<(Handle, Option<Arc<EntityType>>)>,
        tables: Option<Box<TablesSnapshot>>,
        takeoff: Option<Takeoff>,
    ) -> DocChange {
        let mut change = DocChange {
            tables,
            takeoff,
            ..DocChange::default()
        };
        for (handle, image) in before {
            let now = doc.get_entity(handle).is_some();
            match (image, now) {
                (None, true) => change.added.push(handle),
                (None, false) => {}
                (Some(image), true) => change.replaced.push((handle, image)),
                (Some(image), false) => change.removed.push(image),
            }
        }
        change
    }

    /// True when the change left the drawing (and the take-off) as they were.
    pub(crate) fn is_empty(&self, doc: &CadDocument) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.replaced.is_empty()
            && self.takeoff.is_none()
            && self.view.is_none()
            && self.tables.as_ref().is_none_or(|t| t.same_as(doc))
    }

    /// Takes the change back in `doc` and `takeoff`, and returns the change that redoes it.
    /// Removed entities come back under their own handles, into the block they were in (a
    /// removal keeps the block's list of handles; see `CadDocument::remove_entity_arc`).
    pub(crate) fn swap(
        self,
        doc: &mut CadDocument,
        takeoff: &mut Takeoff,
        view: &mut ViewRefs,
    ) -> DocChange {
        let mut redo = DocChange::default();
        for handle in self.added {
            if let Some(entity) = doc.remove_entity_arc(handle) {
                redo.removed.push(entity);
            }
        }
        for entity in self.removed {
            if let Some(handle) = doc.restore_entity_arc(entity) {
                redo.added.push(handle);
            }
        }
        for (handle, entity) in self.replaced {
            if let Some(now) = doc.replace_entity_arc(handle, entity) {
                redo.replaced.push((handle, now));
            }
        }
        redo.tables = self.tables.map(|t| t.swap(doc));
        redo.takeoff = self.takeoff.map(|t| std::mem::replace(takeoff, t));
        redo.view = self.view.map(|delta| Box::new(delta.undo(view)));
        redo
    }
}

/// The parts of the session undo and redo put back.
pub(crate) struct Parts<'a> {
    pub takeoff: &'a mut Takeoff,
    pub boq: &'a mut Option<Boq>,
    pub conflicts: &'a mut Vec<Conflict>,
    pub schedules: &'a mut Vec<Schedule>,
    pub doc: &'a mut CadDocument,
    pub view: ViewRefs<'a>,
    /// Set when the drawing was put back, so the session redraws it.
    pub doc_changed: &'a mut bool,
}

impl Parts<'_> {
    /// The same parts, borrowed again for one step of a group.
    fn reborrow(&mut self) -> Parts<'_> {
        Parts {
            takeoff: &mut *self.takeoff,
            boq: &mut *self.boq,
            conflicts: &mut *self.conflicts,
            schedules: &mut *self.schedules,
            doc: &mut *self.doc,
            view: ViewRefs {
                current: &mut *self.view.current,
                hidden: &mut *self.view.hidden,
                state_hidden: &mut *self.view.state_hidden,
            },
            doc_changed: &mut *self.doc_changed,
        }
    }
}

impl Step {
    /// Puts this step in place and returns what it replaced.
    fn swap(self, mut parts: Parts) -> Step {
        match self {
            Step::ProjectFile(change) => Step::ProjectFile(change.reversed()),
            Step::Takeoff(t) => Step::Takeoff(std::mem::replace(parts.takeoff, t)),
            Step::Boq(b) => Step::Boq(std::mem::replace(parts.boq, b)),
            Step::Conflicts(c, s) => Step::Conflicts(
                std::mem::replace(parts.conflicts, c),
                std::mem::replace(parts.schedules, s),
            ),
            Step::Document(change) => {
                *parts.doc_changed = true;
                Step::Document(change.swap(parts.doc, parts.takeoff, &mut parts.view))
            }
            // Each step in the order held; what redoes them goes the other way round.
            Step::Group(steps, view) => {
                let mut back: Vec<Step> = steps
                    .into_iter()
                    .map(|step| step.swap(parts.reborrow()))
                    .collect();
                back.reverse();
                let view = view.map(|delta| Box::new(delta.undo(&mut parts.view)));
                Step::Group(back, view)
            }
        }
    }
}

/// One undo or redo step: what puts it back, who made it and what it was ("draw.line",
/// "batch").
#[derive(Debug)]
pub(crate) struct Entry {
    step: Step,
    source: Source,
    label: String,
}

#[derive(Debug, Default)]
pub(crate) struct History {
    /// The state before each change, who made it and what it was.
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// Undo stops with this many steps left (a block being edited in place keeps the steps
    /// from before it began).
    floor: usize,
    /// The steps of the groups open now (a batch, a transaction), innermost last: each closes
    /// into one step of the group around it, or of the history.
    groups: Vec<Vec<Entry>>,
    /// Changes whenever a step reaches the undo stack, even when the stack is full.
    revision: u64,
}

impl History {
    /// Records the state as it was before a command by `source`, what the command was being
    /// `label`. Clears what could be redone; inside a group, keeps it for the group.
    pub(crate) fn record(&mut self, before: Step, source: Source, label: &str) {
        let entry = Entry {
            step: before,
            source,
            label: label.to_string(),
        };
        if let Some(group) = self.groups.last_mut() {
            group.push(entry);
            return;
        }
        self.redo.clear();
        self.push(entry);
    }

    /// Pushes a step on the undo stack, dropping the oldest past `MAX_STEPS` (the floor drops
    /// with it).
    fn push(&mut self, entry: Entry) {
        self.revision = self.revision.wrapping_add(1);
        if push_capped(&mut self.undo, entry) {
            self.floor = self.floor.saturating_sub(1);
        }
    }

    /// Opens a group: the steps recorded until it closes become one.
    pub(crate) fn begin_group(&mut self) {
        self.groups.push(Vec::new());
    }

    /// Closes the innermost group into one step by `source` named `label`, with what it did to
    /// the current state (`view`). Nothing is recorded when the group changed nothing; a group of
    /// one step records that step under `label`. True when a step was recorded.
    pub(crate) fn commit_group(
        &mut self,
        source: Source,
        label: &str,
        view: Option<Box<ViewDelta>>,
    ) -> bool {
        let mut steps: Vec<Step> = self
            .groups
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.step)
            .collect();
        let step = match (steps.len(), view) {
            (0, None) => return false,
            (1, None) => steps.pop().expect("one step"),
            (_, view) => {
                steps.reverse();
                Step::Group(steps, view)
            }
        };
        self.record(step, source, label);
        true
    }

    /// Closes the innermost group by putting back every step made in it, last first: the session
    /// is as it was when the group opened. Nothing can be redone from it.
    pub(crate) fn abort_group(&mut self, mut parts: Parts) {
        let entries = self.groups.pop().unwrap_or_default();
        for entry in entries.into_iter().rev() {
            let _redo = entry.step.swap(parts.reborrow());
        }
    }

    /// What can be undone (oldest first) and redone (next first): each step's label and who
    /// made it.
    pub(crate) fn labels(&self) -> (Vec<HistoryStep>, Vec<HistoryStep>) {
        let step = |e: &Entry| HistoryStep {
            label: e.label.clone(),
            source: e.source,
        };
        let undo = self.undo.iter().skip(self.floor).map(step).collect();
        let redo = self.redo.iter().rev().map(step).collect();
        (undo, redo)
    }

    /// How many steps can be undone, floor or not.
    pub(crate) fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Undo stops with `floor` steps left (0: never stops).
    pub(crate) fn set_floor(&mut self, floor: usize) {
        self.floor = floor;
    }

    /// Every step made since the floor was set has been undone: the drawing is as it was then
    /// (what could be redone changes nothing).
    pub(crate) fn edit_was_quiet(&self) -> bool {
        self.undo.len() == self.floor
    }

    /// Ends an edit in place: drops the steps above the floor and everything that could be
    /// redone, and with `keep_earlier` false every step below it too. Clears the floor. How many
    /// undo steps went.
    pub(crate) fn close_edit(&mut self, keep_earlier: bool) -> usize {
        let before = self.undo.len();
        self.undo
            .truncate(if keep_earlier { self.floor } else { 0 });
        self.redo.clear();
        self.floor = 0;
        before - self.undo.len()
    }

    /// Who made the change that `undo` would take back.
    pub(crate) fn top_source(&self) -> Option<Source> {
        self.undo.last().map(|entry| entry.source)
    }

    /// Puts the take-off, BOQ, conflicts or drawing back to their state before the last command.
    /// False when there is none.
    pub(crate) fn undo(&mut self, parts: Parts) -> bool {
        if self.undo.len() <= self.floor {
            return false;
        }
        if let Some(Entry {
            step: Step::ProjectFile(change),
            ..
        }) = self.undo.last()
        {
            if change.apply().is_err() {
                return false;
            }
        }
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Entry {
            step: entry.step.swap(parts),
            ..entry
        });
        true
    }

    /// Redoes the last undone command. False when there is none.
    pub(crate) fn redo(&mut self, parts: Parts) -> bool {
        if let Some(Entry {
            step: Step::ProjectFile(change),
            ..
        }) = self.redo.last()
        {
            if change.apply().is_err() {
                return false;
            }
        }
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        let step = entry.step.swap(parts);
        self.push(Entry { step, ..entry });
        true
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.undo.len() > self.floor
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Pushes `step`, dropping the oldest past `MAX_STEPS`; true when one was dropped.
fn push_capped(stack: &mut Vec<Entry>, step: Entry) -> bool {
    let full = stack.len() == MAX_STEPS;
    if full {
        stack.remove(0);
    }
    stack.push(step);
    full
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_at_most_200_steps() {
        let mut history = History::default();
        let (mut takeoff, mut boq, mut conflicts, mut schedules) =
            (Takeoff::default(), None, Vec::new(), Vec::new());
        let (mut doc, mut doc_changed) = (CadDocument::new(), false);
        let (mut current, mut hidden, mut state_hidden) =
            (Current::default(), HashSet::new(), BTreeMap::new());
        for i in 0..MAX_STEPS + 5 {
            let step = if i % 2 == 0 {
                Step::Takeoff(Takeoff::default())
            } else {
                Step::Document(DocChange::default())
            };
            history.record(step, Source::User, "test");
        }
        let undone = std::iter::from_fn(|| {
            let parts = Parts {
                takeoff: &mut takeoff,
                boq: &mut boq,
                conflicts: &mut conflicts,
                schedules: &mut schedules,
                doc: &mut doc,
                view: ViewRefs {
                    current: &mut current,
                    hidden: &mut hidden,
                    state_hidden: &mut state_hidden,
                },
                doc_changed: &mut doc_changed,
            };
            history.undo(parts).then_some(())
        })
        .count();
        assert_eq!(undone, MAX_STEPS);
    }
}
