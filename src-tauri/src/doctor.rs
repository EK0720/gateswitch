//! "Doctor": one-click health check of what is actually configured on this
//! machine. Read only. Every finding says what is wrong and what to do.

use crate::secrets::Env;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Check {
    /// "ok", "warn" or "bad".
    pub level: &'static str,
    pub app: String,
    pub title: String,
    pub detail: String,
    /// What the "Sửa" button does: "env:<NAME>" clears a stale env var,
    /// "page:<route>" opens the page that fixes it.
    pub fix: Option<String>,
}

fn level_of(bad: bool, warn: bool) -> &'static str {
    if bad { "bad" } else if warn { "warn" } else { "ok" }
}

pub fn run() -> Vec<Check> {
    let mut out = Vec::new();

    let scan = crate::detect::scan();
    for t in &scan.tools {
        out.push(Check {
            level: if t.installed { "ok" } else { "warn" },
            app: t.name.to_string(),
            title: if t.installed { format!("{} installed", t.name) } else { format!("{} not found", t.name) },
            detail: t.path.clone().unwrap_or_else(|| "Not on PATH or in the usual locations".into()),
            fix: None,
        });
    }

    // Claude Code: the settings file and the user env must not fight.
    let text = std::fs::read_to_string(crate::claude::settings_path()).unwrap_or_default();
    let cur = crate::claude::read_current(&text, &|n| crate::secrets::UserEnv.get(n));
    if cur.base_url.is_empty() {
        out.push(Check { level: "warn", app: "Claude Code".into(), title: "Not connected to a gateway".into(), detail: "settings.json has no ANTHROPIC_BASE_URL. Set it up on the Claude Code page.".into(), fix: Some("page:claude-code".into()) });
    } else {
        out.push(Check { level: "ok", app: "Claude Code".into(), title: format!("Using {}", cur.base_url), detail: format!("Model: {}", if cur.model.is_empty() { "default" } else { &cur.model }), fix: None });
    }
    for w in cur.warnings {
        out.push(Check { level: "bad", app: "Claude Code".into(), title: "Configuration problem".into(), detail: w, fix: Some("page:claude-code".into()) });
    }
    // Claude Code ranks the ANTHROPIC_MODEL environment variable above the
    // `model` in settings.json, so a leftover one silently picks the model.
    if let Some(v) = crate::secrets::UserEnv.get("ANTHROPIC_MODEL").filter(|v| !v.is_empty()) {
        out.push(Check {
            level: "bad",
            app: "Claude Code".into(),
            title: "ANTHROPIC_MODEL is set as an environment variable".into(),
            detail: format!("Every new session starts on {v} instead of the model set here, because the environment variable wins over settings.json. Click Fix to remove it (can be undone)."),
            fix: Some("env:ANTHROPIC_MODEL".into()),
        });
    }
    if crate::secrets::UserEnv.get("CLAUDE_CODE_EFFORT_LEVEL").is_some() {
        out.push(Check {
            level: "bad",
            app: "Claude Code".into(),
            title: "CLAUDE_CODE_EFFORT_LEVEL is still set as an environment variable".into(),
            detail: "This variable makes Claude Code send reasoning_effort, which many gateways reject with 400. Click Fix to remove it (can be undone).".into(),
            fix: Some("env:CLAUDE_CODE_EFFORT_LEVEL".into()),
        });
    }

    let in_settings = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.pointer("/env/CLAUDE_CODE_EFFORT_LEVEL").cloned())
        .is_some();
    if in_settings {
        out.push(Check {
            level: "warn",
            app: "Claude Code".into(),
            title: "settings.json still has CLAUDE_CODE_EFFORT_LEVEL".into(),
            detail: "If the gateway returns 400 for reasoning_effort, set up again on the Claude Code page and leave effort at default.".into(),
            fix: Some("page:claude-code".into()),
        });
    }

    // Codex.
    let codex_text = std::fs::read_to_string(crate::codex::config_path()).unwrap_or_default();
    let codex = crate::codex::read_current(&codex_text);
    let catalog_owned = {
        let doc: toml_edit::DocumentMut = codex_text.parse().unwrap_or_default();
        doc.get("model_catalog_json").and_then(|i| i.as_str()).map(|s| crate::codex::is_owned_catalog(s, &crate::data_dir())).unwrap_or(false)
    };
    if codex.catalog && !catalog_owned {
        out.push(Check { level: "warn", app: "Codex".into(), title: "Catalog was not created by this tool".into(), detail: "model_catalog_json points to a file from an old version of this tool. Set up again on the Codex page to use the new catalog.".into(), fix: Some("page:codex".into()) });
    }
    let owned = crate::codex::owned_keys(&crate::providers().list(&crate::secrets::Keychain));
    if crate::codex::is_owned_key(&codex.provider, &owned) {
        let key_set = crate::secrets::UserEnv.get(&codex.env_key).is_some();
        out.push(Check {
            level: level_of(!key_set, false),
            app: "Codex".into(),
            title: format!("Using {} · {}", codex.provider, codex.model),
            detail: if key_set { format!("Variable {} is set", codex.env_key) } else { format!("Environment variable {} is missing — Codex will have no key", codex.env_key) },
            fix: (!key_set).then(|| "page:codex".into()),
        });
        if !catalog_owned {
            out.push(Check { level: "warn", app: "Codex".into(), title: "No catalog for the /model menu".into(), detail: "Enable the catalog when setting up again so /model lists the gateway's models.".into(), fix: Some("page:codex".into()) });
        }
    } else if !codex.provider.is_empty() {
        out.push(Check { level: "ok", app: "Codex".into(), title: format!("Using provider {}", codex.provider), detail: "Not a gateway set up by Gateswitch.".into(), fix: None });
    } else {
        out.push(Check { level: "warn", app: "Codex".into(), title: "No provider configured".into(), detail: "Set it up on the Codex page.".into(), fix: Some("page:codex".into()) });
    }

    // Claude Desktop policy.
    if let Some(p) = crate::desktop::policy_override() {
        out.push(Check { level: "bad", app: "Claude Desktop".into(), title: "Blocked by a management policy".into(), detail: format!("Policy {p} overrides local configuration. Setup in this tool will have no effect."), fix: None });
    }

    // Proxy that can silently reroute traffic.
    for name in ["HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY"] {
        if let Some(v) = crate::secrets::UserEnv.get(name) {
            out.push(Check { level: "warn", app: "Network".into(), title: format!("{name} is set"), detail: format!("All requests go through {}. If the gateway behaves oddly, check this proxy.", mask_userinfo(&v)), fix: None });
        }
    }

    out
}

/// Proxy URLs can carry `user:password@`; never show that part.
fn mask_userinfo(url: &str) -> String {
    let start = url.find("://").map(|i| i + 3).unwrap_or(0);
    let rest = &url[start..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rfind('@') {
        Some(at) => format!("{}***@{}", &url[..start], &rest[at + 1..]),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_credentials_are_masked() {
        assert_eq!(mask_userinfo("http://u:secret@10.0.0.1:8080"), "http://***@10.0.0.1:8080");
        assert_eq!(mask_userinfo("http://10.0.0.1:8080/a@b"), "http://10.0.0.1:8080/a@b");
    }

    #[test]
    fn doctor_runs_without_panic() {
        let checks = run();
        assert!(checks.iter().any(|c| c.app == "Claude Code" || c.app == "Claude Code CLI"));
    }

    #[test]
    #[ignore]
    fn print_doctor() {
        println!("{}", serde_json::to_string_pretty(&run()).unwrap());
    }
}
