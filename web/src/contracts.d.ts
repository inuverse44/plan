// 作ったもの：cargo test -p plan-contracts --features ts（plan-contracts の Rust の型から）。手で直さない

declare type Estimate = number | [number, number, number];

declare type EstKind = "o" | "m" | "e" | "p";

declare type CalendarSpec = { holidays_jp: boolean, year_end: boolean, off: Array<string>, freeze: Array<[string, string]>, };

declare type Colors = { red: number, yellow: number, };

declare type Person = { leave: Array<string>, rate: number | null, };

declare type TaskSpec = { id: string, 
/**
 * 属するストーリーの key（空ならストーリー無し）
 */
story?: string, name: string, short?: string, ticket?: string, deliverable?: string, group?: string, note?: string, effort?: Estimate | null, wait?: Estimate | null, 
/**
 * None は default_owner に任せる
 */
owners?: Array<string> | null, after?: Array<string>, prod?: boolean, not_before?: string | null, fixed?: string | null, done?: boolean, 
/**
 * 実績（Jira の開始日・解決日など）。計算には使わず、ガントに過去の棒として描く
 */
actual_start?: string | null, actual_finish?: string | null, unknown_keys?: Array<string>, };

declare type TaskPatch = { story?: string | null, name?: string | null, short?: string | null, ticket?: string | null, deliverable?: string | null, group?: string | null, note?: string | null, effort?: Estimate | null, wait?: Estimate | null, owners?: Array<string> | null, after?: Array<string> | null, prod?: boolean | null, not_before?: string | null, fixed?: string | null, done?: boolean | null, actual_start?: string | null, actual_finish?: string | null, 
/**
 * 消す項目の名前（not_before・fixed・effort・wait・owners）
 */
clear?: Array<string>, };

declare type Story = { key: string, name: string, purpose: string, dod: string, };

declare type PlanComment = { id: number, 
/**
 * 作業かストーリーの key
 */
target: string, 
/**
 * 書いた時刻（YYYY-MM-DD HH:MM:SS）。書くときに入る
 */
at: string, 
/**
 * 書いた人（web・cli など）。書くときに入る
 */
by: string, text: string, };

declare type ScenarioSpec = { key: string, name: string, note: string, drop: Array<string>, set: Array<[string, TaskPatch]>, add: Array<TaskSpec>, };

declare type Plan = { 
/**
 * エピックの key
 */
key: string, title: string, description: string, start: string, start_pm: boolean, deadline: string | null, deadline_task: string | null, deadline_label: string | null, rates: Array<number>, estimates: Array<EstKind>, round: number, default_owner: string | null, calendar: CalendarSpec, colors: Colors, people: { [key in string]: Person }, stories: Array<Story>, tasks: Array<TaskSpec>, 
/**
 * 空なら基の案 1 つ
 */
scenarios: Array<ScenarioSpec>, comments: Array<PlanComment>, unknown_keys: Array<string>, };

declare type BusinessCalendar = { days: Array<string>, start_slot: number, 
/**
 * days と同じ長さ。本番に触れてはいけない日
 */
frozen: Array<boolean>, 
/**
 * 人ごとの休み（days の番号）
 */
leave: { [key in string]: Array<number> }, 
/**
 * 平日のうち営業日でない日（祝日・年末年始・off）。画面の網掛け用
 */
off: Array<string>, };

declare type Kind = "work" | "wait" | "milestone" | "done";

declare type Scenario = { key: string, name: string, note: string, tasks: Array<TaskSpec>, };

declare type Task = { id: string, kind: Kind, 
/**
 * 半日の数
 */
dur: number, 
/**
 * 工数（人日）か待ち（営業日）
 */
val: number, owners: Array<string>, after: Array<string>, prod: boolean, not_before: string | null, fixed: string | null, };

declare type Change = { id: string, min_start?: number, dur?: number, };

declare type Schedule = { order: Array<number>, es: Array<number>, ef: Array<number>, 
/**
 * 余裕（半日の数）。済は None
 */
tf: Array<number | null>, ff: Array<number | null>, why: Array<string>, end: number, 
/**
 * 推移的な依存を省いた先行
 */
reduced_after: Array<Array<string>>, };

declare type FloatColor = "crit" | "red" | "yellow" | "green" | "wait" | "done";

declare type Risk = { criticality: number, fibonacci: number, activity: number | null, };

declare type Assessment = { colors: Array<FloatColor>, risk: Risk | null, effort: { [key in string]: number }, effort_total: number, efficiency: number | null, 
/**
 * 人 → 日付 → 使う工数（人日）
 */
load: { [key in string]: { [key in string]: number } }, target_slot: number, slack_to_deadline: number | null, critical_path: Array<string>, };

declare type TaskView = { id: string, story: string, name: string, short: string, ticket: string, deliverable: string, group: string, note: string, kind: Kind, owners: Array<string>, after: Array<string>, reduced_after: Array<string>, prod: boolean, dur: number, val: number, base_dur: number, release: number, es: number, ef: number, tf: number | null, ff: number | null, color: FloatColor, why: string, done: boolean, actual_start: string | null, actual_finish: string | null, };

declare type RunResult = { key: string, scenario: string, rate: number, est: EstKind, 
/**
 * 並べた順（order）に並ぶ
 */
tasks: Array<TaskView>, end: number, target: number, slack_to_deadline: number | null, risk: Risk | null, effort: { [key in string]: number }, effort_total: number, efficiency: number | null, load: { [key in string]: { [key in string]: number } }, critical_path: Array<string>, };

declare type ScenarioInfo = { key: string, name: string, note: string, };

declare type Evaluation = { key: string, stories: Array<Story>, title: string, description: string, deadline: string | null, deadline_task: string | null, deadline_label: string | null, calendar: BusinessCalendar, freeze: Array<[string, string]>, scenarios: Array<ScenarioInfo>, rates: Array<number>, estimates: Array<EstKind>, runs: Array<RunResult>, issues: Array<Issue>, };

declare type Level = "error" | "warning";

declare type Issue = { level: Level, scenario: string | null, task: string | null, message: string, };

declare type EpicSettings = { name?: string, description?: string, start?: string, start_pm?: boolean, deadline?: string | null, deadline_task?: string | null, deadline_label?: string | null, rates?: Array<number>, default_owner?: string | null, };

declare type EditOp = { "op": "set_epic", settings: EpicSettings, } | { "op": "set_story", story: Story, } | { "op": "delete_story", key: string, } | { "op": "add_task", task: TaskSpec, } | { "op": "set_task", key: string, patch: TaskPatch, scenario?: string, } | { "op": "delete_task", key: string, } | { "op": "link", task: string, pred: string, } | { "op": "unlink", task: string, pred: string, } | { "op": "clone_scenario", from?: string, key: string, name: string, } | { "op": "set_scenario", key: string, name?: string, note?: string, } | { "op": "delete_scenario", key: string, } | { "op": "add_comment", target: string, text: string, } | { "op": "delete_comment", id: number, };

declare type EpicSummary = { key: string, name: string, start: string, deadline: string | null, stories: number, tasks: number, scenarios: number, };

declare type HistoryEntry = { id: number, at: string, actor: string, epic: string, op: string, payload: string, };

