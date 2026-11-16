//! 元の Python 版（plan.py）の結果と、全部の組み・全部の作業で開始・終了・余裕・自由余裕が一致するか
use plan_access::{JpRuleCalendar, TomlFile};
use plan_manager::PlanningManager;
use std::path::PathBuf;

fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[test]
fn matches_python_on_eol_gce() {
    let m = PlanningManager::new(std::sync::Arc::new(TomlFile(root().join("examples/sample.toml"))), std::sync::Arc::new(JpRuleCalendar));
    let ev = m.evaluate("").unwrap_or_else(|e| panic!("{:?}", e));
    let py: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("tests/fixtures/sample.expected.json")).unwrap()).unwrap();
    let mut bad = vec![];
    let mut n = 0;
    for r in &ev.runs {
        let p = &py[&r.key];
        assert!(!p.is_null(), "組み {} が Python に無い", r.key);
        assert_eq!(p["end_slot"].as_u64().unwrap() as usize, r.end, "{} の終わり", r.key);
        for t in &r.tasks {
            n += 1;
            let e = &p["tasks"][&t.id];
            let tf = t.tf.map(|x| x as f64 / 2.0);
            let ff = t.ff.map(|x| x as f64 / 2.0);
            let ok = e[0].as_u64() == Some(t.es as u64) && e[1].as_u64() == Some(t.ef as u64) && e[2].as_f64() == tf && e[3].as_f64() == ff;
            if !ok { bad.push(format!("{} {}: py {e} rust [{}, {}, {:?}, {:?}]", r.key, t.id, t.es, t.ef, tf, ff)); }
        }
        let rk = r.risk.as_ref().unwrap();
        assert!((p["risk"]["criticality"].as_f64().unwrap() - rk.criticality).abs() < 0.011, "{} のリスク", r.key);
    }
    assert!(bad.is_empty(), "違う {} / {} 件\n{}", bad.len(), n, bad.join("\n"));
    assert_eq!(ev.runs.len(), 27);
}

#[test]
fn jp_holidays_match_python_holidays_package() {
    let want: Vec<String> = std::fs::read_to_string(root().join("tests/fixtures/jp-holidays-2025-2030.txt")).unwrap().lines().map(str::to_string).collect();
    let mut got: Vec<String> = (2025..=2030).flat_map(plan_access::calendar_access::jp_holidays).map(|d| d.to_string()).collect();
    got.sort();
    assert_eq!(got, want);
}
