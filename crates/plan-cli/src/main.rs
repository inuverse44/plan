//! CLI client: the `plan` command for people and AI to read and edit plans. Plans live in SQLite (PLAN_DB, default data/plan.db in the repository)
//!
//!   read    plan epics | dump E | check E | calc E | show E [-s scenario -r 0.6 -e e] | why E TASK | json E | log [E]
//!           plan load [PERSON]  people across epics: each epic's default run added up per day, days over 100%
//!   create  plan epic add KEY NAME START | epic rm KEY | import F.toml [--key K] | new F.toml (a TOML template)
//!   edit    plan task add E KEY k=v … | task set E KEY k=v … [-s scenario] | task rm E KEY
//!           plan dep add E TASK PRED | dep rm E TASK PRED | story set E KEY name=… | scenario clone E FROM|- KEY NAME | scenario rm E KEY
//!   batch   plan apply E ops.json ("-" for stdin)  apply a list of edits at once; any error writes nothing
//!   machine every command accepts --json and returns {"ok":true,"data":…} or {"ok":false,"issues":[…]}
//!   ui      plan serve [--port N] [--no-open] | export E [out.html]
//!   mcp     plan mcp  an MCP server on stdin/stdout (tools: epics dump show calc why check log load apply delete_epic) for AI assistants
//!   k=v     name short story ticket deliverable note owner=a,b after=a,b needs=prod,db effort=1 or 1,2,4 wait=2 not_before=2026-12-01 (empty clears) fixed done actual_start actual_finish
//!   E may be a path to a .toml file: the file is read directly, without the DB
//!   store   with server=URL in ~/.config/plan/config the CLI goes through the server's API, otherwise the local SQLite. PLAN_DB forces local
//!           plan push [E]  copy epics from the local SQLite to the server (all if E is omitted)
use chrono::{Datelike, NaiveDate};
use plan_access::{HttpPlans, JpRuleCalendar, PlanAccess, SqlitePlans, TokenSource, Tokens, TomlFile};
use plan_contracts::*;
use plan_manager::{PlanEditingManager, PlanningManager};
use std::sync::Arc;
use std::path::PathBuf;
use std::process::exit;
use std::sync::atomic::{AtomicBool, Ordering};

/// --json: return machine-readable results (for AI)
static JSON: AtomicBool = AtomicBool::new(false);
fn json() -> bool { JSON.load(Ordering::Relaxed) }
/// Print a result: {"ok":true,"data":…} with --json, otherwise text for people
fn say(human: impl std::fmt::Display, data: serde_json::Value) {
    if json() { println!("{}", serde_json::json!({ "ok": true, "data": data })) } else { println!("{human}") }
}

mod mcp;
mod serve;
mod web;

const W: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn label(d: NaiveDate) -> String { format!("{} {}/{}", W[d.weekday().num_days_from_monday() as usize], d.month(), d.day()) }
fn start_label(cal: &BusinessCalendar, s: usize) -> String { format!("{} {}", label(cal.days[s / 2]), if s % 2 == 1 { "PM" } else { "AM" }) }
fn end_label(cal: &BusinessCalendar, f: usize) -> String { let s = f.saturating_sub(1); format!("{} {}", label(cal.days[s / 2]), if s % 2 == 1 { "PM" } else { "AM" }) }
fn end_day(cal: &BusinessCalendar, f: usize) -> NaiveDate { if f > cal.start_slot { cal.days[(f - 1) / 2] } else { cal.days[cal.start_slot / 2] } }

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("PLAN_DB") { return PathBuf::from(p); }
    // the binary is target/release/plan; data/ is two directories up
    std::env::current_exe().ok().and_then(|e| e.parent()?.parent()?.parent().map(|d| d.join("data/plan.db"))).unwrap_or_else(|| PathBuf::from("plan.db"))
}

pub fn local() -> Arc<SqlitePlans> { Arc::new(SqlitePlans::open(&db_path()).unwrap_or_else(|e| die(&e))) }

/// ~/.config/plan/config (one k=v per line): server=the server URL, token_cmd=a command that prints a bearer token
fn config() -> std::collections::BTreeMap<String, String> {
    let p = std::env::var("HOME").map(|h| PathBuf::from(h).join(".config/plan/config")).unwrap_or_default();
    std::fs::read_to_string(p).unwrap_or_default().lines().filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}
