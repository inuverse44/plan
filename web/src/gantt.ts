// Shared by both Gantt levels (stories, tasks): date scale, weekends, resources' off periods, today, deadline
function ganttFrame(el: HTMLElement, ctx: ViewCtx, nrows: number) {
  const { css, D, iso, mdw, S } = P;
  const { run, base } = ctx, cal = S.cal;
  const acts = run.tasks.map(t => t.actual_start).filter((x): x is string => !!x).sort();
  const t0 = D(acts.length && acts[0] < cal.days[cal.start_slot >> 1] ? acts[0] : cal.days[cal.start_slot >> 1]);
  let last = S.end(Math.max(run.end, base.end)); if (S.ev.deadline && S.ev.deadline > last) last = S.ev.deadline;
  const nd = Math.round((D(last).getTime() - t0.getTime()) / 864e5) + 6;
  // ow: x of the owner column inside the label area. On narrow screens (phones) the label area shrinks and the owner column goes (owners move into the task tooltip; the side panel shows them too)
  const narrow = el.clientWidth < 600;
  const lw = narrow ? 170 : 300, ow = narrow ? -1 : 228, rh = 32, top = 30;
  const avail = Math.max(narrow ? 360 : 640, el.clientWidth - 26);
  const dw = Math.max(14, Math.min(36, (avail - lw) / nd));
  const W = lw + nd * dw + 30, H = top + nrows * rh + 8;
  const xd = (s: string) => lw + Math.round((D(s).getTime() - t0.getTime()) / 864e5) * dw;
  const xs = (s: number) => xd(S.start(s)) + (s % 2 ? dw / 2 : 0);
  const xe = (f: number) => xd(S.end(f)) + ((f - 1) % 2 ? dw : dw / 2);
  const off = new Set(cal.off);
  let head = `<svg id="gsvg" xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="Gantt">
  <defs><pattern id="hatch" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="6" height="6" fill="${css('--wait')}" opacity=".25"/><line x1="0" y1="0" x2="0" y2="6" stroke="${css('--wait')}" stroke-width="3"/></pattern>
  <marker id="ga" viewBox="0 0 8 8" refX="7" refY="4" markerUnits="userSpaceOnUse" markerWidth="8" markerHeight="8" orient="auto"><path d="M0 0L8 4L0 8z" fill="${css('--muted')}"/></marker>
  <marker id="gh" viewBox="0 0 8 8" refX="7" refY="4" markerUnits="userSpaceOnUse" markerWidth="8" markerHeight="8" orient="auto"><path d="M0 0L8 4L0 8z" fill="${css('--accent')}"/></marker></defs>`;
  for (const [n, r] of Object.entries(S.ev.resources)) for (const [a, b] of r.off) { const x1 = Math.max(lw, xd(a)), x2 = Math.min(W, xd(b) + dw); if (x2 > x1) head += `<rect x="${x1}" y="${top - 4}" width="${x2 - x1}" height="${H - top}" fill="${css('--freeze')}"><title>${P.esc(r.label || n)}: ${P.esc(n)} unavailable</title></rect>` }
  if (ow >= 0) head += `<text x="${ow}" y="${top - 12}" font-size="11" style="fill:${css('--muted')}">Owner</text>`;
  for (let i = 0; i < nd; i++) {
    const d = new Date(t0.getTime() + i * 864e5), w = d.getDay(), x = lw + i * dw;
    if (w === 0 || w === 6 || off.has(iso(d))) head += `<rect x="${x}" y="${top - 4}" width="${dw}" height="${H - top}" fill="${css('--faint')}"/>`;
    if (w === 1) head += `<line x1="${x}" y1="${top - 8}" x2="${x}" y2="${H}" stroke="${css('--line')}"/><text x="${x + 3}" y="${top - 12}" font-size="11" style="fill:${css('--muted')}">${P.md(iso(d))}</text>`;
  }
  const today = iso(new Date());
  // Today: its column shaded, a line through its middle, and a wider invisible line that carries the tooltip
  if (today >= cal.days[0] && today <= last) {
    const x = xd(today), m = x + dw / 2, tip = `<title>Today ${mdw(today)}</title>`;
    head += `<rect x="${x}" y="${top - 4}" width="${dw}" height="${H - top + 4}" fill="${css('--accent')}" opacity=".08">${tip}</rect>`
      + `<line x1="${m}" y1="${top - 4}" x2="${m}" y2="${H}" stroke="${css('--accent')}" stroke-width="1.5"/>`
      + `<line x1="${m}" y1="${top - 4}" x2="${m}" y2="${H}" stroke="transparent" stroke-width="8">${tip}</line>`;
  }
  if (S.ev.deadline) { const x = xd(S.ev.deadline) + dw; head += `<line x1="${x}" y1="${top - 8}" x2="${x}" y2="${H}" stroke="${css('--red')}" stroke-width="2" stroke-dasharray="5 3"><title>Deadline ${mdw(S.ev.deadline)}</title></line>` }
  // Bar extent: actual dates for done tasks that have them, otherwise the computed half-days
  const bx = (t: TaskView): [number, number] => t.kind === 'done' && t.actual_start ? [xd(t.actual_start), xd(t.actual_finish || t.actual_start) + dw] : [xs(t.es), xe(t.ef > t.es ? t.ef : t.es + 1)];
  return { t0, nd, lw, ow, rh, top, dw, W, H, xd, xs, xe, bx, last, head };
}

