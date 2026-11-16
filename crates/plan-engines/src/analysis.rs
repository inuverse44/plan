//! AnalysisEngine：隠す変化は「評価の物差しと作法の決まり」（色の境目・リスクの重み・効率・必須の項目）
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

/// 小数第 2 位に丸める。実際の 2 進数の値で丸める（Python の round(x, 2) と同じ）
fn r2(x: f64) -> f64 { format!("{x:.2}").parse().unwrap() }

pub fn assess(plan: &Plan, tasks: &[Task], s: &Schedule, cal: &BusinessCalendar) -> Assessment {
    let colors = tasks.iter().enumerate().map(|(i, t)| color(t.kind, s.tf[i], &plan.colors)).collect();
    // リスク：待ちも活動として余裕で色を付けて数える（Righting Software 10 章）
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
        // 使った半日を数える（休みを飛ばす）。1 半日 ＝ 稼働率 × 0.5 人日ではなく、工数を半日に均等に配る
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

/// 終わりの次の半日 f から、終わる日
pub fn end_day(cal: &BusinessCalendar, f: usize) -> chrono::NaiveDate {
    if f > cal.start_slot { cal.days[(f - 1) / 2] } else { cal.days[cal.start_slot / 2] }
}

/// a < x <= b の営業日の数（b < a なら負）
pub fn bdays_between(cal: &BusinessCalendar, a: chrono::NaiveDate, b: chrono::NaiveDate) -> i64 {
    if a <= b { cal.days.iter().filter(|x| a < **x && **x <= b).count() as i64 } else { -(cal.days.iter().filter(|x| b < **x && **x <= a).count() as i64) }
}

/// 計画の作法の検査（誤り・警告）
pub fn inspect(plan: &Plan, scenarios: &[Scenario]) -> Vec<Issue> {
    let mut out: Vec<Issue> = plan.unknown_keys.iter().map(|k| Issue::warning(format!("知らない項目 {k}"))).collect();
    for sc in scenarios {
        let sk = Some(sc.key.as_str());
        let ids: Vec<&str> = sc.tasks.iter().map(|t| t.id.as_str()).collect();
        let mut errs = Vec::new();
        for t in &sc.tasks {
            if t.id.is_empty() { errs.push(Issue::error("id の無い作業").at(sk, None)); }
            for k in &t.unknown_keys { out.push(Issue::warning(format!("知らない項目 {k}")).at(sk, Some(&t.id))); }
        }
        let mut seen = HashSet::new();
        let dup: Vec<&str> = ids.iter().filter(|i| !seen.insert(**i)).copied().collect();
        if !dup.is_empty() { errs.push(Issue::error(format!("id が重なる：{dup:?}")).at(sk, None)); }
        if !errs.is_empty() { out.extend(errs); continue; }
        for t in &sc.tasks {
            let ts = Some(t.id.as_str());
            for p in &t.after { if !ids.contains(&p.as_str()) { errs.push(Issue::error(format!("after に無い作業 {p}")).at(sk, ts)); } }
            if t.wait.is_some() && estimation::present(&t.effort) { errs.push(Issue::error("effort と wait は片方だけ").at(sk, ts)); }
            if estimation::present(&t.effort) && t.wait.is_none() {
                if t.deliverable.is_empty() { out.push(Issue::warning("成果物（deliverable）が無い").at(sk, ts)); }
                let owners: Vec<String> = t.owners.clone().unwrap_or_else(|| plan.default_owner.clone().into_iter().collect());
                if owners.is_empty() { out.push(Issue::warning("担当が無い（default_owner も無い）").at(sk, ts)); }
                if !plan.people.is_empty() { for o in &owners { if !plan.people.contains_key(o) { out.push(Issue::warning(format!("担当 {o} が people に無い")).at(sk, ts)); } } }
            }
        }
        if let Some(d) = &plan.deadline_task { if !ids.contains(&d.as_str()) { errs.push(Issue::error(format!("deadline_task {d} が無い")).at(sk, None)); } }
        if !errs.is_empty() { out.extend(errs); continue; }
        let tasks: Vec<Task> = estimation::resolve(plan, &sc.tasks, EstKind::E, 1.0);
        if let Err(e) = scheduling::topo(&tasks) { out.push(e.at(sk, None)); continue; }
        let red = scheduling::reduce(&tasks);
        for (i, t) in tasks.iter().enumerate() {
            let extra: Vec<&String> = t.after.iter().filter(|p| !red[i].contains(p)).collect();
            if !extra.is_empty() { out.push(Issue::warning(format!("推移的な依存 {extra:?}（ほかの先行を通して既につながる。消してよい）")).at(sk, Some(&t.id))); }
        }
        let eff: Vec<(usize, f64)> = sc.tasks.iter().enumerate().filter(|(_, t)| estimation::present(&t.effort) && !t.done && t.wait.is_none())
            .map(|(i, t)| (i, estimation::value(t.effort.as_ref().unwrap(), EstKind::E, 0.5))).collect();
        if eff.len() >= 4 {
            let n = eff.len() as f64;
            let mu = eff.iter().map(|x| x.1).sum::<f64>() / n;
            let sd = (eff.iter().map(|x| (x.1 - mu).powi(2)).sum::<f64>() / n).sqrt();
            for (i, v) in &eff {
                if *v > mu + sd && *v >= 5.0 {
                    out.push(Issue::warning(format!("大きすぎる作業（god activity、{} 人日 > 平均＋1σ {:.1}）。分けられないか", fmt_num(*v), mu + sd)).at(sk, Some(&sc.tasks[*i].id)));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|i| seen.insert(i.line()));
    out
}
