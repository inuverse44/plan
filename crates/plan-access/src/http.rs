//! HttpPlans：計画をサーバー（plan serve、Cloud Run）の API 越しに読む・直す。CLI が使う。
//! 確かめ（validate）と記録はサーバー側がする。ここは送って結果を受けるだけ
use crate::plan_access::{PlanAccess, Validate};
use crate::token::{e, Tokens};
use chrono::NaiveDate;
use plan_contracts::*;
use serde_json::{json, Value};

pub struct HttpPlans {
    base: String,
    tokens: Option<Tokens>,
    actor: String,
}

fn enc(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }

impl HttpPlans {
    /// base はサーバーの URL（例 https://plan-xxxx.run.app）。tokens が無ければ認証を付けない（手元のサーバー）
    pub fn new(base: &str, tokens: Option<Tokens>, actor: &str) -> Self { HttpPlans { base: base.trim_end_matches('/').into(), tokens, actor: actor.into() } }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, Vec<Issue>> {
        let mut req = ureq::request(method, &format!("{}{}", self.base, path));
        if let Some(t) = &self.tokens { req = req.set("Authorization", &format!("Bearer {}", t.get()?)); }
        let res = match body { Some(b) => req.send_json(b), None => req.call() };
        let v: Value = match res {
            Ok(r) => r.into_json().map_err(|x| e(format!("サーバーの答えを読めない（{path}）：{x}")))?,
            Err(ureq::Error::Status(c, r)) => return Err(e(format!("サーバー {c}（{path}）：{}", r.into_string().unwrap_or_default().chars().take(300).collect::<String>()))),
            Err(x) => return Err(e(format!("サーバーにつながらない（{}）：{x}", self.base))),
        };
        if v["ok"] == json!(false) { return Err(serde_json::from_value(v["issues"].clone()).unwrap_or_else(|_| e("サーバーの誤り"))); }
        Ok(if v.get("data").is_some() { v["data"].clone() } else { v })
    }
}

impl PlanAccess for HttpPlans {
    fn read_plan(&self, epic: &str) -> Result<Plan, Vec<Issue>> {
        let v = self.call("GET", &format!("/api/epic/{}", enc(epic)), None)?;
        serde_json::from_value(v["plan"].clone()).map_err(|x| e(format!("計画を読めない：{x}")))
    }
    fn list_epics(&self) -> Result<Vec<EpicSummary>, Vec<Issue>> {
        serde_json::from_value(self.call("GET", "/api/epics", None)?).map_err(e)
    }
    fn apply(&self, epic: &str, _actor: &str, ops: &[EditOp], _validate: Validate) -> Result<(), Vec<Issue>> {
        self.call("POST", &format!("/api/epic/{}/ops", enc(epic)), Some(json!({ "ops": ops, "actor": self.actor }))).map(|_| ())
    }
    fn create_epic(&self, key: &str, name: &str, start: NaiveDate, _actor: &str) -> Result<(), Vec<Issue>> {
        self.call("POST", "/api/epics", Some(json!({ "key": key, "name": name, "start": start, "actor": self.actor }))).map(|_| ())
    }
    fn delete_epic(&self, key: &str, _actor: &str) -> Result<(), Vec<Issue>> {
        self.call("POST", &format!("/api/epic/{}/delete", enc(key)), Some(json!({ "actor": self.actor }))).map(|_| ())
    }
    fn import(&self, plan: &Plan, _actor: &str) -> Result<(), Vec<Issue>> {
        self.call("POST", "/api/import", Some(json!({ "plan": plan, "actor": self.actor }))).map(|_| ())
    }
    fn history(&self, epic: Option<&str>, limit: usize) -> Result<Vec<HistoryEntry>, Vec<Issue>> {
        let q = match epic { Some(ep) => format!("?epic={}&limit={limit}", enc(ep)), None => format!("?limit={limit}") };
        serde_json::from_value(self.call("GET", &format!("/api/history{q}"), None)?).map_err(e)
    }
    fn version(&self) -> i64 { self.call("GET", "/api/version", None).ok().and_then(|v| v["version"].as_i64()).unwrap_or(0) }
}