// Owner column: names joined, shortened to fit; the full list in the tooltip
function ownerCell(owners: string[], x: number, y: number) {
  if (!owners.length || x < 0) return '';
  const o = owners.join(', ');
  return `<text x="${x}" y="${y + 21}" font-size="12" style="fill:${P.css('--muted')}">${P.esc(o.length > 6 ? o.slice(0, 5) + '…' : o)}<title>${P.esc(o)}</title></text>`;
}

// Stories / Tasks switch (top left of the Gantt)
const ganttLevel = (ctx: ViewCtx) => `<div class="seg small" style="margin-bottom:8px" role="group" aria-label="Gantt level">${[['story', 'Stories'], ['task', 'Tasks']].map(([v, l]) => `<button data-level="${v}" aria-pressed="${ctx.level === v}">${l}</button>`).join('')}</div>`;
const wireLevel = (el: HTMLElement, ctx: ViewCtx) => el.querySelectorAll<HTMLElement>('[data-level]').forEach(b => b.onclick = () => ctx.setLevel(b.dataset.level as 'story' | 'task'));

// Story level: one row per story, spanning its earliest start to latest finish, colored by its least-float task
P.views.ganttStories = function (el: HTMLElement, ctx: ViewCtx) {
  const { css, esc, mdw, S } = P;
  const { run, base } = ctx;
  const stories = ctx.plan?.stories || [];
  const known = new Set(stories.map(x => x.key));
  const groupOf = (t: TaskView) => known.has(t.story) ? t.story : '';
  const mine = (t: TaskView) => !ctx.owner || t.owners.includes(ctx.owner);
  let groups: { key: string; name: string }[] = stories.map(x => ({ key: x.key, name: x.name }));
  if (run.tasks.some(t => !known.has(t.story))) groups.push({ key: '', name: 'No story' });
  if (ctx.owner) groups = groups.filter(g => run.tasks.some(t => groupOf(t) === g.key && mine(t)));
  const span = (r: RunResult, g: { key: string; name: string }) => {
    const all = r.tasks.filter(t => groupOf(t) === g.key && mine(t) && (t.kind !== 'done' || t.actual_start));
    const mem = all.filter(t => t.kind !== 'done');
    if (!all.length) return null;
    const xs2 = all.map(t => bx(t));
    const rank: Partial<Record<FloatColor, number>> = { crit: 0, red: 1, yellow: 2, green: 3, wait: 4 };
    const worst = mem.map(t => t.color).sort((a, b) => (rank[a] ?? 9) - (rank[b] ?? 9))[0];
    const fl = mem.filter(t => t.tf != null && t.kind !== 'wait').map(t => t.tf as number);
    return { x1: Math.min(...xs2.map(v => v[0])), x2: Math.max(...xs2.map(v => v[1])), n: all.length, done: all.length - mem.length, color: (worst || 'done') as FloatColor, minf: fl.length ? Math.min(...fl) / 2 : null };
  };
  const { lw, ow, rh, top, W, xs, xe, bx, head } = ganttFrame(el, ctx, groups.length);
  let s = head;
  const ry = (i: number) => top + i * rh;
  // Story dependencies: one line per pair of stories that have any task dependency between them
  const K = P.index(run), seen = new Set();
  let deps = '';
  for (const t of run.tasks) for (const p of t.reduced_after) {
    if (!mine(t) || !mine(K[p])) continue;
    const a = groupOf(K[p]), b = groupOf(t), k = a + '>' + b;
    if (a === b || seen.has(k)) continue; seen.add(k);
    const ia = groups.findIndex(g => g.key === a), ib = groups.findIndex(g => g.key === b);
    const sa = span(run, groups[ia]), sb = span(run, groups[ib]); if (!sa || !sb) continue;
    const x1 = sa.x2, y1 = ry(ia) + rh / 2, x2 = sb.x1, y2 = ry(ib) + rh / 2;
    const d = x2 - 12 >= x1 ? `M${x1} ${y1}H${x1 + 6}V${y2}H${x2 - 1}` : `M${x1} ${y1}h6V${y2 + (y2 > y1 ? -rh / 2 : rh / 2)}H${x2 - 8}V${y2}H${x2 - 1}`;
    deps += `<path d="${d}" fill="none" stroke="${css('--muted')}" stroke-width="1.2" opacity=".55" marker-end="url(#ga)"/>`;
  }
  let bars = '';
  groups.forEach((g, i) => {
    const y = ry(i), sp = span(run, g), b0 = span(base, g), sel = ctx.selStory === g.key && g.key;
    if (sel) bars += `<rect x="0" y="${y}" width="${W}" height="${rh}" fill="${css('--accent')}" opacity=".08"/>`;
    bars += `<g class="story-row" data-story="${esc(g.key)}">` + (g.key ? P.ICON('story', 6, y + 8) : '') +
      `<text x="${g.key ? 28 : 8}" y="${y + 21}" font-size="13" font-weight="600" style="${g.key ? '' : `fill:${css('--muted')}`}">${esc(g.name.slice(0, 15))}<title>${esc(g.name)}</title></text>`;
    bars += ownerCell([...new Set(run.tasks.filter(t => groupOf(t) === g.key && mine(t)).flatMap(t => t.owners))], ow, y);
    if (sp) {
      const x1 = sp.x1, w = Math.max(6, sp.x2 - x1), crit = sp.color === 'crit';
      if (b0 && (b0.x1 !== sp.x1 || b0.x2 !== sp.x2)) bars += `<rect x="${b0.x1}" y="${y + 7}" width="${Math.max(3, b0.x2 - b0.x1)}" height="${rh - 14}" rx="4" fill="none" stroke="${css('--muted')}" stroke-dasharray="3 3"/>`;
      const tip = `${g.name}\n${sp.n} tasks (${sp.done} done) · min float ${sp.minf ?? '—'}d`;
      bars += `<rect x="${x1}" y="${y + 6}" width="${w}" height="${rh - 12}" rx="4" fill="${sp.color === 'wait' ? 'url(#hatch)' : css(P.COLOR[sp.color])}" style="cursor:pointer"><title>${esc(tip)}</title></rect>`;
      if (crit) bars += `<rect x="${x1 - 2}" y="${y + 4}" width="${w + 4}" height="${rh - 8}" rx="5" fill="none" stroke="${css('--f-crit')}" stroke-width="1.5" pointer-events="none"/>`;
      bars += `<text x="${x1 + w + 8}" y="${y + 21}" font-size="11.5" style="fill:${css('--muted')}">${sp.n}</text>`;
    }
    bars += `</g>`;
  });
  el.innerHTML = ganttLevel(ctx) + `<div style="overflow-x:auto">${s}${deps}${bars}</svg></div>` + P.LEGEND();
  wireLevel(el, ctx);
  el.querySelectorAll<SVGGElement>('.story-row').forEach(g => g.addEventListener('click', () => g.dataset.story ? ctx.selectStory(g.dataset.story) : null));
};

