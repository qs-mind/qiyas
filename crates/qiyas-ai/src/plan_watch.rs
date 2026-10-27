//! Conversation advice; planning never blocks a valid drawing edit.
use crate::{
    prompt::Lang,
    surface::{Plan, PlanStatus},
};
use serde_json::{json, Value};

pub(crate) const STALE_AFTER: usize = 8;
/// A finish is refused this many times for the same open plan; after that the model may end its
/// turn, listing honestly what is not done (a plan it cannot or will not finish must not trap it).
pub(crate) const MAX_REFUSALS: usize = 2;

#[derive(Default)]
pub(crate) struct PlanWatch {
    plan: Option<Plan>,
    pub(crate) mutations: usize,
    refusals: usize,
    /// The plan is this turn's work: it was posted or updated this turn, or the user said to
    /// continue it. An old plan left open does not hold a later question ("what is the area?").
    armed: bool,
}

impl PlanWatch {
    pub(crate) fn update(&mut self, plan: Plan) {
        self.armed = true;
        if self.plan.as_ref() != Some(&plan) {
            self.mutations = 0;
            self.refusals = 0;
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
            .filter(|p| p.status != PlanStatus::Done)
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

    /// A new user message starts again: the refusals were about the last one.
    pub(crate) fn new_turn(&mut self) {
        self.refusals = 0;
        self.armed = false;
    }

    /// The user asked to continue the open plan: it is this turn's work again.
    pub(crate) fn arm(&mut self) {
        self.armed = true;
    }

    /// The finish gate. `Some(advice)` refuses ending the work while plan phases are open,
    /// unless the user asked to stop or the model has already been refused `MAX_REFUSALS`
    /// times (then it must end honestly). Real limits (budget, provider, cancel) never come here.
    pub(crate) fn finish_gate(&mut self, user_stopped: bool, lang: Lang) -> Option<Value> {
        let open = self.open_phases();
        if !self.armed || open.is_empty() || user_stopped || self.refusals >= MAX_REFUSALS {
            return None;
        }
        let list = open
            .iter()
            .map(|p| {
                format!(
                    "{} ({})",
                    p.title,
                    if p.status == PlanStatus::Active {
                        "active"
                    } else {
                        "pending"
                    }
                )
            })
            .collect::<Vec<_>>();
        self.refusals += 1;
        let message = match lang {
            Lang::En => "Not finished: plan phases are still open. The user asked for the whole job, so continue with them now on your stated assumptions. Missing verified data is not a reason to stop: draw on written assumptions and mark the sheet For review. If a phase truly cannot be done, mark it skipped and say why in its title. Your final answer must list what is done and what is not, honestly.",
            Lang::Ar => "لم ينتهِ العمل: ما زالت مراحل من الخطة مفتوحة. طلب المستخدم العمل كله، فتابعها الآن بافتراضاتك المعلنة. نقص البيانات الموثقة ليس سببًا للتوقف: ارسم على افتراضات مكتوبة وضع على اللوحة «للمراجعة». وإن استحال إنجاز مرحلة فعلًا فاجعلها skipped واذكر السبب في عنوانها. يجب أن تذكر إجابتك الأخيرة بصدق ما أُنجز وما لم يُنجز.",
        };
        Some(
            json!({"code":"open_phases","open_phases":list,"refusal":self.refusals,"message":message}),
        )
    }

    pub(crate) fn stale(&self) -> bool {
        self.mutations >= STALE_AFTER && !self.unfinished().is_empty()
    }

    pub(crate) fn advice(&self) -> Option<Value> {
        self.stale().then(|| json!({"code":"stale_plan", "mutations_since_update":self.mutations,
            "unfinished_phases":self.unfinished().iter().map(|phase| &phase.title).collect::<Vec<_>>(),
            "message":"Review these unfinished phases against the user's brief. Mizan decides when to update the plan."}))
    }

    pub(crate) fn final_text(&self, lang: Lang) -> Option<String> {
        let phases = self.unfinished();
        if phases.is_empty() {
            return None;
        }
        let title = match lang {
            Lang::En => "Unfinished phases",
            Lang::Ar => "مراحل لم تكتمل",
        };
        Some(format!(
            "\n\n{title}:\n{}",
            phases
                .iter()
                .map(|p| format!("- {}", p.title))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_finish_gate_names_open_phases_and_lets_go_after_two_refusals_or_a_stop() {
        let plan = |a: &str, b: &str| {
            crate::surface::read_plan(&json!({"phases":[{"id":"a","title":"Plans","status":a},{"id":"b","title":"Sections","status":b}]})).unwrap()
        };
        let mut watch = PlanWatch::default();
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "no plan, no gate"
        );
        watch.update(plan("done", "pending"));
        let refusal = watch.finish_gate(false, Lang::En).unwrap();
        assert_eq!(refusal["open_phases"], json!(["Sections (pending)"]));
        assert!(
            watch.finish_gate(true, Lang::En).is_none(),
            "the user said stop"
        );
        assert!(watch.finish_gate(false, Lang::Ar).is_some());
        assert!(
            watch.finish_gate(false, Lang::En).is_none(),
            "two refusals are enough"
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
        assert!(watch.final_text(Lang::En).unwrap().contains("BOQ"));
        let mut done = plan;
        for p in &mut done.phases {
            p.status = PlanStatus::Done;
        }
        watch.update(done);
        assert_eq!(watch.mutations, 0);
        assert!(watch.final_text(Lang::Ar).is_none());
    }
}
