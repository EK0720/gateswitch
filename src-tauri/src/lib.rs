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
mod secrets;
mod store;
mod usage;

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
async fn probe_models(id: String, models: Option<Vec<String>>) -> Result<providers::Provider, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ps = providers();
        let p = ps.get(&id, &Keychain)?;
        let key = Keychain.get(&providers::account(&id)).ok_or("No key for this provider")?;
        let list = models.unwrap_or_else(|| p.models.clone());
        if list.is_empty() {
            return Err("No models to test. Click Load models first.".into());
        }
        let results = probe::probe_all(&p.base_url, &key, list);
        ps.set_probes(&id, results, &Keychain)
    })
    .await
    .map_err(|e| e.to_string())?
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
        let (cpath, bytes) = codex::build_catalog(&models, &req.model, req.images, &data)?;
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

/// Token usage read from the apps' own logs. Only runs when the page opens.
#[tauri::command]
async fn usage_stats() -> Result<usage::Usage, String> {
    tauri::async_runtime::spawn_blocking(usage::collect).await.map_err(|e| e.to_string())
}

/// One-click health check of what is configured on this machine.
#[tauri::command]
async fn doctor_run() -> Result<Vec<doctor::Check>, String> {
    tauri::async_runtime::spawn_blocking(doctor::run).await.map_err(|e| e.to_string())
}

/// Env vars left behind by older setups that break a gateway (they make the app
/// send fields the upstream rejects). Shown in Settings so the user can remove them.
const STALE_ENV: &[&str] = &["CLAUDE_CODE_EFFORT_LEVEL"];

#[tauri::command]
fn stale_env() -> Vec<String> {
    STALE_ENV.iter().filter(|n| UserEnv.get(n).is_some()).map(|n| n.to_string()).collect()
}

#[tauri::command]
fn clear_stale_env(name: String) -> Result<(), String> {
    if !STALE_ENV.contains(&name.as_str()) {
        return Err("This variable is not in the list allowed to be removed".into());
    }
    // Recorded like a setup so it shows in Khôi phục and can be undone.
    store()
        .apply_with_env(&format!("Remove variable {name}"), "Old environment variable causing gateway 400 errors", &[], &[(name, None)], Some((&UserEnv, &Keychain)))
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

/// What "Khôi phục mặc định" would change, per app. Read only.
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

/// Capability test (streaming, tools, responses/messages, caching, images)
/// for a few models. Costs a few hundred tokens per model; the UI asks first.
#[tauri::command]
async fn capability_test(id: String, models: Vec<String>) -> Result<Vec<(String, probe::Caps)>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = providers().get(&id, &Keychain)?;
        let key = Keychain.get(&providers::account(&id)).ok_or("No key for this provider")?;
        if models.is_empty() || models.len() > 8 {
            return Err("Choose 1 to 8 models".into());
        }
        Ok(probe::capabilities(&p.base_url, &key, models))
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
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Backups from builds before encryption: seal them in the background.
            std::thread::spawn(|| store().seal_old_backups());
            if let Some(window) = app.get_webview_window("main") {
                let glass = apply_effects(&window);
                let _ = window.eval(&format!("document.documentElement.dataset.glass='{glass}'"));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan, history, undo,
            list_providers, save_provider, remove_provider, set_provider_headers, refresh_models, probe_models,
            codex_current, codex_setup, claude_current, claude_setup,
            claude_desktop_current, claude_desktop_setup, claude_desktop_restart,
            orca_status, orca_sync, orca_restart, claude_desktop_remove, codex_names, set_codex_names, codex_desktop_restart,
            support_report, usage_stats, doctor_run, stale_env, clear_stale_env,
            reset_preview, reset_apply, preset_samples, preset_parse, preset_import, preset_export,
            capability_test, codex_image_status, codex_image_setup, codex_image_remove
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
