//! Detect coding tools and IDEs on this machine (read-only).
//!
//! IDEs are only *suggestions*: we show the ones we find so users know a setup
//! also reaches them, plus the one action each needs to pick it up.

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Tool {
    pub id: &'static str,
    pub name: &'static str,
    pub installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ide {
    pub id: &'static str,
    pub name: &'static str,
    /// Which engines can reach this IDE through our setup.
    pub uses: Vec<&'static str>,
    /// What to do after a Codex setup, if the IDE can use it.
    pub codex: Option<&'static str>,
    /// What to do after a Claude Code setup, if the IDE can use it.
    pub claude: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub os: &'static str,
    pub tools: Vec<Tool>,
    pub ides: Vec<Ide>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn first_existing(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|p| p.exists())
}

/// Look up an executable on PATH (plus PATHEXT on Windows).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or(".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn tool(id: &'static str, name: &'static str, found: Option<PathBuf>) -> Tool {
    Tool { id, name, installed: found.is_some(), path: found.map(|p| p.display().to_string()) }
}

fn local_appdata() -> PathBuf {
    dirs::data_local_dir().unwrap_or_default()
}

fn roaming() -> PathBuf {
    dirs::config_dir().unwrap_or_default()
}

fn store_package(prefix: &str) -> Option<PathBuf> {
    let dir = local_appdata().join("Packages");
    std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().map(|n| n.to_string_lossy().starts_with(prefix)).unwrap_or(false))
}

/// Package family name for the Store (MSIX) Claude Desktop, from the
/// WindowsApps path of a running install, e.g. Claude_pzs8sxrjxfjjc.
pub fn claude_desktop_family() -> Option<String> {
    store_package("Claude_")?.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// AppsFolder id (Family!AppId) to launch the Store build. The manifest of
/// Claude Desktop declares Application Id "Claude" (verified on 4.16336.0.0).
pub fn claude_desktop_appid(family: &str) -> String {
    format!("{family}!Claude")
}

/// Package family of the Store Codex Desktop, e.g. OpenAI.Codex_2p2nqsd0c76g0.
pub fn codex_desktop_family() -> Option<String> {
    store_package("OpenAI.Codex_")?.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// The Codex manifest declares Application Id "App" (checked on 26.1103.3517.0).
pub fn codex_desktop_appid(family: &str) -> String {
    format!("{family}!App")
}

fn claude_desktop_installed() -> Option<PathBuf> {
    if cfg!(windows) {
        return store_package("Claude_").or_else(|| {
            first_existing([
                local_appdata().join("AnthropicClaude"),
                local_appdata().join("Programs/Claude/Claude.exe"),
            ])
        });
    }
    if cfg!(target_os = "macos") {
        return first_existing(["/Applications/Claude.app".into(), home().join("Applications/Claude.app")]);
    }
    which("claude-desktop")
}

fn codex_desktop_installed() -> Option<PathBuf> {
    if cfg!(windows) {
        return store_package("OpenAI.Codex_");
    }
    if cfg!(target_os = "macos") {
        return first_existing(["/Applications/Codex.app".into(), "/Applications/ChatGPT.app".into()]);
    }
    None
}

fn orca_installed() -> Option<PathBuf> {
    if cfg!(windows) {
        return first_existing([local_appdata().join("Programs/orca/Orca.exe")]);
    }
    if cfg!(target_os = "macos") {
        let home = dirs::home_dir().unwrap_or_default();
        return first_existing(["/Applications/Orca.app".into(), home.join("Applications/Orca.app")]);
    }
    // Linux: .deb installs /opt/Orca with a launcher on PATH; an AppImage
    // leaves only the user data folder behind.
    which("orca").or_else(|| first_existing(["/opt/Orca/orca".into()])).or_else(|| crate::orca::user_data().filter(|p| p.is_dir()))
}

/// IDE catalogue. All entries read the shared CLI config (~/.codex/config.toml,
/// ~/.claude/settings.json) written by this app.
/// - VS Code / Antigravity: official Codex and Claude Code extensions.
/// - JetBrains: Claude Agent and Codex in AI Chat (confirmed working through a
///   gateway by the user, Nov 2026).
/// - Visual Studio: no official extension; community extensions wrap the CLI
///   (marketplace: dliedke.ClaudeCodeExtension, firish.bridgev1).
/// - Zed: Codex as an external agent reads ~/.codex/config.toml.
/// Cursor is excluded: it uses its own model configuration.
struct IdeDef {
    id: &'static str,
    name: &'static str,
    codex: Option<&'static str>,
    claude: Option<&'static str>,
    win: &'static [&'static str],
    mac: &'static [&'static str],
    linux: &'static [&'static str],
}

