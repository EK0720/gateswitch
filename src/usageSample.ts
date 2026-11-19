// Browser-preview data for the Usage page. One seeded list of model calls is
// generated once, then filtered and aggregated the same way for every view, so
// the tabs agree with each other and nothing jumps between refetches.
import type {
  BlocksReport, Counted, Insight, SessionDetail, Totals, UsageAnalytics, UsageApp, UsageBlock,
  UsageBucket, UsageFilter, UsagePrice, UsageReport, UsageRow, UsageSettings,
} from "./api";

type Rec = Totals & {
  ts: number; day: string; hour: string; app: UsageApp; project: string; model: string;
  session: string; thread: number; expired: boolean;
};
type Thread = { label: string };
type Session = { key: string; label: string; app: UsageApp; project: string; threads: Thread[] };

const pad = (n: number) => String(n).padStart(2, "0");
/** Local calendar day; toISOString would give the UTC day. */
export const dayOf = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const hourOf = (d: Date) => `${dayOf(d)}T${pad(d.getHours())}`;
export const parseDay = (s: string) => { const [y, m, d] = s.split("-").map(Number); return new Date(y, m - 1, d); };
export const addDays = (s: string, n: number) => { const d = parseDay(s); d.setDate(d.getDate() + n); return dayOf(d); };

function rng(seed: number) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PROJECTS = ["gateswitch", "atlas-web", "notes-api"];
const MODELS: Record<UsageApp, string[]> = {
  claude: ["cc/claude-opus-5-5", "cc/claude-sonnet-5-5"],
  codex: ["cx/gpt-6", "cx/gpt-6-mini"],
  orca: ["cx/gpt-6", "gm/gemini-3-pro"],
};
const SUB_MODEL: Record<UsageApp, string> = { claude: "cc/claude-haiku-5-5", codex: "cx/gpt-6-mini", orca: "gm/gemini-3-pro" };
const TITLES = [
  "Fix tray icon on Windows", "Add CSV export", "Refactor provider store", "Investigate flaky probe test",
  "Write release notes", "Migrate settings schema", "Speed up log reader", "Review onboarding copy",
  "Port chart to SVG", "Debug Codex catalog sync", "Add dark theme tokens", "Clean up i18n keys",
];
const SUBS = ["Explore the codebase", "Review the diff", "Run the test suite", "Search the docs", "Check i18n coverage"];

let recs: Rec[] = [];
let sessions = new Map<string, Session>();

function build() {
  const r = rng(20261119);
  const pick = <T,>(a: T[]) => a[Math.floor(r() * a.length)];
  const now = Date.now();
  const today = new Date(); today.setHours(0, 0, 0, 0);
  let sid = 0;

  function session(app: UsageApp, project: string, start: number, turns: number) {
    const key = `${app}:${(0x3a7f + sid * 7919).toString(16)}-${pad(sid++ % 100)}`;
    const s: Session = { key, label: pick(TITLES), app, project, threads: [{ label: "main" }] };
    sessions.set(key, s);
    const model = r() < 0.7 ? MODELS[app][0] : MODELS[app][1];
    thread(s, 0, model, start, turns, 14_000 + r() * 10_000);
    const subs = app === "orca" ? 0 : r() < 0.45 ? 1 + Math.floor(r() * 3) : 0;
    for (let i = 0; i < subs; i++) {
      s.threads.push({ label: pick(SUBS) });
      thread(s, i + 1, SUB_MODEL[app], start + (4 + r() * 30) * 60_000, 6 + Math.floor(r() * 24), 7_000 + r() * 6_000);
    }
  }

  function thread(s: Session, ti: number, model: string, start: number, turns: number, ctx0: number) {
    let ts = start, ctx = ctx0, last = 0;
    for (let i = 0; i < turns; i++) {
      ts += (r() < 0.06 ? 6 + r() * 20 : 0.2 + r() * 1.6) * 60_000;
      if (ts > now) break;
      const grow = 600 + r() * 3200;
      const expired = last > 0 && ts - last > 5 * 60_000;
      last = ts;
      const output = Math.round(150 + r() * 2200);
      let input: number, cached: number, cache_write: number;
      if (s.app === "claude") {
        cache_write = Math.round(expired || i === 0 ? ctx : grow);
        input = Math.round(2 + r() * 40);
        cached = Math.max(0, Math.round(ctx - cache_write));
      } else {
        cache_write = 0;
        input = Math.round(grow + (expired || i === 0 ? ctx * 0.9 : 0));
        cached = Math.max(0, Math.round(ctx - input));
      }
      ctx += grow + output;
      if (r() < 0.015 && ctx > 120_000) ctx *= 0.35; // compaction
      const d = new Date(ts);
      recs.push({ ts, day: dayOf(d), hour: hourOf(d), app: s.app, project: s.project, model, session: s.key, thread: ti, expired, input, cached, cache_write, output });
    }
  }

  for (let back = 59; back >= 0; back--) {
    const d = new Date(today); d.setDate(d.getDate() - back);
    const weekend = d.getDay() === 0 || d.getDay() === 6;
    const n = weekend ? Math.floor(r() * 2.2) : 2 + Math.floor(r() * 4);
    for (let k = 0; k < n; k++) {
      const hour = pick([9, 10, 10, 11, 13, 14, 15, 15, 16, 17, 20, 21, 22]);
      const start = d.getTime() + (hour * 60 + r() * 50) * 60_000;
      const u = r();
      const app: UsageApp = u < 0.48 ? "claude" : u < 0.86 ? "codex" : "orca";
      session(app, pick(PROJECTS), start, 15 + Math.floor(r() * 90));
    }
  }
  // An active 5h block: a session that started a bit over two hours ago.
  session("claude", "gateswitch", now - 128 * 60_000, 400);
  recs.sort((a, b) => a.ts - b.ts);
}

