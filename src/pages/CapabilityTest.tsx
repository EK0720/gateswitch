import { CapCheck, Caps } from "../api";
import { t, tr } from "../i18n";

export const MAX_CAP_MODELS = 8;

// Built at render time so t() follows the current language.
export const capCols = (): [keyof Caps, string, string][] => [
  ["endpoint", "API", t("/responses for Codex, /messages for Claude")],
  ["streaming", "Stream", t("Streams partial output (SSE)")],
  ["tools", "Tool", t("Can call tools/functions")],
  ["caching", "Cache", t("Second call reports cached tokens")],
  ["images", t("Images"), t("Accepts image input")],
];

function title(label: string, hint: string, c: CapCheck) {
  const why = c.note ? tr(c.note) : c.latency_ms != null ? `${c.latency_ms} ms` : "";
  return `${label}: ${hint}${why ? " · " + why : ""}`;
}

/** Compact capability result for one model, shown in its table row. */
export function CapPills({ caps }: { caps: Caps }) {
  return (
    <span className="caps">
      {capCols().map(([k, label, hint]) => {
        const c = caps[k];
        if (!c) return null;
        return <span key={k} className={`cap ${c.ok ? "y" : "n"}`} title={title(label, hint, c)}>{c.ok ? "✓" : "✗"} {label}</span>;
      })}
    </span>
  );
}
