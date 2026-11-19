mod claude;
mod codex;
mod desktop;
mod detect;
mod doctor;
mod headers;
mod image;
mod orca;
mod preset;
mod report;
mod reset;
mod restart;
mod restart_unix;
mod rlog;
mod seal;
mod probe;
mod providers;
mod quota_watch;
mod secrets;
mod store;
mod usage;
mod usage_live;
mod usage_prefs;
mod usage_report;

use secrets::{Env, Keychain, UserEnv, Vault};
use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default().join("ModelSwitcher").join("next")
}

pub(crate) fn store() -> store::Store {
    store::Store::new(data_dir())
}

pub(crate) fn providers() -> providers::Providers {
    providers::Providers::new(data_dir())
}

#[tauri::command]
fn scan() -> detect::Scan {
    detect::scan()
}

#[tauri::command]
fn history() -> Vec<store::Change> {
    store().history()
}

#[tauri::command]
fn undo(id: String) -> Result<store::Change, String> {
    store().undo_with(&id, Some((&UserEnv, &Keychain)))
}

// ---------------------------------------------------------------- providers

#[tauri::command]
fn list_providers() -> Vec<providers::Provider> {
    providers().list(&Keychain)
}

#[tauri::command]
fn save_provider(id: Option<String>, name: String, base_url: String, key: Option<String>) -> Result<providers::Provider, String> {
    providers().upsert(id.as_deref(), &name, &base_url, key.as_deref(), &Keychain)
}

/// Custom headers as `Name: value` lines (empty text = none).
#[tauri::command]
fn set_provider_headers(id: String, text: String) -> Result<providers::Provider, String> {
    providers().set_headers(&id, headers::parse(&text)?, &Keychain)
}

#[tauri::command]
fn remove_provider(id: String) -> Result<(), String> {
    providers().remove(&id, &Keychain)
}

/// Network call: run off the main thread so the window stays responsive.
#[tauri::command]
async fn refresh_models(id: String) -> Result<providers::Provider, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ps = providers();
        let p = ps.get(&id, &Keychain)?;
        let key = Keychain.get(&providers::account(&id)).ok_or("No key for this provider")?;
        let (models, info, ms) = providers::fetch_models(&p.base_url, &key)?;
        ps.set_models(&id, models, info, ms, &Keychain)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Kiểm tra model": one tiny request per model, only when the user clicks.
/// `models` = None probes every model the provider lists.
#[tauri::command]
async fn probe_models(app: tauri::AppHandle, id: String, models: Option<Vec<String>>) -> Result<providers::Provider, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ps = providers();
        let p = ps.get(&id, &Keychain)?;
        let key = Keychain.get(&providers::account(&id)).ok_or("No key for this provider")?;
        let list = models.unwrap_or_else(|| p.models.clone());
        if list.is_empty() {
            return Err("No models to test. Click Load models first.".into());
        }
        let results = probe::probe_all(&p.base_url, &key, list);
        let saved = ps.set_probes(&id, results.clone(), &Keychain)?;
        let auto = usage_prefs::settings_get_in(&data_dir()).quota_auto;
        let tools = if auto { tool_models() } else { vec![] };
        quota_watch::after_probe(&app, &saved, &results, auto, &tools);
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// (provider id, model) that Codex and Claude Code are set to, when they point
/// at a saved provider. Claude Code is matched by its base URL.
fn tool_models() -> Vec<(String, String)> {
    let mut out = vec![];
    let cx = codex_current();
    if !cx.provider_id.is_empty() {
        out.push((cx.provider_id, cx.model));
    }
    let cc = claude_current();
    if !cc.base_url.is_empty() {
        let root = claude::base_root(&cc.base_url);
        if let Some(p) = providers().list(&Keychain).into_iter().find(|p| claude::base_root(&p.base_url) == root) {
            out.push((p.id, cc.model));
        }
    }
    out
}

