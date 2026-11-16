//! The WebClient's calculation entry point (WASM). Runs PlanningManager on one epic's plan (JSON) received from the server
use plan_access::{JpRuleCalendar, JsonPlan};
use plan_contracts::{Change, EstKind, Issue, Plan};
use plan_manager::PlanningManager;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Planner {
    mgr: Option<PlanningManager>,
    err: Vec<Issue>,
}

fn issues_json(e: &[Issue]) -> String { serde_json::json!({ "ok": false, "issues": e }).to_string() }

#[wasm_bindgen]
impl Planner {
    #[wasm_bindgen(constructor)]
    pub fn new(plan_json: String) -> Planner {
        match serde_json::from_str::<Plan>(&plan_json) {
            Ok(p) => Planner { mgr: Some(PlanningManager::new(Arc::new(JsonPlan(p)), Arc::new(JpRuleCalendar))), err: vec![] },
            Err(e) => Planner { mgr: None, err: vec![Issue::error(format!("cannot read the plan JSON: {e}"))] },
        }
    }

    pub fn evaluate(&self) -> String {
        let Some(m) = &self.mgr else { return issues_json(&self.err) };
        match m.evaluate("") { Ok(ev) => serde_json::json!({ "ok": true, "evaluation": ev }).to_string(), Err(e) => issues_json(&e) }
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
