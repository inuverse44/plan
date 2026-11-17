//! What to do now, across epics (part of the AnalysisEngine): the next task, what starts today, what is due this week, and risks.
//! Works on each epic's computed run; the manager collects the runs
use crate::{analysis, scheduling};
use chrono::{Datelike, Duration, NaiveDate};
use plan_contracts::*;

/// One epic's open work tasks as agenda items, and the epic's state
pub fn epic_items(epic: &str, name: &str, deadline: Option<NaiveDate>, run: &RunResult, cal: &BusinessCalendar) -> (Vec<AgendaTask>, EpicStatus) {
    let done: std::collections::HashSet<&str> = run.tasks.iter().filter(|t| t.done).map(|t| t.id.as_str()).collect();
    let items = run.tasks.iter().filter(|t| t.kind == Kind::Work && !t.done).filter_map(|t| Some(AgendaTask {
        epic: epic.into(), epic_name: name.into(), id: t.id.clone(), name: if t.short.is_empty() { t.name.clone() } else { t.short.clone() },
        ticket: t.ticket.clone(), owners: t.owners.clone(),
        start: scheduling::day_of(cal, t.es).ok()?, finish: analysis::end_day(cal, t.ef.max(t.es + 1)),
        float_days: t.tf.unwrap_or(0) as f64 / 2.0, ready: t.after.iter().all(|p| done.contains(p.as_str())), started: t.actual_start.is_some(), deadline,
    })).collect();
    let status = EpicStatus { epic: epic.into(), name: name.into(), deadline, target: deadline.map(|_| analysis::end_day(cal, run.target)), slack_days: run.slack_to_deadline, issues: vec![] };
    (items, status)
}

/// Put the agenda together for one owner (None: everyone)
pub fn build(owner: Option<&str>, today: NaiveDate, items: Vec<AgendaTask>, epics: Vec<EpicStatus>) -> Agenda {
    let mine: Vec<AgendaTask> = items.into_iter().filter(|t| owner.is_none_or(|o| t.owners.iter().any(|x| x == o))).collect();
    let mut ready: Vec<&AgendaTask> = mine.iter().filter(|t| t.ready).collect();
    ready.sort_by(|a, b| a.float_days.total_cmp(&b.float_days).then(a.start.cmp(&b.start))
        .then(a.deadline.unwrap_or(NaiveDate::MAX).cmp(&b.deadline.unwrap_or(NaiveDate::MAX))).then(a.epic.cmp(&b.epic)).then(a.id.cmp(&b.id)));
    let by_start = |mut v: Vec<AgendaTask>| { v.sort_by(|a, b| a.start.cmp(&b.start).then(a.float_days.total_cmp(&b.float_days))); v };
    let sunday = today + Duration::days(6 - today.weekday().num_days_from_monday() as i64);
    let start_today = by_start(mine.iter().filter(|t| t.start <= today && !t.started).cloned().collect());
    let mut due_this_week: Vec<AgendaTask> = mine.iter().filter(|t| t.finish <= sunday).cloned().collect();
    due_this_week.sort_by(|a, b| a.finish.cmp(&b.finish).then(a.float_days.total_cmp(&b.float_days)));
    let mut risks: Vec<AgendaRisk> = epics.iter().filter_map(|e| {
        let (s, t) = (e.slack_days?, e.target?);
        (s < 0).then(|| AgendaRisk { epic: e.epic.clone(), task: None, message: format!("{}: planned to reach the target on {t}, {} business day(s) after the deadline {}", e.name, -s, e.deadline.map(|d| d.to_string()).unwrap_or_default()) })
    }).collect();
    for t in &start_today {
        if t.start < today {
            let why = if t.float_days == 0.0 { "critical: every day late moves the end".to_string() } else { format!("{}d of float left", fmt_num(t.float_days)) };
            risks.push(AgendaRisk { epic: t.epic.clone(), task: Some(t.id.clone()), message: format!("{} was planned to start on {} and has not started ({why})", t.id, t.start) });
        }
    }
    for e in &epics { for i in &e.issues { risks.push(AgendaRisk { epic: e.epic.clone(), task: i.task.clone(), message: format!("cannot evaluate: {}", i.line()) }); } }
    Agenda { owner: owner.map(str::to_string), today, next: ready.first().map(|t| (*t).clone()), then: ready.get(1).map(|t| (*t).clone()), start_today, due_this_week, risks, epics }
}
