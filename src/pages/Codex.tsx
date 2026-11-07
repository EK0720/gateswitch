import { useEffect, useMemo, useState } from "react";
import { api, CodexCurrent, CodexNames, errText, Provider, Scan } from "../api";
import { Icon } from "../icons";
import { locale, t, tr } from "../i18n";
import { groupOf, statusText } from "./ModelTable";
import { ImageGuidance } from "./ImageGuidance";

const EFFORTS = [["default", "Default"], ["low", "low"], ["medium", "medium"], ["high", "high"]] as const;

export function Codex({ scan, notify, goProviders }: { scan: Scan | null; notify: (t: string) => void; goProviders: () => void }) {
  const [providers, setProviders] = useState<Provider[] | null>(null);
  const [current, setCurrent] = useState<CodexCurrent | null>(null);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [catalog, setCatalog] = useState(true);
  const [images, setImages] = useState(true);
  const [effort, setEffort] = useState("default");
  const [names, setNames] = useState<CodexNames | null>(null);
  const [nameDraft, setNameDraft] = useState({ key: "", env: "" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState(false);

  useEffect(() => {
    Promise.all([api.listProviders(), api.codexCurrent()]).then(([ps, cur]) => {
      setProviders(ps);
      setCurrent(cur);
      const fromConfig = ps.find((p) => p.id === cur.provider_id);
      const pick = fromConfig ?? ps.find((p) => p.has_key) ?? ps[0];
      if (pick) setProviderId(pick.id);
      if (cur.model) setModel(cur.model);
      if (cur.effort) setEffort(cur.effort);
    });
  }, []);

  useEffect(() => {
    if (!providerId) return;
    api.codexNames(providerId).then((n) => { setNames(n); setNameDraft({ key: n.key, env: n.env }); }).catch(() => setNames(null));
  }, [providerId]);

  const provider = providers?.find((p) => p.id === providerId);
  const models = provider?.models ?? [];
  useEffect(() => {
    if (provider && models.length && !models.includes(model)) {
      setModel(models.find((m) => /gpt|codex/i.test(m)) ?? models[0]);
    }
  }, [providerId, models.length]);

  const desktop = scan?.tools.find((x) => x.id === "codex-desktop")?.installed;
  const ides = useMemo(() => (scan?.ides ?? []).filter((i) => i.uses.includes("codex")), [scan]);

  const [restarting, setRestarting] = useState(false);
  async function restart() {
    if (!confirm(t("Quit and reopen Codex Desktop? Running sessions in the app will stop."))) return;
    setRestarting(true); setError("");
    try {
      notify(await api.codexDesktopRestart(false));
    } catch (e) {
      const raw = typeof e === "string" ? e : "";
      // Soft close failed (app hid to the tray): offer to force quit.
      if (raw.includes("Codex is still running in the background") && confirm(t("Codex is still running in the system tray. Force quit and reopen?"))) {
        try {
          notify(await api.codexDesktopRestart(true));
        } catch (e2) {
          setError(errText(e2));
        }
      } else {
        setError(errText(e));
      }
    } finally {
      setRestarting(false);
    }
  }

  async function saveNames(reset = false) {
    try {
      const n = await api.setCodexNames(providerId, reset ? "" : nameDraft.key.trim(), reset ? "" : nameDraft.env.trim());
      setNames(n);
      setNameDraft({ key: n.key, env: n.env });
      notify(t("Names saved. Click Set up to apply."));
    } catch (e) {
      setError(errText(e));
    }
  }

  async function setup() {
    setBusy(true); setError(""); setDone(false);
    try {
      const r = await api.codexSetup({ provider_id: providerId, model, effort: effort === "default" ? null : effort, catalog, images });
      setCurrent(await api.codexCurrent());
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
        <div className="head"><h1>Codex</h1></div>
        <div className="card empty">
          {t("No providers yet.")}
          <div style={{ marginTop: 12 }}><button className="btn primary" onClick={goProviders}><Icon name="plus" />{t("Add provider")}</button></div>
        </div>
      </>
    );
  }

  const active = current?.provider_id === providerId && current?.provider === names?.key && current?.model === model;
  const namesDirty = !!names && (nameDraft.key !== names.key || nameDraft.env !== names.env);

  // Last "Kiểm tra model" result for the chosen model (Providers page).
  const probe = provider?.probes?.[model];
  const sameGroup = provider && model ? models.filter((m) => m !== model && groupOf(provider, m) === groupOf(provider, model)) : [];
  const alt = provider ? sameGroup.find((m) => provider.probes?.[m]?.status === "ok") ?? sameGroup.find((m) => !provider.probes?.[m] || provider.probes[m].status !== "quota") : undefined;

  return (
    <>
      <div className="head">
        <h1>Codex</h1>
        <div className="acts">
          <button className="btn primary" disabled={busy || !provider?.has_key || !model} onClick={setup}>
            {busy ? t("Setting up…") : active ? t("Set up again") : t("Set up")}
          </button>
        </div>
      </div>

      {current?.model && (
        <div className="now">
          <i className="dot ok" />{t("In use")} <b>{current.model}</b>
          <span className="muted">{t("via")} {current.base_url.replace(/^https?:\/\//, "") || current.provider}</span>
        </div>
      )}

      {probe?.status === "quota" && (
        <div className="card alert">
          <Icon name="warn" />
          <div>
            <b>{t("{model} is out of quota", { model })}</b>
            <div className="muted">{t("Checked at")} {new Date(probe.checked_at).toLocaleString(locale())}{probe.message ? `: ${probe.message}` : ""}</div>
          </div>
          {alt && (
            <button className="btn small" style={{ marginLeft: "auto" }} onClick={() => setModel(alt)}>
              {t("Switch to another model in the group")}
            </button>
          )}
        </div>
      )}

      <div className="card form">
        <div className="field">
          <label htmlFor="cx-gw">Gateway</label>
          <select id="cx-gw" className="select" value={providerId} onChange={(e) => setProviderId(e.target.value)}>
            {providers?.map((p) => (
              <option key={p.id} value={p.id} disabled={!p.has_key}>
                {p.name} — {p.base_url.replace(/^https?:\/\//, "")}{p.has_key ? "" : ` (${t("no key")})`}
              </option>
            ))}
          </select>
        </div>
        <div className="field">
          <label htmlFor="cx-model">Model{!models.length ? <small>{t("Models not fetched, type one")}</small> : probe && probe.status !== "quota" ? <small>{statusText(probe)}</small> : null}</label>
          {models.length ? (
            <select id="cx-model" className="select" value={model} onChange={(e) => setModel(e.target.value)}>
              {!models.includes(model) && model && <option value={model}>{model}</option>}
              {models.map((m) => <option key={m}>{m}</option>)}
            </select>
          ) : (
            <input id="cx-model" className="input" placeholder="gpt-6.1-sol" value={model} onChange={(e) => setModel(e.target.value.trim())} />
          )}
        </div>
        <div className="field">
          <label>{t("/model menu")}<small>{models.length ? t("{n} models from the gateway", { n: models.length }) : t("Selected model only")}</small></label>
          <input type="checkbox" className="switch" checked={catalog} onChange={(e) => setCatalog(e.target.checked)} aria-label={t("/model menu")} />
        </div>
        <details className="adv">
          <summary><Icon name="chev" />{t("Advanced")}</summary>
          <div className="field">
            <label>{t("Default effort")}<small>{t("Gateways may ignore it")}</small></label>
            <div className="seg" role="group">
              {EFFORTS.map(([v, label]) => (
                <button key={v} aria-pressed={effort === v} onClick={() => setEffort(v)}>{t(label)}</button>
              ))}
            </div>
          </div>
          <div className="field">
            <label>{t("Image input")}<small>{t("Allow image attachments")}</small></label>
            <input type="checkbox" className="switch" checked={images} disabled={!catalog} onChange={(e) => setImages(e.target.checked)} aria-label={t("Image input")} />
          </div>
          {names && (
            <>
              <div className="field">
                <label htmlFor="cx-name">{t("Name in config.toml")}<small>{t("Default: gateway")}</small></label>
                <input id="cx-name" className="input mono-input" value={nameDraft.key} onChange={(e) => setNameDraft({ ...nameDraft, key: e.target.value.toLowerCase() })} spellCheck={false} />
              </div>
              <div className="field">
                <label htmlFor="cx-env">{t("Key environment variable")}<small>{t("Codex reads the key from this variable")}</small></label>
                <input id="cx-env" className="input mono-input" value={nameDraft.env} onChange={(e) => setNameDraft({ ...nameDraft, env: e.target.value.toUpperCase() })} spellCheck={false} />
              </div>
              <div className="form-acts" style={{ borderTop: 0, paddingTop: 0 }}>
                <button className="btn small ghost" onClick={() => saveNames(true)}>{t("Reset to default")}</button>
                <button className="btn small" disabled={!namesDirty} onClick={() => saveNames()}>{t("Save names")}</button>
              </div>
            </>
          )}
        </details>
      </div>

      {error && <div className="card banner err" style={{ marginTop: 14 }}>{error}</div>}

      <ImageGuidance provider={provider} notify={notify} />

      {desktop && (
        <div className="probe-bar restart-bar">
          <span className="muted">{t("Codex Desktop reads its config only at launch.")}</span>
          <button className="btn small" disabled={restarting} onClick={restart}>{restarting ? t("Restarting…") : t("Restart Codex Desktop")}</button>
        </div>
      )}

      {done && (
        <div className="card next on">
          <span className="check"><Icon name="check" /></span>
          <div>
            <b>{t("Codex is set up")}</b>
            <ul>
              <li>Codex CLI <span>{t("open a new terminal and run codex")}</span></li>
              {desktop && <li>Codex Desktop <span>{t("restart it with the bar above")}</span></li>}
              <li>Orca <span>{t("follows automatically; just restart the daemon on the Orca page")}</span></li>
              {ides.map((i) => <li key={i.id}>{i.name} <span>{tr(i.codex ?? "")}</span></li>)}
            </ul>
          </div>
        </div>
      )}
    </>
  );
}
