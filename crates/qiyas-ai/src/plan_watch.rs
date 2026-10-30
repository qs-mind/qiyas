//! Conversation advice; planning never blocks a valid drawing edit.
use crate::{
    prompt::Lang,
    surface::{Plan, PlanStatus},
};
use serde_json::{json, Value};

pub(crate) const STALE_AFTER: usize = 8;
/// At most one factual reminder per user turn, even when the plan changes.
pub(crate) const MAX_NOTES: usize = 1;

#[derive(Default)]
pub(crate) struct PlanWatch {
    plan: Option<Plan>,
    pub(crate) mutations: usize,
    notes: usize,
    /// The plan is this turn's work: it was posted or updated this turn, or the user said to
    /// continue it. An old plan left open does not hold a later question ("what is the area?").
    armed: bool,
}

impl PlanWatch {
    pub(crate) fn update(&mut self, plan: Plan) {
        self.armed = true;
        if self.plan.as_ref() != Some(&plan) {
            self.mutations = 0;
            self.plan = Some(plan);
        }
    }

    pub(crate) fn changed(&mut self) {
        if self.plan.is_some() {
            self.mutations += 1;
        }
    }

    pub(crate) fn unfinished(&self) -> Vec<&crate::surface::PlanPhase> {
        self.plan
            .as_ref()
            .into_iter()
            .flat_map(|p| &p.phases)
            .filter(|p| matches!(p.status, PlanStatus::Pending | PlanStatus::Active))
            .collect()
    }

    /// Phases still to do: pending or active. A skipped phase was set aside on purpose.
    pub(crate) fn open_phases(&self) -> Vec<&crate::surface::PlanPhase> {
        self.plan
            .as_ref()
            .into_iter()
            .flat_map(|p| &p.phases)
            .filter(|p| matches!(p.status, PlanStatus::Pending | PlanStatus::Active))
            .collect()
    }

    /// A new user message starts again: the notes were about the last one.
    pub(crate) fn new_turn(&mut self) {
        self.notes = 0;
        self.armed = false;
    }

    /// The user asked to continue the open plan: it is this turn's work again.
    pub(crate) fn arm(&mut self) {
        self.armed = true;
    }

    /// One factual reminder unless the user stopped; callers handle structured asks.
    pub(crate) fn finish_gate(&mut self, user_stopped: bool, lang: Lang) -> Option<Value> {
        let open = self.open_phases();
        if !self.armed || open.is_empty() || user_stopped || self.notes >= MAX_NOTES {
            return None;
        }
        let list = open
            .iter()
            .map(|p| {
                format!(
                    "{} ({})",
                    p.title,
                    if p.status == PlanStatus::Active {
                        match lang {
                            Lang::En => "active",
                            Lang::Ar => "نشطة",
                        }
                    } else {
                        match lang {
                            Lang::En => "pending",
                            Lang::Ar => "قيد الانتظار",
                        }
                    }
                )
            })
            .collect::<Vec<_>>();
        self.notes += 1;
        let message = match lang {
            Lang::En => "These plan phases remain open. You may finish them, mark them skipped with a reason, or ask the engineer for what blocks them.",
            Lang::Ar => "ما زالت هذه المراحل مفتوحة. يمكنك إكمالها أو تجاوزها مع ذكر السبب أو سؤال المهندس عما يمنع إكمالها.",
        };
        Some(json!({"code":"open_phases","open_phases":list,"message":message}))
    }

    pub(crate) fn stale(&self) -> bool {
        self.mutations >= STALE_AFTER && !self.unfinished().is_empty()
    }

    pub(crate) fn advice(&self) -> Option<Value> {
        self.stale().then(|| json!({"code":"stale_plan", "mutations_since_update":self.mutations,
            "unfinished_phases":self.unfinished().iter().map(|phase| &phase.title).collect::<Vec<_>>(),
            "message":"The plan has not been updated during these edits. / لم يتم تحديث الخطة خلال هذه التعديلات."}))
    }

    pub(crate) fn status(&self, lang: Lang) -> Option<crate::agent::AgentEvent> {
        let items: Vec<_> = self.open_phases().iter().map(|p| p.title.clone()).collect();
        (!items.is_empty()).then(|| crate::agent::status("open_phases", items, lang))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_finish_gate_names_open_phases_once_per_turn() {
        let plan = |a: &str, b: &str| {
            crate::surface::read_plan(&json!({"phases":[{"id":"a","title":"Plans","status":a},{"id":"b","title":"Sections","status":b}]})).unwrap()
        };
        let mut watch = PlanWatch::default();
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "no plan, no gate"
        );
        watch.update(plan("done", "pending"));
        let note = watch.finish_gate(false, Lang::En).unwrap();
        assert_eq!(note["open_phases"], json!(["Sections (pending)"]));
        assert!(
            watch.finish_gate(true, Lang::En).is_none(),
            "the user said stop"
        );
        assert!(watch.finish_gate(false, Lang::Ar).is_none());
        watch.update(plan("active", "pending"));
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "a changed plan cannot reset this turn's note limit"
        );
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "one reminder is enough"
        );
        watch.new_turn();
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "an old open plan does not hold a later question"
        );
        watch.arm();
        assert!(
            watch.finish_gate(false, Lang::En).is_some(),
            "continue makes it this turn's work"
        );
        watch.new_turn();
        watch.update(plan("done", "pending"));
        assert!(
            watch.finish_gate(false, Lang::En).is_some(),
            "a plan posted this turn"
        );
        watch.update(plan("done", "skipped"));
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "skipped is a decision"
        );
        watch.update(plan("done", "done"));
        assert!(watch.finish_gate(false, Lang::En).is_none());
    }

    #[test]
    fn an_identical_plan_does_not_hide_unfinished_work() {
        let plan = crate::surface::read_plan(&json!({"phases":[{"id":"rooms","title":"Rooms","status":"active"},{"id":"boq","title":"BOQ","status":"skipped"}]})).unwrap();
        let mut watch = PlanWatch::default();
        watch.update(plan.clone());
        for _ in 0..STALE_AFTER - 1 {
            watch.changed();
        }
        assert!(!watch.stale());
        watch.update(plan.clone());
        watch.changed();
        assert!(watch.stale());
        assert_eq!(
            serde_json::to_value(watch.status(Lang::En).unwrap()).unwrap()["items"],
            json!(["Rooms"])
        );
        let mut done = plan;
        for p in &mut done.phases {
            p.status = PlanStatus::Done;
        }
        watch.update(done);
        assert_eq!(watch.mutations, 0);
        assert!(watch.status(Lang::Ar).is_none());
    }
}
