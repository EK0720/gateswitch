import { useState } from "react";
import { api, errText, Provider } from "../api";
import { t } from "../i18n";

// Some gateways (9router-based) detect Claude Code by `User-Agent: claude-cli/…`
// or `x-app: cli` and then reshape the request, which can end in 400
// "reasoning_effort: Extra inputs are not permitted". Overriding BOTH headers
// with any neutral value skips that path; the values themselves mean nothing.
// They must not contain claude-cli, claude-code, codex-cli, gemini-cli… and
// x-app must not be "cli". Side effect: the gateway no longer answers Claude
// Code's warmup/title requests itself, so those reach the model (a few tokens).
const COMPAT = "User-Agent: model-switcher/1\nx-app: model-switcher";
const same = (h: Record<string, string>) =>
  Object.keys(h).length === 2 && h["User-Agent"] === "model-switcher/1" && h["x-app"] === "model-switcher";

/** Advanced row on the Claude pages: shows the provider's headers and offers the 400 fix. */
export function HeaderHelp({ provider, onChange }: { provider: Provider | undefined; onChange: (p: Provider) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const n = Object.keys(provider?.headers ?? {}).length;
  const added = same(provider?.headers ?? {});

  async function add() {
    if (!provider) return;
    if (n > 0 && !confirm(t("Replace the provider's {n} custom headers with these?", { n }))) return;
    setBusy(true); setError("");
    try {
      onChange(await api.setProviderHeaders(provider.id, COMPAT));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="field">
      <label>
        {t("Custom headers")}
        <small>{n ? t("{n} from the provider, sent with every request", { n }) : t("None. Edit them on the Providers page.")}</small>
      </label>
      <details className="fix">
        <summary>{t("Getting 400 “reasoning_effort: Extra inputs are not permitted”?")}</summary>
        <p>{t("Some gateways recognise Claude Code by its User-Agent and x-app headers and rewrite the request, which this error comes from. Sending neutral values for both skips that path. Both are needed. Set up again after adding, then start a new Claude session.")}</p>
        <pre>{COMPAT}</pre>
        <div className="fix-acts">
          {error && <span className="err">{error}</span>}
          <button className="btn small" disabled={busy || !provider || added} onClick={add}>
            {added ? t("Already added") : busy ? t("Saving…") : n > 0 ? t("Replace with these headers") : t("Add these headers")}
          </button>
        </div>
      </details>
    </div>
  );
}
