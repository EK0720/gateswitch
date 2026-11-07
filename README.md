# Model Switcher

**Point Codex, Claude Code, Claude Desktop and Orca at any OpenAI/Anthropic-compatible gateway in one click, and undo it just as easily.** Windows · macOS · Linux.

**English** · [Tiếng Việt](README.vi.md)

> Independent project. Not affiliated with OpenAI or Anthropic.

## Features

- **One place for every gateway**: add a provider (base URL + API key), fetch its model list, measure latency.
- **One-click setup per app**: Codex CLI/Desktop, Claude Code, Claude Desktop (third-party inference), Orca.
- **Gateway presets**: import a `.json` preset published by your gateway, or a sample preset (9router, LiteLLM, OpenRouter), then paste your key.
- **Capability check**: real requests tell you whether the gateway supports streaming, tools, prompt caching and images, per model.
- **Doctor**: finds broken config, stale environment variables that cause HTTP 400, proxies and managed policies, with one-click fixes.
- **Usage**: token counts and cache hit rate from local logs, with cost estimates at prices you enter.
- **Undo and Restore defaults**: every change can be undone; one step puts every app back the way it was before the tool.

## Download

> **Note:** the Windows build has been tested on real machines. The **macOS and Linux builds have not been tested** on real machines yet (they are only built by CI). If something breaks, please open an issue and attach a support report (Settings → Support report).

The builds are **not code-signed**, so your OS will warn you the first time:

- **Windows** (SmartScreen "Windows protected your PC"): click **More info → Run anyway**.
- **macOS** ("cannot be opened because the developer cannot be verified"): right-click the app → **Open**, or run `xattr -dr com.apple.quarantine "/Applications/Model Switcher.app"`.
- **Linux** (AppImage): `chmod +x model-switcher*.AppImage`, then run it.

There is no auto-update; check [Releases](../../releases) for new versions.

Get the installer from [Releases](../../releases) and compare its SHA-256 with the one listed on the release page before running it:

```powershell
Get-FileHash .\Model.Switcher_x64-setup.exe -Algorithm SHA256
```

```sh
shasum -a 256 Model.Switcher*.dmg   # macOS
sha256sum model-switcher*.AppImage  # Linux
```

## Quick start

1. **Providers → Add** (or **Import preset**): enter the base URL (usually ending in `/v1`) and the API key.
2. Click **Fetch models**.
3. Open the page for the app you use (Codex, Claude Code…), pick the provider and model, click **Set up**. Restart the app.

## Supported apps

| App | File changed | Notes |
|---|---|---|
| Codex CLI / Desktop | `~/.codex/config.toml` | Provider name and key variable default to `gateway` / `GATEWAY_API_KEY` (the gateway's name is not exposed), editable under Advanced; adds a catalog for the `/model` menu |
| Claude Code | `~/.claude/settings.json` | Adds a "From gateway" entry to the `/model` menu |
| Claude Desktop | Third-party inference `configLibrary` | Not written if the machine has a managed policy; can also delete old entries the tool did not create (undoable) |
| Orca | No separate setup | Orca copies `~/.codex/config.toml` into its own Codex home; the tool shows whether it is in sync, with Sync now and daemon restart |

## Security and undo

- Provider keys live in the OS keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux), not in the tool's files.
- Some apps can only read a key from their own config file (Claude Code, Claude Desktop). The tool writes the key only to those places.
- Every write is atomic and backed up. Backups are encrypted: DPAPI on Windows, AES-256-GCM with the key held in the keychain on macOS/Linux.
- Keys never appear in logs, error messages, support reports or preset files.
- **Restore defaults** (Restore page) has two modes:
  - *Remove what the tool added*: removes only the tool's additions and keeps your other edits.
  - *Back to original*: puts each file back exactly as it was before the tool first wrote it.

## Troubleshooting

- Run **Doctor** (Settings or Ctrl+K). Every issue comes with a fix.
- Gateway returns HTTP 400 `reasoning_effort`: remove the `CLAUDE_CODE_EFFORT_LEVEL` variable (Doctor has a Fix button) and keep the default effort level.
- Reporting a bug: Settings → **Support report**. Keys and user folders are masked; review it before sending.
- Check the gateway with curl:

```sh
curl -s "$BASE_URL/models" -H "Authorization: Bearer $API_KEY"
```

## Build from source

Requires Node 20+, stable Rust and [Tauri's system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm ci
npm run tauri dev      # run
npm run tauri build    # package
cd src-tauri && cargo test
```

## Contributing

Issues and pull requests are welcome, especially test reports from macOS and Linux. English is the source language for the UI (`src/i18n`); translations go in their own file, such as `vi.ts`.

## License

[MIT](LICENSE)
