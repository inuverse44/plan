//! 層をまたいで受け渡す型だけを置く（振る舞いは持たない）。設計は docs/design-v0.3.md
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------- 計画（PlanAccess が読み、Engine が使う） ----------

/// 工数・待ちの見積もり。1 つの数か、楽観・最も起こりうる・悲観の 3 つ
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Estimate {
    One(f64),
    Three(f64, f64, f64),
}

/// 見積もりの種類
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
        match self { EstKind::O => "楽観", EstKind::M => "最も起こりうる", EstKind::E => "期待値（PERT）", EstKind::P => "悲観" }
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
    /// 属するストーリーの key（空ならストーリー無し）
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
    /// None は default_owner に任せる
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
    /// 実績（Jira の開始日・解決日など）。計算には使わず、ガントに過去の棒として描く
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_start: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_finish: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_keys: Vec<String>,
}

/// 案で上書きする項目（書かれた項目だけ Some）
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
    /// 消す項目の名前（not_before・fixed・effort・wait・owners）
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

/// 作業・ストーリーに残すコメント（一次情報のリンクなど）。日程の計算には使わない
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(rename = "PlanComment"))]
pub struct Comment {
    pub id: i64,
    /// 作業かストーリーの key
    pub target: String,
    /// 書いた時刻（YYYY-MM-DD HH:MM:SS）。書くときに入る
    #[serde(default)]
    pub at: String,
    /// 書いた人（web・cli など）。書くときに入る
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
    /// エピックの key
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
    /// 空なら基の案 1 つ
    pub scenarios: Vec<ScenarioSpec>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    pub unknown_keys: Vec<String>,
}

// ---------- 暦（CalendarAccess が作る） ----------

/// 営業日の列。時刻は「半日の番号」（slot）で数える：日 i の午前が 2i、午後が 2i+1
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct BusinessCalendar {
    pub days: Vec<NaiveDate>,
    pub start_slot: usize,
    /// days と同じ長さ。本番に触れてはいけない日
    pub frozen: Vec<bool>,
    /// 人ごとの休み（days の番号）
    pub leave: BTreeMap<String, Vec<usize>>,
    /// 平日のうち営業日でない日（祝日・年末年始・off）。画面の網掛け用
    pub off: Vec<NaiveDate>,
}

// ---------- 1 回の計算（Engine どうしの受け渡し） ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Work,
    Wait,
    Milestone,
    Done,
}

/// 案ごとの作業（ScenarioEngine の出力）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Scenario {
    pub key: String,
    pub name: String,
    pub note: String,
    pub tasks: Vec<TaskSpec>,
}

/// 日数の決まった作業（EstimationEngine の出力、SchedulingEngine の入力）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Task {
    pub id: String,
    pub kind: Kind,
    /// 半日の数
    pub dur: usize,
    /// 工数（人日）か待ち（営業日）
    pub val: f64,
    pub owners: Vec<String>,
    pub after: Vec<String>,
    pub prod: bool,
    pub not_before: Option<NaiveDate>,
    pub fixed: Option<NaiveDate>,
}

/// 画面で動かした変更。min_start は開始の下限（半日の番号）、dur は日数（半日の数）
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(optional_fields))]
pub struct Change {
    pub id: String,
    pub min_start: Option<usize>,
    pub dur: Option<usize>,
}

/// SchedulingEngine の出力。添字は tasks と同じ
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Schedule {
    pub order: Vec<usize>,
    pub es: Vec<usize>,
    pub ef: Vec<usize>,
    /// 余裕（半日の数）。済は None
    pub tf: Vec<Option<usize>>,
    pub ff: Vec<Option<usize>>,
    pub why: Vec<String>,
    pub end: usize,
    /// 推移的な依存を省いた先行
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

/// AnalysisEngine の出力
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Assessment {
    pub colors: Vec<FloatColor>,
    pub risk: Option<Risk>,
    pub effort: BTreeMap<String, f64>,
    pub effort_total: f64,
    pub efficiency: Option<f64>,
    /// 人 → 日付 → 使う工数（人日）
    pub load: BTreeMap<String, BTreeMap<NaiveDate, f64>>,
    pub target_slot: usize,
    pub slack_to_deadline: Option<i64>,
    pub critical_path: Vec<String>,
}

// ---------- 外へ出す形（Manager → Client） ----------

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
    /// 並べた順（order）に並ぶ
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

// ---------- 誤りと警告 ----------

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
        if let Some(t) = &self.task { s.push_str(&format!("{t}：")); }
        s.push_str(&self.message);
        s
    }
}

/// 案・稼働率・見積もりの組みの名前（"plan4|1|e"）
pub fn run_key(scenario: &str, rate: f64, est: EstKind) -> String {
    format!("{scenario}|{}|{}", fmt_num(rate), est.code())
}

/// 1.0 → "1"、0.8 → "0.8"（Python の :g と同じ見た目）
pub fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 { format!("{}", x as i64) } else { format!("{}", (x * 1e6).round() / 1e6) }
}

// ---------- 編集（PlanEditingManager → PlanAccess） ----------

/// エピックの設定（日程の条件）
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

/// 1 つの編集。scenario が Some なら、その案の差分として書く（基の作業は変えない）
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
    /// target は作業かストーリーの key
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

/// 画面（web/src）の型を書き出す：cargo test -p plan-contracts --features ts
#[cfg(all(test, feature = "ts"))]
mod ts_export {
    use super::*;
    use ts_rs::{Config, TS};
    #[test]
    fn export_ts() {
        let cfg = Config::new().with_large_int("number");
        let mut out = String::from("// 作ったもの：cargo test -p plan-contracts --features ts（plan-contracts の Rust の型から）。手で直さない\n\n");
        for d in [Estimate::decl(&cfg), EstKind::decl(&cfg), CalendarSpec::decl(&cfg), Colors::decl(&cfg), Person::decl(&cfg), TaskSpec::decl(&cfg), TaskPatch::decl(&cfg), Story::decl(&cfg), Comment::decl(&cfg), ScenarioSpec::decl(&cfg), Plan::decl(&cfg), BusinessCalendar::decl(&cfg), Kind::decl(&cfg), Scenario::decl(&cfg), Task::decl(&cfg), Change::decl(&cfg), Schedule::decl(&cfg), FloatColor::decl(&cfg), Risk::decl(&cfg), Assessment::decl(&cfg), TaskView::decl(&cfg), RunResult::decl(&cfg), ScenarioInfo::decl(&cfg), Evaluation::decl(&cfg), Level::decl(&cfg), Issue::decl(&cfg), EpicSettings::decl(&cfg), EditOp::decl(&cfg), EpicSummary::decl(&cfg), HistoryEntry::decl(&cfg)] { out.push_str(&d.replace("type ", "declare type ")); out.push_str("\n\n"); }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/src/contracts.d.ts");
        std::fs::write(path, out).unwrap();
    }
}