/// The server URL (PLAN_SERVER or server in the config). With PLAN_DB set, use the local store
fn server() -> Option<(String, Option<String>)> {
    if std::env::var("PLAN_DB").is_ok() { return None; }
    let c = config();
    let url = std::env::var("PLAN_SERVER").ok().or_else(|| c.get("server").cloned()).filter(|s| !s.is_empty())?;
    Some((url, std::env::var("PLAN_TOKEN_CMD").ok().or_else(|| c.get("token_cmd").cloned())))
}
fn remote(url: &str, token_cmd: Option<String>) -> Arc<HttpPlans> { Arc::new(HttpPlans::new(url, token_cmd.map(|c| Tokens::new(TokenSource::Command(c))))) }

/// Where plans live: the server API if configured, otherwise the local SQLite
pub fn store() -> Arc<dyn PlanAccess + Send + Sync> {
    match server() { Some((u, t)) => remote(&u, t), None => local() }
}
/// The store name shown to people
fn place() -> String { server().map(|(u, _)| u).unwrap_or_else(|| db_path().display().to_string()) }

/// E is a .toml path (read the file) or an epic key (read the store)
fn planning(e: &str) -> (PlanningManager, String) {
    if e.ends_with(".toml") { return (PlanningManager::new(Arc::new(TomlFile(PathBuf::from(e))), Arc::new(JpRuleCalendar)), String::new()); }
    (PlanningManager::new(store(), Arc::new(JpRuleCalendar)), e.to_string())
}

pub fn die(issues: &[Issue]) -> ! {
    if json() { println!("{}", serde_json::json!({ "ok": false, "issues": issues })); exit(1) }
    for i in issues { eprintln!("{}: {}", if i.level == Level::Error { "error" } else { "warning" }, i.line()); }
    exit(1)
}

fn pick<'a>(ev: &'a Evaluation, s: Option<&str>, r: Option<&str>, e: Option<&str>) -> Result<&'a RunResult, Vec<Issue>> {
    let s = s.map(str::to_string).unwrap_or_else(|| ev.scenarios[0].key.clone());
    let r: f64 = r.and_then(|x| x.parse().ok()).unwrap_or(ev.rates[0]);
    let e = e.and_then(EstKind::parse).unwrap_or(if ev.estimates.contains(&EstKind::E) { EstKind::E } else { ev.estimates[0] });
    let key = run_key(&s, r, e);
    ev.runs.iter().find(|x| x.key == key).ok_or_else(|| vec![Issue::error(format!("no such run: {key} (scenarios {:?}, utilizations {:?}, estimates {:?})",
        ev.scenarios.iter().map(|s| &s.key).collect::<Vec<_>>(), ev.rates, ev.estimates.iter().map(|e| e.code()).collect::<Vec<_>>()))])
}

/// The machine-readable results shared by `--json` and the MCP tools
pub fn epics_data() -> Result<serde_json::Value, Vec<Issue>> {
    Ok(serde_json::json!({ "db": place(), "epics": PlanEditingManager::new(store()).epics()? }))
}
pub fn dump_data(epic: &str) -> Result<serde_json::Value, Vec<Issue>> {
    Ok(serde_json::to_value(PlanEditingManager::new(store()).plan(epic)?).unwrap())
}
pub fn log_data(epic: Option<&str>, limit: usize) -> Result<serde_json::Value, Vec<Issue>> {
    let hs = PlanEditingManager::new(store()).history(epic, limit)?;
    Ok(serde_json::json!(hs.iter().map(|h| serde_json::json!({ "id": h.id, "at": h.at, "actor": h.actor, "epic": h.epic, "op": h.op,
        "change": serde_json::from_str::<serde_json::Value>(&h.payload).unwrap_or(serde_json::Value::Null) })).collect::<Vec<_>>()))
}
pub fn apply_data(epic: &str, actor: &str, ops: &[EditOp]) -> Result<serde_json::Value, Vec<Issue>> {
    let pm = PlanningManager::new(store(), Arc::new(JpRuleCalendar));
    let before = pm.evaluate(epic).ok();
    let ed = PlanEditingManager::new(store());
    ed.apply(epic, actor, ops)?;
    let after = pm.evaluate(epic);
    Ok(serde_json::json!({ "epic": epic, "applied": ops.len(), "version": ed.version(), "impact": impact(before.as_ref(), after) }))
}

/// One run's headline: the date compared to the deadline, the end, slack and the critical path
fn headline(ev: &Evaluation, r: &RunResult) -> serde_json::Value {
    let day = |f: usize| end_day(&ev.calendar, f).to_string();
    serde_json::json!({ "target": day(r.target), "finish": day(r.end), "slack_to_deadline": r.slack_to_deadline, "critical_path": r.critical_path })
}

