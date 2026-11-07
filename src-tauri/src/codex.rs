//! Codex: edit only the keys we own in ~/.codex/config.toml, keep everything else
//! (comments, MCP servers, other providers) byte-for-byte where possible.

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

/// Prefix of provider tables written by builds before 0.2 (`ms-<id>` /
/// `MS_<ID>_API_KEY`). Still recognised so reset and setup can clean them up.
pub const LEGACY_PREFIX: &str = "ms-";
const RESERVED: &[&str] = &["openai", "ollama", "lmstudio", "amazon-bedrock", "oss"];

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub catalog: bool,
    #[serde(default)]
    pub images: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Current {
    pub model: String,
    pub provider: String,
    pub base_url: String,
    pub env_key: String,
    pub effort: String,
    pub catalog: bool,
    /// Saved provider this config points at (filled by the command, not parsed).
    #[serde(default)]
    pub provider_id: String,
}

/// Where Codex keeps its config. Orca exports CODEX_HOME (and ORCA_CODEX_HOME)
/// pointing at its private runtime home into every terminal it opens; when this
/// tool runs from such a terminal that value is not the user's real Codex home,
/// so it is ignored. A CODEX_HOME the user set in their own environment wins.
pub fn codex_home() -> PathBuf {
    use crate::secrets::Env;
    if let Some(u) = crate::secrets::UserEnv.get("CODEX_HOME").filter(|u| !u.trim().is_empty()) {
        return PathBuf::from(u);
    }
    match std::env::var_os("CODEX_HOME") {
        Some(v) if !v.is_empty() && !is_orca_home(Path::new(&v)) => PathBuf::from(v),
        _ => dirs::home_dir().unwrap_or_default().join(".codex"),
    }
}

/// Orca's runtime home (…/orca/codex-runtime-home/…), or what ORCA_CODEX_HOME names.
fn is_orca_home(p: &Path) -> bool {
    if std::env::var_os("ORCA_CODEX_HOME").map(|o| Path::new(&o) == p).unwrap_or(false) {
        return true;
    }
    p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("codex-runtime-home"))
}

pub fn config_path() -> PathBuf {
    codex_home().join("config.toml")
}

/// Host-based name (`https://api.openrouter.ai/v1` -> `openrouter-ai`) that an
/// earlier 0.2 build used as the default. Kept only to recognise and replace it.
pub fn host_slug(base_url: &str) -> String {
    let rest = base_url.split("://").nth(1).unwrap_or(base_url);
    let host = rest.split('/').next().unwrap_or("").to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("api.").unwrap_or(host);
    let mut s: String = host.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    let s = s.trim_matches('-').to_string();
    let s = if s.is_empty() { "gateway".to_string() } else { s };
    // Env var names must not start with a digit (127.0.0.1 -> gw-127-0-0-1).
    let s = if s.starts_with(|c: char| c.is_ascii_digit()) { format!("gw-{s}") } else { s };
    if RESERVED.contains(&s.as_str()) { format!("{s}-gateway") } else { s }
}

/// Environment variable Codex reads the key from: `<NAME>_API_KEY`.
pub fn env_for(name: &str) -> String {
    let up: String = name.to_uppercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    format!("{up}_API_KEY")
}

/// Names written by builds before 0.2, for cleanup.
pub fn legacy_key(provider_id: &str) -> String {
    format!("{LEGACY_PREFIX}{provider_id}")
}
pub fn legacy_env(provider_id: &str) -> String {
    format!("MS_{}", env_for(provider_id))
}

pub fn valid_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok {
        return Err("Codex provider names may only contain lowercase letters, digits, - and _".into());
    }
    if RESERVED.contains(&name) {
        return Err("This name is reserved by Codex; choose another".into());
    }
    Ok(())
}

pub fn valid_env(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if ok { Ok(()) } else { Err("Variable names may only contain UPPERCASE letters, digits and _, and cannot start with a digit".into()) }
}

/// Provider table name and env var for a saved provider.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Names {
    pub key: String,
    pub env: String,
    /// Names earlier builds wrote for this provider (`ms-<id>`, host-based),
    /// replaced on setup and still recognised by cleanup.
    pub old_keys: Vec<String>,
    pub old_envs: Vec<String>,
}

/// Default Codex provider name: neutral, so config.toml and the env var do not
/// reveal which gateway is used. Can be changed per provider.
pub const DEFAULT_NAME: &str = "gateway";

/// Codex provider names this tool writes or wrote, for the saved providers.
pub fn owned_keys(saved: &[crate::providers::Provider]) -> Vec<String> {
    saved.iter().flat_map(|p| { let n = names(p); std::iter::once(n.key).chain(n.old_keys) }).collect()
}

/// Provider table written by this tool (current or legacy naming).
pub fn is_owned_key(key: &str, owned: &[String]) -> bool {
    key.starts_with(LEGACY_PREFIX) || owned.iter().any(|o| o == key)
}

