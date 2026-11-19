import { KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api, BlocksReport, errText, onUsageChanged, onUsageSettingsChanged, UsageAnalytics, UsageApp, UsageBlock, UsageFilter,
  UsagePrice, UsageReport, UsageRow, UsageSettings,
} from "../api";
import { Icon } from "../icons";
import { t } from "../i18n";
import { Info } from "./Info";
import { Menu } from "./Menu";
import { Analysis } from "./usage/Analysis";
import { Blocks } from "./usage/Blocks";
import { Chart, Metric, StackBy } from "./usage/Chart";
import { Filters, RangePicker } from "./usage/Filters";
import { Prices, SettingsPop } from "./usage/Prices";
import { SessionPanel } from "./usage/SessionPanel";
import { UsageTable } from "./usage/Tables";
import {
  addDays, all, costOf, rowCost, duration, fmt, inAll, loadJSON, pct, Range, readPct, resolveRange, saveJSON,
  timeLabel, today, usd, writePct,
} from "./usage/util";

type Tab = "projects" | "models" | "sessions" | "analysis" | "blocks";
type View = { range: Range; app: UsageApp | null; project: string | null; model: string | null; tab: Tab; metric: Metric; stack: StackBy };
const VIEW_KEY = "usage-view";
const DEFAULT_VIEW: View = { range: { kind: "30d", from: null, to: null }, app: null, project: null, model: null, tab: "projects", metric: "tokens", stack: "app" };

/** Phase 0 kept prices in localStorage; they now live in the backend. Migrated once per app run. */
const OLD_PRICE_KEY = "usage-prices";
let pricesMigrated = false;
async function loadPrices(): Promise<Record<string, UsagePrice>> {
  const current = await api.usagePricesGet();
  if (pricesMigrated) return current;
  pricesMigrated = true;
  try {
    const raw = localStorage.getItem(OLD_PRICE_KEY);
    if (!raw) return current;
    const old = JSON.parse(raw) as Record<string, UsagePrice>;
    const missing = Object.fromEntries(Object.entries(old).filter(([k, v]) => !(k in current) && v && typeof v.input === "number"));
    if (!Object.keys(missing).length) { localStorage.removeItem(OLD_PRICE_KEY); return current; }
    const next = { ...missing, ...current };
    await api.usagePricesSet(next);
    localStorage.removeItem(OLD_PRICE_KEY); // only after the backend has them
    return next;
  } catch {
    pricesMigrated = false;
    return current;
  }
}

const TABS: { id: Tab; label: () => string }[] = [
  { id: "projects", label: () => t("Projects") },
  { id: "models", label: () => t("Models") },
  { id: "sessions", label: () => t("Sessions") },
  { id: "analysis", label: () => t("Analysis") },
  { id: "blocks", label: () => t("Blocks") },
];