/// How the edits moved the schedule: the default run (first scenario, first utilization, e) before and after,
/// and every other run whose date, end or slack moved. `before` is null when the plan could not be evaluated before
fn impact(before: Option<&Evaluation>, after: Result<Evaluation, Vec<Issue>>) -> serde_json::Value {
    let after = match after { Ok(a) => a, Err(is) => return serde_json::json!({ "error": is }) };
    let Ok(ra) = pick(&after, None, None, None) else { return serde_json::Value::Null };
    let rb = before.and_then(|b| b.runs.iter().find(|r| r.key == ra.key).map(|r| (b, r)));
    let a = headline(&after, ra);
    let b = rb.map(|(ev, r)| headline(ev, r));
    let moved: Vec<serde_json::Value> = after.runs.iter().filter(|r| r.key != ra.key).filter_map(|r| {
        let (bev, br) = before.and_then(|b| b.runs.iter().find(|x| x.key == r.key).map(|x| (b, x)))?;
        let (hb, ha) = (headline(bev, br), headline(&after, r));
        let same = ["target", "finish", "slack_to_deadline"].iter().all(|k| hb[k] == ha[k]);
        (!same).then(|| serde_json::json!({ "scenario": r.scenario, "rate": r.rate, "est": r.est,
            "target": [hb["target"], ha["target"]], "finish": [hb["finish"], ha["finish"]], "slack_to_deadline": [hb["slack_to_deadline"], ha["slack_to_deadline"]] }))
    }).collect();
    let changed = b.as_ref().map(|b| b != &a).unwrap_or(true);
    serde_json::json!({ "run": { "scenario": ra.scenario, "rate": ra.rate, "est": ra.est }, "changed": changed, "before": b, "after": a, "other_runs_moved": moved })
}

/// The impact as one or two lines for people
fn impact_text(i: &serde_json::Value) -> String {
    if let Some(e) = i.get("error") { return format!("schedule not evaluated: {e}"); }
    let (Some(a), Some(run)) = (i.get("after"), i.get("run")) else { return String::new() };
    let head = format!("{} · {} · {}", run["scenario"].as_str().unwrap_or(""), run["rate"], run["est"].as_str().unwrap_or(""));
    let s = |v: &serde_json::Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
    let mut out = match i.get("before").filter(|b| !b.is_null()) {
        Some(_) if i["changed"] == serde_json::json!(false) => format!("{head}: no change (target {}, slack {})", s(&a["target"]), s(&a["slack_to_deadline"])),
        Some(b) => {
            let mut t = format!("{head}: target {} → {}, finish {} → {}, slack {} → {}", s(&b["target"]), s(&a["target"]), s(&b["finish"]), s(&a["finish"]), s(&b["slack_to_deadline"]), s(&a["slack_to_deadline"]));
            if b["critical_path"] != a["critical_path"] { t += &format!("\n  critical path: {}", a["critical_path"].as_array().map(|x| x.iter().map(s).collect::<Vec<_>>().join(" → ")).unwrap_or_default()); }
            t
        }
        None => format!("{head}: target {}, finish {}, slack {}", s(&a["target"]), s(&a["finish"]), s(&a["slack_to_deadline"])),
    };
    let n = i["other_runs_moved"].as_array().map(|x| x.len()).unwrap_or(0);
    if n > 0 { out += &format!("\n  {n} other run(s) moved (plan calc)"); }
    out
}
/// People across epics: each epic's default run, added up per person and day. A day over 1 (one person's whole day) is over 100%
pub fn load_data(person: Option<&str>) -> Result<serde_json::Value, Vec<Issue>> {
    use std::collections::BTreeMap;
    let ls = PlanningManager::new(store(), Arc::new(JpRuleCalendar)).epic_loads()?;
    let mut by: BTreeMap<&str, BTreeMap<NaiveDate, BTreeMap<&str, f64>>> = BTreeMap::new();
    for l in &ls {
        for (p, days) in &l.load {
            if person.is_some_and(|x| x != p) { continue; }
            for (d, v) in days { if *v > 0.0 { *by.entry(p).or_default().entry(*d).or_default().entry(&l.epic).or_default() += v; } }
        }
    }
    let r2 = |x: f64| (x * 100.0).round() / 100.0;
    let people: serde_json::Map<String, serde_json::Value> = by.iter().map(|(p, days)| {
        let ds: Vec<serde_json::Value> = days.iter().map(|(d, es)| { let total: f64 = es.values().sum();
            serde_json::json!({ "date": d, "total": r2(total), "over": total > 1.0 + 1e-6, "by_epic": es.iter().map(|(e, v)| (e.to_string(), serde_json::json!(r2(*v)))).collect::<serde_json::Map<_, _>>() }) }).collect();
        let over: Vec<&serde_json::Value> = ds.iter().filter(|d| d["over"] == serde_json::json!(true)).map(|d| &d["date"]).collect();
        let epics: std::collections::BTreeSet<&str> = days.values().flat_map(|es| es.keys().copied()).collect();
        (p.to_string(), serde_json::json!({ "epics": epics, "over": over, "days": ds }))
    }).collect();
    Ok(serde_json::json!({
        "epics": ls.iter().map(|l| serde_json::json!({ "epic": l.epic, "name": l.name, "scenario": l.scenario, "rate": l.rate, "est": l.est, "issues": l.issues })).collect::<Vec<_>>(),
        "people": people,
    }))
}

