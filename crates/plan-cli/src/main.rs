//! CliClient：AI と人が読む・直すための plan コマンド。計画は SQLite（PLAN_DB、既定はリポジトリの data/plan.db）
//!
//!   読む    plan epics | dump E | check E | calc E | show E [-s 案 -r 0.6 -e e] | why E 作業 | json E | log [E]
//!   作る    plan epic add KEY 名前 開始日 | epic rm KEY | import F.toml [--key K] | new F.toml（TOML のひな形）
//!   直す    plan task add E KEY k=v … | task set E KEY k=v … [-s 案] | task rm E KEY
//!           plan dep add E 作業 先行 | dep rm E 作業 先行 | story set E KEY name=… | scenario clone E 元|- KEY 名前 | scenario rm E KEY
//!   まとめ  plan apply E ops.json（- で標準入力）  編集の一覧を 1 回で当てる。1 つでも誤りがあれば何も書かない
//!   機械用  どのコマンドも --json を付けると {"ok":true,"data":…} か {"ok":false,"issues":[…]} を返す
//!   画面    plan serve [--port N] [--no-open] | export E [out.html]
//!   k=v     name short story ticket deliverable note owner=a,b after=a,b effort=1 か 1,2,4 wait=2 prod=true not_before=2026-12-01（空で消す） fixed done actual_start actual_finish
//!   E に .toml のパスを渡すと、DB を通さずにそのファイルを読む
//!   置き場  ~/.config/plan/config に server=URL があればサーバー（Cloud Run）の API 越し、無ければ手元の SQLite。PLAN_DB を付けると手元に
//!           plan push [E]  手元の SQLite のエピックをサーバーへ写す（無ければ全部）
use chrono::{Datelike, NaiveDate};
use plan_access::{HttpPlans, JpRuleCalendar, PlanAccess, SqlitePlans, TokenSource, Tokens, TomlFile};
use plan_contracts::*;
use plan_manager::{PlanEditingManager, PlanningManager};
use std::sync::Arc;
use std::path::PathBuf;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};

/// --json：結果を機械が読める形で返す（AI が読む）
static JSON: AtomicBool = AtomicBool::new(false);
fn json() -> bool { JSON.load(Ordering::Relaxed) }
/// 結果を出す。--json なら {"ok":true,"data":…}、そうでなければ人が読む文
fn say(human: impl std::fmt::Display, data: serde_json::Value) {
    if json() { println!("{}", serde_json::json!({ "ok": true, "data": data })) } else { println!("{human}") }
}

mod serve;
mod web;

const W: [&str; 7] = ["月", "火", "水", "木", "金", "土", "日"];

fn label(d: NaiveDate) -> String { format!("{}/{}（{}）", d.month(), d.day(), W[d.weekday().num_days_from_monday() as usize]) }
fn start_label(cal: &BusinessCalendar, s: usize) -> String { format!("{} {}", label(cal.days[s / 2]), if s % 2 == 1 { "午後" } else { "午前" }) }
fn end_label(cal: &BusinessCalendar, f: usize) -> String { let s = f.saturating_sub(1); format!("{} {}", label(cal.days[s / 2]), if s % 2 == 1 { "午後" } else { "午前" }) }
fn end_day(cal: &BusinessCalendar, f: usize) -> NaiveDate { if f > cal.start_slot { cal.days[(f - 1) / 2] } else { cal.days[cal.start_slot / 2] } }

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("PLAN_DB") { return PathBuf::from(p); }
    // バイナリは target/release/plan。その 2 つ上の data/
    std::env::current_exe().ok().and_then(|e| e.parent()?.parent()?.parent().map(|d| d.join("data/plan.db"))).unwrap_or_else(|| PathBuf::from("plan.db"))
}

pub fn local() -> Arc<SqlitePlans> { Arc::new(SqlitePlans::open(&db_path()).unwrap_or_else(|e| die(&e))) }

