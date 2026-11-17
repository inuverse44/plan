//! Edits against a stale version are rejected; versions are per epic
use plan_access::{SqlitePlans, TomlFile};
use plan_contracts::{EditOp, TaskPatch};
use plan_manager::PlanEditingManager;
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn note(k: &str, n: &str) -> Vec<EditOp> { vec![EditOp::SetTask { key: k.into(), patch: TaskPatch { note: Some(n.into()), ..Default::default() }, scenario: None }] }

#[test]
fn stale_base_is_a_conflict() {
    let db = std::env::temp_dir().join(format!("plan-test-conflict-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let ed = PlanEditingManager::new(Arc::new(SqlitePlans::open(&db).unwrap()));
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("a"), "test").unwrap();
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("b"), "test").unwrap();
    let (v0, _) = ed.epic_version("a").unwrap();

    // someone else (mcp) edits a; an edit based on the old version fails and writes nothing
    ed.apply_at("a", "mcp", &note("DS", "from mcp"), Some(v0)).unwrap();
    let (v1, by) = ed.epic_version("a").unwrap();
    assert!(v1 > v0); assert_eq!(by, "mcp");
    let e = ed.apply_at("a", "web", &note("DS", "from web"), Some(v0)).unwrap_err();
    assert!(e[0].message.starts_with("conflict:") && e[0].message.contains("by mcp"), "{e:?}");
    assert_eq!(ed.plan("a").unwrap().tasks.iter().find(|t| t.id == "DS").unwrap().note, "from mcp");

    // edits to another epic do not move a's version; no base means no check
    let (vb, _) = ed.epic_version("b").unwrap();
    ed.apply_at("b", "cli", &note("DS", "b"), Some(vb)).unwrap();
    assert_eq!(ed.epic_version("a").unwrap().0, v1);
    ed.apply_at("a", "web", &note("DS", "from web"), Some(v1)).unwrap();
    ed.apply_at("a", "cli", &note("DS", "no base"), None).unwrap();
}
