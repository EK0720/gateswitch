import { useEffect, useState } from "react";
import { api, errText, ImageGuidance as Status, Provider } from "../api";
import { t } from "../i18n";
import { Info } from "./Info";

// Only to sort likely image models first; any model name can be typed.
const IMAGE_MODEL = /image|img|astra|dall-?e|imagen|flux|seedream|midjourney|sdxl|stable-?diffusion|kolors|hidream|recraft|ideogram/i;

/** Codex image generation: a guidance block in AGENTS.md that calls the gateway's /images/generations. */
export function ImageGuidance({ provider, notify }: { provider: Provider | undefined; notify: (t: string) => void }) {
  const [status, setStatus] = useState<Status | null>(null);
  const [model, setModel] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => { api.codexImageStatus().then(setStatus).catch(() => setStatus(null)); }, []);

  const all = provider?.models ?? [];
  const candidates = all.filter((m) => IMAGE_MODEL.test(m));
  // Suggestions: likely image models first, then every other gateway model.
  const suggestions = [...candidates, ...all.filter((m) => !IMAGE_MODEL.test(m))];
  const listId = `img-models-${provider?.id ?? ""}`;
  const saved = provider ? status?.models[provider.base_url] ?? "" : "";
  useEffect(() => {
    setModel(saved || candidates[0] || "");
  }, [saved, provider?.id]);

  async function run(fn: () => Promise<{ message: string }>) {
    setBusy(true); setError("");
    try {
      notify(t((await fn()).message));
      setStatus(await api.codexImageStatus());
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  const sameSetup = !!status?.installed && !status.outdated && saved === model;
  const host = provider?.base_url.replace(/^https?:\/\//, "") ?? "";

  return (
    <div className="card form" style={{ marginTop: 14 }}>
      <div className="field">
        <label>
          {t("Image generation")}
          <Info text={t("Adds a marked section to Codex's AGENTS.md telling it to create images through /images/generations, as transparent PNG by default. The section names no gateway: it uses whichever provider Codex is set up with and the image model chosen here for that gateway. It holds no key; its helper reads the key variable at run time. The rest of AGENTS.md is untouched; undo or Restore defaults removes it.")} />
          <small>
            {status?.outdated
              ? t("Old version pinned to one gateway; update it")
              : saved
                ? t("{host}: {model}", { host, model: saved })
                : t("Image model for {host}", { host: host || "—" })}
          </small>
        </label>
        <div className="img-row">
          <input className="input mono-input" list={listId} placeholder={t("Image model name on your gateway")} value={model}
            onChange={(e) => setModel(e.target.value.trim())} aria-label={t("Image model")} spellCheck={false} />
          <datalist id={listId}>{suggestions.map((m) => <option key={m} value={m} />)}</datalist>
          <button className="btn small" disabled={busy || !provider?.has_key || !model || sameSetup}
            onClick={() => run(() => api.codexImageSetup(provider!.id, model))}>
            {busy ? t("Saving…") : status?.installed && saved ? t("Update") : t("Add")}
          </button>
          {status?.installed && (
            <button className="btn small ghost" disabled={busy} onClick={() => run(api.codexImageRemove)}>{t("Remove")}</button>
          )}
        </div>
      </div>
      {error && <div className="err" style={{ padding: "0 0 12px", fontSize: 13 }}>{error}</div>}
    </div>
  );
}
