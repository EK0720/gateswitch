//! Saved gateways. Metadata in providers.json, keys in the OS keychain.

use crate::secrets::Vault;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// OpenAI-style base URL, normally ending in /v1.
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub checked_at: Option<String>,
    /// Per-model metadata from /models (owned_by groups upstream accounts on 9router).
    #[serde(default)]
    pub info: BTreeMap<String, ModelInfo>,
    /// Last "Kiểm tra model" result per model.
    #[serde(default)]
    pub probes: BTreeMap<String, crate::probe::Probe>,
    /// Codex provider table name. None = "gateway".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_name: Option<String>,
    /// Env var Codex reads the key from. None = `<CODEX_NAME>_API_KEY`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_env: Option<String>,
    /// Extra HTTP headers the gateway asks for (see headers.rs). Not secrets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_deserializing)]
    pub has_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelInfo {
    #[serde(default)]
    pub owned_by: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

pub struct Providers {
    path: PathBuf,
}

pub fn account(id: &str) -> String {
    format!("provider:{id}")
}

/// Accept http(s) without credentials/query/fragment; strip trailing slashes.
pub fn normalize_url(raw: &str) -> Result<String, String> {
    let url = raw.trim().trim_end_matches('/');
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("URL must start with https:// or http://")?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || host.contains('@') || url.contains('?') || url.contains('#') || url.contains(char::is_whitespace) {
        return Err("Invalid URL: must not contain credentials, ?, # or spaces".into());
    }
    Ok(url.to_string())
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "gateway".into() } else { s }
}

impl Providers {
    pub fn new(root: PathBuf) -> Self {
        Providers { path: root.join("providers.json") }
    }

    pub fn list(&self, vault: &dyn Vault) -> Vec<Provider> {
        let mut items: Vec<Provider> = fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        for p in &mut items {
            p.has_key = vault.get(&account(&p.id)).is_some();
        }
        items
    }

    fn save(&self, items: &[Provider]) -> Result<(), String> {
        let text = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
        crate::store::atomic_write(&self.path, text.as_bytes())
    }

    pub fn get(&self, id: &str, vault: &dyn Vault) -> Result<Provider, String> {
        self.list(vault).into_iter().find(|p| p.id == id).ok_or("Provider not found".into())
    }

