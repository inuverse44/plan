// Activity-on-arrow network: tasks are arrows, events are circles. Arrows that skip layers get virtual points. Wide graphs wrap to the frame's aspect ratio (odd rows run right to left)
P.views.net = function (el: HTMLElement, ctx: ViewCtx) {
  const { css, esc } = P;
  // Leave out done tasks (draw only the remaining dependencies)
  const run = { ...ctx.run, tasks: ctx.run.tasks.filter(t => t.kind !== 'done').map(t => ({ ...t, reduced_after: t.reduced_after.filter(p => ctx.run.tasks.find(x => x.id === p)?.kind !== 'done') })) }, K = P.index(run);
  // Edge: a task arrow (with id) or a dummy. pts are the points it passes (virtual points included)
  type Edge = { a: string; b: string; id?: string; dummy?: boolean; pts: string[] };
  type Pt = { x: number; y: number; row: number };
  const order = run.tasks.map(t => t.id);
  const red: Record<string, string[]> = Object.fromEntries(run.tasks.map(t => [t.id, t.reduced_after]));
  const has = new Set(Object.values(red).flat());
  const sinks = order.filter(k => !has.has(k));
  const endN: Record<string, string> = Object.fromEntries(order.map(k => [k, 'e:' + k]));
  if (sinks.length === 1) endN[sinks[0]] = 'F';
  let E: Edge[] = []; const merges: Record<string, string> = {};
  const startN = (k: string) => {
    const ps = red[k]; if (!ps.length) return 'S'; if (ps.length === 1) return endN[ps[0]];
    const mk = [...ps].sort().join(','); if (!merges[mk]) { merges[mk] = 'm:' + mk; for (const p of ps) E.push({ a: endN[p], b: merges[mk], dummy: true, pts: [] }) }
    return merges[mk];
  };
  for (const k of order) E.push({ a: startN(k), b: endN[k], id: k, pts: [] });
  if (sinks.length > 1) for (const k of sinks) E.push({ a: endN[k], b: 'F', dummy: true, pts: [] });
  // Remove dummies: if a task's end event has only one outgoing dummy, connect the task arrow to the dummy's target
  for (let again = true; again;) {
    again = false;
    for (const d of E.filter(e => e.dummy)) {
      const outs = E.filter(e => e.a === d.a), ins = E.filter(e => e.b === d.a);
      if (outs.length === 1 && ins.length === 1 && d.a !== 'S') { ins[0].b = d.b; E = E.filter(e => e !== d); again = true; break }
    }
  }
  const real = [...new Set(E.flatMap(e => [e.a, e.b]))];
  const rank: Record<string, number> = { S: 0 };
  for (let ch = true; ch;) { ch = false; for (const e of E) if (rank[e.a] != null && (rank[e.b] == null || rank[e.b] < rank[e.a] + 1)) { rank[e.b] = rank[e.a] + 1; ch = true } }
  const R0 = Math.max(...Object.values(rank)); rank.F = R0;
  const nodes = [...real], segs: [string, string][] = [];
  E.forEach((e, ei) => {
    e.pts = [e.a];
    for (let r = rank[e.a] + 1; r < rank[e.b]; r++) { const v = `v:${ei}:${r}`; rank[v] = r; nodes.push(v); e.pts.push(v) }
    e.pts.push(e.b);
    for (let i = 1; i < e.pts.length; i++) segs.push([e.pts[i - 1], e.pts[i]]);
  });
  const layers: string[][] = Array.from({ length: R0 + 1 }, () => []);
  for (const n of nodes) layers[rank[n]].push(n);
  const ord: Record<string, number> = {}; layers.forEach(L => L.forEach((n, i) => ord[n] = i));
  const nb = (n: string, up: boolean) => segs.filter(g => up ? g[1] === n : g[0] === n).map(g => up ? g[0] : g[1]);
  for (let it = 0; it < 6; it++) for (const up of [true, false]) {
    for (const L of (up ? layers.slice(1) : layers.slice(0, -1).reverse())) {
      const bc = (n: string) => { const v = nb(n, up).map(m => ord[m]); return v.length ? v.reduce((a, b) => a + b, 0) / v.length : ord[n] };
      L.sort((a, b) => bc(a) - bc(b) || ord[a] - ord[b]); L.forEach((n, i) => ord[n] = i);
    }
  }
  const maxL = Math.max(...layers.map(L => L.length)), pad = 40, gap = 54;
  const Wv = Math.max(600, el.clientWidth - 30), Hv = Math.max(340, innerHeight - el.getBoundingClientRect().top - 80);
  let best = 1, bs = -1, bdx = 120, bdy = 48;
  for (let k = 1; k <= Math.max(1, Math.ceil((R0 + 1) / 4)); k++) {
    const per = Math.ceil((R0 + 1) / k);
    const dx = Math.max(108, Math.min(200, (Wv - 2 * pad) / Math.max(1, per - 1)));
    const dy = Math.max(42, Math.min(66, (Hv - 2 * pad - (k - 1) * gap) / Math.max(1, k * (maxL - 1))));
    const w = (per - 1) * dx + 2 * pad, h = k * (maxL - 1) * dy + (k - 1) * gap + 2 * pad;
    const sc = Math.min(1, Wv / w, Hv / h) * (1 + 0.02 * Math.min(dx, 150) / 150);
    if (sc > bs + 1e-6) { bs = sc; best = k; bdx = dx; bdy = dy }
  }
  const dx = bdx, dy = bdy, per = Math.ceil((R0 + 1) / best), rowH = (maxL - 1) * dy + gap;
  const Pn: Record<string, Pt> = {};
  for (const n of nodes) {
    const r = rank[n], row = Math.floor(r / per), c = r % per, L = layers[r];
    Pn[n] = { x: pad + (row % 2 ? per - 1 - c : c) * dx, y: pad + row * rowH + ((maxL - L.length) * dy) / 2 + ord[n] * dy, row };
  }
  // Align the virtual points of a long arrow at one height per row so it runs straight (otherwise it zigzags layer by layer).
  // Avoid heights taken by other lanes or circles by shifting half a step at a time
  const lanes: { row: number; x0: number; x1: number; y: number }[] = [], near = dy * 0.45;
  const chains: string[][] = [];
  E.forEach(e => {
    const byRow: Record<number, string[]> = {};
    for (const n of e.pts) if (n.startsWith('v:')) (byRow[Pn[n].row] ||= []).push(n);
    for (const vs of Object.values(byRow)) chains.push(vs);
  });
  chains.sort((a, b) => b.length - a.length);
  for (const vs of chains) {
    const xs = vs.map(n => Pn[n].x), x0 = Math.min(...xs) - 1, x1 = Math.max(...xs) + 1, row = Pn[vs[0]].row;
    const ys = vs.map(n => Pn[n].y).sort((a, b) => a - b), mid = ys[ys.length >> 1];
    const free = (y: number) => !lanes.some(l => l.row === row && l.x0 <= x1 && x0 <= l.x1 && Math.abs(l.y - y) < near) &&
      !real.some(n => Pn[n].row === row && Pn[n].x >= x0 && Pn[n].x <= x1 && Math.abs(Pn[n].y - y) < near);
    let y = mid;
    for (let i = 1; i < 40 && !free(y); i++) y = mid + Math.ceil(i / 2) * (dy / 2) * (i % 2 ? 1 : -1);
    y = Math.max(pad, y);
    for (const n of vs) Pn[n].y = y;
    lanes.push({ row, x0, x1, y });
  }
  const W = Math.max(...Object.values(Pn).map(p => p.x)) + pad + 50, H = Math.max(...nodes.map(n => Pn[n].y)) + pad;
  const rel = ctx.focus ? P.related(run, ctx.focus) : null;
  let s = `<svg id="nsvg" xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}" role="img" aria-label="Network"><defs>`;
  for (const c of ['crit', 'red', 'yellow', 'green', 'wait', 'muted', 'accent', 'focus'] as ColorName[]) s += `<marker id="m-${c}" viewBox="0 0 10 10" refX="9" refY="5" markerUnits="userSpaceOnUse" markerWidth="10" markerHeight="10" orient="auto"><path d="M0 1L10 5L0 9z" fill="${css(P.COLOR[c])}"/></marker>`;
  s += `</defs>`;
  const pair: Record<string, number> = {}, labels: { x: number; y: number; id: string; t: TaskView; op: number; crit: boolean }[] = [];
  for (const e of E) {
    const pk = e.a + '>' + e.b, nth = pair[pk] = (pair[pk] || 0) + 1, bend = (nth - 1) * 20 * (nth % 2 ? 1 : -1);
    const t: TaskView | undefined = e.id ? K[e.id] : undefined;
    const crit = !!t && t.tf === 0;
    const col: ColorName = !t ? 'muted' : t.kind === 'wait' ? (crit ? 'crit' : 'wait') : (t.color === 'done' ? 'muted' : t.color);
    const isSel = !!e.id && e.id === ctx.sel, on = !!rel && !!e.id && rel.has(e.id);
    const stroke: ColorName = isSel ? 'focus' : on ? 'accent' : col, op = rel ? (on || isSel ? 1 : .15) : 1;
    const pts = e.pts.map(n => Pn[n]);
    let d = `M${pts[0].x} ${pts[0].y}`, lab: { x: number; y: number } | null = null, bestScore = -1e9;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = { ...pts[i] };
      if (i === pts.length - 1) { const dir = a.row === b.row ? Math.sign(b.x - a.x) || 1 : (a.row % 2 ? 1 : -1); b.x -= dir * 6 }
      if (a.row === b.row) {
        const mx = (a.x + b.x) / 2;
        d += `C${mx} ${a.y + bend} ${mx} ${b.y + bend} ${b.x} ${b.y}`;
        const score = -Math.abs(i - pts.length / 2); if (score > bestScore) { bestScore = score; lab = { x: mx, y: (a.y + b.y) / 2 + bend * .75 } }
      } else {
        const ox = a.row % 2 === 0 ? Math.max(a.x, b.x) + 44 : Math.min(a.x, b.x) - 44;
        d += `C${ox} ${a.y} ${ox} ${b.y} ${b.x} ${b.y}`;
        if (bestScore <= -1e9) lab = { x: ox - (a.row % 2 === 0 ? 8 : -8), y: (a.y + b.y) / 2 };
      }
    }
    const sw = e.dummy ? 1 : (crit ? 3 : 1.8), dash = !t ? '2 4' : t.kind === 'wait' ? '6 4' : '';
    s += `<path d="${d}" fill="none" stroke="${css(P.COLOR[stroke])}" stroke-width="${isSel ? sw + 1 : sw}" ${dash ? `stroke-dasharray="${dash}"` : ''} opacity="${op}" marker-end="url(#m-${stroke})" ${e.id ? `data-id="${e.id}" style="cursor:pointer"` : ''}>${t ? `<title>${esc(t.name)}</title>` : ''}</path>`;
    if (e.id && e.dummy !== true) s += `<path d="${d}" fill="none" stroke="transparent" stroke-width="14" data-id="${e.id}" style="cursor:pointer"/>`;
    if (e.id && t && lab) labels.push({ ...lab, id: e.id, t, op, crit });
  }
  for (const n of real) { const p = Pn[n], big = n === 'S' || n === 'F'; s += `<circle cx="${p.x}" cy="${p.y}" r="${big ? 7 : 4}" fill="${big ? css('--fg') : css('--surface')}" stroke="${css('--fg')}" stroke-width="1.5"/>` }
  for (const l of labels) {
    const text = l.t.short || l.t.name;
    const w = [...text].reduce((a, c) => a + (c.charCodeAt(0) > 0x2e80 ? 12 : 7), 0) + (l.t.prod ? 18 : 8);
    s += `<g opacity="${l.op}" data-id="${l.id}" style="cursor:pointer"><rect x="${l.x - w / 2}" y="${l.y - 20}" width="${w}" height="17" rx="4" fill="${css('--surface')}" opacity=".92"/>` +
      (l.t.prod ? P.PROD(l.x - w / 2 + 9, l.y - 11.5, 9) : '') +
      `<text x="${l.x + (l.t.prod ? 6 : 0)}" y="${l.y - 7.5}" font-size="12" text-anchor="middle" font-weight="${l.crit ? 600 : 400}">${esc(text)}</text><title>${esc(l.t.name)}</title></g>`;
  }
  el.innerHTML = `<div style="display:flex;justify-content:center">${s}</svg></div>` + P.LEGEND();
  const svg = el.querySelector<SVGSVGElement>('#nsvg')!;
  svg.querySelectorAll<SVGElement>('[data-id]').forEach(n => {
    n.addEventListener('mouseenter', () => ctx.hover(n.dataset.id!));
    n.addEventListener('click', () => ctx.select(n.dataset.id!));
  });
  svg.addEventListener('mouseleave', () => ctx.hover(null));
};
