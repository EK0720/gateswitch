# Changelog

## Unreleased

### Fixed
- Codex 0.157+ refused the gateway model catalog with "missing field `supported_reasoning_levels`", so Codex and Orca would not start. The catalog now always includes the field. Press **Set up** on the Codex page once to write a fixed catalog.
- Usage: Claude Code numbers were wrong. Claude's `input_tokens` excludes cached tokens while Codex's includes them, and cache writes were ignored, so cache hit could show tens of thousands of percent, "tokens in" was far too low and estimated cost could go negative. Every app now counts four separate kinds (input, cache read, cache write, output); cache hit is cache read ÷ all input.
- Usage: days follow the computer's time zone instead of UTC, so late-night work no longer lands on the previous day.
- Usage: no longer stops after 4000 session files per app.
- Usage: totals in the "By app" and "By model" rows sit on the right again.

### Added
- Codex: **Effort levels in /model** (Codex page, Advanced). When on, the catalog lists low/medium/high/xhigh, Codex shows its effort picker and sends `reasoning.effort`. Without it, 9router runs `cx/` models at its default "low" effort. Off by default, because gateways that forward to a plain chat API reject the field. The capability test now also sends one request with `reasoning.effort` and remembers the answer per provider: accepted turns the switch on, a 400 turns it off.
- Usage: a cache-write price per model (empty means 1.25 × input, as Anthropic bills it).
- Settings: a **Notifications & background** section. The tray icon, the always-on-top mini window, the 80% / 100% block alerts and the block limit moved here from the Usage page's settings menu, which keeps Auto refresh and links to it. Everything in the section is off by default.
- **Notify when it's back**: the out-of-quota card on the Codex and Claude Code pages can watch the model and send a notification once it works again. A watch covers the whole quota group (every `cx/` model shares one account). When the gateway says when the limit resets, Gateswitch checks once, just after that time, counted from when the error was seen. Otherwise it backs off from 5 to 30 minutes and gives up after 24 hours. A refused check costs nothing, and the one that works asks for at most 16 tokens. Watches survive a restart and are listed in Settings, where you can cancel them. Out-of-credit errors (balance, billing, HTTP 402) are never watched, because waiting doesn't top up an account.
- Optional **Watch automatically** (off by default): after **Test models**, a watch starts by itself when the quota group of the model Codex or Claude Code is set to is out of quota.
- Optional **Closing the window hides it to the tray** (needs the tray icon). Gateswitch keeps running, so alerts and quota watches still arrive. Open it again or quit from the tray menu.

## 0.2.4

### Fixed
- Doctor's **Fix** for `ANTHROPIC_MODEL` failed with "not in the list allowed to be removed". It now removes the variable (can be undone on the Restore page). A test makes sure every variable Doctor offers to fix can be removed.

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
