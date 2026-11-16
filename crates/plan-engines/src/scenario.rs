//! ScenarioEngine. Encapsulates how scenarios are built; today: drop, set and add applied to the base tasks
use plan_contracts::{Issue, Plan, Scenario, TaskPatch, TaskSpec};

pub fn apply_patch(t: &mut TaskSpec, p: &TaskPatch) { apply(t, p) }

pub fn build(plan: &Plan) -> Result<Vec<Scenario>, Vec<Issue>> {
    if plan.scenarios.is_empty() {
        return Ok(vec![Scenario { key: "base".into(), name: "Base".into(), note: String::new(), tasks: plan.tasks.clone() }]);
    }
    let mut out = Vec::new();
    let mut errs = Vec::new();
    for s in &plan.scenarios {
        let mut ts: Vec<TaskSpec> = plan.tasks.iter().filter(|t| !s.drop.contains(&t.id)).cloned().collect();
        for (tid, patch) in &s.set {
            match ts.iter_mut().find(|t| &t.id == tid) {
                Some(t) => apply(t, patch),
                None => errs.push(Issue::error(format!("set: no task {tid}")).at(Some(&s.key), None)),
            }
        }
        for a in &s.add {
            if ts.iter().any(|t| t.id == a.id) {
                errs.push(Issue::error(format!("add: task {} already exists in the base (use set to change it)", a.id)).at(Some(&s.key), None));
            } else {
                ts.push(a.clone());
            }
        }
        out.push(Scenario { key: s.key.clone(), name: s.name.clone(), note: s.note.clone(), tasks: ts });
    }
    if errs.is_empty() { Ok(out) } else { Err(errs) }
}

fn apply(t: &mut TaskSpec, p: &TaskPatch) {
    macro_rules! set { ($($f:ident),*) => { $( if let Some(v) = &p.$f { t.$f = v.clone(); } )* } }
    set!(story, name, short, ticket, deliverable, group, note, after, needs);
    if let Some(v) = &p.done { t.done = *v; }
    // An effort replaces a wait and the other way round, as an edit of the base task does (plan_access::mutate)
    if p.effort.is_some() { t.effort = p.effort.clone(); t.wait = None; }
    if p.wait.is_some() { t.wait = p.wait.clone(); t.effort = None; }
    if p.owners.is_some() { t.owners = p.owners.clone(); }
    if p.not_before.is_some() { t.not_before = p.not_before; }
    if p.fixed.is_some() { t.fixed = p.fixed; }
    if p.actual_start.is_some() { t.actual_start = p.actual_start; }
    if p.actual_finish.is_some() { t.actual_finish = p.actual_finish; }
    for c in &p.clear {
        match c.as_str() {
            "actual_start" => t.actual_start = None,
            "actual_finish" => t.actual_finish = None,
            "not_before" => t.not_before = None,
            "fixed" => t.fixed = None,
            "effort" => t.effort = None,
            "wait" => t.wait = None,
            "owners" | "owner" => t.owners = None,
            _ => {}
        }
    }
}
