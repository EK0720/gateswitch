import { useEffect, useState } from "react";
import { api, errText, Usage as UsageData } from "../api";
import { t } from "../i18n";
import { Info } from "./Info";

function fmt(n: number): string {
  if (n >= 1_000_000) return t("{n} M", { n: (n / 1_000_000).toFixed(1) });
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)} k`;
  return String(n);
}

const APPS = [
  { key: "claude", name: "Claude Code" },
  { key: "codex", name: "Codex" },
  { key: "orca", name: "Orca" },
] as const;

/** USD per 1M tokens, typed by the user. Gateways price differently, so no defaults. */
type Price = { input: number; cached: number; output: number };
const PRICE_KEY = "usage-prices";
function loadPrices(): Record<string, Price> {
  try { return JSON.parse(localStorage.getItem(PRICE_KEY) || "{}"); } catch { return {}; }
}
const usd = (n: number) => (n >= 100 ? `$${n.toFixed(0)}` : `$${n.toFixed(2)}`);

function PriceInput({ value, onChange, label }: { value: number | undefined; onChange: (v: number) => void; label: string }) {
  return (
    <input className="input price" inputMode="decimal" aria-label={label} placeholder={label}
      defaultValue={value ?? ""} onBlur={(e) => onChange(Number(e.target.value.replace(",", ".")) || 0)} />
  );
}

export function Usage() {
  const [data, setData] = useState<UsageData | null>(null);
  const [error, setError] = useState("");
  const [prices, setPrices] = useState<Record<string, Price>>(loadPrices);
  const [editPrices, setEditPrices] = useState(false);

  function setPrice(model: string, field: keyof Price, v: number) {
    setPrices((cur) => {
      const next = { ...cur, [model]: { ...{ input: 0, cached: 0, output: 0 }, ...cur[model], [field]: v } };
      try { localStorage.setItem(PRICE_KEY, JSON.stringify(next)); } catch {}
      return next;
    });
  }
  const cost = (m: string, tot: { input: number; cached: number; output: number }) => {
    const p = prices[m];
    if (!p) return null;
    return ((tot.input - tot.cached) * p.input + tot.cached * p.cached + tot.output * p.output) / 1_000_000;
  };

  useEffect(() => {
    api.usage().then(setData).catch((e) => setError(errText(e)));
  }, []);

  if (error) return (<><div className="head"><h1>{t("Usage")}</h1></div><div className="card banner err">{error}</div></>);
  if (!data) return (<><div className="head"><h1>{t("Usage")}</h1></div><div className="card empty">{t("Reading logs…")}<p className="muted" style={{ margin: "6px 0 0", fontSize: 12.5 }}>{t("The first read can take a while when the logs are large. Later opens only read new sessions.")}</p></div></>);

  const totalIn = data.claude.input + data.codex.input + data.orca.input;
  const totalCached = data.claude.cached + data.codex.cached + data.orca.cached;
  const totalOut = data.claude.output + data.codex.output + data.orca.output;
  const hit = totalIn > 0 ? Math.round((totalCached / totalIn) * 100) : 0;
  const days = data.days.slice(-30);
  const max = Math.max(1, ...days.map((d) => d.input + d.output));
  const models = Object.entries(data.by_model).sort((a, b) => b[1].input + b[1].output - (a[1].input + a[1].output)).slice(0, 12);
  const empty = totalIn + totalOut === 0;
  const costs = models.map(([m, tot]) => cost(m, tot)).filter((c): c is number => c != null);
  const totalCost = costs.reduce((a, b) => a + b, 0);

  return (
    <>
      <div className="head"><h1>{t("Usage")}<Info text={t("Read from logs already on this machine; nothing is sent anywhere. {a} Claude Code sessions, {b} Codex sessions, {c} Orca sessions.", { a: data.files[0], b: data.files[1], c: data.files[2] })} /></h1></div>

      {empty ? (
        <div className="card empty">{t("No token data found in the logs yet.")}</div>
      ) : (
        <>
          <div className="stats">
            <div><b>{fmt(totalIn)}</b><span>{t("tokens in")}</span></div>
            <div><b>{fmt(totalOut)}</b><span>{t("tokens out")}</span></div>
            <div><b>{hit}%</b><span>{t("cache hit")}</span></div>
            {costs.length > 0 && <div><b>{usd(totalCost)}</b><span>{t("estimated ({n} priced models)", { n: costs.length })}</span></div>}
          </div>

          {totalIn > 1000 && hit === 0 && (
            <div className="card alert">
              <div><b>{t("Cache hit is 0")}</b><p>{t("The gateway is not returning prompt cache, so costs may be several times higher.")}</p></div>
            </div>
          )}

          {days.length > 0 && (
            <>
              <div className="section-title">{t("Last 30 days")}</div>
              <div className="card" style={{ padding: "16px 18px" }}>
                <div className="bars">
                  {days.map((d) => (
                    <div key={d.day} className="bar" title={t("{day}: {in} in, {out} out", { day: d.day, in: fmt(d.input), out: fmt(d.output) })}>
                      <i style={{ height: `${Math.max(2, ((d.input + d.output) / max) * 100)}%` }} />
                    </div>
                  ))}
                </div>
                <div className="bar-axis"><span>{days[0].day.slice(5)}</span><span>{days[days.length - 1].day.slice(5)}</span></div>
              </div>
            </>
          )}

          <div className="section-title">{t("By app")}</div>
          <div className="card">
            {APPS.map((a) => {
              const tot = data[a.key];
              const sum = tot.input + tot.output;
              return (
                <div className="issue" key={a.key}>
                  <div>
                    <b>{a.name}</b>
                    <p>{t("{in} in · {out} out", { in: fmt(tot.input), out: fmt(tot.output) })}{tot.input > 0 ? ` · ${t("{p}% cache", { p: Math.round((tot.cached / tot.input) * 100) })}` : ""}</p>
                  </div>
                  <span className="muted">{sum === 0 ? "—" : fmt(sum)}</span>
                </div>
              );
            })}
          </div>

          {models.length > 0 && (
            <>
              <div className="section-title">{t("By model")}
                <button className="btn small ghost" style={{ marginLeft: "auto" }} onClick={() => setEditPrices((v) => !v)}>{editPrices ? t("Done") : t("Enter prices")}</button>
              </div>
              {editPrices && <p className="muted" style={{ margin: "0 0 8px", fontSize: 12.5 }}>{t("USD per 1M tokens: in · cache · out. Take them from your gateway's price list. Stored on this machine only.")}</p>}
              <div className="card">
                {models.map(([m, tot]) => {
                  const c = cost(m, tot);
                  return (
                    <div className="issue" key={m}>
                      <div><b>{m}</b><p>{t("{in} in · {cached} cache · {out} out", { in: fmt(tot.input), cached: fmt(tot.cached), out: fmt(tot.output) })}</p></div>
                      {editPrices ? (
                        <div className="prices">
                          <PriceInput label={t("in")} value={prices[m]?.input} onChange={(v) => setPrice(m, "input", v)} />
                          <PriceInput label={t("cache")} value={prices[m]?.cached} onChange={(v) => setPrice(m, "cached", v)} />
                          <PriceInput label={t("out")} value={prices[m]?.output} onChange={(v) => setPrice(m, "output", v)} />
                        </div>
                      ) : (
                        <span className="muted">{c != null ? usd(c) : fmt(tot.input + tot.output)}</span>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </>
      )}
    </>
  );
}
