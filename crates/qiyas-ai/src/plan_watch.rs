//! Conversation advice; planning never blocks a valid drawing edit.
use crate::{
    prompt::Lang,
    surface::{Plan, PlanStatus},
};
use serde_json::{json, Value};

pub(crate) const STALE_AFTER: usize = 8;

#[derive(Default)]
pub(crate) struct PlanWatch {
    plan: Option<Plan>,
    pub(crate) mutations: usize,
}

impl PlanWatch {
    pub(crate) fn update(&mut self, plan: Plan) {
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
            .filter(|p| p.status != PlanStatus::Done)
            .collect()
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
