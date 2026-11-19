//! Usage page settings, prices, export files and the opt-in log watcher
//! (shared by the Usage page and the mini window).

use crate::usage::{self, UsageState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Price per million tokens, in whatever currency the user enters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsagePrice {
    pub input: f64,
    pub cached: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write: Option<f64>,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UsageSettings {
    pub watch: bool,
    pub tray: bool,
    pub overlay: bool,
    pub notify: bool,
    pub block_limit: Option<u64>,
    /// Start a quota watch by itself after "Kiểm tra model" finds the group
    /// Codex / Claude Code uses out of quota. The manual button works without it.
    pub quota_auto: bool,
    /// Closing the main window hides it to the tray (only while `tray` is on).
    pub close_to_tray: bool,
}

impl Default for UsageSettings {
    fn default() -> Self {
        UsageSettings { watch: true, tray: false, overlay: false, notify: false, block_limit: None, quota_auto: false, close_to_tray: false }
    }
}

fn read_or_default<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?;
    crate::store::atomic_write(path, &bytes)
}

pub fn prices_get_in(dir: &Path) -> BTreeMap<String, UsagePrice> {
    read_or_default(&dir.join("usage-prices.json"))
}

pub fn prices_set_in(dir: &Path, prices: &BTreeMap<String, UsagePrice>) -> Result<(), String> {
    if prices.values().any(|p| [p.input, p.cached, p.output, p.write.unwrap_or(0.0)].iter().any(|x| !x.is_finite() || *x < 0.0)) {
        return Err("Prices must be zero or more".into());
    }
    write_json(&dir.join("usage-prices.json"), prices)
}

pub fn settings_get_in(dir: &Path) -> UsageSettings {
    read_or_default(&dir.join("usage-settings.json"))
}

pub fn settings_set_in(dir: &Path, s: &UsageSettings) -> Result<(), String> {
    write_json(&dir.join("usage-settings.json"), s)
}

/// Write an export into Downloads (or the data folder) without overwriting
/// an earlier one. Returns the file path.
pub fn write_export(dir: &Path, date: &str, ext: &str, text: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|_| "Could not create the export folder".to_string())?;
    let mut path = dir.join(format!("gateswitch-usage-{date}.{ext}"));
    let mut n = 1;
    while path.exists() {
        n += 1;
        path = dir.join(format!("gateswitch-usage-{date}-{n}.{ext}"));
    }
    std::fs::write(&path, text).map_err(|e| format!("Could not write the export file: {}", e.kind()))?;
    Ok(path)
}

pub fn export_dir() -> PathBuf {
    dirs::download_dir().filter(|d| d.is_dir()).unwrap_or_else(crate::data_dir)
}

// ------------------------------------------------------------------ watcher

/// Holds the file watcher while the Usage page or the mini window is visible.
/// Dropping it ends the debounce thread, so nothing runs when it is off.
#[derive(Default, Clone)]
pub struct Watch(std::sync::Arc<WatchInner>);

#[derive(Default)]
pub struct WatchInner {
    slot: Mutex<Option<notify::RecommendedWatcher>>,
    /// Who wants it (window labels). A set, not a count: a page may send "off"
    /// twice (hidden, then unmounted) and the overlay may vanish without one.
    users: Mutex<std::collections::BTreeSet<String>>,
    /// Bumped by every call, so a delayed stop knows whether it is still wanted.
    generation: std::sync::atomic::AtomicU64,
}

const DEBOUNCE: Duration = Duration::from_secs(2);
/// Refresh at least this often while logs never go quiet for DEBOUNCE.
const MAX_WAIT: Duration = Duration::from_secs(8);
/// "Off" waits this long, so on/off/on in quick succession (React StrictMode,
/// page remounts) keeps one watcher instead of tearing it down and rebuilding.
const STOP_DELAY: Duration = Duration::from_millis(1500);

/// Add or remove `user`'s hold on the watcher. It runs while anyone holds it.
/// Returns whether the watcher should now run.
fn hold(users: &mut std::collections::BTreeSet<String>, user: &str, on: bool) -> bool {
    if on {
        users.insert(user.to_string());
    } else {
        users.remove(user);
    }
    !users.is_empty()
}