/// Fill `provider_id` from the saved providers.
pub fn with_provider_id(mut cur: Current, saved: &[crate::providers::Provider]) -> Current {
    if let Some(p) = saved.iter().find(|p| { let n = names(p); n.key == cur.provider || n.old_keys.contains(&cur.provider) }) {
        cur.provider_id = p.id.clone();
    }
    cur
}

pub fn names(p: &crate::providers::Provider) -> Names {
    let key = p.codex_name.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| DEFAULT_NAME.to_string());
    let env = p.codex_env.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| env_for(&key));
    let host = host_slug(&p.base_url);
    let old_keys: Vec<String> = [legacy_key(&p.id), host.clone()].into_iter().filter(|k| *k != key).collect();
    let old_envs: Vec<String> = [legacy_env(&p.id), env_for(&host)].into_iter().filter(|e| *e != env).collect();
    Names { key, env, old_keys, old_envs }
}

pub fn read_current(text: &str) -> Current {
    let doc: DocumentMut = text.parse().unwrap_or_default();
    let s = |item: Option<&Item>| item.and_then(|i| i.as_str()).unwrap_or("").to_string();
    let provider = s(doc.get("model_provider"));
    let p = doc.get("model_providers").and_then(|t| t.get(&provider));
    Current {
        model: s(doc.get("model")),
        base_url: s(p.and_then(|t| t.get("base_url"))),
        env_key: s(p.and_then(|t| t.get("env_key"))),
        effort: s(doc.get("model_reasoning_effort")),
        catalog: doc.get("model_catalog_json").is_some(),
        provider,
        provider_id: String::new(),
    }
}

pub fn validate_model(model: &str) -> Result<(), String> {
    let ok = !model.is_empty()
        && model.len() <= 256
        && model.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@+[]-".contains(c));
    if ok { Ok(()) } else { Err("Invalid model ID".into()) }
}

pub fn catalog_dir(data: &Path) -> PathBuf {
    data.join("codex-catalogs")
}

pub fn is_owned_catalog(path: &str, data: &Path) -> bool {
    let p = Path::new(path);
    let name_ok = p
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.len() == 69 && n.ends_with(".json") && n[..64].chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or(false);
    name_ok && p.parent().map(|d| d == catalog_dir(data)).unwrap_or(false)
}

/// Build the gateway model catalog for Codex's /model picker. Content-addressed
/// file name so old catalogs stay valid for undo.
pub fn build_catalog(models: &[String], selected: &str, images: bool, data: &Path) -> Result<(PathBuf, Vec<u8>), String> {
    let mut ids: Vec<&str> = models.iter().map(String::as_str).chain([selected]).collect();
    ids.sort_by_key(|s| s.to_lowercase());
    ids.dedup();
    for id in &ids {
        validate_model(id)?;
    }
    // No "supported_reasoning_levels": advertising effort levels makes Codex
    // send reasoning.effort on every request, and gateways whose upstream is a
    // plain chat API reject it with "Extra inputs are not permitted".
    let modalities = if images { json!(["text", "image"]) } else { json!(["text"]) };
    let entries: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            json!({
                "slug": id, "display_name": id,
                "description": "Gateway model",
                "visibility": "list", "supported_in_api": true, "priority": i,
                "shell_type": "unified_exec",
                "base_instructions": "You are Codex, a coding assistant. Follow the user instructions and repository guidance. Use available tools to inspect, edit and verify work. Report results accurately.",
                "supports_reasoning_summaries": false, "default_reasoning_summary": "none",
                "support_verbosity": false, "experimental_supported_tools": [],
                "input_modalities": modalities,
                "truncation_policy": {"mode": "tokens", "limit": 10000}
            })
        })
        .collect();
    let mut bytes = serde_json::to_vec_pretty(&json!({ "models": entries })).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    Ok((catalog_dir(data).join(format!("{digest}.json")), bytes))
}