export function Usage({ notify, go }: { notify?: (msg: string) => void; go?: (route: string) => void }) {
  const [view, setViewState] = useState<View>(() => loadJSON(VIEW_KEY, DEFAULT_VIEW));
  const setView = (patch: Partial<View>) => setViewState((v) => { const next = { ...v, ...patch }; saveJSON(VIEW_KEY, next); return next; });

  const [report, setReport] = useState<UsageReport | null>(null);
  const [analytics, setAnalytics] = useState<UsageAnalytics | null>(null);
  const [blocks, setBlocks] = useState<BlocksReport | null>(null);
  const [current, setCurrent] = useState<UsageBlock | null>(null);
  const [prices, setPrices] = useState<Record<string, UsagePrice>>({});
  const [settings, setSettings] = useState<UsageSettings | null>(null);
  const [error, setError] = useState("");
  const [updated, setUpdated] = useState<Date | null>(null);
  const [tick, setTick] = useState(0);
  const [busy, setBusy] = useState(false);
  const [editPrices, setEditPrices] = useState(false);
  const [session, setSession] = useState<{ key: string; label: string } | null>(null);
  const say = (m: string) => notify?.(m);

  const filter = useMemo<UsageFilter>(() => {
    const r = resolveRange(view.range);
    return { from: r.from, to: r.to, apps: view.app ? [view.app] : null, project: view.project, model: view.model };
  }, [view.range, view.app, view.project, view.model]);

  // Report: keep the previous one on screen while refetching; drop out-of-order replies.
  const seq = useRef(0);
  useEffect(() => {
    const id = ++seq.current;
    api.usageQuery(filter)
      .then((r) => { if (id === seq.current) { setReport(r); setError(""); setUpdated(new Date()); } })
      .catch((e) => { if (id === seq.current) setError(errText(e)); })
      .finally(() => { if (id === seq.current) setBusy(false); });
  }, [filter, tick]);

  const tabSeq = useRef(0);
  useEffect(() => {
    if (view.tab !== "analysis" && view.tab !== "blocks") return;
    const id = ++tabSeq.current;
    if (view.tab === "analysis") api.usageAnalytics(filter).then((a) => { if (id === tabSeq.current) setAnalytics(a); }).catch((e) => { if (id === tabSeq.current) setError(errText(e)); });
    else api.usageBlocks(filter).then((b) => { if (id === tabSeq.current) setBlocks(b); }).catch((e) => { if (id === tabSeq.current) setError(errText(e)); });
  }, [filter, view.tab, tick]);

  // The 5h block tile is about right now, whatever range is selected.
  useEffect(() => {
    api.usageBlocks({ from: addDays(today(), -1), to: null, apps: null, project: null, model: null })
      .then((b) => setCurrent(b.current)).catch(() => {});
  }, [tick]);

  useEffect(() => {
    loadPrices().then(setPrices).catch(() => {});
    const loadSettings = () => api.usageSettingsGet().then(setSettings).catch(() => {});
    loadSettings();
    // The tray menu or the mini window's × may change them.
    return onUsageSettingsChanged(loadSettings);
  }, []);

  // Auto refresh while visible: the backend watches the log folders and emits "usage-changed".
  const watch = settings?.watch ?? false;
  useEffect(() => {
    if (!watch) return;
    let timer: number | undefined;
    const bump = () => { window.clearTimeout(timer); timer = window.setTimeout(() => setTick((x) => x + 1), 800); };
    const vis = () => {
      if (document.hidden) api.usageWatch(false).catch(() => {});
      else { api.usageWatch(true).catch(() => {}); bump(); }
    };
    if (!document.hidden) api.usageWatch(true).catch(() => {});
    const off = onUsageChanged(bump);
    document.addEventListener("visibilitychange", vis);
    return () => {
      off();
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", vis);
      api.usageWatch(false).catch(() => {});
    };
  }, [watch]);

  function refresh() { setBusy(true); setTick((x) => x + 1); }

  function savePrices(next: Record<string, UsagePrice>) {
    setPrices(next);
    api.usagePricesSet(next).catch((e) => say(errText(e)));
  }
  function saveSettings(next: UsageSettings) {
    setSettings(next);
    api.usageSettingsSet(next).catch((e) => say(errText(e)));
  }
  async function exportAs(format: "csv" | "json") {
    try { say(t("Saved to {path}", { path: await api.usageExport(filter, format) })); } catch (e) { say(errText(e)); }
  }

  const closeSession = useCallback(() => setSession(null), []);
  const openSession = (r: UsageRow) => setSession({ key: r.key, label: r.label });

  function onTabKey(e: KeyboardEvent<HTMLDivElement>) {
    if ((e.target as HTMLElement).getAttribute("role") !== "tab") return;
    const i = TABS.findIndex((x) => x.id === view.tab);
    let n = -1;
    if (e.key === "ArrowRight") n = (i + 1) % TABS.length;
    else if (e.key === "ArrowLeft") n = (i - 1 + TABS.length) % TABS.length;
    else if (e.key === "Home") n = 0;
    else if (e.key === "End") n = TABS.length - 1;
    if (n < 0) return;
    e.preventDefault();
    setView({ tab: TABS[n].id });
    (e.currentTarget.querySelectorAll("[role=tab]")[n] as HTMLElement | undefined)?.focus();
  }

  const info = report
    ? t("Read from logs already on this machine; nothing is sent anywhere. {a} Claude Code sessions, {b} Codex sessions, {c} Orca sessions.", { a: report.files[0], b: report.files[1], c: report.files[2] })
      + (report.skipped ? " " + t("{n} files could not be read.", { n: report.skipped }) : "")
    : "";

  const head = (
    <div className="head u-head">
      <h1>{t("Usage")}{report && <Info text={info} />}</h1>
      <div className="acts u-acts">
        <RangePicker range={view.range} onChange={(range) => setView({ range })} />
        <span className="u-tools">
          {updated && (
            <span className="u-updated" title={watch ? t("Auto refresh is on") : undefined}>
              {watch && <i className="u-live" aria-hidden="true" />}
              {t("Updated {time}", { time: timeLabel(updated.toISOString()) })}
            </span>
          )}
          <button className="btn u-icon" onClick={refresh} disabled={busy} aria-label={t("Refresh")} title={t("Refresh")}><Icon name="refresh" /></button>
          <Menu label={<><Icon name="dl" />{t("Export")}</>} items={[
            { label: t("Export CSV"), run: () => exportAs("csv") },
            { label: t("Export JSON"), run: () => exportAs("json") },
          ]} />
          {settings && <SettingsPop settings={settings} onChange={saveSettings} onMore={go ? () => go("settings") : undefined} />}
        </span>
      </div>
    </div>
  );

  if (!report) {
    return (
      <>
        {head}
        {error ? <div className="card banner err">{error}</div> : (
          <div aria-busy="true">
            <div className="u-kpis">{[0, 1, 2, 3].map((i) => <div key={i} className="u-skel" style={{ height: 104 }} />)}</div>
            <div className="u-skel" style={{ height: 240, marginTop: 12 }} />
            <p className="muted u-note" style={{ marginTop: 12 }}>{t("Reading logs…")} {t("The first read can take a while when the logs are large. Later opens only read new sessions.")}</p>
          </div>
        )}
      </>
    );
  }

  const noLogs = report.projects.length === 0 && report.models.length === 0;
  if (noLogs) return (<>{head}<div className="card empty">{t("No token data found in the logs yet.")}</div></>);

  const total = report.total;
  const tokens = all(total);
  const prev = all(report.prev_total);
  const delta = filter.from && prev > 0 ? ((tokens - prev) / prev) * 100 : null;
  const modelCosts = report.by_model.map((m) => costOf(prices[m.key], m)).filter((c): c is number => c != null);
  const totalCost = modelCosts.reduce((a, b) => a + b, 0);
  const limit = settings?.block_limit ?? null;
  const filtered = !!(view.app || view.project || view.model);
  const cur = current ? all(current) : 0;

  return (
    <>
      {head}
      <Filters app={view.app} project={view.project} model={view.model} projects={report.projects} models={report.models} onChange={(f) => setView(f)} />
      {error && <div className="card banner err" style={{ marginBottom: 12 }}>{error}</div>}

      <>
          <div className="u-kpis">
            <div className="u-kpi">
              <small>{t("Tokens")}</small>
              <b>{fmt(tokens)}</b>
              <span>
                {delta != null && <em className={delta >= 0 ? "up" : "down"}>{delta >= 0 ? "▲" : "▼"} {pct(Math.abs(delta))}</em>}
                {delta != null ? t("vs previous period") : t("{in} in · {out} out", { in: fmt(inAll(total)), out: fmt(total.output) })}
              </span>
            </div>
            <div className="u-kpi">
              <small>{t("Estimated cost")}</small>
              {modelCosts.length > 0 ? (
                <><b>≈{usd(totalCost)}</b><span>{t("{n} of {m} models priced", { n: modelCosts.length, m: report.by_model.length })}</span></>
              ) : (
                <><b className="muted">—</b><span><button className="link" onClick={() => { setView({ tab: "models" }); setEditPrices(true); }}>{t("Enter prices")}</button></span></>
              )}
            </div>
            <div className="u-kpi">
              <small>{t("Cache hit")}</small>
              <b>{pct(readPct(total))}</b>
              <span>{total.cache_write > 0 ? t("{r} read · {w} write", { r: pct(readPct(total)), w: pct(writePct(total)) }) : t("of input read from cache")}</span>
            </div>
            <div className={`u-kpi ${current ? "live" : ""}`}>
              <small>{t("5-hour block")}</small>
              {current ? (
                <>
                  <b>{fmt(cur)}{limit ? <small className="muted"> / {fmt(limit)}</small> : null}</b>
                  {limit != null && (
                    <span className="u-meter small" role="progressbar" aria-valuemin={0} aria-valuemax={limit} aria-valuenow={cur} aria-label={t("5-hour block")}>
                      <i className={cur > limit ? "over" : cur > limit * 0.8 ? "near" : ""} style={{ width: `${Math.min(100, (cur / limit) * 100)}%` }} />
                    </span>
                  )}
                  <span>{t("{d} left", { d: duration(Date.parse(current.end) - Date.now()) })}{current.tokens_per_min != null && ` · ${t("{n}/min", { n: fmt(current.tokens_per_min) })}`}</span>
                </>
              ) : (
                <><b className="muted">—</b><span>{t("No active block")}</span></>
              )}
            </div>
          </div>

          {inAll(total) > 1000 && total.cached === 0 && (
            <div className="card alert">
              <Icon name="warn" />
              <div><b>{t("Cache hit is 0")}</b><p>{t("The gateway is not returning prompt cache, so costs may be several times higher.")}</p></div>
            </div>
          )}

          <Chart
            report={report} prices={prices} metric={view.metric} stack={view.stack}
            onMetric={(metric) => setView({ metric })} onStack={(stack) => setView({ stack })}
            onPickDay={(d) => setView({ range: { kind: "custom", from: d, to: d } })}
          />

          {tokens === 0 ? (
            <div className="card empty" style={{ marginTop: 12 }}>
              {t("No usage in this range.")}
              {filtered && <div><button className="btn small" style={{ marginTop: 10 }} onClick={() => setView({ app: null, project: null, model: null })}>{t("Clear filters")}</button></div>}
            </div>
          ) : (<>
          <div className="u-tabs" role="tablist" aria-label={t("Breakdown")} onKeyDown={onTabKey}>
            {TABS.map((x) => (
              <button key={x.id} role="tab" id={`u-tab-${x.id}`} aria-controls="u-panel" aria-selected={view.tab === x.id} tabIndex={view.tab === x.id ? 0 : -1} onClick={() => setView({ tab: x.id })}>
                {x.label()}
                {x.id === "sessions" && <span className="count">{report.sessions.length}</span>}
              </button>
            ))}
            {view.tab === "models" && (
              <button className="btn small ghost u-tab-act" onClick={() => setEditPrices((v) => !v)}>{editPrices ? t("Done") : t("Enter prices")}</button>
            )}
          </div>

          <div id="u-panel" role="tabpanel" aria-labelledby={`u-tab-${view.tab}`} className="u-panel">
            {view.tab === "projects" && (
              <UsageTable rows={report.by_project} kind="project" cost={(r) => rowCost(prices, r.models)} onOpen={(r) => setView({ project: r.key })} openLabel={() => t("Show only this project")} />
            )}
            {view.tab === "models" && (
              <>
                {editPrices && <Prices models={report.models} prices={prices} onChange={savePrices} />}
                <UsageTable rows={report.by_model} kind="model" cost={(r) => rowCost(prices, r.models ?? { [r.key]: r })} onOpen={(r) => setView({ model: r.key })} openLabel={() => t("Show only this model")} />
              </>
            )}
            {view.tab === "sessions" && (
              <>
                <p className="muted u-note">{t("The 100 largest sessions in this range. Click one to see its subagents.")}</p>
                <UsageTable rows={report.sessions} kind="session" cost={(r) => rowCost(prices, r.models)} onOpen={openSession} openLabel={() => t("Open session")} />
              </>
            )}
            {view.tab === "analysis" && (analytics ? <Analysis data={analytics} onOpenSession={openSession} /> : <div className="u-skel" style={{ height: 320 }} />)}
            {view.tab === "blocks" && (blocks ? <Blocks data={blocks} limit={limit} /> : <div className="u-skel" style={{ height: 320 }} />)}
          </div>
          </>)}
      </>

      {session && <SessionPanel sessionKey={session.key} title={session.label} prices={prices} onClose={closeSession} />}
    </>
  );
}
