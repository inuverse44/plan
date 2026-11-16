//! Types passed between layers, and nothing else (no behavior). See docs/design.md
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------- Plans (read by PlanAccess, used by the engines) ----------

/// An effort or wait estimate: one number, or optimistic, most likely and pessimistic
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Estimate {
    One(f64),
    Three(f64, f64, f64),
}

/// Kind of estimate
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum EstKind {
    O,
    M,
    E,
    P,
}

impl EstKind {
    pub fn code(self) -> &'static str {
        match self { EstKind::O => "o", EstKind::M => "m", EstKind::E => "e", EstKind::P => "p" }
    }
    pub fn name(self) -> &'static str {
        match self { EstKind::O => "optimistic", EstKind::M => "most likely", EstKind::E => "expected (PERT)", EstKind::P => "pessimistic" }
    }
    pub fn parse(s: &str) -> Option<EstKind> {
        match s { "o" => Some(EstKind::O), "m" => Some(EstKind::M), "e" => Some(EstKind::E), "p" => Some(EstKind::P), _ => None }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct CalendarSpec {
    pub holidays_jp: bool,
    pub year_end: bool,
    pub off: Vec<NaiveDate>,
    pub freeze: Vec<(NaiveDate, NaiveDate)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Colors {
    pub red: f64,
    pub yellow: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Person {
    pub leave: Vec<NaiveDate>,
    pub rate: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskSpec {
    pub id: String,
    /// Key of the story this task belongs to (empty: no story)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub story: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub short: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ticket: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub deliverable: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Estimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<Estimate>,
    /// None falls back to default_owner
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prod: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
    /// Actual dates (e.g. from an issue tracker). Not used for scheduling; drawn as past bars in the Gantt chart
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_finish: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_keys: Vec<String>,
}

/// Fields a scenario overrides (only the written ones are Some)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deliverable: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Estimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<Estimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prod: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixed: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_finish: Option<NaiveDate>,
    /// Names of fields to clear (not_before, fixed, effort, wait, owners)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clear: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Story {
    pub key: String,
    pub name: String,
    pub purpose: String,
    pub dod: String,
}

/// A comment on a task or story (for example a link to the source). Not used for scheduling
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "PlanComment"))]
pub struct Comment {
    pub id: i64,
    /// Key of a task or story
    pub target: String,
    /// When it was written (YYYY-MM-DD HH:MM:SS); set on write
    #[serde(default)]
    pub at: String,
    /// Who wrote it (web, cli, ...); set on write
    #[serde(default)]
    pub by: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ScenarioSpec {
    pub key: String,
    pub name: String,
    pub note: String,
    pub drop: Vec<String>,
    pub set: Vec<(String, TaskPatch)>,
    pub add: Vec<TaskSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Plan {
    /// Epic key
    #[serde(default)]
    pub key: String,
    pub title: String,
    pub description: String,
    pub start: NaiveDate,
    pub start_pm: bool,
    pub deadline: Option<NaiveDate>,
    pub deadline_task: Option<String>,
    pub deadline_label: Option<String>,
    pub rates: Vec<f64>,
    pub estimates: Vec<EstKind>,
    pub round: f64,
    pub default_owner: Option<String>,
    pub calendar: CalendarSpec,
    pub colors: Colors,
    pub people: BTreeMap<String, Person>,
    #[serde(default)]
    pub stories: Vec<Story>,
    pub tasks: Vec<TaskSpec>,
    /// Empty: just the base scenario
    pub scenarios: Vec<ScenarioSpec>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    pub unknown_keys: Vec<String>,
}

// ---------- Calendar (built by CalendarAccess) ----------

/// Business days. Time is counted in half-day slots: day i morning is 2i, afternoon is 2i+1
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BusinessCalendar {
    pub days: Vec<NaiveDate>,
    pub start_slot: usize,
    /// Same length as days: days when production must not be touched
    pub frozen: Vec<bool>,
    /// Days off per person (indexes into days)
    pub leave: BTreeMap<String, Vec<usize>>,
    /// Weekdays that are not business days (holidays, year end, off). Shaded in the UI
    pub off: Vec<NaiveDate>,
}

// ---------- One calculation (passed between engines) ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Work,
    Wait,
    Milestone,
    Done,
}

/// Tasks of one scenario (output of ScenarioEngine)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Scenario {
    pub key: String,
    pub name: String,
    pub note: String,
    pub tasks: Vec<TaskSpec>,
}

