//! People across epics: every epic's default run, its load per person and day
use plan_access::{JpRuleCalendar, SqlitePlans, TomlFile};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[test]
fn every_epic_brings_its_default_run_load() {
    let db = std::env::temp_dir().join(format!("plan-test-loads-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let ed = PlanEditingManager::new(store.clone());
    for k in ["a", "b"] { ed.import(&TomlFile(root().join("examples/sample.toml")), Some(k), "test").unwrap(); }
    // an empty epic is listed too, with no load
    ed.create_epic("broken", "Broken", chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(), "test").unwrap();
    let pm = PlanningManager::new(store.clone(), Arc::new(JpRuleCalendar));
    let ls = pm.epic_loads().unwrap();
    assert_eq!(ls.iter().map(|l| l.epic.as_str()).collect::<Vec<_>>(), ["a", "b", "broken"]);
    let (a, b) = (&ls[0], &ls[1]);
    assert_eq!((a.scenario.as_str(), a.rate, a.est), ("plan1", 1.0, plan_contracts::EstKind::E));
    // the same as the default run of evaluate
    let ev = pm.evaluate("a").unwrap();
    assert_eq!(a.load, ev.runs[0].load);
    assert_eq!(a.load, b.load);
    // one epic alone never puts a person over a whole day at 100% utilization
    assert!(a.load.values().flat_map(|d| d.values()).all(|v| *v <= 1.0 + 1e-9));
    assert!(ls[2].load.is_empty());
    let _ = std::fs::remove_file(&db);
}