const VSC_CODEX: &str = "Install the Codex (OpenAI) extension to use it.";
const VSC_CLAUDE: &str = "Install the Claude Code (Anthropic) extension to use it.";
const JB_CODEX: &str = "Choose Codex in AI Chat.";
const JB_CLAUDE: &str = "Choose Claude Agent in AI Chat.";
const VS_CODEX: &str = "No official extension yet. Use the community extension \"Claude Code Extension for Visual Studio\" (supports Codex) or run codex in a terminal.";
const VS_CLAUDE: &str = "No official extension yet. Use the community extension \"Claude Code Extension for Visual Studio\" or run claude in a terminal.";
const ZED_CODEX: &str = "Install Codex under Agent Settings › External Agents.";

const IDES: &[IdeDef] = &[
    IdeDef { id: "vscode", name: "VS Code", codex: Some(VSC_CODEX), claude: Some(VSC_CLAUDE),
        win: &["%R/Code"], mac: &["~/Library/Application Support/Code"], linux: &["~/.config/Code"] },
    IdeDef { id: "antigravity", name: "Antigravity", codex: Some(VSC_CODEX), claude: Some(VSC_CLAUDE),
        win: &["%R/Antigravity"], mac: &["~/Library/Application Support/Antigravity"], linux: &["~/.config/Antigravity"] },
    IdeDef { id: "jetbrains", name: "JetBrains (Rider, IntelliJ…)", codex: Some(JB_CODEX), claude: Some(JB_CLAUDE),
        win: &["%R/JetBrains"], mac: &["~/Library/Application Support/JetBrains"], linux: &["~/.config/JetBrains"] },
    IdeDef { id: "visualstudio", name: "Visual Studio", codex: Some(VS_CODEX), claude: Some(VS_CLAUDE),
        win: &["C:/Program Files/Microsoft Visual Studio"], mac: &[], linux: &[] },
    IdeDef { id: "zed", name: "Zed", codex: Some(ZED_CODEX), claude: None,
        win: &["%L/Zed"], mac: &["~/Library/Application Support/Zed"], linux: &["~/.config/zed"] },
];

fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("%R/") {
        roaming().join(rest)
    } else if let Some(rest) = p.strip_prefix("%L/") {
        local_appdata().join(rest)
    } else if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(p)
    }
}

pub fn ides_in(paths_exist: impl Fn(&Path) -> bool) -> Vec<Ide> {
    IDES.iter()
        .filter(|d| {
            let list = if cfg!(windows) { d.win } else if cfg!(target_os = "macos") { d.mac } else { d.linux };
            list.iter().any(|p| paths_exist(&expand(p)))
        })
        .map(|d| Ide {
            id: d.id,
            name: d.name,
            uses: [("codex", d.codex), ("claude", d.claude)].into_iter().filter(|(_, t)| t.is_some()).map(|(e, _)| e).collect(),
            codex: d.codex,
            claude: d.claude,
        })
        .collect()
}

pub fn scan() -> Scan {
    let os = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };
    let npm_bin = |n: &str| roaming().join("npm").join(format!("{n}.cmd"));
    Scan {
        os,
        tools: vec![
            tool("codex-cli", "Codex CLI", which("codex").or_else(|| first_existing([npm_bin("codex")]))),
            tool("codex-desktop", "Codex Desktop", codex_desktop_installed()),
            tool("claude-cli", "Claude Code CLI", which("claude").or_else(|| {
                first_existing([home().join(".local/bin/claude.exe"), home().join(".local/bin/claude"), npm_bin("claude")])
            })),
            tool("claude-desktop", "Claude Desktop", claude_desktop_installed()),
            tool("orca", "Orca", orca_installed()),
        ],
        ides: ides_in(|p| p.exists()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_found_ides_are_suggested() {
        let found = ides_in(|p| p.to_string_lossy().contains("Antigravity") || p.to_string_lossy().contains("JetBrains"));
        let ids: Vec<_> = found.iter().map(|i| i.id).collect();
        assert!(ids.contains(&"antigravity"));
        assert!(ids.contains(&"jetbrains"));
        assert!(!ids.contains(&"vscode"));
        // Cursor uses its own model config, not the Codex/Claude CLI settings.
        assert!(!IDES.iter().any(|d| d.id == "cursor"));
        let zed = ides_in(|p| p.to_string_lossy().to_lowercase().contains("zed"));
        assert_eq!(zed[0].uses, vec!["codex"]);
    }

    #[test]
    fn scan_runs() {
        let s = scan();
        assert_eq!(s.tools.len(), 5);
    }
}

#[cfg(test)]
mod print {
    #[test]
    #[ignore]
    fn print_scan() {
        println!("{}", serde_json::to_string_pretty(&super::scan()).unwrap());
    }
}
