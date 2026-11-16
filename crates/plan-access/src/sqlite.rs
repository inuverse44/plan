//! SqlitePlans: plans in SQLite. One database holds every epic; people, days off and the calendar are shared across epics.
//! Every change is one row (JSON) in the history table, so an AI can read what changed and when
use crate::plan_access::{mutate, PlanAccess, Validate};
use chrono::NaiveDate;
use plan_contracts::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

pub struct SqlitePlans {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS epics (
  key TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  start TEXT NOT NULL, start_pm INTEGER NOT NULL DEFAULT 0, deadline TEXT, deadline_task TEXT, deadline_label TEXT,
  rates TEXT NOT NULL DEFAULT '[1,0.8,0.6]', estimates TEXT NOT NULL DEFAULT '["e","m","p"]', round REAL NOT NULL DEFAULT 0.5,
  default_owner TEXT, owners TEXT NOT NULL DEFAULT '[]', red REAL NOT NULL DEFAULT 2, yellow REAL NOT NULL DEFAULT 5, sort INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS stories (
  epic TEXT NOT NULL, key TEXT NOT NULL, name TEXT NOT NULL, purpose TEXT NOT NULL DEFAULT '', dod TEXT NOT NULL DEFAULT '',
  sort INTEGER NOT NULL, PRIMARY KEY (epic, key));
CREATE TABLE IF NOT EXISTS tasks (
  epic TEXT NOT NULL, key TEXT NOT NULL, story TEXT NOT NULL DEFAULT '', name TEXT NOT NULL, short TEXT NOT NULL DEFAULT '',
  ticket TEXT NOT NULL DEFAULT '', deliverable TEXT NOT NULL DEFAULT '', grp TEXT NOT NULL DEFAULT '', note TEXT NOT NULL DEFAULT '',
  effort TEXT, wait TEXT, owners TEXT, prod INTEGER NOT NULL DEFAULT 0, not_before TEXT, fixed TEXT, done INTEGER NOT NULL DEFAULT 0,
  sort INTEGER NOT NULL, PRIMARY KEY (epic, key));
CREATE TABLE IF NOT EXISTS deps (epic TEXT NOT NULL, task TEXT NOT NULL, pred TEXT NOT NULL, sort INTEGER NOT NULL, PRIMARY KEY (epic, task, pred));
CREATE TABLE IF NOT EXISTS scenarios (
  epic TEXT NOT NULL, key TEXT NOT NULL, name TEXT NOT NULL, note TEXT NOT NULL DEFAULT '',
  drop_tasks TEXT NOT NULL DEFAULT '[]', sets TEXT NOT NULL DEFAULT '[]', adds TEXT NOT NULL DEFAULT '[]', sort INTEGER NOT NULL, PRIMARY KEY (epic, key));
CREATE TABLE IF NOT EXISTS comments (
  epic TEXT NOT NULL, id INTEGER NOT NULL, target TEXT NOT NULL, at TEXT NOT NULL, by TEXT NOT NULL DEFAULT '', text TEXT NOT NULL, PRIMARY KEY (epic, id));
CREATE TABLE IF NOT EXISTS people (name TEXT PRIMARY KEY, rate REAL);
CREATE TABLE IF NOT EXISTS leave (person TEXT NOT NULL, day TEXT NOT NULL, PRIMARY KEY (person, day));
CREATE TABLE IF NOT EXISTS calendar_off (day TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS freeze (start TEXT NOT NULL, finish TEXT NOT NULL, PRIMARY KEY (start, finish));
CREATE TABLE IF NOT EXISTS resources (name TEXT PRIMARY KEY, label TEXT NOT NULL DEFAULT '');
CREATE TABLE IF NOT EXISTS resource_off (name TEXT NOT NULL, start TEXT NOT NULL, finish TEXT NOT NULL, PRIMARY KEY (name, start, finish));
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS history (
  id INTEGER PRIMARY KEY AUTOINCREMENT, at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
  actor TEXT NOT NULL, epic TEXT NOT NULL, op TEXT NOT NULL, payload TEXT NOT NULL);
"#;

fn e(x: impl std::fmt::Display) -> Vec<Issue> { vec![Issue::error(format!("database: {x}"))] }
fn d(s: Option<String>) -> Option<NaiveDate> { s.and_then(|x| NaiveDate::parse_from_str(&x, "%Y-%m-%d").ok()) }

/// An estimate as readable text ("1.5" or "[1, 2, 4]")
fn est_to(e: &Option<Estimate>) -> Option<String> {
    e.as_ref().map(|e| match e { Estimate::One(v) => fmt_num(*v), Estimate::Three(o, m, p) => format!("[{}, {}, {}]", fmt_num(*o), fmt_num(*m), fmt_num(*p)) })
}
fn est_from(s: Option<String>) -> Option<Estimate> {
    let s = s?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    if let Some(x) = v.as_f64() { return Some(Estimate::One(x)); }
    let a: Vec<f64> = v.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
    (a.len() == 3).then(|| Estimate::Three(a[0], a[1], a[2]))
}

impl SqlitePlans {
    pub fn open(path: &Path) -> Result<Self, Vec<Issue>> {
        if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
        let conn = Connection::open(path).map_err(e)?;
        conn.execute_batch(SCHEMA).map_err(e)?;
        // Columns added later (added to old databases too; no-op if present)
        for col in ["actual_start", "actual_finish", "needs"] {
            let has: bool = conn.prepare("SELECT 1 FROM pragma_table_info('tasks') WHERE name=?").map_err(e)?.exists([col]).map_err(e)?;
            if !has { conn.execute_batch(&format!("ALTER TABLE tasks ADD COLUMN {col} TEXT")).map_err(e)?; }
        }
        let has: bool = conn.prepare("SELECT 1 FROM pragma_table_info('epics') WHERE name='owners'").map_err(e)?.exists([]).map_err(e)?;
        if !has { conn.execute_batch("ALTER TABLE epics ADD COLUMN owners TEXT NOT NULL DEFAULT '[]'").map_err(e)?; }
        // The old forms: tasks.prod → tasks.needs = ["prod"], the freeze table → the resource "prod"
        conn.execute_batch(&format!("BEGIN;
            UPDATE tasks SET needs = '[\"{PROD}\"]', prod = 0 WHERE prod = 1 AND (needs IS NULL OR needs = '[]');
            UPDATE tasks SET prod = 0 WHERE prod = 1;
            INSERT OR IGNORE INTO resources (name, label) SELECT '{PROD}', 'Release freeze' WHERE EXISTS (SELECT 1 FROM freeze);
            INSERT OR IGNORE INTO resource_off (name, start, finish) SELECT '{PROD}', start, finish FROM freeze;
            DELETE FROM freeze;
            COMMIT;")).map_err(e)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").map_err(e)?;
        Ok(SqlitePlans { conn: Mutex::new(conn) })
    }

    fn common(c: &Connection) -> rusqlite::Result<(CalendarSpec, BTreeMap<String, Person>, BTreeMap<String, Resource>)> {
        let get = |k: &str| c.query_row("SELECT value FROM settings WHERE key=?", [k], |r| r.get::<_, String>(0)).optional();
        let holidays_jp = get("holidays")?.map(|v| v != "none").unwrap_or(true);
        let year_end = get("year_end")?.map(|v| v == "true").unwrap_or(true);
        let off = c.prepare("SELECT day FROM calendar_off ORDER BY day")?.query_map([], |r| r.get::<_, String>(0))?.filter_map(|x| d(x.ok())).collect();
        let mut resources = BTreeMap::new();
        for row in c.prepare("SELECT name, label FROM resources ORDER BY name")?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (n, label) = row?;
            let off = c.prepare("SELECT start, finish FROM resource_off WHERE name=? ORDER BY start")?.query_map([&n], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .filter_map(|x| x.ok()).filter_map(|(a, b)| Some((d(Some(a))?, d(Some(b))?))).collect();
            resources.insert(n, Resource { label, off });
        }
        let mut people = BTreeMap::new();
        for row in c.prepare("SELECT name, rate FROM people ORDER BY name")?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<f64>>(1)?)))? {
            let (n, rate) = row?;
            let leave = c.prepare("SELECT day FROM leave WHERE person=? ORDER BY day")?.query_map([&n], |r| r.get::<_, String>(0))?.filter_map(|x| d(x.ok())).collect();
            people.insert(n, Person { leave, rate });
        }
        Ok((CalendarSpec { holidays_jp, year_end, off, freeze: vec![] }, people, resources))
    }

    fn read(c: &Connection, epic: &str) -> Result<Plan, Vec<Issue>> {
        let row = c.query_row("SELECT name, description, start, start_pm, deadline, deadline_task, deadline_label, rates, estimates, round, default_owner, red, yellow FROM epics WHERE key=?", [epic], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, bool>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, f64>(9)?, r.get::<_, Option<String>>(10)?, r.get::<_, f64>(11)?, r.get::<_, f64>(12)?))
        }).optional().map_err(e)?.ok_or_else(|| vec![Issue::error(format!("no epic {epic}"))])?;
        let owners: Vec<String> = serde_json::from_str(&c.query_row("SELECT owners FROM epics WHERE key=?", [epic], |r| r.get::<_, String>(0)).map_err(e)?).unwrap_or_default();
        let (calendar, people, resources) = Self::common(c).map_err(e)?;
        let stories = c.prepare("SELECT key, name, purpose, dod FROM stories WHERE epic=? ORDER BY sort").map_err(e)?
            .query_map([epic], |r| Ok(Story { key: r.get(0)?, name: r.get(1)?, purpose: r.get(2)?, dod: r.get(3)? })).map_err(e)?.filter_map(|x| x.ok()).collect();
        let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for x in c.prepare("SELECT task, pred FROM deps WHERE epic=? ORDER BY sort").map_err(e)?.query_map([epic], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(e)? {
            let (t, p) = x.map_err(e)?; deps.entry(t).or_default().push(p);
        }
        let tasks = c.prepare("SELECT key, story, name, short, ticket, deliverable, grp, note, effort, wait, owners, needs, not_before, fixed, done, actual_start, actual_finish FROM tasks WHERE epic=? ORDER BY sort").map_err(e)?
            .query_map([epic], |r| {
                let id: String = r.get(0)?;
                Ok(TaskSpec {
                    after: deps.get(&id).cloned().unwrap_or_default(), story: r.get(1)?, name: r.get(2)?, short: r.get(3)?, ticket: r.get(4)?, deliverable: r.get(5)?,
                    group: r.get(6)?, note: r.get(7)?, effort: est_from(r.get(8)?), wait: est_from(r.get(9)?),
                    owners: r.get::<_, Option<String>>(10)?.and_then(|s| serde_json::from_str(&s).ok()),
                    needs: r.get::<_, Option<String>>(11)?.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(), prod: false,
                    not_before: d(r.get(12)?), fixed: d(r.get(13)?), done: r.get(14)?, actual_start: d(r.get(15)?), actual_finish: d(r.get(16)?), unknown_keys: vec![], id,
                })
            }).map_err(e)?.filter_map(|x| x.ok()).collect();
        let scenarios = c.prepare("SELECT key, name, note, drop_tasks, sets, adds FROM scenarios WHERE epic=? ORDER BY sort").map_err(e)?
            .query_map([epic], |r| Ok(ScenarioSpec {
                key: r.get(0)?, name: r.get(1)?, note: r.get(2)?,
                drop: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
                set: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_default(),
                add: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
            })).map_err(e)?.filter_map(|x| x.ok()).collect();
        let comments = c.prepare("SELECT id, target, at, by, text FROM comments WHERE epic=? ORDER BY id").map_err(e)?
            .query_map([epic], |r| Ok(Comment { id: r.get(0)?, target: r.get(1)?, at: r.get(2)?, by: r.get(3)?, text: r.get(4)? })).map_err(e)?.filter_map(|x| x.ok()).collect();
        let (name, description, start, start_pm, deadline, deadline_task, deadline_label, rates, ests, round, default_owner, red, yellow) = row;
        Ok(Plan {
            key: epic.to_string(), title: name, description, start: d(Some(start)).ok_or_else(|| vec![Issue::error("start is not a date")])?, start_pm,
            deadline: d(deadline), deadline_task, deadline_label, rates: serde_json::from_str(&rates).unwrap_or(vec![1.0]),
            estimates: serde_json::from_str::<Vec<String>>(&ests).unwrap_or_default().iter().filter_map(|x| EstKind::parse(x)).collect(),
            round, default_owner, owners, calendar, colors: Colors { red, yellow }, people, resources, stories, tasks, scenarios, comments, unknown_keys: vec![],
        })
    }

    fn write(tx: &Transaction, p: &Plan) -> rusqlite::Result<()> {
        let k = &p.key;
        tx.execute("INSERT INTO epics (key, name, description, start, start_pm, deadline, deadline_task, deadline_label, rates, estimates, round, default_owner, red, yellow, sort)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,(SELECT COALESCE(MAX(sort),0)+1 FROM epics))
            ON CONFLICT(key) DO UPDATE SET name=?2, description=?3, start=?4, start_pm=?5, deadline=?6, deadline_task=?7, deadline_label=?8, rates=?9, estimates=?10, round=?11, default_owner=?12, red=?13, yellow=?14",
            params![k, p.title, p.description, p.start.to_string(), p.start_pm, p.deadline.map(|x| x.to_string()), p.deadline_task, p.deadline_label,
                serde_json::to_string(&p.rates).unwrap(), serde_json::to_string(&p.estimates.iter().map(|e| e.code()).collect::<Vec<_>>()).unwrap(),
                p.round, p.default_owner, p.colors.red, p.colors.yellow])?;
        tx.execute("UPDATE epics SET owners=?2 WHERE key=?1", params![k, serde_json::to_string(&p.owners).unwrap()])?;
        for t in ["stories", "tasks", "deps", "scenarios", "comments"] { tx.execute(&format!("DELETE FROM {t} WHERE epic=?"), [k])?; }
        for (i, s) in p.stories.iter().enumerate() {
            tx.execute("INSERT INTO stories (epic, key, name, purpose, dod, sort) VALUES (?,?,?,?,?,?)", params![k, s.key, s.name, s.purpose, s.dod, i as i64])?;
        }
        for (i, t) in p.tasks.iter().enumerate() {
            tx.execute("INSERT INTO tasks (epic, key, story, name, short, ticket, deliverable, grp, note, effort, wait, owners, needs, not_before, fixed, done, actual_start, actual_finish, sort) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![k, t.id, t.story, t.name, t.short, t.ticket, t.deliverable, t.group, t.note, est_to(&t.effort), est_to(&t.wait),
                    t.owners.as_ref().map(|o| serde_json::to_string(o).unwrap()), (!t.needs.is_empty()).then(|| serde_json::to_string(&t.needs).unwrap()), t.not_before.map(|x| x.to_string()), t.fixed.map(|x| x.to_string()), t.done,
                    t.actual_start.map(|x| x.to_string()), t.actual_finish.map(|x| x.to_string()), i as i64])?;
            for (j, pr) in t.after.iter().enumerate() { tx.execute("INSERT OR IGNORE INTO deps (epic, task, pred, sort) VALUES (?,?,?,?)", params![k, t.id, pr, (i * 100 + j) as i64])?; }
        }
        for (i, s) in p.scenarios.iter().enumerate() {
            tx.execute("INSERT INTO scenarios (epic, key, name, note, drop_tasks, sets, adds, sort) VALUES (?,?,?,?,?,?,?,?)",
                params![k, s.key, s.name, s.note, serde_json::to_string(&s.drop).unwrap(), serde_json::to_string(&s.set).unwrap(), serde_json::to_string(&s.add).unwrap(), i as i64])?;
        }
        for c in &p.comments {
            tx.execute("INSERT INTO comments (epic, id, target, at, by, text) VALUES (?,?,?,?,?,?)", params![k, c.id, c.target, c.at, c.by, c.text])?;
        }
        Ok(())
    }

    fn log(tx: &Transaction, actor: &str, epic: &str, op: &str, payload: &str) -> rusqlite::Result<()> {
        tx.execute("INSERT INTO history (actor, epic, op, payload) VALUES (?,?,?,?)", params![actor, epic, op, payload]).map(|_| ())
    }
}

