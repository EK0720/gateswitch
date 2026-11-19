import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { tr } from "./i18n";
import * as us from "./usageSample";

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
  /** Codex may send reasoning.effort: true/false once tested or chosen, absent = unknown. */
  effort_levels?: boolean | null;
};
export type CodexCurrent = { model: string; provider: string; base_url: string; env_key: string; effort: string; catalog: boolean; provider_id: string };
export type CodexNames = { key: string; env: string; old_keys: string[]; old_envs: string[] };
export type CodexRequest = { provider_id: string; model: string; effort: string | null; catalog: boolean; images: boolean; levels: boolean };

export type ClaudeCurrent = { model: string; base_url: string; auth: string; effort: string; picker: boolean; warnings: string[] };
export type ClaudeRequest = { provider_id: string; model: string; auth: string | null; effort: string | null; picker: boolean };

export type DesktopEntry = { id: string; ours: boolean; name: string; base_url: string; auth: string; has_key: boolean; default_model: string; model_count: number; discovery: boolean | null; headers: number; applied: boolean };
export type DesktopCurrent = { entries: DesktopEntry[]; effective: string | null; warnings: string[] };
export type DesktopRequest = { provider_id: string; model: string; auth: string | null; one_m: boolean; all_models: boolean };

export type ImageGuidance = { path: string; installed: boolean; outdated: boolean; models: Record<string, string> };

export type OrcaStatus = { config: boolean; runtime: string | null; catalog: string | null; provider: string; model: string; in_sync: boolean };

/** Disjoint kinds: `input` excludes both cache kinds. `cache_write` is Claude only. */
export type Totals = { input: number; cached: number; cache_write: number; output: number };

// ---------------------------------------------------------------- Usage v2 contract
// Shared by src-tauri/src/usage*.rs (serde) and src/pages/Usage*.tsx. Field names are snake_case on both sides.
// Dates are LOCAL calendar days "YYYY-MM-DD"; instants are RFC 3339 UTC strings.

export type UsageApp = "claude" | "codex" | "orca";
/** Totals plus the number of model calls (assistant turns / token_count deltas). */
export type Counted = Totals & { turns: number };
export type UsageFilter = {
  /** Inclusive local days. null = no bound. */
  from: string | null;
  to: string | null;
  /** null or empty = all apps. */
  apps: UsageApp[] | null;
  /** Project label exactly as returned in UsageReport.projects. */
  project: string | null;
  model: string | null;
};
export type UsageRow = Counted & {
  /** Stable id: app name, project label, model id, or session key. */
  key: string;
  label: string;
  app: UsageApp | null;
  project: string | null;
  /** Last activity, RFC 3339. */
  last: string;
  /** Sessions only: number of subagent/worker threads under it. */
  subagents: number;
  /** Per-model split of this row (so the UI can price it). */
  models: Record<string, Totals>;
};
export type UsageBucket = Totals & {
  /** Local day "YYYY-MM-DD" or local hour "YYYY-MM-DDTHH". */
  start: string;
  by_app: Partial<Record<UsageApp, Totals>>;
  by_model: Record<string, Totals>;
};
export type UsageReport = {
  filter: UsageFilter;
  total: Counted;
  /** Same-length period right before `filter.from` (zeros when from is null). */
  prev_total: Counted;
  /** "hour" when from == to, else "day". Buckets cover the whole range, empty ones included. */
  bucket: "hour" | "day";
  buckets: UsageBucket[];
  by_app: UsageRow[];
  by_project: UsageRow[];
  by_model: UsageRow[];
  /** Top 100 sessions by total tokens. */
  sessions: UsageRow[];
  /** Filter options over all history. */
  projects: string[];
  models: string[];
  /** Session files read per app [claude, codex, orca]. */
  files: [number, number, number];
  /** Files that could not be read. */
  skipped: number;
};
export type SessionDetail = {
  key: string;
  label: string;
  app: UsageApp;
  project: string | null;
  /** Main thread first, then one row per subagent/worker (label = its description or nickname when the log has one). */
  threads: UsageRow[];
  by_model: UsageRow[];
};
/** Claude-plan style 5-hour window: starts at the first call after the previous window ended. */
export type UsageBlock = Counted & {
  start: string;
  end: string;
  active: boolean;
  /** Active block only: tokens per minute since it started, and projected total at `end`. */
  tokens_per_min: number | null;
  projected: number | null;
};
export type BlocksReport = { blocks: UsageBlock[]; current: UsageBlock | null };
export type Insight = { level: "info" | "warn"; title: string; detail: string };
export type UsageAnalytics = {
  /** Shares 0..1 of all tokens (input + cache read + cache write + output). */
  share: { input: number; cached: number; cache_write: number; output: number };
  /** Local hour 0..23 → total tokens. */
  by_hour: number[];
  main: Counted;
  subagents: Counted;
  /** Turns whose previous turn in the same thread was > 5 min earlier and that wrote cache again. */
  cache_expiries: number;
  /** Sessions with the largest single-turn context (input + cache read + cache write). */
  biggest_context: (UsageRow & { max_context: number })[];
  insights: Insight[];
};
export type UsagePrice = { input: number; cached: number; write?: number; output: number };
export type UsageSettings = {
  /** Auto refresh while the Usage page is open. */
  watch: boolean;
  /** Tray icon with today's tokens. Off by default. */
  tray: boolean;
  /** Small always-on-top window. Off by default. */
  overlay: boolean;
  /** Desktop notification when the current 5h block passes 80% / 100% of block_limit. Off by default. */
  notify: boolean;
  /** Tokens per 5h block; null = no limit set. */
  block_limit: number | null;
  /** Start a quota watch by itself after "Check models" finds the group Codex / Claude Code uses out of quota. Off by default. */
  quota_auto: boolean;
  /** Closing the main window hides it to the tray (only while `tray` is on). Off by default. */
  close_to_tray: boolean;
};
/** src-tauri/src/quota_watch.rs `Watch`: one per (provider, quota group). Instants are RFC 3339. */
export type QuotaWatch = {
  provider_id: string;
  group: string;
  /** Model the checks use. */
  model: string;
  since: string;
  next_at: string;
  attempts: number;
  /** Last gateway message, key masked. */
  message: string;
  origin: "auto" | "manual" | string;
  last_check: string;
};
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
      "gpt-6": ok(890), "gpt-6-mini": { status: "quota", latency_ms: null, message: "HTTP 429: The usage limit has been reached (reset after 2h 13m)", checked_at: sampleNow },
      "gemini-3-pro": ok(720), "deepseek-v4": { status: "quota", latency_ms: null, message: "HTTP 402: Insufficient balance. Top up to continue.", checked_at: sampleNow },
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

