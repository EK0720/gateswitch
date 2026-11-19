//! Optional live views of usage, all off by default (Settings › Notifications & background):
//! - tray icon whose tooltip shows today's tokens and the current 5-hour block
//! - mini window ("overlay"): a small always-on-top window, same frontend
//! - alerts: one OS notification per block at 80% and at 100% of the block limit
//!
//! `apply` makes the running app match the saved settings, at start and on
//! every settings change. Nothing here runs while all three are off. While the
//! tray or alerts are on, one thread re-reads the numbers every 60 s (cheap:
//! the index is only re-scanned when older than its stale time) and the log
//! watcher, when a page holds it, pushes fresh numbers right after a change.
//! Tray and notification texts are English: the UI language lives in the
//! webview's localStorage, which the backend cannot read cheaply.

use crate::usage::{self, Counted, UsageState};
use crate::usage_prefs::{self, UsageSettings};
use crate::usage_report::{fmt_tokens, UsageBlock};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const OVERLAY: &str = "overlay";
const TRAY_ID: &str = "usage";
const TICK: Duration = Duration::from_secs(60);
/// Save the overlay position once it stops moving for this long.
const POS_SAVE_DELAY: Duration = Duration::from_millis(600);

#[derive(Default)]
pub struct Live {
    inner: Mutex<Inner>,
    /// Serialises read-check-write of the alert state (ticker and watcher both check).
    alert: Mutex<()>,
}

