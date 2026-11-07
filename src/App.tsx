import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, Change, errText, Scan } from "./api";
import { Icon, IconName } from "./icons";
import { Overview } from "./pages/Overview";
import { Restore } from "./pages/Restore";
import { Providers } from "./pages/Providers";
import { Codex } from "./pages/Codex";
import { ClaudeCode } from "./pages/ClaudeCode";
import { ClaudeDesktop } from "./pages/ClaudeDesktop";
import { Orca } from "./pages/Orca";
import { Usage } from "./pages/Usage";
import { Apps } from "./pages/Apps";
import { Settings } from "./pages/Settings";
import { Palette, PaletteItem } from "./Palette";
import { TitleBar } from "./TitleBar";
import { getLang, Lang, setLang, t, tr } from "./i18n";
import { autoCheck, checkForUpdate, currentVersion, dismiss, installUpdate, isDismissed, useUpdate } from "./updater";

type Route =
  | "overview" | "providers" | "codex" | "claude-code" | "claude-desktop" | "orca"
  | "usage" | "apps" | "restore" | "settings";

type NavItem = { id: Route; label: string; icon: IconName; tool?: string };

const groups = (): NavItem[][] => [
  [
    { id: "overview", label: t("Overview"), icon: "home" },
    { id: "providers", label: t("Providers"), icon: "plug" },
  ],
  [
    { id: "codex", label: "Codex", icon: "term", tool: "codex-cli" },
    { id: "claude-code", label: "Claude Code", icon: "spark", tool: "claude-cli" },
    { id: "claude-desktop", label: "Claude Desktop", icon: "win", tool: "claude-desktop" },
    { id: "orca", label: "Orca", icon: "orca", tool: "orca" },
  ],
  [
    { id: "usage", label: t("Usage"), icon: "chart" },
    { id: "apps", label: t("Apps"), icon: "box" },
    { id: "restore", label: t("Restore"), icon: "undo" },
    { id: "settings", label: t("Settings"), icon: "gear" },
  ],
];

