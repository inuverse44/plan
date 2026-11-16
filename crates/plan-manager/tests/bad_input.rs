//! Input that used to crash, cut tasks short or lose data: each comes back as an issue instead
use plan_access::{JpRuleCalendar, JsonPlan, PlanAccess, SqlitePlans, TomlFile};
use plan_contracts::*;
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn sample() -> Plan { TomlFile(root().join("examples/sample.toml")).read_plan("").unwrap() }
fn planning(p: Plan) -> PlanningManager { PlanningManager::new(Arc::new(JsonPlan(p)), Arc::new(JpRuleCalendar)) }
fn first_scenario() -> String { sample().scenarios[0].key.clone() }
fn change(v: serde_json::Value) -> Vec<Change> { serde_json::from_value(v).unwrap() }

#[test]
fn saving_a_start_past_the_calendar_is_an_issue() {
    let r = planning(sample()).changes_to_ops("", &first_scenario(), 1.0, EstKind::E, &change(serde_json::json!([{ "id": "SPEC", "min_start": 100_000 }])));
    let e = r.unwrap_err();
    assert_eq!(e[0].task.as_deref(), Some("SPEC"), "{e:?}");
}

#[test]
fn a_huge_duration_is_an_issue() {
    let e = planning(sample()).what_if("", &first_scenario(), 1.0, EstKind::E, &change(serde_json::json!([{ "id": "SPEC", "dur": 1u64 << 62 }]))).unwrap_err();
    assert_eq!(e[0].task.as_deref(), Some("SPEC"), "{e:?}");
}

#[test]
fn a_task_past_the_calendar_is_an_issue() {
    // 2000 half-days is longer than the calendar (500 business days)
    let e = planning(sample()).what_if("", &first_scenario(), 1.0, EstKind::E, &change(serde_json::json!([{ "id": "SPEC", "dur": 2000 }]))).unwrap_err();
    assert!(e[0].message.contains("calendar"), "{e:?}");
}

#[test]
fn a_rate_of_zero_is_an_issue() {
    let mut p = sample();
    p.people.insert("someone".into(), Person { leave: vec![], rate: Some(0.0) });
    p.rates.push(0.0);
    let e = planning(p.clone()).evaluate("").unwrap_err();
    assert!(e.iter().any(|i| i.message.starts_with("rates: 0")), "{e:?}");
    assert!(e.iter().any(|i| i.message.starts_with("rate of someone: 0")), "{e:?}");
    // what-if does not lint, but still must not crash
    assert!(planning(p).what_if("", &first_scenario(), 0.0, EstKind::E, &[]).is_err());
}

fn store(name: &str) -> (PathBuf, Arc<SqlitePlans>, PlanEditingManager) {
    let db = std::env::temp_dir().join(format!("plan-test-bad-{name}-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let s = Arc::new(SqlitePlans::open(&db).unwrap());
    (db, s.clone(), PlanEditingManager::new(s))
}

fn edit(ed: &PlanEditingManager, epic: &str, v: serde_json::Value) { ed.apply(epic, "test", &serde_json::from_value::<Vec<EditOp>>(v).unwrap()).unwrap(); }
/// The task as the scenario "alt" schedules it
fn in_alt(s: &Arc<SqlitePlans>, epic: &str, id: &str) -> TaskView {
    let ev = PlanningManager::new(s.clone(), Arc::new(JpRuleCalendar)).evaluate(epic).unwrap_or_else(|e| panic!("{e:?}"));
    let run = ev.runs.iter().find(|r| r.scenario == "alt").unwrap();
    run.tasks.iter().find(|t| t.id == id).unwrap().clone()
}

#[test]
fn in_a_scenario_clear_then_set_keeps_the_new_value() {
    let (_db, s, ed) = store("clear");
    ed.import(&JsonPlan(sample()), Some("eol"), "test").unwrap();
    edit(&ed, "eol", serde_json::json!([
        { "op": "clone_scenario", "from": null, "key": "alt", "name": "Alt" },
        { "op": "set_task", "key": "SPEC", "scenario": "alt", "patch": { "clear": ["effort"] } },
    ]));
    assert_eq!(in_alt(&s, "eol", "SPEC").kind, Kind::Milestone);
    edit(&ed, "eol", serde_json::json!([{ "op": "set_task", "key": "SPEC", "scenario": "alt", "patch": { "effort": 3 } }]));
    let t = in_alt(&s, "eol", "SPEC");
    assert_eq!((t.kind, t.val), (Kind::Work, 3.0));
    edit(&ed, "eol", serde_json::json!([{ "op": "set_task", "key": "SPEC", "scenario": "alt", "patch": { "clear": ["effort"] } }]));
    assert_eq!(in_alt(&s, "eol", "SPEC").kind, Kind::Milestone);
}

/// The same patches give the same task whether they edit the base task or a scenario
#[test]
fn a_scenario_edit_matches_a_base_edit() {
    let (_db, s, ed) = store("parity");
    let patches = [
        serde_json::json!({ "effort": 1 }),                         // a wait becomes work
        serde_json::json!({ "owners": ["x"], "not_before": "2026-12-01" }),
        serde_json::json!({ "clear": ["not_before"], "name": "renamed" }),
        serde_json::json!({ "wait": 2 }),                           // and back to a wait
    ];
    for key in ["base", "scen"] {
        ed.import(&JsonPlan(sample()), Some(key), "test").unwrap();
        edit(&ed, key, serde_json::json!([{ "op": "clone_scenario", "from": null, "key": "alt", "name": "Alt" }]));
    }
    for p in &patches {
        edit(&ed, "base", serde_json::json!([{ "op": "set_task", "key": "DSM", "patch": p }]));
        edit(&ed, "scen", serde_json::json!([{ "op": "set_task", "key": "DSM", "scenario": "alt", "patch": p }]));
        let (a, b) = (in_alt(&s, "base", "DSM"), in_alt(&s, "scen", "DSM"));
        assert_eq!((a.kind, a.val, &a.owners, a.release, &a.name), (b.kind, b.val, &b.owners, b.release, &b.name), "after {p}");
    }
}

#[test]
fn import_refuses_a_plan_with_errors() {
    let (_db, _s, ed) = store("import");
    let mut p = sample();
    let last = p.tasks.last().unwrap().id.clone();
    p.tasks[0].after.push(last); // a loop
    assert!(ed.import(&JsonPlan(p), Some("loop"), "test").is_err());
    assert!(ed.import(&JsonPlan(sample()), Some("a/b"), "test").is_err());
    assert!(ed.epics().unwrap().is_empty());
}

#[test]
fn an_unreadable_row_is_an_error_and_is_not_erased() {
    let (db, s, ed) = store("row");
    ed.import(&JsonPlan(sample()), Some("eol"), "test").unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute("UPDATE tasks SET effort = 'oops' WHERE epic = 'eol' AND key = 'SPEC'", []).unwrap();
    let e = s.read_plan("eol").unwrap_err();
    assert!(e[0].message.contains("oops"), "{e:?}");
    let edit = serde_json::from_value::<Vec<EditOp>>(serde_json::json!([{ "op": "set_task", "key": "DS", "patch": { "note": "x" } }])).unwrap();
    assert!(ed.apply("eol", "test", &edit).is_err());
    let n: i64 = c.query_row("SELECT COUNT(*) FROM tasks WHERE epic = 'eol' AND key = 'SPEC'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}