    /// Add or update. `key: None` keeps the stored key.
    pub fn upsert(&self, id: Option<&str>, name: &str, base_url: &str, key: Option<&str>, vault: &dyn Vault) -> Result<Provider, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a provider name".into());
        }
        let base_url = normalize_url(base_url)?;
        let mut items = self.list(vault);
        let id = match id {
            Some(id) => id.to_string(),
            None => {
                let base = slug(name);
                let mut candidate = base.clone();
                let mut n = 2;
                while items.iter().any(|p| p.id == candidate) {
                    candidate = format!("{base}-{n}");
                    n += 1;
                }
                candidate
            }
        };
        if let Some(k) = key.map(str::trim).filter(|k| !k.is_empty()) {
            vault.put(&account(&id), k)?;
        }
        let entry = match items.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                if p.base_url != base_url {
                    p.models.clear();
                    p.info.clear();
                    p.probes.clear();
                    p.latency_ms = None;
                }
                p.name = name.into();
                p.base_url = base_url;
                p.clone()
            }
            None => {
                let p = Provider { id: id.clone(), name: name.into(), base_url, models: vec![], latency_ms: None, checked_at: None, info: BTreeMap::new(), probes: BTreeMap::new(), codex_name: None, codex_env: None, headers: BTreeMap::new(), has_key: false };
                items.push(p.clone());
                p
            }
        };
        self.save(&items)?;
        Ok(Provider { has_key: vault.get(&account(&id)).is_some(), ..entry })
    }

    pub fn remove(&self, id: &str, vault: &dyn Vault) -> Result<(), String> {
        let items: Vec<Provider> = self.list(vault).into_iter().filter(|p| p.id != id).collect();
        self.save(&items)?;
        vault.delete(&account(id))
    }

    pub fn set_models(&self, id: &str, models: Vec<String>, info: BTreeMap<String, ModelInfo>, latency_ms: u64, vault: &dyn Vault) -> Result<Provider, String> {
        let mut items = self.list(vault);
        let p = items.iter_mut().find(|p| p.id == id).ok_or("Provider not found")?;
        p.probes.retain(|m, _| models.contains(m));
        p.models = models;
        p.info = info;
        p.latency_ms = Some(latency_ms);
        p.checked_at = Some(chrono::Local::now().to_rfc3339());
        let out = p.clone();
        self.save(&items)?;
        Ok(out)
    }

    /// Replace the provider's custom headers (empty map = none).
    pub fn set_headers(&self, id: &str, headers: BTreeMap<String, String>, vault: &dyn Vault) -> Result<Provider, String> {
        crate::headers::validate(&headers)?;
        let mut items = self.list(vault);
        let p = items.iter_mut().find(|p| p.id == id).ok_or("Provider not found")?;
        p.headers = headers;
        let out = p.clone();
        self.save(&items)?;
        Ok(Provider { has_key: vault.get(&account(id)).is_some(), ..out })
    }

    /// Override the Codex provider name / env var (empty = default from host).
    pub fn set_codex_names(&self, id: &str, name: Option<String>, env: Option<String>, vault: &dyn Vault) -> Result<Provider, String> {
        let name = name.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let env = env.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(n) = &name { crate::codex::valid_name(n)?; }
        if let Some(e) = &env { crate::codex::valid_env(e)?; }
        let mut items = self.list(vault);
        let p = items.iter_mut().find(|p| p.id == id).ok_or("Provider not found")?;
        p.codex_name = name;
        p.codex_env = env;
        let out = p.clone();
        self.save(&items)?;
        Ok(out)
    }

    /// Merge probe results (keeps older results for models not in this run).
    pub fn set_probes(&self, id: &str, results: Vec<(String, crate::probe::Probe)>, vault: &dyn Vault) -> Result<Provider, String> {
        let mut items = self.list(vault);
        let p = items.iter_mut().find(|p| p.id == id).ok_or("Provider not found")?;
        for (m, r) in results {
            p.probes.insert(m, r);
        }
        let out = p.clone();
        self.save(&items)?;
        Ok(Provider { has_key: vault.get(&account(id)).is_some(), ..out })
    }
}

/// owned_by / capabilities per model, when the gateway reports them (9router does).
pub fn parse_info(body: &serde_json::Value) -> BTreeMap<String, ModelInfo> {
    let list = body.get("data").or_else(|| body.get("models")).and_then(|v| v.as_array());
    let mut out = BTreeMap::new();
    for m in list.into_iter().flatten() {
        let Some(id) = m.get("id").or_else(|| m.get("name")).and_then(|v| v.as_str()) else { continue };
        let owned_by = m.get("owned_by").and_then(|v| v.as_str()).filter(|s| !s.is_empty() && s.len() <= 64).map(String::from);
        let capabilities: Vec<String> = match m.get("capabilities") {
            Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            Some(serde_json::Value::Object(o)) => o.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect(),
            _ => vec![],
        };
        if owned_by.is_some() || !capabilities.is_empty() {
            out.insert(id.to_string(), ModelInfo { owned_by, capabilities });
        }
    }
    out
}