/// ~/.config/plan/config（1 行 1 つ、k=v）。server=サーバーの URL、token_cmd=トークンを出すコマンド
fn config() -> std::collections::BTreeMap<String, String> {
    let p = std::env::var("HOME").map(|h| PathBuf::from(h).join(".config/plan/config")).unwrap_or_default();
    std::fs::read_to_string(p).unwrap_or_default().lines().filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}
/// サーバーの URL（PLAN_SERVER か config の server）。PLAN_DB を付けたときは手元を使う
fn server() -> Option<(String, Option<String>)> {
    if std::env::var("PLAN_DB").is_ok() { return None; }
    let c = config();
    let url = std::env::var("PLAN_SERVER").ok().or_else(|| c.get("server").cloned()).filter(|s| !s.is_empty())?;
    Some((url, std::env::var("PLAN_TOKEN_CMD").ok().or_else(|| c.get("token_cmd").cloned())))
}
fn remote(url: &str, token_cmd: Option<String>) -> Arc<HttpPlans> { Arc::new(HttpPlans::new(url, token_cmd.map(|c| Tokens::new(TokenSource::Command(c))), "cli")) }

/// 計画の置き場：サーバーがあればその API、無ければ手元の SQLite
pub fn store() -> Arc<dyn PlanAccess + Send + Sync> {
    match server() { Some((u, t)) => remote(&u, t), None => local() }
}
/// 置き場の名前（人に見せる）
fn place() -> String { server().map(|(u, _)| u).unwrap_or_else(|| db_path().display().to_string()) }

/// E が .toml のパスならそのファイル、そうでなければ DB のエピック
fn planning(e: &str) -> (PlanningManager, String) {
    if e.ends_with(".toml") { return (PlanningManager::new(Arc::new(TomlFile(PathBuf::from(e))), Arc::new(JpRuleCalendar)), String::new()); }
    (PlanningManager::new(store(), Arc::new(JpRuleCalendar)), e.to_string())
}

pub fn die(issues: &[Issue]) -> ! {
    if json() { println!("{}", serde_json::json!({ "ok": false, "issues": issues })); exit(1) }
    for i in issues { eprintln!("{}：{}", if i.level == Level::Error { "誤り" } else { "警告" }, i.line()); }
    exit(1)
}

fn pick<'a>(ev: &'a Evaluation, s: Option<&str>, r: Option<&str>, e: Option<&str>) -> &'a RunResult {
    let s = s.map(str::to_string).unwrap_or_else(|| ev.scenarios[0].key.clone());
    let r: f64 = r.and_then(|x| x.parse().ok()).unwrap_or(ev.rates[0]);
    let e = e.and_then(EstKind::parse).unwrap_or(if ev.estimates.contains(&EstKind::E) { EstKind::E } else { ev.estimates[0] });
    let key = run_key(&s, r, e);
    ev.runs.iter().find(|x| x.key == key).unwrap_or_else(|| {
        eprintln!("誤り：組みが無い：{key}（案 {:?}、稼働率 {:?}、見積もり {:?}）", ev.scenarios.iter().map(|s| &s.key).collect::<Vec<_>>(), ev.rates, ev.estimates.iter().map(|e| e.code()).collect::<Vec<_>>());
        exit(1)
    })
}

fn pct(r: f64) -> String { format!("{}%", (r * 100.0).round()) }
fn signed(x: i64) -> String { if x >= 0 { format!("+{x}") } else { x.to_string() } }

