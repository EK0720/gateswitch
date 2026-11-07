//! Orca ships its own Codex runtime with a private CODEX_HOME at
//! <Orca user data>/codex-runtime-home/home. Orca's user data folder (from its
//! app.asar): ORCA_USER_DATA_PATH if set, else %APPDATA%/orca on Windows,
//! ~/Library/Application Support/orca on macOS, $XDG_CONFIG_HOME/orca or
//! ~/.config/orca on Linux. Orca itself mirrors the user's
//! ~/.codex/config.toml into that home (its "codex-config" sync), so the
//! gateway, model, provider table and model_catalog_json from the main Codex
//! setup reach Orca without a separate setup. Claude Code in Orca terminals uses
//! the normal ~/.claude settings and user env, so it needs nothing here.
//!
//! What this module adds: a status that says whether Orca's copy matches the
//! main config, and a "sync now" that copies the routing keys over when Orca
//! has not mirrored yet (it does so when it starts Codex). The running daemon
//! keeps the old config until restarted (restart.rs).

use crate::codex;
use serde::Serialize;
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item};

/// Keys that decide which gateway/model Codex uses.
const ROUTING: &[&str] = &["model", "model_provider", "model_reasoning_effort", "model_catalog_json"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Status {
    /// The separate home's config.toml exists.
    pub config: bool,
    /// A bundled codex.exe runtime was found.
    pub runtime: Option<String>,
    /// Catalog file Orca's config points at, if any.
    pub catalog: Option<String>,
    /// Orca's provider/model, as Orca's config says.
    pub provider: String,
    pub model: String,
    /// Orca's copy has the same routing as ~/.codex/config.toml.
    pub in_sync: bool,
}

/// Orca's user data folder, resolved the way Orca does it. `dirs::config_dir`
/// gives %APPDATA%, ~/Library/Application Support or $XDG_CONFIG_HOME|~/.config.
pub fn user_data() -> Option<PathBuf> {
    match std::env::var_os("ORCA_USER_DATA_PATH") {
        Some(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => Some(dirs::config_dir()?.join("orca")),
    }
}

pub fn home() -> Option<PathBuf> {
    Some(user_data()?.join("codex-runtime-home").join("home"))
}

pub fn config_path() -> Option<PathBuf> {
    home().map(|h| h.join("config.toml"))
}

/// Newest bundled codex runtime under packages/app-server-daemon/releases.
pub fn runtime() -> Option<PathBuf> {
    let releases = home()?.join("packages").join("app-server-daemon").join("releases");
    let bin = if cfg!(windows) { "codex.exe" } else { "codex" };
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for e in std::fs::read_dir(&releases).ok()?.flatten() {
        let exe = e.path().join("bin").join(bin);
        if exe.is_file() {
            let v = version_key(&e.file_name().to_string_lossy());
            if best.as_ref().map_or(true, |(b, _)| v > *b) {
                best = Some((v, exe));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Version sort, not name sort: "0.55.0" > "0.9.0".
fn version_key(name: &str) -> Vec<u64> {
    name.trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse().unwrap_or(0)).collect()
}

fn routing(doc: &DocumentMut) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = ROUTING
        .iter()
        .map(|k| (k.to_string(), doc.get(k).map(|i| i.to_string().trim().to_string()).unwrap_or_default()))
        .collect();
    let provider = doc.get("model_provider").and_then(|i| i.as_str()).unwrap_or("");
    let table = doc.get("model_providers").and_then(|t| t.get(provider));
    for k in ["base_url", "env_key", "wire_api"] {
        let v = table.and_then(|t| t.get(k)).map(|i| i.to_string().trim().to_string()).unwrap_or_default();
        out.push((format!("provider.{k}"), v));
    }
    out
}

/// Same gateway, model, effort, catalog and provider table in both files.
pub fn same_routing(main: &str, orca: &str) -> bool {
    let a: DocumentMut = main.parse().unwrap_or_default();
    let b: DocumentMut = orca.parse().unwrap_or_default();
    routing(&a) == routing(&b)
}

pub fn status() -> Status {
    let cfg = config_path().filter(|p| p.is_file());
    let text = cfg.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    let doc: DocumentMut = text.parse().unwrap_or_default();
    let cur = codex::read_current(&text);
    let main = std::fs::read_to_string(codex::config_path()).unwrap_or_default();
    Status {
        config: cfg.is_some(),
        runtime: runtime().map(|p| p.display().to_string()),
        catalog: doc.get("model_catalog_json").and_then(|i| i.as_str()).map(String::from).filter(|s| !s.is_empty()),
        in_sync: cfg.is_some() && same_routing(&main, &text),
        provider: cur.provider,
        model: cur.model,
    }
}

/// Copy the routing keys and the active provider table from the main config
/// into Orca's, keeping everything else in Orca's file (projects, hooks…).
pub fn sync_text(main: &str, orca: &str) -> Result<String, String> {
    let src: DocumentMut = main.parse().map_err(|e| format!("~/.codex/config.toml has a syntax error: {e}"))?;
    let mut dst: DocumentMut = orca.parse().map_err(|e| format!("Orca's config.toml has a syntax error: {e}"))?;
    let provider = src.get("model_provider").and_then(|i| i.as_str()).unwrap_or("").to_string();
    if provider.is_empty() {
        return Err("Main Codex has no gateway configured. Set it up on the Codex page first.".into());
    }
    for k in ROUTING {
        match src.get(k) {
            Some(v) => dst[*k] = v.clone(),
            None => {
                dst.remove(k);
            }
        }
    }
    if let Some(table) = src.get("model_providers").and_then(|t| t.get(&provider)).cloned() {
        if !dst.contains_key("model_providers") {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            dst["model_providers"] = Item::Table(t);
        }
        let ps = dst["model_providers"].as_table_mut().ok_or("model_providers must be a table")?;
        ps.insert(&provider, table);
    }
    Ok(dst.to_string())
}

/// Path and new text for "sync now".
pub fn sync() -> Result<(PathBuf, String), String> {
    let path = config_path().ok_or("Orca is only available on Windows")?;
    if !path.is_file() {
        return Err("Orca has no Codex configuration yet. Open Codex in Orca once first.".into());
    }
    let main_path = codex::config_path();
    if std::fs::canonicalize(&main_path).ok() == std::fs::canonicalize(&path).ok() {
        return Err("Main Codex and Orca are using the same file".into());
    }
    let main = std::fs::read_to_string(&main_path).map_err(|_| "~/.codex/config.toml does not exist. Set it up on the Codex page first.".to_string())?;
    let before = std::fs::read_to_string(&path).unwrap_or_default();
    Ok((path, sync_text(&main, &before)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_key_sorts_numerically() {
        assert!(version_key("0.55.0") > version_key("0.9.0"));
        assert!(version_key("v1.2.10") > version_key("1.2.9"));
    }

    #[test]
    fn home_is_under_orca_user_data() {
        let h = home().unwrap();
        assert!(h.ends_with(std::path::Path::new("codex-runtime-home").join("home")));
        assert_eq!(h.parent().unwrap().parent().unwrap(), user_data().unwrap());
    }

    #[test]
    fn sync_copies_routing_and_keeps_orca_extras() {
        let main = "model = \"cx/a\"
model_provider = \"gw\"
model_catalog_json = '/c.json'

[model_providers.gw]
base_url = \"https://g/v1\"
env_key = \"GW_API_KEY\"
";
        let orca = "model = \"old\"
model_reasoning_effort = \"high\"

[projects.'C:/x']
trust_level = \"trusted\"
";
        assert!(!same_routing(main, orca));
        let out = sync_text(main, orca).unwrap();
        assert!(same_routing(main, &out));
        assert!(out.contains("trust_level"));
        assert!(!out.contains("model_reasoning_effort"));
        assert!(sync_text("", orca).is_err());
    }

    #[test]
    fn status_without_orca_is_empty() {
        // On CI (no %APPDATA%/orca) this must not panic and must report missing.
        let s = status();
        if home().is_none() {
            assert!(!s.config);
            assert!(s.runtime.is_none());
        }
    }
}
