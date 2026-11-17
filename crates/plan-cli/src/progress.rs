//! Recording progress in one step ("started X", "finished X") for the CLI and MCP: find the task, write the actual date, report the effect.
//! The task is found by id or ticket (exact, any case), otherwise by a part of its name; several matches are returned instead of guessing
use plan_contracts::{EditOp, Issue, TaskPatch};
use plan_manager::PlanEditingManager;
use serde_json::{json, Value};

#[derive(Clone, Copy, PartialEq)]
pub enum Step { Start, Finish }

/// Base tasks matching the query: exact id or ticket first, else names containing it. Only not-done tasks unless exact
fn find(ed: &PlanEditingManager, epic: Option<&str>, query: &str) -> Result<Vec<(String, plan_contracts::TaskSpec)>, Vec<Issue>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() { return Err(vec![Issue::error("say which task (its id, ticket or part of its name)")]); }
    let keys: Vec<String> = match epic { Some(e) => vec![e.to_string()], None => ed.epics()?.into_iter().map(|e| e.key).collect() };
    let mut all = vec![];
    for k in keys { for t in ed.plan(&k)?.tasks { all.push((k.clone(), t)); } }
    let exact: Vec<_> = all.iter().filter(|(_, t)| t.id.to_lowercase() == q || (!t.ticket.is_empty() && t.ticket.to_lowercase() == q)).cloned().collect();
    if !exact.is_empty() { return Ok(exact); }
    Ok(all.into_iter().filter(|(_, t)| !t.done && (t.name.to_lowercase().contains(&q) || t.short.to_lowercase().contains(&q))).collect())
}

/// Record a start or a finish. `date` defaults to today; `person` picks whose next task to report
pub fn record(step: Step, query: &str, epic: Option<&str>, date: Option<chrono::NaiveDate>, person: Option<&str>, actor: &str) -> Result<Value, Vec<Issue>> {
    let ed = PlanEditingManager::new(crate::store());
    let found = find(&ed, epic, query)?;
    let (k, t) = match found.as_slice() {
        [one] => one.clone(),
        [] => return Err(vec![Issue::error(format!("no open task matches {query:?}"))]),
        many => return Ok(json!({ "changed": false, "candidates": many.iter().map(|(k, t)| json!({ "epic": k, "id": t.id, "name": t.name, "ticket": t.ticket })).collect::<Vec<_>>(),
            "note": "several tasks match; call again with the task id (and epic)" })),
    };
    let day = date.unwrap_or_else(|| chrono::Local::now().date_naive());
    let patch = match step {
        Step::Start => TaskPatch { actual_start: Some(day), ..Default::default() },
        // a finish without a recorded start: the start stays unknown
        Step::Finish => TaskPatch { done: Some(true), actual_finish: Some(day), ..Default::default() },
    };
    let mut d = crate::apply_data(&k, actor, &[EditOp::SetTask { key: t.id.clone(), patch, scenario: None }])?;
    let who = person.map(str::to_string).or_else(|| t.owners.as_ref().and_then(|o| (o.len() == 1).then(|| o[0].clone())));
    let next = crate::agenda_data(who.as_deref()).map(|a| json!({ "owner": who, "next": a["next"], "then": a["then"] })).unwrap_or(Value::Null);
    let o = d.as_object_mut().unwrap();
    o.insert("changed".into(), json!(true));
    o.insert("task".into(), json!({ "epic": k, "id": t.id, "name": t.name, if step == Step::Start { "actual_start" } else { "actual_finish" }: day }));
    o.insert("next".into(), next);
    Ok(d)
}
