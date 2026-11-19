// Mini window ("overlay"): today's tokens and the current 5-hour block.
// Opened by the backend when the user turns it on in Settings › Notifications & background; the
// same frontend bundle, picked by window label in main.tsx.
import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, inTauri, onUsageChanged, onUsageSettingsChanged, UsageBlock, UsagePrice, UsageReport, UsageSettings } from "../api";
import { setLang, t } from "../i18n";
import { addDays, all, costOf, duration, fmt, pct, readPct, today, usd } from "../pages/usage/util";

const EVERY = 30_000;

/** Same theme as the main window: it is stored in localStorage, which both windows share. */
function applyTheme() {
  let dark = matchMedia("(prefers-color-scheme: dark)").matches;
  try {
    const saved = localStorage.getItem("theme");
    if (saved) dark = saved === "dark";
  } catch {}
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

export function Overlay() {
  const [settings, setSettings] = useState<UsageSettings | null>(null);
  const [report, setReport] = useState<UsageReport | null>(null);
  const [block, setBlock] = useState<UsageBlock | null>(null);
  const [prices, setPrices] = useState<Record<string, UsagePrice>>({});
  const [tick, setTick] = useState(0);
  const [, setLangTick] = useState(0);
  const down = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    applyTheme();
    document.body.classList.add("ov-body");
    // Theme or language changed in the main window.
    const onStorage = (e: StorageEvent) => {
      if (e.key === "theme") applyTheme();
      if (e.key === "lang" && (e.newValue === "en" || e.newValue === "vi")) { setLang(e.newValue); setLangTick((x) => x + 1); }
    };
    window.addEventListener("storage", onStorage);
    const id = window.setInterval(() => setTick((x) => x + 1), EVERY);
    const loadSettings = () => api.usageSettingsGet().then(setSettings).catch(() => {});
    loadSettings();
    const offSettings = onUsageSettingsChanged(loadSettings);
    return () => { window.removeEventListener("storage", onStorage); window.clearInterval(id); offSettings(); };
  }, []);

  // Live updates through the shared log watcher while this window exists.
  const watch = settings?.watch ?? false;
  useEffect(() => {
    if (!watch) return;
    let timer: number | undefined;
    api.usageWatch(true).catch(() => {});
    const off = onUsageChanged(() => { window.clearTimeout(timer); timer = window.setTimeout(() => setTick((x) => x + 1), 800); });
    return () => { off(); window.clearTimeout(timer); api.usageWatch(false).catch(() => {}); };
  }, [watch]);

  useEffect(() => {
    const d = today();
    api.usageQuery({ from: d, to: d, apps: null, project: null, model: null }).then(setReport).catch(() => {});
    api.usageBlocks({ from: addDays(d, -1), to: null, apps: null, project: null, model: null }).then((b) => setBlock(b.current)).catch(() => {});
    api.usagePricesGet().then(setPrices).catch(() => {});
  }, [tick]);

  const total = report?.total;
  const costs = (report?.by_model ?? []).map((m) => costOf(prices[m.key], m)).filter((c): c is number => c != null);
  const cost = costs.length ? costs.reduce((a, b) => a + b, 0) : null;
  const limit = settings?.block_limit ?? null;
  const used = block ? all(block) : 0;
  const ratio = limit ? used / limit : 0;
  const level = ratio >= 1 ? "over" : ratio >= 0.8 ? "near" : "";

  // Drag only once the mouse moves, so a plain double-click opens the main window.
  function onMouseDown(e: React.MouseEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    down.current = { x: e.screenX, y: e.screenY };
  }
  function onMouseMove(e: React.MouseEvent) {
    const d = down.current;
    if (!d || !(e.buttons & 1)) { down.current = null; return; }
    if (Math.abs(e.screenX - d.x) + Math.abs(e.screenY - d.y) > 3) {
      down.current = null;
      if (inTauri) getCurrentWindow().startDragging().catch(() => {});
    }
  }
  function close() {
    // The backend turns the setting off when this window is closed.
    if (inTauri) getCurrentWindow().close().catch(() => {});
  }

  return (
    <div className="ov" onMouseDown={onMouseDown} onMouseMove={onMouseMove} onMouseUp={() => { down.current = null; }}
      onDoubleClick={(e) => { if (!(e.target as HTMLElement).closest("button")) api.usageOpenMain().catch(() => {}); }}
      title={t("Double-click to open Gateswitch")}>
      <div className="ov-top">
        <span className="ov-label">{t("Today")}</span>
        <b className="ov-big">{total ? fmt(all(total)) : "…"}</b>
        {cost != null && <span className="ov-cost">≈{usd(cost)}</span>}
        <button className="ov-x" aria-label={t("Close mini window")} title={t("Close mini window")} onClick={close}>×</button>
      </div>
      <div className="ov-sub muted">{total ? `${t("Cache hit")} ${pct(readPct(total))}` : " "}</div>
      <div className="ov-block">
        <div className="ov-row">
          <span className="ov-label">{t("5-hour block")}</span>
          <span className="ov-val">
            {block ? <>{fmt(used)}{limit ? <> · <b className={level}>{Math.round(ratio * 100)}%</b></> : null}</> : <span className="muted">{t("No active block")}</span>}
          </span>
        </div>
        {block && limit ? (
          <span className="u-meter small ov-meter"><i className={level} style={{ width: `${Math.max(1, Math.min(100, ratio * 100))}%` }} /></span>
        ) : <span className="ov-meter-gap" />}
        <small className="muted">
          {block ? t("{d} left", { d: duration(Date.parse(block.end) - Date.now()) }) : " "}
          {block && !limit ? <> · {t("No limit set")}</> : null}
        </small>
      </div>
    </div>
  );
}
