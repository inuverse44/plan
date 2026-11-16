// Load: effort per person per week against capacity (utilization × business days, days off excluded).
// When a person also works on other epics, their loads (each epic's default run) are stacked on top and capacity is the person's whole week
P.views.load = function (el: HTMLElement, ctx: ViewCtx) {
  const { css, esc, D, iso, md, S } = P;
  const run = ctx.run, cal = S.cal, r = Number(run.rate);
  const off = new Set(cal.off), st = cal.days[cal.start_slot >> 1];
  const mon = (s: string) => { const d = D(s); d.setDate(d.getDate() - ((d.getDay() + 6) % 7)); return iso(d) };
  const weeks = []; for (let w = D(mon(st)); w <= D(mon(S.endDay(run.end))); w = new Date(w.getTime() + 7 * 864e5)) weeks.push(iso(w));
  const shade = (i: number) => `opacity:${0.75 - 0.2 * (i % 3)}`;
  let out = '', anyOthers = false;
  for (const [p, byDay] of Object.entries(run.load)) {
    const lv = new Set((cal.leave[p] || []).map(i => cal.days[i]));
    const others = ctx.others.filter(o => o.load[p] && Object.values(o.load[p]).some(v => v > 0));
    const shared = others.length > 0; anyOthers ||= shared;
    const sum = (m: Record<string, number>) => { const wk: Record<string, number> = {}; for (const [d, v] of Object.entries(m)) wk[mon(d)] = (wk[mon(d)] || 0) + v; return wk };
    const wk = sum(byDay), owk = others.map(o => sum(o.load[p]));
    // Days over one person's whole day, all epics added up
    const daily: Record<string, number> = { ...byDay }; for (const o of others) for (const [d, v] of Object.entries(o.load[p])) daily[d] = (daily[d] || 0) + v;
    const overDays = (k: string) => Object.entries(daily).filter(([d, v]) => mon(d) === k && v > 1 + 1e-9).map(([d]) => md(d));
    // Capacity: this epic's share (utilization) alone, or the whole person when other epics take part of the week
    const cap = (k: string) => { let n = 0; for (let i = 0; i < 5; i++) { const s = iso(new Date(D(k).getTime() + i * 864e5)); if (!off.has(s) && !lv.has(s) && s >= st) n += (s === st && cal.start_slot % 2) ? .5 : 1 } return n * (shared ? 1 : r) };
    const tot = (k: string) => (wk[k] || 0) + owk.reduce((a, w) => a + (w[k] || 0), 0);
    const maxv = Math.max(5, Math.ceil(Math.max(...weeks.map(k => Math.max(tot(k), cap(k))))));
    const bw = 48, hh = 190, hl = 36, HW = hl + weeks.length * (bw + 14) + 12, y0 = 40 + hh, sc = (v: number) => hh * v / maxv;
    let h = `<svg xmlns="http://www.w3.org/2000/svg" width="${HW}" height="${hh + 60}" viewBox="0 0 ${HW} ${hh + 60}" role="img" aria-label="${esc(p)} load">`;
    h += `<text x="0" y="14" font-size="13" font-weight="600">${esc(p)}${shared ? `<tspan font-weight="400" style="fill:${css('--muted')}"> · also ${esc(others.map(o => o.epic).join(', '))}</tspan>` : ''}</text><text x="${hl + 4}" y="30" font-size="11" style="fill:${css('--muted')}">person-days</text>`;
    for (let v = 0; v <= maxv; v++) { const y = y0 - sc(v); h += `<line x1="${hl}" y1="${y}" x2="${HW}" y2="${y}" stroke="${css('--faint')}"/><text x="${hl - 6}" y="${y + 4}" font-size="11" text-anchor="end" style="fill:${css('--muted')}">${v}</text>` }
    weeks.forEach((k, i) => {
      const x = hl + 10 + i * (bw + 14), v = wk[k] || 0, c = cap(k), t = tot(k), over = t > c + 1e-9, od = overDays(k);
      const tip = [`Week of ${md(k)}: ${t.toFixed(1)} / ${c.toFixed(1)}`, `${S.ev.key || 'this epic'} ${v.toFixed(1)}`, ...others.map((o, j) => `${o.epic} ${(owk[j][k] || 0).toFixed(1)}`), ...(od.length ? [`Over 100%: ${od.join(', ')}`] : [])].join('\n');
      let y = y0;
      h += `<g><title>${esc(tip)}</title>`;
      h += `<rect x="${x}" y="${y - sc(v)}" width="${bw}" height="${sc(v)}" rx="3" fill="${css(over && !shared ? '--red' : '--accent')}"/>`; y -= sc(v);
      others.forEach((o, j) => { const w = owk[j][k] || 0; if (w > 0) { h += `<rect x="${x}" y="${y - sc(w)}" width="${bw}" height="${sc(w)}" fill="${css('--muted')}" style="${shade(j)}"/>`; y -= sc(w) } });
      if (shared && over) h += `<rect x="${x}" y="${y0 - sc(t)}" width="${bw}" height="${sc(t - c)}" fill="none" stroke="${css('--red')}" stroke-width="2.5"/>`;
      h += `</g>`;
      h += `<line x1="${x - 4}" x2="${x + bw + 4}" y1="${y0 - sc(c)}" y2="${y0 - sc(c)}" stroke="${css('--fg')}" stroke-width="2" stroke-dasharray="4 3"/>`;
      h += `<text x="${x + bw / 2}" y="${hh + 58}" font-size="11" text-anchor="middle" style="fill:${css(od.length ? '--red' : '--muted')}" font-weight="${od.length ? 600 : 400}">${md(k)}</text>`;
    });
    out += h + '</svg>';
  }
  el.innerHTML = (out || '') + `<div class="legend"><span><i class="sw" style="background:var(--accent)"></i>Planned</span>${anyOthers ? '<span><i class="sw" style="background:var(--muted);opacity:.75"></i>Other epics</span>' : ''}<span><i class="sw" style="${anyOthers ? 'background:none;outline:2px solid var(--red);outline-offset:-2px' : 'background:var(--red)'}"></i>Over</span><span><i class="sw" style="height:0;border-top:2px dashed var(--fg);background:none;border-radius:0"></i>Capacity${anyOthers ? ' (whole person when shared)' : ''}</span></div>`;
};
