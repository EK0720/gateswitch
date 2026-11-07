import { useEffect, useState } from "react";
import { api, Provider, Scan } from "../api";
import { Icon } from "../icons";
import { t, tr } from "../i18n";

type Row = { id: string; name: string; badge: string; color: string; tools: string[] };

type Link = {
  on: boolean;
  gateway: string;
  model: string;
  /** Extra note: stale env var, quota, daemon needing restart… */
  warn?: string;
};

export function Overview({ scan, onRefresh, go }: { scan: Scan | null; onRefresh: () => void; go: (r: string) => void }) {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [codex, setCodex] = useState<Awaited<ReturnType<typeof api.codexCurrent>> | null>(null);
  const [claude, setClaude] = useState<Awaited<ReturnType<typeof api.claudeCurrent>> | null>(null);
  const [desktop, setDesktop] = useState<Awaited<ReturnType<typeof api.claudeDesktopCurrent>> | null>(null);

  useEffect(() => {
    api.listProviders().then(setProviders).catch(() => setProviders([]));
    api.codexCurrent().then(setCodex).catch(() => setCodex(null));
    api.claudeCurrent().then(setClaude).catch(() => setClaude(null));
    api.claudeDesktopCurrent().then(setDesktop).catch(() => setDesktop(null));
  }, [scan]);

  const has = (id: string) => scan?.tools.find((x) => x.id === id)?.installed ?? false;
  const providerName = (pid: string) => providers.find((p) => p.id === pid)?.name ?? pid;
  const probeOf = (pid: string, model: string) => providers.find((p) => p.id === pid)?.probes?.[model];

  function codexLink(): Link {
    const cur = codex;
    if (!cur?.provider_id) return { on: false, gateway: "", model: "" };
    const pid = cur.provider_id;
    const probe = probeOf(pid, cur.model);
    return {
      on: true,
      gateway: providerName(pid) || cur.base_url,
      model: cur.model,
      warn: probe?.status === "quota" ? t("Model is out of quota") : undefined,
    };
  }

  function claudeLink(): Link {
    if (!claude?.base_url) return { on: false, gateway: "", model: "" };
    const viaSettings = !claude.auth;
    return {
      on: !!claude.auth,
      gateway: claude.base_url.replace(/^https?:\/\//, ""),
      model: claude.model || t("default model"),
      warn: viaSettings ? t("No key in settings.json, still using the claude.ai account") : claude.warnings[0] && tr(claude.warnings[0]),
    };
  }

  function desktopLink(): Link {
    const e = desktop?.entries.find((x) => x.applied);
    if (!e || !e.has_key) return { on: false, gateway: "", model: "" };
    return { on: true, gateway: e.base_url.replace(/^https?:\/\//, ""), model: e.default_model || t("discovered via /models"), warn: e.auth ? undefined : t("Key auth method unknown") };
  }

  function orcaLink(): Link {
    if (!has("orca")) return { on: false, gateway: "", model: "" };
    // Orca follows the main Codex home's catalog; report from the main setup.
    const l = codexLink();
    return l.on ? { ...l, warn: t("Shares the main Codex configuration") } : { on: false, gateway: "", model: "" };
  }

  const rows: (Row & { link: () => Link; page: string })[] = [
    { id: "codex", name: "Codex", badge: "Cx", color: "#111", tools: ["codex-cli", "codex-desktop"], page: "codex", link: codexLink },
    { id: "claude-code", name: "Claude Code", badge: "CC", color: "#C2633F", tools: ["claude-cli"], page: "claude-code", link: claudeLink },
    { id: "claude-desktop", name: "Claude Desktop", badge: "CD", color: "#8A5A3C", tools: ["claude-desktop"], page: "claude-desktop", link: desktopLink },
    { id: "orca", name: "Orca", badge: "Or", color: "#2F6FB0", tools: ["orca"], page: "orca", link: orcaLink },
  ];

  function surfaces(r: Row): string {
    const parts: string[] = [];
    if (r.id === "codex") {
      if (has("codex-cli")) parts.push("CLI");
      if (has("codex-desktop")) parts.push("Desktop");
    }
    if (r.id === "claude-code" && has("claude-cli")) parts.push("CLI");
    if (r.id === "orca") parts.push(t("Codex in Orca"));
    if (r.id === "codex" || r.id === "claude-code") {
      for (const ide of scan?.ides ?? []) {
        if (ide.uses.includes(r.id === "codex" ? "codex" : "claude")) parts.push(ide.name.replace(/ \(.*\)/, ""));
      }
    }
    return parts.join(", ");
  }

  const issues = rows
    .map((r) => ({ r, l: r.link() }))
    .filter(({ r, l }) => r.tools.some(has) && (!l.on || l.warn))
    .map(({ r, l }) => ({
      id: r.id,
      name: r.name,
      bad: !l.on,
      text: l.warn ?? t("App is installed but not connected to a gateway"),
      action: l.on ? undefined : t("Set up {name}", { name: r.name }),
    }));

  return (
    <>
      <div className="head">
        <h1>{t("Overview")}</h1>
        <div className="acts">
          <button className="btn primary" onClick={onRefresh}>
            <Icon name="play" />{t("Check all")}
          </button>
        </div>
      </div>

      <div className="card">
        {rows.map((r) => {
          const on = r.tools.some(has);
          const l = r.link();
          return (
            <div className="row" key={r.id} onClick={() => go(r.page)} style={{ cursor: "pointer" }} title={t("Open {name}", { name: r.name })}>
              <span className="appicon" style={{ background: r.color }}>{r.badge}</span>
              <div className="app-name">
                {r.name}
                <small>{scan ? (on ? surfaces(r) || t("Installed") : t("Not installed")) : t("Scanning…")}</small>
              </div>
              <div className={`wire ${l.on ? "" : "off"}`}><i /></div>
              <div className="target">
                {l.on ? (
                  <>
                    <b>{l.gateway}</b>
                    <span>{l.model}</span>
                  </>
                ) : (
                  <>
                    <b className="muted">{t("Not connected")}</b>
                    <span>{on ? t("Pick a provider to set up") : t("Install the app first")}</span>
                  </>
                )}
              </div>
              <div className="state">
                <i className={`dot ${l.on ? (l.warn ? "warn" : "ok") : ""}`} />
                {l.warn ? t("Check") : l.on ? t("Ready") : "—"}
              </div>
            </div>
          );
        })}
      </div>

      {issues.length > 0 && (
        <>
          <div className="section-title">{t("To fix")} <span className="count">{issues.length}</span></div>
          <div className="card">
            {issues.map((i) => (
              <div className="issue" key={i.id}>
                <i className={`dot ${i.bad ? "bad" : "warn"}`} />
                <div>
                  <b>{i.text}</b>
                  <p className="muted">{i.name}</p>
                </div>
                <button className="btn small" onClick={() => go(i.id === "orca" ? "orca" : i.id)}>{i.action ?? t("View")}</button>
              </div>
            ))}
          </div>
        </>
      )}

      {scan && scan.ides.length > 0 && (
        <>
          <div className="section-title">{t("Detected IDEs")} <span className="count">{scan.ides.length}</span></div>
          <div className="card">
            {scan.ides.map((ide) => (
              <div className="issue" key={ide.id}>
                <i className="dot ok" />
                <div>
                  <b>{ide.name}</b>
                  {ide.codex && <p><b className="tag">Codex</b> {ide.codex}</p>}
                  {ide.claude && <p><b className="tag">Claude</b> {ide.claude}</p>}
                </div>
                <span />
              </div>
            ))}
          </div>
        </>
      )}
    </>
  );
}
