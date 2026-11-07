# Contributing

Thanks for helping. Small, focused pull requests are easiest to review.

## Most wanted

- **macOS and Linux test reports.** Install the build from Releases, set up one app, restart it, undo. Open a [test report issue](../../issues/new/choose) with what worked and what did not.
- **Translations.** English is the source language. Copy `src/i18n/vi.ts` to `src/i18n/<code>.ts`, translate the values, and register the language in `src/i18n.ts`.
- **Gateway presets** for gateways you use (`model-switcher/preset@1`, never containing keys).

## Development

Requires Node 20+, stable Rust and [Tauri's system dependencies](https://tauri.app/start/prerequisites/).

```sh
npm ci
npm run tauri dev          # app with hot reload
npm run dev                # UI only, in the browser, with sample data
npx tsc --noEmit           # type check
cd src-tauri && cargo test # Rust tests
```

## Ground rules

- **Never log or display a key.** Mask secrets in every message, report and test fixture. Use obviously fake keys in tests.
- **Every file write goes through the change store** (`apply` / `apply_full`) so it is atomic, backed up and undoable.
- **UI text is English** and wrapped in `t("…")`; add the Vietnamese entry to `src/i18n/vi.ts` if you can (or leave it, a maintainer will).
- Do not touch files outside the app's documented config locations.
- Run the type check and Rust tests before opening a pull request; CI runs them too, plus a secret scan.
