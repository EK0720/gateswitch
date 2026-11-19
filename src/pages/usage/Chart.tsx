import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Totals, UsagePrice, UsageReport } from "../../api";
import { t } from "../../i18n";
import { all, APP_COLOR, APPS, costOf, dayLabel, fmt, SERIES, usd } from "./util";

export type Metric = "tokens" | "cost";
export type StackBy = "app" | "model";

type Series = { key: string; label: string; color: string };

const H = 168; // plot height
const PAD_L = 46, PAD_B = 22, PAD_T = 8;

function niceMax(v: number) {
  if (v <= 0) return 1;
  const p = 10 ** Math.floor(Math.log10(v));
  const f = v / p;
  return ([1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].find((s) => f <= s) ?? 10) * p;
}

export function Chart({ report, prices, metric, stack, onMetric, onStack, onPickDay }: {
  report: UsageReport;
  prices: Record<string, UsagePrice>;
  metric: Metric;
  stack: StackBy;
  onMetric: (m: Metric) => void;
  onStack: (s: StackBy) => void;
  onPickDay: (day: string) => void;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const [w, setW] = useState(640);
  const [hover, setHover] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(Math.max(280, el.clientWidth)));
    ro.observe(el);
    setW(Math.max(280, el.clientWidth));
    return () => ro.disconnect();
  }, []);

  const anyPrice = Object.keys(prices).length > 0;
  // Cost needs model totals: bucket.by_app has no model split, so cost is always stacked by model.
  const by: StackBy = metric === "cost" ? "model" : stack;
  const val = (model: string | null, x: Totals) => (metric === "tokens" ? all(x) : model ? costOf(prices[model], x) ?? 0 : 0);

  const { series, values } = useMemo(() => {
    if (by === "app") {
      const series: Series[] = APPS.map((a) => ({ key: a.key, label: a.name, color: APP_COLOR[a.key] }));
      const values = report.buckets.map((b) => series.map((s) => { const x = b.by_app[s.key as keyof typeof b.by_app]; return x ? val(null, x) : 0; }));
      return { series, values };
    }
    const sum = new Map<string, number>();
    for (const b of report.buckets) for (const [m, x] of Object.entries(b.by_model)) sum.set(m, (sum.get(m) ?? 0) + val(m, x));
    const ranked = [...sum.entries()].filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1]);
    const top = ranked.slice(0, 5).map(([m]) => m);
    const series: Series[] = top.map((m, i) => ({ key: m, label: m, color: SERIES[i] }));
    if (ranked.length > 5) series.push({ key: "__other", label: t("Other"), color: SERIES[5] });
    const values = report.buckets.map((b) => {
      const row = series.map(() => 0);
      for (const [m, x] of Object.entries(b.by_model)) {
        const i = top.indexOf(m);
        row[i >= 0 ? i : series.length - 1] += val(m, x);
      }
      return row;
    });
    return { series, values };
  }, [report, prices, metric, by]);

  const totals = values.map((r) => r.reduce((a, b) => a + b, 0));
  const max = niceMax(Math.max(0, ...totals));
  const n = report.buckets.length;
  const plotW = w - PAD_L - 4;
  const slot = plotW / Math.max(1, n);
  const barW = Math.max(2, Math.min(28, slot * 0.72));
  const y = (v: number) => PAD_T + H - (v / max) * H;
  const label = (v: number) => (metric === "cost" ? usd(v) : fmt(v));
  const hourly = report.bucket === "hour";
  const bucketLabel = (s: string, long = false) =>
    hourly ? `${s.slice(11, 13)}:00` : long ? dayLabel(s, { weekday: "short", month: "short", day: "numeric" }) : dayLabel(s);
  const every = Math.max(1, Math.ceil(n / Math.max(2, Math.floor(plotW / 64))));

  function key(e: React.KeyboardEvent) {
    if (!n) return;
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const d = e.key === "ArrowRight" ? 1 : -1;
      setHover((h) => (h == null ? (d > 0 ? 0 : n - 1) : Math.min(n - 1, Math.max(0, h + d))));
    } else if (e.key === "Home") { e.preventDefault(); setHover(0); }
    else if (e.key === "End") { e.preventDefault(); setHover(n - 1); }
    else if ((e.key === "Enter" || e.key === " ") && hover != null && !hourly) { e.preventDefault(); onPickDay(report.buckets[hover].start); }
    else if (e.key === "Escape") setHover(null);
  }

  const hb = hover != null ? report.buckets[hover] : null;
  const tipLeft = hover != null ? Math.min(Math.max(PAD_L + slot * hover + slot / 2, 90), w - 90) : 0;

  return (
    <div className="card u-chart">
      <div className="u-chart-head">
        <b>{hourly ? t("By hour") : t("By day")}</b>
        <div className="seg" role="group" aria-label={t("Measure")}>
          <button aria-pressed={metric === "tokens"} onClick={() => onMetric("tokens")}>{t("Tokens")}</button>
          <button aria-pressed={metric === "cost"} disabled={!anyPrice} title={anyPrice ? undefined : t("Enter prices on the Models tab to see cost")} onClick={() => onMetric("cost")}>{t("Cost")}</button>
        </div>
        <div className="seg" role="group" aria-label={t("Stack by")}>
          <button aria-pressed={by === "app"} disabled={metric === "cost"} title={metric === "cost" ? t("Cost is split by model") : undefined} onClick={() => onStack("app")}>{t("App")}</button>
          <button aria-pressed={by === "model"} onClick={() => onStack("model")}>{t("Model")}</button>
        </div>
      </div>
      <div className="u-plot" ref={wrap}>
        <svg
          width={w} height={H + PAD_T + PAD_B} role="img" tabIndex={0}
          aria-label={hourly ? t("Tokens by hour. Use the arrow keys to read each bar.") : t("Tokens by day. Use the arrow keys to read each bar, Enter to open that day.")}
          onKeyDown={key} onMouseDown={(e) => e.preventDefault()} onMouseLeave={() => setHover(null)} onBlur={() => setHover(null)}
        >
          {[0, 0.5, 1].map((f) => (
            <g key={f}>
              <line className="u-grid" x1={PAD_L} x2={w - 4} y1={y(max * f)} y2={y(max * f)} />
              <text className="u-ax" x={PAD_L - 8} y={y(max * f) + 4} textAnchor="end">{f === 0 || totals.some((v) => v > 0) ? label(max * f) : ""}</text>
            </g>
          ))}
          {report.buckets.map((b, i) => {
            const x = PAD_L + slot * i + (slot - barW) / 2;
            let acc = 0;
            return (
              <g key={b.start} className={hover != null && hover !== i ? "dim" : ""}>
                {totals[i] === 0 && <rect className="u-zero" x={x} y={y(0) - 2} width={barW} height={2} rx={1} />}
                {values[i].map((v, si) => {
                  if (v <= 0) return null;
                  const y0 = y(acc), y1 = y(acc + v);
                  acc += v;
                  return <rect key={si} x={x} y={y1} width={barW} height={Math.max(1, y0 - y1)} fill={series[si].color} />;
                })}
                <rect
                  className="u-hit" x={PAD_L + slot * i} y={PAD_T} width={slot} height={H}
                  style={{ cursor: hourly ? "default" : "pointer" }}
                  onMouseEnter={() => setHover(i)}
                  onClick={() => !hourly && onPickDay(b.start)}
                />
                {i % every === 0 && <text className="u-ax" x={PAD_L + slot * i + slot / 2} y={H + PAD_T + 16} textAnchor="middle">{bucketLabel(b.start)}</text>}
              </g>
            );
          })}
        </svg>
        {hb && (
          <div className="u-tip" style={{ left: tipLeft }} role="status">
            <b>{bucketLabel(hb.start, true)}</b>
            <span className="u-tip-total">{label(totals[hover!])}</span>
            {series.map((s, si) => ({ s, v: values[hover!][si] })).filter((r) => r.v > 0).sort((a, b) => b.v - a.v).map(({ s, v }) => (
              <div key={s.key} className="u-tip-row"><i style={{ background: s.color }} /><span>{s.label}</span><em>{label(v)}</em></div>
            ))}
            {totals[hover!] === 0 && <div className="muted">{t("No activity")}</div>}
            {!hourly && totals[hover!] > 0 && <small className="muted">{t("Click to see this day by hour")}</small>}
          </div>
        )}
      </div>
      {series.length > 0 && (
        <div className="u-legend">
          {series.map((s) => <span key={s.key}><i style={{ background: s.color }} />{s.label}</span>)}
        </div>
      )}
    </div>
  );
}