function data() {
  if (!recs.length) build();
  return recs;
}

const zero = (): Counted => ({ input: 0, cached: 0, cache_write: 0, output: 0, turns: 0 });
function add(a: Totals & { turns?: number }, b: Totals) {
  a.input += b.input; a.cached += b.cached; a.cache_write += b.cache_write; a.output += b.output;
  if (a.turns !== undefined) a.turns++;
}
const all = (x: Totals) => x.input + x.cached + x.cache_write + x.output;

function match(f: UsageFilter, x: Rec, range = true) {
  if (range && f.from && x.day < f.from) return false;
  if (range && f.to && x.day > f.to) return false;
  if (f.apps && f.apps.length && !f.apps.includes(x.app)) return false;
  if (f.project && x.project !== f.project) return false;
  if (f.model && x.model !== f.model) return false;
  return true;
}
const filtered = (f: UsageFilter) => data().filter((x) => match(f, x));

function rows(list: Rec[], keyOf: (x: Rec) => string, meta: (k: string, x: Rec) => Partial<UsageRow>): UsageRow[] {
  const m = new Map<string, UsageRow & { _subs: Set<number> }>();
  for (const x of list) {
    const k = keyOf(x);
    let row = m.get(k);
    if (!row) { row = { ...zero(), key: k, label: k, app: null, project: null, last: "", subagents: 0, models: {}, _subs: new Set(), ...meta(k, x) }; m.set(k, row); }
    add(row, x);
    add((row.models[x.model] ??= { input: 0, cached: 0, cache_write: 0, output: 0 }), x);
    row.last = new Date(x.ts).toISOString();
    if (x.thread > 0) row._subs.add(x.thread);
  }
  return [...m.values()].map(({ _subs, ...row }) => ({ ...row, subagents: _subs.size })).sort((a, b) => all(b) - all(a));
}

export function sampleReport(f: UsageFilter): UsageReport {
  const src = data();
  const list = filtered(f);
  const today = dayOf(new Date());
  const from = f.from ?? (src[0]?.day ?? today);
  const to = f.to ?? today;
  const bucket: "hour" | "day" = f.from && f.from === f.to ? "hour" : "day";
  const buckets: UsageBucket[] = [];
  const index = new Map<string, UsageBucket>();
  if (bucket === "hour") {
    for (let h = 0; h < 24; h++) { const b = { start: `${from}T${pad(h)}`, input: 0, cached: 0, cache_write: 0, output: 0, by_app: {}, by_model: {} }; buckets.push(b); index.set(b.start, b); }
  } else {
    for (let d = from; d <= to; d = addDays(d, 1)) { const b = { start: d, input: 0, cached: 0, cache_write: 0, output: 0, by_app: {}, by_model: {} }; buckets.push(b); index.set(d, b); }
  }
  for (const x of list) {
    const b = index.get(bucket === "hour" ? x.hour : x.day);
    if (!b) continue;
    add(b, x);
    add((b.by_app[x.app] ??= { input: 0, cached: 0, cache_write: 0, output: 0 }), x);
    add((b.by_model[x.model] ??= { input: 0, cached: 0, cache_write: 0, output: 0 }), x);
  }
  const total = zero();
  for (const x of list) add(total, x);
  const prev_total = zero();
  if (f.from) {
    const len = Math.round((parseDay(to).getTime() - parseDay(from).getTime()) / 864e5) + 1;
    const pf = addDays(from, -len), pt = addDays(from, -1);
    for (const x of src) if (match(f, x, false) && x.day >= pf && x.day <= pt) add(prev_total, x);
  }
  const ses = rows(list, (x) => x.session, (k) => { const s = sessions.get(k)!; return { label: s.label, app: s.app, project: s.project }; }).slice(0, 100);
  return {
    filter: f, total, prev_total, bucket, buckets,
    by_app: rows(list, (x) => x.app, (k) => ({ label: k === "claude" ? "Claude Code" : k === "codex" ? "Codex" : "Orca", app: k as UsageApp })),
    by_project: rows(list, (x) => x.project, () => ({})),
    by_model: rows(list, (x) => x.model, () => ({})),
    sessions: ses,
    projects: [...new Set(src.map((x) => x.project))].sort(),
    models: [...new Set(src.map((x) => x.model))].sort(),
    files: [214, 167, 23],
    skipped: 0,
  };
}

