import { Scan } from "../api";
import { t, tr } from "../i18n";
import { Info } from "./Info";

const HINT: Record<string, string> = {
  "codex-cli": "Cài: npm i -g @openai/codex",
  "codex-desktop": "Tải từ trang OpenAI Codex",
  "claude-cli": "Cài: npm i -g @anthropic-ai/claude-code",
  "claude-desktop": "Tải từ claude.com/download",
  orca: "Tải Orca từ trang phát hành",
};

export function Apps({ scan }: { scan: Scan | null }) {
  if (!scan) return (<><div className="head"><h1>{t("Apps")}</h1></div><div className="card empty">{t("Scanning…")}</div></>);

  const installed = scan.tools.filter((x) => x.installed);
  const missing = scan.tools.filter((x) => !x.installed);

  return (
    <>
      <div className="head"><h1>{t("Apps")}<Info text={t("The tool only looks to see which apps are on this machine. It never installs anything.")} /></h1></div>

      <div className="section-title">{t("Installed")} <span className="count">{installed.length}</span></div>
      <div className="card">
        {installed.map((x) => (
          <div className="issue" key={x.id}>
            <i className="dot ok" />
            <div><b>{x.name}</b>{x.path && <p>{x.path}</p>}</div>
            <span />
          </div>
        ))}
        {installed.length === 0 && <div className="empty">{t("No apps found.")}</div>}
      </div>

      {missing.length > 0 && (
        <>
          <div className="section-title">{t("Not installed")} <span className="count">{missing.length}</span></div>
          <div className="card">
            {missing.map((x) => (
              <div className="issue" key={x.id}>
                <i className="dot" />
                <div><b>{x.name}</b><p>{HINT[x.id] ? t(HINT[x.id]) : ""}</p></div>
                <span />
              </div>
            ))}
          </div>
        </>
      )}

      {scan.ides.length > 0 && (
        <>
          <div className="section-title">{t("Detected IDEs")} <span className="count">{scan.ides.length}</span></div>
          <div className="card">
            {scan.ides.map((ide) => (
              <div className="issue" key={ide.id}>
                <i className="dot ok" />
                <div>
                  <b>{ide.name}</b>
                  {ide.codex && <p><b className="tag">Codex</b> {tr(ide.codex)}</p>}
                  {ide.claude && <p><b className="tag">Claude</b> {tr(ide.claude)}</p>}
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