export default function App() {
  const [route, setRoute] = useState<Route>("overview");
  const [scan, setScan] = useState<Scan | null>(null);
  const [history, setHistory] = useState<Change[]>([]);
  const [dark, setDark] = useState(() => {
    try {
      const saved = localStorage.getItem("theme");
      if (saved) return saved === "dark";
    } catch {}
    return matchMedia("(prefers-color-scheme: dark)").matches;
  });
  const [toast, setToast] = useState("");
  const [palette, setPalette] = useState(false);
  const [lang, setLangState] = useState<Lang>(getLang());
  const [version, setVersion] = useState("");
  const update = useUpdate();

  useEffect(() => {
    currentVersion().then(setVersion).catch(() => {});
    if (!autoCheck()) return;
    const id = window.setTimeout(checkForUpdate, 4000);
    return () => window.clearTimeout(id);
  }, []);

  function switchLang(next: Lang) {
    setLang(next);
    setLangState(next);
  }

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);

  function toggleTheme() {
    const next = !dark;
    setDark(next);
    try { localStorage.setItem("theme", next ? "dark" : "light"); } catch {}
  }

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function notify(text: string) {
    text = tr(text); // backend messages are Vietnamese
    setToast(text);
    window.setTimeout(() => setToast(""), 2400);
  }

  function refresh() {
    api.scan().then(setScan).catch((e) => notify(errText(e)));
    api.history().then(setHistory).catch(() => setHistory([]));
  }
  useEffect(refresh, []);

  const installed = (id?: string) => scan?.tools.find((t) => t.id === id)?.installed;

  const commands: PaletteItem[] = [
    ...groups().flat().map((n) => ({ id: n.id, label: n.label, icon: n.icon, hint: t("Page"), keywords: n.id, run: () => setRoute(n.id) })),
    { id: "rescan", label: t("Rescan this machine"), icon: "refresh", hint: t("Command"), keywords: "scan quet", run: () => { refresh(); notify(t("Rescanned")); } },
    { id: "doctor", label: t("Run Doctor"), icon: "warn", hint: t("Command"), keywords: "loi kiem tra suc khoe doctor", run: () => setRoute("settings") },
    { id: "report", label: t("Create support report"), icon: "dl", hint: t("Command"), keywords: "support bao cao report", run: () => setRoute("settings") },
    { id: "theme", label: dark ? t("Switch to light theme") : t("Switch to dark theme"), icon: dark ? "sun" : "moon", hint: t("Command"), keywords: "theme dark light", run: toggleTheme },
    { id: "lang", label: lang === "vi" ? "Switch to English" : "Chuyển sang tiếng Việt", icon: "gear", hint: t("Command"), keywords: "language ngon ngu english tieng viet", run: () => switchLang(lang === "vi" ? "en" : "vi") },
  ];

  return (
    <div className="app" key={lang}>
      <TitleBar />
      <aside className="side" aria-label={t("Navigation")}>
        <div className="drag" data-tauri-drag-region />
        <div className="brand">
          <span className="logo"><Icon name="plug" /></span>Model Switcher
        </div>
        <div className="search" role="button" tabIndex={0} onClick={() => setPalette(true)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") setPalette(true); }}>
          <Icon name="search" />{t("Search")}<kbd>Ctrl K</kbd>
        </div>
        {groups().map((g, gi) => (
          <div key={gi}>
            {gi > 0 && <div className="gap" />}
            {g.map((n) => (
              <button
                key={n.id}
                className="nav"
                aria-current={route === n.id ? "page" : undefined}
                onClick={() => setRoute(n.id)}
              >
                <Icon name={n.icon} />
                {n.label}
                {n.tool && <i className={`dot ${installed(n.tool) ? "ok" : ""}`} />}
              </button>
            ))}
          </div>
        ))}
        <div className="foot">
          {version ? `v${version}` : ""}
          <span className="lang" role="group" aria-label={t("Language")}>
            <button aria-pressed={lang === "en"} onClick={() => switchLang("en")}>EN</button>
            <button aria-pressed={lang === "vi"} onClick={() => switchLang("vi")}>VI</button>
          </span>
          <button className="theme" aria-label={t("Toggle light/dark theme")} onClick={toggleTheme}>
            <Icon name={dark ? "sun" : "moon"} />
          </button>
        </div>
      </aside>

      <main>
        {update.kind === "available" && !isDismissed(update) && (
          <div className="update-bar" role="status">
            <Icon name="dl" />
            <span>{t("Version {v} is available.", { v: update.version })}</span>
            <button className="link" onClick={() => openUrl(`https://github.com/EK0720/model-switcher/releases/tag/v${update.version}`).catch(() => {})}>{t("What's new")}</button>
            <span className="grow" />
            <button className="btn ghost small" onClick={dismiss}>{t("Later")}</button>
            <button className="btn primary small" onClick={installUpdate}>{t("Update and restart")}</button>
          </div>
        )}
        {update.kind === "downloading" && (
          <div className="update-bar" role="status">
            <Icon name="dl" />
            <span>{t("Downloading {v}…", { v: update.version })} {update.total ? `${Math.round((update.done / update.total) * 100)}%` : ""}</span>
          </div>
        )}
        <section className="page on" key={route}>
          {route === "overview" && (
            <Overview scan={scan} onRefresh={() => { refresh(); notify(t("Rescanned")); }} go={(r) => setRoute(r as Route)} />
          )}
          {route === "restore" && (
            <Restore
              history={history}
              notify={notify}
              onChanged={refresh}
              onUndo={async (id) => {
                try {
                  await api.undo(id);
                  notify(t("Undone"));
                  refresh();
                } catch (e) {
                  notify(errText(e));
                }
              }}
            />
          )}
          {route === "providers" && <Providers notify={notify} />}
          {route === "codex" && <Codex scan={scan} notify={(m) => { notify(m); refresh(); }} goProviders={() => setRoute("providers")} />}
          {route === "claude-code" && <ClaudeCode scan={scan} notify={(m) => { notify(m); refresh(); }} goProviders={() => setRoute("providers")} />}
          {route === "claude-desktop" && <ClaudeDesktop scan={scan} notify={(m) => { notify(m); refresh(); }} goProviders={() => setRoute("providers")} />}
          {route === "orca" && <Orca scan={scan} notify={(m) => { notify(m); refresh(); }} goCodex={() => setRoute("codex")} />}
          {route === "usage" && <Usage />}
          {route === "apps" && <Apps scan={scan} />}
          {route === "settings" && <Settings dark={dark} onToggleTheme={toggleTheme} notify={(m) => { notify(m); refresh(); }} go={(r) => setRoute(r as Route)} onLang={switchLang} />}
        </section>
      </main>

      {palette && <Palette items={commands} onClose={() => setPalette(false)} />}

      <div className={`toast ${toast ? "on" : ""}`} role="status">
        {toast && <Icon name="check" />}
        {toast}
      </div>
    </div>
  );
}
