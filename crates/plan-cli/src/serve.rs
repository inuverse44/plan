//! The HTTP server (home of the web client). The UI computes in WASM; this server exposes PlanEditingManager over HTTP.
//!
//!   GET  /api/version                     a number that grows on every change
//!   GET  /api/epics                       list of epics
//!   GET  /api/loads                       every epic's load (default run), to see people across epics
//!   POST /api/epics          {key,name,start}
//!   GET  /api/epic/KEY                    one epic's plan (JSON)
//!   POST /api/epic/KEY/ops   {ops:[EditOp]}
//!   POST /api/epic/KEY/save  {scenario,rate,est,changes}   write moves tried in the UI into the plan
//!   POST /api/epic/KEY/delete
//!   GET  /api/epic/KEY/history
//!   GET  /api/history?epic=KEY&limit=N
//!   POST /api/import         {plan}                   replace a whole plan (plan push)
//!   Writing endpoints accept an actor (cli, mcp, web, push, import); the default is web
//!
//! The store is Firestore with PLAN_STORE=firestore:PROJECT, otherwise the local SQLite.
//! On Cloud Run (PORT is set) it listens on 0.0.0.0:PORT and never exits on idle (Cloud Run scales it down)
use crate::web;
use plan_access::{on_cloud_run, FirestorePlans, JpRuleCalendar, PlanAccess, TokenSource, Tokens};
use plan_contracts::{Change, EditOp, EstKind, Issue};
use plan_manager::{PlanEditingManager, PlanningManager};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Response, Server};

const IDLE: Duration = Duration::from_secs(1800);

