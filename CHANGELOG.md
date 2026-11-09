# Changelog

## 0.2.3

### Added
- Doctor flags a leftover `ANTHROPIC_MODEL` environment variable. Claude Code ranks it above the model in `settings.json`, so every new session opened on that model instead of the one set in Gateswitch. **Fix** removes it (can be undone).

## 0.2.2

### Changed
- Linux `.deb`/`.rpm` docs: these packages also update in-app (asking for an admin password).
- First release delivered through the in-app updater.

## 0.2.1

### Changed
- Renamed to **Gateswitch** (was Model Switcher). Settings, keys, backups and history carry over: the app keeps the same identifier, keychain entries and data folder. The repository moved to `EK0720/gateswitch`; old links redirect.

### Added
- In-app updates: a notice when a new version is out, then **Update and restart** in one click. Update packages are signed and checked against the public key built into the app before they install. The check fetches only `latest.json` from this repo's GitHub releases; turn it off in Settings → Updates. Works with the Windows installers, the macOS app and Linux (AppImage in place; `.deb`/`.rpm` ask for an admin password).

### Fixed
- Backup encryption key on macOS/Linux: creating the key is now atomic, so two backups at once can no longer end up with different keys.

## 0.2.0

### Changed
- English is the source language; Vietnamese translation added (`src/i18n/vi.ts`). Switch language in the sidebar or Settings.
- Codex: provider name and environment variable default to `gateway` / `GATEWAY_API_KEY` instead of `ms-<id>` / `MS_<ID>_API_KEY`; editable under Advanced. Setting up again replaces the old table and removes the old variable when it holds the same key (undoable).
- Per-provider custom headers, written to `ANTHROPIC_CUSTOM_HEADERS` (Claude Code) and `inferenceCustomHeaders` (Claude Desktop). The Claude pages have a guide for HTTP 400 `reasoning_effort` with a button that adds compatible headers. Presets carry headers (never keys).
- Gateway capability check: tick models right in the model table; results show next to the model name.
- Orca: no separate setup form. Orca mirrors `~/.codex/config.toml` itself; the Orca page shows status, Sync now and daemon restart.
- When run inside an Orca terminal, the tool ignores the `CODEX_HOME` Orca sets and uses `~/.codex`.
- Claude Desktop: select and delete any entry in configLibrary, including ones created in the app or by older tools.
- Windows: integrated title bar instead of the default window frame. Secondary actions moved into a ⋯ menu, long explanations into ⓘ.
- Codex: image generation guidance through the gateway (`/images/generations`, transparent PNG by default) written to `AGENTS.md`. It names no gateway and holds no key; it follows the provider Codex is set up with and the image model chosen per gateway. Undoable.
- The headers that fix HTTP 400 `reasoning_effort` are now neutral: `User-Agent: model-switcher/1` + `x-app: model-switcher`.
- Restarting Claude Desktop closes only Claude Desktop (never the Claude Code CLI), waits for it to exit, reopens it and checks that it is running.
- Restart Claude Desktop and Codex Desktop on macOS (`osascript quit` → SIGTERM → SIGKILL, reopen with `open -a`) and Claude Desktop on Linux. Not yet run on real machines.
- Each restart step is logged to `restart.log`; support reports include the last 60 lines.
- Usage page caches results per log file (by path hash, size and modified time); reopening reads only new files.
- Model table: separate latency column, out-of-quota count per group, Try button shown on hover.
- Orca on macOS/Linux: finds Orca's own Codex home the way Orca picks its data folder (`ORCA_USER_DATA_PATH`, `~/Library/Application Support/orca`, `$XDG_CONFIG_HOME/orca` or `~/.config/orca`). Restarting the Orca daemon is still Windows-only.
- Desktop app enforces a strict Content Security Policy.
- macOS and Linux builds have not been tested on real machines. Builds are not code-signed and do not auto-update.

## 0.1.0 (unreleased)

Tauri 2 rewrite of Model Switcher.

### Added
- Providers: keys stored in the OS keychain, model list fetch, latency check.
- Gateway preset files (`model-switcher/preset@1`): import from a file or a sample preset, export presets (never containing keys).
- Per-model gateway capability check: endpoint, streaming, tools, prompt caching, images.
- Setup for Codex (CLI, Desktop), Claude Code, Claude Desktop, Orca. Restart Codex Desktop and the Orca daemon.
- Every file write is atomic, with an encrypted backup (DPAPI on Windows, AES-GCM with the key in the keychain on macOS/Linux), and undoable.
- Restore defaults: one step removes everything the tool added, or puts files back to the original before the tool first wrote them. This is itself undoable.
- Doctor with one-click fixes; support report with keys masked.
- Usage page reads local logs: tokens, cache hit rate, cost estimate at user-entered prices.
- Ctrl+K command palette, light/dark theme.