// ---------------------------------------------------------------- quota watch

#[tauri::command]
fn quota_watch_list(qw: tauri::State<'_, quota_watch::QuotaWatch>) -> Vec<quota_watch::Watch> {
    qw.list()
}

/// "Notify when it's back" on an out-of-quota card. Clicking is consent, so
/// this works whether or not automatic watching is on.
#[tauri::command]
fn quota_watch_add(app: tauri::AppHandle, provider_id: String, model: String) -> Result<Vec<quota_watch::Watch>, String> {
    let p = providers().get(&provider_id, &Keychain)?;
    let group = quota_watch::group_in(&p.info, &model);
    let check = quota_watch::check_model(&p.models, &model);
    let w = quota_watch::arm(&p.id, &group, &check, p.probes.get(&model), "manual", chrono::Utc::now())?;
    Ok(app.state::<quota_watch::QuotaWatch>().edit(&app, |l| {
        quota_watch::upsert(l, w);
        true
    }))
}

#[tauri::command]
fn quota_watch_remove(app: tauri::AppHandle, provider_id: String, group: String) -> Vec<quota_watch::Watch> {
    app.state::<quota_watch::QuotaWatch>().edit(&app, |l| {
        let n = l.len();
        l.retain(|w| !(w.provider_id == provider_id && w.group == group));
        l.len() != n
    })
}

/// Quit Claude Desktop and start it again. Only processes whose executable is
/// Claude Desktop's are touched (the Claude Code CLI is also claude.exe).
/// The user confirms in the UI first.
#[tauri::command]
async fn claude_desktop_restart() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| rlog::done("Claude Desktop", restart::claude_desktop())).await.map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------- claude desktop

#[tauri::command]
fn claude_desktop_current() -> desktop::Current {
    let dir = desktop::library_dir();
    let mut cur = desktop::read_current(dir.as_deref());
    if let Some(p) = desktop::policy_override() {
        cur.warnings.insert(0, format!("Claude Desktop is managed by a policy ({p}); local configuration is ignored."));
    }
    cur
}

#[tauri::command]
fn claude_desktop_setup(req: desktop::Request) -> Result<SetupResult, String> {
    let p = providers().get(&req.provider_id, &Keychain)?;
    let key = Keychain
        .get(&providers::account(&p.id))
        .ok_or("This provider has no key. Add one on the Providers page.")?;
    let dir = desktop::library_dir().ok_or("Claude Desktop 3P not found (configLibrary folder). Open the app once and try again.")?;
    let models: Vec<String> = p.models.clone();
    let entry = desktop::render_entry(&req, &p.base_url, &key, &models, &p.headers)?;
    let (id, writes) = desktop::plan(&dir, &p.name, &entry)?;
    let detail = format!("{} · {} · {} model", p.name, req.model, models.len().max(1));
    let change = store().apply("Setup Claude Desktop", &detail, &writes)?;
    let _ = id;
    Ok(SetupResult { change, message: "Claude Desktop is set up. Quit and reopen the app to apply.".into() })
}

/// Remove entries from Claude Desktop's configLibrary, including ones the tool
/// did not create (made in the app or by an older tool). One undoable change.
#[tauri::command]
fn claude_desktop_remove(ids: Vec<String>) -> Result<SetupResult, String> {
    let dir = desktop::library_dir().ok_or("Claude Desktop's configLibrary not found")?;
    let (writes, deletes, names) = desktop::plan_remove(&dir, &ids)?;
    let change = store().apply_full("Remove Claude Desktop configuration", &names.join(", "), &writes, &deletes, &[], None)?;
    Ok(SetupResult { change, message: format!("Removed {} configuration(s). Quit and reopen Claude Desktop.", ids.len()) })
}

// ---------------------------------------------------------------- orca

#[tauri::command]
fn orca_status() -> orca::Status {
    orca::status()
}

