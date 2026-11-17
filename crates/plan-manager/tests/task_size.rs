//! Large tasks: a warning above the epic's max_task_days (default 2 person-days), kept by SQLite and TOML, 0 turns it off
use plan_access::{JpRuleCalendar, SqlitePlans, TomlFile};
use plan_contracts::{EditOp, EpicSettings, Issue};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn large(is: &[Issue]) -> Vec<String> {
    let mut v: Vec<String> = is.iter().filter(|i| i.message.starts_with("large task") && i.scenario.as_deref() == Some("plan1")).filter_map(|i| i.task.clone()).collect();
    v.sort(); v
}

#[test]
fn large_tasks_are_flagged() {
    let db = std::env::temp_dir().join(format!("plan-test-size-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let ed = PlanEditingManager::new(store.clone());
    let pm = PlanningManager::new(store, Arc::new(JpRuleCalendar));
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("a"), "test").unwrap();
    let set = |v: Option<f64>| ed.apply("a", "test", &[EditOp::SetEpic { settings: EpicSettings { max_task_days: Some(v), ..Default::default() } }]);

    // default 2: the sample's 3-day and 5.5-day tasks
    assert_eq!(large(&pm.check("a")), ["API", "TF", "V06"]);
    // a higher limit, kept by SQLite and TOML
    set(Some(4.0)).unwrap();
    assert_eq!(ed.plan("a").unwrap().max_task_days, Some(4.0));
    assert_eq!(large(&pm.check("a")), ["TF"]);
    let toml = plan_access::plan_access::to_toml(&ed.plan("a").unwrap());
    assert!(toml.contains("max_task_days = 4\n"), "{toml}");
    // 0: off; null: back to the default; negative: rejected
    set(Some(0.0)).unwrap();
    assert!(large(&pm.check("a")).is_empty());
    set(None).unwrap();
    assert_eq!(large(&pm.check("a")).len(), 3);
    assert!(set(Some(-1.0)).is_err());
}
