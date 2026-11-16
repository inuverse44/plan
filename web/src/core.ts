// Shared UI helpers. No scheduling here (that is PlanningManager in WASM): date formatting, colors, graph traversal.
// Plan and result types (Plan, RunResult, TaskView, ...) come from contracts.d.ts, generated from the Rust crate plan-contracts.

/** What each view (Gantt, network, load, table, compare) receives */
interface ViewCtx {
  level: 'story' | 'task';
  setLevel(v: 'story' | 'task'): void;
  run: RunResult;
  base: RunResult;
  plan: Plan;
  sel: string | null;
  selStory: string | null;
  focus: string | null;
  scenario: string;
  rate: number;
  est: EstKind;
  editable: boolean;
  hover(id: string | null): void;
  select(id: string): void;
  selectStory(key: string): void;
  link(from: string, to: string): void;
  change(id: string, patch: { min_start?: number; dur?: number }, now: boolean): void;
  commit(): void;
  current(id: string): TaskView | undefined;
  pickRun(scenario: string, rate: number): void;
  dragged: boolean;
  /** Other epics' loads (their default runs), for people who work on several epics. Empty in an exported page */
  others: EpicLoad[];
}
type View = (el: HTMLElement, ctx: ViewCtx) => void;
type ColorName = FloatColor | 'muted' | 'accent' | 'focus';

const P = (() => {
  const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
  const css = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
  const esc = (s: unknown) => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]!));
  const D = (s: string) => new Date(s + 'T00:00:00');
  const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const WD = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'], MO = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const md = (s: string) => { const d = D(s); return `${MO[d.getMonth()]} ${d.getDate()}` };
  const mdw = (s: string) => { const d = D(s); return `${WD[d.getDay()]} ${MO[d.getMonth()]} ${d.getDate()}` };
  const pct = (r: number | string) => Math.round(Number(r) * 100) + '%';
  const num = (x: number) => Math.round(x * 100) / 100;
  // Float level (FloatColor from the engine) → CSS color variable
  const COLOR: Record<ColorName, string> = { crit: '--f-crit', red: '--f-low', yellow: '--f-mid', green: '--f-high', wait: '--wait', done: '--done', muted: '--muted', accent: '--accent', focus: '--focus' };
  const COLOR_NAME: Record<FloatColor, string> = { crit: 'Critical', red: 'Low float', yellow: 'Mid float', green: 'High float', wait: 'Wait', done: 'Done' };

  // Half-day slots and dates. ev and cal are set when an epic is opened
  const S = {
    ev: null as unknown as Evaluation, cal: null as unknown as BusinessCalendar,
    start: (s: number) => S.cal.days[s >> 1], half: (s: number) => (s % 2 ? 'PM' : 'AM'),
    end: (f: number) => S.cal.days[Math.max(0, f - 1) >> 1], endHalf: (f: number) => ((f - 1) % 2 ? 'PM' : 'AM'),
    endDay: (f: number) => (f > S.cal.start_slot ? S.cal.days[(f - 1) >> 1] : S.cal.days[S.cal.start_slot >> 1]),
  };

  function index(run: RunResult): Record<string, TaskView> { return Object.fromEntries(run.tasks.map(t => [t.id, t])) }
  function succOf(run: RunResult): Record<string, string[]> {
    const m: Record<string, string[]> = Object.fromEntries(run.tasks.map(t => [t.id, []]));
    for (const t of run.tasks) for (const p of t.after) m[p]?.push(t.id);
    return m;
  }
  // Tasks reachable backwards and forwards from the given task
  function related(run: RunResult, id: string): Set<string> {
    const K = index(run), su = succOf(run), set = new Set([id]);
    const up = (k: string) => { for (const p of K[k]?.after || []) if (!set.has(p)) { set.add(p); up(p) } };
    const dn = (k: string) => { for (const s of su[k] || []) if (!set.has(s)) { set.add(s); dn(s) } };
    up(id); dn(id); return set;
  }
  function legend(items: [string, string, string?][]) {
    return `<div class="legend">${items.map(([c, l, extra]) => `<span><i class="sw" style="background:var(${c});${extra || ''}"></i>${l}</span>`).join('')}</div>`;
  }
  const LEGEND = () => legend([['--f-crit', 'Critical', 'outline:2px solid var(--f-crit);outline-offset:1px'], ['--f-low', 'Low'], ['--f-mid', 'Mid'], ['--f-high', 'High float'],
    ['--wait', 'Wait', 'background:repeating-linear-gradient(135deg,var(--wait) 0 3px,transparent 3px 6px)']]);
  // Epic, story and task icons (symbols in index.html)
  const ICON = (kind: 'epic' | 'story' | 'task', x: number, y: number, s = 16) => `<use href="#i-${kind}" x="${x}" y="${y}" width="${s}" height="${s}"/>`;
  let toastTimer = 0;
  function toast(s: string) { const t = $('toast'); t.textContent = s; t.style.opacity = '1'; clearTimeout(toastTimer); toastTimer = setTimeout(() => t.style.opacity = '0', 1600) }
  // Marker for tasks that need resources (an SVG, not an emoji); the tooltip names them and their off periods
  const needsTip = (needs: string[]) => 'Needs ' + needs.map(n => { const r = S.ev?.resources?.[n]; return r?.label ? `${n} (avoids ${r.label})` : n }).join(', ');
  const NEEDS = (needs: string[], x: number, y: number, s = 11) => needs.length ? `<path d="M${x} ${y - s / 2}l${s / 2} ${s / 2}l${-s / 2} ${s / 2}l${-s / 2} ${-s / 2}z" fill="none" stroke="${css('--red')}" stroke-width="1.6"><title>${esc(needsTip(needs))}</title></path>` : '';

  const views: Record<string, View> = {};
  return { $, css, esc, D, iso, md, mdw, pct, num, COLOR, COLOR_NAME, S, index, succOf, related, legend, LEGEND, ICON, toast, NEEDS, views };
})();
