import { useEffect, useMemo, useState } from "react";
import { api, ClaudeCurrent, errText, Provider, Scan } from "../api";
import { Icon } from "../icons";
import { locale, t, tr } from "../i18n";
import { groupOf, statusText } from "./ModelTable";
import { HeaderHelp } from "./HeaderHelp";

const EFFORTS = [["default", "Default"], ["low", "low"], ["medium", "medium"], ["high", "high"], ["max", "max"]] as const;
const effortLabel = (v: string, label: string) => (v === "default" ? t(label) : label);
const AUTHS = [["bearer", "Bearer"], ["x-api-key", "x-api-key"]] as const;

const root = (url: string) => url.replace(/\/+$/, "").replace(/\/v1$/, "");
const host = (url: string) => url.replace(/^https?:\/\//, "");

export function ClaudeCode({ scan, notify, goProviders }: { scan: Scan | null; notify: (t: string) => void; goProviders: () => void }) {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [current, setCurrent] = useState<ClaudeCurrent | null>(null);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [picker, setPicker] = useState(false);
  const [auth, setAuth] = useState("bearer");
  const [effort, setEffort] = useState("default");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);

  useEffect(() => {
    Promise.all([api.listProviders(), api.claudeCurrent()]).then(([ps, cur]) => {
      setProviders(ps);
      setCurrent(cur);
      const fromConfig = cur.base_url ? ps.find((p) => root(p.base_url) === root(cur.base_url)) : undefined;
      const pick = fromConfig ?? ps.find((p) => p.has_key) ?? ps[0];
      if (pick) setProviderId(pick.id);
      if (cur.model) setModel(cur.model);
      if (cur.auth) setAuth(cur.auth);
      if (cur.effort) setEffort(cur.effort);
      setPicker(cur.picker);
    });
  }, []);

  const provider = providers?.find((p) => p.id === providerId);
  const models = provider?.models ?? [];
  useEffect(() => {
    if (provider && models.length && !models.includes(model)) {
      setModel(models.find((m) => /claude.*opus/i.test(m)) ?? models.find((m) => /claude/i.test(m)) ?? models[0]);
    }
  }, [providerId, models.length]);

  const ides = useMemo(() => (scan?.ides ?? []).filter((i) => i.uses.includes("claude")), [scan]);
  const probe = provider?.probes?.[model];
  const sameGroup = provider && model ? models.filter((m) => m !== model && groupOf(provider, m) === groupOf(provider, model)) : [];
  const alt = provider ? sameGroup.find((m) => provider.probes?.[m]?.status === "ok") ?? sameGroup.find((m) => provider.probes?.[m]?.status !== "quota") : undefined;

  async function setup() {
    setBusy(true); setError(""); setDone(false);
    try {
      const r = await api.claudeSetup({ provider_id: providerId, model, auth, effort: effort === "default" ? null : effort, picker });
      setCurrent(await api.claudeCurrent());
      setDone(true);
      notify(r.message);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  if (providers && providers.length === 0) {
    return (
      <>
        <div className="head"><h1>Claude Code</h1></div>
        <div className="card empty">
          {t("No providers yet.")}
          <div style={{ marginTop: 12 }}><button className="btn primary" onClick={goProviders}><Icon name="plus" />{t("Add provider")}</button></div>
        </div>
      </>
    );
  }

  const active = !!provider && !!current?.base_url && root(current.base_url) === root(provider.base_url) && current.model === model;

  return (
    <>
      <div className="head">
        <h1>Claude Code</h1>
        <div className="acts">
          <button className="btn primary" disabled={busy || !provider?.has_key || !model} onClick={setup}>
            {busy ? t("Setting up…") : active ? t("Set up again") : t("Set up Claude Code")}
          </button>
        </div>
      </div>

      {current?.base_url && (
        <div className="now">
          <i className={`dot ${current.auth ? "ok" : "warn"}`} />{t("In use")} <b>{current.model || t("default model")}</b>
          <span className="muted">{t("via {host}", { host: host(current.base_url) })}</span>
        </div>
      )}

      {current?.warnings.map((w) => (
        <div key={w} className="card alert"><Icon name="warn" /><div>{tr(w)}</div></div>
      ))}

      {probe?.status === "quota" && (
        <div className="card alert">
          <Icon name="warn" />
          <div>
            <b>{t("{model} is out of quota", { model })}</b>
            <div className="muted">{t("Checked at {time}", { time: new Date(probe.checked_at).toLocaleString(locale()) })}{probe.message ? `: ${tr(probe.message)}` : ""}</div>
          </div>
          {alt && <button className="btn small" style={{ marginLeft: "auto" }} onClick={() => setModel(alt)}>{t("Switch to another model in the group")}</button>}
        </div>
      )}

      <div className="card form" style={{ marginTop: 14 }}>
        <div className="field">
          <label htmlFor="cc-gw">Gateway</label>
          <select id="cc-gw" className="select" value={providerId} onChange={(e) => setProviderId(e.target.value)}>
            {providers?.map((p) => (
              <option key={p.id} value={p.id} disabled={!p.has_key}>
                {p.name} — {host(root(p.base_url))}{p.has_key ? "" : ` ${t("(no key)")}`}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="cc-model">Model{!models.length ? <small>{t("Models not fetched, type one")}</small> : probe && probe.status !== "quota" ? <small>{statusText(probe)}</small> : null}</label>
          {models.length ? (
            <select id="cc-model" className="select" value={model} onChange={(e) => setModel(e.target.value)}>
              {!models.includes(model) && model && <option value={model}>{model}</option>}
              {models.map((m) => <option key={m}>{m}</option>)}
            </select>
          ) : (
            <input id="cc-model" className="input" placeholder="claude-opus-5.5[1m]" value={model} onChange={(e) => setModel(e.target.value.trim())} />
          )}
        </div>
        <div className="field">
          <label>{t("Only gateway models in /model")}<small>{picker ? t("{n} models, Claude's built-in models hidden", { n: models.length || 1 }) : t("Gateway models appear below the built-in models")}</small></label>
          <input type="checkbox" className="switch" checked={picker} onChange={(e) => setPicker(e.target.checked)} aria-label={t("Only gateway models in /model")} />
        </div>
        <details className="adv">
          <summary><Icon name="chev" />{t("Advanced")}</summary>
          <div className="field">
            <label>{t("Key header")}<small>{t("Switch to the other one if the gateway returns 401")}</small></label>
            <div className="seg" role="group">
              {AUTHS.map(([v, label]) => <button key={v} aria-pressed={auth === v} onClick={() => setAuth(v)}>{label}</button>)}
            </div>
          </div>
          <div className="field">
            <label>{t("Default effort")}<small>{t("Gateways may ignore it")}</small></label>
            <div className="seg" role="group">
              {EFFORTS.map(([v, label]) => <button key={v} aria-pressed={effort === v} onClick={() => setEffort(v)}>{effortLabel(v, label)}</button>)}
            </div>
          </div>
          <HeaderHelp provider={provider} onChange={(p) => setProviders((ps) => ps?.map((x) => (x.id === p.id ? { ...x, ...p } : x)) ?? ps)} />
        </details>
      </div>

      {error && <div className="card banner err" style={{ marginTop: 14 }}>{error}</div>}

      {done && (
        <div className="card next on">
          <span className="check"><Icon name="check" /></span>
          <div>
            <b>{t("Claude Code is set up")}</b>
            <ul>
              <li>Claude Code CLI <span>{t("open a new terminal, run claude, then /status to see the gateway")}</span></li>
              {ides.map((i) => <li key={i.id}>{i.name} <span>{tr(i.claude ?? "")}</span></li>)}
            </ul>
          </div>
        </div>
      )}
    </>
  );
}
