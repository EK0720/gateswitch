import type { Totals, UsageApp, UsagePrice } from "../../api";
import { locale, t } from "../../i18n";

export const APPS: { key: UsageApp; name: string }[] = [
  { key: "claude", name: "Claude Code" },
  { key: "codex", name: "Codex" },
  { key: "orca", name: "Orca" },
];
export const appName = (a: UsageApp | null | undefined) => APPS.find((x) => x.key === a)?.name ?? "";

/** All input sent to the model: uncached + cache read + cache write (kinds are disjoint). */
export const inAll = (x: Totals) => x.input + x.cached + x.cache_write;
export const all = (x: Totals) => inAll(x) + x.output;
/** Share of input served from / written to cache, 0..100. */
export const readPct = (x: Totals) => (inAll(x) > 0 ? (x.cached / inAll(x)) * 100 : 0);
export const writePct = (x: Totals) => (inAll(x) > 0 ? (x.cache_write / inAll(x)) * 100 : 0);

export function fmt(n: number): string {
  if (n >= 1_000_000_000) return t("{n} B", { n: (n / 1_000_000_000).toFixed(2) });
  if (n >= 1_000_000) return t("{n} M", { n: (n / 1_000_000).toFixed(1) });
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)} k`;
  return String(Math.round(n));
}
export const usd = (n: number) => (n >= 100 ? `$${n.toFixed(0)}` : n >= 0.01 || n === 0 ? `$${n.toFixed(2)}` : "<$0.01");
export const pct = (n: number) => (n > 0 && n < 1 ? "<1%" : `${Math.round(n)}%`);

/** USD for these tokens at this price (per 1M). Cache write defaults to 1.25 × input. */
export function costOf(p: UsagePrice | undefined, x: Totals): number | null {
  if (!p) return null;
  const write = p.write ?? p.input * 1.25;
  return (x.input * p.input + x.cached * p.cached + x.cache_write * write + x.output * p.output) / 1_000_000;
}

export type RowCost = { usd: number; partial: boolean };
/** ≈$ of a row from its per-model split. null when no model in it is priced; partial when only some are. */
export function rowCost(prices: Record<string, UsagePrice>, models: Record<string, Totals> | undefined): RowCost | null {
  let usd = 0, priced = 0, used = 0;
  for (const [m, x] of Object.entries(models ?? {})) {
    if (all(x) === 0) continue;
    used++;
    const c = costOf(prices[m], x);
    if (c != null) { usd += c; priced++; }
  }
  return priced ? { usd, partial: priced < used } : null;
}
export const sumCost = (list: (RowCost | null)[]): RowCost | null =>
  list.reduce<RowCost | null>((a, c) => (c ? { usd: (a?.usd ?? 0) + c.usd, partial: !!a?.partial || c.partial } : a ? { ...a } : a), null);

/** "5M", "500k", "1.5 m", "20000000" → tokens. "" → null. NaN → undefined (invalid). */
export function parseTokens(s: string): number | null | undefined {
  const v = s.trim().toLowerCase().replace(/,/g, ".").replace(/\s+/g, "");
  if (!v) return null;
  const m = /^(\d+(?:\.\d+)?)([kmb]?)$/.exec(v);
  if (!m) return undefined;
  const mul = m[2] === "k" ? 1e3 : m[2] === "m" ? 1e6 : m[2] === "b" ? 1e9 : 1;
  return Math.round(Number(m[1]) * mul);
}
export const shortTokens = (n: number) => (n >= 1e9 ? `${+(n / 1e9).toFixed(2)}B` : n >= 1e6 ? `${+(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${+(n / 1e3).toFixed(0)}k` : String(n));

// ---- dates (local)
const pad = (n: number) => String(n).padStart(2, "0");
export const dayOf = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
export const parseDay = (s: string) => { const [y, m, d] = s.slice(0, 10).split("-").map(Number); return new Date(y, m - 1, d); };
export const addDays = (s: string, n: number) => { const d = parseDay(s); d.setDate(d.getDate() + n); return dayOf(d); };
export const today = () => dayOf(new Date());

export function dayLabel(s: string, opts: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" }) {
  return parseDay(s).toLocaleDateString(locale(), opts);
}
export function timeLabel(iso: string) {
  return new Date(iso).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
}
export function dateTimeLabel(iso: string) {
  return new Date(iso).toLocaleString(locale(), { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}
/** "3 min ago", "2 h ago", "Nov 3". */
export function ago(iso: string) {
  const m = (Date.now() - Date.parse(iso)) / 60_000;
  if (m < 1) return t("just now");
  if (m < 60) return t("{n} min ago", { n: Math.round(m) });
  if (m < 24 * 60) return t("{n} h ago", { n: Math.round(m / 60) });
  return new Date(iso).toLocaleDateString(locale(), { month: "short", day: "numeric" });
}
export function duration(ms: number) {
  const m = Math.max(0, Math.round(ms / 60_000));
  return m >= 60 ? t("{h} h {m} min", { h: Math.floor(m / 60), m: m % 60 }) : t("{n} min", { n: m });
}

// ---- range
export type RangeKind = "today" | "7d" | "30d" | "month" | "custom";
export type Range = { kind: RangeKind; from: string | null; to: string | null };
export function resolveRange(r: Range): { from: string | null; to: string | null } {
  const d = today();
  switch (r.kind) {
    case "today": return { from: d, to: d };
    case "7d": return { from: addDays(d, -6), to: d };
    case "30d": return { from: addDays(d, -29), to: d };
    case "month": return { from: `${d.slice(0, 8)}01`, to: d };
    default: return { from: r.from, to: r.to };
  }
}

// ---- remembered view
export function loadJSON<T>(key: string, fallback: T): T {
  try { const v = localStorage.getItem(key); return v ? { ...fallback, ...JSON.parse(v) } : fallback; } catch { return fallback; }
}
export function saveJSON(key: string, v: unknown) {
  try { localStorage.setItem(key, JSON.stringify(v)); } catch {}
}

/** Series colours (tokens in styles.css). Index 5 = "Other". */
export const SERIES = ["var(--s1)", "var(--s2)", "var(--s3)", "var(--s4)", "var(--s5)", "var(--s-other)"];
export const APP_COLOR: Record<UsageApp, string> = { claude: "var(--s1)", codex: "var(--s2)", orca: "var(--s3)" };
export const KIND_COLOR = { input: "var(--s4)", cached: "var(--s2)", cache_write: "var(--s3)", output: "var(--s1)" };
