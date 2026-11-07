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
    { id: "orca", name: "Orca", installed: false, path: null },
  ],
  ides: [
    { id: "vscode", name: "VS Code", uses: ["codex", "claude"], codex: "Cài extension Codex (OpenAI) là dùng được.", claude: "Cài extension Claude Code (Anthropic) là dùng được." },
    { id: "antigravity", name: "Antigravity", uses: ["codex", "claude"], codex: "Cài extension Codex (OpenAI) là dùng được.", claude: "Cài extension Claude Code (Anthropic) là dùng được." },
    { id: "jetbrains", name: "JetBrains (Rider, IntelliJ…)", uses: ["codex", "claude"], codex: "Chọn Codex trong AI Chat.", claude: "Chọn Claude Agent trong AI Chat." },
  ],
};
let sampleProviders: Provider[] = [
  {
    id: "9router", name: "9router", base_url: "http://localhost:20128/v1",
    models: ["cc/claude-opus-5.5[1m]", "cc/claude-sonnet-5", "combo-smart", "cx/gpt-6-astra", "cx/gpt-6.1-sol", "gcli/gemini-3-pro"],
    latency_ms: 142, checked_at: new Date().toISOString(), has_key: true,
    info: {
      "cc/claude-opus-5.5[1m]": { owned_by: "cc", capabilities: [] }, "cc/claude-sonnet-5": { owned_by: "cc", capabilities: [] },
      "combo-smart": { owned_by: "combo", capabilities: [] }, "cx/gpt-6-astra": { owned_by: "cx", capabilities: [] },
      "cx/gpt-6.1-sol": { owned_by: "cx", capabilities: [] }, "gcli/gemini-3-pro": { owned_by: "gcli", capabilities: [] },
    },
    probes: {},
  },
];

function preview<T>(value: T): Promise<T> {
  return new Promise((r) => setTimeout(() => r(value), 300));
}