fn load(person: Option<&str>) {
    let d = load_data(person).unwrap_or_else(|e| die(&e));
    if json() { return say("", d); }
    println!("# Load across epics (each epic's default run; 1 = one person's whole day)\n");
    for e in d["epics"].as_array().unwrap() {
        let is = e["issues"].as_array().map(|x| x.len()).unwrap_or(0);
        if is > 0 { println!("- {}: not evaluated ({is} issue(s); plan check {})", e["epic"].as_str().unwrap_or(""), e["epic"].as_str().unwrap_or("")); }
        else { println!("- {}: {} · {} · {}", e["epic"].as_str().unwrap_or(""), e["scenario"].as_str().unwrap_or(""), pct(e["rate"].as_f64().unwrap_or(1.0)), e["est"].as_str().unwrap_or("")); }
    }
    for (p, v) in d["people"].as_object().unwrap() {
        let epics: Vec<&str> = v["epics"].as_array().unwrap().iter().filter_map(|x| x.as_str()).collect();
        let days = v["days"].as_array().unwrap();
        let peak = days.iter().filter_map(|x| x["total"].as_f64()).fold(0.0, f64::max);
        let over: Vec<&serde_json::Value> = days.iter().filter(|x| x["over"] == serde_json::json!(true)).collect();
        println!("\n## {p} ({}): {} day(s) over 100%, peak {}", epics.join(", "), over.len(), pct(peak));
        if over.is_empty() { continue; }
        println!("\n| Date | Total | {} |\n|---|---|{}", epics.join(" | "), "---|".repeat(epics.len()));
        for x in over {
            let cell = |e: &str| x["by_epic"].get(e).and_then(|v| v.as_f64()).map(fmt_num).unwrap_or_default();
            println!("| {} | {} | {} |", label(NaiveDate::parse_from_str(x["date"].as_str().unwrap(), "%Y-%m-%d").unwrap()), pct(x["total"].as_f64().unwrap()), epics.iter().map(|e| cell(e)).collect::<Vec<_>>().join(" | "));
        }
    }
}

/// check: all issues; ok is false when any is an error
pub fn check_data(target: &str) -> (bool, Vec<Issue>) {
    let (m, ep) = planning(target);
    let is = m.check(&ep);
    (!is.iter().any(|i| i.level == Level::Error), is)
}
/// calc, show and why as JSON. `run` picks scenario, utilization and estimate; `task` is for why
pub fn eval_data(cmd: &str, target: &str, run: (Option<&str>, Option<&str>, Option<&str>), task: Option<&str>) -> Result<serde_json::Value, Vec<Issue>> {
    let (m, ep) = planning(target);
    let ev = m.evaluate(&ep)?;
    let cal = &ev.calendar;
    let day = |f: usize| end_day(cal, f).to_string();
    Ok(match cmd {
        "calc" => serde_json::json!(ev.runs.iter().map(|r| serde_json::json!({ "scenario": r.scenario, "rate": r.rate, "est": r.est, "target": day(r.target), "slack_to_deadline": r.slack_to_deadline, "finish": day(r.end), "effort": r.effort_total, "risk": r.risk })).collect::<Vec<_>>()),
        "show" => { let r = pick(&ev, run.0, run.1, run.2)?; serde_json::json!({ "scenario": r.scenario, "rate": r.rate, "est": r.est, "target": day(r.target), "finish": day(r.end), "slack_to_deadline": r.slack_to_deadline, "critical_path": r.critical_path,
            "tasks": r.tasks.iter().map(|t| serde_json::json!({ "id": t.id, "name": t.name, "story": t.story, "kind": t.kind, "start": cal.days[t.es / 2].to_string(), "start_pm": t.es % 2 == 1, "finish": day(t.ef.max(t.es + 1)), "float_days": t.tf.map(|x| x as f64 / 2.0), "after": t.after, "driven_by": t.why })).collect::<Vec<_>>() }) }
        "why" => {
            let r = pick(&ev, run.0, run.1, run.2)?;
            let id = task.ok_or_else(|| vec![Issue::error("why needs a task")])?;
            serde_json::to_value(r.tasks.iter().find(|t| t.id == id).ok_or_else(|| vec![Issue::error(format!("no task {id}"))])?).unwrap()
        }
        _ => serde_json::to_value(&ev).unwrap(),
    })
}

