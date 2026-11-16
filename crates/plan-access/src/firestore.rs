//! FirestorePlans: plans in Firestore (REST). Used by the server on Cloud Run.
//!
//!   epics/{key}        one epic's plan (the Plan JSON as the document's fields, readable in the console)
//!   history/{id, 12 digits}  one document per change (id, at, actor, epic, op, payload); id is a global sequence
//!   meta/state         version (the last history id)
//!
//! Edits run read → mutate → validate → write + history in one transaction (the same contract as the SQLite store).
//! Firestore arrays cannot hold arrays, so an array inside an array is wrapped as {"_a": [...]}
use crate::plan_access::{mutate, PlanAccess, Validate};
use crate::token::{e, Tokens};
use chrono::NaiveDate;
use plan_contracts::*;
use serde_json::{json, Map, Value};
use std::sync::Mutex;

pub struct FirestorePlans {
    base: String,
    tokens: Tokens,
    /// There is one server (max-instances 1) and every write goes through it, so the version can be cached
    version: Mutex<Option<i64>>,
}

// ---------- JSON ↔ Firestore values ----------

fn to_fs(v: &Value, in_array: bool) -> Value {
    match v {
        Value::Null => json!({ "nullValue": null }),
        Value::Bool(b) => json!({ "booleanValue": b }),
        Value::Number(n) => match n.as_i64() { Some(i) => json!({ "integerValue": i.to_string() }), None => json!({ "doubleValue": n.as_f64() }) },
        Value::String(s) => json!({ "stringValue": s }),
        Value::Array(a) => {
            let arr = json!({ "arrayValue": { "values": a.iter().map(|x| to_fs(x, true)).collect::<Vec<_>>() } });
            if in_array { json!({ "mapValue": { "fields": { "_a": arr } } }) } else { arr }
        }
        Value::Object(o) => json!({ "mapValue": { "fields": fields(o) } }),
    }
}
fn fields(o: &Map<String, Value>) -> Value { Value::Object(o.iter().map(|(k, v)| (k.clone(), to_fs(v, false))).collect()) }

fn from_fs(v: &Value) -> Value {
    let Some((k, x)) = v.as_object().and_then(|o| o.iter().next()) else { return Value::Null };
    match k.as_str() {
        "booleanValue" => x.clone(),
        "integerValue" => x.as_str().and_then(|s| s.parse::<i64>().ok()).map(Value::from).unwrap_or(Value::Null),
        "doubleValue" => x.clone(),
        "stringValue" | "timestampValue" | "referenceValue" => x.clone(),
        "arrayValue" => Value::Array(x["values"].as_array().map(|a| a.iter().map(from_fs).collect()).unwrap_or_default()),
        "mapValue" => {
            let f = from_fields(&x["fields"]);
            match f.as_object() { Some(o) if o.len() == 1 && o.contains_key("_a") => o["_a"].clone(), _ => f }
        }
        _ => Value::Null,
    }
}
fn from_fields(f: &Value) -> Value {
    Value::Object(f.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), from_fs(v))).collect()).unwrap_or_default())
}

fn plan_from_doc(doc: &Value) -> Result<Plan, Vec<Issue>> {
    let mut p: Plan = serde_json::from_value(from_fields(&doc["fields"])).map_err(|x| e(format!("cannot read the plan from Firestore: {x}")))?;
    p.normalize();
    Ok(p)
}

impl FirestorePlans {
    pub fn new(project: &str, tokens: Tokens) -> Self {
        FirestorePlans { base: format!("https://firestore.googleapis.com/v1/projects/{project}/databases/(default)/documents"), tokens, version: Mutex::new(None) }
    }
    fn name(&self, path: &str) -> String { format!("{}/{}", self.base.trim_start_matches("https://firestore.googleapis.com/v1/"), path) }

    fn call(&self, method: &str, url: &str, body: Option<Value>) -> Result<Option<Value>, Vec<Issue>> {
        let send = || {
            let t = self.tokens.get()?;
            let req = ureq::request(method, url).set("Authorization", &format!("Bearer {t}"));
            Ok::<_, Vec<Issue>>(match &body { Some(b) => req.send_json(b.clone()), None => req.call() })
        };
        let mut res = send()?;
        // A 401 means the token went stale: fetch a new one and try once more
        if matches!(res, Err(ureq::Error::Status(401, _))) { self.tokens.forget(); res = send()?; }
        match res {
            Ok(r) => Ok(Some(r.into_json().map_err(e)?)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(c, r)) => Err(e(format!("Firestore {c}: {}", r.into_string().unwrap_or_default()))),
            Err(x) => Err(e(x)),
        }
    }
    fn get(&self, path: &str, tx: Option<&str>) -> Result<Option<Value>, Vec<Issue>> {
        let mut url = format!("{}/{}", self.base, path);
        if let Some(t) = tx { url.push_str(&format!("?transaction={}", urlenc(t))); }
        self.call("GET", &url, None)
    }
    fn begin(&self) -> Result<String, Vec<Issue>> {
        let v = self.call("POST", &format!("{}:beginTransaction", self.base), Some(json!({})))?.unwrap_or_default();
        v["transaction"].as_str().map(str::to_string).ok_or_else(|| e("cannot begin a transaction"))
    }
    fn commit(&self, tx: Option<&str>, writes: Vec<Value>) -> Result<(), Vec<Issue>> {
        let mut b = json!({ "writes": writes });
        if let Some(t) = tx { b["transaction"] = json!(t); }
        self.call("POST", &format!("{}:commit", self.base), Some(b)).map(|_| ())
    }
    fn update(&self, path: &str, obj: &Value) -> Value { json!({ "update": { "name": self.name(path), "fields": fields(obj.as_object().unwrap()) } }) }

