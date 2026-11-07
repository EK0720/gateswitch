import { useEffect, useState } from "react";
import { api, errText, Provider } from "../api";
import { Icon } from "../icons";
import { locale, t } from "../i18n";
import { Menu } from "./Menu";
import { ModelTable } from "./ModelTable";
import { HeadersEditor } from "./HeadersEditor";
import { PresetImport } from "./PresetImport";

const palette = ["#7A2E8E", "#1F6F5C", "#B0552F", "#2F6FB0", "#8A5A3C", "#4B5563"];
const color = (id: string) => palette[[...id].reduce((a, c) => a + c.charCodeAt(0), 0) % palette.length];
const mono = (name: string) => name.replace(/[^A-Za-z0-9]/g, "").slice(0, 2) || "?";

type Draft = { id: string | null; name: string; baseUrl: string; key: string };
const empty: Draft = { id: null, name: "", baseUrl: "", key: "" };

export function Providers({ notify }: { notify: (t: string) => void }) {
  const [items, setItems] = useState<Provider[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [importing, setImporting] = useState(false);
  const [editHeaders, setEditHeaders] = useState(false);

  async function exportPreset(p: Provider) {
    try {
      const text = await api.presetExport(p.id);
      const a = document.createElement("a");
      a.href = URL.createObjectURL(new Blob([text], { type: "application/json" }));
      a.download = `${p.id}.preset.json`;
      a.click();
      URL.revokeObjectURL(a.href);
      notify(t("Preset exported (no key included)"));
    } catch (e) {
      notify(errText(e));
    }
  }

  const load = () => api.listProviders().then((list) => {
    setItems(list);
    setSelected((s) => s ?? list[0]?.id ?? null);
  });
  useEffect(() => { load(); }, []);

  async function save() {
    if (!draft) return;
    setBusy("save"); setError("");
    try {
      const p = await api.saveProvider({ id: draft.id, name: draft.name, baseUrl: draft.baseUrl, key: draft.key || null });
      setDraft(null);
      setSelected(p.id);
      await load();
      notify(draft.id ? t("Provider saved") : t("Provider added"));
      if (p.has_key) refresh(p.id);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy("");
    }
  }

  async function refresh(id: string) {
    setBusy("refresh:" + id); setError("");
    try {
      const p = await api.refreshModels(id);
      await load();
      notify(t("Fetched {n} models", { n: p.models.length }));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy("");
    }
  }

  async function remove(p: Provider) {
    if (!confirm(t("Delete {name}? Its key is also removed from the keychain. App configurations already set up stay unchanged.", { name: p.name }))) return;
    await api.removeProvider(p.id);
    setSelected(null);
    await load();
    notify(t("Provider deleted"));
  }

  const current = items?.find((p) => p.id === selected);

  return (
    <>
      <div className="head">
        <h1>{t("Providers")}</h1>
        <div className="acts">
          <button className="btn" onClick={() => { setImporting(true); setDraft(null); }}>
            <Icon name="dl" />{t("Import preset")}
          </button>
          <button className="btn primary" onClick={() => { setDraft({ ...empty }); setError(""); setImporting(false); }}>
            <Icon name="plus" />{t("Add provider")}
          </button>
        </div>
      </div>

      {importing && (
        <PresetImport
          onCancel={() => setImporting(false)}
          onDone={async (p) => {
            setImporting(false);
            setSelected(p.id);
            await load();
            notify(t("Provider added from preset"));
            if (!p.models.length) refresh(p.id);
          }}
        />
      )}

      {draft && (
        <div className="card form" style={{ marginBottom: 16 }}>
          <div className="field">
            <label htmlFor="pv-name">{t("Name")}</label>
            <input id="pv-name" className="input" autoFocus placeholder={t("e.g. My gateway")} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </div>
          <div className="field">
            <label htmlFor="pv-url">Base URL<small>{t("Usually ends with /v1")}</small></label>
            <input id="pv-url" className="input" placeholder="https://your-gateway.example.com/v1" value={draft.baseUrl} onChange={(e) => setDraft({ ...draft, baseUrl: e.target.value })} />
          </div>
          <div className="field">
            <label htmlFor="pv-key">API key<small>{draft.id ? t("Leave empty to keep the current key") : t("Stored in the system keychain")}</small></label>
            <input id="pv-key" className="input" type="password" autoComplete="off" placeholder="sk-…" value={draft.key} onChange={(e) => setDraft({ ...draft, key: e.target.value })} />
          </div>
          <div className="form-acts">
            {error && <span className="err">{error}</span>}
            <button className="btn" onClick={() => setDraft(null)}>{t("Cancel")}</button>
            <button className="btn primary" disabled={busy === "save"} onClick={save}>{busy === "save" ? t("Saving…") : t("Save")}</button>
          </div>
        </div>
      )}

      {items && items.length === 0 && !draft && (
        <div className="card empty">
          {t("No providers yet. Add your gateway (base URL and API key) to use it with Codex, Claude Code and Claude Desktop.")}
        </div>
      )}

      {items && items.length > 0 && (
        <div className="provs">
          {items.map((p) => (
            <button key={p.id} className="prov" aria-pressed={p.id === selected} onClick={() => { setSelected(p.id); setEditHeaders(false); }}>
              <div className="top">
                <span className="mono" style={{ background: color(p.id) }}>{mono(p.name)}</span>
                <div><b>{p.name}</b><span className="url">{p.base_url.replace(/^https?:\/\//, "")}</span></div>
                <i className={`dot ${!p.has_key ? "bad" : p.models.length ? "ok" : "warn"}`} />
              </div>
              <div className="meta">
                {!p.has_key ? <span>{t("No key")}</span> : <>
                  <span>{p.models.length ? t("{n} models", { n: p.models.length }) : t("Models not fetched")}</span>
                  {p.latency_ms != null && <span>{p.latency_ms} ms</span>}
                </>}
              </div>
            </button>
          ))}
        </div>
      )}

      {current && (
        <div className="card cap">
          <div className="cap-head">
            <b>{current.name}</b>
            <span>{current.checked_at ? t("Updated {time}", { time: new Date(current.checked_at).toLocaleString(locale()) }) : t("Models not fetched")}</span>
            <div className="cap-acts">
              <Menu items={[
                { label: Object.keys(current.headers ?? {}).length ? t("Custom headers ({n})", { n: Object.keys(current.headers ?? {}).length }) : t("Custom headers"), run: () => setEditHeaders(true) },
                { label: t("Export preset"), run: () => exportPreset(current) },
                { label: t("Delete provider"), run: () => remove(current), danger: true },
              ]} />
              <button className="btn small" onClick={() => setDraft({ id: current.id, name: current.name, baseUrl: current.base_url, key: "" })}>{t("Edit")}</button>
              <button className="btn small" disabled={!current.has_key || busy.startsWith("refresh")} onClick={() => refresh(current.id)}>
                <Icon name="refresh" />{busy === "refresh:" + current.id ? t("Fetching…") : t("Fetch models")}
              </button>
            </div>
          </div>
          {editHeaders && (
            <HeadersEditor key={current.id} provider={current} onClose={() => setEditHeaders(false)}
              onSaved={async () => { setEditHeaders(false); await load(); notify(t("Headers saved. Set up Claude again to apply them.")); }} />
          )}
          {error && !draft && <div className="banner err">{error}</div>}
          {current.models.length > 0 ? (
            <>
              <ModelTable provider={current} notify={notify} onChange={load} />
            </>
          ) : (
            <div className="empty">{current.has_key ? t("Click Fetch models to get the list from the gateway.") : t("Add a key to fetch the model list.")}</div>
          )}
        </div>
      )}
    </>
  );
}
