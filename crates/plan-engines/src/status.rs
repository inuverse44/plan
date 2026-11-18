//! Status (part of the AnalysisEngine). Encapsulates how status is read from a task and rolled up, and what a change of status writes.
//! Status is never stored: it is derived from done, the actual start and the hold, so it cannot drift from them
use chrono::NaiveDate;
use plan_contracts::*;

/// A task's status, checked top to bottom: done, blocked, in review, started, otherwise to do
pub fn of(done: bool, actual_start: Option<NaiveDate>, hold: Option<&Hold>) -> Status {
    if done { return Status::Done; }
    match hold.map(|h| h.kind) {
        Some(HoldKind::Blocked) => Status::Blocked,
        Some(HoldKind::Review) => Status::Review,
        None if actual_start.is_some() => Status::Doing,
        None => Status::Todo,
    }
}

/// The status of a story or an epic from its tasks: all done → done; any blocked → blocked; any started → doing; otherwise to do (also when empty)
pub fn roll_up(tasks: impl IntoIterator<Item = Status>) -> Status {
    let v: Vec<Status> = tasks.into_iter().collect();
    if v.is_empty() { return Status::Todo; }
    if v.iter().all(|s| *s == Status::Done) { return Status::Done; }
    if v.contains(&Status::Blocked) { return Status::Blocked; }
    if v.iter().any(|s| *s != Status::Todo) { return Status::Doing; }
    Status::Todo
}

/// The edit that moves a task to `to`. `date` is the day given by the person (None: today, and dates already recorded are kept)
pub fn transition(t: &TaskSpec, to: Status, date: Option<NaiveDate>, today: NaiveDate, note: &str) -> TaskPatch {
    let day = date.unwrap_or(today);
    let mut p = TaskPatch::default();
    let reopen = |p: &mut TaskPatch| if t.done { p.done = Some(false); p.clear.push("actual_finish".into()); };
    let start = |p: &mut TaskPatch| if date.is_some() || t.actual_start.is_none() { p.actual_start = Some(day); };
    let hold = |p: &mut TaskPatch, kind: HoldKind| {
        // the same hold again keeps its first day unless a day is given; an empty note keeps the old one
        let same = t.hold.as_ref().filter(|h| h.kind == kind && date.is_none());
        let note = if note.is_empty() { same.map(|h| h.note.clone()).unwrap_or_default() } else { note.to_string() };
        p.hold = Some(Hold { kind, since: same.map(|h| h.since).unwrap_or(day), note });
    };
    match to {
        Status::Todo => {
            if t.done { p.done = Some(false); }
            p.clear.extend(["actual_start", "actual_finish", "hold"].map(String::from));
        }
        Status::Doing => { reopen(&mut p); start(&mut p); p.clear.push("hold".into()); }
        // a review follows work, so it implies a start; a task can be blocked before it starts
        Status::Review => { reopen(&mut p); start(&mut p); hold(&mut p, HoldKind::Review); }
        Status::Blocked => { reopen(&mut p); hold(&mut p, HoldKind::Blocked); }
        Status::Done => {
            p.done = Some(true);
            if date.is_some() || t.actual_finish.is_none() || !t.done { p.actual_finish = Some(day); }
            p.clear.push("hold".into());
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> NaiveDate { s.parse().unwrap() }
    fn hold(kind: HoldKind) -> Hold { Hold { kind, since: d("2026-11-10"), note: "x".into() } }

    #[test]
    fn status_is_read_top_to_bottom() {
        assert_eq!(of(false, None, None), Status::Todo);
        assert_eq!(of(false, Some(d("2026-11-01")), None), Status::Doing);
        assert_eq!(of(false, Some(d("2026-11-01")), Some(&hold(HoldKind::Review))), Status::Review);
        assert_eq!(of(false, None, Some(&hold(HoldKind::Blocked))), Status::Blocked);
        assert_eq!(of(true, None, Some(&hold(HoldKind::Blocked))), Status::Done);
    }

    #[test]
    fn roll_up_rules() {
        use Status::*;
        assert_eq!(roll_up([]), Todo);
        assert_eq!(roll_up([Todo, Todo]), Todo);
        assert_eq!(roll_up([Done, Done]), Done);
        assert_eq!(roll_up([Done, Todo]), Doing);
        assert_eq!(roll_up([Review, Todo]), Doing);
        assert_eq!(roll_up([Doing, Blocked, Done]), Blocked);
    }

    #[test]
    fn transitions_write_dates_and_holds() {
        let today = d("2026-11-18");
        let t = TaskSpec { id: "T".into(), ..Default::default() };
        let p = transition(&t, Status::Review, None, today, "alice");
        assert_eq!(p.actual_start, Some(today));
        assert_eq!(p.hold, Some(Hold { kind: HoldKind::Review, since: today, note: "alice".into() }));
        // blocked before starting: no start date
        assert_eq!(transition(&t, Status::Blocked, None, today, "").actual_start, None);
        // the same hold keeps its first day and note
        let held = TaskSpec { hold: Some(hold(HoldKind::Blocked)), actual_start: Some(d("2026-11-02")), ..t.clone() };
        let p = transition(&held, Status::Blocked, None, today, "");
        assert_eq!(p.hold.as_ref().map(|h| (h.since, h.note.as_str())), Some((d("2026-11-10"), "x")));
        // back to doing keeps the start and drops the hold
        let p = transition(&held, Status::Doing, None, today, "");
        assert_eq!(p.actual_start, None);
        assert!(p.clear.contains(&"hold".to_string()));
        // done clears the hold; reopening a done task removes the finish
        let p = transition(&held, Status::Done, None, today, "");
        assert_eq!((p.done, p.actual_finish), (Some(true), Some(today)));
        assert!(p.clear.contains(&"hold".to_string()));
        let done = TaskSpec { done: true, actual_finish: Some(d("2026-11-12")), ..held };
        let p = transition(&done, Status::Doing, None, today, "");
        assert_eq!(p.done, Some(false));
        assert!(p.clear.contains(&"actual_finish".to_string()));
    }
}
