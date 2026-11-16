// Table: numbers per task. Compare: scenarios × utilizations (for the selected estimate)
P.views.table = function (el: HTMLElement, ctx: ViewCtx) {
  const { esc, mdw, S } = P;
  const run = ctx.run;
  const rows = run.tasks.filter(t => !ctx.owner || t.owners.includes(ctx.owner)).sort((a, b) => a.es - b.es);
  let h = '<table><thead><tr><th>ID</th><th>Task</th><th>Deliverable</th><th>Ticket</th><th>Owner</th><th>Effort</th><th>Wait</th><th>After</th><th>Start</th><th>Finish</th><th>Float</th><th>Driven by</th></tr></thead><tbody>';
  for (const t of rows) {
    const v = t.dur / 2 * (t.kind === 'work' ? Number(run.rate) * Math.max(t.owners.length, 1) : 1);
    h += `<tr data-id="${esc(t.id)}" class="${ctx.sel === t.id ? 'sel' : ''}"><td class="n">${esc(t.id)}</td><td>${esc(t.name)}</td><td>${esc(t.deliverable)}</td><td class="n">${esc(t.ticket)}</td><td>${esc(t.owners.join(', '))}</td>` +
      `<td class="n">${t.kind === 'work' ? P.num(v) : ''}</td><td class="n">${t.kind === 'wait' ? P.num(v) : ''}</td><td class="n">${esc(t.after.join(', '))}</td>` +
      `<td class="n">${mdw(S.start(t.es))} ${S.half(t.es)}</td><td class="n">${t.dur ? mdw(S.end(t.ef)) + ' ' + S.endHalf(t.ef) : ''}</td>` +
      `<td class="n" style="color:var(${P.COLOR[t.color]})">${t.tf == null ? '—' : t.tf / 2}${t.tf === 0 ? ' ●' : ''}</td><td>${esc(t.why)}</td></tr>`;
  }
  el.innerHTML = h + '</tbody></table>';
  el.querySelectorAll<HTMLTableRowElement>('tbody tr').forEach(tr => tr.addEventListener('click', () => ctx.select(tr.dataset.id!)));
};

P.views.cmp = function (el: HTMLElement, ctx: ViewCtx) {
  const { esc, md, mdw, pct, S } = P;
  const ev = S.ev, dl = ev.deadline;
  let h = `<table><thead><tr><th>Scenario</th><th>Utilization</th>${dl ? `<th>Target</th><th>Slack</th>` : ''}<th>Finish</th><th>Effort</th><th>Risk</th></tr></thead><tbody>`;
  for (const r of ev.runs.filter(r => r.est === ctx.est)) {
    const cur = r.scenario === ctx.scenario && Number(r.rate) === Number(ctx.rate);
    const name = ev.scenarios.find(s => s.key === r.scenario)?.name ?? r.scenario;
    h += `<tr class="${cur ? 'cur' : ''}" data-s="${esc(r.scenario)}" data-r="${r.rate}"><td>${esc(name)}</td><td class="n">${pct(r.rate)}</td>` +
      (dl ? `<td class="n">${mdw(S.endDay(r.target))}</td><td class="n ${(r.slack_to_deadline ?? 0) < 0 ? 'late' : ''}">${(r.slack_to_deadline ?? 0) > 0 ? '+' : ''}${r.slack_to_deadline ?? ''}</td>` : '') +
      `<td class="n">${mdw(S.endDay(r.end))}</td><td class="n">${r.effort_total}</td><td class="n">${r.risk?.criticality ?? ''}</td></tr>`;
  }
  el.innerHTML = h + '</tbody></table>';
  el.querySelectorAll<HTMLTableRowElement>('tbody tr').forEach(tr => tr.addEventListener('click', () => ctx.pickRun(tr.dataset.s!, Number(tr.dataset.r))));
};
