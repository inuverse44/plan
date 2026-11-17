// Routing and state. #/ lists the epics, #/e/KEY opens one.
// Scheduling runs in WASM (Planner → PlanningManager). Edits go to the server API (PlanEditingManager) and are stored there.
// Responses of the server (plan-cli serve.rs)
type ApiRes<T = unknown> = { ok: true; data: T } | { ok: false; issues: Issue[] };
type Boot = { mode: 'live' } | { mode: 'export'; plan: Plan; wasm: string };
type ViewKey = 'gantt' | 'net' | 'load' | 'table' | 'cmp';
/** Moves tried in the UI, per task id. Kept separately for each scenario × utilization × estimate. */
type Moves = Record<string, { min_start?: number; dur?: number }>;
type Draft = { kind: 'task'; story: string } | { kind: 'story' };

(async () => {
  const { $, esc, mdw, md, pct, S } = P;
  const boot: Boot = JSON.parse($('boot').textContent!);
  const LIVE = boot.mode === 'live';
  const KEEP = 'plan-ui:v4';
  const VIEWS: [ViewKey, string][] = [['gantt', 'Gantt'], ['net', 'Network'], ['load', 'Load'], ['table', 'Table'], ['cmp', 'Compare']];
  const st = {
    level: 'task' as 'story' | 'task', settings: false, epic: null as string | null,
    s: '' , r: '', e: 'e' as EstKind, view: 'gantt' as ViewKey,
    sel: null as string | null, selStory: null as string | null, hover: null as string | null, draft: null as Draft | null,
    target: 'all' as 'all' | 'one',
    changes: {} as Record<string, Moves>, hist: {} as Record<string, string[]>, fut: {} as Record<string, string[]>, pending: null as string | null,
    prefs: {} as Record<string, { s: string; r: string; e: EstKind }>,
    /** Owner filter ('' = everyone), shared by the epic list and the epics */
    owner: '',
  };
  // null while no epic is open (nothing reads them before that; render checks ev first)
  let planner: wasm_bindgen.Planner = null!, ev: Evaluation = null!, plan: Plan = null!, version: number | null = null, curCache: { sig: string; run: RunResult } | null = null;
  try { const k = JSON.parse(localStorage.getItem(KEEP) || '{}'); st.view = k.view || 'gantt'; st.level = k.level || 'task'; st.prefs = k.prefs || {}; st.owner = k.owner || '' } catch { st.prefs = {} }
  await wasm_bindgen(boot.mode === 'live' ? { module_or_path: '/pkg/plan_web_bg.wasm' } : { module_or_path: Uint8Array.from(atob(boot.wasm), c => c.charCodeAt(0)) });

  const fmt = (r: number | string) => String(Number(r));
  const key = () => `${st.s}|${fmt(st.r)}|${st.e}`;
  const save = () => { try { if (st.epic) st.prefs[st.epic] = { s: st.s, r: st.r, e: st.e }; localStorage.setItem(KEEP, JSON.stringify({ view: st.view, level: st.level, prefs: st.prefs, owner: st.owner })) } catch { } };
  const api = async <T = unknown>(path: string, body?: unknown): Promise<ApiRes<T>> => (await fetch(path, body === undefined ? {} : { method: 'POST', body: JSON.stringify(body) })).json();
  // Other epics' loads (people across epics); see loadOthers
  let others: EpicLoad[] = [];
  // Child element that is known to exist
  const q = <T extends Element = HTMLElement>(root: ParentNode, sel: string) => root.querySelector(sel) as T;
  const issueText = (is: Issue[]) => is.map(i => (i.scenario ? `[${i.scenario}] ` : '') + (i.task ? i.task + ': ' : '') + i.message).join('\n');
  const hasScenarios = () => plan && plan.scenarios.length > 0;

  // ---------- Epic list ----------
  async function home() {
    st.epic = null; ev = null!; plan = null!;
    $('app-epic').hidden = true; $('app-help').hidden = true; $('app-home').hidden = false;
    document.title = 'Epics';
    const res: ApiRes<EpicSummary[]> = LIVE ? await api<EpicSummary[]>('/api/epics') : { ok: true, data: [] };
    const all = res.ok ? res.data : [];
    const names = [...new Set(all.flatMap(x => [...(x.owners || []), ...(x.people || [])]))].sort();
    if (st.owner && !names.includes(st.owner)) st.owner = '';
    const list = st.owner ? all.filter(x => (x.owners || []).includes(st.owner) || (x.people || []).includes(st.owner)) : all;
    const filter = names.length ? `<label class="field home-filter">Owner<select id="home-owner"><option value="">Everyone</option>${names.map(n => `<option ${n === st.owner ? 'selected' : ''}>${esc(n)}</option>`).join('')}</select></label>` : '';
    const I = (k: string) => `<svg class="ico"><use href="#i-${k}"/></svg>`;
    $('home-list').innerHTML = filter + (list.length ? `<table><thead><tr><th>Epic</th><th>Key</th><th>Owners</th><th>Start</th><th>Deadline</th><th>${I('story')}</th><th>${I('task')}</th><th>Scenarios</th></tr></thead><tbody>` +
      list.map(x => `<tr data-k="${esc(x.key)}"><td>${I('epic')} ${esc(x.name)}</td><td class="n">${esc(x.key)}</td><td>${esc((x.owners || []).join(', '))}</td><td class="n">${mdw(x.start)}</td><td class="n">${x.deadline ? mdw(x.deadline) : ''}</td><td class="n">${x.stories}</td><td class="n">${x.tasks}</td><td class="n">${x.scenarios}</td></tr>`).join('') + '</tbody></table>'
      : `<p class="empty">${all.length ? 'No epics for ' + esc(st.owner) : 'No epics yet'}</p>`);
    const ho = document.getElementById('home-owner') as HTMLSelectElement | null;
    if (ho) ho.onchange = () => { st.owner = ho.value; save(); home() };
    $('home-list').querySelectorAll<HTMLElement>('tr[data-k]').forEach(tr => tr.onclick = () => location.hash = '#/e/' + encodeURIComponent(tr.dataset.k!));
    $<HTMLInputElement>('new-start').value ||= new Date().toISOString().slice(0, 10);
  }
  $('new-epic').onsubmit = async e => {
    e.preventDefault();
    const body = { key: $<HTMLInputElement>('new-key').value.trim(), name: $<HTMLInputElement>('new-name').value.trim(), start: $<HTMLInputElement>('new-start').value };
    const res = await api('/api/epics', body);
    if (!res.ok) { P.toast(issueText(res.issues)); return }
    location.hash = '#/e/' + encodeURIComponent(body.key);
  };

  // ---------- Epic ----------
  function evaluate() {
    planner = new wasm_bindgen.Planner(JSON.stringify(plan));
    const res: { ok: true; evaluation: Evaluation } | { ok: false; issues: Issue[] } = JSON.parse(planner.evaluate());
    if (!res.ok) { $('errors').innerHTML = `<div class="err">${esc(issueText(res.issues))}</div>`; ev = null!; return false }
    $('errors').innerHTML = '';
    ev = res.evaluation; S.ev = ev; S.cal = ev.calendar;
    const warns = ev.issues.filter(i => i.level === 'warning');
    $('warn').hidden = !warns.length; $('warn').textContent = `⚠ ${warns.length}`; $('warn').title = issueText(warns);
    const sc = ev.scenarios.map(s => s.key), rates = ev.rates.map(fmt), pr: Partial<{ s: string; r: string; e: EstKind }> = (st.epic && st.prefs[st.epic]) || {};
    if (!sc.includes(st.s)) st.s = pr.s && sc.includes(pr.s) ? pr.s : sc[0];
    if (!rates.includes(fmt(st.r))) st.r = pr.r && rates.includes(fmt(pr.r)) ? pr.r : rates[0];
    if (!ev.estimates.includes(st.e)) st.e = pr.e && ev.estimates.includes(pr.e) ? pr.e : (ev.estimates.includes('e') ? 'e' : ev.estimates[0]);
    curCache = null;
    return true;
  }
  async function openEpic(k: string, keepChanges: boolean) {
    $('app-home').hidden = true; $('app-help').hidden = true; $('app-epic').hidden = false;
    if (st.epic !== k) { Object.assign(st, { epic: k, s: '', r: '', sel: null, selStory: null, changes: {}, hist: {}, fut: {} }) }
    if (boot.mode === 'live') {
      const res = await api<{ plan: Plan; version: number }>('/api/epic/' + encodeURIComponent(k));
      if (!res.ok) {
        // Deleted epic or a mistyped link: back to the list (replacing the bad address, so Back does not return to it)
        if (res.issues.some(i => i.message === `no epic ${k}`)) { history.replaceState(null, '', '#/'); home(); P.toast(`Epic ${k} was not found`); return }
        $('errors').innerHTML = `<div class="err">${esc(issueText(res.issues))}</div>`; return
      }
      plan = res.data.plan; version = res.data.version;
    } else plan = boot.plan;
    if (!keepChanges) { st.changes = {}; st.hist = {}; st.fut = {} }
    if (evaluate()) render();
    if (boot.mode === 'live') loadOthers(k);
  }

  // Other epics' loads for the Load view (people across epics). Fetched after the epic is shown; renders again when they arrive
  async function loadOthers(k: string) {
    const res = await api<EpicLoad[]>('/api/loads');
    if (!res.ok || st.epic !== k) return;
    others = res.data.filter(l => l.epic !== k && Object.keys(l.load).length);
    if (st.view === 'load') render();
  }

  // Send edits to the server (stored there), then reload
  async function edit(ops: EditOp[]): Promise<boolean> {
    if (!LIVE || !st.epic) return false;
    const res = await api('/api/epic/' + encodeURIComponent(st.epic) + '/ops', { ops });
    if (!res.ok) { P.toast(issueText(res.issues)); render(); return false }
    await openEpic(st.epic, true);
    return true;
  }
  // Edit target: all scenarios (the base tasks) or only this scenario (its overrides)
  const scen = () => (hasScenarios() && st.target === 'one' ? st.s : undefined);
  const setTask = (k: string, patch: TaskPatch) => edit([{ op: 'set_task', key: k, patch, scenario: scen() }]);

  // ---------- What-if moves ----------
  const baseRun = () => ev.runs.find(r => r.key === key())!;
  const changeList = (): Change[] => Object.entries(st.changes[key()] || {}).map(([id, v]) => ({ id, ...v }));
  function curRun(): RunResult {
    const list = changeList();
    if (!list.length) return baseRun();
    const sig = key() + JSON.stringify(list);
    if (curCache?.sig === sig) return curCache.run;
    const res: { ok: true; run: RunResult } | { ok: false; issues: Issue[] } = JSON.parse(planner.what_if(st.s, Number(st.r), st.e, JSON.stringify(list)));
    if (!res.ok) { P.toast(issueText(res.issues)); return baseRun() }
    curCache = { sig, run: res.run }; return res.run;
  }
  const snap = () => JSON.stringify(st.changes[key()] || {});
  function change(id: string, patch: { min_start?: number; dur?: number }, now: boolean) {
    const k = key(), b = baseRun().tasks.find(t => t.id === id)!;
    if (st.pending == null) st.pending = snap();
    const m = st.changes[k] || (st.changes[k] = {}), v = { ...(m[id] || {}) };
    if (patch.min_start !== undefined) { if (patch.min_start <= b.release) delete v.min_start; else v.min_start = patch.min_start }
    if (patch.dur !== undefined) { if (patch.dur === b.base_dur) delete v.dur; else v.dur = patch.dur }
    if (Object.keys(v).length) m[id] = v; else delete m[id];
    now ? commit() : schedule();
  }
  function commit() { const k = key(); if (st.pending != null && st.pending !== snap()) { (st.hist[k] ||= []).push(st.pending); st.fut[k] = [] } st.pending = null; render() }
  function undo() { const k = key(), h = st.hist[k]; if (!h?.length) return; (st.fut[k] ||= []).push(snap()); st.changes[k] = JSON.parse(h.pop()!); render() }
  function redo() { const k = key(), f = st.fut[k]; if (!f?.length) return; (st.hist[k] ||= []).push(snap()); st.changes[k] = JSON.parse(f.pop()!); render() }
  function reset() { const k = key(); if (!changeList().length) return; (st.hist[k] ||= []).push(snap()); st.changes[k] = {}; st.fut[k] = []; render() }
  async function saveChanges() {
    if (!LIVE || !st.epic || !changeList().length) return;
    const res = await api('/api/epic/' + encodeURIComponent(st.epic) + '/save', { scenario: st.s, rate: Number(st.r), est: st.e, changes: changeList() });
    if (!res.ok) { P.toast(issueText(res.issues)); return }
    P.toast('Saved'); await openEpic(st.epic, false);
  }

  // ---------- Top controls ----------
  const EST: Record<EstKind, string> = { o: 'Optimistic', m: 'Most likely', e: 'Expected (PERT)', p: 'Pessimistic' };
  function sel(name: 's' | 'r' | 'e' | 'o', label: string, items: [string, string][], cur: string, title: string) {
    return `<label class="field" title="${esc(title)}">${label}<select data-k="${name}">${items.map(([v, l]) => `<option value="${esc(v)}" ${String(v) === String(cur) ? 'selected' : ''}>${esc(l)}</option>`).join('')}</select></label>`;
  }
  // People in this epic (its owners and its tasks' owners), and the owner filter if that person is one of them
  const people = () => [...new Set([...plan.owners, ...ev.runs[0].tasks.flatMap(t => t.owners)])].sort();
  const owner = () => (st.owner && people().includes(st.owner) ? st.owner : '');
  function controls() {
    document.title = ev.title; $('title').textContent = ev.title;
    $('controls').innerHTML = (hasScenarios() ? sel('s', 'Scenario', ev.scenarios.map(s => [s.key, s.name]), st.s, 'Scenario of this epic') : '') +
      (LIVE ? `<button class="icon-btn icon-only" id="clone" title="Duplicate scenario" aria-label="Duplicate scenario"><svg viewBox="0 0 24 24"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg></button>` : '') +
      sel('r', 'Utilization', ev.rates.map(r => [fmt(r), pct(r)]), fmt(st.r), "Share of each person's day spent on this epic") +
      sel('e', 'Estimate', ev.estimates.map(e => [e, EST[e]]), st.e, 'Which estimate to use') +
      (people().length ? sel('o', 'Owner', [['', 'Everyone'], ...people().map((n): [string, string] => [n, n])], owner(), 'Show only the tasks of this person (the schedule stays as computed for everyone)') : '');
    $('controls').querySelectorAll('select').forEach(b => b.onchange = () => { const k = b.dataset.k as 's' | 'r' | 'e' | 'o'; if (k === 'e') st.e = b.value as EstKind; else if (k === 'o') st.owner = b.value; else st[k] = b.value; st.sel = null; save(); render() });
    if ($('clone')) $('clone').onclick = async () => {
      const n = (plan.scenarios.length || 0) + 1, k = 's' + n + '-' + Date.now().toString(36).slice(-3);
      const from = hasScenarios() ? st.s : undefined;
      if (await edit([{ op: 'clone_scenario', from, key: k, name: `Scenario ${n}` }])) { st.s = k; st.target = 'one'; st.settings = true; st.sel = null; save(); render(); P.toast(`Scenario ${n} created`) }
    };
    $('tabs').innerHTML = VIEWS.map(([v, l], i) => `<button role="tab" data-v="${v}" aria-pressed="${st.view === v}" title="${l} (${i + 1})">${l}</button>`).join('');
    $('tabs').querySelectorAll('button').forEach(b => b.onclick = () => { st.view = b.dataset.v as ViewKey; save(); render() });
    $('save').hidden = !LIVE; $('add-task').hidden = !LIVE; $('add-story').hidden = !LIVE; $('settings').hidden = !LIVE; $('reset').hidden = !LIVE;
  }
  function cards(base: RunResult, cur: RunResult) {
    const changed = cur !== base, was = (a: string, b: string) => changed && a !== b ? `<span class="was">${a}</span>` : '';
    let h = '';
    if (ev.deadline) {
      const t0 = mdw(S.endDay(base.target)), t1 = mdw(S.endDay(cur.target)), sl = cur.slack_to_deadline ?? 0;
      h += `<div class="card" title="${esc(ev.deadline_label || ev.deadline_task || 'Finish')}"><div class="k">Target</div><div class="v ${sl < 0 ? 'late' : 'ok'}">${was(t0, t1)}${t1}</div></div>`;
      h += `<div class="card" title="Business days to ${md(ev.deadline)}"><div class="k">Slack</div><div class="v ${sl < 0 ? 'late' : ''}">${sl > 0 ? '+' : ''}${sl}d</div></div>`;
    }
    const e0 = mdw(S.endDay(base.end)), e1 = mdw(S.endDay(cur.end));
    h += `<div class="card"><div class="k">Finish</div><div class="v">${was(e0, e1)}${e1}</div></div>`;
    h += `<div class="card" title="Person-days"><div class="k">Effort</div><div class="v">${P.num(cur.effort_total)}d</div></div>`;
    h += `<div class="card" title="Criticality risk (Fibonacci ${cur.risk?.fibonacci ?? '—'}, activity ${cur.risk?.activity ?? '—'})"><div class="k">Risk</div><div class="v">${cur.risk?.criticality ?? '—'}</div></div>`;
    $('cards').innerHTML = h;
    const k = key();
    $<HTMLButtonElement>('undo').disabled = !(st.hist[k] || []).length; $<HTMLButtonElement>('redo').disabled = !(st.fut[k] || []).length;
    $<HTMLButtonElement>('reset').disabled = !changed; $<HTMLButtonElement>('save').disabled = !changed;
  }

  // ---------- Side panel (edit a task, story or epic) ----------
  const estText = (e: Estimate | null | undefined) => e == null ? '' : Array.isArray(e) ? e.join(', ') : String(e);
  const estParse = (s: string): Estimate | null => { const xs = s.split(/[,、\s]+/).filter(Boolean).map(Number); if (!xs.length || xs.some(isNaN)) return null; return xs.length === 3 ? [xs[0], xs[1], xs[2]] : xs.length === 1 ? xs[0] : null };
  function specOf(id: string): TaskSpec {
    // The task as seen in the selected scenario (base + overrides), for display
    const t: TaskSpec & Record<string, unknown> = { ...(plan.tasks.find(x => x.id === id) || { id, name: id }) };
    const sc = plan.scenarios.find(s => s.key === st.s);
    const add = sc?.add?.find(x => x.id === id); if (add) return { ...add };
    const p = sc?.set?.find(([k]) => k === id)?.[1];
    if (p) { for (const [k, v] of Object.entries(p)) if (k !== 'clear' && v != null) t[k === 'owners' ? 'owners' : k] = v; if (p.effort != null) delete t.wait; if (p.wait != null) delete t.effort }
    return t;
  }
  function side(cur: RunResult): void {
    const el = $('side');
    if (st.draft) return draftPanel(el);
    if (st.selStory) return storyPanel(el);
    const t = st.sel ? cur.tasks.find(x => x.id === st.sel) : undefined;
    if (!t) { el.hidden = !(LIVE && st.settings); if (LIVE && st.settings) epicPanel(el); return }
    el.hidden = false;
    const sp = specOf(t.id), su = P.succOf(cur)[t.id] || [];
    const ro = !LIVE;
    const inp = (f: string, v: unknown, ph = '') => `<input data-f="${f}" value="${esc(v ?? '')}" placeholder="${esc(ph)}" ${ro ? 'disabled' : ''}>`;
    const stories: [string, string][] = [['', '—'], ...plan.stories.map((s): [string, string] => [s.key, s.name])];
    // Predecessor candidates: exclude itself, current predecessors and its successors (linking would make a loop)
    const desc = descend(cur, t.id), others = cur.tasks.filter(x => x.id !== t.id && !t.after.includes(x.id) && !desc.has(x.id));
    el.innerHTML = `<button class="close" aria-label="Close (Esc)">×</button>
      ${hasScenarios() && LIVE ? `<div class="seg small" style="margin-bottom:10px">${[['all', 'All scenarios'], ['one', 'This scenario']].map(([v, l]) => `<button data-target="${v}" aria-pressed="${st.target === v}">${l}</button>`).join('')}</div>` : ''}
      <div class="form">
        <label>Name${inp('name', sp.name)}</label>
        <label>Short${inp('short', sp.short, sp.name)}</label>
        <label>Story<select data-f="story" ${ro ? 'disabled' : ''}>${stories.map(([k, n]) => `<option value="${esc(k)}" ${k === (sp.story || '') ? 'selected' : ''}>${esc(n)}</option>`).join('')}</select></label>
        <label>Deliverable${inp('deliverable', sp.deliverable)}</label>
        <div class="row2"><label>Ticket${inp('ticket', sp.ticket)}</label><label>Owner${inp('owners', (sp.owners || []).join(', '), plan.default_owner || '')}</label></div>
        <label>Needs${inp('needs', (sp.needs || []).join(', '), Object.keys(plan.resources || {}).join(', ') || 'prod')}</label>
        <div class="row2"><label>${sp.wait != null ? 'Wait (days)' : 'Effort (person-days)'}${inp(sp.wait != null ? 'wait' : 'effort', estText(sp.wait ?? sp.effort), '1 or 1, 2, 4')}</label>
          <label>Not before<input type="date" data-f="not_before" value="${sp.not_before || ''}" ${ro ? 'disabled' : ''}></label></div>
        <div class="checks"><label><input type="checkbox" data-f="waitkind" ${sp.wait != null ? 'checked' : ''} ${ro ? 'disabled' : ''}>Wait</label>
          <label><input type="checkbox" data-f="done" ${sp.done ? 'checked' : ''} ${ro ? 'disabled' : ''}>Done</label></div>
        <div class="row2"><label>Actual start<input type="date" data-f="actual_start" value="${sp.actual_start || ''}" ${ro ? 'disabled' : ''}></label>
          <label>Actual finish<input type="date" data-f="actual_finish" value="${sp.actual_finish || ''}" ${ro ? 'disabled' : ''}></label></div>
      </div>
      <dl><dt>Start</dt><dd>${mdw(S.start(t.es))} ${S.half(t.es)}</dd><dt>Finish</dt><dd>${t.dur ? mdw(S.end(t.ef)) + ' ' + S.endHalf(t.ef) : '—'}</dd>
      <dt>Float</dt><dd style="color:var(${P.COLOR[t.color]})">${t.tf == null ? '—' : t.tf / 2 + 'd'}${t.ff != null ? ` (free ${t.ff / 2}d)` : ''}</dd><dt>Driven by</dt><dd>${esc(t.why)}</dd>
      <dt>After</dt><dd><div class="links">${t.after.map(i => `<span class="chip-x"><button data-go="${esc(i)}">${esc(i)}</button>${LIVE ? `<button class="x" data-unlink="${esc(i)}" aria-label="Remove ${esc(i)}">×</button>` : ''}</span>`).join('')}
        ${LIVE ? `<select id="add-pred"><option value="">+ add</option>${others.map(x => `<option value="${esc(x.id)}">${esc(x.id)} ${esc(x.short || x.name)}</option>`).join('')}</select>` : ''}</div></dd>
      <dt>Before</dt><dd><div class="links">${su.map(i => `<button data-go="${esc(i)}">${esc(i)}</button>`).join('') || '—'}</div></dd></dl>
      ${notesHtml(t.id)}
      ${LIVE ? `<button class="danger" id="del-task">Delete task</button>` : ''}`;
    q(el, '.close').onclick = () => { st.sel = null; render() };
    notesWire(el, t.id);
    el.querySelectorAll<HTMLElement>('[data-go]').forEach(b => b.onclick = () => { st.sel = b.dataset.go!; render() });
    el.querySelectorAll<HTMLElement>('[data-target]').forEach(b => b.onclick = () => { st.target = b.dataset.target as 'all' | 'one'; render() });
    if (ro) return;
    el.querySelectorAll<HTMLElement>('[data-unlink]').forEach(b => b.onclick = () => setTask(t.id, { after: t.after.filter(x => x !== b.dataset.unlink) }));
    const addPred = $<HTMLSelectElement>('add-pred'); addPred.onchange = () => addPred.value && setTask(t.id, { after: [...t.after, addPred.value] });
    $('del-task').onclick = async () => { if (await edit([{ op: 'delete_task', key: t.id }])) { st.sel = null; P.toast(`Deleted ${t.id}`) } };
    el.querySelectorAll<HTMLInputElement>('[data-f]').forEach(inpEl => inpEl.onchange = () => {
      const f = inpEl.dataset.f!, v = inpEl.type === 'checkbox' ? inpEl.checked : inpEl.value.trim();
      if (f === 'waitkind') { const cur = estParse(estText(sp.wait ?? sp.effort)) ?? 1; return setTask(t.id, v ? { wait: cur, clear: ['effort'] } : { effort: cur, clear: ['wait'] }) }
      // Done without a finish date: finished today (the viewer's local day)
      if (f === 'done' && v === true && !sp.actual_finish) return setTask(t.id, { done: true, actual_finish: P.iso(new Date()) });
      if (typeof v === 'boolean') return setTask(t.id, { [f]: v });
      if (f === 'effort' || f === 'wait') { const x = estParse(v); if (x == null) { P.toast('Enter 1 number or 3 (optimistic, likely, pessimistic)'); return render() } return setTask(t.id, { [f]: x }) }
      if (f === 'needs') return setTask(t.id, { needs: v ? String(v).split(/[,、\s]+/).filter(Boolean) : [] });
      if (f === 'owners') return setTask(t.id, v ? { owners: v.split(/[,、\s]+/).filter(Boolean) } : { clear: ['owners'] });
      if (f === 'not_before' || f === 'actual_start' || f === 'actual_finish') return setTask(t.id, v ? { [f]: v } : { clear: [f] });
      setTask(t.id, { [f]: v });
    });
  }
  function descend(run: RunResult, id: string) { const su = P.succOf(run), s = new Set<string>(); const dn = (k: string) => { for (const x of su[k] || []) if (!s.has(x)) { s.add(x); dn(x) } }; dn(id); return s }
  function storyPanel(el: HTMLElement): void {
    const s = plan.stories.find(x => x.key === st.selStory); if (!s) { st.selStory = null; return side(curRun()) }
    el.hidden = false;
    const inside = plan.tasks.filter(t => t.story === s.key);
    el.innerHTML = `<button class="close" aria-label="Close">×</button><div class="sub"><svg class="ico"><use href="#i-story"/></svg> ${esc(s.key)}</div>
      <div class="form"><label>Name<input data-f="name" value="${esc(s.name)}"></label><label>Goal<input data-f="purpose" value="${esc(s.purpose)}"></label><label>Done when<input data-f="dod" value="${esc(s.dod)}"></label></div>
      ${notesHtml(s.key)}
      <button class="danger" id="del-story">${inside.length ? `Delete story + ${inside.length} task${inside.length > 1 ? 's' : ''}` : 'Delete story'}</button>`;
    q(el, '.close').onclick = () => { st.selStory = null; render() };
    notesWire(el, s.key);
    el.querySelectorAll<HTMLInputElement>('[data-f]').forEach(i => i.onchange = () => edit([{ op: 'set_story', story: { ...s, [i.dataset.f!]: i.value.trim() } }]));
    // Delete the story's tasks too (otherwise they would be left without a story)
    $('del-story').onclick = async () => { if (await edit([...inside.map((t): EditOp => ({ op: 'delete_task', key: t.id })), { op: 'delete_story', key: s.key }])) { st.selStory = null; P.toast(`Deleted ${s.key}${inside.length ? ` and ${inside.length} tasks` : ''}`) } };
  }
  function epicPanel(el: HTMLElement) {
    const sc = plan.scenarios.find(x => x.key === st.s);
    el.innerHTML = `<button class="close" aria-label="Close">×</button><div class="sub"><svg class="ico"><use href="#i-epic"/></svg> ${esc(plan.key)}</div><div class="form">
      <label>Name<input data-e="name" value="${esc(plan.title)}"></label>
      <label>Owners<input data-e="owners" value="${esc(plan.owners.join(', '))}" placeholder="alice, bob"></label>
      <div class="row2"><label>Start<input type="date" data-e="start" value="${plan.start}"></label><label>Deadline<input type="date" data-e="deadline" value="${plan.deadline || ''}"></label></div>
      <div class="row2"><label>Target task<select data-e="deadline_task"><option value="">Finish</option>${plan.tasks.map(t => `<option value="${esc(t.id)}" ${plan.deadline_task === t.id ? 'selected' : ''}>${esc(t.short || t.name)}</option>`).join('')}</select></label>
        <label>Default owner<input data-e="default_owner" value="${esc(plan.default_owner || '')}"></label></div>
      <label title="Tasks larger than this get a warning to split them. Empty: 2; 0: no warning">Max task size (person-days)<input type="number" min="0" step="0.5" data-e="max_task_days" value="${plan.max_task_days ?? ''}" placeholder="2"></label>
      ${sc ? `<label>Scenario name<input data-sc="name" value="${esc(sc.name)}"></label>` : ''}
    </div>${sc ? `<button class="danger" id="del-sc">Delete scenario</button>` : ''}${LIVE ? `<button class="danger" id="del-epic">Delete epic</button>` : ''}`;
    q(el, '.close').onclick = () => { st.settings = false; render() };
    el.querySelectorAll<HTMLInputElement>('[data-e]').forEach(i => i.onchange = () => {
      const f = i.dataset.e!, v = i.value.trim(), s: Record<string, string | null> = {};
      if (f === 'owners') return edit([{ op: 'set_epic', settings: { owners: v.split(/[,、\s]+/).filter(Boolean) } }]);
      if (f === 'max_task_days') return edit([{ op: 'set_epic', settings: { max_task_days: v === '' ? null : Number(v) } }]);
      if (f === 'deadline' || f === 'deadline_task' || f === 'default_owner') s[f] = v || null; else s[f] = v;
      edit([{ op: 'set_epic', settings: s as EpicSettings }]);
    });
    el.querySelectorAll<HTMLInputElement>('[data-sc]').forEach(i => i.onchange = () => edit([{ op: 'set_scenario', key: sc!.key, name: i.value.trim() }]));
    if (sc) $('del-sc').onclick = () => edit([{ op: 'delete_scenario', key: sc.key }]);
    // Delete the whole epic: a second click within a few seconds confirms (no browser dialog)
    if (LIVE) {
      const b = $('del-epic'); let armed = 0;
      b.onclick = async () => {
        if (!armed) { b.textContent = `Click again to delete ${plan.key}`; armed = setTimeout(() => { armed = 0; b.textContent = 'Delete epic' }, 4000); return }
        clearTimeout(armed);
        const k = plan.key, res = await api('/api/epic/' + encodeURIComponent(k) + '/delete', {});
        if (!res.ok) { P.toast(issueText(res.issues)); return }
        st.settings = false; location.hash = '#/'; P.toast(`Deleted ${k} (its content stays in the log)`);
      };
    }
  }

  // ---------- Render ----------
  let raf = 0;
  function schedule() { if (!raf) raf = requestAnimationFrame(() => { raf = 0; render() }) }
  function render() {
    if (!ev) return;
    controls();
    const base = baseRun(), cur = curRun();
    cards(base, cur); side(cur);
    const ctx: ViewCtx = {
      level: st.level, setLevel: v => { st.level = v; save(); render() },
      run: cur, base, plan, sel: st.sel, selStory: st.selStory, focus: st.hover || st.sel, scenario: st.s, rate: Number(st.r), est: st.e, editable: LIVE,
      hover: id => { if (st.hover !== id) { st.hover = id; schedule() } },
      select: id => { st.draft = null; st.selStory = null; st.sel = st.sel === id ? null : id; render() },
      selectStory: k => { st.draft = null; st.sel = null; st.selStory = st.selStory === k ? null : k; render() },
      link: (from, to) => { const t = cur.tasks.find(x => x.id === to); if (t && !t.after.includes(from)) setTask(to, { after: [...t.after, from] }) },
      change, commit, current: id => curRun().tasks.find(t => t.id === id),
      pickRun: (s, r) => { st.s = s; st.r = fmt(r); st.view = 'gantt'; save(); render() },
      dragged: false, others, owner: owner() || null,
    };
    (P.views[st.view] || P.views.gantt)($('view'), ctx);
  }

  // ---------- Add ----------
  $('add-task').onclick = () => { st.draft = { kind: 'task', story: st.selStory || (st.sel ? specOf(st.sel).story : '') || '' }; st.settings = false; render() };
  $('add-story').onclick = () => { st.draft = { kind: 'story' }; st.settings = false; render() };
  function draftPanel(el: HTMLElement) {
    const d = st.draft!, task = d.kind === 'task';
    el.hidden = false;
    el.innerHTML = `<button class="close" aria-label="Cancel (Esc)">×</button><div class="sub"><svg class="ico"><use href="#i-${d.kind}"/></svg> New ${d.kind}</div>
      <form class="form" id="draft">
        <label>Name<input data-d="name" required></label>
        <label>Key<input data-d="key" placeholder="${task ? 'T1' : 'S1'}"></label>
        ${task ? `<label>Story<select data-d="story"><option value="">—</option>${plan.stories.map(s => `<option value="${esc(s.key)}" ${d.kind === 'task' && s.key === d.story ? 'selected' : ''}>${esc(s.name)}</option>`).join('')}</select></label>
        <label>Effort (person-days)<input data-d="effort" value="1" placeholder="1 or 1, 2, 4"></label>` : ''}
        <button class="primary" type="submit">Add ${d.kind}</button>
      </form>`;
    const cancel = () => { st.draft = null; render() };
    q(el, '.close').onclick = cancel;
    const f = q<HTMLFormElement>(el, '#draft'), v = (n: string) => f.querySelector<HTMLInputElement>(`[data-d=${n}]`)?.value.trim() || '';
    f.onkeydown = e => { if (e.key === 'Escape') cancel() };
    f.onsubmit = async e => {
      e.preventDefault();
      const name = v('name'); if (!name) return;
      const used = task ? plan.tasks.map(t => t.id) : plan.stories.map(s => s.key), pre = task ? 'T' : 'S';
      let k = v('key'); if (!k) { let n = used.length + 1; while (used.includes(pre + n)) n++; k = pre + n }
      if (used.includes(k)) { P.toast(`${k} already exists`); return }
      let op: EditOp;
      if (task) { const ef = estParse(v('effort') || '1'); if (ef == null) { P.toast('Enter 1 number or 3 (optimistic, likely, pessimistic)'); return }
        op = { op: 'add_task', task: { id: k, name, story: v('story'), effort: ef } } }
      else op = { op: 'set_story', story: { key: k, name, purpose: '', dod: '' } };
      st.draft = null;
      if (await edit([op])) { if (task) { st.sel = k; st.selStory = null } else { st.selStory = k; st.sel = null } render() }
    };
    setTimeout(() => q(f, '[data-d=name]').focus(), 0);
  }

  // ---------- Comments (e.g. links to the source of truth) ----------
  // Turn URLs into links; show long URLs as host + short path
  const linkify = (s: string) => esc(s).replace(/https?:\/\/[^\s<]+/g, u => { let host = u; try { const x = new URL(u.replace(/&amp;/g, '&')); host = x.host + (x.pathname.length > 1 ? x.pathname.slice(0, 28) + (x.pathname.length > 28 ? '…' : '') : '') } catch { } return `<a href="${u}" target="_blank" rel="noopener" title="${u}">${esc(host)}</a>` });
  function notesHtml(target: string) {
    const cs = (plan.comments || []).filter(c => c.target === target);
    return `<div class="notes"><div class="notes-h">Comments${cs.length ? ` <span class="count">${cs.length}</span>` : ''}</div>
      ${cs.map(c => `<div class="note"><div class="note-meta">${esc(c.at.slice(0, 16))}${c.by && c.by !== 'web' ? ` · ${esc(c.by)}` : ''}${LIVE ? `<button class="x" data-del-note="${c.id}" aria-label="Delete comment">×</button>` : ''}</div><div class="note-text">${linkify(c.text)}</div></div>`).join('')}
      ${LIVE ? `<textarea class="note-in" rows="2" placeholder="Link or note (Enter to add, Shift+Enter for new line)"></textarea>` : ''}</div>`;
  }
  function notesWire(el: HTMLElement, target: string) {
    if (!LIVE) return;
    el.querySelectorAll<HTMLElement>('[data-del-note]').forEach(b => b.onclick = () => edit([{ op: 'delete_comment', id: Number(b.dataset.delNote) }]));
    const ta = q<HTMLTextAreaElement>(el, '.note-in');
    ta.onkeydown = e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); const t = ta.value.trim(); if (t) edit([{ op: 'add_comment', target, text: t }]) } };
  }

  // ---------- Keyboard ----------
  addEventListener('keydown', e => {
    if ((e.target as Element).closest('input,textarea,select') || !ev || !st.epic) return;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); e.shiftKey ? redo() : undo(); return }
    if (mod && e.key.toLowerCase() === 's') { e.preventDefault(); saveChanges(); return }
    if (mod) return;
    const n = Number(e.key); if (n >= 1 && n <= VIEWS.length) { st.view = VIEWS[n - 1][0]; save(); render(); return }
    if (e.key === 'Escape') { st.sel = null; st.selStory = null; st.draft = null; render(); return }
    if (e.key === 'n' && LIVE) { $('add-task').click(); return }
    const cur = curRun(), ids = [...cur.tasks].sort((a, b) => a.es - b.es).map(t => t.id);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const i = st.sel == null ? -1 : ids.indexOf(st.sel), j = st.sel == null ? 0 : Math.max(0, Math.min(ids.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)));
      st.sel = ids[j]; st.selStory = null; render(); return;
    }
    if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && st.sel) {
      e.preventDefault();
      const t = cur.tasks.find(x => x.id === st.sel); if (!t || t.kind === 'done') return;
      const d = e.key === 'ArrowRight' ? 1 : -1;
      if (e.shiftKey) change(t.id, { dur: Math.max(1, t.dur + d) }, true); else change(t.id, { min_start: Math.max(S.cal.start_slot, t.es + d) }, true);
    }
  });
  $('undo').onclick = undo; $('redo').onclick = redo; $('reset').onclick = reset; $('save').onclick = saveChanges;
  // Light/dark toggle (remembered; follows the system until chosen)
  const isDark = () => document.documentElement.dataset.theme ? document.documentElement.dataset.theme === 'dark' : matchMedia('(prefers-color-scheme: dark)').matches;
  document.querySelectorAll<HTMLElement>('.theme').forEach(b => b.onclick = () => {
    const t = isDark() ? 'light' : 'dark';
    document.documentElement.dataset.theme = t;
    try { localStorage.setItem('plan-theme', t) } catch { }
    render();
  });
  $('settings').onclick = () => { st.settings = !st.settings; st.sel = null; st.selStory = null; render() };
  let rz = 0; addEventListener('resize', () => { clearTimeout(rz); rz = setTimeout(render, 150) });
  matchMedia('(prefers-color-scheme: dark)').addEventListener('change', render);

  // ---------- Routing and picking up changes made elsewhere (CLI, AI) ----------
  function help() {
    $('app-home').hidden = true; $('app-epic').hidden = true; $('app-help').hidden = false; document.title = 'Help';
    $('app-help').innerHTML = HELP;
    renderMath($('app-help'));
  }
  function route() {
    if (location.hash === '#/help') return help();
    const m = location.hash.match(/^#\/e\/(.+)$/);
    if (m) openEpic(decodeURIComponent(m[1]), false); else if (LIVE) home(); else openEpic(boot.plan.key, false);
  }
  addEventListener('hashchange', route);
  if (LIVE) setInterval(async () => {
    try {
      const v: number = (await (await fetch('/api/version')).json()).version;
      if (version != null && v !== version) { version = v; if (st.epic) { await openEpic(st.epic, true); P.toast('Reloaded changes from CLI') } else home() }
      version = v;
    } catch { }
  }, 2000);
  if (!LIVE) { $('back').removeAttribute('href'); $('back').title = 'Plan'; $('back').setAttribute('aria-label', 'Plan') }
  route();
})();

