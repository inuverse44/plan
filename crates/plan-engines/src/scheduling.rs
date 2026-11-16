//! SchedulingEngine. Encapsulates how tasks are placed.
//! Tasks go in order of latest start from a resource-free forward/backward pass (least float first), one task per person at a time, back-filling gaps.
//! A person's days off pause a task; a needed resource's off days push the whole task past them (it does not straddle them).
//! Float is the largest delay of that one task, from where it was placed, that does not move the end, found by re-scheduling (waiting for people included).
use plan_contracts::{BusinessCalendar, Change, Issue, Kind, Schedule, Task};
use std::collections::HashMap;

pub fn topo(tasks: &[Task]) -> Result<Vec<usize>, Issue> {
    let idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let mut state = vec![0u8; tasks.len()];
    let mut order = Vec::new();
    fn visit(i: usize, tasks: &[Task], idx: &HashMap<&str, usize>, state: &mut [u8], order: &mut Vec<usize>, path: &mut Vec<String>) -> Result<(), Issue> {
        if state[i] == 2 { return Ok(()); }
        if state[i] == 1 {
            path.push(tasks[i].id.clone());
            return Err(Issue::error(format!("dependency loop: {}", path.join(" → "))));
        }
        state[i] = 1;
        path.push(tasks[i].id.clone());
        for p in &tasks[i].after {
            let j = *idx.get(p.as_str()).ok_or_else(|| Issue::error(format!("unknown task in after: {p}")).at(None, Some(&tasks[i].id)))?;
            visit(j, tasks, idx, state, order, path)?;
        }
        path.pop();
        state[i] = 2;
        order.push(i);
        Ok(())
    }
    for i in 0..tasks.len() { visit(i, tasks, &idx, &mut state, &mut order, &mut Vec::new())?; }
    Ok(order)
}

/// Predecessors without transitive ones (with A→B→C and A→C, A→C is dropped)
pub fn reduce(tasks: &[Task]) -> Vec<Vec<String>> {
    let idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let mut memo: Vec<Option<std::collections::HashSet<String>>> = vec![None; tasks.len()];
    fn anc(i: usize, tasks: &[Task], idx: &HashMap<&str, usize>, memo: &mut Vec<Option<std::collections::HashSet<String>>>) -> std::collections::HashSet<String> {
        if let Some(s) = &memo[i] { return s.clone(); }
        let mut s = std::collections::HashSet::new();
        for p in &tasks[i].after {
            s.insert(p.clone());
            if let Some(&j) = idx.get(p.as_str()) { s.extend(anc(j, tasks, idx, memo)); }
        }
        memo[i] = Some(s.clone());
        s
    }
    (0..tasks.len()).map(|i| {
        let t = &tasks[i];
        t.after.iter().filter(|p| !t.after.iter().any(|q| q != *p && idx.get(q.as_str()).map(|&j| anc(j, tasks, &idx, &mut memo).contains(*p)).unwrap_or(false))).cloned().collect()
    }).collect()
}

fn slot_on_or_after(cal: &BusinessCalendar, d: chrono::NaiveDate) -> usize {
    cal.days.iter().position(|x| *x >= d).unwrap_or(cal.days.len()) * 2
}

/// Earliest start (the latest of plan start, not_before, fixed and a moved start) and the reason
pub fn release(t: &Task, cal: &BusinessCalendar, ch: Option<&Change>) -> (usize, &'static str) {
    let mut s = cal.start_slot;
    let mut why = "plan start";
    for d in [t.not_before, t.fixed].into_iter().flatten() {
        let x = slot_on_or_after(cal, d);
        if x > s || why == "plan start" { s = s.max(x); why = "not before"; }
    }
    if let Some(m) = ch.and_then(|c| c.min_start) {
        if m > s { s = m; why = "not before (moved)"; }
    }
    (s, why)
}

