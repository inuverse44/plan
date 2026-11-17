//! The agenda across epics: next task, starting today, due this week, risks
use chrono::NaiveDate;
use plan_access::{JpRuleCalendar, SqlitePlans, TomlFile};
use plan_contracts::{EditOp, EpicSettings, TaskPatch};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn day(s: &str) -> NaiveDate { s.parse().unwrap() }

#[test]
fn agenda_picks_one_next_task_and_flags_late_starts() {
    let db = std::env::temp_dir().join(format!("plan-test-agenda-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let ed = PlanEditingManager::new(store.clone());
    let pm = PlanningManager::new(store, Arc::new(JpRuleCalendar));
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("a"), "test").unwrap();
    ed.apply("a", "test", &[EditOp::SetEpic { settings: EpicSettings { start: Some(day("2026-11-09")), ..Default::default() } }]).unwrap();

    // On the first day: DS starts the plan (critical, ready), API is ready too
    let a = pm.agenda(Some("alice"), day("2026-11-09")).unwrap();
    assert_eq!(a.next.as_ref().unwrap().id, "DS");
    assert_eq!(a.then.as_ref().unwrap().id, "API");
    assert!(a.risks.is_empty(), "{:?}", a.risks);

    // A week later nothing has started: late starts are risks, the earliest critical one is still next
    let a = pm.agenda(Some("alice"), day("2026-11-17")).unwrap();
    assert_eq!(a.next.as_ref().unwrap().id, "DS");
    assert!(a.risks.iter().any(|r| r.task.as_deref() == Some("DS") && r.message.contains("critical")), "{:?}", a.risks);
    assert!(a.start_today.iter().all(|t| t.start <= day("2026-11-17") && !t.started));
    assert!(a.due_this_week.iter().all(|t| t.finish <= day("2026-11-22")));

    // Done tasks leave; a started task is no longer a late start
    let set = |k: &str, p: TaskPatch| ed.apply("a", "test", &[EditOp::SetTask { key: k.into(), patch: p, scenario: None }]).unwrap();
    set("DS", TaskPatch { done: Some(true), actual_start: Some(day("2026-11-09")), actual_finish: Some(day("2026-11-10")), ..Default::default() });
    set("API", TaskPatch { actual_start: Some(day("2026-11-12")), ..Default::default() });
    let a = pm.agenda(Some("alice"), day("2026-11-17")).unwrap();
    assert_ne!(a.next.as_ref().unwrap().id, "DS");
    assert!(!a.risks.iter().any(|r| matches!(r.task.as_deref(), Some("DS") | Some("API"))), "{:?}", a.risks);

    // Someone without tasks: nothing to do; a deadline already passed is an epic risk
    assert!(pm.agenda(Some("nobody"), day("2026-11-17")).unwrap().next.is_none());
    ed.apply("a", "test", &[EditOp::SetEpic { settings: EpicSettings { deadline: Some(Some(day("2026-11-20"))), ..Default::default() } }]).unwrap();
    let a = pm.agenda(None, day("2026-11-17")).unwrap();
    assert!(a.risks.iter().any(|r| r.task.is_none() && r.message.contains("after the deadline")), "{:?}", a.risks);
    // ... shown to alice, who has tasks there, but not to someone who has none
    let late = |who: &str| pm.agenda(Some(who), day("2026-11-17")).unwrap().risks.iter().any(|r| r.task.is_none() && r.message.contains("after the deadline"));
    assert!(late("alice"));
    assert!(!late("nobody"));
}