#[derive(Default)]
struct Inner {
    settings: UsageSettings,
    /// Dropping the sender ends the ticker thread. `true` = re-scan now.
    ticker: Option<Sender<bool>>,
    pos_gen: u64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ------------------------------------------------------------------ apply

/// Make tray, overlay and the ticker match `s`. Idempotent. Not from a sync
/// command or a menu handler: creating a window there deadlocks on Windows.
pub fn apply(app: &AppHandle, s: &UsageSettings) {
    set_tray(app, s.tray);
    set_overlay(app, s.overlay);
    let live = app.state::<Live>();
    let mut g = lock(&live.inner);
    g.settings = s.clone();
    if !(s.tray || s.notify) {
        g.ticker = None;
    } else if let Some(tx) = &g.ticker {
        // Limit or tray may have changed: recompute now.
        let _ = tx.send(false);
    } else {
        let (tx, rx) = std::sync::mpsc::channel();
        g.ticker = Some(tx);
        let app = app.clone();
        std::thread::spawn(move || ticker(app, rx));
    }
}

/// Save new settings, apply them and tell every window.
pub fn save_and_apply(app: &AppHandle, s: &UsageSettings) -> Result<(), String> {
    usage_prefs::settings_set_in(&crate::data_dir(), s)?;
    apply(app, s);
    let _ = app.emit("usage-settings-changed", s);
    Ok(())
}

fn toggle_overlay(app: &AppHandle) {
    let mut s = usage_prefs::settings_get_in(&crate::data_dir());
    s.overlay = !s.overlay;
    let _ = save_and_apply(app, &s);
}

pub fn open_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// ------------------------------------------------------------------ numbers

fn ticker(app: AppHandle, rx: Receiver<bool>) {
    let mut force = false;
    loop {
        update(&app, force);
        force = match rx.recv_timeout(TICK) {
            Ok(f) => f,
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
    }
}

fn update(app: &AppHandle, force: bool) {
    let state = app.state::<UsageState>().inner().clone();
    if force && usage::lock(&state).refresh() {
        let _ = app.emit("usage-changed", ());
    }
    let today = usage::today_totals(&state);
    let block = usage::current_block(&state);
    publish(app, &today, block.as_ref());
}

/// Called by the log watcher after a re-scan that found new lines.
pub fn after_refresh(app: &AppHandle) {
    // Free: a reply from a watched quota group ends its watch.
    crate::quota_watch::after_usage(app);
    let s = lock(&app.state::<Live>().inner).settings.clone();
    if s.tray || s.notify {
        let state = app.state::<UsageState>().inner().clone();
        let today = usage::today_totals(&state);
        let block = usage::current_block(&state);
        publish(app, &today, block.as_ref());
    }
}

/// No lock is held here: set_tooltip waits for the main thread.
fn publish(app: &AppHandle, today: &Counted, block: Option<&UsageBlock>) {
    let s = lock(&app.state::<Live>().inner).settings.clone();
    if s.tray {
        if let Some(t) = app.tray_by_id(TRAY_ID) {
            let _ = t.set_tooltip(Some(tooltip_text(today.t.sum(), block.map(|b| b.c.t.sum()), s.block_limit)));
        }
    }
    if s.notify {
        if let Some(b) = block {
            check_alert(app, b, s.block_limit);
        }
    }
}

/// "Gateswitch — today 1.2B tok · block 62%".
pub fn tooltip_text(today: u64, block: Option<u64>, limit: Option<u64>) -> String {
    let b = match (block, limit.filter(|l| *l > 0)) {
        (None, _) => "no active block".to_string(),
        (Some(u), Some(l)) => format!("block {}%", (u as f64 / l as f64 * 100.0).round() as u64),
        (Some(u), None) => format!("block {} tok", fmt_tokens(u)),
    };
    format!("Gateswitch — today {} tok · {b}", fmt_tokens(today))
}

// ------------------------------------------------------------------ alerts

/// Last alert sent, persisted so a restart does not repeat it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Notified {
    /// Start of the block (RFC 3339).
    pub block: String,
    /// 0, 80 or 100.
    pub level: u8,
}

/// Which alert (80 or 100) is due now, and the state to store if one is.
/// Jumping from below 80% straight past 100% sends only the 100% alert.
pub fn alert_due(prev: &Notified, block_start: &str, used: u64, limit: Option<u64>) -> Option<(u8, Notified)> {
    let limit = limit.filter(|l| *l > 0)? as u128;
    let used = used as u128;
    let level = if used >= limit { 100 } else if used * 5 >= limit * 4 { 80 } else { 0 };
    let sent = if prev.block == block_start { prev.level } else { 0 };
    (level > sent).then(|| (level, Notified { block: block_start.to_string(), level }))
}

fn notified_path(dir: &Path) -> std::path::PathBuf {
    dir.join("usage-notified.json")
}

pub fn notified_get_in(dir: &Path) -> Notified {
    std::fs::read(notified_path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn notified_set_in(dir: &Path, n: &Notified) -> Result<(), String> {
    let bytes = serde_json::to_vec(n).map_err(|e| e.to_string())?;
    crate::store::atomic_write(&notified_path(dir), &bytes)
}

fn check_alert(app: &AppHandle, b: &UsageBlock, limit: Option<u64>) {
    let live = app.state::<Live>();
    let _one = lock(&live.alert);
    let dir = crate::data_dir();
    let used = b.c.t.sum();
    let Some((level, next)) = alert_due(&notified_get_in(&dir), &b.start, used, limit) else { return };
    // Stored first: a failed toast must not repeat every minute.
    let _ = notified_set_in(&dir, &next);
    let ends = chrono::DateTime::parse_from_rfc3339(&b.end).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
    let title = if level >= 100 { "5-hour block limit reached".to_string() } else { format!("5-hour block at {level}%") };
    let body = format!("{} of {} tokens used. The block ends at {ends}.", fmt_tokens(used), fmt_tokens(limit.unwrap_or(0)));
    toast(app, &format!("Gateswitch: {title}"), &body);
}

/// One OS notification (block alerts, quota watch). A failed toast is ignored.
pub fn toast(app: &AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

// ------------------------------------------------------------------ tray

fn set_tray(app: &AppHandle, on: bool) {
    let exists = app.tray_by_id(TRAY_ID).is_some();
    if on && !exists {
        if let Err(e) = build_tray(app) {
            eprintln!("tray: {e}");
        }
    } else if !on && exists {
        drop(app.remove_tray_by_id(TRAY_ID));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open Gateswitch", true, None::<&str>)?,
            &MenuItem::with_id(app, "overlay", "Show/hide mini window", true, None::<&str>)?,
            &MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    // The numbers come from the ticker: never read the logs on the main thread.
    let mut b = TrayIconBuilder::with_id(TRAY_ID).tooltip("Gateswitch").menu(&menu).on_menu_event(|app, ev| match ev.id().as_ref() {
        "open" => open_main(app),
        "overlay" => {
            // Off the main thread: creating a window from a menu handler deadlocks on Windows.
            let app = app.clone();
            std::thread::spawn(move || toggle_overlay(&app));
        }
        "refresh" => {
            if let Some(tx) = &lock(&app.state::<Live>().inner).ticker {
                let _ = tx.send(true);
            }
        }
        "quit" => app.exit(0),
        _ => {}
    });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

// ------------------------------------------------------------------ overlay

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
struct Pos {
    x: i32,
    y: i32,
}

fn pos_path() -> std::path::PathBuf {
    crate::data_dir().join("usage-overlay.json")
}

fn set_overlay(app: &AppHandle, on: bool) {
    match (on, app.get_webview_window(OVERLAY)) {
        (true, None) => {
            if let Err(e) = build_overlay(app) {
                eprintln!("overlay: {e}");
            }
        }
        // Destroyed, not hidden: its timer and watcher hold end with it.
        (false, Some(w)) => {
            let _ = w.destroy();
        }
        _ => {}
    }
}

const OVERLAY_W: f64 = 260.0;
const OVERLAY_H: f64 = 120.0;

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    use tauri::{PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
    // The page picks the overlay view from the window label.
    let w = WebviewWindowBuilder::new(app, OVERLAY, WebviewUrl::App("index.html".into()))
        .title("Gateswitch usage")
        .inner_size(OVERLAY_W, OVERLAY_H)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()?;
    let size = w.outer_size().unwrap_or_default();
    let monitors = w.available_monitors().unwrap_or_default();
    let rects: Vec<(i32, i32, u32, u32)> = monitors.iter().map(|m| (m.position().x, m.position().y, m.size().width, m.size().height)).collect();
    let saved: Option<Pos> = std::fs::read(pos_path()).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let pos = saved.filter(|p| on_screen(&rects, p.x, p.y, size.width)).or_else(|| {
        // Default: top-right corner of the primary monitor.
        let m = w.primary_monitor().ok().flatten()?;
        let margin = (24.0 * m.scale_factor()) as i32;
        Some(Pos { x: m.position().x + m.size().width as i32 - size.width as i32 - margin, y: m.position().y + margin * 2 })
    });
    if let Some(p) = pos {
        let _ = w.set_position(PhysicalPosition::new(p.x, p.y));
    }
    w.show()
}

/// Whether enough of a window at (x, y) is on some monitor to grab it.
fn on_screen(monitors: &[(i32, i32, u32, u32)], x: i32, y: i32, w: u32) -> bool {
    let w = w.max(1) as i64;
    monitors.iter().any(|&(mx, my, mw, mh)| {
        let (mx, my, mw, mh) = (mx as i64, my as i64, mw as i64, mh as i64);
        let (x, y) = (x as i64, y as i64);
        // The top strip (drag area) must be visible: at least 40 px across.
        let ix = (x + w).min(mx + mw) - x.max(mx);
        y >= my && y + 10 <= my + mh && ix >= 40.min(w)
    })
}

/// Window events from lib.rs.
pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    use tauri::WindowEvent;
    let app = window.app_handle();
    match (window.label(), event) {
        // "Closing hides to the tray": only while the tray icon really exists,
        // or there would be no way back. Timers and the quota watch keep running.
        ("main", WindowEvent::CloseRequested { api, .. }) => {
            let s = usage_prefs::settings_get_in(&crate::data_dir());
            if s.tray && s.close_to_tray && app.tray_by_id(TRAY_ID).is_some() {
                api.prevent_close();
                let _ = window.hide();
            }
        }
        // The overlay must not keep the app alive: closing main quits, as before.
        // Destroy (not close) keeps overlay=true for the next start.
        ("main", WindowEvent::Destroyed) => {
            if let Some(o) = app.get_webview_window(OVERLAY) {
                let _ = o.destroy();
            }
        }
        (OVERLAY, WindowEvent::CloseRequested { .. }) => {
            // The user closed it (× or Alt+F4): remember it as off.
            let mut s = usage_prefs::settings_get_in(&crate::data_dir());
            if s.overlay {
                s.overlay = false;
                let _ = usage_prefs::settings_set_in(&crate::data_dir(), &s);
                lock(&app.state::<Live>().inner).settings.overlay = false;
                let _ = app.emit("usage-settings-changed", &s);
            }
        }
        (OVERLAY, WindowEvent::Destroyed) => {
            // It may vanish without telling the watcher it is gone.
            let app2 = app.clone();
            std::thread::spawn(move || {
                let state = app2.state::<UsageState>().inner().clone();
                let watch = app2.state::<usage_prefs::Watch>().inner().clone();
                let _ = usage_prefs::set_watch(app2.clone(), state, watch, OVERLAY, false);
            });
        }
        (OVERLAY, WindowEvent::Moved(p)) => {
            let live = app.state::<Live>();
            let n = {
                let mut g = lock(&live.inner);
                g.pos_gen += 1;
                g.pos_gen
            };
            let pos = Pos { x: p.x, y: p.y };
            let app2 = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(POS_SAVE_DELAY);
                if lock(&app2.state::<Live>().inner).pos_gen == n {
                    if let Ok(b) = serde_json::to_vec(&pos) {
                        let _ = crate::store::atomic_write(&pos_path(), &b);
                    }
                }
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const B1: &str = "2026-11-19T08:00:00Z";
    const B2: &str = "2026-11-19T13:30:00Z";

    #[test]
    fn alerts_fire_once_per_level_per_block() {
        let none = Notified::default();
        assert_eq!(alert_due(&none, B1, 900, None), None, "no limit, no alert");
        assert_eq!(alert_due(&none, B1, 900, Some(0)), None, "zero limit counts as none");
        assert_eq!(alert_due(&none, B1, 799, Some(1000)), None);
        let (lvl, s80) = alert_due(&none, B1, 800, Some(1000)).unwrap();
        assert_eq!((lvl, s80.level, s80.block.as_str()), (80, 80, B1));
        assert_eq!(alert_due(&s80, B1, 950, Some(1000)), None, "80% only once");
        let (lvl, s100) = alert_due(&s80, B1, 1000, Some(1000)).unwrap();
        assert_eq!(lvl, 100);
        assert_eq!(alert_due(&s100, B1, 5000, Some(1000)), None, "100% only once");
        // A new block starts over.
        assert_eq!(alert_due(&s100, B2, 10, Some(1000)), None);
        assert_eq!(alert_due(&s100, B2, 850, Some(1000)).unwrap().0, 80);
    }

    #[test]
    fn jump_past_both_sends_only_the_100_alert() {
        let (lvl, s) = alert_due(&Notified::default(), B1, 1200, Some(1000)).unwrap();
        assert_eq!((lvl, s.level), (100, 100));
        assert_eq!(alert_due(&s, B1, 1300, Some(1000)), None, "the skipped 80% alert is not sent later");
    }

    #[test]
    fn alert_state_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(notified_get_in(dir.path()), Notified::default());
        let (_, s) = alert_due(&notified_get_in(dir.path()), B1, 820, Some(1000)).unwrap();
        notified_set_in(dir.path(), &s).unwrap();
        let reloaded = notified_get_in(dir.path());
        assert_eq!(reloaded, s);
        assert_eq!(alert_due(&reloaded, B1, 830, Some(1000)), None);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(alert_due(&Notified::default(), B1, u64::MAX, Some(u64::MAX)).unwrap().0, 100);
        assert_eq!(alert_due(&Notified::default(), B1, u64::MAX / 10 * 9, Some(u64::MAX)).unwrap().0, 80);
    }

    #[test]
    fn tooltip_reads_well() {
        assert_eq!(tooltip_text(1_234_000_000, Some(620), Some(1000)), "Gateswitch — today 1.2B tok · block 62%");
        assert_eq!(tooltip_text(950, Some(12_000_000), None), "Gateswitch — today 950 tok · block 12.0M tok");
        assert_eq!(tooltip_text(0, None, Some(1000)), "Gateswitch — today 0 tok · no active block");
    }

    #[test]
    fn overlay_position_must_be_reachable() {
        let mons = [(0, 0, 1920, 1080), (1920, 0, 1280, 1024)];
        assert!(on_screen(&mons, 100, 100, 260));
        assert!(on_screen(&mons, 1800, 50, 260), "across two monitors");
        assert!(!on_screen(&mons, 4000, 100, 260), "monitor unplugged");
        assert!(!on_screen(&mons, 100, -200, 260), "title strip above the screen");
        assert!(!on_screen(&[], 0, 0, 260));
    }
}
