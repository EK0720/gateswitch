import type { UsagePrice, UsageSettings } from "../../api";
import { Icon } from "../../icons";
import { t } from "../../i18n";
import { Pop } from "./Filters";

function PriceInput({ value, onChange, label }: { value: number | undefined; onChange: (v: number | undefined) => void; label: string }) {
  return (
    <input className="input u-price" inputMode="decimal" aria-label={label} placeholder="—"
      defaultValue={value ?? ""} onBlur={(e) => { const s = e.target.value.trim().replace(",", "."); onChange(s === "" ? undefined : Number(s) || 0); }} />
  );
}

/** Per-model prices, USD per 1M tokens. Model ids are gateway-prefixed, so there are no built-in prices. */
export function Prices({ models, prices, onChange }: { models: string[]; prices: Record<string, UsagePrice>; onChange: (next: Record<string, UsagePrice>) => void }) {
  function set(model: string, field: keyof UsagePrice, v: number | undefined) {
    const row: UsagePrice = { ...(prices[model] ?? { input: 0, cached: 0, output: 0 }) };
    if (v === undefined) { if (field === "write") delete row.write; else row[field] = 0; } else row[field] = v;
    const next = { ...prices, [model]: row };
    if (!row.input && !row.cached && !row.output && row.write == null) delete next[model];
    onChange(next);
  }
  return (
    <div className="card u-prices">
      <p className="muted u-note">{t("USD per 1M tokens: in · cache read · cache write · out. Take them from your gateway's price list. An empty cache write counts as 1.25 × in. Stored on this machine only.")}</p>
      <div className="u-price-head" aria-hidden="true"><span /><span>{t("in")}</span><span>{t("read")}</span><span>{t("write")}</span><span>{t("out")}</span></div>
      {models.map((m) => (
        <div className="u-price-row" key={m}>
          <span className="u-mono" title={m}>{m}</span>
          <PriceInput label={`${m} ${t("in")}`} value={prices[m]?.input} onChange={(v) => set(m, "input", v)} />
          <PriceInput label={`${m} ${t("read")}`} value={prices[m]?.cached} onChange={(v) => set(m, "cached", v)} />
          <PriceInput label={`${m} ${t("write")}`} value={prices[m]?.write} onChange={(v) => set(m, "write", v)} />
          <PriceInput label={`${m} ${t("out")}`} value={prices[m]?.output} onChange={(v) => set(m, "output", v)} />
        </div>
      ))}
    </div>
  );
}

/** Page-level settings. Tray, mini window and alerts live in Settings › Notifications & background. */
export function SettingsPop({ settings, onChange, onMore }: { settings: UsageSettings; onChange: (s: UsageSettings) => void; onMore?: () => void }) {
  return (
    <Pop className="u-settings" label={t("Usage settings")} summary={<span className="btn"><Icon name="gear" /></span>}>
      {() => (
        <div className="u-set-list">
          <label className="u-set">
            <span><b>{t("Auto refresh")}</b><small>{t("Update while this page is open")}</small></span>
            <input type="checkbox" className="switch" checked={settings.watch} onChange={(e) => onChange({ ...settings, watch: e.target.checked })} />
          </label>
          {onMore && (
            <div className="u-set u-set-more">
              <span><small>{t("Tray icon, mini window and alerts")}</small></span>
              <button className="link" onClick={onMore}>{t("More in Settings")}</button>
            </div>
          )}
        </div>
      )}
    </Pop>
  );
}