fn pct(r: f64) -> String { format!("{}%", (r * 100.0).round()) }
fn signed(x: i64) -> String { if x >= 0 { format!("+{x}") } else { x.to_string() } }

fn calc(ev: &Evaluation) {
    let cal = &ev.calendar;
    let tgt = ev.deadline_label.clone().or(ev.deadline_task.clone()).unwrap_or_else(|| "End".into());
    println!("# {} (start {})\n", ev.title, start_label(cal, cal.start_slot));
    println!("| Scenario | Utilization | Estimate | {tgt} | Slack to deadline (business days) | End | Total effort | Efficiency | Risk (criticality / Fibonacci / activity) |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for r in &ev.runs {
        let rk = r.risk.as_ref();
        println!("| {} | {} | {} | {} | {} | {} | {} | {} | {} / {} / {} |", r.scenario, pct(r.rate), r.est.code(), label(end_day(cal, r.target)),
            r.slack_to_deadline.map(signed).unwrap_or_default(), label(end_day(cal, r.end)), fmt_num(r.effort_total),
            r.efficiency.map(|e| pct(e)).unwrap_or_default(),
            rk.map(|x| fmt_num(x.criticality)).unwrap_or_default(), rk.map(|x| fmt_num(x.fibonacci)).unwrap_or_default(),
            rk.and_then(|x| x.activity).map(fmt_num).unwrap_or("—".into()));
    }
    println!("\nScenarios: {}", ev.scenarios.iter().map(|s| format!("{} = {}", s.key, s.name)).collect::<Vec<_>>().join(", "));
    println!("Estimates: {}", ev.estimates.iter().map(|e| format!("{} = {}", e.code(), e.name())).collect::<Vec<_>>().join(", "));
    println!("Efficiency = total effort / (people × business days). Healthy risk is 0.3 to 0.75 (decompress down to 0.5)");
}

fn color_name(c: FloatColor) -> &'static str {
    match c { FloatColor::Crit => "● critical", FloatColor::Red => "low", FloatColor::Yellow => "mid", FloatColor::Green => "high", FloatColor::Wait => "wait", FloatColor::Done => "done" }
}

