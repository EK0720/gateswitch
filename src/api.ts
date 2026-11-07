import { invoke } from "@tauri-apps/api/core";
import { tr } from "./i18n";

export type Tool = { id: string; name: string; installed: boolean; path: string | null };
export type Ide = { id: string; name: string; uses: string[]; codex: string | null; claude: string | null };
export type Scan = { os: "windows" | "macos" | "linux"; tools: Tool[]; ides: Ide[] };
export type FileChange = { path: string; backup: string | null; after_sha: string };
export type Change = { id: string; time: string; title: string; detail: string; files: FileChange[]; undone: boolean };
export type ProbeStatus = "ok" | "quota" | "auth" | "missing" | "upstream" | "timeout" | "error";
export type Probe = { status: ProbeStatus; latency_ms: number | null; message: string; checked_at: string };
export type ModelInfo = { owned_by: string | null; capabilities: string[] };
export type Provider = {
  id: string; name: string; base_url: string; models: string[];
  latency_ms: number | null; checked_at: string | null; has_key: boolean;
  info: Record<string, ModelInfo>; probes: Record<string, Probe>;
  headers?: Record<string, string>;
};
export type CodexCurrent = { model: string; provider: string; base_url: string; env_key: string; effort: string; catalog: boolean; provider_id: string };
export type CodexNames = { key: string; env: string; old_keys: string[]; old_envs: string[] };
export type CodexRequest = { provider_id: string; model: string; effort: string | null; catalog: boolean; images: boolean };

export type ClaudeCurrent = { model: string; base_url: string; auth: string; effort: string; picker: boolean; warnings: string[] };
export type ClaudeRequest = { provider_id: string; model: string; auth: string | null; effort: string | null; picker: boolean };

export type DesktopEntry = { id: string; ours: boolean; name: string; base_url: string; auth: string; has_key: boolean; default_model: string; model_count: number; discovery: boolean | null; headers: number; applied: boolean };
export type DesktopCurrent = { entries: DesktopEntry[]; effective: string | null; warnings: string[] };
export type DesktopRequest = { provider_id: string; model: string; auth: string | null; one_m: boolean; all_models: boolean };

export type ImageGuidance = { path: string; installed: boolean; outdated: boolean; models: Record<string, string> };

export type OrcaStatus = { config: boolean; runtime: string | null; catalog: string | null; provider: string; model: string; in_sync: boolean };

export type Totals = { input: number; cached: number; output: number };
export type DayStat = { day: string; input: number; cached: number; output: number };
export type Usage = { claude: Totals; codex: Totals; orca: Totals; days: DayStat[]; by_model: Record<string, Totals>; files: [number, number, number] };
export type Check = { level: "ok" | "warn" | "bad"; app: string; title: string; detail: string; fix?: string | null };

export const inTauri = "__TAURI_INTERNALS__" in window;

// Browser preview (vite without Tauri) gets sample data so the UI can be reviewed.
const sample: Scan = {
  os: "windows",
  tools: [
    { id: "codex-cli", name: "Codex CLI", installed: true, path: "codex" },
    { id: "codex-desktop", name: "Codex Desktop", installed: true, path: null },
    { id: "claude-cli", name: "Claude Code CLI", installed: true, path: "claude" },
    { id: "claude-desktop", name: "Claude Desktop", installed: true, path: null },
    { id: "orca", name: "Orca", installed: true, path: null },
  ],
  ides: [
    { id: "vscode", name: "VS Code", uses: ["codex", "claude"], codex: "Install the Codex (OpenAI) extension to use it.", claude: "Install the Claude Code (Anthropic) extension to use it." },
    { id: "antigravity", name: "Antigravity", uses: ["codex", "claude"], codex: "Install the Codex (OpenAI) extension to use it.", claude: "Install the Claude Code (Anthropic) extension to use it." },
    { id: "jetbrains", name: "JetBrains (Rider, IntelliJ…)", uses: ["codex", "claude"], codex: "Choose Codex in AI Chat.", claude: "Choose Claude Agent in AI Chat." },
  ],
};
// Fictional gateways and models for the browser preview and screenshots.
const sampleModels = ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5", "gpt-6", "gpt-6-mini", "gemini-3-pro", "deepseek-v4", "qwen-4-coder", "image-gen-2"];
const sampleNow = new Date().toISOString();
const ok = (ms: number): Probe => ({ status: "ok", latency_ms: ms, message: "", checked_at: sampleNow });
let sampleProviders: Provider[] = [
  {
    id: "northwind", name: "Northwind AI", base_url: "https://api.northwind.example/v1",
    models: sampleModels,
    latency_ms: 142, checked_at: sampleNow, has_key: true,
    info: Object.fromEntries(sampleModels.map((m) => [m, { owned_by: m.split("-")[0], capabilities: [] }])),
    probes: {
      "claude-opus-5-5": ok(1180), "claude-sonnet-5-5": ok(640), "claude-haiku-5-5": ok(310),
      "gpt-6": ok(890), "gpt-6-mini": { status: "quota", latency_ms: null, message: "HTTP 429: usage limit reached", checked_at: sampleNow },
      "gemini-3-pro": ok(720), "deepseek-v4": ok(560),
    },
  },
  {
    id: "local", name: "Local router", base_url: "http://localhost:4000/v1",
    models: ["qwen-4-coder", "llama-5-70b"], latency_ms: 9, checked_at: sampleNow, has_key: true,
    info: {}, probes: {},
  },
  {
    id: "acme", name: "Acme Cloud", base_url: "https://llm.acme.example/v1",
    models: ["gpt-6", "claude-sonnet-5-5"], latency_ms: 233, checked_at: sampleNow, has_key: true,
    info: {}, probes: {},
  },
];

