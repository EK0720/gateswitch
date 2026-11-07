//! Restart Codex Desktop and Orca's Codex daemon so they pick up a new setup.
//!
//! Ported from the Python app (services.restart_codex_desktop, orca_restart.py).
//! Both only touch processes we can positively identify by executable path,
//! and the UI asks the user before calling either.

/// Error prefix the UI uses to offer "force quit" instead of a plain error.
pub const STILL_RUNNING: &str = "Codex is still running in the background";

#[cfg(not(windows))]
pub fn codex_desktop(force: bool) -> Result<String, String> {
    crate::restart_unix::restart(&crate::restart_unix::CODEX, force)
}

#[cfg(not(windows))]
pub fn orca_daemon() -> Result<String, String> {
    Err("Restarting the Orca daemon is only supported on Windows for now".into())
}

#[cfg(not(windows))]
pub fn claude_desktop() -> Result<String, String> {
    crate::restart_unix::restart(&crate::restart_unix::CLAUDE, false)
}

#[cfg(windows)]
pub use win::{claude_desktop, codex_desktop, orca_daemon};

/// Claude Desktop's own executable, not the Claude Code CLI (also claude.exe,
/// usually under ~/.local/bin or npm). Store build: WindowsApps\Claude_<id>\app\claude.exe;
/// plain installer: %LOCALAPPDATA%\AnthropicClaude\app-<ver>\claude.exe.
pub fn is_claude_desktop_exe(path_lc: &str) -> bool {
    path_lc.ends_with("\\claude.exe") && (path_lc.contains("\\windowsapps\\claude_") || path_lc.contains("\\anthropicclaude\\"))
}

/// `app-server` as a whole argument; the install path itself contains
/// `app-server-daemon`, which must not match.
pub fn has_app_server_arg(cmd: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(i) = cmd[from..].find("app-server") {
        let start = from + i;
        let end = start + "app-server".len();
        let before = cmd[..start].chars().next_back();
        let after = cmd[end..].chars().next();
        if !before.is_some_and(|c| word(c) || c == '\\' || c == '/') && !after.is_some_and(word) {
            return true;
        }
        from = end;
    }
    false
}

/// Model slugs listed in a catalog file, used to check the daemon loaded it.
pub fn catalog_slugs(text: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("models").and_then(|m| m.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("slug").and_then(|s| s.as_str()).map(String::from))
        .collect()
}

#[cfg(windows)]
mod win {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashSet;
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    const NO_WINDOW: u32 = 0x0800_0000;

    #[derive(Debug, Clone, Deserialize)]
    struct Proc {
        #[serde(rename = "ProcessId")]
        pid: u32,
        #[serde(rename = "ParentProcessId", default)]
        ppid: u32,
        #[serde(rename = "ExecutablePath", default)]
        path: Option<String>,
        #[serde(rename = "CommandLine", default)]
        cmd: Option<String>,
        /// FILETIME (UTC, 100 ns), so it can be compared with Orca's pid file.
        #[serde(rename = "C", default)]
        created: u64,
    }

    impl Proc {
        fn path_lc(&self) -> String {
            self.path.as_deref().unwrap_or("").replace('/', "\\").to_lowercase()
        }
        fn cmd(&self) -> &str {
            self.cmd.as_deref().unwrap_or("")
        }
    }

