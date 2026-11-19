import { useEffect, useState } from "react";
import { api, errText, onUsageSettingsChanged, Provider, UsageSettings } from "../api";
import { t } from "../i18n";
import { parseTokens, shortTokens } from "./usage/util";
import { hhmm, useQuotaWatches } from "./QuotaWatch";

type Flag = "tray" | "close_to_tray" | "overlay" | "notify" | "quota_auto";

/** Settings › Notifications & background. Everything here is off by default. */
export function Background({ notify }: { notify: (m: string) => void }) {
  const [s, setS] = useState<UsageSettings | null>(null);
  const [limit, setLimit] = useState("");
  const [watches, setWatches] = useQuotaWatches();
  const [providers, setProviders] = useState<Provider[]>([]);

  useEffect(() => {
    const load = () => api.usageSettingsGet().then((x) => { setS(x); setLimit(x.block_limit != null ? shortTokens(x.block_limit) : ""); }).catch(() => {});
    load();
    api.listProviders().then(setProviders).catch(() => {});
    // The mini window's × and the tray menu change these too.
    return onUsageSettingsChanged(load);
  }, []);

  if (!s) return null;
  const save = (next: UsageSettings) => {
    setS(next);
    api.usageSettingsSet(next).catch((e) => notify(errText(e)));
  };
  const parsed = parseTokens(limit);

  const row = (k: Flag, label: string, hint: string, disabled = false) => (
    <label className={`issue row2 bg-row ${disabled ? "off" : ""}`} key={k}>
      <div><b>{label}</b><p>{hint}</p></div>
      <input type="checkbox" className="switch" checked={s[k] && !disabled} disabled={disabled} aria-label={label}
        onChange={(e) => save({ ...s, [k]: e.target.checked })} />
    </label>
  );

  async function cancel(providerId: string, group: string) {
    try { setWatches(await api.quotaWatchRemove(providerId, group)); } catch (e) { notify(errText(e)); }
  }
  const providerName = (id: string) => providers.find((p) => p.id === id)?.name ?? id;

  return (
    <>
      <div className="section-title">{t("Notifications & background")}</div>
      <div className="card">
        {row("tray", t("Tray icon"), t("Show today's tokens in the tray"))}
        {row("close_to_tray", t("Closing the window hides it to the tray"),
          s.tray ? t("Gateswitch keeps running; open or quit it from the tray menu") : t("Turn on the tray icon first"), !s.tray)}
        {row("overlay", t("Mini window always on top"), t("Today's tokens and the current block in a small window"))}
        {row("notify", t("Alert at 80% / 100% of the 5-hour block"), t("One notification per block at each level"))}
        <div className="issue row2">
          <div><b>{t("Block limit")}</b><p>{t("Tokens per 5-hour block, e.g. 5M or 500k")}</p></div>
          <input className={`input u-limit ${parsed === undefined ? "bad" : ""}`} value={limit} placeholder={t("None")} aria-label={t("Block limit")} aria-invalid={parsed === undefined}
            onChange={(e) => setLimit(e.target.value)}
            onBlur={() => { if (parsed !== undefined && parsed !== s.block_limit) save({ ...s, block_limit: parsed }); }}
            onKeyDown={(e) => { if (e.key === "Enter") (e.target as HTMLInputElement).blur(); }} />
        </div>
        {row("quota_auto", t("Watch automatically and notify when quota comes back"),
          t("After Test models finds the model Codex or Claude Code uses out of quota. One check at the announced reset; a check that works costs at most one 16-token request."))}
        <div className="bg-watches">
          <div className="bg-watches-head">{t("Active quota watches")} <span className="count">{watches.length}</span></div>
          {watches.length === 0 && <p className="muted">{t("None. Use “Notify when it's back” on an out-of-quota card on the Codex or Claude Code page.")}</p>}
          {watches.map((w) => (
            <div className="bg-watch" key={`${w.provider_id}/${w.group}`}>
              <div>
                <b>{w.group}</b> <span className="u-mono">{w.model}</span>
                <span className="muted"> · {providerName(w.provider_id)}{w.origin === "auto" ? ` · ${t("automatic")}` : ""}</span>
                <p>{t("Next check {time}", { time: hhmm(w.next_at) })}{w.message ? ` · ${w.message}` : ""}</p>
              </div>
              <button className="btn small ghost" onClick={() => cancel(w.provider_id, w.group)}>{t("Cancel")}</button>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