    fn rollback(&self, tx: &str) { let _ = self.call("POST", &format!("{}:rollback", self.base), Some(json!({ "transaction": tx }))); }

    /// Run f in a transaction. On error, roll back (do not leave locks). On contention (409), retry up to 3 times
    fn in_tx(&self, f: impl Fn(&str) -> Result<(), Vec<Issue>>) -> Result<(), Vec<Issue>> {
        for n in 0..3 {
            let tx = self.begin()?;
            match f(&tx) {
                Ok(()) => return Ok(()),
                Err(is) => {
                    self.rollback(&tx);
                    let busy = is.iter().any(|i| i.message.starts_with("Firestore 409"));
                    if !busy || n == 2 { return Err(is); }
                    std::thread::sleep(std::time::Duration::from_millis(200 * (n + 1) as u64));
                }
            }
        }
        unreachable!()
    }

    fn read_version(&self, tx: Option<&str>) -> Result<i64, Vec<Issue>> {
        Ok(self.get("meta/state", tx)?.map(|d| from_fields(&d["fields"])["version"].as_i64().unwrap_or(0)).unwrap_or(0))
    }

    /// Write the plan and append the changes to history, in one transaction
    fn write(&self, tx: &str, actor: &str, epic: &str, plan: Option<&Plan>, logs: Vec<(String, String)>) -> Result<(), Vec<Issue>> {
        let v0 = self.read_version(Some(tx))?;
        let mut w = vec![];
        match plan {
            Some(p) => w.push(self.update(&format!("epics/{epic}"), &serde_json::to_value(p).unwrap())),
            None => w.push(json!({ "delete": self.name(&format!("epics/{epic}")) })),
        }
        let at = now_jst();
        for (i, (op, payload)) in logs.iter().enumerate() {
            let id = v0 + 1 + i as i64;
            w.push(self.update(&format!("history/{id:012}"), &json!({ "id": id, "at": at, "actor": actor, "epic": epic, "op": op, "payload": payload })));
        }
        let v1 = v0 + logs.len() as i64;
        w.push(self.update("meta/state", &json!({ "version": v1 })));
        self.commit(Some(tx), w)?;
        *self.version.lock().unwrap() = Some(v1);
        Ok(())
    }

    fn all_plans(&self) -> Result<Vec<Plan>, Vec<Issue>> {
        let v = self.call("GET", &format!("{}/epics?pageSize=300", self.base), None)?.unwrap_or_default();
        v["documents"].as_array().map(|a| a.iter().map(plan_from_doc).collect()).unwrap_or(Ok(vec![]))
    }
}

/// Time in JST (YYYY-MM-DD HH:MM:SS), the same format as SQLite's datetime('now','localtime')
fn now_jst() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + 9 * 3600;
    chrono::DateTime::from_timestamp(secs, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}
fn urlenc(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }

/// Defaults for a new epic (same as the SQLite store). Calendar and people are copied from another epic
fn new_plan(key: &str, name: &str, start: NaiveDate, like: Option<&Plan>) -> Plan {
    Plan {
        key: key.into(), title: name.into(), description: String::new(), start, start_pm: false, deadline: None, deadline_task: None, deadline_label: None,
        rates: vec![1.0, 0.8, 0.6], estimates: vec![EstKind::E, EstKind::M, EstKind::P], round: 0.5, default_owner: None, owners: vec![],
        calendar: like.map(|p| p.calendar.clone()).unwrap_or(CalendarSpec { holidays_jp: true, year_end: true, off: vec![], freeze: vec![] }),
        colors: Colors { red: 2.0, yellow: 5.0 }, people: like.map(|p| p.people.clone()).unwrap_or_default(), resources: like.map(|p| p.resources.clone()).unwrap_or_default(),
        stories: vec![], tasks: vec![], scenarios: vec![], comments: vec![], unknown_keys: vec![],
    }
}