/// Return the new config text. `catalog_path` is set when the catalog is on.
pub fn render(before: &str, req: &Request, names: &Names, base_url: &str, catalog_path: Option<&Path>, data: &Path) -> Result<String, String> {
    validate_model(&req.model)?;
    valid_name(&names.key)?;
    valid_env(&names.env)?;
    let key = names.key.clone();
    let mut doc: DocumentMut = before.parse().map_err(|e| format!("config.toml has a syntax error, fix it first: {e}"))?;

    if !doc.contains_key("model_providers") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc["model_providers"] = Item::Table(t);
    }
    let providers = doc["model_providers"].as_table_mut().ok_or("model_providers must be a table")?;
    // Same gateway under a name an earlier build used: drop it, the new table replaces it.
    for old in &names.old_keys {
        providers.remove(old);
    }
    if !providers.contains_key(&key) {
        providers.insert(&key, Item::Table(Table::new()));
    }
    let p = providers[&key].as_table_mut().ok_or("Provider must be a table")?;
    for k in ["auth", "experimental_bearer_token", "requires_openai_auth"] {
        p.remove(k);
    }
    p["name"] = value(key.clone());
    p["base_url"] = value(base_url);
    p["env_key"] = value(names.env.clone());
    p["wire_api"] = value("responses");

    doc["model_provider"] = value(key);
    doc["model"] = value(req.model.trim());
    match req.effort.as_deref() {
        None | Some("") | Some("default") => {
            doc.remove("model_reasoning_effort");
        }
        Some(e) if ["low", "medium", "high", "xhigh"].contains(&e) => {
            doc["model_reasoning_effort"] = value(e);
        }
        Some(_) => return Err("Invalid effort".into()),
    }
    match catalog_path {
        Some(path) => doc["model_catalog_json"] = value(path.display().to_string()),
        None => {
            let owned = doc.get("model_catalog_json").and_then(|i| i.as_str()).map(|s| is_owned_catalog(s, data)).unwrap_or(false);
            if owned {
                doc.remove("model_catalog_json");
            }
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(model: &str) -> Request {
        Request { provider_id: "9router".into(), model: model.into(), effort: None, catalog: false, images: false }
    }

    fn nm() -> Names {
        Names { key: "gateway".into(), env: "GATEWAY_API_KEY".into(), old_keys: vec!["ms-9router".into(), "localhost-20128".into()], old_envs: vec!["MS_9ROUTER_API_KEY".into(), "LOCALHOST_20128_API_KEY".into()] }
    }

    #[test]
    fn keeps_unrelated_content_and_comments() {
        let before = "# my notes\nsandbox_mode = \"workspace-write\"\n\n[mcp_servers.docs]\ncommand = \"npx\"\n";
        let out = render(before, &req("gpt-6.1-sol"), &nm(), "http://localhost:20128/v1", None, Path::new("/d")).unwrap();
        assert!(out.contains("# my notes"));
        assert!(out.contains("[mcp_servers.docs]"));
        let cur = read_current(&out);
        assert_eq!(cur.model, "gpt-6.1-sol");
        assert_eq!(cur.provider, "gateway");
        assert_eq!(cur.env_key, "GATEWAY_API_KEY");
        assert_eq!(cur.base_url, "http://localhost:20128/v1");
    }

    #[test]
    fn effort_default_removes_key_and_old_auth_dropped() {
        let before = "model_reasoning_effort = \"high\"\n[model_providers.ms-9router]\nrequires_openai_auth = true\n";
        let out = render(before, &req("x"), &nm(), "https://g/v1", None, Path::new("/d")).unwrap();
        assert!(!out.contains("model_reasoning_effort"));
        assert!(!out.contains("requires_openai_auth"));
        assert!(!out.contains("ms-9router"), "old ms-<id> table must be replaced");
    }

    #[test]
    fn catalog_only_removed_when_owned() {
        let data = Path::new("/data");
        let owned = catalog_dir(data).join(format!("{}.json", "a".repeat(64)));
        let before = format!("model_catalog_json = '{}'\n", owned.display());
        let out = render(&before, &req("x"), &nm(), "https://g/v1", None, data).unwrap();
        assert!(!out.contains("model_catalog_json"));
        let foreign = "model_catalog_json = '/elsewhere/models.json'\n";
        let out = render(foreign, &req("x"), &nm(), "https://g/v1", None, data).unwrap();
        assert!(out.contains("/elsewhere/models.json"));
    }

    #[test]
    fn catalog_is_content_addressed_and_includes_selected() {
        let data = Path::new("/data");
        let (p1, b1) = build_catalog(&["b".into(), "a".into()], "z[1m]", true, data).unwrap();
        let (p2, _) = build_catalog(&["a".into(), "b".into()], "z[1m]", true, data).unwrap();
        assert_eq!(p1, p2);
        assert!(is_owned_catalog(&p1.display().to_string(), data));
        let v: serde_json::Value = serde_json::from_slice(&b1).unwrap();
        assert_eq!(v["models"].as_array().unwrap().len(), 3);
        assert_eq!(v["models"][0]["input_modalities"][1], "image");
    }

    #[test]
    fn host_based_names() {
        assert_eq!(host_slug("https://api.openrouter.ai/v1"), "openrouter-ai");
        assert_eq!(host_slug("http://localhost:20128/v1"), "localhost-20128");
        assert_eq!(host_slug("http://127.0.0.1:4000"), "gw-127-0-0-1-4000");
        assert_eq!(env_for("openrouter-ai"), "OPENROUTER_AI_API_KEY");
        assert_eq!(legacy_env("acme"), "MS_ACME_API_KEY");
        assert!(valid_name("openai").is_err());
        assert!(valid_env("1BAD").is_err());
        assert!(valid_env("MY_GATEWAY_KEY").is_ok());
    }

    #[test]
    fn orca_home_is_not_codex_home() {
        let base = Path::new("users").join("x");
        assert!(is_orca_home(&base.join("orca").join("codex-runtime-home").join("home")));
        assert!(!is_orca_home(&base.join(".codex")));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(render("", &req("bad id"), &nm(), "https://g/v1", None, Path::new("/d")).is_err());
        assert!(render("not = [valid", &req("x"), &nm(), "https://g/v1", None, Path::new("/d")).is_err());
    }
}