/// Parse an OpenAI (`data[]`) or Anthropic-style (`models[]` / `data[]`) model list.
pub fn parse_models(body: &serde_json::Value) -> Result<Vec<String>, String> {
    let list = body
        .get("data")
        .or_else(|| body.get("models"))
        .or(Some(body))
        .and_then(|v| v.as_array())
        .ok_or("Gateway returned a model list in the wrong format (expected data[] or models[])")?;
    let mut ids: Vec<String> = list
        .iter()
        .filter_map(|m| m.get("id").or_else(|| m.get("name")).and_then(|v| v.as_str()))
        .filter(|id| id.len() <= 256 && id.chars().all(|c| c.is_ascii_alphanumeric() || "_./:@+[]-".contains(c)))
        .map(String::from)
        .collect();
    ids.sort_by_key(|s| s.to_lowercase());
    ids.dedup();
    if ids.is_empty() {
        return Err("Gateway returned no models".into());
    }
    Ok(ids)
}

/// GET {base}/models with the stored key. Returns models and round-trip time.
pub fn fetch_models(base_url: &str, key: &str) -> Result<(Vec<String>, BTreeMap<String, ModelInfo>, u64), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let started = Instant::now();
    let mut resp = agent
        .get(&format!("{base_url}/models"))
        .header("Authorization", &format!("Bearer {key}"))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("Could not connect to {base_url}: {e}"))?;
    let ms = started.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();
    match status {
        200..=299 => {}
        401 | 403 => return Err(format!("Key rejected (HTTP {status}). Check the key.")),
        404 => return Err("Gateway has no /models (HTTP 404). Check whether the URL includes /v1.".into()),
        _ => return Err(format!("Gateway returned HTTP {status}.")),
    }
    let body: serde_json::Value = resp.body_mut().read_json().map_err(|_| "Gateway returned non-JSON content".to_string())?;
    Ok((parse_models(&body)?, parse_info(&body), ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::Mem;

    #[test]
    fn upsert_keeps_key_in_vault_not_file() {
        let dir = tempfile::tempdir().unwrap();
        let ps = Providers::new(dir.path().into());
        let v = Mem::default();
        let p = ps.upsert(None, "9router", "http://localhost:20128/v1/", Some("sk-abc"), &v).unwrap();
        assert_eq!(p.id, "9router");
        assert_eq!(p.base_url, "http://localhost:20128/v1");
        assert!(p.has_key);
        let file = fs::read_to_string(dir.path().join("providers.json")).unwrap();
        assert!(!file.contains("sk-abc"));
        let again = ps.upsert(None, "9router", "http://x.dev/v1", None, &v).unwrap();
        assert_eq!(again.id, "9router-2");
        ps.remove("9router", &v).unwrap();
        assert!(v.get(&account("9router")).is_none());
    }

    #[test]
    fn url_validation() {
        assert!(normalize_url("ftp://x").is_err());
        assert!(normalize_url("https://user:pw@x.dev/v1").is_err());
        assert!(normalize_url("https://x.dev/v1?a=1").is_err());
        assert_eq!(normalize_url(" https://x.dev/v1/ ").unwrap(), "https://x.dev/v1");
    }

    #[test]
    fn parse_both_shapes_and_reject_bad_ids() {
        let a = serde_json::json!({"data":[{"id":"gpt-6"},{"id":"claude-opus-5.5[1m]"},{"id":"bad id"}]});
        assert_eq!(parse_models(&a).unwrap(), vec!["claude-opus-5.5[1m]", "gpt-6"]);
        let b = serde_json::json!({"models":[{"name":"x"}]});
        assert_eq!(parse_models(&b).unwrap(), vec!["x"]);
        assert!(parse_models(&serde_json::json!({"data":[]})).is_err());
        let c = serde_json::json!({"data":[
            {"id":"cx/gpt-6","owned_by":"cx","capabilities":{"vision":true,"tools":false}},
            {"id":"combo","owned_by":"combo","capabilities":["chat"]},
            {"id":"bare"}]});
        let info = parse_info(&c);
        assert_eq!(info["cx/gpt-6"].owned_by.as_deref(), Some("cx"));
        assert_eq!(info["cx/gpt-6"].capabilities, vec!["vision"]);
        assert_eq!(info["combo"].capabilities, vec!["chat"]);
        assert!(!info.contains_key("bare"));
    }
}