/// Orca mirrors ~/.codex/config.toml itself when it starts Codex. This copies
/// the routing keys over right away (one undoable change) for when Orca has not
/// done it yet; the daemon still needs a restart.
#[tauri::command]
fn orca_sync() -> Result<SetupResult, String> {
    let (path, after) = orca::sync()?;
    let change = store().apply("Sync Codex to Orca", "Copy gateway and model from ~/.codex/config.toml", &[(path, after.into_bytes())])?;
    Ok(SetupResult { change, message: "Synced. Restart the daemon for Orca to apply it.".into() })
}

#[tauri::command]
fn codex_current() -> codex::Current {
    let cur = codex::read_current(&std::fs::read_to_string(codex::config_path()).unwrap_or_default());
    codex::with_provider_id(cur, &providers().list(&Keychain))
}

/// Codex provider name and env var a saved provider will use.
#[tauri::command]
fn codex_names(id: String) -> Result<codex::Names, String> {
    Ok(codex::names(&providers().get(&id, &Keychain)?))
}

/// Override (or with empty strings, reset) the Codex provider name / env var.
#[tauri::command]
fn set_codex_names(id: String, name: String, env: String) -> Result<codex::Names, String> {
    let p = providers().set_codex_names(&id, Some(name), Some(env), &Keychain)?;
    Ok(codex::names(&p))
}

#[derive(Serialize)]
struct SetupResult {
    change: store::Change,
    message: String,
}

#[tauri::command]
fn codex_setup(req: codex::Request) -> Result<SetupResult, String> {
    let ps = providers();
    let p = ps.get(&req.provider_id, &Keychain)?;
    let key = Keychain
        .get(&providers::account(&p.id))
        .ok_or("This provider has no key. Add one on the Providers page.")?;
    let data = data_dir();
    let path = codex::config_path();
    let before = std::fs::read_to_string(&path).unwrap_or_default();

    let mut writes = Vec::new();
    let catalog = if req.catalog {
        let models = if p.models.is_empty() { vec![req.model.clone()] } else { p.models.clone() };
        let (cpath, bytes) = codex::build_catalog(&models, &req.model, req.images, req.levels, &data)?;
        if !cpath.exists() {
            writes.push((cpath.clone(), bytes));
        }
        Some(cpath)
    } else {
        None
    };
    let names = codex::names(&p);
    let after = codex::render(&before, &req, &names, &p.base_url, catalog.as_deref(), &data)?;
    writes.push((path, after.into_bytes()));

    let mut env_sets = vec![];
    if UserEnv.get(&names.env).as_deref() != Some(key.as_str()) {
        env_sets.push((names.env.clone(), Some(key.clone())));
    }
    // Variables earlier builds made for this provider, if they hold this key:
    // the new table no longer reads them. Undo brings them back.
    for old in &names.old_envs {
        if UserEnv.get(old).as_deref() == Some(key.as_str()) {
            env_sets.push((old.clone(), None));
        }
    }
    let mut detail = format!("{} · {}", p.name, req.model);
    if catalog.is_some() {
        detail.push_str(&format!(" · {} model trong /model", p.models.len().max(1)));
    }
    let change = store().apply_with_env("Setup Codex", &detail, &writes, &env_sets, Some((&UserEnv, &Keychain)))?;
    // The user's switch is the latest word on this gateway; keep it for next time.
    if req.catalog && p.effort_levels != Some(req.levels) {
        let _ = ps.set_effort_levels(&p.id, Some(req.levels), &Keychain);
    }
    Ok(SetupResult { change, message: "Codex is set up".into() })
}

/// Image guidance in Codex's AGENTS.md and the image model per gateway.
#[tauri::command]
fn codex_image_status() -> image::Installed {
    image::status(&codex::codex_home())
}

