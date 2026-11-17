//! `plan mcp`: a Model Context Protocol server on stdin/stdout for AI assistants.
//!
//! Messages are JSON-RPC 2.0, one per line. The tools reach the same store as the CLI
//! (the server in ~/.config/plan/config with its token_cmd, otherwise the local SQLite) and
//! return the same data as `--json`. Edits are recorded with the actor `mcp`.
//!
//!   claude mcp add plan -- /path/to/plan mcp
use plan_contracts::Issue;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

const APPLY_HELP: &str = r#"Apply a list of edits to one epic at once. If any edit is invalid (a dependency loop, an unknown task, removing the deadline task, a field this build does not know…) nothing is written.
Edits:
  {"op":"set_story","story":{"key":"S1","name":"Investigate","purpose":"","dod":""}}
  {"op":"add_task","task":{"id":"T1","name":"…","story":"S1","deliverable":"…","effort":[1,2,4],"after":["T0"]}}
  {"op":"set_task","key":"T1","patch":{"effort":2,"not_before":"2026-12-01"},"scenario":null}
  {"op":"link","task":"T2","pred":"T1"}   {"op":"unlink","task":"T2","pred":"T1"}
  {"op":"delete_task","key":"T3"}   {"op":"delete_story","key":"S1"}
  {"op":"clone_scenario","from":"plan1","key":"plan2","name":"Plan 2"}   {"op":"set_scenario","key":"plan2","name":"…"}   {"op":"delete_scenario","key":"plan2"}
  {"op":"set_epic","settings":{"deadline":"2026-12-22","deadline_task":"CUT","owners":["alice","bob"],"max_task_days":2}}
  {"op":"add_comment","target":"T1","text":"Ticket: https://example.com/issues/42"}   {"op":"delete_comment","id":3}