// Gantt: drag a bar to move its start, drag its right edge to change its duration. Links run from a predecessor's end to a successor's start
P.views.gantt = function (el: HTMLElement, ctx: ViewCtx) {
  if (ctx.level === 'story') return P.views.ganttStories(el, ctx);
  const { css, esc, D, iso, mdw, S } = P;
  const { run, base } = ctx, cal = S.cal;
  const B = P.index(base);
  // Rows: a header per story (in plan order), its tasks below by start before any move; tasks without a story last
  const stories = (ctx.plan?.stories || []);
  const sIdx: Record<string, number> = Object.fromEntries(stories.map((x, i) => [x.key, i]));
  // Done tasks with actual dates come first by actual start, the rest by computed start
  const when = (t: TaskView) => t.kind === 'done' && t.actual_start ? (D(t.actual_start).getTime() - D('2000-01-01').getTime()) / 864e5 - 1e6 : t.es;
  const ids = base.tasks.filter(t => !ctx.owner || t.owners.includes(ctx.owner)).map(t => t.id).sort((a, b) => ((sIdx[B[a].story] ?? 1e9) - (sIdx[B[b].story] ?? 1e9)) || (when(B[a]) - when(B[b])));
  // A row is a task id or a story header
  const rows: (string | { story: string })[] = [];
  let curS: string | undefined = undefined;
  for (const id of ids) {
    const sk = sIdx[B[id].story] != null ? B[id].story : '';
    if (sk !== curS && sk) rows.push({ story: sk });
    curS = sk; rows.push(id);
  }
  if (!ctx.owner) for (const x of stories) if (!ids.some(id => B[id].story === x.key)) rows.push({ story: x.key });
  const K = P.index(run);
  const { t0, nd, lw, ow, rh, top, dw, W, H, xd, xs, xe, bx, last, head } = ganttFrame(el, ctx, rows.length);
  let s = head;
  const ry = (id: string) => top + rows.indexOf(id) * rh;
  const taskRows = rows.filter((r): r is string => typeof r === 'string');
  const rel = ctx.focus ? P.related(run, ctx.focus) : null;
  // Highlight of the selected row
  if (ctx.sel && rows.includes(ctx.sel)) s += `<rect x="0" y="${ry(ctx.sel)}" width="${W}" height="${rh}" fill="${css('--accent')}" opacity=".08"/>`;

  let deps = '';
  for (const k of taskRows) for (const p of K[k].reduced_after) {
    if (!rows.includes(p)) continue; // filtered out by owner
    const x1 = bx(K[p])[1], y1 = ry(p) + rh / 2, x2 = bx(K[k])[0], y2 = ry(k) + rh / 2;
    const on = rel && rel.has(p) && rel.has(k), tight = K[p].ef === K[k].es;
    const d = x2 - 12 >= x1 ? `M${x1} ${y1}H${x1 + 6}V${y2}H${x2 - 1}` : `M${x1} ${y1}h6V${y2 + (y2 > y1 ? -rh / 2 : rh / 2)}H${x2 - 8}V${y2}H${x2 - 1}`;
    deps += `<path d="${d}" fill="none" stroke="${css(on ? '--accent' : '--muted')}" stroke-width="${on ? 2 : 1.2}" opacity="${rel ? (on ? 1 : .12) : (tight ? .75 : .35)}" marker-end="url(#${on ? 'gh' : 'ga'})"/>`;
  }
  let bars = '';
  rows.forEach((r, i) => {
    if (typeof r === 'string') return;
    const y = top + i * rh, sk = r.story, sname = stories.find(x => x.key === sk)?.name || sk;
    const mem = run.tasks.filter(t => (sk ? t.story === sk : !(t.story in sIdx)) && (t.kind !== 'done' || t.actual_start));
    const sel = ctx.selStory && ctx.selStory === sk;
    bars += `<g class="story-row" data-story="${esc(sk)}"><rect x="0" y="${y}" width="${W}" height="${rh}" fill="${css('--surface-2')}" opacity="${sel ? .9 : .45}"/>`;
    bars += P.ICON('story', 6, y + 8) + `<text x="28" y="${y + 21}" font-size="13" font-weight="600">${esc(sname.slice(0, 15))}<title>${esc(sname)}</title></text>`;
    if (mem.length) {
      const x1 = Math.min(...mem.map(t => bx(t)[0])), x2 = Math.max(...mem.map(t => bx(t)[1]));
      bars += `<path d="M${x1} ${y + 22}V${y + 12}H${x2}V${y + 22}" fill="none" stroke="${css('--muted')}" stroke-width="2"/>`;
    }
    bars += `</g>`;
  });
  for (const k of taskRows) {
    const t = K[k], b = B[k], y = ry(k), dim = rel && !rel.has(k);
    const crit = t.tf === 0 && t.kind !== 'done';
    const tip = `${t.name}\n${mdw(S.start(t.es))} ${S.half(t.es)} – ${t.dur ? mdw(S.end(t.ef)) + ' ' + S.endHalf(t.ef) : ''}\nFloat ${t.tf == null ? '—' : t.tf / 2}d · ${t.why}${ow < 0 && t.owners.length ? '\nOwner: ' + t.owners.join(', ') : ''}`;
    bars += `<g data-id="${k}" opacity="${dim ? .25 : 1}">`;
    const ind = t.story && sIdx[t.story] != null ? 14 : 0;
    bars += P.ICON('task', 6 + ind, y + 9, 14) + `<text x="${26 + ind}" y="${y + 21}" font-size="13" font-weight="${crit ? 600 : 400}" style="cursor:pointer">${esc((t.short || t.name).slice(0, 14))}<title>${esc(tip)}</title></text>`;
    bars += ownerCell(t.owners, ow, y);
    bars += P.NEEDS(t.needs, lw - 12, y + 16);
    if (b.es !== t.es || b.ef !== t.ef) bars += `<rect x="${xs(b.es)}" y="${y + 7}" width="${Math.max(3, xe(b.ef) - xs(b.es))}" height="${rh - 14}" rx="4" fill="none" stroke="${css('--muted')}" stroke-dasharray="3 3"/>`;
    if (t.kind === 'milestone') {
      const cx = xs(t.es); bars += `<path class="bar" data-id="${k}" d="M${cx} ${y + 6}l10 10l-10 10l-10-10z" fill="${css(crit ? '--f-crit' : '--muted')}"><title>${esc(tip)}</title></path>`;
    } else {
      const [bx1, bx2] = bx(t), x1 = bx1, w = Math.max(6, bx2 - bx1), wait = t.kind === 'wait';
      const fill = wait ? 'url(#hatch)' : css(P.COLOR[t.color]);
      bars += `<rect class="${t.kind === 'done' ? '' : 'bar'}" data-id="${k}" x="${x1}" y="${y + 6}" width="${w}" height="${rh - 12}" rx="4" fill="${fill}"
        stroke="${crit ? css('--f-crit') : (ctx.sel === k ? css('--focus') : 'none')}" stroke-width="${crit ? 2 : 2}" ${crit && !wait ? `stroke-opacity=".0"` : ''}><title>${esc(tip)}</title></rect>`;
      if (crit) bars += `<rect x="${x1 - 2}" y="${y + 4}" width="${w + 4}" height="${rh - 8}" rx="5" fill="none" stroke="${css('--f-crit')}" stroke-width="1.5" pointer-events="none"/>`;
      if (t.kind !== 'done') bars += `<rect class="rz" data-id="${k}" x="${x1 + w - 6}" y="${y + 6}" width="12" height="${rh - 12}" fill="transparent"/>`;
      // Link handle: drag from here onto another task
      if (ctx.editable && (ctx.focus === k || ctx.sel === k)) bars += `<circle class="link" data-id="${k}" cx="${x1 + w + 12}" cy="${y + rh / 2}" r="6" fill="${css('--surface')}" stroke="${css('--accent')}" stroke-width="2" style="cursor:crosshair"><title>Drag onto the task that should follow</title></circle>`;
    }
    bars += `</g>`;
  }
  el.innerHTML = ganttLevel(ctx) + `<div style="overflow-x:auto">${s}${deps}${bars}</svg></div>` + P.LEGEND();
  wireLevel(el, ctx);

  const svg = el.querySelector<SVGSVGElement>('#gsvg')!;
  svg.querySelectorAll<SVGGElement>('g[data-id]').forEach(g => {
    g.addEventListener('mouseenter', () => ctx.hover(g.dataset.id!));
    g.addEventListener('click', () => { if (!ctx.dragged) ctx.select(g.dataset.id!) });
  });
  svg.addEventListener('mouseleave', () => ctx.hover(null));
  svg.querySelectorAll<SVGGElement>('.story-row').forEach(g => g.addEventListener('click', () => ctx.selectStory(g.dataset.story!)));
  svg.querySelectorAll<SVGCircleElement>('.link').forEach(c => c.addEventListener('pointerdown', ev => {
    ev.preventDefault(); ev.stopPropagation();
    const from = c.dataset.id!, box = svg.getBoundingClientRect();
    const x0 = c.cx.baseVal.value, y0 = c.cy.baseVal.value;
    const line = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    line.setAttribute('fill', 'none'); line.setAttribute('stroke', css('--accent')); line.setAttribute('stroke-width', '2'); line.setAttribute('stroke-dasharray', '5 4');
    svg.appendChild(line);
    ctx.dragged = true;
    const mv = (e: PointerEvent) => line.setAttribute('d', `M${x0} ${y0}L${e.clientX - box.left} ${e.clientY - box.top}`);
    const up = (e: PointerEvent) => {
      removeEventListener('pointermove', mv); removeEventListener('pointerup', up); line.remove();
      const g = document.elementFromPoint(e.clientX, e.clientY)?.closest<SVGGElement>('g[data-id]');
      setTimeout(() => ctx.dragged = false);
      if (g && g.dataset.id !== from) ctx.link(from, g.dataset.id!);
    };
    addEventListener('pointermove', mv); addEventListener('pointerup', up);
  }));
  // Drag to move. Below 4 px it is a click (drag threshold)
  const slotAt = (px: number) => {
    const f = (px - lw) / dw, i = Math.floor(f), dd = iso(new Date(t0.getTime() + i * 864e5));
    const j = cal.days.indexOf(dd);
    if (j < 0) { const n = cal.days.findIndex(z => z > dd); return Math.max(0, n) * 2 }
    return j * 2 + (f - i >= .5 ? 1 : 0);
  };
  svg.querySelectorAll<SVGElement>('.bar, .rz').forEach(node => node.addEventListener('pointerdown', ev => {
    const id = node.dataset.id!, rz = node.classList.contains('rz'), t = K[id];
    const left = () => svg.getBoundingClientRect().left;
    const x0 = ev.clientX, grab = slotAt(ev.clientX - left()) - t.es;
    let moving = false; ctx.dragged = false;
    const mv = (e: PointerEvent) => {
      if (!moving && Math.abs(e.clientX - x0) < 4) return;
      moving = true; ctx.dragged = true; node.classList.add('dragging');
      const sl = slotAt(e.clientX - left());
      if (rz) ctx.change(id, { dur: Math.max(1, sl - ctx.current(id)!.es + 1) }, false);
      else ctx.change(id, { min_start: Math.max(cal.start_slot, sl - grab) }, false);
    };
    const up = () => { removeEventListener('pointermove', mv); removeEventListener('pointerup', up); if (moving) ctx.commit(); else ctx.select(id); setTimeout(() => ctx.dragged = false) };
    addEventListener('pointermove', mv); addEventListener('pointerup', up);
    ev.preventDefault();
  }));
};
