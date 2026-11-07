//! restart.log: one line per restart step, so a restart that fails on a
//! user's machine can be diagnosed from the support report.
//!
//! Lines never contain keys (the restart code has none) and the home folder is
//! replaced by `~`. The file is capped: past 256 KB the older half is dropped.

use std::io::Write;
use std::path::PathBuf;

const MAX: u64 = 256 * 1024;

pub fn path() -> PathBuf {
    crate::data_dir().join("logs").join("restart.log")
}

pub fn log(app: &str, msg: &str) {
    let p = path();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(&p));
    if std::fs::metadata(&p).map(|m| m.len() > MAX).unwrap_or(false) {
        if let Ok(text) = std::fs::read_to_string(&p) {
            let mut cut = text.len() / 2;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            let keep = text[cut..].split_once('\n').map(|(_, r)| r).unwrap_or("");
            let _ = std::fs::write(&p, keep);
        }
    }
    let line = format!("{} [{app}] {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), crate::report::scrub(msg, &[]));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Last `n` lines, for the support report.
pub fn tail(n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}

/// Log the outcome of a restart and pass it through.
pub fn done(app: &str, r: Result<String, String>) -> Result<String, String> {
    match &r {
        Ok(m) => log(app, &format!("OK: {m}")),
        Err(e) => log(app, &format!("FAILED: {e}")),
    }
    r
}