export function sampleSession(key: string): SessionDetail {
  const s = sessions.get(key) ?? [...sessions.values()][0];
  const list = data().filter((x) => x.session === s.key);
  const threads = rows(list, (x) => String(x.thread), (k) => ({ label: s.threads[Number(k)]?.label ?? k, app: s.app, project: s.project }))
    .sort((a, b) => Number(a.key) - Number(b.key))
    .map((r) => ({ ...r, key: `${s.key}#${r.key}` }));
  return { key: s.key, label: s.label, app: s.app, project: s.project, threads, by_model: rows(list, (x) => x.model, () => ({ app: s.app })) };
}

const BLOCK = 5 * 3600_000;
export function sampleBlocks(f: UsageFilter): BlocksReport {
  const now = Date.now();
  const blocks: UsageBlock[] = [];
  let cur: UsageBlock | null = null, end = 0;
  for (const x of filtered(f)) {
    if (!cur || x.ts >= end) {
      end = x.ts + BLOCK;
      cur = { ...zero(), start: new Date(x.ts).toISOString(), end: new Date(end).toISOString(), active: false, tokens_per_min: null, projected: null };
      blocks.push(cur);
    }
    add(cur, x);
  }
  const last = blocks[blocks.length - 1];
  let current: UsageBlock | null = null;
  if (last && Date.parse(last.end) > now) {
    const mins = Math.max(1, (now - Date.parse(last.start)) / 60_000);
    last.active = true;
    last.tokens_per_min = Math.round(all(last) / mins);
    last.projected = Math.round((all(last) / mins) * 300);
    current = last;
  }
  return { blocks, current };
}

const fmtM = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : `${Math.round(n / 1e3)}k`);

export function sampleAnalytics(f: UsageFilter): UsageAnalytics {
  const list = filtered(f);
  const tot = zero(), main = zero(), subagents = zero();
  const by_hour = Array(24).fill(0);
  const subModels = new Map<string, number>();
  const maxCtx = new Map<string, number>();
  let cache_expiries = 0, expiryTokens = 0;
  for (const x of list) {
    add(tot, x);
    add(x.thread === 0 ? main : subagents, x);
    if (x.thread > 0) subModels.set(x.model, (subModels.get(x.model) ?? 0) + all(x));
    by_hour[new Date(x.ts).getHours()] += all(x);
    if (x.expired && x.cache_write > 0) { cache_expiries++; expiryTokens += x.cache_write; }
    const c = x.input + x.cached + x.cache_write;
    if (c > (maxCtx.get(x.session) ?? 0)) maxCtx.set(x.session, c);
  }
  const sum = all(tot) || 1;
  const sesRows = new Map(rows(list, (x) => x.session, (k) => { const s = sessions.get(k)!; return { label: s.label, app: s.app, project: s.project }; }).map((r) => [r.key, r]));
  const biggest_context = [...maxCtx.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5)
    .map(([k, max_context]) => ({ ...sesRows.get(k)!, max_context }));

  const insights: Insight[] = [];
  if (cache_expiries > 0) insights.push({ level: "warn", title: `${cache_expiries} cache rewrites after idle gaps`, detail: `A pause longer than 5 minutes lets the prompt cache expire, so the next turn writes the whole context again. These rewrites cost ${fmtM(expiryTokens)} tokens.` });
  if (biggest_context[0] && biggest_context[0].max_context > 150_000) insights.push({ level: "warn", title: `One session reached ${fmtM(biggest_context[0].max_context)} tokens of context`, detail: "Long contexts make every turn expensive. Start a new session or compact when the task changes." });
  const subShare = all(subagents) / sum;
  if (subShare > 0.02) {
    const top = [...subModels.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
    insights.push({ level: "info", title: `Subagents used ${Math.round(subShare * 100)}% of tokens`, detail: `Most of it went to ${top}.` });
  }
  const peak = by_hour.indexOf(Math.max(...by_hour));
  if (all(tot) > 0) insights.push({ level: "info", title: `Busiest hour: ${pad(peak)}:00`, detail: `${Math.round((by_hour[peak] / sum) * 100)}% of tokens were spent between ${pad(peak)}:00 and ${pad((peak + 1) % 24)}:00.` });

  return {
    share: { input: tot.input / sum, cached: tot.cached / sum, cache_write: tot.cache_write / sum, output: tot.output / sum },
    by_hour, main, subagents, cache_expiries, biggest_context, insights,
  };
}

export let samplePrices: Record<string, UsagePrice> = {
  "cc/claude-opus-5-5": { input: 5, cached: 0.5, write: 6.25, output: 25 },
  "cc/claude-sonnet-5-5": { input: 3, cached: 0.3, output: 15 },
  "cx/gpt-6": { input: 1.25, cached: 0.125, output: 10 },
};
export function setSamplePrices(p: Record<string, UsagePrice>) { samplePrices = { ...p }; }

export let sampleSettings: UsageSettings = { watch: true, tray: false, overlay: false, notify: false, block_limit: 40_000_000, quota_auto: false, close_to_tray: false };
export function setSampleSettings(s: UsageSettings) { sampleSettings = { ...s }; }