impl PlanAccess for SqlitePlans {
    fn read_plan(&self, epic: &str) -> Result<Plan, Vec<Issue>> { Self::read(&self.conn.lock().unwrap(), epic) }

    fn list_epics(&self) -> Result<Vec<EpicSummary>, Vec<Issue>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT key, name, start, deadline, (SELECT COUNT(*) FROM stories s WHERE s.epic=e.key), (SELECT COUNT(*) FROM tasks t WHERE t.epic=e.key), (SELECT COUNT(*) FROM scenarios c WHERE c.epic=e.key), owners FROM epics e ORDER BY sort").map_err(e)?;
        let rows = st.query_map([], |r| Ok(EpicSummary { key: r.get(0)?, name: r.get(1)?, start: d(Some(r.get(2)?)).unwrap_or_default(), deadline: d(r.get(3)?), owners: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
            stories: r.get::<_, i64>(4)? as usize, tasks: r.get::<_, i64>(5)? as usize, scenarios: r.get::<_, i64>(6)? as usize })).map_err(e)?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    fn apply(&self, epic: &str, actor: &str, ops: &[EditOp], validate: Validate) -> Result<(), Vec<Issue>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(e)?;
        let mut plan = Self::read(&tx, epic)?;
        for op in ops { mutate(&mut plan, op).map_err(|i| vec![i])?; }
        // Stamp new comments with the time and the author
        if plan.comments.iter().any(|c| c.at.is_empty()) {
            let now: String = tx.query_row("SELECT datetime('now','localtime')", [], |r| r.get(0)).map_err(e)?;
            for c in plan.comments.iter_mut().filter(|c| c.at.is_empty()) { c.at = now.clone(); c.by = actor.to_string(); }
        }
        let issues = validate(&plan);
        if issues.iter().any(|i| i.level == Level::Error) { return Err(issues); }
        Self::write(&tx, &plan).map_err(e)?;
        for op in ops {
            let v = serde_json::to_value(op).unwrap();
            Self::log(&tx, actor, epic, v["op"].as_str().unwrap_or(""), &v.to_string()).map_err(e)?;
        }
        tx.commit().map_err(e)
    }

    fn create_epic(&self, key: &str, name: &str, start: NaiveDate, actor: &str) -> Result<(), Vec<Issue>> {
        if key.is_empty() || key.contains(|c: char| c.is_whitespace() || c == '/') { return Err(vec![Issue::error("the key must not contain spaces or /")]); }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(e)?;
        let n: i64 = tx.query_row("SELECT COUNT(*) FROM epics WHERE key=?", [key], |r| r.get(0)).map_err(e)?;
        if n > 0 { return Err(vec![Issue::error(format!("epic {key} already exists"))]); }
        tx.execute("INSERT INTO epics (key, name, start, sort) VALUES (?,?,?,(SELECT COALESCE(MAX(sort),0)+1 FROM epics))", params![key, name, start.to_string()]).map_err(e)?;
        Self::log(&tx, actor, key, "create_epic", &serde_json::json!({ "key": key, "name": name, "start": start }).to_string()).map_err(e)?;
        tx.commit().map_err(e)
    }

    fn delete_epic(&self, key: &str, actor: &str) -> Result<(), Vec<Issue>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(e)?;
        let plan = Self::read(&tx, key)?;
        for t in ["epics", "stories", "tasks", "deps", "scenarios", "comments"] {
            tx.execute(&format!("DELETE FROM {t} WHERE {}=?", if t == "epics" { "key" } else { "epic" }), [key]).map_err(e)?;
        }
        // Keep the deleted plan in the history (so it can be restored)
        Self::log(&tx, actor, key, "delete_epic", &serde_json::to_string(&plan).unwrap()).map_err(e)?;
        tx.commit().map_err(e)
    }

    fn import(&self, plan: &Plan, actor: &str) -> Result<(), Vec<Issue>> {
        if plan.key.is_empty() { return Err(vec![Issue::error("the imported plan has no key")]); }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction().map_err(e)?;
        // Merge the shared calendar and people (do not remove other epics' entries)
        let cal = &plan.calendar;
        tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('holidays', ?), ('year_end', ?)", params![if cal.holidays_jp { "JP" } else { "none" }, cal.year_end.to_string()]).map_err(e)?;
        for d in &cal.off { tx.execute("INSERT OR IGNORE INTO calendar_off (day) VALUES (?)", [d.to_string()]).map_err(e)?; }
        for (n, r) in &plan.resources {
            tx.execute("INSERT INTO resources (name, label) VALUES (?1,?2) ON CONFLICT(name) DO UPDATE SET label=CASE WHEN ?2 = '' THEN label ELSE ?2 END", params![n, r.label]).map_err(e)?;
            for (a, b) in &r.off { tx.execute("INSERT OR IGNORE INTO resource_off (name, start, finish) VALUES (?,?,?)", [n, &a.to_string(), &b.to_string()]).map_err(e)?; }
        }
        for (n, p) in &plan.people {
            tx.execute("INSERT INTO people (name, rate) VALUES (?,?) ON CONFLICT(name) DO UPDATE SET rate=COALESCE(?2, rate)", params![n, p.rate]).map_err(e)?;
            for l in &p.leave { tx.execute("INSERT OR IGNORE INTO leave (person, day) VALUES (?,?)", [n, &l.to_string()]).map_err(e)?; }
        }
        Self::write(&tx, plan).map_err(e)?;
        Self::log(&tx, actor, &plan.key, "import", &serde_json::json!({ "tasks": plan.tasks.len(), "scenarios": plan.scenarios.len() }).to_string()).map_err(e)?;
        tx.commit().map_err(e)
    }

    fn history(&self, epic: Option<&str>, limit: usize) -> Result<Vec<HistoryEntry>, Vec<Issue>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT id, at, actor, epic, op, payload FROM history WHERE (?1 IS NULL OR epic=?1) ORDER BY id DESC LIMIT ?2").map_err(e)?;
        let rows = st.query_map(params![epic, limit as i64], |r| Ok(HistoryEntry { id: r.get(0)?, at: r.get(1)?, actor: r.get(2)?, epic: r.get(3)?, op: r.get(4)?, payload: r.get(5)? })).map_err(e)?;
        Ok(rows.filter_map(|x| x.ok()).collect())
    }

    fn version(&self) -> i64 {
        self.conn.lock().unwrap().query_row("SELECT COALESCE(MAX(id),0) FROM history", [], |r| r.get(0)).unwrap_or(0)
    }
}
