//! PlanAccess: read and edit an epic's plan, and record the changes.
//! Implementations: SqlitePlans and FirestorePlans (stores), HttpPlans (a server), TomlFile (import and export), TomlText / JsonPlan (in the browser, read only)
use chrono::NaiveDate;
use plan_contracts::*;
use std::collections::BTreeMap;
use toml::{Table, Value};

pub type Validate<'a> = &'a dyn Fn(&Plan) -> Vec<Issue>;

pub trait PlanAccess {
    fn read_plan(&self, epic: &str) -> Result<Plan, Vec<Issue>>;
    fn list_epics(&self) -> Result<Vec<EpicSummary>, Vec<Issue>> { Err(vec![Issue::error("this store has no list of epics")]) }
    /// Apply a batch of edits. The result goes through validate; on any error nothing is written
    fn apply(&self, _epic: &str, _actor: &str, _ops: &[EditOp], _validate: Validate) -> Result<(), Vec<Issue>> { Err(vec![Issue::error("this store is read only")]) }
    fn create_epic(&self, _key: &str, _name: &str, _start: NaiveDate, _actor: &str) -> Result<(), Vec<Issue>> { Err(vec![Issue::error("this store is read only")]) }
    fn delete_epic(&self, _key: &str, _actor: &str) -> Result<(), Vec<Issue>> { Err(vec![Issue::error("this store is read only")]) }
    /// Store a whole plan (replacing an epic with the same key)
    fn import(&self, _plan: &Plan, _actor: &str) -> Result<(), Vec<Issue>> { Err(vec![Issue::error("this store is read only")]) }
    fn history(&self, _epic: Option<&str>, _limit: usize) -> Result<Vec<HistoryEntry>, Vec<Issue>> { Ok(vec![]) }
    /// A number that grows on every change (the UI reloads when it changes)
    fn version(&self) -> i64 { 0 }
}

/// TOML received as a string
pub struct TomlText(pub String);
impl PlanAccess for TomlText {
    fn read_plan(&self, _: &str) -> Result<Plan, Vec<Issue>> { parse(&self.0) }
}