pub fn schedule(tasks: &[Task], cal: &BusinessCalendar, changes: &[Change]) -> Result<Schedule, Issue> {
    let n = tasks.len();
    let ch: HashMap<&str, &Change> = changes.iter().map(|c| (c.id.as_str(), c)).collect();
    let dur: Vec<usize> = tasks.iter().map(|t| ch.get(t.id.as_str()).and_then(|c| c.dur).unwrap_or(t.dur)).collect();
    let rel: Vec<(usize, &str)> = tasks.iter().map(|t| release(t, cal, ch.get(t.id.as_str()).copied())).collect();
    let order = topo(tasks)?;
    let idx: HashMap<&str, usize> = tasks.iter().enumerate().map(|(i, t)| (t.id.as_str(), i)).collect();
    let preds: Vec<Vec<usize>> = tasks.iter().map(|t| t.after.iter().map(|p| idx[p.as_str()]).collect()).collect();
    let mut succ = vec![Vec::new(); n];
    for (i, ps) in preds.iter().enumerate() { for &p in ps { succ[p].push(i); } }

    // Resource-free forward/backward pass (only for the order)
    let mut es = vec![0i64; n];
    let mut ef = vec![0i64; n];
    for &k in &order {
        es[k] = if tasks[k].kind == Kind::Done { 0 } else { preds[k].iter().map(|&p| ef[p]).chain([rel[k].0 as i64]).max().unwrap() };
        ef[k] = es[k] + dur[k] as i64;
    }
    let cend = ef.iter().copied().max().unwrap_or(0);
    let mut ls = vec![0i64; n];
    for &k in order.iter().rev() {
        let lf = succ[k].iter().map(|&s| ls[s]).chain([cend]).min().unwrap();
        ls[k] = lf - dur[k] as i64;
    }
    let mut pos = vec![0usize; n];
    for (i, &k) in order.iter().enumerate() { pos[k] = i; }

    let owners: Vec<&String> = { let mut v: Vec<&String> = tasks.iter().flat_map(|t| t.owners.iter()).collect(); v.sort(); v.dedup(); v };
    let oidx: HashMap<&str, usize> = owners.iter().enumerate().map(|(i, o)| (o.as_str(), i)).collect();
    let towners: Vec<Vec<usize>> = tasks.iter().map(|t| t.owners.iter().map(|o| oidx[o.as_str()]).collect()).collect();
    let leave: Vec<std::collections::HashSet<usize>> = owners.iter().map(|o| cal.leave.get(*o).map(|v| v.iter().copied().collect()).unwrap_or_default()).collect();
    // Days each task's resources cannot be used, and which resource blocks each day
    let blocked: Vec<HashMap<usize, &str>> = tasks.iter().map(|t| {
        let mut m = HashMap::new();
        for nd in &t.needs { for &d in cal.blocked.get(nd).map(|v| v.as_slice()).unwrap_or(&[]) { m.entry(d).or_insert(nd.as_str()); } }
        m
    }).collect();
    let limit = cal.days.len() * 2;
    if let Some(k) = (0..n).find(|&k| dur[k] > limit) {
        return Err(Issue::error(format!("longer than the calendar ({} business days)", cal.days.len())).at(None, Some(&tasks[k].id)));
    }

    // The last value is the first task that ran past the end of the calendar, if any
    let run = |delay: Option<(usize, usize)>| -> (Vec<usize>, Vec<usize>, Vec<String>, usize, Option<usize>) {
        let mut busy: Vec<HashMap<usize, usize>> = vec![HashMap::new(); owners.len()];
        let mut s_es = vec![0usize; n];
        let mut s_ef = vec![0usize; n];
        let mut why = vec![String::new(); n];
        let mut placed = vec![false; n];
        let mut past = None;
        for _ in 0..n {
            let k = (0..n).filter(|&c| !placed[c] && preds[c].iter().all(|&p| placed[p]))
                .min_by_key(|&c| (ls[c], es[c], pos[c])).unwrap();
            placed[k] = true;
            let t = &tasks[k];
            if t.kind == Kind::Done { s_es[k] = cal.start_slot; s_ef[k] = cal.start_slot; why[k] = "done".into(); continue; }
            let mut s0 = 0usize;
            let mut reason = String::new();
            let mut first = true;
            for &p in &preds[k] {
                if first || s_ef[p] > s0 { s0 = s_ef[p]; reason = format!("after {}", tasks[p].id); first = false; }
            }
            if first || rel[k].0 > s0 { s0 = rel[k].0; reason = rel[k].1.to_string(); }
            // delay test: start no earlier than the given slot (the placed start plus the delay)
            if let Some((dk, m)) = delay { if dk == k && m > s0 { s0 = m; reason = "(delay test)".into(); } }
            let mut s = s0;
            let used = loop {
                let mut used = Vec::with_capacity(dur[k]);
                let mut x = s;
                let mut hit: Option<String> = None;
                while used.len() < dur[k] && x < limit {
                    if let Some(&o) = towners[k].iter().find(|&&o| busy[o].contains_key(&x)) { hit = Some(format!("owner busy ({})", tasks[busy[o][&x]].id)); break; }
                    if towners[k].iter().any(|&o| leave[o].contains(&(x / 2))) { x += 1; continue; }
                    if let Some(r) = blocked[k].get(&(x / 2)) { hit = Some(format!("{r} unavailable")); break; }
                    used.push(x);
                    x += 1;
                }
                match hit {
                    None => break used,
                    Some(h) => { s = x + 1; reason = h; }
                }
            };
            if used.len() < dur[k] && past.is_none() { past = Some(k); }
            for &x in &used { for &o in &towners[k] { busy[o].insert(x, k); } }
            s_es[k] = used.first().copied().unwrap_or(s);
            s_ef[k] = used.last().map(|x| x + 1).unwrap_or(s);
            why[k] = reason;
        }
        let end = s_ef.iter().copied().max().unwrap_or(cal.start_slot);
        (s_es, s_ef, why, end, past)
    };

    let (bes, bef, bwhy, bend, past) = run(None);
    if let Some(k) = past {
        return Err(Issue::error(format!("runs past the end of the calendar ({} business days)", cal.days.len())).at(None, Some(&tasks[k].id)));
    }
    let mut tf = vec![None; n];
    for k in 0..n {
        if tasks[k].kind == Kind::Done { continue; }
        // delay from where the task was placed, not from its earliest start: a task pushed back by a freeze or a busy owner has no float hidden in that push
        // running past the calendar counts as moving the end
        let moves = |d: usize| { let r = run(Some((k, bes[k] + d))); r.3 != bend || r.4.is_some() };
        if moves(1) { tf[k] = Some(0); continue; }
        let cap = 240;
        let (mut lo, mut hi) = (1usize, 2usize);
        while hi <= cap && !moves(hi) { lo = hi; hi *= 2; }
        hi = hi.min(cap + 1);
        while hi - lo > 1 { let mid = (lo + hi) / 2; if moves(mid) { hi = mid } else { lo = mid } }
        tf[k] = Some(lo);
    }
    let ff = (0..n).map(|k| {
        if tasks[k].kind == Kind::Done { return None; }
        let nxt = succ[k].iter().filter(|&&s| tasks[s].kind != Kind::Done).map(|&s| bes[s]).min().unwrap_or(bend);
        Some(nxt.saturating_sub(bef[k]))
    }).collect();
    Ok(Schedule { order, es: bes, ef: bef, tf, ff, why: bwhy, end: bend, reduced_after: reduce(tasks) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plan_contracts::{BusinessCalendar, Change, Kind, Task};

    fn cal(days: usize) -> BusinessCalendar {
        let d0 = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        BusinessCalendar { days: (0..days as i64).map(|i| d0 + chrono::Duration::days(i)).collect(), start_slot: 0, blocked: Default::default(), leave: Default::default(), off: vec![] }
    }
    fn work(id: &str, dur: usize, after: &[&str]) -> Task {
        Task { id: id.into(), kind: Kind::Work, dur, val: dur as f64 / 2.0, owners: vec!["a".into()], after: after.iter().map(|s| s.to_string()).collect(), needs: vec![], not_before: None, fixed: None }
    }

    #[test]
    fn fits_the_calendar() {
        let s = schedule(&[work("A", 4, &[]), work("B", 2, &["A"])], &cal(10), &[]).unwrap();
        assert_eq!((s.es, s.ef, s.end), (vec![0, 4], vec![4, 6], 6));
        assert_eq!(s.tf, vec![Some(0), Some(0)]);
    }

    #[test]
    fn a_duration_longer_than_the_calendar_is_an_error() {
        let ch = Change { id: "A".into(), dur: Some(1 << 62), ..Default::default() };
        let e = schedule(&[work("A", 4, &[])], &cal(10), &[ch]).unwrap_err();
        assert_eq!(e.task.as_deref(), Some("A"), "{e:?}");
    }

    #[test]
    fn a_task_running_past_the_calendar_is_an_error() {
        // each fits alone (20 slots), but B starts after A and cannot finish
        let e = schedule(&[work("A", 12, &[]), work("B", 12, &["A"])], &cal(10), &[]).unwrap_err();
        assert_eq!(e.task.as_deref(), Some("B"), "{e:?}");
        assert!(e.message.contains("past the end of the calendar"), "{e:?}");
    }

    #[test]
    fn delay_tests_near_the_end_of_the_calendar_do_not_hide_float() {
        // A has no float: any delay pushes B past the calendar
        let s = schedule(&[work("A", 10, &[]), work("B", 10, &["A"])], &cal(10), &[]).unwrap();
        assert_eq!(s.tf, vec![Some(0), Some(0)]);
    }
}
