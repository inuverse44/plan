//! SQLite に取り込んでも結果が変わらないか、編集が効くか、誤りのある編集を拒むか
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
    // dump → 取り込み直しても同じ
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
    // 輪になるつなぎ方は拒む（STOP は DS の後ろ）
    let r = ed.apply("eol", "test", &[EditOp::Link { task: "DS".into(), pred: "STOP".into() }]);
    assert!(r.is_err(), "輪を拒まなかった");
    assert_eq!(store.version(), v0, "拒んだのに記録された");
    // 足す・つなぐ・案を複製する
    ed.apply("eol", "test", &[
        EditOp::AddTask { task: TaskSpec { id: "DOC".into(), name: "手順書".into(), deliverable: "手順書".into(), effort: Some(Estimate::One(1.0)), ..Default::default() } },
        EditOp::Link { task: "DOC".into(), pred: "CUT".into() },
        EditOp::CloneScenario { from: Some("plan4".into()), key: "plan5".into(), name: "案 5".into() },
        EditOp::SetTask { key: "SPEC".into(), patch: TaskPatch { effort: Some(Estimate::One(2.0)), ..Default::default() }, scenario: Some("plan5".into()) },
    ]).unwrap();
    let ev = pm.evaluate("eol").unwrap();
    assert_eq!(ev.scenarios.len(), 4);
    let r5 = ev.runs.iter().find(|r| r.key == "plan5|1|e").unwrap();
    assert!(r5.tasks.iter().any(|t| t.id == "DOC" && t.after == vec!["CUT".to_string()]));
    assert_eq!(r5.tasks.iter().find(|t| t.id == "SPEC").unwrap().val, 2.0);
    let r4 = ev.runs.iter().find(|r| r.key == "plan4|1|e").unwrap();
    assert_eq!(r4.tasks.iter().find(|t| t.id == "SPEC").unwrap().val, 1.0, "案 4 は変わらない");
    // 記録が 4 行増えた
    assert_eq!(store.version() - v0, 4);
    let h = ed.history(Some("eol"), 10).unwrap();
    assert_eq!(h[0].op, "set_task");
    // 締め切りと比べる作業は消せない
    assert!(ed.apply("eol", "test", &[EditOp::DeleteTask { key: "CUT".into() }]).is_err());
    // 消すと依存も外れる
    ed.apply("eol", "test", &[EditOp::DeleteTask { key: "WATCH".into() }]).unwrap();
    let p = ed.plan("eol").unwrap();
    assert!(p.tasks.iter().all(|t| !t.after.contains(&"WATCH".to_string())));
}
