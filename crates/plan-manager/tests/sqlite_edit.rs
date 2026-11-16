//! Importing into SQLite keeps the results, edits take effect, and invalid edits are refused
use plan_access::{JpRuleCalendar, PlanAccess, SqlitePlans, TomlFile};
use plan_contracts::*;
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

fn setup(name: &str) -> (Arc<SqlitePlans>, PlanEditingManager, PlanningManager) {
    let db = std::env::temp_dir().join(format!("plan-test-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let ed = PlanEditingManager::new(store.clone());
    let pm = PlanningManager::new(store.clone(), Arc::new(JpRuleCalendar));
    (store, ed, pm)
}

#[test]
fn import_then_evaluate_matches_toml() {
    let (_s, ed, pm) = setup("import");
    let key = ed.import(&TomlFile(root().join("examples/sample.toml")), Some("eol"), "test").unwrap();
    assert_eq!(key, "eol");
    let from_db = pm.evaluate("eol").unwrap();
    let from_toml = PlanningManager::new(Arc::new(TomlFile(root().join("examples/sample.toml"))), Arc::new(JpRuleCalendar)).evaluate("").unwrap();
    assert_eq!(from_db.runs.len(), from_toml.runs.len());
    for (a, b) in from_db.runs.iter().zip(&from_toml.runs) {
        assert_eq!(a.key, b.key);
        for (x, y) in a.tasks.iter().zip(&b.tasks) { assert_eq!((x.id.as_str(), x.es, x.ef, x.tf), (y.id.as_str(), y.es, y.ef, y.tf), "{}", a.key); }
    }
    // dump → import again gives the same result
    let text = plan_access::plan_access::to_toml(&ed.plan("eol").unwrap());
    let again = plan_access::plan_access::parse(&text).unwrap();
    assert_eq!(again.tasks.len(), ed.plan("eol").unwrap().tasks.len());
    assert_eq!(again.scenarios.len(), 3);
}

#[test]
fn edits_apply_and_bad_edits_are_rejected() {
    let (store, ed, pm) = setup("edit");
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("eol"), "test").unwrap();
    let v0 = store.version();
    // A link that makes a loop is refused (STOP comes after DS)
    let r = ed.apply("eol", "test", &[EditOp::Link { task: "DS".into(), pred: "STOP".into() }]);
    assert!(r.is_err(), "the loop was not refused");
    assert_eq!(store.version(), v0, "a refused edit was recorded");
    // Add, link, clone a scenario
    ed.apply("eol", "test", &[
        EditOp::AddTask { task: TaskSpec { id: "DOC".into(), name: "Runbook".into(), deliverable: "Runbook".into(), effort: Some(Estimate::One(1.0)), ..Default::default() } },
        EditOp::Link { task: "DOC".into(), pred: "CUT".into() },
        EditOp::CloneScenario { from: Some("plan4".into()), key: "plan5".into(), name: "Plan 5".into() },
        EditOp::SetTask { key: "SPEC".into(), patch: TaskPatch { effort: Some(Estimate::One(2.0)), ..Default::default() }, scenario: Some("plan5".into()) },
    ]).unwrap();
    let ev = pm.evaluate("eol").unwrap();
    assert_eq!(ev.scenarios.len(), 4);
    let r5 = ev.runs.iter().find(|r| r.key == "plan5|1|e").unwrap();
    assert!(r5.tasks.iter().any(|t| t.id == "DOC" && t.after == vec!["CUT".to_string()]));
    assert_eq!(r5.tasks.iter().find(|t| t.id == "SPEC").unwrap().val, 2.0);
    let r4 = ev.runs.iter().find(|r| r.key == "plan4|1|e").unwrap();
    assert_eq!(r4.tasks.iter().find(|t| t.id == "SPEC").unwrap().val, 1.0, "plan 4 is unchanged");
    // four history entries were added
    assert_eq!(store.version() - v0, 4);
    let h = ed.history(Some("eol"), 10).unwrap();
    assert_eq!(h[0].op, "set_task");
    // The deadline task cannot be deleted
    assert!(ed.apply("eol", "test", &[EditOp::DeleteTask { key: "CUT".into() }]).is_err());
    // Deleting a task removes its links
    ed.apply("eol", "test", &[EditOp::DeleteTask { key: "WATCH".into() }]).unwrap();
    let p = ed.plan("eol").unwrap();
    assert!(p.tasks.iter().all(|t| !t.after.contains(&"WATCH".to_string())));
}