fn show(ev: &Evaluation, r: &RunResult) {
    let cal = &ev.calendar;
    let sc = ev.scenarios.iter().find(|s| s.key == r.scenario).unwrap();
    println!("# {}: {} · utilization {} · {}\n", ev.title, sc.name, pct(r.rate), r.est.name());
    if let (Some(dl), Some(sl)) = (ev.deadline, r.slack_to_deadline) {
        println!("- {}: {} (deadline {}, slack {} business days)", ev.deadline_label.clone().or(ev.deadline_task.clone()).unwrap_or("End".into()), label(end_day(cal, r.target)), label(dl), signed(sl));
    }
    println!("- End: {}, critical path: {}\n", label(end_day(cal, r.end)), r.critical_path.join(" → "));
    println!("| ID | Task | Ticket | Owner | Effort | Wait | Start | Finish | Float | Color | Driven by |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    let mut ts: Vec<&TaskView> = r.tasks.iter().collect();
    ts.sort_by_key(|t| t.es);
    for t in ts {
        println!("| {} | {}{} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", t.id, if t.needs.is_empty() { "" } else { "◆ " }, t.name, t.ticket, t.owners.join(", "),
            if t.kind == Kind::Work { fmt_num(t.val) } else { String::new() }, if t.kind == Kind::Wait { fmt_num(t.val) } else { String::new() },
            start_label(cal, t.es), if t.ef > t.es { end_label(cal, t.ef) } else { start_label(cal, t.es) },
            t.tf.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or_default(), color_name(t.color), t.why);
    }
}

fn why(ev: &Evaluation, r: &RunResult, id: &str) {
    let cal = &ev.calendar;
    let t = r.tasks.iter().find(|t| t.id == id).unwrap_or_else(|| { eprintln!("error: no task {id}"); exit(1) });
    println!("{} {}", t.id, t.name);
    println!("- Start {}: {}", start_label(cal, t.es), t.why);
    if t.kind == Kind::Work {
        println!("- Finish {}, {} days (effort {} / utilization {} / {} owner(s), rounded up to half a day)", end_label(cal, t.ef), fmt_num(t.dur as f64 / 2.0), fmt_num(t.val), pct(r.rate), t.owners.len().max(1));
    } else {
        println!("- Finish {}, {} days", end_label(cal, t.ef.max(t.es + 1)), fmt_num(t.dur as f64 / 2.0));
    }
    println!("- Float {} days (delay possible without moving the end, owners' availability included), free float {} days (without moving the next task)",
        t.tf.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or("—".into()), t.ff.map(|x| fmt_num(x as f64 / 2.0)).unwrap_or("—".into()));
    for p in r.tasks.iter().filter(|p| t.after.contains(&p.id)) { println!("- After {}: finishes {}", p.id, end_label(cal, p.ef.max(p.es + 1))); }
}

const TEMPLATE: &str = include_str!("../../../examples/template.toml");

fn parse_est(v: &str) -> Result<Estimate, String> {
    let xs: Vec<f64> = v.split(',').map(|x| x.trim().parse::<f64>()).collect::<Result<_, _>>().map_err(|_| format!("expected numbers: {v}"))?;
    match xs.as_slice() { [a] => Ok(Estimate::One(*a)), [o, m, p] => Ok(Estimate::Three(*o, *m, *p)), _ => Err(format!("expected one or three numbers: {v}")) }
}
fn list(v: &str) -> Vec<String> { v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect() }
fn date(v: &str) -> Result<NaiveDate, String> { NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| format!("dates are YYYY-MM-DD: {v}")) }

