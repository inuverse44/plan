//! Owners of the epic itself: set with set_epic, kept by SQLite and TOML, shown in the epic list
use plan_access::{SqlitePlans, TomlFile};
use plan_contracts::{EditOp, EpicSettings};
use plan_manager::PlanEditingManager;
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[test]
fn epic_owners_round_trip() {
    let db = std::env::temp_dir().join(format!("plan-test-owners-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let ed = PlanEditingManager::new(Arc::new(SqlitePlans::open(&db).unwrap()));
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("a"), "test").unwrap();
    // none at first
    assert!(ed.plan("a").unwrap().owners.is_empty());
    assert!(ed.epics().unwrap()[0].owners.is_empty());

    // set (blanks dropped), then read back from SQLite and the list
    let set = |o: &[&str]| ed.apply("a", "test", &[EditOp::SetEpic { settings: EpicSettings { owners: Some(o.iter().map(|x| x.to_string()).collect()), ..Default::default() } }]).unwrap();
    set(&["alice", " ", "bob"]);
    assert_eq!(ed.plan("a").unwrap().owners, ["alice", "bob"]);
    assert_eq!(ed.epics().unwrap()[0].owners, ["alice", "bob"]);
    // other settings leave them alone
    ed.apply("a", "test", &[EditOp::SetEpic { settings: EpicSettings { name: Some("Renamed".into()), ..Default::default() } }]).unwrap();
    assert_eq!(ed.plan("a").unwrap().owners, ["alice", "bob"]);

    // TOML keeps them
    let toml = plan_access::plan_access::to_toml(&ed.plan("a").unwrap());
    assert!(toml.contains("owners = [\"alice\", \"bob\"]"), "{toml}");
    let f = std::env::temp_dir().join(format!("plan-test-owners-{}.toml", std::process::id()));
    std::fs::write(&f, &toml).unwrap();
    ed.import(&TomlFile(f.clone()), Some("b"), "test").unwrap();
    assert_eq!(ed.plan("b").unwrap().owners, ["alice", "bob"]);

    // cleared with an empty list
    set(&[]);
    assert!(ed.plan("a").unwrap().owners.is_empty());
    let _ = std::fs::remove_file(&db);
    let _ = std::fs::remove_file(&f);
}