/// A task with a fixed duration (output of EstimationEngine, input of SchedulingEngine)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Task {
    pub id: String,
    pub kind: Kind,
    /// In half-days
    pub dur: usize,
    /// Effort (person-days) or wait (business days)
    pub val: f64,
    pub owners: Vec<String>,
    pub after: Vec<String>,
    pub prod: bool,
    pub not_before: Option<NaiveDate>,
    pub fixed: Option<NaiveDate>,
}

/// A move made in the UI. min_start is the earliest start (slot), dur the duration (half-days)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct Change {
    pub id: String,
    pub min_start: Option<usize>,
    pub dur: Option<usize>,
}

/// Output of SchedulingEngine. Indexes match tasks
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Schedule {
    pub order: Vec<usize>,
    pub es: Vec<usize>,
    pub ef: Vec<usize>,
    /// Float in half-days. None for done tasks
    pub tf: Vec<Option<usize>>,
    pub ff: Vec<Option<usize>>,
    pub why: Vec<String>,
    pub end: usize,
    /// Predecessors without transitive ones
    pub reduced_after: Vec<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum FloatColor {
    Crit,
    Red,
    Yellow,
    Green,
    Wait,
    Done,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Risk {
    pub criticality: f64,
    pub fibonacci: f64,
    pub activity: Option<f64>,
}

/// Output of AnalysisEngine
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Assessment {
    pub colors: Vec<FloatColor>,
    pub risk: Option<Risk>,
    pub effort: BTreeMap<String, f64>,
    pub effort_total: f64,
    pub efficiency: Option<f64>,
    /// Person → date → effort used (person-days)
    pub load: BTreeMap<String, BTreeMap<NaiveDate, f64>>,
    pub target_slot: usize,
    pub slack_to_deadline: Option<i64>,
    pub critical_path: Vec<String>,
}

// ---------- Results (Manager → Client) ----------

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TaskView {
    pub id: String,
    pub story: String,
    pub name: String,
    pub short: String,
    pub ticket: String,
    pub deliverable: String,
    pub group: String,
    pub note: String,
    pub kind: Kind,
    pub owners: Vec<String>,
    pub after: Vec<String>,
    pub reduced_after: Vec<String>,
    pub prod: bool,
    pub dur: usize,
    pub val: f64,
    pub base_dur: usize,
    pub release: usize,
    pub es: usize,
    pub ef: usize,
    pub tf: Option<usize>,
    pub ff: Option<usize>,
    pub color: FloatColor,
    pub why: String,
    pub done: bool,
    pub actual_start: Option<NaiveDate>,
    pub actual_finish: Option<NaiveDate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RunResult {
    pub key: String,
    pub scenario: String,
    pub rate: f64,
    pub est: EstKind,
    /// In scheduling order
    pub tasks: Vec<TaskView>,
    pub end: usize,
    pub target: usize,
    pub slack_to_deadline: Option<i64>,
    pub risk: Option<Risk>,
    pub effort: BTreeMap<String, f64>,
    pub effort_total: f64,
    pub efficiency: Option<f64>,
    pub load: BTreeMap<String, BTreeMap<NaiveDate, f64>>,
    pub critical_path: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ScenarioInfo {
    pub key: String,
    pub name: String,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Evaluation {
    pub key: String,
    pub stories: Vec<Story>,
    pub title: String,
    pub description: String,
    pub deadline: Option<NaiveDate>,
    pub deadline_task: Option<String>,
    pub deadline_label: Option<String>,
    pub calendar: BusinessCalendar,
    pub freeze: Vec<(NaiveDate, NaiveDate)>,
    pub scenarios: Vec<ScenarioInfo>,
    pub rates: Vec<f64>,
    pub estimates: Vec<EstKind>,
    pub runs: Vec<RunResult>,
    pub issues: Vec<Issue>,
}

// ---------- Errors and warnings ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Issue {
    pub level: Level,
    pub scenario: Option<String>,
    pub task: Option<String>,
    pub message: String,
}

impl Issue {
    pub fn error(msg: impl Into<String>) -> Issue {
        Issue { level: Level::Error, scenario: None, task: None, message: msg.into() }
    }
    pub fn warning(msg: impl Into<String>) -> Issue {
        Issue { level: Level::Warning, scenario: None, task: None, message: msg.into() }
    }
    pub fn at(mut self, scenario: Option<&str>, task: Option<&str>) -> Issue {
        self.scenario = scenario.map(str::to_string);
        self.task = task.map(str::to_string);
        self
    }
    pub fn line(&self) -> String {
        let mut s = String::new();
        if let Some(sc) = &self.scenario { s.push_str(&format!("[{sc}] ")); }
        if let Some(t) = &self.task { s.push_str(&format!("{t}: ")); }
        s.push_str(&self.message);
        s
    }
}

/// Name of a scenario × utilization × estimate run ("plan4|1|e")
pub fn run_key(scenario: &str, rate: f64, est: EstKind) -> String {
    format!("{scenario}|{}|{}", fmt_num(rate), est.code())
}

/// 1.0 → "1", 0.8 → "0.8" (like Python's :g)
pub fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{}", (x * 1e6).round() / 1e6) }
}

// ---------- Edits (PlanEditingManager → PlanAccess) ----------

/// Epic settings (scheduling conditions)
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct EpicSettings {
    pub name: Option<String>,
    pub description: Option<String>,
    pub start: Option<NaiveDate>,
    pub start_pm: Option<bool>,
    pub deadline: Option<Option<NaiveDate>>,
    pub deadline_task: Option<Option<String>>,
    pub deadline_label: Option<Option<String>>,
    pub rates: Option<Vec<f64>>,
    pub default_owner: Option<Option<String>>,
}

/// One edit. With scenario = Some, it is written as that scenario's difference (the base tasks stay)
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    SetEpic { settings: EpicSettings },
    SetStory { story: Story },
    DeleteStory { key: String },
    AddTask { task: TaskSpec },
    SetTask { key: String, patch: TaskPatch, #[serde(default)] scenario: Option<String> },
    DeleteTask { key: String },
    Link { task: String, pred: String },
    Unlink { task: String, pred: String },
    CloneScenario { from: Option<String>, key: String, name: String },
    SetScenario { key: String, name: Option<String>, note: Option<String> },
    DeleteScenario { key: String },
    /// target is a task or story key
    AddComment { target: String, text: String },
    DeleteComment { id: i64 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct EpicSummary {
    pub key: String,
    pub name: String,
    pub start: NaiveDate,
    pub deadline: Option<NaiveDate>,
    pub stories: usize,
    pub tasks: usize,
    pub scenarios: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct HistoryEntry {
    pub id: i64,
    pub at: String,
    pub actor: String,
    pub epic: String,
    pub op: String,
    pub payload: String,
}

/// Export the types for the UI (web/src): cargo test -p plan-contracts --features ts
#[cfg(all(test, feature = "ts"))]
mod ts_export {
    use super::*;
    use ts_rs::{Config, TS};
    #[test]
    fn export_ts() {
        let cfg = Config::new().with_large_int("number");
        let mut out = String::from("// Generated by cargo test -p plan-contracts --features ts from the Rust types in plan-contracts. Do not edit\n\n");
        for d in [Estimate::decl(&cfg), EstKind::decl(&cfg), CalendarSpec::decl(&cfg), Colors::decl(&cfg), Person::decl(&cfg), TaskSpec::decl(&cfg), TaskPatch::decl(&cfg), Story::decl(&cfg), Comment::decl(&cfg), ScenarioSpec::decl(&cfg), Plan::decl(&cfg), BusinessCalendar::decl(&cfg), Kind::decl(&cfg), Scenario::decl(&cfg), Task::decl(&cfg), Change::decl(&cfg), Schedule::decl(&cfg), FloatColor::decl(&cfg), Risk::decl(&cfg), Assessment::decl(&cfg), TaskView::decl(&cfg), RunResult::decl(&cfg), ScenarioInfo::decl(&cfg), Evaluation::decl(&cfg), Level::decl(&cfg), Issue::decl(&cfg), EpicSettings::decl(&cfg), EditOp::decl(&cfg), EpicSummary::decl(&cfg), HistoryEntry::decl(&cfg)] { out.push_str(&d.replace("type ", "declare type ")); out.push_str("\n\n"); }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/src/contracts.d.ts");
        std::fs::write(path, out).unwrap();
    }
}