/// Choose the image model for this provider's gateway and make sure the
/// (gateway-free) guidance block is in AGENTS.md. The helper follows whatever
/// provider Codex uses, so this only records "gateway URL → image model".
/// The provider's Codex env var is set so the helper can read the key.
#[tauri::command]
fn codex_image_setup(provider_id: String, model: String) -> Result<SetupResult, String> {
    let p = providers().get(&provider_id, &Keychain)?;
    let key = Keychain
        .get(&providers::account(&p.id))
        .ok_or("This provider has no key. Add one on the Providers page.")?;
    let model = image::valid_model(&model)?.to_string();
    let home = codex::codex_home();
    let names = codex::names(&p);
    let agents = image::agents_path(&home);
    let before = std::fs::read_to_string(&agents).unwrap_or_default();
    let after = image::install(&before)?;
    let models_path = image::models_path(&home);
    let mut models = image::read_models(&std::fs::read_to_string(&models_path).unwrap_or_default());
    models.insert(p.base_url.clone(), model.clone());
    let mut writes = vec![(models_path, image::write_models(&models))];
    if after != before {
        writes.push((agents, after.into_bytes()));
    }
    let mut env_sets = vec![];
    if UserEnv.get(&names.env).as_deref() != Some(key.as_str()) {
        env_sets.push((names.env.clone(), Some(key)));
    }
    let change = store().apply_with_env("Codex image generation", &format!("{} · {}", p.name, model), &writes, &env_sets, Some((&UserEnv, &Keychain)))?;
    Ok(SetupResult { change, message: "Image generation set up".into() })
}

/// Remove the block and the image model list. One undoable change.
#[tauri::command]
fn codex_image_remove() -> Result<SetupResult, String> {
    let home = codex::codex_home();
    let agents = image::agents_path(&home);
    let before = std::fs::read_to_string(&agents).unwrap_or_default();
    let mut writes = vec![];
    if let Some(after) = image::remove(&before)? {
        writes.push((agents, after.into_bytes()));
    }
    let models = image::models_path(&home);
    let deletes: Vec<std::path::PathBuf> = if models.exists() { vec![models] } else { vec![] };
    if writes.is_empty() && deletes.is_empty() {
        return Err("No image guidance to remove".into());
    }
    let change = store().apply_full("Remove Codex image generation", "", &writes, &deletes, &[], None)?;
    Ok(SetupResult { change, message: "Image guidance removed".into() })
}

// ---------------------------------------------------------------- claude code

#[tauri::command]
fn claude_current() -> claude::Current {
    let text = std::fs::read_to_string(claude::settings_path()).unwrap_or_default();
    claude::read_current(&text, &|n| UserEnv.get(n))
}

#[tauri::command]
fn claude_setup(req: claude::Request) -> Result<SetupResult, String> {
    let p = providers().get(&req.provider_id, &Keychain)?;
    let key = Keychain
        .get(&providers::account(&p.id))
        .ok_or("This provider has no key. Add one on the Providers page.")?;
    let path = claude::settings_path();
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    let owned = reset::owned_header_values(&providers().list(&Keychain));
    let after = claude::render(&before, &req, &p.base_url, &key, &p.models, &p.headers, &owned)?;
    let mut detail = format!("{} · {}", p.name, req.model);
    if req.picker {
        detail.push_str(&format!(" · {} model trong /model", p.models.len().max(1)));
    }
    let change = store().apply("Setup Claude Code", &detail, &[(path, after.into_bytes())])?;
    Ok(SetupResult { change, message: "Claude Code is set up".into() })
}

// ------------------------------------------------------------------ usage
// Token usage read from the apps' own logs: only on demand (Usage page, and the
// optional tray, mini window and alerts, all off by default).