const inMin = (n: number) => new Date(Date.now() + n * 60_000).toISOString();
let sampleWatches: QuotaWatch[] = [
  { provider_id: "northwind", group: "gpt", model: "gpt-6-mini", since: sampleNow, next_at: inMin(133), attempts: 0, message: "HTTP 429: The usage limit has been reached (reset after 2h 13m)", origin: "manual", last_check: sampleNow },
  { provider_id: "acme", group: "claude", model: "claude-sonnet-5-5", since: sampleNow, next_at: inMin(20), attempts: 2, message: "HTTP 429: rate limit exceeded", origin: "auto", last_check: sampleNow },
];
const emitWatches = () => window.dispatchEvent(new CustomEvent("preview-quota-watch"));

/** Quota group as the backend computes it (quota_watch.rs `group_in`): owned_by, else the id prefix; "[1m]" ignored. */
export function watchGroup(p: Provider, model: string): string {
  const base = model.replace(/(.)\[[^\]]*\]$/, "$1");
  const owned = p.info?.[model]?.owned_by ?? p.info?.[base]?.owned_by;
  if (owned) return owned;
  const i = base.indexOf("/");
  return i > 0 ? base.slice(0, i) : base;
}

/** Out-of-credit error: waiting won't fix it. Same words as quota_watch.rs `is_billing`. */
export function isBilling(message: string): boolean {
  const m = message.toLowerCase();
  return ["balance", "credit", "insufficient", "payment", "billing", "http 402"].some((w) => m.includes(w));
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

  usageQuery: (filter: UsageFilter): Promise<UsageReport> =>
    inTauri ? invoke("usage_query", { filter }) : preview(us.sampleReport(filter)),
  usageSession: (key: string): Promise<SessionDetail> =>
    inTauri ? invoke("usage_session", { key }) : preview(us.sampleSession(key)),
  usageBlocks: (filter: UsageFilter): Promise<BlocksReport> =>
    inTauri ? invoke("usage_blocks", { filter }) : preview(us.sampleBlocks(filter)),
  usageAnalytics: (filter: UsageFilter): Promise<UsageAnalytics> =>
    inTauri ? invoke("usage_analytics", { filter }) : preview(us.sampleAnalytics(filter)),
  /** Writes the export and returns the saved file path. */
  usageExport: (filter: UsageFilter, format: "csv" | "json"): Promise<string> =>
    inTauri ? invoke("usage_export", { filter, format }) : preview(`~/Downloads/gateswitch-usage.${format}`),
  usagePricesGet: (): Promise<Record<string, UsagePrice>> =>
    inTauri ? invoke("usage_prices_get") : preview({ ...us.samplePrices }),
  usagePricesSet: (prices: Record<string, UsagePrice>): Promise<void> => {
    if (inTauri) return invoke("usage_prices_set", { prices });
    us.setSamplePrices(prices);
    return preview(undefined);
  },
  usageSettingsGet: (): Promise<UsageSettings> =>
    inTauri ? invoke("usage_settings_get") : preview({ ...us.sampleSettings }),
  usageSettingsSet: (settings: UsageSettings): Promise<void> => {
    if (inTauri) return invoke("usage_settings_set", { settings });
    us.setSampleSettings(settings);
    return preview(undefined);
  },
  usageWatch: (on: boolean): Promise<void> => (inTauri ? invoke("usage_watch", { on }) : Promise.resolve()),
  /** Mini window: bring the main window to the front. */
  usageOpenMain: (): Promise<void> => (inTauri ? invoke("usage_open_main") : Promise.resolve()),
  quotaWatchList: (): Promise<QuotaWatch[]> => (inTauri ? invoke("quota_watch_list") : preview([...sampleWatches])),
  quotaWatchAdd: (providerId: string, model: string): Promise<QuotaWatch[]> => {
    if (inTauri) return invoke("quota_watch_add", { providerId, model });
    const p = sampleProviders.find((x) => x.id === providerId)!;
    const probe = p.probes[model];
    if (probe && isBilling(probe.message)) return Promise.reject("Out of credit: top up, waiting won't help");
    const w: QuotaWatch = { provider_id: providerId, group: watchGroup(p, model), model, since: new Date().toISOString(), next_at: inMin(probe ? 133 : 0.1), attempts: 0, message: probe?.message ?? "", origin: "manual", last_check: new Date().toISOString() };
    sampleWatches = [...sampleWatches.filter((x) => !(x.provider_id === providerId && x.group === w.group)), w];
    emitWatches();
    return preview([...sampleWatches]);
  },
  quotaWatchRemove: (providerId: string, group: string): Promise<QuotaWatch[]> => {
    if (inTauri) return invoke("quota_watch_remove", { providerId, group });
    sampleWatches = sampleWatches.filter((x) => !(x.provider_id === providerId && x.group === group));
    emitWatches();
    return preview([...sampleWatches]);
  },
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
      : preview(models.map((m): [string, Caps] => [m, { endpoint: { ok: true, latency_ms: 420, note: "" }, streaming: { ok: true, latency_ms: 610, note: "" }, tools: { ok: true, latency_ms: 800, note: "" }, caching: { ok: false, latency_ms: null, note: "The second call had no cached tokens" }, images: { ok: true, latency_ms: 900, note: "" }, reasoning: { ok: true, latency_ms: 700, note: "" } }])),
};