function sampleUsage(): Usage {
  const days: DayStat[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(Date.now() - i * 864e5);
    const w = d.getDay() === 0 || d.getDay() === 6 ? 0.35 : 1;
    const input = Math.round((1.2e6 + 0.9e6 * Math.sin(i / 3) ** 2) * w);
    days.push({ day: d.toISOString().slice(0, 10), input, cached: Math.round(input * 0.86), output: Math.round(input * 0.03) });
  }
  const by = (i: number, c: number, o: number): Totals => ({ input: i, cached: c, output: o });
  return {
    claude: by(18.4e6, 16.1e6, 0.52e6), codex: by(21.7e6, 18.9e6, 0.61e6), orca: by(2.1e6, 1.7e6, 0.07e6), days,
    by_model: { "claude-opus-5-5": by(11.2e6, 9.9e6, 0.31e6), "gpt-6": by(14.8e6, 12.9e6, 0.41e6), "claude-sonnet-5-5": by(7.2e6, 6.2e6, 0.21e6), "gpt-6-mini": by(6.9e6, 6.0e6, 0.2e6), "gemini-3-pro": by(2.1e6, 1.7e6, 0.07e6) },
    files: [214, 167, 23],
  };
}

function preview<T>(value: T): Promise<T> {
  return new Promise((r) => setTimeout(() => r(value), 300));
}

function probeModelsPreview(id: string, models: string[] | null): Promise<Provider> {
  const p = sampleProviders.find((x) => x.id === id)!;
  const now = new Date().toISOString();
  const fake = (m: string): Probe =>
    m.startsWith("cx/") ? { status: "quota", latency_ms: null, message: "HTTP 429: usage limit reached", checked_at: now }
    : m.startsWith("gcli/") ? { status: "timeout", latency_ms: null, message: "No response after 30 seconds", checked_at: now }
    : { status: "ok", latency_ms: 400 + m.length * 20, message: "", checked_at: now };
  for (const m of models ?? p.models) p.probes[m] = fake(m);
  return new Promise((r) => setTimeout(() => r({ ...p, probes: { ...p.probes } }), 1200));
}

