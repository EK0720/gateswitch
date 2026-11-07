//! Gateway preset files: a gateway publishes one JSON, the user imports it and
//! pastes a key. Presets never contain a key; one that does is refused.
//!
//! ```json
//! { "schema": "model-switcher/preset@1", "name": "My gateway",
//!   "base_url": "https://gw.example.com/v1", "auth": "bearer",
//!   "models": ["cx/gpt-5", "cc/claude-sonnet-5[1m]"],
//!   "defaults": { "codex": "cx/gpt-5", "claude": "cc/claude-sonnet-5[1m]" },
//!   "docs": "https://gw.example.com/docs" }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "model-switcher/preset@1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    #[serde(default)]
    pub schema: String,
    pub name: String,
    pub base_url: String,
    /// "bearer" (default) or "x-api-key".
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    /// Default model per app: "codex", "claude", "claude-desktop", "orca".
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub docs: Option<String>,
    /// Custom headers the gateway asks for (not secrets).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

const SECRET_FIELDS: &[&str] = &["key", "api_key", "apiKey", "token", "auth_token", "secret", "password"];

pub fn parse(text: &str) -> Result<Preset, String> {
    let v: serde_json::Value = serde_json::from_str(text.trim().trim_start_matches('\u{feff}')).map_err(|e| format!("Preset file is not valid JSON: {e}"))?;
    if let Some(obj) = v.as_object() {
        if let Some(f) = SECRET_FIELDS.iter().find(|f| obj.contains_key(**f)) {
            return Err(format!("Preset has the field \"{f}\". Presets must not contain keys; paste the key separately in the tool."));
        }
    }
    let mut p: Preset = serde_json::from_value(v).map_err(|e| format!("Preset is missing a field: {e}"))?;
    if !p.schema.is_empty() && p.schema != SCHEMA {
        return Err(format!("Preset uses format {}, which this tool does not support yet", p.schema));
    }
    p.schema = SCHEMA.into();
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        return Err("Preset is missing a name".into());
    }
    p.base_url = crate::providers::normalize_url(&p.base_url)?;
    match p.auth.as_deref() {
        None | Some("bearer") | Some("x-api-key") => {}
        Some(other) => return Err(format!("Invalid auth \"{other}\" (bearer or x-api-key)")),
    }
    for m in p.models.iter().chain(p.defaults.values()) {
        crate::codex::validate_model(m)?;
    }
    crate::headers::validate(&p.headers)?;
    for app in p.defaults.keys() {
        if !["codex", "claude", "claude-desktop", "orca"].contains(&app.as_str()) {
            return Err(format!("defaults has an unknown app: {app}"));
        }
    }
    Ok(p)
}

pub fn export(p: &crate::providers::Provider) -> String {
    let preset = Preset {
        schema: SCHEMA.into(),
        name: p.name.clone(),
        base_url: p.base_url.clone(),
        auth: None,
        models: p.models.clone(),
        defaults: BTreeMap::new(),
        docs: None,
        headers: p.headers.clone(),
    };
    serde_json::to_string_pretty(&preset).unwrap_or_default()
}

/// Generic local gateways people commonly run. No hosted service is bundled
/// as a default, so the public build carries no one's private endpoint.
pub fn samples() -> Vec<Preset> {
    let p = |name: &str, url: &str| Preset {
        schema: SCHEMA.into(),
        name: name.into(),
        base_url: url.into(),
        auth: None,
        models: vec![],
        defaults: BTreeMap::new(),
        docs: None,
        headers: BTreeMap::new(),
    };
    vec![
        p("9router (this machine)", "http://localhost:20128/v1"),
        p("LiteLLM (this machine)", "http://localhost:4000/v1"),
        p("OpenRouter", "https://openrouter.ai/api/v1"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes() {
        let p = parse(r#"{"name":" GW ","base_url":"https://gw.example.com/v1/","models":["a"],"defaults":{"codex":"a"}}"#).unwrap();
        assert_eq!(p.name, "GW");
        assert_eq!(p.base_url, "https://gw.example.com/v1");
        assert_eq!(p.schema, SCHEMA);
    }

    #[test]
    fn refuses_keys_and_bad_values() {
        assert!(parse(r#"{"name":"g","base_url":"https://g/v1","api_key":"sk-x"}"#).unwrap_err().contains("key"));
        assert!(parse(r#"{"name":"g","base_url":"ftp://g"}"#).is_err());
        assert!(parse(r#"{"name":"g","base_url":"https://g/v1","auth":"basic"}"#).is_err());
        assert!(parse(r#"{"name":"g","base_url":"https://g/v1","defaults":{"vim":"a"}}"#).is_err());
        assert!(parse(r#"{"schema":"other@9","name":"g","base_url":"https://g/v1"}"#).is_err());
    }

    #[test]
    fn samples_are_valid() {
        for s in samples() {
            parse(&serde_json::to_string(&s).unwrap()).unwrap();
        }
    }
}
