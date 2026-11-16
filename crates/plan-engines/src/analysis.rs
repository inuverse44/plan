//! AnalysisEngine. Encapsulates how a plan is judged: float thresholds, risk weights, efficiency, required fields
use crate::{estimation, scheduling};
use plan_contracts::*;
use std::collections::{BTreeMap, HashSet};

pub fn color(kind: Kind, tf: Option<usize>, colors: &Colors) -> FloatColor {
    match kind {
        Kind::Done => FloatColor::Done,
        Kind::Wait => FloatColor::Wait,
        _ => band(tf.unwrap_or(0), colors),
    }
}

fn band(tf: usize, c: &Colors) -> FloatColor {
    let f = tf as f64 / 2.0;
    if f <= 0.0 { FloatColor::Crit } else if f <= c.red { FloatColor::Red } else if f <= c.yellow { FloatColor::Yellow } else { FloatColor::Green }
}

/// Round to 2 decimals using the actual binary value (same as Python's round(x, 2))
fn r2(x: f64) -> f64 { format!("{x:.2}").parse().unwrap() }

pub fn assess(plan: &Plan, tasks: &[Task], s: &Schedule, cal: &BusinessCalendar) -> Assessment {
    let colors = tasks.iter().enumerate().map(|(i, t)| color(t.kind, s.tf[i], &plan.colors)).collect();
    // Risk: waits count as activities, colored by their float (Righting Software, ch. 10)
    let acts: Vec<usize> = (0..tasks.len()).filter(|&i| matches!(tasks[i].kind, Kind::Work | Kind::Wait) && tasks[i].dur > 0).collect();
    let risk = if acts.is_empty() { None } else {
        let mut cnt = [0f64; 4];
        for &i in &acts {
            let b = band(s.tf[i].unwrap_or(0), &plan.colors);
            cnt[match b { FloatColor::Crit => 0, FloatColor::Red => 1, FloatColor::Yellow => 2, _ => 3 }] += 1.0;
        }
        let n = acts.len() as f64;
        let fl: Vec<f64> = acts.iter().map(|&i| s.tf[i].unwrap_or(0) as f64 / 2.0).collect();
        let m = fl.iter().copied().fold(0.0, f64::max);
        Some(Risk {
            criticality: r2((4.0 * cnt[0] + 3.0 * cnt[1] + 2.0 * cnt[2] + cnt[3]) / (4.0 * n)),
            fibonacci: r2((4.24 * cnt[0] + 2.62 * cnt[1] + 1.62 * cnt[2] + cnt[3]) / (4.24 * n)),
            activity: if m == 0.0 { None } else { Some(r2(1.0 - fl.iter().sum::<f64>() / (m * n))) },
        })
    };
    let mut effort: BTreeMap<String, f64> = BTreeMap::new();
    let mut load: BTreeMap<String, BTreeMap<chrono::NaiveDate, f64>> = BTreeMap::new();
    for (i, t) in tasks.iter().enumerate() {
        if t.kind != Kind::Work { continue; }
        let share = t.val / t.owners.len().max(1) as f64;
        for o in &t.owners { *effort.entry(o.clone()).or_default() += share; }
        // Count the half-days used (skipping days off). The effort is spread evenly over them, rather than utilization × 0.5 person-days per half-day
        let per = if t.dur > 0 { t.val / t.owners.len().max(1) as f64 / t.dur as f64 } else { 0.0 };
        let (mut x, mut c) = (s.es[i], 0);
        while c < t.dur && x / 2 < cal.days.len() {
            if t.owners.iter().any(|o| cal.leave.get(o).map(|v| v.contains(&(x / 2))).unwrap_or(false)) { x += 1; continue; }
            for o in &t.owners { *load.entry(o.clone()).or_default().entry(cal.days[x / 2]).or_default() += per; }
            c += 1;
            x += 1;
        }
    }
    let effort_total = r2(effort.values().sum());
    let span = (s.end.saturating_sub(cal.start_slot)) as f64 / 2.0;
    let efficiency = if effort.is_empty() || span <= 0.0 { None } else { Some(r2(effort.values().sum::<f64>() / (effort.len() as f64 * span))) };
    let target_slot = plan.deadline_task.as_ref().and_then(|d| tasks.iter().position(|t| &t.id == d)).map(|i| s.ef[i]).unwrap_or(s.end);
    let slack_to_deadline = plan.deadline.map(|dl| {
        let day = end_day(cal, target_slot);
        bdays_between(cal, day, dl)
    });
    let critical_path = s.order.iter().filter(|&&i| s.tf[i] == Some(0)).map(|&i| tasks[i].id.clone()).collect();
    Assessment { colors, risk, effort, effort_total, efficiency, load, target_slot, slack_to_deadline, critical_path }
}

/// The finish day, from f (the half-day after the end)
pub fn end_day(cal: &BusinessCalendar, f: usize) -> chrono::NaiveDate {
    if f > cal.start_slot { cal.days[(f - 1) / 2] } else { cal.days[cal.start_slot / 2] }
}