export const api = {
  scan: (): Promise<Scan> => (inTauri ? invoke("scan") : preview(sample)),
  history: (): Promise<Change[]> => (inTauri ? invoke("history") : preview([])),
  undo: (id: string): Promise<Change> => invoke("undo", { id }),

  listProviders: (): Promise<Provider[]> => (inTauri ? invoke("list_providers") : preview(sampleProviders)),
  saveProvider: (p: { id: string | null; name: string; baseUrl: string; key: string | null }): Promise<Provider> => {
    if (inTauri) return invoke("save_provider", { id: p.id, name: p.name, baseUrl: p.baseUrl, key: p.key });
    const np: Provider = { id: p.id ?? p.name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), name: p.name, base_url: p.baseUrl.replace(/\/+$/, ""), models: [], latency_ms: null, checked_at: null, has_key: !!p.key, info: {}, probes: {} };
    sampleProviders = [...sampleProviders.filter((x) => x.id !== np.id), np];
    return preview(np);
  },
  setProviderHeaders: (id: string, text: string): Promise<Provider> => {
    if (inTauri) return invoke("set_provider_headers", { id, text });
    const headers = Object.fromEntries(text.split("\n").map((l) => l.split(":")).filter((p) => p.length > 1).map(([k, ...v]) => [k.trim(), v.join(":").trim()]));
    sampleProviders = sampleProviders.map((x) => (x.id === id ? { ...x, headers } : x));
    return preview(sampleProviders.find((x) => x.id === id)!);
  },
  removeProvider: (id: string): Promise<void> => {
    if (inTauri) return invoke("remove_provider", { id });
    sampleProviders = sampleProviders.filter((x) => x.id !== id);
    return preview(undefined);
  },
  refreshModels: (id: string): Promise<Provider> =>
    inTauri ? invoke("refresh_models", { id }) : preview(sampleProviders.find((p) => p.id === id)!),

  probeOne: (id: string, model: string): Promise<Provider> => {
    if (inTauri) return invoke("probe_models", { id, models: [model] });
    return probeModelsPreview(id, [model]);
  },

  probeModels: (id: string, models: string[] | null): Promise<Provider> => {
    if (inTauri) return invoke("probe_models", { id, models });
    return probeModelsPreview(id, models);
  },

  claudeCurrent: (): Promise<ClaudeCurrent> =>
    inTauri ? invoke("claude_current") : preview({ model: "claude-opus-5-5", base_url: "https://api.northwind.example", auth: "bearer", effort: "", picker: true, warnings: [] }),
  claudeSetup: (req: ClaudeRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_setup", { req }) : preview({ change: { id: "y", time: new Date().toISOString(), title: "Setup Claude Code", detail: "", files: [], undone: false }, message: "Claude Code set up" }),

  orcaStatus: (): Promise<OrcaStatus> =>
    inTauri ? invoke("orca_status") : preview({ config: true, runtime: "C:/…/codex.exe", catalog: null, provider: "gateway", model: "gpt-6", in_sync: true }),
  orcaSync: (): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("orca_sync") : preview({ change: { id: "o", time: new Date().toISOString(), title: "Sync Codex to Orca", detail: "", files: [], undone: false }, message: "Synced. Restart the daemon for Orca to apply it." }),
  claudeDesktopRemove: (ids: string[]): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_desktop_remove", { ids }) : preview({ change: { id: "d", time: new Date().toISOString(), title: "Remove Claude Desktop configurations", detail: "", files: [], undone: false }, message: `Removed ${ids.length} configuration(s). Quit and reopen Claude Desktop.` }),
  codexNames: (id: string): Promise<CodexNames> =>
    inTauri ? invoke("codex_names", { id }) : preview({ key: "gateway", env: "GATEWAY_API_KEY", old_keys: [], old_envs: [] }),
  setCodexNames: (id: string, name: string, env: string): Promise<CodexNames> =>
    inTauri ? invoke("set_codex_names", { id, name, env }) : preview({ key: name || "gateway", env: env || "GATEWAY_API_KEY", old_keys: [], old_envs: [] }),

  claudeDesktopCurrent: (): Promise<DesktopCurrent> =>
    inTauri ? invoke("claude_desktop_current") : preview({ entries: [{ id: "e0", ours: false, name: "Default", base_url: "https://api.anthropic.com", auth: "bearer", has_key: true, default_model: "", model_count: 0, discovery: true, headers: 0, applied: false }, { id: "e1", ours: true, name: "", base_url: "https://api.northwind.example/v1", auth: "bearer", has_key: true, default_model: "claude-opus-5-5", model_count: 9, discovery: null, headers: 2, applied: true }], effective: "e1", warnings: [] }),
  claudeDesktopRestart: (): Promise<string> => (inTauri ? invoke("claude_desktop_restart") : preview("Restarted Claude Desktop")),
  codexDesktopRestart: (force: boolean): Promise<string> =>
    inTauri ? invoke("codex_desktop_restart", { force }) : preview("Restarted Codex Desktop"),
  supportReport: (): Promise<string> =>
    inTauri ? invoke("support_report") : preview(JSON.stringify({ app: "preview", os: "windows", doctor: [] }, null, 2)),
  orcaRestart: (): Promise<string> => (inTauri ? invoke("orca_restart") : preview("Restarted Codex in Orca (12 models). Open a new Codex tab if the old one does not reconnect.")),

  claudeDesktopSetup: (req: DesktopRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_desktop_setup", { req }) : preview({ change: { id: "z", time: new Date().toISOString(), title: "Setup Claude Desktop", detail: "", files: [], undone: false }, message: "Claude Desktop set up" }),

  codexCurrent: (): Promise<CodexCurrent> =>
    inTauri ? invoke("codex_current") : preview({ model: "gpt-6", provider: "gateway", base_url: "https://api.northwind.example/v1", env_key: "GATEWAY_API_KEY", effort: "high", catalog: true, provider_id: "northwind" }),
  codexSetup: (req: CodexRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_setup", { req }) : preview({ change: { id: "x", time: new Date().toISOString(), title: "Setup Codex", detail: "", files: [], undone: false }, message: "Codex set up" }),

  codexImageStatus: (): Promise<ImageGuidance> =>
    inTauri ? invoke("codex_image_status") : preview({ path: "~/.codex/AGENTS.md", installed: true, outdated: false, models: { "https://api.northwind.example/v1": "image-gen-2" } }),
  codexImageSetup: (providerId: string, model: string): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_image_setup", { providerId, model }) : preview({ change: { id: "i", time: new Date().toISOString(), title: "Codex image guidance", detail: "", files: [], undone: false }, message: "Image guidance added to AGENTS.md" }),
  codexImageRemove: (): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_image_remove") : preview({ change: { id: "j", time: new Date().toISOString(), title: "Remove Codex image guidance", detail: "", files: [], undone: false }, message: "Image guidance removed" }),

  usage: (): Promise<Usage> =>
    inTauri ? invoke("usage_stats") : preview(sampleUsage()),
  doctor: (): Promise<Check[]> =>
    inTauri ? invoke("doctor_run") : preview([{ level: "ok", app: "Codex", title: "Installed", detail: "codex" }]),
  staleEnv: (): Promise<string[]> => (inTauri ? invoke("stale_env") : preview([])),
  clearStaleEnv: (name: string): Promise<void> => (inTauri ? invoke("clear_stale_env", { name }) : preview(undefined)),

  resetPreview: (req: ResetRequest): Promise<ResetItem[]> =>
    inTauri ? invoke("reset_preview", { req }) : preview(req.apps.map((app) => ({ app, path: `~/${app}/config`, action: "edit", changes: ["Remove env ANTHROPIC_BASE_URL", "Remove the selected model"], has_original: true }))),
  resetApply: (req: ResetRequest): Promise<Change> =>
    inTauri ? invoke("reset_apply", { req }) : preview({ id: "r", time: new Date().toISOString(), title: "Restore defaults", detail: req.apps.join(", "), files: [], undone: false }),

  presetSamples: (): Promise<Preset[]> =>
    inTauri ? invoke("preset_samples") : preview([{ schema: "model-switcher/preset@1", name: "Local router", base_url: "http://localhost:4000/v1", auth: null, models: [], defaults: {}, docs: null }]),
  presetParse: (text: string): Promise<Preset> => (inTauri ? invoke("preset_parse", { text }) : preview(JSON.parse(text))),
  presetImport: (text: string, key: string): Promise<Provider> =>
    inTauri ? invoke("preset_import", { text, key }) : api.saveProvider({ id: null, name: JSON.parse(text).name, baseUrl: JSON.parse(text).base_url, key }),
  presetExport: (id: string): Promise<string> =>
    inTauri ? invoke("preset_export", { id }) : preview(JSON.stringify({ schema: "model-switcher/preset@1", name: id, base_url: "https://api.northwind.example/v1", models: [] }, null, 2)),

  capabilityTest: (id: string, models: string[]): Promise<[string, Caps][]> =>
    inTauri
      ? invoke("capability_test", { id, models })
      : preview(models.map((m): [string, Caps] => [m, { endpoint: { ok: true, latency_ms: 420, note: "" }, streaming: { ok: true, latency_ms: 610, note: "" }, tools: { ok: true, latency_ms: 800, note: "" }, caching: { ok: false, latency_ms: null, note: "The second call had no cached tokens" }, images: { ok: true, latency_ms: 900, note: "" } }])),
};

export type ResetRequest = { apps: string[]; mode: "clean" | "original" };
export type ResetItem = { app: string; path: string; action: "edit" | "delete" | "env"; changes: string[]; has_original: boolean };
export type Preset = { schema: string; name: string; base_url: string; auth: string | null; models: string[]; defaults: Record<string, string>; docs: string | null };
export type CapCheck = { ok: boolean; latency_ms: number | null; note: string };
export type Caps = { endpoint: CapCheck | null; streaming: CapCheck | null; tools: CapCheck | null; caching: CapCheck | null; images: CapCheck | null };

/** Error text for the UI, translated when the app is in English. */
export function errText(e: unknown): string {
  return tr(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
}