fn calc(ev: &Evaluation) {
    let cal = &ev.calendar;
    let tgt = ev.deadline_label.clone().or(ev.deadline_task.clone()).unwrap_or_else(|| "全体の終わり".into());
    println!("# {}（開始 {}）\n", ev.title, start_label(cal, cal.start_slot));
    println!("| 案 | 稼働率 | 見積もり | {tgt} | 締め切りまでの余裕（営業日） | 全体の終わり | 工数の計 | 効率 | リスク（クリティカリティ／フィボナッチ／活動） |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for r in &ev.runs {
        let rk = r.risk.as_ref();
        println!("| {} | {} | {} | {} | {} | {} | {} | {} | {}／{}／{} |", r.scenario, pct(r.rate), r.est.code(), label(end_day(cal, r.target)),
            r.slack_to_deadline.map(signed).unwrap_or_default(), label(end_day(cal, r.end)), fmt_num(r.effort_total),
            r.efficiency.map(|e| pct(e)).unwrap_or_default(),
            rk.map(|x| fmt_num(x.criticality)).unwrap_or_default(), rk.map(|x| fmt_num(x.fibonacci)).unwrap_or_default(),
            rk.and_then(|x| x.activity).map(fmt_num).unwrap_or("—".into()));
    }
    println!("\n案：{}", ev.scenarios.iter().map(|s| format!("{}＝{}", s.key, s.name)).collect::<Vec<_>>().join("、"));
    println!("見積もり：{}", ev.estimates.iter().map(|e| format!("{}＝{}", e.code(), e.name())).collect::<Vec<_>>().join("、"));
    println!("効率＝工数の計 ÷（人数 × 期間の営業日）。リスクの目安は 0.3〜0.75（0.5 まで減圧）");
}

fn color_name(c: FloatColor) -> &'static str {
    match c { FloatColor::Crit => "●クリティカル", FloatColor::Red => "赤", FloatColor::Yellow => "黄", FloatColor::Green => "緑", FloatColor::Wait => "待ち", FloatColor::Done => "済" }
}

fn show(ev: &Evaluation, r: &RunResult) {
    let cal = &ev.calendar;
    let sc = ev.scenarios.iter().find(|s| s.key == r.scenario).unwrap();
    println!("# {}：{}・稼働率 {}・{}\n", ev.title, sc.name, pct(r.rate), r.est.name());
    if let (Some(dl), Some(sl)) = (ev.deadline, r.slack_to_deadline) {
        println!("- {}：{}（締め切り {}、余裕 {} 営業日）", ev.deadline_label.clone().or(ev.deadline_task.clone()).unwrap_or("全体の終わり".into()), label(end_day(cal, r.target)), label(dl), signed(sl));
    }
    println!("- 全体の終わり：{}、クリティカルパス：{}\n", label(end_day(cal, r.end)), r.critical_path.join(" → "));
    println!("| ID | 作業 | チケット | 担当 | 工数 | 待ち | 開始 | 終了 | 余裕 | 色 | 開始を決めたもの |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut ts: Vec<&TaskView> = r.tasks.iter().collect();
    ts.sort_by_key(|t| t.es);
    for t in ts {
        println!("| {} | {}{} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", t.id, if t.prod { "◆ " } else { "" }, t.name, t.ticket, t.owners.join("・"),
            if t.kind == Kind::Work { fmt_num(t.val) } else { String::new() }, if t.kind == Kind::Wait { fmt_num(t.val) } else { String::new() },
            start_label(cal, t.es), if t.ef > t.es { end_label(cal, t.ef) } else { start_label(cal, t.es) },
            t.tf.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or_default(), color_name(t.color), t.why);
    }
}

fn why(ev: &Evaluation, r: &RunResult, id: &str) {
    let cal = &ev.calendar;
    let t = r.tasks.iter().find(|t| t.id == id).unwrap_or_else(|| { eprintln!("誤り：作業 {id} が無い"); exit(1) });
    println!("{} {}", t.id, t.name);
    println!("- 開始 {}：{}", start_label(cal, t.es), t.why);
    if t.kind == Kind::Work {
        println!("- 終了 {}、日数 {}（工数 {} ÷ 稼働率 {} ÷ 担当 {} 人、半日に切り上げ）", end_label(cal, t.ef), fmt_num(t.dur as f64 / 2.0), fmt_num(t.val), pct(r.rate), t.owners.len().max(1));
    } else {
        println!("- 終了 {}、日数 {}", end_label(cal, t.ef.max(t.es + 1)), fmt_num(t.dur as f64 / 2.0));
    }
    println!("- 余裕 {} 日（全体の終わりを動かさずに遅らせられる日数。人の待ちも込み）、自由余裕 {} 日（後の作業を動かさない）",
        t.tf.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or("—".into()), t.ff.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or("—".into()));
    for p in r.tasks.iter().filter(|p| t.after.contains(&p.id)) { println!("- 先行 {}：終了 {}", p.id, end_label(cal, p.ef.max(p.es + 1))); }
}

const TEMPLATE: &str = include_str!("../../../examples/template.toml");

fn parse_est(v: &str) -> Result<Estimate, String> {
    let xs: Vec<f64> = v.split(',').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("数で書く：{v}"))?;
    match xs.as_slice() { [a] => Ok(Estimate::One(*a)), [o, m, p] => Ok(Estimate::Three(*o, *m, *p)), _ => Err(format!("1 つか 3 つ：{v}")) }
}
fn list(v: &str) -> Vec<String> { v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect() }
fn date(v: &str) -> Result<NaiveDate, String> { NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| format!("日付は YYYY-MM-DD：{v}")) }