export type ResetRequest = { apps: string[]; mode: "clean" | "original" };
export type ResetItem = { app: string; path: string; action: "edit" | "delete" | "env"; changes: string[]; has_original: boolean };
export type Preset = { schema: string; name: string; base_url: string; auth: string | null; models: string[]; defaults: Record<string, string>; docs: string | null };
export type CapCheck = { ok: boolean; latency_ms: number | null; note: string };
export type Caps = { endpoint: CapCheck | null; streaming: CapCheck | null; tools: CapCheck | null; caching: CapCheck | null; images: CapCheck | null; reasoning?: CapCheck | null };

/** Subscribe to "usage-changed" (backend saw new log lines). Returns an unsubscribe that is safe to call before `listen` resolves. */
export function onUsageChanged(cb: () => void): () => void {
  if (!inTauri) return () => {};
  let off: (() => void) | null = null;
  let dead = false;
  listen("usage-changed", () => cb()).then((un) => { if (dead) un(); else off = un; }).catch((e) => console.warn("usage-changed listen failed", e));
  return () => { dead = true; off?.(); };
}

/** Subscribe to "usage-settings-changed" (saved from another window, the tray or the mini window's ×). */
export function onUsageSettingsChanged(cb: () => void): () => void {
  if (!inTauri) return () => {};
  let off: (() => void) | null = null;
  let dead = false;
  listen("usage-settings-changed", () => cb()).then((un) => { if (dead) un(); else off = un; }).catch((e) => console.warn("usage-settings-changed listen failed", e));
  return () => { dead = true; off?.(); };
}

/** Subscribe to "quota-watch-changed" (a watch was added, checked, ended or cancelled). */
export function onQuotaWatchChanged(cb: () => void): () => void {
  if (!inTauri) {
    window.addEventListener("preview-quota-watch", cb);
    return () => window.removeEventListener("preview-quota-watch", cb);
  }
  let off: (() => void) | null = null;
  let dead = false;
  listen("quota-watch-changed", () => cb()).then((un) => { if (dead) un(); else off = un; }).catch((e) => console.warn("quota-watch-changed listen failed", e));
  return () => { dead = true; off?.(); };
}

/** Error text for the UI, translated when the app is in English. */
export function errText(e: unknown): string {
  return tr(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
}
