//! The WebClient's calculation entry point (WASM). Runs PlanningManager on one epic's plan (JSON) received from the server
use plan_access::{JpRuleCalendar, JsonPlan};
use plan_contracts::{Change, EstKind, Issue, Plan, Status};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Planner {
    mgr: Option<PlanningManager>,
    ed: Option<PlanEditingManager>,
    err: Vec<Issue>,
}

fn issues_json(e: &[Issue]) -> String { serde_json::json!({ "ok": false, "issues": e }).to_string() }

#[wasm_bindgen]
impl Planner {
    #[wasm_bindgen(constructor)]
    pub fn new(plan_json: String) -> Planner {
        match serde_json::from_str::<Plan>(&plan_json) {
            Ok(p) => {
                let store = Arc::new(JsonPlan(p));
                Planner { mgr: Some(PlanningManager::new(store.clone(), Arc::new(JpRuleCalendar))), ed: Some(PlanEditingManager::new(store)), err: vec![] }
            }
            Err(e) => Planner { mgr: None, ed: None, err: vec![Issue::error(format!("cannot read the plan JSON: {e}"))] },
        }
    }

    pub fn evaluate(&self) -> String {
        let Some(m) = &self.mgr else { return issues_json(&self.err) };
        match m.evaluate("") { Ok(ev) => serde_json::json!({ "ok": true, "evaluation": ev }).to_string(), Err(e) => issues_json(&e) }
    }

    /// The edits that move a task to a status (todo, doing, review, blocked, done); today is YYYY-MM-DD, date empty for today
    pub fn status_ops(&self, task: &str, to: &str, date: &str, today: &str, note: &str) -> String {
        let Some(ed) = &self.ed else { return issues_json(&self.err) };
        let Some(to) = Status::parse(to) else { return issues_json(&[Issue::error(format!("unknown status {to}"))]) };
        let day = |s: &str| s.parse::<chrono::NaiveDate>().ok();
        let Some(today) = day(today) else { return issues_json(&[Issue::error(format!("today must be YYYY-MM-DD: {today}"))]) };
        match ed.status_ops("", task, to, day(date), today, note) {
            Ok(ops) => serde_json::json!({ "ok": true, "ops": ops }).to_string(),
            Err(e) => issues_json(&e),
        }
    }

    pub fn what_if(&self, scenario: &str, rate: f64, est: &str, changes: &str) -> String {
        let Some(m) = &self.mgr else { return issues_json(&self.err) };
        let ch: Vec<Change> = match serde_json::from_str(changes) { Ok(c) => c, Err(e) => return issues_json(&[Issue::error(e.to_string())]) };
        match m.what_if("", scenario, rate, EstKind::parse(est).unwrap_or(EstKind::E), &ch) {
            Ok(r) => serde_json::json!({ "ok": true, "run": r }).to_string(),
            Err(e) => issues_json(&e),
        }
    }
}