/// Number of business days with a < x <= b (negative if b < a)
pub fn bdays_between(cal: &BusinessCalendar, a: chrono::NaiveDate, b: chrono::NaiveDate) -> i64 {
    if a <= b { cal.days.iter().filter(|x| a < **x && **x <= b).count() as i64 } else { -(cal.days.iter().filter(|x| b < **x && **x <= a).count() as i64) }
}

/// Lint the plan (errors and warnings)
pub fn inspect(plan: &Plan, scenarios: &[Scenario]) -> Vec<Issue> {
    let mut out: Vec<Issue> = plan.unknown_keys.iter().map(|k| Issue::warning(format!("unknown key {k}"))).collect();
    let bad_rate = |r: f64| !(r.is_finite() && r > 0.0);
    for r in plan.rates.iter().copied().filter(|&r| bad_rate(r)) { out.push(Issue::error(format!("rates: {} is not a number > 0", fmt_num(r)))); }
    for (name, p) in &plan.people {
        if let Some(r) = p.rate.filter(|&r| bad_rate(r)) { out.push(Issue::error(format!("rate of {name}: {} is not a number > 0", fmt_num(r)))); }
    }
    for sc in scenarios {
        let sk = Some(sc.key.as_str());
        let ids: Vec<&str> = sc.tasks.iter().map(|t| t.id.as_str()).collect();
        let mut errs = Vec::new();
        for t in &sc.tasks {
            if t.id.is_empty() { errs.push(Issue::error("task without an id").at(sk, None)); }
            for k in &t.unknown_keys { out.push(Issue::warning(format!("unknown key {k}")).at(sk, Some(&t.id))); }
        }
        let mut seen = HashSet::new();
        let dup: Vec<&str> = ids.iter().filter(|i| !seen.insert(**i)).copied().collect();
        if !dup.is_empty() { errs.push(Issue::error(format!("duplicate ids: {dup:?}")).at(sk, None)); }
        if !errs.is_empty() { out.extend(errs); continue; }
        for t in &sc.tasks {
            let ts = Some(t.id.as_str());
            for p in &t.after { if !ids.contains(&p.as_str()) { errs.push(Issue::error(format!("unknown task in after: {p}")).at(sk, ts)); } }
            if t.wait.is_some() && estimation::present(&t.effort) { errs.push(Issue::error("use effort or wait, not both").at(sk, ts)); }
            if estimation::present(&t.effort) && t.wait.is_none() {
                if t.deliverable.is_empty() { out.push(Issue::warning("no deliverable").at(sk, ts)); }
                let owners: Vec<String> = t.owners.clone().unwrap_or_else(|| plan.default_owner.clone().into_iter().collect());
                if owners.is_empty() { out.push(Issue::warning("no owner (and no default_owner)").at(sk, ts)); }
                for n in &t.needs { if !plan.resources.contains_key(n) { out.push(Issue::warning(format!("needs {n}, which is not in resources (it never blocks)")).at(sk, ts)); } }
                if !plan.people.is_empty() { for o in &owners { if !plan.people.contains_key(o) { out.push(Issue::warning(format!("owner {o} is not in people")).at(sk, ts)); } } }
            }
        }
        if let Some(d) = &plan.deadline_task { if !ids.contains(&d.as_str()) { errs.push(Issue::error(format!("deadline_task {d} does not exist")).at(sk, None)); } }
        if !errs.is_empty() { out.extend(errs); continue; }
        let tasks: Vec<Task> = estimation::resolve(plan, &sc.tasks, EstKind::E, 1.0);
        if let Err(e) = scheduling::topo(&tasks) { out.push(e.at(sk, None)); continue; }
        let red = scheduling::reduce(&tasks);
        for (i, t) in tasks.iter().enumerate() {
            let extra: Vec<&String> = t.after.iter().filter(|p| !red[i].contains(p)).collect();
            if !extra.is_empty() { out.push(Issue::warning(format!("transitive dependencies {extra:?} (already reached through other predecessors; can be removed)")).at(sk, Some(&t.id))); }
        }
        let eff: Vec<(usize, f64)> = sc.tasks.iter().enumerate().filter(|(_, t)| estimation::present(&t.effort) && !t.done && t.wait.is_none())
            .map(|(i, t)| (i, estimation::value(t.effort.as_ref().unwrap(), EstKind::E, 0.5))).collect();
        if eff.len() >= 4 {
            let n = eff.len() as f64;
            let mu = eff.iter().map(|x| x.1).sum::<f64>() / n;
            let sd = (eff.iter().map(|x| (x.1 - mu).powi(2)).sum::<f64>() / n).sqrt();
            for (i, v) in &eff {
                if *v > mu + sd && *v >= 5.0 {
                    out.push(Issue::warning(format!("god activity ({} person-days > mean + 1σ {:.1}); consider splitting it", fmt_num(*v), mu + sd)).at(sk, Some(&sc.tasks[*i].id)));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|i| seen.insert(i.line()));
    out
}
