//! Resources: things tasks need besides people (an environment, a room…). Old plans (prod = true, calendar.freeze) read as needs = ["prod"] and resources.prod
use plan_access::{JpRuleCalendar, SqlitePlans, TomlFile};
use plan_contracts::*;
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn eval_file(rel: &str) -> Evaluation {
    PlanningManager::new(Arc::new(TomlFile(root().join(rel))), Arc::new(JpRuleCalendar)).evaluate("").unwrap_or_else(|e| panic!("{e:?}"))
}
fn eval_text(text: &str) -> Evaluation {
    PlanningManager::new(Arc::new(plan_access::TomlText(text.to_string())), Arc::new(JpRuleCalendar)).evaluate("").unwrap_or_else(|e| panic!("{e:?}"))
}

fn same(a: &Evaluation, b: &Evaluation) {
    assert_eq!(a.runs.len(), b.runs.len());
    for (x, y) in a.runs.iter().zip(&b.runs) {
        for (s, t) in x.tasks.iter().zip(&y.tasks) { assert_eq!((&s.id, s.es, s.ef, s.tf, &s.needs), (&t.id, t.es, t.ef, t.tf, &t.needs), "{}", x.key); }
    }
}

#[test]
fn the_old_form_schedules_like_the_new_one() {
    let old = eval_file("tests/fixtures/sample-old-form.toml");
    let new = eval_file("examples/sample.toml");
    same(&old, &new);
    assert_eq!(old.resources["prod"].label, "Release freeze");
    assert_eq!(old.resources["prod"].off.len(), 2);
    // written back, the old form is gone
    let text = plan_access::plan_access::to_toml(&plan_access::plan_access::parse(&std::fs::read_to_string(root().join("tests/fixtures/sample-old-form.toml")).unwrap()).unwrap());
    assert!(!text.contains("prod = ") && !text.contains("freeze ="), "{text}");
    assert!(text.contains("needs = [\"prod\"]") && text.contains("[resources.\"prod\"]"), "{text}");
    same(&eval_text(&text), &new);
}

#[test]
fn any_resource_blocks_only_the_tasks_that_need_it() {
    let text = r#"
title = "Rooms"
start = "2026-12-21"
rates = [1.0]
estimates = ["m"]
[calendar]
holidays = "none"
year_end = false
[resources.lab]
label = "Lab closed"
off = [["2026-12-21", "2026-12-22"]]
[people.a]
leave = []
[people.b]
leave = []
[[tasks]]
id = "X"
name = "Experiment"
deliverable = "d"
effort = 1
owner = ["a"]
needs = ["lab"]
[[tasks]]
id = "Y"
name = "Write"
deliverable = "d"
effort = 1
owner = ["b"]
[[tasks]]
id = "Z"
name = "Also in the lab, at the same time as X"
deliverable = "d"
effort = 1
owner = ["b"]
needs = ["lab"]
after = ["Y"]
"#;
    let ev = eval_text(text);
    let r = &ev.runs[0];
    let t = |id: &str| r.tasks.iter().find(|t| t.id == id).unwrap();
    let day = |s: usize| ev.calendar.days[s / 2].to_string();
    assert_eq!(day(t("X").es), "2026-12-23");
    assert_eq!(t("X").why, "lab unavailable");
    assert_eq!(day(t("Y").es), "2026-12-21", "Y does not need the lab");
    // a resource has no capacity: X and Z use the lab on the same day
    assert_eq!(day(t("Z").es), "2026-12-23");
}

#[test]
fn a_need_that_is_not_a_resource_is_a_warning() {
    let mut p = plan_access::plan_access::parse(&std::fs::read_to_string(root().join("examples/sample.toml")).unwrap()).unwrap();
    p.tasks[0].needs = vec!["db".into()];
    let is = plan_engines::analysis::inspect(&p, &plan_engines::scenario::build(&p).unwrap());
    assert!(is.iter().any(|i| i.message.contains("needs db")), "{is:?}");
}

#[test]
fn old_sqlite_rows_and_old_edits_are_moved_to_needs() {
    let db = std::env::temp_dir().join(format!("plan-test-resources-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    {
        let store = Arc::new(SqlitePlans::open(&db).unwrap());
        PlanEditingManager::new(store).import(&TomlFile(root().join("examples/sample.toml")), Some("eol"), "test").unwrap();
    }
    // Make it look like a database written before resources: prod = 1, the freeze table, no needs, no resources
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch("UPDATE tasks SET prod = 1, needs = NULL WHERE needs IS NOT NULL;
            INSERT INTO freeze SELECT start, finish FROM resource_off; DELETE FROM resource_off; DELETE FROM resources;").unwrap();
    }
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let pm = PlanningManager::new(store.clone(), Arc::new(JpRuleCalendar));
    same(&pm.evaluate("eol").unwrap(), &eval_file("examples/sample.toml"));

    // An edit in the old form ({"prod": true}) still works and is kept as needs
    let ed = PlanEditingManager::new(store.clone());
    let ops: Vec<EditOp> = serde_json::from_str(r#"[{"op":"set_task","key":"DS","patch":{"prod":true}},{"op":"set_task","key":"CUT","patch":{"prod":false}}]"#).unwrap();
    ed.apply("eol", "test", &ops).unwrap();
    let p = ed.plan("eol").unwrap();
    assert_eq!(p.tasks.iter().find(|t| t.id == "DS").unwrap().needs, vec!["prod"]);
    assert!(p.tasks.iter().find(|t| t.id == "CUT").unwrap().needs.is_empty());
    let _ = std::fs::remove_file(&db);
}

/// Firestore and the HTTP API hold plans as JSON: an old document (prod: true, calendar.freeze) reads as the new form
#[test]
fn old_json_reads_as_the_new_form() {
    let new = plan_access::plan_access::parse(&std::fs::read_to_string(root().join("examples/sample.toml")).unwrap()).unwrap();
    let mut v = serde_json::to_value(&new).unwrap();
    v["calendar"]["freeze"] = serde_json::to_value(&new.resources["prod"].off).unwrap();
    v.as_object_mut().unwrap().remove("resources");
    for t in v["tasks"].as_array_mut().unwrap() {
        if let Some(n) = t.as_object_mut().unwrap().remove("needs") { assert_eq!(n, serde_json::json!(["prod"])); t["prod"] = true.into(); }
    }
    let mut old: Plan = serde_json::from_value(v).unwrap();
    old.normalize();
    assert_eq!(old.resources, new.resources);
    assert_eq!(old.tasks.iter().map(|t| t.needs.clone()).collect::<Vec<_>>(), new.tasks.iter().map(|t| t.needs.clone()).collect::<Vec<_>>());
    // and it is written in the new form only
    let out = serde_json::to_value(&old).unwrap();
    assert!(out["calendar"].get("freeze").is_none() && out["tasks"].as_array().unwrap().iter().all(|t| t.get("prod").is_none()), "{out}");
}
