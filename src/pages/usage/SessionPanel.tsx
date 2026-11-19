import { useEffect, useRef, useState } from "react";
import { api, errText, SessionDetail, UsagePrice } from "../../api";
import { t } from "../../i18n";
import { CostText } from "./Tables";
import { all, APP_COLOR, appName, fmt, inAll, pct, readPct, rowCost, sumCost } from "./util";

/** Side drawer with one session: main thread + subagents, and the split by model. */
export function SessionPanel({ sessionKey, title, prices, onClose }: {
  sessionKey: string;
  title: string;
  prices: Record<string, UsagePrice>;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [error, setError] = useState("");
  const close = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let live = true;
    setDetail(null); setError("");
    api.usageSession(sessionKey).then((d) => live && setDetail(d)).catch((e) => live && setError(errText(e)));
    return () => { live = false; };
  }, [sessionKey]);

  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", onKey);
    return () => { window.removeEventListener("keydown", onKey); back?.focus?.(); };
  }, [onClose]);

  const total = detail ? detail.threads.reduce((a, r) => a + all(r), 0) || 1 : 1;
  const threadCosts = detail?.threads.map((r) => rowCost(prices, r.models)) ?? [];
  const modelCosts = detail?.by_model.map((m) => rowCost(prices, m.models ?? { [m.key]: m })) ?? [];
  const sessionCost = sumCost(threadCosts) ?? sumCost(modelCosts);

  return (
    <div className="u-drawer-bg" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <aside className="u-drawer" role="dialog" aria-label={t("Session")}>
        <div className="u-drawer-head">
          <div>
            <small className="muted">{t("Session")}</small>
            <h2>{detail?.label ?? title}</h2>
            {detail && (
              <p className="muted">
                <i className="u-swatch" style={{ background: APP_COLOR[detail.app] }} />{appName(detail.app)}
                {detail.project && <> · {detail.project}</>}
              </p>
            )}
          </div>
          <button ref={close} className="btn small ghost" onClick={onClose} aria-label={t("Close")}>✕</button>
        </div>

        {error && <div className="card banner err">{error}</div>}
        {!detail && !error && <div className="u-skel" style={{ height: 160 }} />}
        {detail && (
          <>
            <div className="u-mini-stats">
              <div><b>{fmt(total)}</b><span>{t("tokens")}</span></div>
              <div><b>{pct(readPct(detail.threads.reduce((a, r) => ({ input: a.input + r.input, cached: a.cached + r.cached, cache_write: a.cache_write + r.cache_write, output: 0 }), { input: 0, cached: 0, cache_write: 0, output: 0 })))}</b><span>{t("cache hit")}</span></div>
              <div><b>{detail.threads.reduce((a, r) => a + r.turns, 0).toLocaleString()}</b><span>{t("turns")}</span></div>
              {sessionCost && <div><b><CostText c={sessionCost} prefix="≈" /></b><span>{t("cost")}</span></div>}
            </div>

            <div className="section-title">{t("Threads")}</div>
            <div className="card">
              {detail.threads.map((r, i) => (
                <div className="u-thread" key={r.key}>
                  <div>
                    <b>{i === 0 ? t("Main thread") : r.label}</b>
                    <p>{t("{turns} turns · {in} in · {out} out", { turns: r.turns, in: fmt(inAll(r)), out: fmt(r.output) })}</p>
                  </div>
                  <span className="u-sharebar"><i style={{ width: `${Math.max(1.5, (all(r) / total) * 100)}%`, background: i === 0 ? "var(--plum)" : "var(--s2)" }} /></span>
                  {sessionCost && <span className="muted u-thread-cost">{threadCosts[i] ? <CostText c={threadCosts[i]} prefix="≈" /> : ""}</span>}
                  <em>{fmt(all(r))}</em>
                </div>
              ))}
            </div>

            <div className="section-title">{t("By model")}</div>
            <div className="card">
              {detail.by_model.map((m, i) => (
                <div className="u-thread" key={m.key}>
                  <div><b className="u-mono">{m.label}</b><p>{t("{turns} turns · {p} cache hit", { turns: m.turns, p: pct(readPct(m)) })}</p></div>
                  <span className="muted">{modelCosts[i] ? <CostText c={modelCosts[i]} prefix="≈" /> : ""}</span>
                  <em>{fmt(all(m))}</em>
                </div>
              ))}
            </div>
          </>
        )}
      </aside>
    </div>
  );
}