/// Run `f` on the usage index off the async runtime. The index is re-scanned
/// incrementally when it is stale or `refresh` is set.
async fn with_usage<T: Send + 'static>(
    state: tauri::State<'_, usage::UsageState>,
    refresh: bool,
    f: impl FnOnce(&usage::Index) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut s = usage::lock(&st);
        f(s.index(refresh))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn usage_query(state: tauri::State<'_, usage::UsageState>, filter: usage_report::UsageFilter, refresh: Option<bool>) -> Result<usage_report::UsageReport, String> {
    with_usage(state, refresh.unwrap_or(false), move |i| usage_report::query(i, &filter, &chrono::Local, chrono::Utc::now())).await
}

#[tauri::command]
async fn usage_session(state: tauri::State<'_, usage::UsageState>, key: String) -> Result<usage_report::SessionDetail, String> {
    with_usage(state, false, move |i| usage_report::session(i, &key)).await
}

#[tauri::command]
async fn usage_blocks(state: tauri::State<'_, usage::UsageState>, filter: usage_report::UsageFilter) -> Result<usage_report::BlocksReport, String> {
    with_usage(state, false, move |i| usage_report::blocks(i, &filter, &chrono::Local, chrono::Utc::now())).await
}

#[tauri::command]
async fn usage_analytics(state: tauri::State<'_, usage::UsageState>, filter: usage_report::UsageFilter) -> Result<usage_report::UsageAnalytics, String> {
    with_usage(state, false, move |i| usage_report::analytics(i, &filter, &chrono::Local, chrono::Utc::now())).await
}

/// Writes the export into Downloads and returns the file path.
#[tauri::command]
async fn usage_export(state: tauri::State<'_, usage::UsageState>, filter: usage_report::UsageFilter, format: String) -> Result<String, String> {
    with_usage(state, false, move |i| {
        let ext = if format == "json" { "json" } else if format == "csv" { "csv" } else { return Err("Unknown export format".into()) };
        let text = usage_report::export_text(i, &filter, &chrono::Local, chrono::Utc::now(), ext)?;
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        usage_prefs::write_export(&usage_prefs::export_dir(), &date, ext, &text).map(|p| p.display().to_string())
    })
    .await
}

#[tauri::command]
fn usage_prices_get() -> std::collections::BTreeMap<String, usage_prefs::UsagePrice> {
    usage_prefs::prices_get_in(&data_dir())
}

#[tauri::command]
fn usage_prices_set(prices: std::collections::BTreeMap<String, usage_prefs::UsagePrice>) -> Result<(), String> {
    usage_prefs::prices_set_in(&data_dir(), &prices)
}

#[tauri::command]
fn usage_settings_get() -> usage_prefs::UsageSettings {
    usage_prefs::settings_get_in(&data_dir())
}

/// Saves and applies right away (tray, mini window, alerts). Async: creating
/// the mini window from a sync command deadlocks on Windows.
#[tauri::command]
async fn usage_settings_set(app: tauri::AppHandle, settings: usage_prefs::UsageSettings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || usage_live::save_and_apply(&app, &settings)).await.map_err(|e| e.to_string())?
}

/// Watch the log folders while the Usage page or the mini window is visible;
/// emits "usage-changed". Each window holds the watcher separately.
#[tauri::command]
async fn usage_watch(app: tauri::AppHandle, window: tauri::Window, state: tauri::State<'_, usage::UsageState>, on: bool) -> Result<(), String> {
    let st = state.inner().clone();
    let user = window.label().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let watch = app.state::<usage_prefs::Watch>().inner().clone();
        usage_prefs::set_watch(app, st, watch, &user, on)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Mini window: double-click brings the main window back.
#[tauri::command]
async fn usage_open_main(app: tauri::AppHandle) {
    usage_live::open_main(&app);
}

/// One-click health check of what is configured on this machine.
#[tauri::command]
async fn doctor_run() -> Result<Vec<doctor::Check>, String> {
    tauri::async_runtime::spawn_blocking(doctor::run).await.map_err(|e| e.to_string())
}

/// Env vars left behind by older setups that break a gateway (they make the app
/// send fields the upstream rejects). Shown in Settings so the user can remove them.
const STALE_ENV: &[&str] = &["CLAUDE_CODE_EFFORT_LEVEL"];

/// Other variables Doctor may offer to remove, with the reason recorded for undo.
/// ANTHROPIC_MODEL outranks settings.json, so a leftover one pins every new
/// Claude Code session to the wrong model.
const REMOVABLE_ENV: &[(&str, &str)] = &[("ANTHROPIC_MODEL", "Old environment variable overriding the Claude Code model")];

#[tauri::command]
fn stale_env() -> Vec<String> {
    STALE_ENV.iter().filter(|n| UserEnv.get(n).is_some()).map(|n| n.to_string()).collect()
}

fn removal_reason(name: &str) -> Option<&'static str> {
    if STALE_ENV.contains(&name) {
        return Some("Old environment variable causing gateway 400 errors");
    }
    REMOVABLE_ENV.iter().find(|(n, _)| *n == name).map(|(_, why)| *why)
}

