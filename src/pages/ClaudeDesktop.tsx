import { useEffect, useState } from "react";
import { api, DesktopCurrent, errText, Provider, Scan } from "../api";
import { Icon } from "../icons";
import { locale, t, tr } from "../i18n";
import { Info } from "./Info";
import { Menu } from "./Menu";
import { groupOf, statusText } from "./ModelTable";
import { HeaderHelp } from "./HeaderHelp";

const AUTHS = [["bearer", "Bearer"], ["x-api-key", "x-api-key"]] as const;

const host = (url: string) => url.replace(/^https?:\/\//, "");

export function ClaudeDesktop({ scan, notify, goProviders }: { scan: Scan | null; notify: (t: string) => void; goProviders: () => void }) {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [current, setCurrent] = useState<DesktopCurrent | null>(null);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [auth, setAuth] = useState("bearer");
  const [oneM, setOneM] = useState(true);
  const [allModels, setAllModels] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);
  const [picked, setPicked] = useState<string[]>([]);

  const installed = scan?.tools.find((x) => x.id === "claude-desktop")?.installed;

  useEffect(() => {
    Promise.all([api.listProviders(), api.claudeDesktopCurrent()]).then(([ps, cur]) => {
      setProviders(ps);
      setCurrent(cur);
      const pick = ps.find((p) => p.has_key) ?? ps[0];
      if (pick) setProviderId(pick.id);
      if (pick?.models.length) setModel(pick.models.find((m) => /claude/i.test(m)) ?? pick.models[0]);
    });
  }, []);

  const provider = providers?.find((p) => p.id === providerId);
  const models = provider?.models ?? [];
  useEffect(() => {
    if (provider && models.length && !models.includes(model)) {
      setModel(models.find((m) => /claude.*opus/i.test(m)) ?? models.find((m) => /claude/i.test(m)) ?? models[0]);
    }
  }, [providerId, models.length]);

  const probe = provider?.probes?.[model];
  const sameGroup = provider && model ? models.filter((m) => m !== model && groupOf(provider, m) === groupOf(provider, model)) : [];
  const alt = provider ? sameGroup.find((m) => provider.probes?.[m]?.status === "ok") ?? sameGroup.find((m) => provider.probes?.[m]?.status !== "quota") : undefined;

  const [restarting, setRestarting] = useState(false);
  async function restart() {
    if (!confirm(t("Quit and reopen Claude Desktop? Running chats and sessions will stop."))) return;
    setRestarting(true);
    try {
      notify(await api.claudeDesktopRestart());
    } catch (e) {
      notify(errText(e));
    } finally {
      setRestarting(false);
    }
  }

  async function setup() {
    setBusy(true); setError(""); setDone(false);
    try {
      const r = await api.claudeDesktopSetup({ provider_id: providerId, model, auth, one_m: oneM, all_models: allModels });
      setCurrent(await api.claudeDesktopCurrent());
      setDone(true);
      notify(r.message);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  async function remove(ids: string[]) {
    if (!ids.length) return;
    const names = current?.entries.filter((e) => ids.includes(e.id)).map((e) => e.name || e.id.slice(0, 8)).join(", ");
    if (!confirm(t("Remove {n} configuration(s) from Claude Desktop: {names}? You can undo this on the Restore page.", { n: ids.length, names: names ?? "" }))) return;
    setError("");
    try {
      const r = await api.claudeDesktopRemove(ids);
      setCurrent(await api.claudeDesktopCurrent());
      setPicked([]);
      notify(r.message);
    } catch (e) {
      setError(errText(e));
    }
  }

  if (providers && providers.length === 0) {
    return (
      <>
        <div className="head"><h1>Claude Desktop</h1></div>
        <div className="card empty">
          {t("No providers yet.")}
          <div style={{ marginTop: 12 }}><button className="btn primary" onClick={goProviders}><Icon name="plus" />{t("Add provider")}</button></div>
        </div>
      </>
    );
  }

  const entries = current?.entries ?? [];
  const foreign = entries.filter((e) => !e.ours).map((e) => e.id);
  const toggle = (id: string) => setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));

  return (
    <>
      <div className="head">
        <h1>Claude Desktop<Info text={t("Writes to the third-party configLibrary. The app's Developer → Configure Third-Party Inference window shows the same configurations. The app reads them only at launch.")} /></h1>
        <div className="acts">
          <Menu items={[
            { label: t("Remove external configurations ({n})", { n: foreign.length }), run: () => remove(foreign), danger: true, hidden: !foreign.length },
          ]} />
          <button className="btn primary" disabled={busy || !provider?.has_key || !model} onClick={setup}>
            {busy ? t("Setting up…") : t("Set up")}
          </button>
        </div>
      </div>

      {installed === false && (
        <div className="card alert"><Icon name="warn" /><div>{t("Claude Desktop not found. You can still set it up; the app uses this configuration once installed.")}</div></div>
      )}
      {current?.warnings.map((w) => (
        <div key={w} className="card alert"><Icon name="warn" /><div>{tr(w)}</div></div>
      ))}

      {probe?.status === "quota" && (
        <div className="card alert">
          <Icon name="warn" />
          <div>
            <b>{t("{model} is out of quota", { model })}</b>
            <div className="muted">{t("Checked at")} {new Date(probe.checked_at).toLocaleString(locale())}{probe.message ? `: ${probe.message}` : ""}</div>
          </div>
          {alt && <button className="btn small" style={{ marginLeft: "auto" }} onClick={() => setModel(alt)}>{t("Switch to another model in the group")}</button>}
        </div>
      )}

      {entries.length > 0 && (
        <div className="card" style={{ marginTop: 14 }}>
          <div className="cap-head">
            <b>{t("Existing configurations")}</b>
            {picked.length > 0 && (
              <div className="cap-acts">
                <button className="btn small" onClick={() => setPicked([])}>{t("Clear selection")}</button>
                <button className="btn small danger" onClick={() => remove(picked)}>{t("Remove {n}", { n: picked.length })}</button>
              </div>
            )}
          </div>
          <table className="models">
            <thead><tr><th style={{ width: 44 }} /><th style={{ width: "38%" }}>{t("Name")}</th><th>Gateway</th><th>{t("Default model")}</th></tr></thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.id} className={e.applied ? "grp" : ""}>
                  <td><input type="checkbox" className="row-check" checked={picked.includes(e.id)} onChange={() => toggle(e.id)} aria-label={t("Select {name}", { name: e.name || e.id.slice(0, 8) })} /></td>
                  <td className="mname">
                    {e.name || e.id.slice(0, 8)}
                    {e.applied && <b className="tag">{t("In use")}</b>}
                    {e.headers > 0 && <span className="tag muted-tag" title={t("Sends {n} custom headers", { n: e.headers })}>{t("{n} headers", { n: e.headers })}</span>}
                    {!e.ours && <span className="tag muted-tag" title={t("Created in the app or by another tool")}>{t("External")}</span>}
                  </td>
                  <td>{host(e.base_url) || "—"}{!e.has_key && <span className="muted"> · {t("key missing")}</span>}</td>
                  <td>{e.default_model || <span className="muted">{t("discovered via /models")}</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div className="card form" style={{ marginTop: 14 }}>
        <div className="field">
          <label htmlFor="cd-gw">Gateway</label>
          <select id="cd-gw" className="select" value={providerId} onChange={(e) => setProviderId(e.target.value)}>
            {providers?.map((p) => (
              <option key={p.id} value={p.id} disabled={!p.has_key}>
                {p.name} — {host(p.base_url.replace(/\/v1$/, ""))}{p.has_key ? "" : ` (${t("no key")})`}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="cd-model">{t("Default model")}{!models.length ? <small>{t("Models not fetched, type one")}</small> : probe && probe.status !== "quota" ? <small>{statusText(probe)}</small> : null}</label>
          {models.length ? (
            <select id="cd-model" className="select" value={model} onChange={(e) => setModel(e.target.value)}>
              {!models.includes(model) && model && <option value={model}>{model}</option>}
              {models.map((m) => <option key={m}>{m}</option>)}
            </select>
          ) : (
            <input id="cd-model" className="input" placeholder="cc/claude-opus-5.5[1m]" value={model} onChange={(e) => setModel(e.target.value.trim())} />
          )}
        </div>
        <div className="field">
          <label>{t("Full model list")}<small>{allModels ? t("{n} models in the menu", { n: models.length || 1 }) : t("Default model only")}</small></label>
          <input type="checkbox" className="switch" checked={allModels} onChange={(e) => setAllModels(e.target.checked)} aria-label={t("Full model list")} />
        </div>
        <details className="adv">
          <summary><Icon name="chev" />{t("Advanced")}</summary>
          <div className="field">
            <label>{t("Declare 1M context")}<small>{t("Models with the [1m] suffix")}</small></label>
            <input type="checkbox" className="switch" checked={oneM} onChange={(e) => setOneM(e.target.checked)} aria-label={t("Declare 1M context")} />
          </div>
          <div className="field">
            <label>{t("Key header")}<small>{t("Switch if the gateway returns 401")}</small></label>
            <div className="seg" role="group">
              {AUTHS.map(([v, label]) => <button key={v} aria-pressed={auth === v} onClick={() => setAuth(v)}>{label}</button>)}
            </div>
          </div>
          <HeaderHelp provider={provider} onChange={(p) => setProviders((ps) => ps?.map((x) => (x.id === p.id ? { ...x, ...p } : x)) ?? ps)} />
        </details>
      </div>

      {error && <div className="card banner err" style={{ marginTop: 14 }}>{error}</div>}

      {installed && (
        <div className="probe-bar restart-bar">
          <span className="muted">{t("Claude Desktop reads its config only at launch.")}</span>
          <button className="btn small" disabled={restarting} onClick={restart}>{restarting ? t("Restarting…") : t("Restart Claude Desktop")}</button>
        </div>
      )}

      {done && (
        <div className="card next on">
          <span className="check"><Icon name="check" /></span>
          <div>
            <b>{t("Claude Desktop is set up")}</b>
            <ul>
              <li>{t("Quit the app fully and reopen it")} <span>{t("or use the restart bar above")}</span></li>
            </ul>
          </div>
        </div>
      )}
    </>
  );
}
