// Load: effort per person per week against capacity (utilization × business days, days off excluded)
P.views.load = function (el: HTMLElement, ctx: ViewCtx) {
  const { css, esc, D, iso, md, S } = P;
  const run = ctx.run, cal = S.cal, r = Number(run.rate);
  const off = new Set(cal.off), st = cal.days[cal.start_slot >> 1];
  const mon = (s: string) => { const d = D(s); d.setDate(d.getDate() - ((d.getDay() + 6) % 7)); return iso(d) };
  const weeks = []; for (let w = D(mon(st)); w <= D(mon(S.endDay(run.end))); w = new Date(w.getTime() + 7 * 864e5)) weeks.push(iso(w));
  let out = '';
  for (const [p, byDay] of Object.entries(run.load)) {
    const lv = new Set((cal.leave[p] || []).map(i => cal.days[i]));
    const wk: Record<string, number> = {}; for (const [d, v] of Object.entries(byDay)) wk[mon(d)] = (wk[mon(d)] || 0) + v;
    const cap = (k: string) => { let n = 0; for (let i = 0; i < 5; i++) { const s = iso(new Date(D(k).getTime() + i * 864e5)); if (!off.has(s) && !lv.has(s) && s >= st) n += (s === st && cal.start_slot % 2) ? .5 : 1 } return n * r };
    const maxv = Math.max(5, Math.ceil(Math.max(...weeks.map(k => Math.max(wk[k] || 0, cap(k))))));
    const bw = 48, hh = 190, hl = 36, HW = hl + weeks.length * (bw + 14) + 12;
    let h = `<svg xmlns="http://www.w3.org/2000/svg" width="${HW}" height="${hh + 60}" viewBox="0 0 ${HW} ${hh + 60}" role="img" aria-label="${esc(p)} load">`;
    h += `<text x="0" y="14" font-size="13" font-weight="600">${esc(p)}</text><text x="${hl + 4}" y="30" font-size="11" style="fill:${css('--muted')}">person-days</text>`;
    for (let v = 0; v <= maxv; v++) { const y = 40 + hh - hh * v / maxv; h += `<line x1="${hl}" y1="${y}" x2="${HW}" y2="${y}" stroke="${css('--faint')}"/><text x="${hl - 6}" y="${y + 4}" font-size="11" text-anchor="end" style="fill:${css('--muted')}">${v}</text>` }
    weeks.forEach((k, i) => {
      const x = hl + 10 + i * (bw + 14), v = wk[k] || 0, c = cap(k), over = v > c + 1e-9;
      h += `<rect x="${x}" y="${40 + hh - hh * v / maxv}" width="${bw}" height="${hh * v / maxv}" rx="3" fill="${css(over ? '--red' : '--accent')}"><title>Week of ${md(k)}: ${v.toFixed(1)} / ${c.toFixed(1)}</title></rect>`;
      h += `<line x1="${x - 4}" x2="${x + bw + 4}" y1="${40 + hh - hh * c / maxv}" y2="${40 + hh - hh * c / maxv}" stroke="${css('--fg')}" stroke-width="2" stroke-dasharray="4 3"/>`;
      h += `<text x="${x + bw / 2}" y="${hh + 58}" font-size="11" text-anchor="middle" style="fill:${css('--muted')}">${md(k)}</text>`;
    });
    out += h + '</svg>';
  }
  el.innerHTML = (out || '') + `<div class="legend"><span><i class="sw" style="background:var(--accent)"></i>Planned</span><span><i class="sw" style="background:var(--red)"></i>Over</span><span><i class="sw" style="height:0;border-top:2px dashed var(--fg);background:none;border-radius:0"></i>Capacity</span></div>`;
};
