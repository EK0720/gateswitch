import { ReactNode, useState } from "react";
import type { UsageRow } from "../../api";
import { t } from "../../i18n";
import { ago, all, APP_COLOR, appName, fmt, RowCost, usd } from "./util";

/** "≈$1.23", with a subtle mark when some models in it have no price. */
export function CostText({ c, prefix = "" }: { c: RowCost | null; prefix?: string }) {
  if (!c) return <span className="muted">—</span>;
  return (
    <span className={c.partial ? "u-cost partial" : "u-cost"} title={c.partial ? t("Some models here have no price, so the real cost is higher") : undefined}>
      {prefix}{usd(c.usd)}{c.partial && <sup aria-hidden="true">+</sup>}
    </span>
  );
}

type SortKey = "label" | "turns" | "input" | "cached" | "cache_write" | "output" | "total" | "cost" | "last";

type Col = { key: SortKey; label: string; num?: boolean; get: (r: UsageRow) => number | string; show: (r: UsageRow) => ReactNode };

/** Sortable token table for projects, models and sessions. */
export function UsageTable({ rows, kind, cost, onOpen, openLabel }: {
  rows: UsageRow[];
  kind: "project" | "model" | "session";
  /** ≈$ per row (from its per-model split), or null when unpriced. */
  cost?: (r: UsageRow) => RowCost | null;
  onOpen?: (r: UsageRow) => void;
  openLabel?: (r: UsageRow) => string;
}) {
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "total", desc: true });
  const grand = rows.reduce((a, r) => a + all(r), 0) || 1;
  const anyWrite = rows.some((r) => r.cache_write > 0);
  const anyCost = !!cost && rows.some((r) => cost(r) != null);

  const cols: Col[] = [
    { key: "turns", label: t("Turns"), num: true, get: (r) => r.turns, show: (r) => r.turns.toLocaleString() },
    { key: "input", label: t("Input"), num: true, get: (r) => r.input, show: (r) => fmt(r.input) },
    { key: "cached", label: t("Cache read"), num: true, get: (r) => r.cached, show: (r) => fmt(r.cached) },
    ...(anyWrite ? [{ key: "cache_write" as const, label: t("Cache write"), num: true, get: (r: UsageRow) => r.cache_write, show: (r: UsageRow) => (r.cache_write ? fmt(r.cache_write) : <span className="muted">—</span>) }] : []),
    { key: "output", label: t("Output"), num: true, get: (r) => r.output, show: (r) => fmt(r.output) },
    { key: "total", label: t("Total"), num: true, get: (r) => all(r), show: (r) => <b>{fmt(all(r))}</b> },
    ...(anyCost ? [{ key: "cost" as const, label: "≈$", num: true, get: (r: UsageRow) => cost!(r)?.usd ?? -1, show: (r: UsageRow) => <CostText c={cost!(r)} /> }] : []),
    ...(kind === "session" ? [{ key: "last" as const, label: t("Last"), get: (r: UsageRow) => r.last, show: (r: UsageRow) => <span className="muted">{ago(r.last)}</span> }] : []),
  ];

  const getter = sort.key === "label" ? (r: UsageRow) => r.label.toLowerCase() : cols.find((c) => c.key === sort.key)?.get ?? ((r: UsageRow) => all(r));
  const sorted = [...rows].sort((a, b) => {
    const x = getter(a), y = getter(b);
    const c = x < y ? -1 : x > y ? 1 : 0;
    return sort.desc ? -c : c;
  });

  function th(key: SortKey, label: string, num?: boolean) {
    const on = sort.key === key;
    return (
      <th key={key} className={num ? "num" : ""} aria-sort={on ? (sort.desc ? "descending" : "ascending") : "none"}>
        <button className="u-sort" onClick={() => setSort({ key, desc: on ? !sort.desc : key !== "label" })}>
          {label}<span aria-hidden="true">{on ? (sort.desc ? "▾" : "▴") : ""}</span>
        </button>
      </th>
    );
  }

  const nameHead = kind === "project" ? t("Project") : kind === "model" ? t("Model") : t("Session");

  return (
    <div className="card tablewrap">
      <table className="u-table">
        <thead>
          <tr>
            {th("label", nameHead)}
            {cols.map((c) => th(c.key, c.label, c.num))}
            <th className="u-share-h"><span className="sr">{t("Share")}</span></th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((r) => {
            const share = (all(r) / grand) * 100;
            return (
              <tr key={r.key}>
                <td className="u-name">
                  {onOpen ? (
                    <button className="u-link" onClick={() => onOpen(r)} title={openLabel?.(r)}>{r.label}</button>
                  ) : <span title={r.label}>{r.label}</span>}
                  {kind === "session" && (
                    <small>
                      {r.app && <><i className="u-swatch" style={{ background: APP_COLOR[r.app] }} />{appName(r.app)}</>}
                      {r.project && <> · {r.project}</>}
                      {r.subagents > 0 && <> · {r.subagents === 1 ? t("1 subagent") : t("{n} subagents", { n: r.subagents })}</>}
                    </small>
                  )}
                </td>
                {cols.map((c) => <td key={c.key} className={c.num ? "num" : ""}>{c.show(r)}</td>)}
                <td className="u-share" title={`${share.toFixed(1)}%`}>
                  <span className="u-sharebar"><i style={{ width: `${Math.max(1.5, share)}%` }} /></span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