// Help page (kept off the main screen; opened from ?). Formulas are TeX rendered with KaTeX
var HELP = String.raw`<p><a href="javascript:history.back()">← Back</a></p>
<h1>Help</h1>
<h2>Units</h2>
<p><svg class="ico"><use href="#i-epic"/></svg> <b>Epic</b> — one plan, scheduled together. <svg class="ico"><use href="#i-story"/></svg> <b>Story</b> — a group of tasks; its bracket spans its tasks. <svg class="ico"><use href="#i-task"/></svg> <b>Task</b> (subtask) — the unit that is scheduled. Only tasks are scheduled; stories and the epic are containers.</p>

<h2>Duration</h2>
<p>Time is counted in business half-days (weekends, Japanese public holidays, Dec 29 – Jan 3 and listed days off are skipped [8]). A task with effort \(E\) person-days, \(n\) owners and utilization \(u\) takes</p>
$$d = \frac{1}{2}\left\lceil \frac{2E}{u\,n} \right\rceil \ \text{business days.}$$
<p>A wait takes its days as written and uses no one's hands. One person does one task at a time.</p>

<h2>Estimate</h2>
<p>With optimistic \(o\), most likely \(m\) and pessimistic \(p\), the expected (PERT) estimate is [1, p.162; 2]</p>
$$E = \operatorname{round}_{0.5}\!\left(\frac{o + 4m + p}{6}\right).$$

<h2>Order and float</h2>
<p>Tasks are placed in order of least float first (latest start of the resource-free critical-path pass [3]), filling gaps where they fit. The total float of task \(i\) is how far it can slip without moving the finish \(T\) [1, p.200]; here it is measured with owners' availability included:</p>
$$F_i = \max\{\delta \ge 0 : T(\text{task } i \text{ delayed by } \delta) = T\}.$$
<p>Without resource limits this equals \(LS_i - ES_i\) [3]. Colors: critical \(F_i = 0\) (orange), then low \((0, 2]\), mid \((2, 5]\), high \(> 5\) days in blue from dark to light.</p>

<h2>Cards</h2>
<p><b>Target</b> — finish of the target task (⚙). <b>Slack</b> — business days from Target to the deadline; negative means late. <b>Finish</b> — end of the epic. <b>Effort</b> — total person-days.</p>
<p><b>Risk</b> is criticality risk, with \(N\) activities of which \(N_c, N_r, N_y, N_g\) are critical, low, mid and high float [1, p.242]:</p>
$$R_c = \frac{4N_c + 3N_r + 2N_y + N_g}{4N}.$$
<p>The tooltip also shows Fibonacci risk [1, p.244] and activity risk with the largest float \(M\) [1, p.246]:</p>
$$R_f = \frac{4.24N_c + 2.62N_r + 1.62N_y + N_g}{4.24N}, \qquad R_a = 1 - \frac{\sum_i F_i}{M\,N}.$$
<p>Values between 0.3 and 0.75 are healthy; above 0.75 the plan is fragile, below 0.3 it is over-decompressed [1, p.253].</p>

<h2>Colors</h2>
<p>Float uses one hue (blue) from dark to light plus orange for critical, so it reads under red–green color vision deficiency (about 1 in 20 men in Japan [6]). Borders reach 3:1 against their surface [4]; dark-mode text is off-white to avoid glare [5]. Palettes follow [7].</p>

<h2>Keys</h2>
<p><kbd>1</kbd>–<kbd>5</kbd> views · <kbd>↑</kbd><kbd>↓</kbd> select task · <kbd>←</kbd><kbd>→</kbd> move start by half a day · <kbd>Shift</kbd>+<kbd>←</kbd><kbd>→</kbd> change duration · <kbd>N</kbd> new task · <kbd>⌘Z</kbd> undo · <kbd>⇧⌘Z</kbd> redo · <kbd>⌘S</kbd> save moves · <kbd>Esc</kbd> close</p>

<h2>Editing</h2>
<p>Click a bar to edit it on the right; changes save at once. Drag a bar to try a new start, drag its right edge to try a new duration, then <b>Save</b>. To link tasks, drag from the circle at the right of the selected bar onto the task that should follow. Links that would make a loop are refused. With several scenarios, choose whether an edit applies to <b>All scenarios</b> or <b>This scenario</b>.</p>

<h2>For AI and the terminal</h2>
<p>Every change is logged with who made it. <code>plan epics --json</code> · <code>plan dump KEY</code> · <code>plan log KEY --json</code> · <code>plan show KEY --json</code> · <code>plan apply KEY ops.json --json</code>. See the README.</p>

<h2>References</h2>
<ol class="refs">
<li>J. Löwy, <i>Righting Software</i>. Addison-Wesley, 2019. Ch. 7 (PERT, p.162), Ch. 8 (total float, p.200), Ch. 10 (risk, pp.242–253).</li>
<li>D. G. Malcolm, J. H. Roseboom, C. E. Clark, W. Fazar, “Application of a technique for research and development program evaluation,” <i>Operations Research</i>, 7(5), 646–669, 1959. <a href="https://doi.org/10.1287/opre.7.5.646">doi:10.1287/opre.7.5.646</a></li>
<li>J. E. Kelley Jr., M. R. Walker, “Critical-path planning and scheduling,” <i>Proc. Eastern Joint Computer Conference</i>, Boston, 1959, p. 160ff.</li>
<li>W3C, <a href="https://www.w3.org/TR/WCAG21/#non-text-contrast"><i>Web Content Accessibility Guidelines 2.1</i></a>, SC 1.4.3 Contrast (Minimum) and SC 1.4.11 Non-text Contrast, 2018.</li>
<li>Google, <a href="https://m2.material.io/design/color/dark-theme"><i>Material Design: Dark theme</i></a>.</li>
<li>Tottori Prefecture, <a href="https://www.pref.tottori.lg.jp/item/1100584.htm">カラーユニバーサルデザイン</a> (Color universal design, in Japanese).</li>
<li>nextlevelbuilder, <a href="https://github.com/nextlevelbuilder/ui-ux-pro-max-skill"><i>ui-ux-pro-max-skill</i></a> (MIT), colors.csv No. 16 Productivity Tool and No. 81 Developer Tool.</li>
<li>国民の祝日に関する法律（昭和 23 年法律第 178 号） (Act on National Holidays, Act No. 178 of 1948, in Japanese). Equinox days are computed with the usual approximation valid for 1980–2099.</li>
</ol>`;

// Load KaTeX only when Help is opened
declare function renderMathInElement(el: HTMLElement, opts: object): void;
function renderMath(el: HTMLElement) {
  const go = () => renderMathInElement(el, { delimiters: [{ left: '$$', right: '$$', display: true }, { left: '\\(', right: '\\)', display: false }], throwOnError: false });
  if ('renderMathInElement' in window) return go();
  const base = 'https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/';
  const css = document.createElement('link'); css.rel = 'stylesheet'; css.href = base + 'katex.min.css'; document.head.appendChild(css);
  const s1 = document.createElement('script'); s1.src = base + 'katex.min.js';
  s1.onload = () => { const s2 = document.createElement('script'); s2.src = base + 'contrib/auto-render.min.js'; s2.onload = go; document.head.appendChild(s2) };
  document.head.appendChild(s1);
}
