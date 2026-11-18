//! Task status (#48): moving tasks through statuses writes dates and holds, stories and the epic roll up,
//! the agenda leaves held tasks out of "next" and reports them, and holds survive SQLite and TOML
use chrono::NaiveDate;
use plan_access::{JpRuleCalendar, PlanAccess, SqlitePlans, TomlFile};
use plan_contracts::{EditOp, EpicSettings, HoldKind, Status, Story, TaskPatch};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn day(s: &str) -> NaiveDate { s.parse().unwrap() }

#[test]
fn statuses_roll_up_and_show_in_the_agenda() {
    let db = std::env::temp_dir().join(format!("plan-test-status-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&db);
    let store = Arc::new(SqlitePlans::open(&db).unwrap());
    let ed = PlanEditingManager::new(store.clone());
    let pm = PlanningManager::new(store.clone(), Arc::new(JpRuleCalendar));
    ed.import(&TomlFile(root().join("examples/sample.toml")), Some("s"), "test").unwrap();
    ed.apply("s", "test", &[EditOp::SetEpic { settings: EpicSettings { start: Some(day("2026-11-09")), ..Default::default() } }]).unwrap();
    let today = day("2026-11-17");
    let mv = |task: &str, to: Status, note: &str| { let ops = ed.status_ops("s", task, to, None, today, note).unwrap(); ed.apply("s", "test", &ops).unwrap(); };
    let run = || pm.evaluate("s").unwrap().runs.remove(0);
    let status = |id: &str| run().tasks.iter().find(|t| t.id == id).unwrap().status;

    assert_eq!(run().status, Status::Todo);
    // a story with DS and SPEC; an empty story stays to do
    ed.apply("s", "test", &[
        EditOp::SetStory { story: Story { key: "S1".into(), name: "Start".into(), ..Default::default() } },
        EditOp::SetStory { story: Story { key: "S2".into(), name: "Empty".into(), ..Default::default() } },
        EditOp::SetTask { key: "DS".into(), patch: TaskPatch { story: Some("S1".into()), ..Default::default() }, scenario: None },
        EditOp::SetTask { key: "SPEC".into(), patch: TaskPatch { story: Some("S1".into()), ..Default::default() }, scenario: None },
    ]).unwrap();
    let ds_story = "S1".to_string();
    assert_eq!(run().story_status["S2"], Status::Todo);

    mv("DS", Status::Doing, "");
    assert_eq!(status("DS"), Status::Doing);
    assert_eq!(run().status, Status::Doing);
    assert_eq!(run().story_status[&ds_story], Status::Doing);

    // in review: started, held since today with the note; not the next task any more
    mv("DS", Status::Review, "bob");
    let p = store.read_plan("s").unwrap();
    let ds = p.tasks.iter().find(|t| t.id == "DS").unwrap();
    assert_eq!(ds.actual_start, Some(today));
    assert_eq!(ds.hold.as_ref().map(|h| (h.kind, h.since, h.note.as_str())), Some((HoldKind::Review, today, "bob")));
    let a = pm.agenda(Some("alice"), today).unwrap();
    assert_ne!(a.next.as_ref().map(|t| t.id.as_str()), Some("DS"));
    assert!(a.on_hold.iter().any(|t| t.id == "DS" && t.status == Status::Review));
    // a review is a risk only after REVIEW_DAYS
    assert!(!a.risks.iter().any(|r| r.task.as_deref() == Some("DS")), "{:?}", a.risks);
    let later = pm.agenda(Some("alice"), day("2026-11-20")).unwrap();
    assert!(later.risks.iter().any(|r| r.task.as_deref() == Some("DS") && r.message.contains("in review since 2026-11-17 (3 day(s)") && r.message.contains("bob")), "{:?}", later.risks);

    // blocked: always a risk, and the story and epic show it
    mv("API", Status::Blocked, "waiting for the vendor's key");
    assert_eq!(status("API"), Status::Blocked);
    assert_eq!(run().status, Status::Blocked);
    let a = pm.agenda(None, today).unwrap();
    assert!(a.risks.iter().any(|r| r.task.as_deref() == Some("API") && r.message.contains("blocked since") && r.message.contains("vendor")), "{:?}", a.risks);
    assert!(!a.start_today.iter().any(|t| t.id == "API"), "a blocked task is not a late start");

    // done clears the hold; unblocking goes back to in progress or to do
    mv("DS", Status::Done, "");
    let p = store.read_plan("s").unwrap();
    let ds = p.tasks.iter().find(|t| t.id == "DS").unwrap();
    assert!(ds.done && ds.hold.is_none() && ds.actual_finish == Some(today));
    mv("API", Status::Todo, "");
    assert_eq!(status("API"), Status::Todo);
    assert_eq!(status("DS"), Status::Done);
    assert_eq!(run().status, Status::Doing);

    // holds survive the TOML dump and import
    mv("API", Status::Blocked, "key");
    let toml = plan_access::plan_access::to_toml(&store.read_plan("s").unwrap());
    assert!(toml.contains("hold = { kind = \"blocked\", since = \"2026-11-17\", note = \"key\" }"), "{toml}");
    let f = std::env::temp_dir().join(format!("plan-test-status-{}.toml", std::process::id()));
    std::fs::write(&f, &toml).unwrap();
    let back = TomlFile(f.clone()).read_plan("").unwrap();
    assert_eq!(back.tasks.iter().find(|t| t.id == "API").unwrap().hold, store.read_plan("s").unwrap().tasks.iter().find(|t| t.id == "API").unwrap().hold);
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&db);
}
