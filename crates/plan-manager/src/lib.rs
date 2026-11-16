//! Managers: only the order of the use cases.
//! PlanningManager checks, evaluates and tries moves; PlanEditingManager creates, edits, links and clones scenarios
use plan_access::{CalendarAccess, PlanAccess};
use std::sync::Arc;
use plan_contracts::*;
use plan_engines::{analysis, estimation, scenario, scheduling};

pub struct PlanningManager {
    plan_access: Arc<dyn PlanAccess + Send + Sync>,
    calendar_access: Arc<dyn CalendarAccess + Send + Sync>,
}

struct Loaded {
    plan: Plan,
    cal: BusinessCalendar,
    scenarios: Vec<Scenario>,
}

impl PlanningManager {
    pub fn new(plan_access: Arc<dyn PlanAccess + Send + Sync>, calendar_access: Arc<dyn CalendarAccess + Send + Sync>) -> Self {
        PlanningManager { plan_access, calendar_access }
    }

    fn load(&self, epic: &str) -> Result<Loaded, Vec<Issue>> {
        let plan = self.plan_access.read_plan(epic)?;
        let cal = self.calendar_access.business_calendar(&plan);
        let scenarios = scenario::build(&plan)?;
        Ok(Loaded { plan, cal, scenarios })
    }

    pub fn check(&self, epic: &str) -> Vec<Issue> {
        match self.load(epic) {
            Ok(l) => analysis::inspect(&l.plan, &l.scenarios),
            Err(e) => e,
        }
    }

    pub fn evaluate(&self, epic: &str) -> Result<Evaluation, Vec<Issue>> {
        let l = self.load(epic)?;
        let issues = analysis::inspect(&l.plan, &l.scenarios);
        if issues.iter().any(|i| i.level == Level::Error) { return Err(issues); }
        let mut runs = Vec::new();
        for sc in &l.scenarios {
            for &rate in &l.plan.rates {
                for &est in &l.plan.estimates {
                    runs.push(run(&l, sc, rate, est, &[]).map_err(|e| vec![e])?);
                }
            }
        }
        Ok(Evaluation {
            key: l.plan.key.clone(), stories: l.plan.stories.clone(),
            title: l.plan.title.clone(), description: l.plan.description.clone(),
            deadline: l.plan.deadline, deadline_task: l.plan.deadline_task.clone(), deadline_label: l.plan.deadline_label.clone(),
            resources: l.plan.resources.clone(),
            scenarios: l.scenarios.iter().map(|s| ScenarioInfo { key: s.key.clone(), name: s.name.clone(), note: s.note.clone() }).collect(),
            rates: l.plan.rates.clone(), estimates: l.plan.estimates.clone(), runs, issues, calendar: l.cal,
        })
    }

    /// Every epic's load (its default run), to see people who work on several epics. Each epic is scheduled on its own, as before;
    /// this only adds the loads up. An epic that cannot be evaluated comes back with its issues and no load
    pub fn epic_loads(&self) -> Result<Vec<EpicLoad>, Vec<Issue>> {
        Ok(self.plan_access.list_epics()?.into_iter().map(|e| {
            let one = || -> Result<EpicLoad, Vec<Issue>> {
                let l = self.load(&e.key)?;
                let issues = analysis::inspect(&l.plan, &l.scenarios);
                if issues.iter().any(|i| i.level == Level::Error) { return Err(issues); }
                let sc = l.scenarios.first().ok_or_else(|| vec![Issue::error("no scenario")])?;
                let rate = *l.plan.rates.first().unwrap_or(&1.0);
                let est = if l.plan.estimates.contains(&EstKind::E) { EstKind::E } else { *l.plan.estimates.first().unwrap_or(&EstKind::E) };
                let r = run(&l, sc, rate, est, &[]).map_err(|i| vec![i])?;
                Ok(EpicLoad { epic: e.key.clone(), name: l.plan.title.clone(), scenario: sc.key.clone(), rate, est, load: r.load, issues: vec![] })
            };
            one().unwrap_or_else(|is| EpicLoad { epic: e.key.clone(), name: e.name.clone(), scenario: String::new(), rate: 1.0, est: EstKind::E, load: Default::default(), issues: is })
        }).collect())
    }

    pub fn what_if(&self, epic: &str, scenario_key: &str, rate: f64, est: EstKind, changes: &[Change]) -> Result<RunResult, Vec<Issue>> {
        let l = self.load(epic)?;
        let sc = l.scenarios.iter().find(|s| s.key == scenario_key).ok_or_else(|| vec![Issue::error(format!("no scenario {scenario_key}"))])?;
        run(&l, sc, rate, est, changes).map_err(|e| vec![e])
    }