fn patch(kvs: &[String]) -> Result<TaskPatch, String> {
    let mut p = TaskPatch::default();
    for kv in kvs {
        let (k, v) = kv.split_once('=').ok_or(format!("expected k=v: {kv}"))?;
        match k {
            "name" => p.name = Some(v.into()), "short" => p.short = Some(v.into()), "story" => p.story = Some(v.into()),
            "ticket" => p.ticket = Some(v.into()), "deliverable" => p.deliverable = Some(v.into()), "note" => p.note = Some(v.into()), "group" => p.group = Some(v.into()),
            "owner" | "owners" => if v.is_empty() { p.clear.push("owners".into()) } else { p.owners = Some(list(v)) },
            "after" => p.after = Some(list(v)),
            "effort" => if v.is_empty() { p.clear.push("effort".into()) } else { p.effort = Some(parse_est(v)?) },
            "wait" => if v.is_empty() { p.clear.push("wait".into()) } else { p.wait = Some(parse_est(v)?) },
            "needs" => p.needs = Some(list(v)),
            "prod" => p.prod = Some(v == "true"), // the old form of needs=prod (resolved against the task's needs)
            "done" => p.done = Some(v == "true"),
            "not_before" => if v.is_empty() { p.clear.push("not_before".into()) } else { p.not_before = Some(date(v)?) },
            "fixed" => if v.is_empty() { p.clear.push("fixed".into()) } else { p.fixed = Some(date(v)?) },
            "actual_start" => if v.is_empty() { p.clear.push("actual_start".into()) } else { p.actual_start = Some(date(v)?) },
            "actual_finish" => if v.is_empty() { p.clear.push("actual_finish".into()) } else { p.actual_finish = Some(date(v)?) },
            _ => return Err(format!("unknown key: {k}")),
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
    t.effort = p.effort.clone(); t.wait = p.wait.clone(); t.needs = p.needs.clone().unwrap_or_default(); t.prod = p.prod.unwrap_or(false); t.normalize(); t.done = p.done.unwrap_or(false); t.not_before = p.not_before; t.fixed = p.fixed; t.actual_start = p.actual_start; t.actual_finish = p.actual_finish;
}

fn edit(epic: &str, ops: Vec<EditOp>) {
    match apply_data(epic, "cli", &ops) {
        Ok(d) => say(format!("applied {} edit(s)\n{}", ops.len(), impact_text(&d["impact"])), d),
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
            "-h" | "--help" => { println!("{}", include_str!("main.rs").lines().take(13).map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n")); return }
            x => pos.push(x.to_string()),
        }
        i += 1;
    }
    let a = |n: usize| pos.get(n).cloned().unwrap_or_else(|| { eprintln!("error: missing arguments (plan -h)"); exit(2) });
    let cmd = pos.first().cloned().unwrap_or_else(|| { eprintln!("usage: plan -h"); exit(2) });
    match cmd.as_str() {
        "new" => {
            let file = PathBuf::from(a(1));
            if file.exists() { eprintln!("error: {} already exists", file.display()); exit(1) }
            std::fs::write(&file, TEMPLATE.replace("{today}", &chrono::Local::now().date_naive().to_string())).unwrap();
            println!("wrote {}", file.display());
        }
        "serve" => serve::run(port, open),
        "mcp" => mcp::run(),
        "push" => {
            let (url, t) = server().unwrap_or_else(|| die(&[Issue::error("no server=URL in ~/.config/plan/config")]));
            let src = local(); let dst = remote(&url, t);
            let keys: Vec<String> = match pos.get(1) { Some(k) => vec![k.clone()], None => src.list_epics().unwrap_or_else(|e| die(&e)).into_iter().map(|x| x.key).collect() };
            for k in &keys { let p = src.read_plan(k).unwrap_or_else(|e| die(&e)); dst.import(&p, "push").unwrap_or_else(|e| die(&e)); }
            say(format!("copied to {}: {}", url, keys.join(", ")), serde_json::json!({ "server": url, "epics": keys }));
        }
        "epics" => {
            if json() { return say("", epics_data().unwrap_or_else(|e| die(&e))); }
            let ed = PlanEditingManager::new(store());
            println!("| key | name | start | deadline | stories | tasks | scenarios |\n|---|---|---|---|---|---|---|");
            for x in ed.epics().unwrap_or_else(|e| die(&e)) {
                println!("| {} | {} | {} | {} | {} | {} | {} |", x.key, x.name, x.start, x.deadline.map(|d| d.to_string()).unwrap_or_default(), x.stories, x.tasks, x.scenarios);
            }
            println!("\nstore: {}", place());
        }
        "epic" => match a(1).as_str() {
            "add" => { let ed = PlanEditingManager::new(store()); ed.create_epic(&a(2), &a(3), date(&a(4)).unwrap_or_else(|m| die(&[Issue::error(m)])), "cli").unwrap_or_else(|e| die(&e)); say(format!("created: {}", a(2)), serde_json::json!({ "epic": a(2) })) }
            "rm" => { let ed = PlanEditingManager::new(store()); ed.delete_epic(&a(2), "cli").unwrap_or_else(|e| die(&e)); say(format!("deleted: {} (its content stays in plan log)", a(2)), serde_json::json!({ "epic": a(2) })) }
            x => { eprintln!("error: epic {x}? (add, rm)"); exit(2) }
        },
        "import" => {
            let ed = PlanEditingManager::new(store());
            let k = ed.import(&TomlFile(PathBuf::from(a(1))), key.as_deref(), "import").unwrap_or_else(|e| die(&e));
            say(format!("imported: {k}"), serde_json::json!({ "epic": k }));
        }
        "dump" => {
            if json() { return say("", dump_data(&a(1)).unwrap_or_else(|e| die(&e))); }
            let p = PlanEditingManager::new(store()).plan(&a(1)).unwrap_or_else(|e| die(&e));
            print!("{}", plan_access::plan_access::to_toml(&p))
        }
        "apply" => {
            let src = a(2);
            let text = if src == "-" { let mut t = String::new(); std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).unwrap(); t } else { std::fs::read_to_string(&src).unwrap_or_else(|e| die(&[Issue::error(format!("cannot read {src}: {e}"))])) };
            let ops: Vec<EditOp> = serde_json::from_str(&text).unwrap_or_else(|e| die(&[Issue::error(format!("edits must be a JSON list: {e}. Example: [{{\"op\":\"set_task\",\"key\":\"T1\",\"patch\":{{\"effort\":2}}}}]"))]));
            edit(&a(1), ops)
        }
        "load" => load(pos.get(1).map(String::as_str)),
        "log" => {
            if json() { return say("", log_data(pos.get(1).map(String::as_str), 50).unwrap_or_else(|e| die(&e))); }
            let hs = PlanEditingManager::new(store()).history(pos.get(1).map(String::as_str), 50).unwrap_or_else(|e| die(&e));
            for h in hs { println!("{}\t{}\t{}\t{}\t{}\t{}", h.id, h.at, h.actor, h.epic, h.op, h.payload) }
        }
        "task" => {
            let (ep, k) = (a(2), a(3));
            match a(1).as_str() {
                "add" => edit(&ep, vec![EditOp::AddTask { task: task_from(&k, patch(&pos[4..]).unwrap_or_else(|m| die(&[Issue::error(m)]))) }]),
                "set" => edit(&ep, vec![EditOp::SetTask { key: k, patch: patch(&pos[4..]).unwrap_or_else(|m| die(&[Issue::error(m)])), scenario: s }]),
                "rm" => edit(&ep, vec![EditOp::DeleteTask { key: k }]),
                x => { eprintln!("error: task {x}? (add, set, rm)"); exit(2) }
            }
        }
        "dep" => match a(1).as_str() {
            "add" => edit(&a(2), vec![EditOp::Link { task: a(3), pred: a(4) }]),
            "rm" => edit(&a(2), vec![EditOp::Unlink { task: a(3), pred: a(4) }]),
            x => { eprintln!("error: dep {x}? (add, rm)"); exit(2) }
        },
        "story" => match a(1).as_str() {
            "set" => {
                let mut st = Story { key: a(3), ..Default::default() };
                for kv in &pos[4..] { match kv.split_once('=') { Some(("name", v)) => st.name = v.into(), Some(("purpose", v)) => st.purpose = v.into(), Some(("dod", v)) => st.dod = v.into(), _ => die(&[Issue::error(format!("expected name, purpose or dod: {kv}"))]) } }
                if st.name.is_empty() { st.name = st.key.clone(); }
                edit(&a(2), vec![EditOp::SetStory { story: st }])
            }
            "rm" => edit(&a(2), vec![EditOp::DeleteStory { key: a(3) }]),
            x => { eprintln!("error: story {x}? (set, rm)"); exit(2) }
        },
        "scenario" => match a(1).as_str() {
            "clone" => { let from = a(3); edit(&a(2), vec![EditOp::CloneScenario { from: (from != "-").then_some(from), key: a(4), name: a(5) }]) }
            "rm" => edit(&a(2), vec![EditOp::DeleteScenario { key: a(3) }]),
            x => { eprintln!("error: scenario {x}? (clone, rm)"); exit(2) }
        },
        "check" | "calc" | "show" | "why" | "json" | "export" => {
            let target = a(1);
            if cmd == "check" {
                let (ok, is) = check_data(&target);
                if json() { println!("{}", serde_json::json!({ "ok": ok, "issues": is })); exit(if ok { 0 } else { 1 }) }
                for i in &is { println!("{}: {}", if i.level == Level::Error { "error" } else { "warning" }, i.line()); }
                if is.is_empty() { println!("no errors or warnings") }
                exit(if ok { 0 } else { 1 });
            }
            if json() && cmd != "export" {
                let task = if cmd == "why" { Some(a(2)) } else { None };
                return say("", eval_data(&cmd, &target, (s.as_deref(), r.as_deref(), e.as_deref()), task.as_deref()).unwrap_or_else(|e| die(&e)));
            }
            let (m, ep) = planning(&target);
            let ev = m.evaluate(&ep).unwrap_or_else(|is| die(&is));
            let run = || pick(&ev, s.as_deref(), r.as_deref(), e.as_deref()).unwrap_or_else(|e| die(&e));
            match cmd.as_str() {
                "calc" => calc(&ev),
                "show" => show(&ev, run()),
                "why" => why(&ev, run(), &a(2)),
                "json" => println!("{}", serde_json::to_string_pretty(&ev).unwrap()),
                _ => {
                    let plan = if target.ends_with(".toml") { TomlFile(PathBuf::from(&target)).read_plan("").unwrap_or_else(|e| die(&e)) } else { store().read_plan(&ep).unwrap_or_else(|e| die(&e)) };
                    let out = pos.get(2).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{}.html", if plan.key.is_empty() { "plan" } else { &plan.key })));
                    std::fs::write(&out, web::export_page(&plan)).unwrap();
                    println!("wrote {}", out.display());
                }
            }
            for i in ev.issues.iter().filter(|i| i.level == Level::Warning) { eprintln!("warning: {}", i.line()); }
        }
        x => { eprintln!("error: unknown command {x} (plan -h)"); exit(2) }
    }
}
