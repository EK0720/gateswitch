import { useEffect, useState } from "react";
import { api, Check, errText } from "../api";
import { Icon } from "../icons";
import { getLang, Lang, setLang, t, tr } from "../i18n";
import { Info } from "./Info";

export function Settings({ dark, onToggleTheme, notify, go, onLang }: { dark: boolean; onToggleTheme: () => void; notify: (t: string) => void; go: (route: string) => void; onLang?: (l: Lang) => void }) {
  const lang = getLang();
  function pickLang(l: Lang) {
    if (l === lang) return;
    if (onLang) onLang(l); else setLang(l);
  }
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [stale, setStale] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [report, setReport] = useState("");

  function load() {
    setBusy(true); setError("");
    Promise.all([api.doctor(), api.staleEnv()])
      .then(([c, s]) => { setChecks(c); setStale(s); })
      .catch((e) => setError(errText(e)))
      .finally(() => setBusy(false));
  }
  useEffect(load, []);

  async function clear(name: string) {
    try {
      await api.clearStaleEnv(name);
      notify(t("Removed {name}", { name }));
      load();
    } catch (e) {
      notify(errText(e));
    }
  }

  async function makeReport() {
    try {
      setReport(await api.supportReport());
    } catch (e) {
      notify(errText(e));
    }
  }

  async function copyReport() {
    try {
      await navigator.clipboard.writeText(report);
      notify(t("Report copied"));
    } catch {
      notify(t("Could not copy. Select the text and press Ctrl+C."));
    }
  }

  const bad = checks?.filter((c) => c.level !== "ok") ?? [];
  const ok = checks?.filter((c) => c.level === "ok") ?? [];

  return (
    <>
      <div className="head">
        <h1>{t("Settings")}</h1>
        <div className="acts">
          <button className="btn primary" disabled={busy} onClick={load}>{busy ? t("Testing…") : t("Check again")}</button>
        </div>
      </div>

      <div className="section-title">{t("Appearance")}</div>
      <div className="card">
        <div className="issue">
          <div><b>{t("Theme")}</b><p>{dark ? t("Dark") : t("Light")}</p></div>
          <button className="btn small" onClick={onToggleTheme}><Icon name={dark ? "sun" : "moon"} />{dark ? t("Light") : t("Dark")}</button>
        </div>
        <div className="issue">
          <div><b>{t("Language")}</b></div>
          <div className="seg" role="group" aria-label={t("Language")}>
            <button aria-pressed={lang === "en"} onClick={() => pickLang("en")}>English</button>
            <button aria-pressed={lang === "vi"} onClick={() => pickLang("vi")}>Tiếng Việt</button>
          </div>
        </div>
      </div>

      {stale.length > 0 && (
        <>
          <div className="section-title">{t("Stale environment variables causing errors")}</div>
          <div className="card">
            {stale.map((name) => (
              <div className="issue" key={name}>
                <i className="dot bad" />
                <div>
                  <b>{name}</b>
                  <p>{t("Makes apps send an extra field the gateway rejects (error 400). Remove it so the apps work correctly.")}</p>
                </div>
                <button className="btn small" onClick={() => clear(name)}>{t("Remove")}</button>
              </div>
            ))}
          </div>
        </>
      )}

      <div className="section-title">Doctor {checks && <span className="count">{bad.length}</span>}</div>
      {error && <div className="card banner err">{error}</div>}
      {!checks && !error && <div className="card empty">{t("Testing…")}</div>}
      {checks && bad.length === 0 && <div className="card empty">{t("No problems found.")}</div>}
      {bad.length > 0 && (
        <div className="card">
          {bad.map((c, i) => (
            <div className="issue" key={i}>
              <i className={`dot ${c.level === "bad" ? "bad" : "warn"}`} />
              <div><b>{tr(c.title)}</b><p>{c.app} — {tr(c.detail)}</p></div>
              {c.fix ? (
                <button className="btn small" onClick={() => {
                  const [kind, arg] = c.fix!.split(":");
                  if (kind === "env") clear(arg); else go(arg);
                }}>{c.fix.startsWith("env:") ? t("Fix") : t("Open to fix")}</button>
              ) : <span />}
            </div>
          ))}
        </div>
      )}

      {ok.length > 0 && (
        <details className="adv" style={{ marginTop: 18 }}>
          <summary><Icon name="chev" />{t("{n} checks OK", { n: ok.length })}</summary>
          <div className="card" style={{ marginTop: 8 }}>
            {ok.map((c, i) => (
              <div className="issue" key={i}>
                <i className="dot ok" />
                <div><b>{tr(c.title)}</b><p>{tr(c.detail)}</p></div>
                <span />
              </div>
            ))}
          </div>
        </details>
      )}

      <div className="section-title">{t("Support report")}</div>
      <div className="card">
        <div className="issue">
          <div>
            <b>{t("Create a report to send when you need help")}<Info text={t("Includes Doctor results, the current configuration and recent history. Keys and user folders are masked. Review it before sending.")} /></b>
          </div>
          <span />
          <button className="btn small" onClick={makeReport}>{report ? t("Regenerate") : t("Create report")}</button>
        </div>
        {report && (
          <>
            <pre className="report">{report}</pre>
            <div className="form-acts" style={{ padding: "12px 18px" }}>
              <button className="btn ghost small" onClick={() => setReport("")}>{t("Close")}</button>
              <button className="btn primary small" onClick={copyReport}>{t("Copy")}</button>
            </div>
          </>
        )}
      </div>
    </>
  );
}