    /// Turn tried moves into edits of the plan (Save in the UI)
    pub fn changes_to_ops(&self, epic: &str, scenario_key: &str, rate: f64, est: EstKind, changes: &[Change]) -> Result<Vec<EditOp>, Vec<Issue>> {
        let l = self.load(epic)?;
        let sc = l.scenarios.iter().find(|s| s.key == scenario_key).ok_or_else(|| vec![Issue::error("no such scenario")])?;
        let tasks = estimation::resolve(&l.plan, &sc.tasks, est, rate);
        let base = l.plan.scenarios.is_empty();
        Ok(changes.iter().filter_map(|c| {
            let t = tasks.iter().find(|t| t.id == c.id)?;
            let mut patch = TaskPatch::default();
            if let Some(s) = c.min_start { patch.not_before = Some(l.cal.days[s / 2]); }
            if let Some(d) = c.dur {
                let v = Estimate::One(estimation::effort_for(t, d, rate));
                if t.kind == Kind::Wait { patch.wait = Some(v) } else { patch.effort = Some(v) }
            }
            Some(EditOp::SetTask { key: c.id.clone(), patch, scenario: if base { None } else { Some(scenario_key.to_string()) } })
        }).collect())
    }
}

/// Create and edit plans. Edits are checked with the engines; an edit with errors is not written
pub struct PlanEditingManager {
    plan_access: Arc<dyn PlanAccess + Send + Sync>,
}

impl PlanEditingManager {
    pub fn new(plan_access: Arc<dyn PlanAccess + Send + Sync>) -> Self { PlanEditingManager { plan_access } }

    pub fn epics(&self) -> Result<Vec<EpicSummary>, Vec<Issue>> { self.plan_access.list_epics() }
    pub fn plan(&self, epic: &str) -> Result<Plan, Vec<Issue>> { self.plan_access.read_plan(epic) }
    pub fn create_epic(&self, key: &str, name: &str, start: chrono::NaiveDate, actor: &str) -> Result<(), Vec<Issue>> { self.plan_access.create_epic(key, name, start, actor) }
    pub fn delete_epic(&self, key: &str, actor: &str) -> Result<(), Vec<Issue>> { self.plan_access.delete_epic(key, actor) }
    pub fn history(&self, epic: Option<&str>, limit: usize) -> Result<Vec<HistoryEntry>, Vec<Issue>> { self.plan_access.history(epic, limit) }
    pub fn version(&self) -> i64 { self.plan_access.version() }

    pub fn apply(&self, epic: &str, actor: &str, ops: &[EditOp]) -> Result<(), Vec<Issue>> {
        self.plan_access.apply(epic, actor, ops, &validate)
    }

    /// Import a plan from another store (TOML, ...)
    pub fn import(&self, from: &dyn PlanAccess, key: Option<&str>, actor: &str) -> Result<String, Vec<Issue>> {
        let mut p = from.read_plan("")?;
        if let Some(k) = key { p.key = k.to_string(); }
        let errs: Vec<Issue> = validate(&p).into_iter().filter(|i| i.level == Level::Error).collect();
        if !errs.is_empty() { return Err(errs); }
        self.plan_access.import(&p, actor)?;
        Ok(p.key)
    }
}

/// Errors in the edited plan (a scenario cannot be built, a dependency loop, a mistyped predecessor, ...)
fn validate(p: &Plan) -> Vec<Issue> {
    match scenario::build(p) {
        Ok(sc) => analysis::inspect(p, &sc).into_iter().filter(|i| i.level == Level::Error).collect(),
        Err(e) => e,
    }
}

fn run(l: &Loaded, sc: &Scenario, rate: f64, est: EstKind, changes: &[Change]) -> Result<RunResult, Issue> {
    let tasks = estimation::resolve(&l.plan, &sc.tasks, est, rate);
    let s = scheduling::schedule(&tasks, &l.cal, changes)?;
    let a = analysis::assess(&l.plan, &tasks, &s, &l.cal);
    let views = s.order.iter().map(|&i| {
        let (t, spec) = (&tasks[i], &sc.tasks[i]);
        let ch = changes.iter().find(|c| c.id == t.id);
        TaskView {
            id: t.id.clone(), story: spec.story.clone(), name: spec.name.clone(), short: spec.short.clone(), ticket: spec.ticket.clone(), deliverable: spec.deliverable.clone(),
            group: spec.group.clone(), note: spec.note.clone(), kind: t.kind, owners: t.owners.clone(), after: t.after.clone(),
            reduced_after: s.reduced_after[i].clone(), needs: t.needs.clone(), dur: ch.and_then(|c| c.dur).unwrap_or(t.dur), val: t.val, base_dur: t.dur,
            release: scheduling::release(t, &l.cal, None).0, es: s.es[i], ef: s.ef[i], tf: s.tf[i], ff: s.ff[i], color: a.colors[i], why: s.why[i].clone(),
            done: spec.done, actual_start: spec.actual_start, actual_finish: spec.actual_finish,
        }
    }).collect();
    Ok(RunResult {
        key: run_key(&sc.key, rate, est), scenario: sc.key.clone(), rate, est, tasks: views, end: s.end, target: a.target_slot,
        slack_to_deadline: a.slack_to_deadline, risk: a.risk, effort: a.effort, effort_total: a.effort_total, efficiency: a.efficiency,
        load: a.load, critical_path: a.critical_path,
    })
}