fn reply(req: tiny_http::Request, ctype: &str, body: Vec<u8>) {
    let r = Response::from_data(body)
        .with_header(Header::from_bytes("Content-Type", ctype).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        // Do not keep connections open: they filled Chrome's limit of 6 per host and the UI hung while loading
        .with_header(Header::from_bytes("Connection", "close").unwrap());
    let _ = req.respond(r);
}
fn json(req: tiny_http::Request, v: serde_json::Value) { reply(req, "application/json; charset=utf-8", v.to_string().into_bytes()) }
fn result(req: tiny_http::Request, r: Result<serde_json::Value, Vec<Issue>>) {
    json(req, match r { Ok(v) => serde_json::json!({ "ok": true, "data": v }), Err(e) => serde_json::json!({ "ok": false, "issues": e }) })
}

fn backend() -> (Arc<dyn PlanAccess + Send + Sync>, String) {
    match std::env::var("PLAN_STORE").ok().and_then(|s| s.strip_prefix("firestore:").map(str::to_string)) {
        Some(project) => {
            let src = if on_cloud_run() { TokenSource::Metadata } else { TokenSource::Command("gcloud auth print-access-token".into()) };
            (Arc::new(FirestorePlans::new(&project, Tokens::new(src))), format!("Firestore {project}"))
        }
        None => (crate::local(), crate::db_path().display().to_string()),
    }
}

pub fn run(port: u16, open: bool) {
    let (st, place) = backend();
    let ed = PlanEditingManager::new(st.clone());
    let pm = PlanningManager::new(st.clone(), Arc::new(JpRuleCalendar));
    let cloud = std::env::var("PORT").ok().and_then(|p| p.parse::<u16>().ok());
    let server = match cloud { Some(p) => Server::http(("0.0.0.0", p)), None => Server::http(("127.0.0.1", port)) }.expect("cannot open the port");
    let url = format!("http://{}/", server.server_addr().to_ip().unwrap());
    println!("listening on {url} (store: {place}{})", if cloud.is_some() { "" } else { "; stops after 30 idle minutes" });
    if open && cloud.is_none() { let _ = std::process::Command::new("open").arg(&url).status(); }
    let last = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    { let last = last.clone(); std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        if cloud.is_none() && start.elapsed().as_secs() - last.load(Ordering::Relaxed) > IDLE.as_secs() { std::process::exit(0) }
    }); }
    for mut req in server.incoming_requests() {
        last.store(start.elapsed().as_secs(), Ordering::Relaxed);
        let url = req.url().split('?').next().unwrap_or("/").to_string();
        let mut body = String::new();
        if *req.method() == Method::Post { let _ = std::io::Read::read_to_string(req.as_reader(), &mut body); }
        let b: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        let parts: Vec<&str> = url.trim_matches('/').split('/').collect();
        let actor = b["actor"].as_str().filter(|a| ["cli", "web", "push", "import", "mcp"].contains(a)).unwrap_or("web").to_string();
        let query = req.url().split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
        let qp = |k: &str| query.split('&').filter_map(|kv| kv.split_once('=')).find(|(a, _)| *a == k).map(|(_, v)| decode(v));
        match (req.method().clone(), parts.as_slice()) {
            (Method::Get, [""]) => reply(req, "text/html; charset=utf-8", web::live_page().into_bytes()),
            (Method::Get, ["api", "version"]) => json(req, serde_json::json!({ "version": ed.version() })),
            (Method::Get, ["api", "epics"]) => result(req, ed.epics().map(|v| serde_json::to_value(v).unwrap())),
            (Method::Get, ["api", "loads"]) => result(req, pm.epic_loads().map(|v| serde_json::to_value(v).unwrap())),
            (Method::Post, ["api", "epics"]) => {
                let r = (|| {
                    let start = b["start"].as_str().and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()).ok_or_else(|| vec![Issue::error("start must be YYYY-MM-DD")])?;
                    ed.create_epic(b["key"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""), start, &actor).map(|_| serde_json::json!({}))
                })();
                result(req, r)
            }
            (Method::Get, ["api", "epic", k]) => result(req, ed.plan(&decode(k)).map(|p| serde_json::json!({ "version": ed.version(), "plan": p }))),
            (Method::Get, ["api", "epic", k, "history"]) => result(req, ed.history(Some(&decode(k)), 100).map(|h| serde_json::to_value(h).unwrap())),
            (Method::Post, ["api", "epic", k, "ops"]) => {
                let r = serde_json::from_value::<Vec<EditOp>>(b["ops"].clone()).map_err(|e| vec![Issue::error(format!("invalid edits: {e}"))])
                    .and_then(|ops| ed.apply(&decode(k), &actor, &ops)).map(|_| serde_json::json!({}));
                result(req, r)
            }
            (Method::Post, ["api", "epic", k, "save"]) => {
                let k = decode(k);
                let r = (|| {
                    let ch: Vec<Change> = serde_json::from_value(b["changes"].clone()).map_err(|e| vec![Issue::error(e.to_string())])?;
                    let est = EstKind::parse(b["est"].as_str().unwrap_or("e")).unwrap_or(EstKind::E);
                    let ops = pm.changes_to_ops(&k, b["scenario"].as_str().unwrap_or(""), b["rate"].as_f64().unwrap_or(1.0), est, &ch)?;
                    ed.apply(&k, &actor, &ops).map(|_| serde_json::json!({ "ops": ops.len() }))
                })();
                result(req, r)
            }
            (Method::Get, ["api", "history"]) => {
                let lim = qp("limit").and_then(|x| x.parse().ok()).unwrap_or(100);
                result(req, ed.history(qp("epic").as_deref(), lim).map(|h| serde_json::to_value(h).unwrap()))
            }
            (Method::Post, ["api", "import"]) => {
                let r = serde_json::from_value::<plan_contracts::Plan>(b["plan"].clone()).map_err(|e| vec![Issue::error(format!("invalid plan: {e}"))])
                    .and_then(|p| ed.import(&plan_access::JsonPlan(p), None, &actor).map(|k| serde_json::json!({ "epic": k })));
                result(req, r)
            }
            (Method::Post, ["api", "epic", k, "delete"]) => result(req, ed.delete_epic(&decode(k), &actor).map(|_| serde_json::json!({}))),
            (Method::Get, _) => match web::asset(&url) { Some((ct, b)) => reply(req, ct, b), None => reply(req, "text/plain", b"not found".to_vec()) },
            _ => reply(req, "text/plain", b"method not allowed".to_vec()),
        }
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) { out.push(v); i += 3; continue; }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
