import type { UsageAnalytics, UsageRow } from "../../api";
import { t, tr } from "../../i18n";
import { all, APP_COLOR, appName, fmt, KIND_COLOR, pct } from "./util";

function Donut({ parts }: { parts: { v: number; color: string }[] }) {
  const R = 44, C = 2 * Math.PI * R;
  let off = 0;
  return (
    <svg className="u-donut" viewBox="0 0 120 120" aria-hidden="true">
      <circle cx="60" cy="60" r={R} className="u-donut-track" />
      {parts.map((p, i) => {
        const len = p.v * C;
        const el = len > 0 ? <circle key={i} cx="60" cy="60" r={R} stroke={p.color} strokeDasharray={`${len} ${C - len}`} strokeDashoffset={-off} transform="rotate(-90 60 60)" /> : null;
        off += len;
        return el;
      })}
    </svg>
  );
}

export function Analysis({ data, onOpenSession }: { data: UsageAnalytics; onOpenSession: (r: UsageRow) => void }) {
  const kinds = [
    { key: "input" as const, label: t("Input (uncached)"), color: KIND_COLOR.input },
    { key: "cached" as const, label: t("Cache read"), color: KIND_COLOR.cached },
    { key: "cache_write" as const, label: t("Cache write"), color: KIND_COLOR.cache_write },
    { key: "output" as const, label: t("Output"), color: KIND_COLOR.output },
  ];
  const hourMax = Math.max(1, ...data.by_hour);
  const mainAll = all(data.main), subAll = all(data.subagents);
  const ms = mainAll + subAll || 1;
  const peak = data.by_hour.indexOf(Math.max(...data.by_hour));

  return (
    <>
      {data.insights.length > 0 && (
        <div className="u-insights">
          {data.insights.map((x, i) => (
            <div key={i} className={`card u-insight lvl-${x.level}`}>
              <b>{tr(x.title)}</b>
              <p>{tr(x.detail)}</p>
            </div>
          ))}
        </div>
      )}

      <div className="u-grid2">
        <div className="card u-pane">
          <h3>{t("Where tokens go")}</h3>
          <div className="u-share-wrap">
            <Donut parts={kinds.map((k) => ({ v: data.share[k.key], color: k.color }))} />
            <ul className="u-kinds">
              {kinds.map((k) => (
                <li key={k.key}><i style={{ background: k.color }} />{k.label}<em>{pct(data.share[k.key] * 100)}</em></li>
              ))}
            </ul>
          </div>
        </div>

        <div className="card u-pane">
          <h3>{t("Main thread vs subagents")}</h3>
          {[{ label: t("Main threads"), v: mainAll, turns: data.main.turns, color: "var(--plum)" }, { label: t("Subagents"), v: subAll, turns: data.subagents.turns, color: "var(--s2)" }].map((r) => (
            <div className="u-vs" key={r.label}>
              <div><span>{r.label}</span><em>{fmt(r.v)} · {pct((r.v / ms) * 100)}</em></div>
              <span className="u-sharebar"><i style={{ width: `${Math.max(1, (r.v / ms) * 100)}%`, background: r.color }} /></span>
              <small className="muted">{t("{n} turns", { n: r.turns.toLocaleString() })}</small>
            </div>
          ))}
        </div>
      </div>

      <div className="card u-pane">
        <h3>{t("Time of day")}<span className="muted">{t("Busiest at {h}:00", { h: String(peak).padStart(2, "0") })}</span></h3>
        <div className="u-hours" role="list">
          {data.by_hour.map((v, h) => (
            <i key={h} role="listitem" title={`${String(h).padStart(2, "0")}:00 · ${fmt(v)}`} aria-label={`${String(h).padStart(2, "0")}:00 ${fmt(v)}`}
              style={{ background: v === 0 ? undefined : `color-mix(in srgb, var(--plum) ${Math.round(12 + (v / hourMax) * 88)}%, transparent)` }} />
          ))}
        </div>
        <div className="u-hours-axis"><span>00</span><span>06</span><span>12</span><span>18</span><span>23</span></div>
      </div>

      <div className="u-grid2">
        <div className="card u-pane">
          <h3>{t("Cache expiries")}</h3>
          <div className="u-big">{data.cache_expiries.toLocaleString()}</div>
          <p className="muted u-note">{t("Turns that came more than 5 minutes after the previous one in the same thread and had to write the cache again. Shorter pauses keep the cache warm.")}</p>
        </div>
        <div className="card u-pane">
          <h3>{t("Biggest contexts")}</h3>
          {data.biggest_context.length === 0 && <p className="muted u-note">{t("Nothing yet.")}</p>}
          <ol className="u-ctx">
            {data.biggest_context.map((r) => (
              <li key={r.key}>
                <button className="u-link" onClick={() => onOpenSession(r)}>{r.label}</button>
                <small>{r.app && <><i className="u-swatch" style={{ background: APP_COLOR[r.app] }} />{appName(r.app)}</>}{r.project ? ` · ${r.project}` : ""}</small>
                <em>{fmt(r.max_context)}</em>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </>
  );
}
