//! Reading edits from clients (CLI, MCP, HTTP) strictly: a field this build does not know is an error, not silently dropped.
//!
//! `EditOp` and `EpicSettings` reject unknown fields themselves. Tasks, patches and stories are also stored
//! in plans, where an older build must still read newer data, so they stay lenient there and are checked here instead.
use chrono::NaiveDate;
use plan_contracts::{EditOp, Issue, Story, TaskPatch, TaskSpec};
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde_json::Value;

/// Parse a JSON list of edits, rejecting unknown fields anywhere in them
pub fn parse(v: Value) -> Result<Vec<EditOp>, Vec<Issue>> {
    let Value::Array(items) = &v else { return Err(vec![Issue::error("edits must be a JSON list, e.g. [{\"op\":\"set_task\",\"key\":\"T1\",\"patch\":{\"effort\":2}}]")]) };
    let mut issues = vec![];
    for (i, op) in items.iter().enumerate() {
        let at = |what: &str, extra: Vec<String>, known: &[&str]| Issue::error(format!("edit {} ({}): unknown field{} in {what}: {} (known: {})",
            i + 1, op["op"].as_str().unwrap_or("?"), if extra.len() > 1 { "s" } else { "" }, extra.join(", "), known.join(", ")));
        let mut check = |what: &str, obj: &Value, known: &[&str]| {
            let extra = unknown(obj, known);
            if !extra.is_empty() { issues.push(at(what, extra, known)); }
        };
        match op["op"].as_str() {
            Some("add_task") => check("task", &op["task"], fields::<TaskSpec>()),
            Some("set_task") => check("patch", &op["patch"], fields::<TaskPatch>()),
            Some("set_story") => check("story", &op["story"], fields::<Story>()),
            _ => {}
        }
    }
    if !issues.is_empty() { return Err(issues); }
    serde_json::from_value(v).map_err(|e| vec![Issue::error(format!("invalid edits: {e}"))])
}

/// Marking a task done without a finish date records `today` as its actual finish (the person's local day, so the CLI and MCP fill it, not the server)
pub fn finish_today(ops: &mut [EditOp], today: NaiveDate) {
    for op in ops {
        match op {
            EditOp::SetTask { patch, .. } if patch.done == Some(true) && patch.actual_finish.is_none() && !patch.clear.iter().any(|c| c == "actual_finish") => patch.actual_finish = Some(today),
            EditOp::AddTask { task } if task.done && task.actual_finish.is_none() => task.actual_finish = Some(today),
            _ => {}
        }
    }
}

fn unknown(obj: &Value, known: &[&str]) -> Vec<String> {
    obj.as_object().map(|m| m.keys().filter(|k| !known.contains(&k.as_str())).cloned().collect()).unwrap_or_default()
}

/// The field names serde reads for a struct (asked from its Deserialize impl, so it cannot drift from the type)
fn fields<'de, T: Deserialize<'de>>() -> &'static [&'static str] {
    struct Probe<'a>(&'a mut &'static [&'static str]);
    #[derive(Debug)]
    struct Stop;
    impl std::fmt::Display for Stop { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("stop") } }
    impl std::error::Error for Stop {}
    impl de::Error for Stop { fn custom<M: std::fmt::Display>(_: M) -> Self { Stop } }
    impl<'de> Deserializer<'de> for Probe<'_> {
        type Error = Stop;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Stop> { Err(Stop) }
        fn deserialize_struct<V: Visitor<'de>>(self, _: &'static str, f: &'static [&'static str], _: V) -> Result<V::Value, Stop> { *self.0 = f; Err(Stop) }
        serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any }
    }
    let mut f: &'static [&'static str] = &[];
    let _ = T::deserialize(Probe(&mut f));
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_edits_pass() {
        let ops = parse(json!([
            { "op": "add_task", "task": { "id": "T1", "name": "a", "effort": [1, 2, 3], "actual_finish": "2026-11-17" } },
            { "op": "set_task", "key": "T1", "patch": { "note": "x", "clear": ["fixed"], "prod": true }, "scenario": null },
            { "op": "set_story", "story": { "key": "S1", "name": "s", "purpose": "", "dod": "" } },
            { "op": "set_epic", "settings": { "owners": ["a"] } }
        ])).unwrap();
        assert_eq!(ops.len(), 4);
    }

    #[test]
    fn unknown_fields_are_named() {
        for (v, word) in [
            (json!([{ "op": "set_epic", "settings": { "owner": ["a"] } }]), "owner"),
            (json!([{ "op": "set_task", "key": "T1", "patch": { "efort": 2 } }]), "efort"),
            (json!([{ "op": "add_task", "task": { "id": "T1", "name": "a", "owner": ["x"] } }]), "owner"),
            (json!([{ "op": "set_story", "story": { "key": "S", "name": "s", "purpose": "", "dod": "", "goal": "" } }]), "goal"),
            (json!([{ "op": "link", "task": "T2", "pred": "T1", "lag": 1 }]), "lag"),
        ] {
            let e = parse(v).unwrap_err();
            assert!(e[0].message.contains(word), "{}", e[0].message);
        }
    }

    #[test]
    fn done_without_a_date_finishes_today() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 17).unwrap();
        let mut ops = parse(json!([
            { "op": "set_task", "key": "A", "patch": { "done": true } },
            { "op": "set_task", "key": "B", "patch": { "done": true, "actual_finish": "2026-11-01" } },
            { "op": "set_task", "key": "C", "patch": { "done": false } },
            { "op": "add_task", "task": { "id": "D", "name": "d", "done": true } }
        ])).unwrap();
        finish_today(&mut ops, d);
        let fin = |i: usize| match &ops[i] { EditOp::SetTask { patch, .. } => patch.actual_finish, EditOp::AddTask { task } => task.actual_finish, _ => None };
        assert_eq!([fin(0), fin(1), fin(2), fin(3)], [Some(d), NaiveDate::from_ymd_opt(2026, 11, 1), None, Some(d)]);
    }

    #[test]
    fn stored_tasks_stay_lenient() {
        // An older build must still read a plan written by a newer one
        let t: TaskSpec = serde_json::from_value(json!({ "id": "T1", "name": "a", "from_the_future": 1 })).unwrap();
        assert_eq!(t.id, "T1");
    }
}
