//! Restart on macOS and Linux: ask the app to quit, wait, escalate only to that
//! app's processes, reopen, then check it is running again.
//!
//! - macOS: `osascript quit app`, then SIGTERM, then SIGKILL; reopen with `open -a`.
//! - Linux: SIGTERM, then SIGKILL; reopen the launcher found on PATH.
//!
//! Processes are matched by their executable path inside the app bundle
//! (`/Claude.app/Contents/MacOS/`), never by a bare name, so the Claude Code
//! CLI (`claude`) is not touched. Plain std code: it is compiled (not run) on
//! Windows too, so a Windows build catches mistakes in it.

#![cfg_attr(windows, allow(dead_code))]

use std::process::Command;
use std::time::{Duration, Instant};

pub struct App {
    pub label: &'static str,
    /// macOS application name for `osascript` / `open -a`.
    pub mac_name: &'static str,
    /// `pgrep -f` pattern for the app's own processes.
    pub mac_pattern: &'static str,
    /// Linux launcher on PATH and its `pgrep -f` pattern; None = unsupported.
    pub linux: Option<(&'static str, &'static str)>,
}

pub const CLAUDE: App = App {
    label: "Claude Desktop",
    mac_name: "Claude",
    mac_pattern: "/Claude.app/Contents/",
    // Community builds (claude-desktop-debian and similar); no official Linux app.
    linux: Some(("claude-desktop", "claude-desktop")),
};

pub const CODEX: App = App {
    label: "Codex Desktop",
    mac_name: "Codex",
    mac_pattern: "/Codex.app/Contents/",
    linux: None,
};

fn running(pattern: &str) -> bool {
    Command::new("pgrep").args(["-f", pattern]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn wait(pattern: &str, want_running: bool, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if running(pattern) == want_running {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn signal(sig: &str, pattern: &str) {
    let _ = Command::new("pkill").args([sig, "-f", pattern]).status();
}

pub fn restart(app: &App, force: bool) -> Result<String, String> {
    let log = |m: &str| crate::rlog::log(app.label, m);
    let mac = cfg!(target_os = "macos");
    let (pattern, launcher) = if mac {
        (app.mac_pattern, None)
    } else {
        let (bin, pat) = app.linux.ok_or(format!("Restarting {} is not supported on this operating system", app.label))?;
        (pat, Some(bin))
    };
    let was = running(pattern);
    log(&format!("running before: {was}"));
    if was {
        if mac && !force {
            let script = format!("quit app \"{}\"", app.mac_name);
            let _ = Command::new("osascript").args(["-e", &script]).status();
            log("asked to quit (osascript)");
        } else {
            signal("-TERM", pattern);
            log("sent SIGTERM");
        }
        if !wait(pattern, false, 10) {
            signal("-TERM", pattern);
            if !wait(pattern, false, 5) {
                signal("-KILL", pattern);
                log("sent SIGKILL");
                if !wait(pattern, false, 4) {
                    return Err(format!("{} is still running. Quit it, then open it again.", app.label));
                }
            }
        }
    }
    let spawned = if mac {
        Command::new("open").args(["-a", app.mac_name]).status().map(|s| s.success()).unwrap_or(false)
    } else {
        launcher.and_then(crate::detect::which).map(|p| Command::new(p).spawn().is_ok()).unwrap_or(false)
    };
    log(&format!("launch requested: {spawned}"));
    if !spawned {
        return Ok(format!("Closed {}. Open it again to load the configuration.", app.label));
    }
    if wait(pattern, true, 25) {
        Ok(if was { format!("Restarted {}", app.label) } else { format!("Opened {}", app.label) })
    } else {
        Err(format!("Closed {} but it has not reopened. Open the app manually.", app.label))
    }
}