/// Idempotent per user; tolerates on/off/on arriving in quick succession.
pub fn set_watch(app: tauri::AppHandle, state: UsageState, watch: Watch, user: &str, on: bool) -> Result<(), String> {
    use notify::Watcher;
    use std::sync::atomic::Ordering;
    let watch = watch.0;
    let on = hold(&mut watch.users.lock().unwrap_or_else(|e| e.into_inner()), user, on);
    let generation = watch.generation.fetch_add(1, Ordering::SeqCst) + 1;
    if !on {
        std::thread::spawn(move || {
            std::thread::sleep(STOP_DELAY);
            let mut slot = watch.slot.lock().unwrap_or_else(|e| e.into_inner());
            if watch.generation.load(Ordering::SeqCst) == generation && slot.take().is_some() {
                drop(slot);
                usage::lock(&state).flush();
            }
        });
        return Ok(());
    }
    let mut slot = watch.slot.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return Ok(());
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if ev.paths.iter().any(|p| p.extension().is_some_and(|x| x == "jsonl")) {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|_| "Could not start watching the log folders".to_string())?;
    // Not from the shared state: a scan may hold its lock for a while.
    let dirs: Vec<PathBuf> = usage::Roots::real().dirs.into_iter().map(|(d, _)| d).filter(|d| d.is_dir()).collect();
    for d in &dirs {
        // A folder that cannot be watched only loses live updates.
        let _ = w.watch(d, notify::RecursiveMode::Recursive);
    }
    *slot = Some(w);
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        use tauri::Emitter;
        // Ends when the watcher (and with it the sender) is dropped.
        while rx.recv().is_ok() {
            let first = std::time::Instant::now();
            while first.elapsed() < MAX_WAIT {
                match rx.recv_timeout(DEBOUNCE.min(MAX_WAIT.saturating_sub(first.elapsed()))) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            let changed = usage::lock(&state).refresh();
            // Lock released before emitting: the page queries right away.
            if changed {
                let _ = app.emit("usage-changed", ());
                crate::usage_live::after_refresh(&app);
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_and_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(prices_get_in(dir.path()).is_empty(), "no built-in prices");
        assert_eq!(settings_get_in(dir.path()), UsageSettings::default());
        assert!(UsageSettings::default().watch);
        let mut p = BTreeMap::new();
        p.insert("m".to_string(), UsagePrice { input: 3.0, cached: 0.3, write: None, output: 15.0 });
        prices_set_in(dir.path(), &p).unwrap();
        assert_eq!(prices_get_in(dir.path()), p);
        let v = serde_json::to_value(&p).unwrap();
        assert!(v["m"].get("write").is_none());
        p.insert("bad".into(), UsagePrice { input: -1.0, cached: 0.0, write: None, output: 0.0 });
        assert!(prices_set_in(dir.path(), &p).is_err());
        let s = UsageSettings { block_limit: Some(1000), tray: true, ..Default::default() };
        settings_set_in(dir.path(), &s).unwrap();
        assert_eq!(settings_get_in(dir.path()), s);
        let partial: UsageSettings = serde_json::from_str(r#"{"tray":true}"#).unwrap();
        assert!(partial.watch && partial.tray);
    }

    #[test]
    fn new_background_settings_are_off_and_old_files_still_load() {
        let d = UsageSettings::default();
        assert!(!d.quota_auto && !d.close_to_tray && !d.tray && !d.overlay && !d.notify);
        // usage-settings.json written before these fields existed.
        let old: UsageSettings = serde_json::from_str(r#"{"watch":false,"tray":true,"overlay":false,"notify":true,"block_limit":5000000}"#).unwrap();
        assert!(!old.quota_auto && !old.close_to_tray);
        assert!(old.tray && old.notify && !old.watch);
        assert_eq!(old.block_limit, Some(5_000_000));
        let v = serde_json::to_value(&UsageSettings { quota_auto: true, close_to_tray: true, ..d }).unwrap();
        assert_eq!((v["quota_auto"].as_bool(), v["close_to_tray"].as_bool()), (Some(true), Some(true)));
    }

    #[test]
    fn watcher_holds_are_a_set() {
        let mut u = std::collections::BTreeSet::new();
        assert!(hold(&mut u, "main", true));
        assert!(hold(&mut u, "main", true));
        assert!(hold(&mut u, "overlay", true));
        assert!(hold(&mut u, "main", false));
        assert!(hold(&mut u, "main", false), "a second off from main does not drop the overlay's hold");
        assert!(!hold(&mut u, "overlay", false));
    }

    #[test]
    fn export_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_export(dir.path(), "2026-11-19", "csv", "x").unwrap();
        let b = write_export(dir.path(), "2026-11-19", "csv", "y").unwrap();
        assert!(a.ends_with("gateswitch-usage-2026-11-19.csv"));
        assert!(b.ends_with("gateswitch-usage-2026-11-19-2.csv"));
    }
}
