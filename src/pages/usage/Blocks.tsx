import type { BlocksReport, UsageBlock } from "../../api";
import { t } from "../../i18n";
import { all, dateTimeLabel, duration, fmt, timeLabel } from "./util";

export function Blocks({ data, limit }: { data: BlocksReport; limit: number | null }) {
  const blocks = [...data.blocks].reverse().slice(0, 60);
  if (!blocks.length) return <div className="card empty">{t("No 5-hour blocks in this range.")}</div>;
  const scale = limit ?? Math.max(1, ...blocks.map((b) => Math.max(all(b), b.projected ?? 0)));
  return (
    <>
      <p className="muted u-note">
        {limit
          ? t("Each block starts at the first call after the previous 5-hour window ended. Bars are measured against your limit of {n} tokens.", { n: fmt(limit) })
          : t("Each block starts at the first call after the previous 5-hour window ended. Set a block limit in Settings › Notifications & background to compare against it.")}
      </p>
      <div className="card">
        {blocks.map((b) => <BlockRow key={b.start} b={b} scale={scale} limit={limit} />)}
      </div>
    </>
  );
}

function BlockRow({ b, scale, limit }: { b: UsageBlock; scale: number; limit: number | null }) {
  const v = all(b);
  const w = Math.min(100, (v / scale) * 100);
  const over = limit != null && v > limit;
  const proj = b.projected != null ? Math.min(100, (b.projected / scale) * 100) : null;
  const end = Date.parse(b.end);
  return (
    <div className={`u-block ${b.active ? "active" : ""}`}>
      <div className="u-block-time">
        <b>{dateTimeLabel(b.start)} – {timeLabel(b.end)}</b>
        <small className="muted">
          {b.active
            ? <>{t("Active")} · {t("{d} left", { d: duration(end - Date.now()) })}{b.tokens_per_min != null && <> · {t("{n}/min", { n: fmt(b.tokens_per_min) })}</>}</>
            : t("{n} turns", { n: b.turns.toLocaleString() })}
        </small>
      </div>
      <div className="u-meter" title={proj != null ? t("Projected {n} by the end of the block", { n: fmt(b.projected!) }) : undefined}>
        <i className={over ? "over" : ""} style={{ width: `${Math.max(0.8, w)}%` }} />
        {proj != null && proj > w && <s style={{ left: `${proj}%` }} />}
      </div>
      <em>{fmt(v)}{limit ? <small className="muted"> · {Math.round((v / limit) * 100)}%</small> : null}</em>
    </div>
  );
}