#[tauri::command]
fn clear_stale_env(name: String) -> Result<(), String> {
    let why = removal_reason(&name).ok_or("This variable is not in the list allowed to be removed")?;
    // Recorded like a setup so it shows on the Restore page and can be undone.
    store()
        .apply_with_env(&format!("Remove variable {name}"), why, &[], &[(name, None)], Some((&UserEnv, &Keychain)))
        .map(|_| ())
}

/// Quit Codex Desktop (soft close; `force` kills it) and open it again.
#[tauri::command]
async fn codex_desktop_restart(force: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || rlog::done("Codex Desktop", restart::codex_desktop(force))).await.map_err(|e| e.to_string())?
}

/// Masked JSON snapshot for bug reports. The UI shows it before copying.
#[tauri::command]
async fn support_report() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(report::build).await.map_err(|e| e.to_string())
}

/// Restart Orca's Codex daemon and check it serves the catalog's models.
#[tauri::command]
async fn orca_restart() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| rlog::done("Orca", restart::orca_daemon())).await.map_err(|e| e.to_string())?
}

/// What "Restore defaults" would change, per app. Read only.
#[tauri::command]
async fn reset_preview(req: reset::Request) -> Result<Vec<reset::Item>, String> {
    tauri::async_runtime::spawn_blocking(move || reset::preview(&req)).await.map_err(|e| e.to_string())?
}

