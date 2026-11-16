//! Float of tasks pushed back by something other than their predecessors (a release freeze, a busy owner)
use plan_access::{JpRuleCalendar, TomlFile};
use plan_manager::PlanningManager;
use std::path::PathBuf;
use std::sync::Arc;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[test]
fn a_task_pushed_by_a_freeze_is_critical() {
    let m = PlanningManager::new(Arc::new(TomlFile(root().join("tests/fixtures/freeze-float.toml"))), Arc::new(JpRuleCalendar));
    let ev = m.evaluate("").unwrap_or_else(|e| panic!("{e:?}"));
    let r = &ev.runs[0];
    let t = |id: &str| r.tasks.iter().find(|t| t.id == id).unwrap();
    let cal = &ev.calendar;
    // A on Mon 12/21, the freeze is Tue–Thu, so CUT waits until Fri 12/25
    assert_eq!(cal.days[t("CUT").es / 2].to_string(), "2026-12-25", "{}", t("CUT").why);
    assert_eq!(t("CUT").tf, Some(0), "CUT: any delay moves the end");
    assert_eq!(t("W").tf, Some(0));
    assert!(r.critical_path.contains(&"CUT".to_string()), "{:?}", r.critical_path);
    // A finishes Mon; it may slip three days (Tue–Thu) without moving anything: CUT waits for the freeze anyway
    assert_eq!(t("A").tf, Some(6));
}