Task fields: id name short story ticket deliverable note owners after needs done not_before fixed actual_start actual_finish (dates YYYY-MM-DD), and effort (person-days) or wait (business days): a number or [optimistic, most likely, pessimistic].
needs: resources the task needs besides people (keys of the plan's resources, e.g. ["prod"]); the task is not placed in their off periods (see dump: resources).
max_task_days: tasks with a larger expected effort get a warning in check (default 2, 0 = off, null = default).
done: true without actual_finish records today as the actual finish.
set_task: "scenario" changes only that scenario (null = the base, every scenario). "patch":{"clear":["not_before","effort","wait","owners","fixed"]} removes fields."#;

fn epic_prop() -> Value { json!({ "type": "string", "description": "the epic key (from epics), or a path to a .toml plan file" }) }
fn run_props() -> Value {
    json!({
        "scenario": { "type": "string", "description": "scenario key (default: the first)" },
        "rate": { "type": "number", "description": "utilization, e.g. 0.8 (default: the first)" },
        "est": { "type": "string", "description": "estimate kind: o, m, e or p (default: e when present)" }
    })
}
fn progress_props() -> Value {
    json!({
        "task": { "type": "string", "description": "task id, ticket, or part of its name" },
        "epic": { "type": "string", "description": "only this epic (default: all)" },
        "date": { "type": "string", "description": "YYYY-MM-DD (default: today)" },
        "person": { "type": "string", "description": "whose next task to report (default: the task's owner)" }
    })
}
fn schema(props: Value, required: &[&str]) -> Value { json!({ "type": "object", "properties": props, "required": required }) }
fn with(mut a: Value, b: Value) -> Value { a.as_object_mut().unwrap().extend(b.as_object().unwrap().clone()); a }

pub fn tools() -> Value {
    let ro = json!({ "readOnlyHint": true });
    json!([
        { "name": "epics", "description": "List the epics (key, name, start, deadline, counts of stories, tasks and scenarios) and where plans live.",
          "inputSchema": schema(json!({}), &[]), "annotations": ro },
        { "name": "dump", "description": "The whole plan of one epic as data: settings, stories, tasks, scenarios and comments. Read this before editing.",
          "inputSchema": schema(json!({ "epic": epic_prop() }), &["epic"]), "annotations": ro },
        { "name": "show", "description": "One schedule task by task (start, finish, float, what drives each start), with the date compared to the deadline, slack to the deadline and the critical path.",
          "inputSchema": schema(with(json!({ "epic": epic_prop() }), run_props()), &["epic"]), "annotations": ro },
        { "name": "calc", "description": "Compare every scenario × utilization × estimate: deadline date, slack, finish, total effort and risk.",
          "inputSchema": schema(json!({ "epic": epic_prop() }), &["epic"]), "annotations": ro },
        { "name": "why", "description": "Why one task starts when it does: its predecessors, owners' availability and float.",
          "inputSchema": schema(with(json!({ "epic": epic_prop(), "task": { "type": "string", "description": "task id" } }), run_props()), &["epic", "task"]), "annotations": ro },
        { "name": "check", "description": "Validate a plan: errors (loops, unknown tasks) and warnings (tasks without a deliverable…).",
          "inputSchema": schema(json!({ "epic": epic_prop() }), &["epic"]), "annotations": ro },
        { "name": "log", "description": "Change history, newest first: who (cli, mcp, web), when, which edit.",
          "inputSchema": schema(json!({ "epic": { "type": "string", "description": "only this epic (default: all)" }, "limit": { "type": "integer", "description": "default 50" } }), &[]), "annotations": ro },
        { "name": "load", "description": "People across epics: each epic's default run (first scenario, first utilization, e) added up per person and day (1 = one person's whole day). Lists the days over 100% with the share of each epic. Epics are still scheduled on their own; this only adds the loads up.",
          "inputSchema": schema(json!({ "person": { "type": "string", "description": "only this person (default: everyone)" } }), &[]), "annotations": ro },
        { "name": "next", "description": "The single task a person should do now, across all epics: ready (predecessors done), not done, least float, then earliest start, then earliest deadline. Also the one after it. Each epic's default run.",
          "inputSchema": schema(json!({ "person": { "type": "string", "description": "owner name (default: everyone)" } }), &[]), "annotations": ro },
        { "name": "today", "description": "A daily briefing for a person across all epics: the next task, tasks planned to start by today but not started, tasks due by Sunday, and risks (late starts, epics planned past their deadline, epics that cannot be evaluated). Dates are the local today.",
          "inputSchema": schema(json!({ "person": { "type": "string", "description": "owner name (default: everyone)" } }), &[]), "annotations": ro },
        { "name": "start", "description": "Record that a task started (\"I started X\"): sets its actual start (default today). The task is found across epics by id or ticket, otherwise by part of its name; if several match, the candidates come back and nothing changes. Returns the effect on the schedule and the person's next task.",
          "inputSchema": schema(progress_props(), &["task"]), "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true } },
        { "name": "finish", "description": "Record that a task is done (\"I finished X\"): marks it done with its actual finish (default today). Found like start. Returns the effect on the schedule (new end, slack) and the person's next task.",
          "inputSchema": schema(progress_props(), &["task"]), "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true } },
        { "name": "delete_epic", "description": "Delete a whole epic (its stories, tasks and scenarios). Its content stays in the change history (log). Ask the person before using it.",
          "inputSchema": schema(json!({ "epic": { "type": "string", "description": "the epic key" } }), &["epic"]),
          "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true } },
        { "name": "apply", "description": APPLY_HELP,
          "inputSchema": schema(json!({ "epic": { "type": "string", "description": "the epic key" }, "ops": { "type": "array", "items": { "type": "object" }, "description": "the edits, in order" } }), &["epic", "ops"]),
          "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false } }
    ])
}

/// Run one tool. Ok is the data, Err the issues (both are returned to the model)
pub fn call(name: &str, args: &Value) -> Result<Value, Vec<Issue>> {
    let s = |k: &str| args.get(k).and_then(Value::as_str);
    let need = |k: &str| s(k).ok_or_else(|| vec![Issue::error(format!("{name} needs \"{k}\""))]);
    let rate = args.get("rate").map(|v| v.as_f64().map(|x| x.to_string()).unwrap_or_else(|| v.as_str().unwrap_or_default().to_string()));
    let run = |rate: &Option<String>| -> (Option<String>, Option<String>, Option<String>) { (s("scenario").map(str::to_string), rate.clone(), s("est").map(str::to_string)) };
    match name {
        "epics" => crate::epics_data(),
        "dump" => crate::dump_data(need("epic")?),
        "show" | "calc" | "why" => {
            let (sc, r, e) = run(&rate);
            let task = if name == "why" { Some(need("task")?) } else { None };
            crate::eval_data(name, need("epic")?, (sc.as_deref(), r.as_deref(), e.as_deref()), task)
        }
        "check" => { let (ok, is) = crate::check_data(need("epic")?); if ok { Ok(json!({ "issues": is })) } else { Err(is) } }
        "load" => crate::load_data(s("person")),
        "next" => crate::agenda_data(s("person")).map(|d| json!({ "next": d["next"], "then": d["then"] })),
        "today" => crate::agenda_data(s("person")),
        "start" | "finish" => {
            let date = s("date").map(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| vec![Issue::error(format!("date must be YYYY-MM-DD: {d}"))])).transpose()?;
            let step = if name == "start" { crate::progress::Step::Start } else { crate::progress::Step::Finish };
            crate::progress::record(step, need("task")?, s("epic"), date, s("person"), "mcp")
        }
        "log" => crate::log_data(s("epic"), args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize),
        "apply" => {
            let ops = crate::edits::parse(args.get("ops").cloned().unwrap_or(Value::Null))?;
            crate::apply_data(need("epic")?, "mcp", &ops)
        }
        "delete_epic" => {
            let k = need("epic")?;
            plan_manager::PlanEditingManager::new(crate::store()).delete_epic(k, "mcp")?;
            Ok(json!({ "deleted": k, "note": "its content stays in the log" }))
        }
        _ => Err(vec![Issue::error(format!("unknown tool: {name}"))]),
    }
}

/// Answer one JSON-RPC message. None for notifications
pub fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or_default();
    let ok = |r: Value| Some(json!({ "jsonrpc": "2.0", "id": id, "result": r }));
    match method {
        "initialize" => ok(json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "plan", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "Plans of epics > stories > tasks with dependencies and estimates. Read with epics, dump, show, calc, why, log, load (people across epics); edit with apply (one batch, all or nothing), delete_epic removes a whole epic; its result says how the deadline, slack and critical path moved. The tool keeps plans only, not status."
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": tools() })),
        "tools/call" => {
            let p = &msg["params"];
            let args = p.get("arguments").cloned().unwrap_or(json!({}));
            let (data, err) = match call(p["name"].as_str().unwrap_or_default(), &args) {
                Ok(d) => (json!({ "ok": true, "data": d }), false),
                Err(is) => (json!({ "ok": false, "issues": is }), true),
            };
            ok(json!({ "content": [{ "type": "text", "text": data.to_string() }], "isError": err }))
        }
        m => Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {m}") } })),
    }
}

pub fn run() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg),
            Err(x) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {x}") } })),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/sample.toml");

    fn rpc(method: &str, params: Value) -> Value { handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap() }
    fn tool(name: &str, args: Value) -> (bool, Value) {
        let r = rpc("tools/call", json!({ "name": name, "arguments": args }));
        let body: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        (r["result"]["isError"] == json!(true), body)
    }

    #[test]
    fn handshake_and_tool_list() {
        let r = rpc("initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }));
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert!(r["result"]["capabilities"]["tools"].is_object());
        assert!(handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let names: Vec<String> = rpc("tools/list", json!({}))["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for n in ["epics", "dump", "show", "calc", "why", "check", "log", "load", "delete_epic", "apply"] { assert!(names.contains(&n.to_string()), "{n}"); }
        assert_eq!(rpc("nope", json!({}))["error"]["code"], -32601);
    }

    #[test]
    fn show_matches_the_cli_data() {
        let (err, body) = tool("show", json!({ "epic": SAMPLE }));
        assert!(!err, "{body}");
        let expect = crate::eval_data("show", SAMPLE, (None, None, None), None).unwrap();
        assert_eq!(body["data"], expect);
        assert!(!body["data"]["critical_path"].as_array().unwrap().is_empty());
        let (err, body) = tool("show", json!({ "epic": SAMPLE, "scenario": "no-such" }));
        assert!(err && body["ok"] == json!(false), "{body}");
    }

    #[test]
    fn apply_rejects_malformed_edits() {
        let (err, body) = tool("apply", json!({ "epic": "X", "ops": [{ "op": "no_such_op" }] }));
        assert!(err && body["issues"][0]["message"].as_str().unwrap().contains("invalid edits"), "{body}");
        let (err, _) = tool("apply", json!({ "ops": [] }));
        assert!(err);
    }

    /// epics → apply → dump → log against a fresh local SQLite (PLAN_DB also turns the server off)
    #[test]
    fn edit_round_trip_on_sqlite() {
        let dir = std::env::temp_dir().join(format!("plan-mcp-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PLAN_DB", dir.join("plan.db"));
        plan_manager::PlanEditingManager::new(crate::store()).import(&plan_access::TomlFile(SAMPLE.into()), Some("demo"), "import").unwrap();

        let (err, body) = tool("epics", json!({}));
        assert!(!err && body["data"]["epics"].as_array().unwrap().iter().any(|e| e["key"] == "demo"), "{body}");
        let first = tool("dump", json!({ "epic": "demo" })).1["data"]["tasks"][0]["id"].as_str().unwrap().to_string();

        let (err, body) = tool("apply", json!({ "epic": "demo", "ops": [{ "op": "set_task", "key": first, "patch": { "note": "via mcp" } }] }));
        assert!(!err && body["data"]["applied"] == json!(1), "{body}");
        assert_eq!(body["data"]["impact"]["changed"], json!(false), "a note does not move the schedule: {body}");
        let (_, body) = tool("dump", json!({ "epic": "demo" }));
        assert_eq!(body["data"]["tasks"][0]["note"], "via mcp");
        let (_, body) = tool("log", json!({ "epic": "demo", "limit": 1 }));
        assert_eq!(body["data"][0]["actor"], "mcp");

        // load: one epic alone, nobody over 100%
        let (err, body) = tool("load", json!({}));
        assert!(!err && body["data"]["epics"][0]["epic"] == "demo", "{body}");
        let people = body["data"]["people"].as_object().unwrap();
        assert!(!people.is_empty() && people.values().all(|p| p["over"].as_array().unwrap().is_empty()), "{body}");

        // a loop is refused and nothing is written
        let (err, _) = tool("apply", json!({ "epic": "demo", "ops": [{ "op": "set_task", "key": first, "patch": { "note": "x" } }, { "op": "link", "task": first, "pred": first }] }));
        assert!(err);
        assert_eq!(tool("dump", json!({ "epic": "demo" })).1["data"]["tasks"][0]["note"], "via mcp");

        // a longer task on the critical path moves the end, and apply says so
        let show = tool("show", json!({ "epic": "demo" })).1;
        let crit = show["data"]["critical_path"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string())
            .find(|id| show["data"]["tasks"].as_array().unwrap().iter().any(|t| t["id"] == json!(id) && t["kind"] == "work")).unwrap();
        let (err, body) = tool("apply", json!({ "epic": "demo", "ops": [{ "op": "set_task", "key": crit, "patch": { "effort": 30 } }] }));
        let i = &body["data"]["impact"];
        assert!(!err && i["changed"] == json!(true), "{body}");
        assert_eq!(i["before"]["finish"], show["data"]["finish"]);
        assert!(i["after"]["finish"].as_str().unwrap() > i["before"]["finish"].as_str().unwrap(), "{body}");
        assert_eq!(i["after"]["finish"], tool("show", json!({ "epic": "demo" })).1["data"]["finish"]);

        // delete_epic removes the epic; its content stays in the log
        let (err, body) = tool("delete_epic", json!({ "epic": "demo" }));
        assert!(!err && body["data"]["deleted"] == "demo", "{body}");
        assert!(!tool("epics", json!({})).1["data"]["epics"].as_array().unwrap().iter().any(|e| e["key"] == "demo"));
        assert!(tool("dump", json!({ "epic": "demo" })).0);
        assert_eq!(tool("log", json!({ "epic": "demo", "limit": 1 })).1["data"][0]["actor"], "mcp");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