function probeModelsPreview(id: string, models: string[] | null): Promise<Provider> {
  const p = sampleProviders.find((x) => x.id === id)!;
  const now = new Date().toISOString();
  const fake = (m: string): Probe =>
    m.startsWith("cx/") ? { status: "quota", latency_ms: null, message: "HTTP 429: usage limit reached", checked_at: now }
    : m.startsWith("gcli/") ? { status: "timeout", latency_ms: null, message: "Không phản hồi sau 30 giây", checked_at: now }
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
    inTauri ? invoke("claude_current") : preview({ model: "", base_url: "", auth: "", effort: "", picker: false, warnings: [] }),
  claudeSetup: (req: ClaudeRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_setup", { req }) : preview({ change: { id: "y", time: new Date().toISOString(), title: "Setup Claude Code", detail: "", files: [], undone: false }, message: "Đã setup Claude Code" }),

  orcaStatus: (): Promise<OrcaStatus> =>
    inTauri ? invoke("orca_status") : preview({ config: true, runtime: "C:/…/codex.exe", catalog: null, provider: "gateway", model: "cx/gpt-6.1-sol", in_sync: false }),
  orcaSync: (): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("orca_sync") : preview({ change: { id: "o", time: new Date().toISOString(), title: "Đồng bộ Codex sang Orca", detail: "", files: [], undone: false }, message: "Đã đồng bộ. Khởi động lại daemon để Orca nhận." }),
  claudeDesktopRemove: (ids: string[]): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_desktop_remove", { ids }) : preview({ change: { id: "d", time: new Date().toISOString(), title: "Xóa cấu hình Claude Desktop", detail: "", files: [], undone: false }, message: `Đã xóa ${ids.length} cấu hình. Thoát hẳn rồi mở lại Claude Desktop.` }),
  codexNames: (id: string): Promise<CodexNames> =>
    inTauri ? invoke("codex_names", { id }) : preview({ key: "gateway", env: "GATEWAY_API_KEY", old_keys: [], old_envs: [] }),
  setCodexNames: (id: string, name: string, env: string): Promise<CodexNames> =>
    inTauri ? invoke("set_codex_names", { id, name, env }) : preview({ key: name || "gateway", env: env || "GATEWAY_API_KEY", old_keys: [], old_envs: [] }),

  claudeDesktopCurrent: (): Promise<DesktopCurrent> =>
    inTauri ? invoke("claude_desktop_current") : preview({ entries: [{ id: "e0", ours: false, name: "Default", base_url: "https://api.example.com", auth: "bearer", has_key: true, default_model: "", model_count: 0, discovery: true, headers: 0, applied: false }, { id: "e1", ours: true, name: "", base_url: "https://api.nghimmo.com", auth: "bearer", has_key: true, default_model: "cc/claude-opus-5.5[1m]", model_count: 6, discovery: null, headers: 2, applied: true }], effective: "e1", warnings: [] }),
  claudeDesktopRestart: (): Promise<string> => (inTauri ? invoke("claude_desktop_restart") : preview("Đã khởi động lại Claude Desktop")),
  codexDesktopRestart: (force: boolean): Promise<string> =>
    inTauri ? invoke("codex_desktop_restart", { force }) : preview("Đã khởi động lại Codex Desktop"),
  supportReport: (): Promise<string> =>
    inTauri ? invoke("support_report") : preview(JSON.stringify({ app: "preview", os: "windows", doctor: [] }, null, 2)),
  orcaRestart: (): Promise<string> => (inTauri ? invoke("orca_restart") : preview("Đã khởi động lại Codex trong Orca (12 model)")),

  claudeDesktopSetup: (req: DesktopRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("claude_desktop_setup", { req }) : preview({ change: { id: "z", time: new Date().toISOString(), title: "Setup Claude Desktop", detail: "", files: [], undone: false }, message: "Đã setup Claude Desktop" }),

  codexCurrent: (): Promise<CodexCurrent> =>
    inTauri ? invoke("codex_current") : preview({ model: "", provider: "", base_url: "", env_key: "", effort: "", catalog: false, provider_id: "" }),
  codexSetup: (req: CodexRequest): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_setup", { req }) : preview({ change: { id: "x", time: new Date().toISOString(), title: "Setup Codex", detail: "", files: [], undone: false }, message: "Đã setup Codex" }),

  codexImageStatus: (): Promise<ImageGuidance> =>
    inTauri ? invoke("codex_image_status") : preview({ path: "~/.codex/AGENTS.md", installed: false, outdated: false, models: {} }),
  codexImageSetup: (providerId: string, model: string): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_image_setup", { providerId, model }) : preview({ change: { id: "i", time: new Date().toISOString(), title: "Codex image guidance", detail: "", files: [], undone: false }, message: "Image guidance added to AGENTS.md" }),
  codexImageRemove: (): Promise<{ change: Change; message: string }> =>
    inTauri ? invoke("codex_image_remove") : preview({ change: { id: "j", time: new Date().toISOString(), title: "Remove Codex image guidance", detail: "", files: [], undone: false }, message: "Image guidance removed" }),

  usage: (): Promise<Usage> =>
    inTauri ? invoke("usage_stats") : preview({ claude: { input: 120000, cached: 90000, output: 4000 }, codex: { input: 3000000, cached: 2800000, output: 60000 }, orca: { input: 0, cached: 0, output: 0 }, days: [], by_model: {}, files: [2, 1, 0] }),
  doctor: (): Promise<Check[]> =>
    inTauri ? invoke("doctor_run") : preview([{ level: "ok", app: "Codex", title: "Đã cài", detail: "codex" }]),
  staleEnv: (): Promise<string[]> => (inTauri ? invoke("stale_env") : preview([])),
  clearStaleEnv: (name: string): Promise<void> => (inTauri ? invoke("clear_stale_env", { name }) : preview(undefined)),

  resetPreview: (req: ResetRequest): Promise<ResetItem[]> =>
    inTauri ? invoke("reset_preview", { req }) : preview(req.apps.map((app) => ({ app, path: `~/${app}/config`, action: "edit", changes: ["Bỏ env ANTHROPIC_BASE_URL", "Bỏ model đã chọn"], has_original: true }))),
  resetApply: (req: ResetRequest): Promise<Change> =>
    inTauri ? invoke("reset_apply", { req }) : preview({ id: "r", time: new Date().toISOString(), title: "Khôi phục mặc định", detail: req.apps.join(", "), files: [], undone: false }),

  presetSamples: (): Promise<Preset[]> =>
    inTauri ? invoke("preset_samples") : preview([{ schema: "model-switcher/preset@1", name: "9router (máy này)", base_url: "http://localhost:20128/v1", auth: null, models: [], defaults: {}, docs: null }]),
  presetParse: (text: string): Promise<Preset> => (inTauri ? invoke("preset_parse", { text }) : preview(JSON.parse(text))),
  presetImport: (text: string, key: string): Promise<Provider> =>
    inTauri ? invoke("preset_import", { text, key }) : api.saveProvider({ id: null, name: JSON.parse(text).name, baseUrl: JSON.parse(text).base_url, key }),
  presetExport: (id: string): Promise<string> =>
    inTauri ? invoke("preset_export", { id }) : preview(JSON.stringify({ schema: "model-switcher/preset@1", name: id, base_url: "http://localhost:20128/v1", models: [] }, null, 2)),

  capabilityTest: (id: string, models: string[]): Promise<[string, Caps][]> =>
    inTauri
      ? invoke("capability_test", { id, models })
      : preview(models.map((m): [string, Caps] => [m, { endpoint: { ok: true, latency_ms: 420, note: "" }, streaming: { ok: true, latency_ms: 610, note: "" }, tools: { ok: true, latency_ms: 800, note: "" }, caching: { ok: false, latency_ms: null, note: "Lần gọi thứ hai không có cached token" }, images: { ok: true, latency_ms: 900, note: "" } }])),
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
