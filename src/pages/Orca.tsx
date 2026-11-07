import { useEffect, useState } from "react";
import { api, CodexCurrent, errText, OrcaStatus, Scan } from "../api";
import { Icon } from "../icons";
import { t } from "../i18n";
import { Info } from "./Info";

// Orca copies ~/.codex/config.toml into its own Codex home by itself, and its
// terminals use the normal Claude Code settings. So this page has no setup
// form: it shows whether Orca has the same gateway/model as Codex, offers to
// copy it right now, and restarts Orca's Codex daemon (the step that matters).
export function Orca({ scan, notify, goCodex }: { scan: Scan | null; notify: (t: string) => void; goCodex: () => void }) {
  const [status, setStatus] = useState<OrcaStatus | null>(null);
  const [main, setMain] = useState<CodexCurrent | null>(null);
  const [busy, setBusy] = useState<"" | "sync" | "restart">("");
  const [error, setError] = useState("");

  const installed = scan?.tools.find((x) => x.id === "orca")?.installed;

  function load() {
    Promise.all([api.orcaStatus(), api.codexCurrent()]).then(([st, cur]) => { setStatus(st); setMain(cur); });
  }
  useEffect(load, []);

  async function sync() {
    setBusy("sync"); setError("");
    try {
      notify((await api.orcaSync()).message);
      load();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy("");
    }
  }

  async function restart() {
    if (!confirm(t("Restart Orca's Codex daemon? Codex tasks running in Orca will stop."))) return;
    setBusy("restart"); setError("");
    try {
      notify(await api.orcaRestart());
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy("");
    }
  }

  const ready = !!status?.config && !!status.runtime;
  const noMain = main !== null && !main.provider;

  return (
    <>
      <div className="head">
        <h1>Orca<Info text={t("Orca copies ~/.codex/config.toml into its own Codex home, so the gateway, model and /model catalog follow the Codex page. Claude Code in Orca terminals uses the normal Claude Code settings and needs no extra setup.")} /></h1>
      </div>

      {installed === false && <div className="card alert"><Icon name="warn" /><div>{t("Orca not found on this machine.")}</div></div>}
      {installed && status && !status.config && (
        <div className="card alert"><Icon name="warn" /><div>{t("Open Codex in Orca once, then come back.")}</div></div>
      )}
      {noMain && (
        <div className="card alert">
          <Icon name="warn" />
          <div>{t("Codex is not using a gateway. Set it up on the Codex page and Orca follows.")}</div>
          <button className="btn small" style={{ marginLeft: "auto" }} onClick={goCodex}>{t("Open Codex page")}</button>
        </div>
      )}

      {status?.config && main?.provider && (
        <div className="card" style={{ padding: 0 }}>
          <div style={{ padding: "16px 18px" }}>
            <div className="now" style={{ margin: "0 0 12px" }}>
              <i className={`dot ${status.in_sync ? "ok" : "warn"}`} />
              {status.in_sync ? t("Orca uses the same config as Codex") : t("Orca has not picked up the new Codex config")}
              {!status.in_sync && (
                <button className="btn small" style={{ marginLeft: "auto" }} disabled={!!busy} onClick={sync}>
                  {busy === "sync" ? t("Syncing…") : t("Sync now")}
                </button>
              )}
            </div>
            <dl className="sync">
              <dt>{t("Codex")}</dt><dd>{main.model} · {main.provider}</dd>
              <dt>Orca</dt><dd>{status.model || "—"} · {status.provider || "—"}</dd>
              <dt>{t("/model menu")}</dt><dd>{status.catalog ? t("Catalog on") : t("No catalog")}</dd>
            </dl>
          </div>
        </div>
      )}

      {installed && ready && (
        <div className="probe-bar restart-bar">
          <span className="muted">{t("Orca's Codex daemon keeps the old config until it restarts. Then open a new Codex tab.")}</span>
          <button className="btn small" disabled={!!busy} onClick={restart}>
            {busy === "restart" ? t("Restarting…") : t("Restart daemon")}
          </button>
        </div>
      )}

      {error && <div className="card banner err" style={{ marginTop: 14 }}>{error}</div>}

    </>
  );
}
