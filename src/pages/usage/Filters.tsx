import { ReactNode, useEffect, useRef, useState } from "react";
import type { UsageApp } from "../../api";
import { t } from "../../i18n";
import { APPS, dayLabel, Range, RangeKind, today } from "./util";

/** <details> popover that closes on outside click and Escape. */
export function Pop({ summary, label, className, children }: { summary: ReactNode; label: string; className?: string; children: (close: () => void) => ReactNode }) {
  const ref = useRef<HTMLDetailsElement>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => { if (ref.current && !ref.current.contains(e.target as Node)) ref.current.open = false; };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape" && ref.current) { ref.current.open = false; ref.current.querySelector("summary")?.focus(); } };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", esc); };
  }, [open]);
  return (
    <details className={`menu ${className ?? ""}`} ref={ref} onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}>
      <summary aria-label={label} title={label}>{summary}</summary>
      {open && <div className="menu-pop u-pop">{children(() => { if (ref.current) ref.current.open = false; })}</div>}
    </details>
  );
}

export function RangePicker({ range, onChange }: { range: Range; onChange: (r: Range) => void }) {
  const kinds: { k: Exclude<RangeKind, "custom">; label: string }[] = [
    { k: "today", label: t("Today") },
    { k: "7d", label: t("7 days") },
    { k: "30d", label: t("30 days") },
    { k: "month", label: t("This month") },
  ];
  const [from, setFrom] = useState(range.from ?? today());
  const [to, setTo] = useState(range.to ?? today());
  const custom = range.kind === "custom";
  const customLabel = custom && range.from
    ? range.from === range.to ? dayLabel(range.from) : `${dayLabel(range.from)} – ${range.to ? dayLabel(range.to) : "…"}`
    : t("Custom");
  return (
    <div className="seg u-range" role="group" aria-label={t("Date range")}>
      {kinds.map(({ k, label }) => (
        <button key={k} aria-pressed={range.kind === k} onClick={() => onChange({ kind: k, from: null, to: null })}>{label}</button>
      ))}
      <Pop className={`u-custom ${custom ? "on" : ""}`} label={t("Custom range")} summary={<>{customLabel} ▾</>}>
        {(close) => (
          <form className="u-custom-form" onSubmit={(e) => { e.preventDefault(); if (from && to) { onChange({ kind: "custom", from: from <= to ? from : to, to: from <= to ? to : from }); close(); } }}>
            <label>{t("From")}<input className="input" type="date" value={from} max={today()} onChange={(e) => setFrom(e.target.value)} required /></label>
            <label>{t("To")}<input className="input" type="date" value={to} max={today()} onChange={(e) => setTo(e.target.value)} required /></label>
            <button className="btn primary small" type="submit">{t("Apply")}</button>
          </form>
        )}
      </Pop>
    </div>
  );
}

function Chip({ label, value, options, onChange }: { label: string; value: string | null; options: { v: string; label: string }[]; onChange: (v: string | null) => void }) {
  const cur = options.find((o) => o.v === value);
  return (
    <span className={`u-chip ${value ? "on" : ""}`}>
      <select aria-label={label} value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}>
        <option value="">{label}</option>
        {options.map((o) => <option key={o.v} value={o.v}>{o.label}</option>)}
      </select>
      <span className="u-chip-text" aria-hidden="true">{cur ? cur.label : label} ▾</span>
      {value && <button className="u-chip-x" aria-label={t("Clear {name}", { name: cur?.label ?? value })} onClick={() => onChange(null)}>✕</button>}
    </span>
  );
}

export function Filters({ app, project, model, projects, models, onChange }: {
  app: UsageApp | null; project: string | null; model: string | null;
  projects: string[]; models: string[];
  onChange: (f: { app?: UsageApp | null; project?: string | null; model?: string | null }) => void;
}) {
  const any = app || project || model;
  return (
    <div className="u-filters" role="group" aria-label={t("Filters")}>
      <Chip label={t("All apps")} value={app} options={APPS.map((a) => ({ v: a.key, label: a.name }))} onChange={(v) => onChange({ app: v as UsageApp | null })} />
      <Chip label={t("All projects")} value={project} options={projects.map((p) => ({ v: p, label: p }))} onChange={(v) => onChange({ project: v })} />
      <Chip label={t("All models")} value={model} options={models.map((m) => ({ v: m, label: m }))} onChange={(v) => onChange({ model: v })} />
      {any && <button className="btn small ghost" onClick={() => onChange({ app: null, project: null, model: null })}>{t("Clear filters")}</button>}
    </div>
  );
}