fn patch(kvs: &[String]) -> Result<TaskPatch, String> {
    let mut p = TaskPatch::default();
    for kv in kvs {
        let (k, v) = kv.split_once('=').ok_or(format!("k=v で書く：{kv}"))?;
        match k {
            "name" => p.name = Some(v.into()), "short" => p.short = Some(v.into()), "story" => p.story = Some(v.into()),
            "ticket" => p.ticket = Some(v.into()), "deliverable" => p.deliverable = Some(v.into()), "note" => p.note = Some(v.into()), "group" => p.group = Some(v.into()),
            "owner" | "owners" => if v.is_empty() { p.clear.push("owners".into()) } else { p.owners = Some(list(v)) },
            "after" => p.after = Some(list(v)),
            "effort" => if v.is_empty() { p.clear.push("effort".into()) } else { p.effort = Some(parse_est(v)?) },
            "wait" => if v.is_empty() { p.clear.push("wait".into()) } else { p.wait = Some(parse_est(v)?) },
            "prod" => p.prod = Some(v == "true"), "done" => p.done = Some(v == "true"),
            "not_before" => if v.is_empty() { p.clear.push("not_before".into()) } else { p.not_before = Some(date(v)?) },
            "fixed" => if v.is_empty() { p.clear.push("fixed".into()) } else { p.fixed = Some(date(v)?) },
            "actual_start" => if v.is_empty() { p.clear.push("actual_start".into()) } else { p.actual_start = Some(date(v)?) },
            "actual_finish" => if v.is_empty() { p.clear.push("actual_finish".into()) } else { p.actual_finish = Some(date(v)?) },
            _ => return Err(format!("知らない項目：{k}")),
        }
    }
    Ok(p)
}

fn task_from(key: &str, p: TaskPatch) -> TaskSpec {
    let mut t = TaskSpec { id: key.into(), ..Default::default() };
    plan_engines_patch(&mut t, &p);
    t
}
fn plan_engines_patch(t: &mut TaskSpec, p: &TaskPatch) {
    if let Some(v) = &p.name { t.name = v.clone(); } if let Some(v) = &p.short { t.short = v.clone(); } if let Some(v) = &p.story { t.story = v.clone(); }
    if let Some(v) = &p.ticket { t.ticket = v.clone(); } if let Some(v) = &p.deliverable { t.deliverable = v.clone(); } if let Some(v) = &p.note { t.note = v.clone(); }
    if let Some(v) = &p.group { t.group = v.clone(); } if p.owners.is_some() { t.owners = p.owners.clone(); } if let Some(v) = &p.after { t.after = v.clone(); }
    t.effort = p.effort.clone(); t.wait = p.wait.clone(); t.prod = p.prod.unwrap_or(false); t.done = p.done.unwrap_or(false); t.not_before = p.not_before; t.fixed = p.fixed; t.actual_start = p.actual_start; t.actual_finish = p.actual_finish;
}

