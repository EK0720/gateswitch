import { useEffect, useState } from "react";
import { api, errText, Preset, Provider } from "../api";
import { t } from "../i18n";

/** Import a gateway preset (file or sample), paste a key, done. */
export function PresetImport({ onDone, onCancel }: { onDone: (p: Provider) => void; onCancel: () => void }) {
  const [samples, setSamples] = useState<Preset[]>([]);
  const [text, setText] = useState("");
  const [preset, setPreset] = useState<Preset | null>(null);
  const [key, setKey] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => { api.presetSamples().then(setSamples).catch(() => setSamples([])); }, []);

  async function read(s: string) {
    setText(s); setError(""); setPreset(null);
    if (!s.trim()) return;
    try {
      setPreset(await api.presetParse(s));
    } catch (e) {
      setError(errText(e));
    }
  }

  async function pickFile(f: File | undefined) {
    if (!f) return;
    if (f.size > 256 * 1024) { setError(t("Preset file is too large")); return; }
    read(await f.text());
  }

  async function save() {
    setBusy(true); setError("");
    try {
      const p = await api.presetImport(text, key);
      setKey("");
      onDone(p);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card form" style={{ marginBottom: 16 }}>
      <div className="field">
        <label>{t("Sample presets")}<small>{t("Local gateways or popular services")}</small></label>
        <div className="checks">
          {samples.map((s) => (
            <button key={s.name} className="btn small" onClick={() => read(JSON.stringify(s))}>{s.name}</button>
          ))}
        </div>
      </div>
      <div className="field">
        <label>{t("Preset file")}<small>{t(".json published by the gateway provider")}</small></label>
        <input type="file" accept=".json,application/json" onChange={(e) => pickFile(e.target.files?.[0])} />
      </div>
      <div className="field">
        <label htmlFor="ps-text">{t("Or paste the contents")}<small>{t("Presets never contain a key")}</small></label>
        <textarea id="ps-text" className="input mono-input" rows={4} value={text} onChange={(e) => read(e.target.value)} placeholder='{"name": "...", "base_url": "https://.../v1", "models": [...]}' />
      </div>
      {preset && (
        <div className="field">
          <label>{t("Will add")}</label>
          <div>
            <b>{preset.name}</b> <span className="muted">{preset.base_url}</span>
            <div className="muted">{preset.models.length ? t("{n} models", { n: preset.models.length }) : t("Model list will be fetched from the gateway")}{preset.docs ? ` · ${preset.docs}` : ""}</div>
          </div>
        </div>
      )}
      <div className="field">
        <label htmlFor="ps-key">API key<small>{t("Stored in the system keychain")}</small></label>
        <input id="ps-key" className="input" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder="sk-…" />
      </div>
      <div className="form-acts">
        {error && <span className="err">{error}</span>}
        <button className="btn" onClick={onCancel}>{t("Cancel")}</button>
        <button className="btn primary" disabled={!preset || !key.trim() || busy} onClick={save}>{busy ? t("Adding…") : t("Add provider")}</button>
      </div>
    </div>
  );
}