/// One epic received from the server (JSON). Used in the browser
pub struct JsonPlan(pub Plan);
impl PlanAccess for JsonPlan {
    fn read_plan(&self, _: &str) -> Result<Plan, Vec<Issue>> { Ok(self.0.clone()) }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct TomlFile(pub std::path::PathBuf);

#[cfg(not(target_arch = "wasm32"))]
impl PlanAccess for TomlFile {
    fn read_plan(&self, _: &str) -> Result<Plan, Vec<Issue>> {
        let text = std::fs::read_to_string(&self.0).map_err(|e| vec![Issue::error(format!("cannot read {}: {e}", self.0.display()))])?;
        let mut p = parse(&text)?;
        if p.key.is_empty() { p.key = self.0.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(); }
        Ok(p)
    }
}

// ---------- Apply edits to a plan (independent of the store) ----------

pub fn mutate(plan: &mut Plan, op: &EditOp) -> Result<(), Issue> {
    let has = |p: &Plan, k: &str| p.tasks.iter().any(|t| t.id == k);
    match op {
        EditOp::SetEpic { settings: s } => {
            if let Some(v) = &s.name { plan.title = v.clone(); }
            if let Some(v) = &s.description { plan.description = v.clone(); }
            if let Some(v) = s.start { plan.start = v; }
            if let Some(v) = s.start_pm { plan.start_pm = v; }
            if let Some(v) = &s.deadline { plan.deadline = *v; }
            if let Some(v) = &s.deadline_task { plan.deadline_task = v.clone(); }
            if let Some(v) = &s.deadline_label { plan.deadline_label = v.clone(); }
            if let Some(v) = &s.rates { if v.is_empty() { return Err(Issue::error("rates is empty")); } plan.rates = v.clone(); }
            if let Some(v) = &s.default_owner { plan.default_owner = v.clone(); }
            if let Some(v) = s.max_task_days { if v.is_some_and(|x| !(x >= 0.0) || !x.is_finite()) { return Err(Issue::error("max_task_days must be a number ≥ 0")); } plan.max_task_days = v; }
            if let Some(v) = &s.owners { plan.owners = v.iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(); }
        }
        EditOp::SetStory { story } => {
            if story.key.is_empty() { return Err(Issue::error("story key is empty")); }
            match plan.stories.iter_mut().find(|s| s.key == story.key) { Some(s) => *s = story.clone(), None => plan.stories.push(story.clone()) }
        }
        EditOp::DeleteStory { key } => {
            plan.stories.retain(|s| &s.key != key);
            plan.comments.retain(|c| &c.target != key);
            for t in plan.tasks.iter_mut().filter(|t| &t.story == key) { t.story.clear(); }
        }
        EditOp::AddTask { task } => {
            if task.id.is_empty() { return Err(Issue::error("task key is empty")); }
            if has(plan, &task.id) { return Err(Issue::error(format!("task {} already exists", task.id))); }
            let mut t = task.clone();
            t.normalize();
            if t.name.is_empty() { t.name = t.id.clone(); }
            if t.short.is_empty() { t.short = t.name.clone(); }
            plan.tasks.push(t);
        }
        EditOp::SetTask { key, patch, scenario: None } => {
            let t = plan.tasks.iter_mut().find(|t| &t.id == key).ok_or_else(|| Issue::error(format!("no task {key}")))?;
            let mut patch = patch.clone();
            patch.resolve(&t.needs);
            plan_patch(t, &patch);
        }
        EditOp::SetTask { key, patch, scenario: Some(sk) } => {
            let base = plan.tasks.iter().find(|t| &t.id == key).map(|t| t.needs.clone()).unwrap_or_default();
            let sc = plan.scenarios.iter_mut().find(|s| &s.key == sk).ok_or_else(|| Issue::error(format!("no scenario {sk}")))?;
            let mut patch = patch.clone();
            if let Some(a) = sc.add.iter_mut().find(|t| &t.id == key) { patch.resolve(&a.needs); plan_patch(a, &patch); return Ok(()); }
            match sc.set.iter_mut().find(|(k, _)| k == key) {
                Some((_, p)) => { patch.resolve(p.needs.as_deref().unwrap_or(&base)); merge_patch(p, &patch) }
                None => { patch.resolve(&base); sc.set.push((key.clone(), patch)) }
            }
        }
        EditOp::DeleteTask { key } => {
            if !has(plan, key) { return Err(Issue::error(format!("no task {key}"))); }
            plan.tasks.retain(|t| &t.id != key);
            plan.comments.retain(|c| &c.target != key);
            for t in plan.tasks.iter_mut() { t.after.retain(|p| p != key); }
            for sc in plan.scenarios.iter_mut() {
                sc.set.retain(|(k, _)| k != key);
                sc.drop.retain(|k| k != key);
                for (_, p) in sc.set.iter_mut() { if let Some(a) = p.after.as_mut() { a.retain(|x| x != key); } }
            }
        }
        EditOp::Link { task, pred } => {
            if task == pred { return Err(Issue::error("a task cannot depend on itself")); }
            if !has(plan, pred) { return Err(Issue::error(format!("no task {pred}"))); }
            let t = plan.tasks.iter_mut().find(|t| &t.id == task).ok_or_else(|| Issue::error(format!("no task {task}")))?;
            if !t.after.contains(pred) { t.after.push(pred.clone()); }
        }
        EditOp::Unlink { task, pred } => {
            let t = plan.tasks.iter_mut().find(|t| &t.id == task).ok_or_else(|| Issue::error(format!("no task {task}")))?;
            t.after.retain(|p| p != pred);
        }
        EditOp::CloneScenario { from, key, name } => {
            if key.is_empty() || plan.scenarios.iter().any(|s| &s.key == key) { return Err(Issue::error(format!("scenario key {key} is empty or already used"))); }
            let mut s = match from {
                Some(f) => plan.scenarios.iter().find(|s| &s.key == f).cloned().ok_or_else(|| Issue::error(format!("no scenario {f}")))?,
                None => ScenarioSpec::default(),
            };
            s.key = key.clone();
            s.name = name.clone();
            plan.scenarios.push(s);
        }
        EditOp::SetScenario { key, name, note } => {
            let sc = plan.scenarios.iter_mut().find(|s| &s.key == key).ok_or_else(|| Issue::error(format!("no scenario {key}")))?;
            if let Some(n) = name { sc.name = n.clone(); }
            if let Some(n) = note { sc.note = n.clone(); }
        }
        EditOp::DeleteScenario { key } => {
            plan.scenarios.retain(|s| &s.key != key);
        }
        EditOp::AddComment { target, text } => {
            let text = text.trim();
            if text.is_empty() { return Err(Issue::error("comment is empty")); }
            if !has(plan, target) && !plan.stories.iter().any(|s| &s.key == target) { return Err(Issue::error(format!("no task or story {target}"))); }
            let id = plan.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
            plan.comments.push(Comment { id, target: target.clone(), text: text.to_string(), ..Default::default() });
        }
        EditOp::DeleteComment { id } => {
            if !plan.comments.iter().any(|c| c.id == *id) { return Err(Issue::error(format!("no comment {id}"))); }
            plan.comments.retain(|c| c.id != *id);
        }
    }
    Ok(())
}

fn plan_patch(t: &mut TaskSpec, p: &TaskPatch) {
    macro_rules! set { ($($f:ident),*) => { $( if let Some(v) = &p.$f { t.$f = v.clone(); } )* } }
    set!(story, name, short, ticket, deliverable, group, note, after, needs);
    if let Some(v) = p.done { t.done = v; }
    if p.effort.is_some() { t.effort = p.effort.clone(); t.wait = None; }
    if p.wait.is_some() { t.wait = p.wait.clone(); t.effort = None; }
    if p.owners.is_some() { t.owners = p.owners.clone(); }
    if p.not_before.is_some() { t.not_before = p.not_before; }
    if p.fixed.is_some() { t.fixed = p.fixed; }
    if p.actual_start.is_some() { t.actual_start = p.actual_start; }
    if p.actual_finish.is_some() { t.actual_finish = p.actual_finish; }
    for c in &p.clear {
        match c.as_str() { "actual_start" => t.actual_start = None, "actual_finish" => t.actual_finish = None, "not_before" => t.not_before = None, "fixed" => t.fixed = None, "effort" => t.effort = None, "wait" => t.wait = None, "owners" | "owner" => t.owners = None, _ => {} }
    }
}

/// Fold a later patch into an earlier one, as if both were applied in order: a later value drops an earlier clear
/// of that field, and a later clear drops an earlier value
fn merge_patch(a: &mut TaskPatch, b: &TaskPatch) {
    macro_rules! m { ($($f:ident),*) => { $( if b.$f.is_some() { a.$f = b.$f.clone(); a.clear.retain(|c| c != stringify!($f)); } )* } }
    m!(story, name, short, ticket, deliverable, group, note, effort, wait, owners, after, needs, not_before, fixed, done, actual_start, actual_finish);
    if b.owners.is_some() { a.clear.retain(|c| c != "owner"); }
    if b.effort.is_some() { a.wait = None; }
    if b.wait.is_some() { a.effort = None; }
    for c in &b.clear {
        match c.as_str() {
            "actual_start" => a.actual_start = None, "actual_finish" => a.actual_finish = None, "not_before" => a.not_before = None,
            "fixed" => a.fixed = None, "effort" => a.effort = None, "wait" => a.wait = None, "owners" | "owner" => a.owners = None, _ => {}
        }
        if !a.clear.contains(c) { a.clear.push(c.clone()); }
    }
}

// ---------- Export (TOML, readable by people and AI) ----------

fn q(s: &str) -> String { format!("{:?}", s) }
fn est(e: &Estimate) -> String {
    match e { Estimate::One(v) => fmt_num(*v), Estimate::Three(o, m, p) => format!("[{}, {}, {}]", fmt_num(*o), fmt_num(*m), fmt_num(*p)) }
}
fn qlist(v: &[String]) -> String { format!("[{}]", v.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ")) }

fn task_lines(t: &TaskSpec, out: &mut String) {
    out.push_str(&format!("id = {}
", q(&t.id)));
    if !t.story.is_empty() { out.push_str(&format!("story = {}
", q(&t.story))); }
    out.push_str(&format!("name = {}
", q(&t.name)));
    if !t.short.is_empty() && t.short != t.name { out.push_str(&format!("short = {}
", q(&t.short))); }
    for (k, v) in [("ticket", &t.ticket), ("deliverable", &t.deliverable), ("group", &t.group), ("note", &t.note)] { if !v.is_empty() { out.push_str(&format!("{k} = {}
", q(v))); } }
    if let Some(e) = &t.effort { out.push_str(&format!("effort = {}
", est(e))); }
    if let Some(e) = &t.wait { out.push_str(&format!("wait = {}
", est(e))); }
    if let Some(o) = &t.owners { out.push_str(&format!("owner = {}
", qlist(o))); }
    if !t.after.is_empty() { out.push_str(&format!("after = {}
", qlist(&t.after))); }
    if !t.needs.is_empty() { out.push_str(&format!("needs = {}\n", qlist(&t.needs))); }
    if let Some(d) = t.not_before { out.push_str(&format!("not_before = \"{d}\"\n")); }
    if let Some(d) = t.fixed { out.push_str(&format!("fixed = \"{d}\"\n")); }
    if t.done { out.push_str("done = true\n"); }
    if let Some(d) = t.actual_start { out.push_str(&format!("actual_start = \"{d}\"\n")); }
    if let Some(d) = t.actual_finish { out.push_str(&format!("actual_finish = \"{d}\"\n")); }
}

fn patch_lines(p: &TaskPatch, out: &mut String) {
    for (k, v) in [("story", &p.story), ("name", &p.name), ("short", &p.short), ("ticket", &p.ticket), ("deliverable", &p.deliverable), ("group", &p.group), ("note", &p.note)] {
        if let Some(v) = v { out.push_str(&format!("{k} = {}\n", q(v))); }
    }
    if let Some(e) = &p.effort { out.push_str(&format!("effort = {}\n", est(e))); }
    if let Some(e) = &p.wait { out.push_str(&format!("wait = {}\n", est(e))); }
    if let Some(o) = &p.owners { out.push_str(&format!("owner = {}\n", qlist(o))); }
    if let Some(a) = &p.after { out.push_str(&format!("after = {}\n", qlist(a))); }
    if let Some(v) = &p.needs { out.push_str(&format!("needs = {}\n", qlist(v))); }
    if let Some(d) = p.not_before { out.push_str(&format!("not_before = \"{d}\"\n")); }
    if let Some(d) = p.fixed { out.push_str(&format!("fixed = \"{d}\"\n")); }
    if let Some(v) = p.done { out.push_str(&format!("done = {v}\n")); }
    if !p.clear.is_empty() { out.push_str(&format!("clear = {}\n", qlist(&p.clear))); }
}

/// A plan as TOML (plan dump; can be imported again)
pub fn to_toml(p: &Plan) -> String {
    let mut o = String::new();
    o.push_str(&format!("key = {}\ntitle = {}\n", q(&p.key), q(&p.title)));
    if !p.description.is_empty() { o.push_str(&format!("description = {}\n", q(&p.description))); }
    o.push_str(&format!("start = \"{}\"\nstart_half = {}\n", p.start, q(if p.start_pm { "pm" } else { "am" })));
    if let Some(d) = p.deadline { o.push_str(&format!("deadline = \"{d}\"\n")); }
    if let Some(v) = &p.deadline_task { o.push_str(&format!("deadline_task = {}\n", q(v))); }
    if let Some(v) = &p.deadline_label { o.push_str(&format!("deadline_label = {}\n", q(v))); }
    o.push_str(&format!("rates = [{}]\n", p.rates.iter().map(|r| fmt_num(*r)).collect::<Vec<_>>().join(", ")));
    o.push_str(&format!("estimates = [{}]\n", p.estimates.iter().map(|e| q(e.code())).collect::<Vec<_>>().join(", ")));
    if p.round != 0.5 { o.push_str(&format!("round = {}\n", fmt_num(p.round))); }
    if let Some(v) = &p.default_owner { o.push_str(&format!("default_owner = {}\n", q(v))); }
    if !p.owners.is_empty() { o.push_str(&format!("owners = [{}]\n", p.owners.iter().map(|x| q(x)).collect::<Vec<_>>().join(", "))); }
    if let Some(v) = p.max_task_days { o.push_str(&format!("max_task_days = {}\n", fmt_num(v))); }
    let c = &p.calendar;
    o.push_str(&format!("\n[calendar]\nholidays = {}\nyear_end = {}\n", if c.holidays_jp { "\"JP\"" } else { "false" }, c.year_end));
    if !c.off.is_empty() { o.push_str(&format!("off = [{}]\n", c.off.iter().map(|d| format!("\"{d}\"")).collect::<Vec<_>>().join(", "))); }
    o.push_str(&format!("\n[colors]\nred = {}\nyellow = {}\n", fmt_num(p.colors.red), fmt_num(p.colors.yellow)));
    for (name, pe) in &p.people {
        o.push_str(&format!("\n[people.{}]\nleave = [{}]\n", q(name), pe.leave.iter().map(|d| format!("\"{d}\"")).collect::<Vec<_>>().join(", ")));
        if let Some(r) = pe.rate { o.push_str(&format!("rate = {}\n", fmt_num(r))); }
    }
    for (name, r) in &p.resources {
        o.push_str(&format!("\n[resources.{}]\n", q(name)));
        if !r.label.is_empty() { o.push_str(&format!("label = {}\n", q(&r.label))); }
        o.push_str(&format!("off = [{}]\n", r.off.iter().map(|(a, b)| format!("[\"{a}\", \"{b}\"]")).collect::<Vec<_>>().join(", ")));
    }
    for s in &p.stories {
        o.push_str(&format!("\n[[stories]]\nkey = {}\nname = {}\n", q(&s.key), q(&s.name)));
        if !s.purpose.is_empty() { o.push_str(&format!("purpose = {}\n", q(&s.purpose))); }
        if !s.dod.is_empty() { o.push_str(&format!("dod = {}\n", q(&s.dod))); }
    }
    for t in &p.tasks { o.push_str("\n[[tasks]]\n"); task_lines(t, &mut o); }
    for s in &p.scenarios {
        o.push_str(&format!("\n[scenarios.{}]\nname = {}\n", s.key, q(&s.name)));
        if !s.note.is_empty() { o.push_str(&format!("note = {}\n", q(&s.note))); }
        if !s.drop.is_empty() { o.push_str(&format!("drop = {}\n", qlist(&s.drop))); }
        for (k, pa) in &s.set { o.push_str(&format!("[scenarios.{}.set.{}]\n", s.key, k)); patch_lines(pa, &mut o); }
        for a in &s.add { o.push_str(&format!("[[scenarios.{}.add]]\n", s.key)); task_lines(a, &mut o); }
    }
    o
}

// ---------- Read ----------

fn err(m: impl Into<String>) -> Issue { Issue::error(m) }

fn date(v: &Value, where_: &str) -> Result<NaiveDate, Issue> {
    match v {
        Value::Datetime(d) => d.date.map(|d| NaiveDate::from_ymd_opt(d.year as i32, d.month as u32, d.day as u32).unwrap()).ok_or_else(|| err(format!("{where_}: no date"))),
        Value::String(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| err(format!("{where_}: write dates as YYYY-MM-DD ({s:?})"))),
        _ => Err(err(format!("{where_}: write dates as YYYY-MM-DD"))),
    }
}

fn opt_date(t: &Table, k: &str, where_: &str) -> Result<Option<NaiveDate>, Issue> {
    match t.get(k) {
        None => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(v) => date(v, &format!("{where_}.{k}")).map(Some),
    }
}

fn num(v: &Value) -> Option<f64> { v.as_float().or_else(|| v.as_integer().map(|i| i as f64)) }

fn estimate(v: &Value, where_: &str) -> Result<Estimate, Issue> {
    if let Some(x) = num(v) { return Ok(Estimate::One(x)); }
    if let Some(a) = v.as_array() {
        let xs: Vec<f64> = a.iter().filter_map(num).collect();
        if xs.len() == 3 && a.len() == 3 { return Ok(Estimate::Three(xs[0], xs[1], xs[2])); }
    }
    Err(err(format!("{where_}: effort and wait are a number or [optimistic, most likely, pessimistic]")))
}

fn str_of(t: &Table, k: &str) -> String { t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string() }

fn list_of(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => vec![],
    }
}

const TASK_KEYS: [&str; 19] = ["actual_start", "actual_finish", "story", "id", "name", "short", "ticket", "deliverable", "effort", "wait", "owner", "after", "needs", "prod", "not_before", "fixed", "done", "group", "note"];

fn task(t: &Table, where_: &str) -> Result<TaskSpec, Issue> {
    let id = str_of(t, "id");
    let w = format!("{where_}.{id}");
    let name = t.get("name").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| id.clone());
    Ok(TaskSpec {
        short: t.get("short").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| name.clone()),
        name,
        ticket: str_of(t, "ticket"),
        deliverable: str_of(t, "deliverable"),
        story: str_of(t, "story"),
        group: str_of(t, "group"),
        note: str_of(t, "note"),
        effort: t.get("effort").map(|v| estimate(v, &w)).transpose()?,
        wait: t.get("wait").map(|v| estimate(v, &w)).transpose()?,
        owners: t.get("owner").map(list_of),
        after: t.get("after").map(list_of).unwrap_or_default(),
        needs: t.get("needs").map(list_of).unwrap_or_default(),
        prod: t.get("prod").and_then(|v| v.as_bool()).unwrap_or(false),
        not_before: opt_date(t, "not_before", &w)?,
        fixed: opt_date(t, "fixed", &w)?,
        done: t.get("done").and_then(|v| v.as_bool()).unwrap_or(false),
        actual_start: opt_date(t, "actual_start", &w)?,
        actual_finish: opt_date(t, "actual_finish", &w)?,
        unknown_keys: t.keys().filter(|k| !TASK_KEYS.contains(&k.as_str())).cloned().collect(),
        id,
    })
}

fn patch(t: &Table, w: &str) -> Result<TaskPatch, Issue> {
    let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
    Ok(TaskPatch {
        story: s("story"), clear: t.get("clear").map(list_of).unwrap_or_default(),
        name: s("name"), short: s("short"), ticket: s("ticket"), deliverable: s("deliverable"), group: s("group"), note: s("note"),
        effort: t.get("effort").map(|v| estimate(v, w)).transpose()?,
        wait: t.get("wait").map(|v| estimate(v, w)).transpose()?,
        owners: t.get("owner").map(list_of),
        after: t.get("after").map(list_of),
        needs: t.get("needs").map(list_of),
        prod: t.get("prod").and_then(|v| v.as_bool()),
        not_before: opt_date(t, "not_before", w)?,
        fixed: opt_date(t, "fixed", w)?,
        done: t.get("done").and_then(|v| v.as_bool()),
        actual_start: opt_date(t, "actual_start", w)?,
        actual_finish: opt_date(t, "actual_finish", w)?,
    })
}

const TOP_KEYS: [&str; 22] = ["max_task_days", "resources", "key", "stories", "title", "description", "start", "start_half", "deadline", "deadline_task", "deadline_label", "rates", "estimates",
    "round", "default_owner", "owners", "calendar", "colors", "people", "tasks", "scenarios", "$schema"];

fn one<T>(r: Result<T, Issue>) -> Result<T, Vec<Issue>> { r.map_err(|e| vec![e]) }

/// [[first, last], …]
fn periods(v: Option<&Value>, where_: &str) -> Result<Vec<(NaiveDate, NaiveDate)>, Vec<Issue>> {
    let mut out = Vec::new();
    for p in v.and_then(|v| v.as_array()).cloned().unwrap_or_default() {
        let a = p.as_array().ok_or_else(|| vec![err(format!("{where_}: write [[\"start\", \"end\"], …]"))])?;
        if a.len() != 2 { return Err(vec![err(format!("{where_}: each period is [start, end]"))]); }
        out.push((one(date(&a[0], where_))?, one(date(&a[1], where_))?));
    }
    Ok(out)
}

/// A plan from TOML, with the old forms (prod, calendar.freeze) moved into the new ones
pub fn parse(text: &str) -> Result<Plan, Vec<Issue>> {
    let mut p = parse_raw(text)?;
    p.normalize();
    Ok(p)
}

fn parse_raw(text: &str) -> Result<Plan, Vec<Issue>> {
    let raw: Table = text.parse().map_err(|e: toml::de::Error| vec![err(format!("invalid TOML: {}", e.message()))])?;
    let empty = Table::new();
    let cal = raw.get("calendar").and_then(|v| v.as_table()).unwrap_or(&empty);
    let freeze = periods(cal.get("freeze"), "calendar.freeze")?;
    let mut resources = BTreeMap::new();
    if let Some(rt) = raw.get("resources").and_then(|v| v.as_table()) {
        for (name, v) in rt {
            let t = v.as_table().unwrap_or(&empty);
            resources.insert(name.clone(), Resource { label: str_of(t, "label"), off: periods(t.get("off"), &format!("resources.{name}.off"))? });
        }
    }
    let off = cal.get("off").and_then(|v| v.as_array()).map(|a| a.iter().map(|v| date(v, "calendar.off")).collect::<Result<Vec<_>, _>>()).transpose().map_err(|e| vec![e])?.unwrap_or_default();
    let colors_t = raw.get("colors").and_then(|v| v.as_table());
    let colors = Colors {
        red: colors_t.and_then(|t| t.get("red")).and_then(num).unwrap_or(2.0),
        yellow: colors_t.and_then(|t| t.get("yellow")).and_then(num).unwrap_or(5.0),
    };
    let mut people = BTreeMap::new();
    if let Some(pt) = raw.get("people").and_then(|v| v.as_table()) {
        for (name, v) in pt {
            let t = v.as_table().unwrap_or(&empty);
            let leave = t.get("leave").and_then(|v| v.as_array()).map(|a| a.iter().map(|v| date(v, &format!("people.{name}.leave"))).collect::<Result<Vec<_>, _>>()).transpose().map_err(|e| vec![e])?.unwrap_or_default();
            people.insert(name.clone(), Person { leave, rate: t.get("rate").and_then(num) });
        }
    }
    let mut tasks = Vec::new();
    for v in raw.get("tasks").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
        let tt = v.as_table().ok_or_else(|| vec![err("each entry of tasks must be a table")])?;
        tasks.push(one(task(tt, "tasks"))?);
    }
    let mut scenarios = Vec::new();
    if let Some(st) = raw.get("scenarios").and_then(|v| v.as_table()) {
        for (key, v) in st {
            let t = v.as_table().unwrap_or(&empty);
            let mut set = Vec::new();
            if let Some(s) = t.get("set").and_then(|v| v.as_table()) {
                for (tid, pv) in s { set.push((tid.clone(), one(patch(pv.as_table().unwrap_or(&empty), &format!("scenarios.{key}.set.{tid}")))?)); }
            }
            let mut add = Vec::new();
            for a in t.get("add").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                add.push(one(task(a.as_table().unwrap_or(&empty), &format!("scenarios.{key}.add")))?);
            }
            scenarios.push(ScenarioSpec {
                key: key.clone(),
                name: t.get("name").and_then(|v| v.as_str()).unwrap_or(key).to_string(),
                note: str_of(t, "note"),
                drop: t.get("drop").map(list_of).unwrap_or_default(),
                set,
                add,
            });
        }
    }
    let ests = raw.get("estimates").map(list_of).unwrap_or_else(|| vec!["e".into(), "m".into(), "p".into()]);
    let estimates = ests.iter().map(|e| EstKind::parse(e).ok_or_else(|| vec![err(format!("estimates: one of o, m, e, p ({e:?})"))])).collect::<Result<Vec<_>, _>>()?;
    let start = match raw.get("start") { Some(v) => one(date(v, "start"))?, None => return Err(vec![err("start (the plan's start date) is missing")]) };
    let s = |k: &str| raw.get(k).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
    let stories = raw.get("stories").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_table()).map(|t| Story {
        key: str_of(t, "key"), name: str_of(t, "name"), purpose: str_of(t, "purpose"), dod: str_of(t, "dod"),
    }).collect()).unwrap_or_default();
    Ok(Plan {
        key: s("key").unwrap_or_default(),
        stories,
        title: s("title").unwrap_or_else(|| "Schedule".into()),
        description: s("description").unwrap_or_default(),
        start,
        start_pm: raw.get("start_half").and_then(|v| v.as_str()) == Some("pm"),
        deadline: opt_date(&raw, "deadline", "").map_err(|e| vec![e])?,
        deadline_task: s("deadline_task"),
        deadline_label: s("deadline_label"),
        rates: raw.get("rates").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(num).collect()).unwrap_or_else(|| vec![1.0, 0.8, 0.6]),
        estimates,
        round: raw.get("round").and_then(num).unwrap_or(0.5),
        default_owner: s("default_owner"),
        owners: raw.get("owners").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        max_task_days: raw.get("max_task_days").and_then(num),
        calendar: CalendarSpec {
            holidays_jp: cal.get("holidays").map(|v| v.as_str() == Some("JP") || v.as_bool() == Some(true)).unwrap_or(true),
            year_end: cal.get("year_end").and_then(|v| v.as_bool()).unwrap_or(true),
            off,
            freeze,
        },
        colors,
        people,
        resources,
        tasks,
        scenarios,
        comments: vec![],
        unknown_keys: raw.keys().filter(|k| !TOP_KEYS.contains(&k.as_str())).cloned().collect(),
    })
}