    /// All processes with path, command line and start time. Uses CIM, which
    /// ships with every Windows 10/11, so no extra native dependency.
    fn snapshot() -> Result<Vec<Proc>, String> {
        let script = "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine,\
                      @{n='C';e={if($_.CreationDate){$_.CreationDate.ToFileTimeUtc()}else{0}}} | ConvertTo-Json -Compress";
        let out = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(NO_WINDOW)
            .output()
            .map_err(|e| format!("Could not read the process list: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        serde_json::from_str(text.trim()).map_err(|_| "Could not read the process list".to_string())
    }

    fn descendants(procs: &[Proc], roots: &[u32]) -> Vec<Proc> {
        let mut keep: HashSet<u32> = roots.iter().copied().collect();
        loop {
            let before = keep.len();
            for p in procs {
                if keep.contains(&p.ppid) && p.pid != p.ppid {
                    keep.insert(p.pid);
                }
            }
            if keep.len() == before {
                break;
            }
        }
        procs.iter().filter(|p| keep.contains(&p.pid)).cloned().collect()
    }

    /// Kill exactly these processes. Re-checks pid + start time first so a
    /// reused PID is never touched.
    fn terminate(targets: &[Proc]) -> Result<usize, String> {
        if targets.is_empty() {
            return Ok(0);
        }
        let now = snapshot()?;
        let alive: Vec<u32> = targets
            .iter()
            .filter(|t| now.iter().any(|p| p.pid == t.pid && p.created == t.created))
            .map(|t| t.pid)
            .collect();
        if alive.is_empty() {
            return Ok(0);
        }
        let mut cmd = Command::new("taskkill");
        cmd.arg("/F");
        for pid in &alive {
            cmd.args(["/PID", &pid.to_string()]);
        }
        let _ = cmd.creation_flags(NO_WINDOW).output();
        Ok(alive.len())
    }

    fn wait_gone(pids: &HashSet<u32>, secs: u64) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            match snapshot() {
                Ok(now) if !now.iter().any(|p| pids.contains(&p.pid)) => return true,
                _ => std::thread::sleep(Duration::from_millis(400)),
            }
        }
        false
    }

