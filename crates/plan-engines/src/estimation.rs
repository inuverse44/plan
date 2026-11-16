//! EstimationEngine：隠す変化は「見積もりの決まり」（PERT・丸め・稼働率の掛け方・人ごとの率）
use plan_contracts::{EstKind, Estimate, Kind, Plan, Task, TaskSpec};

/// 見積もり 1 つを、選んだ種類の値にする。期待値は PERT を step に丸める（偶数への丸め）
pub fn value(e: &Estimate, kind: EstKind, step: f64) -> f64 {
    match *e {
        Estimate::One(v) => v,
        Estimate::Three(o, m, p) => match kind {
            EstKind::O => o,
            EstKind::M => m,
            EstKind::P => p,
            EstKind::E => ((o + 4.0 * m + p) / 6.0 / step).round_ties_even() * step,
        },
    }
}

/// 値が「ある」か（0 の 1 つの数は無いのと同じ。案で工数を外すのに使う）
pub fn present(e: &Option<Estimate>) -> bool {
    match e { None => false, Some(Estimate::One(v)) => *v != 0.0, Some(Estimate::Three(..)) => true }
}

fn ceil_slots(x: f64) -> usize {
    ((x * 1e6).round_ties_even() / 1e6).ceil() as usize
}

pub fn resolve(plan: &Plan, tasks: &[TaskSpec], est: EstKind, rate: f64) -> Vec<Task> {
    tasks.iter().map(|t| {
        let kind = if t.done { Kind::Done } else if t.wait.is_some() { Kind::Wait } else if !present(&t.effort) { Kind::Milestone } else { Kind::Work };
        let owners: Vec<String> = match (&t.owners, kind) {
            (Some(o), _) => o.clone(),
            (None, Kind::Work) => plan.default_owner.clone().into_iter().collect(),
            _ => vec![],
        };
        let src = if t.wait.is_some() { &t.wait } else { &t.effort };
        let val = src.as_ref().map(|e| value(e, est, plan.round)).unwrap_or(0.0);
        let dur = match kind {
            Kind::Work => {
                let rates: Vec<Option<f64>> = owners.iter().map(|o| plan.people.get(o).and_then(|p| p.rate)).collect();
                let r = if !owners.is_empty() && rates.iter().all(|x| x.is_some()) { rates.iter().map(|x| x.unwrap()).fold(f64::INFINITY, f64::min) } else { rate };
                ceil_slots(val * 2.0 / r / owners.len().max(1) as f64)
            }
            Kind::Wait => ceil_slots(val * 2.0),
            _ => 0,
        };
        Task { id: t.id.clone(), kind, dur, val, owners, after: t.after.clone(), prod: t.prod, not_before: t.not_before, fixed: t.fixed }
    }).collect()
}

/// 半日の数から工数（人日）に戻す（画面で日数を変えたとき、計画ファイルに書く値）
pub fn effort_for(t: &Task, dur: usize, rate: f64) -> f64 {
    match t.kind {
        Kind::Wait => dur as f64 / 2.0,
        _ => ((dur as f64 / 2.0 * rate * t.owners.len().max(1) as f64) / 0.5).round_ties_even() * 0.5,
    }
}
