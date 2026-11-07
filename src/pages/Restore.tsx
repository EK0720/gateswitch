import { useEffect, useState } from "react";
import { api, Change, errText, ResetItem } from "../api";
import { Icon } from "../icons";
import { locale, t, tr } from "../i18n";
import { Info } from "./Info";

function when(iso: string) {
  const d = new Date(iso);
  const today = new Date();
  const day = d.toDateString() === today.toDateString() ? t("Today") : d.toLocaleDateString(locale(), { day: "numeric", month: "numeric" });
  return [day, d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })];
}

const APPS: [string, string][] = [
  ["claude-code", "Claude Code"],
  ["codex", "Codex"],
  ["orca", "Orca"],
  ["claude-desktop", "Claude Desktop"],
];
const appName = (id: string) => APPS.find(([a]) => a === id)?.[1] ?? id;

/** "Khôi phục mặc định": one step back to before this tool, however many setups ran. */
function ResetCard({ notify, onDone }: { notify: (t: string) => void; onDone: () => void }) {
  const [apps, setApps] = useState<string[]>(APPS.map(([a]) => a));
  const [mode, setMode] = useState<"clean" | "original">("clean");
  const [items, setItems] = useState<ResetItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    setItems(null); setError("");
    if (apps.length === 0) { setItems([]); return; }
    api.resetPreview({ apps, mode }).then(setItems).catch((e) => setError(errText(e)));
  }, [apps.join(","), mode]);

  async function apply() {
    if (!confirm(t("Restore the selected apps to defaults? This is recorded in the history and can be undone. Restart the apps afterwards."))) return;
    setBusy(true); setError("");
    try {
      await api.resetApply({ apps, mode });
      notify(t("Defaults restored"));
      onDone();
      setItems(await api.resetPreview({ apps, mode }));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }

  const toggle = (a: string) => setApps((cur) => (cur.includes(a) ? cur.filter((x) => x !== a) : [...cur, a]));

  return (
    <div className="card">
      <div className="issue">
        <div>
          <b>{t("Restore defaults")}<Info text={t("One step back to how the apps were before this tool, however many setups ran. This can be undone too.")} /></b>
        </div>
        <span />
        <span />
      </div>
      <div className="form" style={{ paddingTop: 0 }}>
        <div className="field">
          <label>{t("App")}</label>
          <div className="checks">
            {APPS.map(([id, name]) => (
              <label key={id} className="check-item">
                <input type="checkbox" checked={apps.includes(id)} onChange={() => toggle(id)} />{name}
              </label>
            ))}
          </div>
        </div>
        <div className="field">
          <label>{t("Restore mode")}
            <small>{mode === "clean" ? t("Only removes what the tool added and keeps all your other edits") : t("Puts files back exactly as they were before the tool first wrote them. Later edits are lost")}</small>
          </label>
          <div className="seg" role="group">
            <button aria-pressed={mode === "clean"} onClick={() => setMode("clean")}>{t("Remove what the tool added")}</button>
            <button aria-pressed={mode === "original"} onClick={() => setMode("original")}>{t("Back to original")}</button>
          </div>
        </div>
      </div>
      {error && <div className="banner err">{error}</div>}
      {items === null && !error && <div className="empty" style={{ padding: 20 }}>{t("Previewing…")}</div>}
      {items && items.length === 0 && <div className="empty" style={{ padding: 20 }}>{t("The selected apps are already at their defaults.")}</div>}
      {items && items.length > 0 && (
        <div className="reset-list">
          {items.map((i, n) => (
            <div key={n} className="reset-item">
              <b>{appName(i.app)}</b>
              <span className="muted">{i.path}</span>
              <ul>{i.changes.map((c, k) => <li key={k}>{tr(c)}</li>)}</ul>
            </div>
          ))}
        </div>
      )}
      <div className="form-acts" style={{ padding: "12px 18px" }}>
        <button className="btn primary" disabled={busy || !items || items.length === 0} onClick={apply}>
          {busy ? t("Restoring…") : t("Restore")}
        </button>
      </div>
    </div>
  );
}

export function Restore({ history, onUndo, notify, onChanged }: { history: Change[]; onUndo: (id: string) => void; notify: (t: string) => void; onChanged: () => void }) {
  return (
    <>
      <div className="head">
        <h1>{t("Restore")}</h1>
      </div>

      <ResetCard notify={notify} onDone={onChanged} />

      <div className="section-title">{t("Change history")}</div>
      <div className="card">
        {history.length === 0 && <div className="empty">{t("No changes yet. Every setup shows up here and can be undone.")}</div>}
        {history.map((c) => {
          const [day, time] = when(c.time);
          return (
            <div className="hist" key={c.id}>
              <time>{day}<br />{time}</time>
              <div><b>{tr(c.title)}</b><p>{(c.detail && tr(c.detail)) || c.files.map((f) => f.path.split(/[\\/]/).pop()).join(", ")}</p></div>
              <button className="btn small" disabled={c.undone} onClick={() => onUndo(c.id)}>
                {c.undone ? t("Undone") : <><Icon name="undo" />{t("Undo")}</>}
              </button>
            </div>
          );
        })}
      </div>
    </>
  );
}