    /// Ask the app to quit like the user clicking X: WM_CLOSE to its visible
    /// top-level windows.
    fn close_windows(pids: &HashSet<u32>) {
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
        };
        unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> i32 {
            let pids = unsafe { &*(lp as *const HashSet<u32>) };
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pids.contains(&pid) && unsafe { IsWindowVisible(hwnd) } != 0 {
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            }
            1
        }
        unsafe { EnumWindows(Some(each), pids as *const _ as LPARAM) };
    }

    // ------------------------------------------------------------ Codex Desktop

    fn codex_roots(procs: &[Proc]) -> (Vec<Proc>, Vec<Proc>) {
        let mains: Vec<&Proc> = procs
            .iter()
            .filter(|p| {
                let path = p.path_lc();
                path.ends_with("\\chatgpt.exe") && path.contains("\\windowsapps\\openai.codex_") && !p.cmd().contains("--type=")
            })
            .collect();
        let main_pids: HashSet<u32> = mains.iter().map(|p| p.pid).collect();
        // Electron helpers are children of the main process; keep real roots only.
        let roots: Vec<Proc> = mains.into_iter().filter(|p| !main_pids.contains(&p.ppid)).cloned().collect();
        let tree = descendants(procs, &roots.iter().map(|p| p.pid).collect::<Vec<_>>());
        (roots, tree)
    }

    /// Soft close first; leftover children (app-server, node) of an app that
    /// already quit are killed since they hold locks that hang the next launch.
    /// The main window itself is only killed when `force`.
    pub fn codex_desktop(force: bool) -> Result<String, String> {
        let family = crate::detect::codex_desktop_family().ok_or("Codex Desktop (Microsoft Store version) not found. Nothing was closed.")?;
        let (roots, tree) = codex_roots(&snapshot()?);
        crate::rlog::log("Codex Desktop", &format!("found {} root(s), {} process(es); force={force}", roots.len(), tree.len()));
        if !roots.is_empty() {
            let root_pids: HashSet<u32> = roots.iter().map(|p| p.pid).collect();
            if force {
                terminate(&tree)?;
            } else {
                close_windows(&tree.iter().map(|p| p.pid).collect());
            }
            if !wait_gone(&root_pids, 10) {
                return Err(if force {
                    "Codex is still running after force quit. Not reopened.".into()
                } else {
                    format!("{STILL_RUNNING} (system tray). You can force quit.")
                });
            }
            terminate(&tree)?;
            wait_gone(&tree.iter().map(|p| p.pid).collect(), 5);
        }
        // MSIX apps must be activated through the shell to get their identity.
        // explorer.exe already has the new user env from the setup broadcast.
        crate::rlog::log("Codex Desktop", &format!("closed; launching {family}"));
        Command::new("explorer.exe")
            .arg(format!(r"shell:AppsFolder\{}", crate::detect::codex_desktop_appid(&family)))
            .spawn()
            .map_err(|e| format!("Could not reopen Codex: {e}"))?;
        let end = Instant::now() + Duration::from_secs(25);
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(600));
            if snapshot().map(|p| !codex_roots(&p).0.is_empty()).unwrap_or(false) {
                return Ok(if roots.is_empty() { "Opened Codex Desktop".into() } else { "Restarted Codex Desktop".into() });
            }
        }
        Err("Closed Codex but it has not reopened. Open the app manually.".into())
    }

    // ------------------------------------------------------------ Claude Desktop

    fn claude_roots(procs: &[Proc]) -> (Vec<Proc>, Vec<Proc>) {
        let mains: Vec<&Proc> = procs
            .iter()
            .filter(|p| super::is_claude_desktop_exe(&p.path_lc()) && !p.cmd().contains("--type="))
            .collect();
        let main_pids: HashSet<u32> = mains.iter().map(|p| p.pid).collect();
        let roots: Vec<Proc> = mains.into_iter().filter(|p| !main_pids.contains(&p.ppid)).cloned().collect();
        let tree = descendants(procs, &roots.iter().map(|p| p.pid).collect::<Vec<_>>());
        (roots, tree)
    }

    /// Close Claude Desktop (window close first, then its own process tree
    /// only) and open it again. Never touches the Claude Code CLI.
    pub fn claude_desktop() -> Result<String, String> {
        let (roots, tree) = claude_roots(&snapshot()?);
        crate::rlog::log("Claude Desktop", &format!("found {} root(s), {} process(es)", roots.len(), tree.len()));
        if !roots.is_empty() {
            let root_pids: HashSet<u32> = roots.iter().map(|p| p.pid).collect();
            close_windows(&tree.iter().map(|p| p.pid).collect());
            // Claude Desktop keeps running in the tray after its window closes.
            if !wait_gone(&root_pids, 6) {
                crate::rlog::log("Claude Desktop", "still running after window close (tray); ending its processes");
                terminate(&tree)?;
                if !wait_gone(&root_pids, 8) {
                    return Err("Claude Desktop is still running. Quit it from the system tray, then open it again.".into());
                }
            }
            terminate(&tree)?;
            wait_gone(&tree.iter().map(|p| p.pid).collect(), 5);
        }
        let family = crate::detect::claude_desktop_family();
        crate::rlog::log("Claude Desktop", &format!("closed; launching {}", family.as_deref().unwrap_or("non-Store install")));
        let launched = match family {
            Some(family) => Command::new("explorer.exe")
                .arg(format!(r"shell:AppsFolder\{}", crate::detect::claude_desktop_appid(&family)))
                .spawn()
                .is_ok(),
            None => roots
                .first()
                .and_then(|r| r.path.clone())
                .map(|exe| Command::new(exe).creation_flags(NO_WINDOW).spawn().is_ok())
                .unwrap_or(false),
        };
        if !launched {
            return Ok("Closed Claude Desktop. Open it again to load the configuration.".into());
        }
        let end = Instant::now() + Duration::from_secs(25);
        while Instant::now() < end {
            std::thread::sleep(Duration::from_millis(600));
            if snapshot().map(|p| !claude_roots(&p).0.is_empty()).unwrap_or(false) {
                return Ok(if roots.is_empty() { "Opened Claude Desktop".into() } else { "Restarted Claude Desktop".into() });
            }
        }
        Err("Closed Claude Desktop but it has not reopened. Open the app manually.".into())
    }

    // ------------------------------------------------------------ Orca daemon

    fn daemon(cli: &Path, home: &Path, action: &str) -> bool {
        let Ok(mut child) = Command::new(cli)
            .args(["app-server", "daemon", action])
            .env("CODEX_HOME", home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
        else {
            return false;
        };
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            if let Ok(Some(st)) = child.try_wait() {
                return st.success();
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.kill();
        false
    }

    /// Ask the daemon for its models over `app-server proxy` (JSON-RPC lines).
    fn model_list(cli: &Path, home: &Path) -> Result<HashSet<String>, String> {
        let mut child = Command::new(cli)
            .args(["app-server", "proxy"])
            .env("CODEX_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Could not run Orca's codex.exe: {e}"))?;
        let stdout = child.stdout.take().ok_or("Could not read from the daemon")?;
        let (tx, rx) = std::sync::mpsc::channel::<serde_json::Value>();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        let stdin = child.stdin.take().ok_or("Could not write to the daemon")?;
        let mut rpc = Rpc { stdin, rx, end: Instant::now() + Duration::from_secs(12) };
        let result = (|| {
            let v = env!("CARGO_PKG_VERSION");
            rpc.call(1, "initialize", serde_json::json!({ "clientInfo": { "name": "model-switcher", "version": v } }))?;
            rpc.send(serde_json::json!({ "method": "initialized" }))?;
            let mut out = HashSet::new();
            let mut cursor: Option<String> = None;
            for page_no in 0..40u64 {
                let mut params = serde_json::json!({ "limit": 100 });
                if let Some(c) = &cursor {
                    params["cursor"] = serde_json::json!(c);
                }
                let page = rpc.call(2 + page_no, "model/list", params)?;
                for m in page.get("data").and_then(|d| d.as_array()).into_iter().flatten() {
                    if let Some(s) = m.get("model").and_then(|s| s.as_str()) {
                        out.insert(s.to_string());
                    }
                }
                cursor = page.get("nextCursor").and_then(|c| c.as_str()).map(String::from);
                if cursor.is_none() {
                    return Ok(out);
                }
            }
            Err("The model list exceeds the check limit".to_string())
        })();
        let _ = child.kill();
        let _ = child.wait();
        result
    }

    struct Rpc {
        stdin: std::process::ChildStdin,
        rx: std::sync::mpsc::Receiver<serde_json::Value>,
        end: Instant,
    }

    impl Rpc {
        fn send(&mut self, msg: serde_json::Value) -> Result<(), String> {
            writeln!(self.stdin, "{msg}").and_then(|_| self.stdin.flush()).map_err(|_| "The daemon closed the check connection".to_string())
        }
        fn call(&mut self, id: u64, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
            self.send(serde_json::json!({ "id": id, "method": method, "params": params }))?;
            loop {
                let left = self.end.saturating_duration_since(Instant::now());
                let v = self.rx.recv_timeout(left).map_err(|e| match e {
                    // The proxy exits at once when nothing listens on the control socket.
                    std::sync::mpsc::RecvTimeoutError::Disconnected => "The daemon is not running or not accepting connections".to_string(),
                    std::sync::mpsc::RecvTimeoutError::Timeout => "The daemon did not respond in time".to_string(),
                })?;
                if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                    if v.get("error").is_some() {
                        return Err("The daemon refused the model list check".into());
                    }
                    return Ok(v.get("result").cloned().unwrap_or_default());
                }
            }
        }
    }

    fn pid_file(path: &Path) -> Option<(u32, u64)> {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        let pid = v.get("pid")?.as_u64()? as u32;
        let start = match v.get("processStartTime") {
            Some(serde_json::Value::String(s)) => s.parse().unwrap_or(0),
            Some(n) => n.as_u64().unwrap_or(0),
            None => 0,
        };
        Some((pid, start))
    }

    /// Kill Orca's daemon trees. Roots must run Orca's own codex.exe.
    fn stop_daemon(home: &Path) -> Result<(), String> {
        let procs = snapshot()?;
        let releases = home.join("packages").join("app-server-daemon").join("releases").display().to_string().to_lowercase();
        let ours = |p: &Proc| p.path_lc().starts_with(&releases);
        let mut roots: Vec<u32> = procs.iter().filter(|p| ours(p) && has_app_server_arg(p.cmd())).map(|p| p.pid).collect();
        for name in ["daemon.pid", "daemon-updater.pid"] {
            if let Some((pid, start)) = pid_file(&home.join("app-server-daemon").join(name)) {
                // CIM start times are rounded to microseconds; allow 1 ms.
                if let Some(p) = procs.iter().find(|p| p.pid == pid && ours(p) && (start == 0 || p.created.abs_diff(start) < 10_000)) {
                    roots.push(p.pid);
                }
            }
        }
        if roots.is_empty() {
            return Ok(());
        }
        let tree = descendants(&procs, &roots);
        terminate(&tree)?;
        if !wait_gone(&roots.into_iter().collect(), 8) {
            return Err("The Orca daemon is still running after stopping".into());
        }
        Ok(())
    }

    /// A stale socket/lock makes a fresh daemon fail to bind. AF_UNIX sockets
    /// are reparse points that `exists()` misreports, so always try to remove.
    fn clear_stale_control(home: &Path) -> Result<(), String> {
        let control = home.join("app-server-control");
        for name in ["app-server-control.sock", "app-server-startup.lock"] {
            let p = control.join(name);
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => {
                    let to = control.join(format!("{name}.stale-{}", chrono::Local::now().timestamp()));
                    match std::fs::rename(&p, &to) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(format!("Could not delete Orca's old {name}: {e}")),
                    }
                }
            }
        }
        // Older versions renamed the whole folder each time; keep the newest one.
        let mut backups: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(home)
            .map(|r| {
                r.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("app-server-control-backup-"))
                    .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path()))
                    .collect()
            })
            .unwrap_or_default();
        backups.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, old) in backups.into_iter().skip(1) {
            let _ = std::fs::remove_dir_all(old);
        }
        Ok(())
    }

    /// Read-only check against the running daemon: `cargo test -- --ignored orca_models_live`.
    #[test]
    #[ignore]
    fn orca_models_live() {
        let (home, cli) = (crate::orca::home().unwrap(), crate::orca::runtime().unwrap());
        let models = model_list(&cli, &home).unwrap();
        println!("{} models", models.len());
        assert!(!models.is_empty());
    }

    pub fn orca_daemon() -> Result<String, String> {
        let home = crate::orca::home().ok_or("Orca not found")?;
        let cli = crate::orca::runtime().ok_or("codex.exe bundled with Orca not found")?;
        let config = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
        let doc: toml_edit::DocumentMut = config.parse().unwrap_or_default();
        let expected: Vec<String> = doc
            .get("model_catalog_json")
            .and_then(|i| i.as_str())
            .map(|p| {
                let p = PathBuf::from(p);
                if p.is_absolute() { p } else { home.join(p) }
            })
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| catalog_slugs(&t))
            .unwrap_or_default();

        let restarted = daemon(&cli, &home, "restart");
        let mut actual = if restarted { model_list(&cli, &home).ok() } else { None };
        crate::rlog::log("Orca", &format!("daemon restart ok={restarted}, model list ok={}", actual.is_some()));
        if actual.is_none() {
            crate::rlog::log("Orca", "stopping the daemon and clearing the stale socket");
            // Normal restart failed: stop exactly Orca's daemon, clear the stale socket, start fresh.
            stop_daemon(&home)?;
            clear_stale_control(&home)?;
            daemon(&cli, &home, "start");
            actual = Some(model_list(&cli, &home).map_err(|e| {
                format!("Stopped the old daemon and restarted it but could not connect ({e}). Reopen Codex in Orca.")
            })?);
        }
        let actual = actual.unwrap_or_default();
        if actual.is_empty() {
            return Err("The daemon returned an empty model list; restart could not be verified".into());
        }
        let missing = expected.iter().filter(|s| !actual.contains(*s)).count();
        if missing > 0 {
            return Err(format!("Daemon is running but {missing} catalog models are missing; not fully applied"));
        }
        Ok(format!("Restarted Codex in Orca ({} models). Open a new Codex tab if the old one does not reconnect.", actual.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_desktop_exe_is_not_the_cli() {
        assert!(is_claude_desktop_exe(r"c:\program files\windowsapps\claude_4.23118.0.0_x64__pzs8sxrjxfjjc\app\claude.exe"));
        assert!(is_claude_desktop_exe(r"c:\users\x\appdata\local\anthropicclaude\app-1.2.3\claude.exe"));
        assert!(!is_claude_desktop_exe(r"c:\users\x\.local\bin\claude.exe"));
        assert!(!is_claude_desktop_exe(r"c:\users\x\appdata\roaming\npm\node_modules\@anthropic-ai\claude-code\claude.exe"));
    }

    #[test]
    fn app_server_arg_is_a_whole_word() {
        let exe = r"C:\Users\x\AppData\Roaming\orca\codex-runtime-home\home\packages\app-server-daemon\releases\1\bin\codex.exe";
        assert!(has_app_server_arg(&format!("\"{exe}\" app-server --listen")));
        assert!(!has_app_server_arg(&format!("\"{exe}\" mcp")));
        assert!(!has_app_server_arg("codex app-server-daemon"));
    }

    #[test]
    fn slugs_from_catalog() {
        assert_eq!(catalog_slugs(r#"{"models":[{"slug":"a"},{"slug":"b"}]}"#), vec!["a", "b"]);
        assert!(catalog_slugs("nope").is_empty());
    }
}
