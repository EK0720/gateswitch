import { getCurrentWindow } from "@tauri-apps/api/window";
import { inTauri } from "./api";
import { t } from "./i18n";

// Windows draws no frame (decorations off in tauri.windows.conf.json), so the
// app supplies the drag strip and the three caption buttons. macOS keeps its
// own traffic lights over the sidebar (titleBarStyle Overlay); Linux keeps the
// system frame, so neither renders buttons here.
export const isWindows = navigator.userAgent.includes("Windows");

function win() {
  return inTauri ? getCurrentWindow() : null;
}

export function TitleBar() {
  return (
    <div className="titlebar" data-tauri-drag-region>
      {isWindows && inTauri && (
        <div className="caption">
          <button aria-label={t("Minimize")} title={t("Minimize")} onClick={() => win()?.minimize()}>
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 5h10" /></svg>
          </button>
          <button aria-label={t("Maximize")} title={t("Maximize")} onClick={() => win()?.toggleMaximize()}>
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M.5.5h9v9h-9z" /></svg>
          </button>
          <button className="close" aria-label={t("Close")} title={t("Close")} onClick={() => win()?.close()}>
            <svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" /></svg>
          </button>
        </div>
      )}
    </div>
  );
}
