import { useEffect, useMemo, useRef, useState } from "react";
import { Icon, IconName } from "./icons";
import { t } from "./i18n";

export type PaletteItem = { id: string; label: string; icon: IconName; hint?: string; keywords?: string; run: () => void };

/** Strip Vietnamese accents so "cai dat" finds "Cài đặt". */
const fold = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/đ/g, "d").replace(/Đ/g, "D").toLowerCase();

export function Palette({ items, onClose }: { items: PaletteItem[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => input.current?.focus(), []);

  const shown = useMemo(() => {
    const words = fold(q).split(/\s+/).filter(Boolean);
    return items.filter((i) => {
      const text = fold(`${i.label} ${i.hint ?? ""} ${i.keywords ?? ""}`);
      return words.every((w) => text.includes(w));
    });
  }, [q, items]);

  useEffect(() => setSel(0), [q]);

  function pick(i: PaletteItem | undefined) {
    if (!i) return;
    onClose();
    i.run();
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, shown.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === "Enter") pick(shown[sel]);
  }

  return (
    <div className="palette-bg" onMouseDown={onClose}>
      <div className="palette" role="dialog" aria-label={t("Search")} onMouseDown={(e) => e.stopPropagation()} onKeyDown={onKey}>
        <input ref={input} value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Go to a page or run a command…")} aria-label={t("Search")} />
        {shown.length === 0 ? (
          <div className="none">{t("No results")}</div>
        ) : (
          <ul role="listbox">
            {shown.map((i, n) => (
              <li key={i.id} role="option" aria-selected={n === sel} onMouseEnter={() => setSel(n)} onClick={() => pick(i)}>
                <Icon name={i.icon} />
                {i.label}
                {i.hint && <small>{i.hint}</small>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
