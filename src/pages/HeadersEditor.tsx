import { useState } from "react";
import { api, errText, Provider } from "../api";
import { t } from "../i18n";
import { Info } from "./Info";

export const headersText = (h: Record<string, string> | undefined) =>
  Object.entries(h ?? {}).map(([k, v]) => `${k}: ${v}`).join("\n");

/** Custom HTTP headers for one provider. Written to Claude Code and Claude Desktop on setup. */
export function HeadersEditor({ provider, onSaved, onClose }: { provider: Provider; onSaved: (p: Provider) => void; onClose: () => void }) {
  const [text, setText] = useState(headersText(provider.headers));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function save() {
    setBusy(true); setError("");
    try {
      onSaved(await api.setProviderHeaders(provider.id, text));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="hdr-edit">
      <label htmlFor="pv-headers">
        {t("Custom headers")}
        <Info text={t("Some gateways need extra request headers, for example to pick a compatible route. One per line as Name: value. Applied to Claude Code and Claude Desktop when you set them up. Do not put keys here.")} />
      </label>
      <textarea id="pv-headers" className="input mono-input" rows={3} spellCheck={false} placeholder={"X-Client: my-app\nx-app: demo"} value={text} onChange={(e) => setText(e.target.value)} />
      <div className="form-acts">
        {error && <span className="err">{error}</span>}
        <button className="btn small" onClick={onClose}>{t("Cancel")}</button>
        <button className="btn small primary" disabled={busy} onClick={save}>{busy ? t("Saving…") : t("Save")}</button>
      </div>
    </div>
  );
}