fn edit(epic: &str, ops: Vec<EditOp>) {
    let ed = PlanEditingManager::new(store());
    match ed.apply(epic, "cli", &ops) {
        Ok(()) => say(format!("書いた（{} 件）", ops.len()), serde_json::json!({ "epic": epic, "applied": ops.len(), "version": ed.version() })),
        Err(e) => die(&e),
    }
}

fn main() {
    if std::env::args().any(|a| a == "--json") { JSON.store(true, Ordering::Relaxed); }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos = vec![];
    let (mut s, mut r, mut e, mut port, mut open, mut key) = (None, None, None, 0u16, true, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--scenario" => { i += 1; s = args.get(i).cloned() }
            "-r" | "--rate" => { i += 1; r = args.get(i).cloned() }
            "-e" | "--est" => { i += 1; e = args.get(i).cloned() }
            "--key" => { i += 1; key = args.get(i).cloned() }
            "--port" => { i += 1; port = args.get(i).and_then(|x| x.parse().ok()).unwrap_or(0) }
            "--no-open" => open = false,
            "--json" => {}
            "-h" | "--help" => { println!("{}", include_str!("main.rs").lines().take(11).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n")); return }
            x => pos.push(x.to_string()),
        }
        i += 1;
    }
    let a = |n: usize| pos.get(n).cloned().unwrap_or_else(|| { eprintln!("誤り：引数が足りない（plan -h）"); exit(2) });
    let cmd = pos.first().cloned().unwrap_or_else(|| { eprintln!("使い方：plan -h"); exit(2) });
    match cmd.as_str() {
        "new" => {
            let file = PathBuf::from(a(1));
            if file.exists() { eprintln!("誤り：{} は既にある", file.display()); exit(1) }
            std::fs::write(&file, TEMPLATE.replace("{today}", &chrono::Local::now().date_naive().to_string())).unwrap();
            println!("{} を書いた", file.display());
        }
        "serve" => serve::run(port, open),
        "push" => {
            let (url, t) = server().unwrap_or_else(|| die(&[Issue::error("~/.config/plan/config に server=URL が無い")]));
            let src = local(); let dst = remote(&url, t);
            let keys: Vec<String> = match pos.get(1) { Some(k) => vec![k.clone()], None => src.list_epics().unwrap_or_else(|e| die(&e)).into_iter().map(|x| x.key).collect() };
            for k in &keys { let p = src.read_plan(k).unwrap_or_else(|e| die(&e)); dst.import(&p, "push").unwrap_or_else(|e| die(&e)); }
            say(format!("{} へ写した：{}", url, keys.join("、")), serde_json::json!({ "server": url, "epics": keys }));
        }
        "epics" => {
            let ed = PlanEditingManager::new(store());
            if json() { let l = ed.epics().unwrap_or_else(|e| die(&e)); return say("", serde_json::json!({ "db": place(), "epics": l })); }
            println!("| key | 名前 | 開始 | 締め切り | ストーリー | 作業 | 案 |\n|---|---|---|---|---|---|---|");
            for x in ed.epics().unwrap_or_else(|e| die(&e)) {
                println!("| {} | {} | {} | {} | {} | {} | {} |", x.key, x.name, x.start, x.deadline.map(|d| d.to_string()).unwrap_or_default(), x.stories, x.tasks, x.scenarios);
            }
            println!("\n置き場：{}", place());
        }
        "epic" => match a(1).as_str() {
            "add" => { let ed = PlanEditingManager::new(store()); ed.create_epic(&a(2), &a(3), date(&a(4)).unwrap_or_else(|m| die(&[Issue::error(m)])), "cli").unwrap_or_else(|e| die(&e)); say(format!("作った：{}", a(2)), serde_json::json!({ "epic": a(2) })) }
            "rm" => { let ed = PlanEditingManager::new(store()); ed.delete_epic(&a(2), "cli").unwrap_or_else(|e| die(&e)); say(format!("消した：{}（中身は plan log に残る）", a(2)), serde_json::json!({ "epic": a(2) })) }
            x => { eprintln!("誤り：epic {x}？（add・rm）"); exit(2) }
        },
        "import" => {
            let ed = PlanEditingManager::new(store());
            let k = ed.import(&TomlFile(PathBuf::from(a(1))), key.as_deref(), "import").unwrap_or_else(|e| die(&e));
            say(format!("取り込んだ：{k}"), serde_json::json!({ "epic": k }));
        }
        "dump" => {
            let ed = PlanEditingManager::new(store());
            let p = ed.plan(&a(1)).unwrap_or_else(|e| die(&e));
            if json() { say("", serde_json::to_value(&p).unwrap()) } else { print!("{}", plan_access::plan_access::to_toml(&p)) }
        }
        "apply" => {
            let src = a(2);
            let text = if src == "-" { let mut t = String::new(); std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).unwrap(); t } else { std::fs::read_to_string(&src).unwrap_or_else(|e| die(&[Issue::error(format!("{src} を読めない：{e}"))])) };
            let ops: Vec<EditOp> = serde_json::from_str(&text).unwrap_or_else(|e| die(&[Issue::error(format!("編集の一覧の形が違う：{e}。例 [{{\"op\":\"set_task\",\"key\":\"T1\",\"patch\":{{\"effort\":2}}}}]"))]));
            edit(&a(1), ops)
        }
        "log" => {
            let ed = PlanEditingManager::new(store());
            let hs = ed.history(pos.get(1).map(String::as_str), 50).unwrap_or_else(|e| die(&e));
            if json() {
                let v: Vec<serde_json::Value> = hs.iter().map(|h| serde_json::json!({ "id": h.id, "at": h.at, "actor": h.actor, "epic": h.epic, "op": h.op, "change": serde_json::from_str::<serde_json::Value>(&h.payload).unwrap_or(serde_json::Value::Null) })).collect();
                return say("", serde_json::json!(v));
            }
            for h in hs { println!("{}\t{}\t{}\t{}\t{}\t{}", h.id, h.at, h.actor, h.epic, h.op, h.payload) }
        }
        "task" => {
            let (ep, k) = (a(2), a(3));
            match a(1).as_str() {
                "add" => edit(&ep, vec![EditOp::AddTask { task: task_from(&k, patch(&pos[4..]).unwrap_or_else(|m| die(&[Issue::error(m)]))) }]),
                "set" => edit(&ep, vec![EditOp::SetTask { key: k, patch: patch(&pos[4..]).unwrap_or_else(|m| die(&[Issue::error(m)])), scenario: s }]),
                "rm" => edit(&ep, vec![EditOp::DeleteTask { key: k }]),
                x => { eprintln!("誤り：task {x}？（add・set・rm）"); exit(2) }
            }
        }
        "dep" => match a(1).as_str() {
            "add" => edit(&a(2), vec![EditOp::Link { task: a(3), pred: a(4) }]),
            "rm" => edit(&a(2), vec![EditOp::Unlink { task: a(3), pred: a(4) }]),
            x => { eprintln!("誤り：dep {x}？（add・rm）"); exit(2) }
        },
        "story" => match a(1).as_str() {
            "set" => {
                let mut st = Story { key: a(3), ..Default::default() };
                for kv in &pos[4..] { match kv.split_once('=') { Some(("name", v)) => st.name = v.into(), Some(("purpose", v)) => st.purpose = v.into(), Some(("dod", v)) => st.dod = v.into(), _ => die(&[Issue::error(format!("name・purpose・dod：{kv}"))]) } }
                if st.name.is_empty() { st.name = st.key.clone(); }
                edit(&a(2), vec![EditOp::SetStory { story: st }])
            }
            "rm" => edit(&a(2), vec![EditOp::DeleteStory { key: a(3) }]),
            x => { eprintln!("誤り：story {x}？（set・rm）"); exit(2) }
        },
        "scenario" => match a(1).as_str() {
            "clone" => { let from = a(3); edit(&a(2), vec![EditOp::CloneScenario { from: (from != "-").then_some(from), key: a(4), name: a(5) }]) }
            "rm" => edit(&a(2), vec![EditOp::DeleteScenario { key: a(3) }]),
            x => { eprintln!("誤り：scenario {x}？（clone・rm）"); exit(2) }
        },
        "check" | "calc" | "show" | "why" | "json" | "export" => {
            let target = a(1);
            let (m, ep) = planning(&target);
            if cmd == "check" {
                let is = m.check(&ep);
                let bad = is.iter().any(|i| i.level == Level::Error);
                if json() { println!("{}", serde_json::json!({ "ok": !bad, "issues": is })); exit(if bad { 1 } else { 0 }) }
                for i in &is { println!("{}：{}", if i.level == Level::Error { "誤り" } else { "警告" }, i.line()); }
                if is.is_empty() { println!("誤り・警告なし") }
                exit(if is.iter().any(|i| i.level == Level::Error) { 1 } else { 0 });
            }
            let ev = m.evaluate(&ep).unwrap_or_else(|is| die(&is));
            if json() && cmd != "export" {
                let cal = &ev.calendar;
                let day = |f: usize| end_day(cal, f).to_string();
                let data = match cmd.as_str() {
                    "calc" => serde_json::json!(ev.runs.iter().map(|r| serde_json::json!({ "scenario": r.scenario, "rate": r.rate, "est": r.est, "target": day(r.target), "slack_to_deadline": r.slack_to_deadline, "finish": day(r.end), "effort": r.effort_total, "risk": r.risk })).collect::<Vec<_>>()),
                    "show" => { let r = pick(&ev, s.as_deref(), r.as_deref(), e.as_deref()); serde_json::json!({ "scenario": r.scenario, "rate": r.rate, "est": r.est, "target": day(r.target), "finish": day(r.end), "slack_to_deadline": r.slack_to_deadline, "critical_path": r.critical_path,
                        "tasks": r.tasks.iter().map(|t| serde_json::json!({ "id": t.id, "name": t.name, "story": t.story, "kind": t.kind, "start": cal.days[t.es / 2].to_string(), "start_pm": t.es % 2 == 1, "finish": day(t.ef.max(t.es + 1)), "float_days": t.tf.map(|x| x as f64 / 2.0), "after": t.after, "driven_by": t.why })).collect::<Vec<_>>() }) }
                    "why" => { let r = pick(&ev, s.as_deref(), r.as_deref(), e.as_deref()); let id = a(2); serde_json::to_value(r.tasks.iter().find(|t| t.id == id).unwrap_or_else(|| die(&[Issue::error(format!("作業 {id} が無い"))]))).unwrap() }
                    _ => serde_json::to_value(&ev).unwrap(),
                };
                return say("", data);
            }
            match cmd.as_str() {
                "calc" => calc(&ev),
                "show" => show(&ev, pick(&ev, s.as_deref(), r.as_deref(), e.as_deref())),
                "why" => why(&ev, pick(&ev, s.as_deref(), r.as_deref(), e.as_deref()), &a(2)),
                "json" => println!("{}", serde_json::to_string_pretty(&ev).unwrap()),
                _ => {
                    let plan = if target.ends_with(".toml") { TomlFile(PathBuf::from(&target)).read_plan("").unwrap_or_else(|e| die(&e)) } else { store().read_plan(&ep).unwrap_or_else(|e| die(&e)) };
                    let out = pos.get(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{}.html", if plan.key.is_empty() { "plan" } else { &plan.key })));
                    std::fs::write(&out, web::export_page(&plan)).unwrap();
                    println!("{} を書いた", out.display());
                }
            }
            for i in ev.issues.iter().filter(|i| i.level == Level::Warning) { eprintln!("警告：{}", i.line()); }
        }
        x => { eprintln!("誤り：知らないコマンド {x}（plan -h）"); exit(2) }
    }
}