/// Take the chosen apps back to default as one undoable change.
#[tauri::command]
async fn reset_apply(req: reset::Request) -> Result<store::Change, String> {
    tauri::async_runtime::spawn_blocking(move || reset::apply(&req)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn preset_samples() -> Vec<preset::Preset> {
    preset::samples()
}

#[tauri::command]
fn preset_parse(text: String) -> Result<preset::Preset, String> {
    preset::parse(&text)
}

/// Add the preset as a provider (with the pasted key) and return its id.
#[tauri::command]
fn preset_import(text: String, key: String) -> Result<providers::Provider, String> {
    let p = preset::parse(&text)?;
    let mut prov = providers().upsert(None, &p.name, &p.base_url, Some(&key), &Keychain)?;
    if !p.headers.is_empty() {
        prov = providers().set_headers(&prov.id, p.headers.clone(), &Keychain)?;
    }
    if !p.models.is_empty() {
        return providers().set_models(&prov.id, p.models.clone(), Default::default(), 0, &Keychain).map(|x| providers::Provider { has_key: true, ..x });
    }
    Ok(prov)
}

/// Preset JSON for a saved provider (never includes the key).
#[tauri::command]
fn preset_export(id: String) -> Result<String, String> {
    let p = providers().get(&id, &Keychain)?;
    Ok(preset::export(&p))
}

/// Capability test (streaming, tools, responses/messages, caching, images,
/// reasoning effort)
/// for a few models. Costs a few hundred tokens per model; the UI asks first.
#[tauri::command]
async fn capability_test(id: String, models: Vec<String>) -> Result<Vec<(String, probe::Caps)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = providers().get(&id, &Keychain)?;
        let key = Keychain.get(&providers::account(&id)).ok_or("No key for this provider")?;
        if models.is_empty() || models.len() > 8 {
            return Err("Choose 1 to 8 models".into());
        }
        let results = probe::capabilities(&p.base_url, &key, models);
        // Remember whether Codex may send reasoning.effort to this gateway.
        if let Some(ok) = probe::effort_verdict_of(&results) {
            providers().set_effort_levels(&id, Some(ok), &Keychain)?;
        }
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Apply OS-native translucency to the window. Returns whether it worked, so the
/// UI can fall back to a solid sidebar (Windows 10, Linux).
fn apply_effects(window: &tauri::WebviewWindow) -> bool {
    #[cfg(target_os = "windows")]
    {
        return window_vibrancy::apply_mica(window, None).is_ok();
    }
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
        return apply_vibrancy(window, NSVisualEffectMaterial::Sidebar, None, None).is_ok();
    }
    #[allow(unreachable_code)]
    {
        let _ = window;
        false
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch (e.g. from the Start menu while the
        // window is hidden in the tray) just brings the running one back, so
        // there is never a second tray icon or quota scheduler.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(usage::new_state())
        .manage(usage_prefs::Watch::default())
        .manage(usage_live::Live::default())
        .manage(quota_watch::QuotaWatch::default())
        .setup(|app| {
            // Backups from builds before encryption: seal them in the background.
            std::thread::spawn(|| store().seal_old_backups());
            if let Some(window) = app.get_webview_window("main") {
                let glass = apply_effects(&window);
                let _ = window.eval(&format!("document.documentElement.dataset.glass='{glass}'"));
            }
            // Tray, mini window and alerts are all off unless the user turned them on.
            let s = usage_prefs::settings_get_in(&data_dir());
            if s.tray || s.overlay || s.notify {
                usage_live::apply(app.handle(), &s);
            }
            // Saved quota watches resume; no thread at all when there are none.
            app.state::<quota_watch::QuotaWatch>().start(app.handle());
            Ok(())
        })
        .on_window_event(usage_live::on_window_event)
        .invoke_handler(tauri::generate_handler![
            scan, history, undo,
            list_providers, save_provider, remove_provider, set_provider_headers, refresh_models, probe_models,
            codex_current, codex_setup, claude_current, claude_setup,
            claude_desktop_current, claude_desktop_setup, claude_desktop_restart,
            orca_status, orca_sync, orca_restart, claude_desktop_remove, codex_names, set_codex_names, codex_desktop_restart,
            support_report, doctor_run, stale_env, clear_stale_env,
            reset_preview, reset_apply, preset_samples, preset_parse, preset_import, preset_export,
            capability_test, codex_image_status, codex_image_setup, codex_image_remove,
            usage_query, usage_session, usage_blocks, usage_analytics, usage_export,
            usage_prices_get, usage_prices_set, usage_settings_get, usage_settings_set, usage_watch, usage_open_main,
            quota_watch_list, quota_watch_add, quota_watch_remove
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// Every "env:NAME" fix Doctor can offer must be accepted by clear_stale_env,
    /// or the Fix button only shows "not in the list allowed to be removed".
    #[test]
    fn doctor_env_fixes_are_removable() {
        let src = include_str!("doctor.rs");
        let names: Vec<&str> = src.split("\"env:").skip(1).filter_map(|s| s.split('"').next()).filter(|n| !n.is_empty() && !n.contains('<')).collect();
        assert!(names.contains(&"ANTHROPIC_MODEL"));
        for n in names {
            assert!(super::removal_reason(n).is_some(), "{n} is offered by Doctor but cannot be removed");
        }
        assert!(super::removal_reason("PATH").is_none());
    }
}