impl PlanAccess for FirestorePlans {
    fn read_plan(&self, epic: &str) -> Result<Plan, Vec<Issue>> {
        let d = self.get(&format!("epics/{epic}"), None)?.ok_or_else(|| e(format!("no epic {epic}")))?;
        let mut p = plan_from_doc(&d)?; p.key = epic.into(); Ok(p)
    }

    fn list_epics(&self) -> Result<Vec<EpicSummary>, Vec<Issue>> {
        let mut v: Vec<EpicSummary> = self.all_plans()?.into_iter().map(|p| EpicSummary {
            key: p.key, name: p.title, start: p.start, deadline: p.deadline, owners: p.owners, stories: p.stories.len(), tasks: p.tasks.len(), scenarios: p.scenarios.len(),
        }).collect();
        v.sort_by(|a, b| a.start.cmp(&b.start));
        Ok(v)
    }

    fn apply(&self, epic: &str, actor: &str, ops: &[EditOp], validate: Validate) -> Result<(), Vec<Issue>> { self.in_tx(|tx| {
        let d = self.get(&format!("epics/{epic}"), Some(tx))?.ok_or_else(|| e(format!("no epic {epic}")))?;
        let mut plan = plan_from_doc(&d)?; plan.key = epic.into();
        for op in ops { mutate(&mut plan, op).map_err(|i| vec![i])?; }
        for c in plan.comments.iter_mut().filter(|c| c.at.is_empty()) { c.at = now_jst(); c.by = actor.into(); }
        let issues = validate(&plan);
        if issues.iter().any(|i| i.level == Level::Error) { return Err(issues); }
        let logs = ops.iter().map(|op| { let v = serde_json::to_value(op).unwrap(); (v["op"].as_str().unwrap_or("").to_string(), v.to_string()) }).collect();
        self.write(tx, actor, epic, Some(&plan), logs)
    }) }

    fn create_epic(&self, key: &str, name: &str, start: NaiveDate, actor: &str) -> Result<(), Vec<Issue>> {
        if key.is_empty() || key.contains(|c: char| c.is_whitespace() || c == '/') { return Err(e("the key must not contain spaces or /")); }
        let like = self.all_plans()?.into_iter().next();
        self.in_tx(|tx| {
            if self.get(&format!("epics/{key}"), Some(tx))?.is_some() { return Err(e(format!("epic {key} already exists"))); }
            let p = new_plan(key, name, start, like.as_ref());
            self.write(tx, actor, key, Some(&p), vec![("create_epic".into(), json!({ "key": key, "name": name, "start": start }).to_string())])
        })
    }

    fn delete_epic(&self, key: &str, actor: &str) -> Result<(), Vec<Issue>> {
        self.in_tx(|tx| {
            let d = self.get(&format!("epics/{key}"), Some(tx))?.ok_or_else(|| e(format!("no epic {key}")))?;
            let plan = plan_from_doc(&d)?;
            // Keep the deleted plan in the history (so it can be restored)
            self.write(tx, actor, key, None, vec![("delete_epic".into(), serde_json::to_string(&plan).unwrap())])
        })
    }

    fn import(&self, plan: &Plan, actor: &str) -> Result<(), Vec<Issue>> {
        if plan.key.is_empty() { return Err(e("the imported plan has no key")); }
        self.in_tx(|tx| self.write(tx, actor, &plan.key, Some(plan), vec![("import".into(), json!({ "tasks": plan.tasks.len(), "scenarios": plan.scenarios.len() }).to_string())]))
    }

    fn history(&self, epic: Option<&str>, limit: usize) -> Result<Vec<HistoryEntry>, Vec<Issue>> {
        let mut q = json!({ "from": [{ "collectionId": "history" }], "orderBy": [{ "field": { "fieldPath": "id" }, "direction": "DESCENDING" }], "limit": limit });
        if let Some(ep) = epic { q["where"] = json!({ "fieldFilter": { "field": { "fieldPath": "epic" }, "op": "EQUAL", "value": { "stringValue": ep } } }); }
        let v = self.call("POST", &format!("{}:runQuery", self.base), Some(json!({ "structuredQuery": q })))?.unwrap_or_default();
        Ok(v.as_array().map(|a| a.iter().filter_map(|r| r.get("document")).map(|d| {
            let f = from_fields(&d["fields"]);
            HistoryEntry { id: f["id"].as_i64().unwrap_or(0), at: f["at"].as_str().unwrap_or("").into(), actor: f["actor"].as_str().unwrap_or("").into(),
                epic: f["epic"].as_str().unwrap_or("").into(), op: f["op"].as_str().unwrap_or("").into(), payload: f["payload"].as_str().unwrap_or("").into() }
        }).collect()).unwrap_or_default())
    }

    fn version(&self) -> i64 {
        let mut v = self.version.lock().unwrap();
        if v.is_none() { *v = self.read_version(None).ok(); }
        v.unwrap_or(0)
    }
}
