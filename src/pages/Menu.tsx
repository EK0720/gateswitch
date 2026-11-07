import { ReactNode, useEffect, useRef } from "react";
import { t } from "../i18n";

export type MenuItem = { label: string; run: () => void; danger?: boolean; disabled?: boolean; hidden?: boolean };

// ⋯ menu for secondary actions, so each page keeps one primary button.
export function Menu({ items, label }: { items: MenuItem[]; label?: ReactNode }) {
  const ref = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    function close(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) ref.current.open = false;
    }
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, []);
  const shown = items.filter((i) => !i.hidden);
  if (!shown.length) return null;
  return (
    <details className="menu" ref={ref}>
      <summary className="btn" aria-label={t("More actions")} title={t("More actions")}>{label ?? "⋯"}</summary>
      <div className="menu-pop" role="menu">
        {shown.map((i) => (
          <button
            key={i.label}
            role="menuitem"
            className={i.danger ? "danger" : ""}
            disabled={i.disabled}
            onClick={() => { if (ref.current) ref.current.open = false; i.run(); }}
          >
            {i.label}
          </button>
        ))}
      </div>
    </details>
  );
}
