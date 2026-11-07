//! Claude Code: route through a gateway via the `env` block of
//! ~/.claude/settings.json (official method, works on every OS and reaches
//! background agents). Only keys we own are touched; key order is preserved.
//!
//! Source: https://code.claude.com/docs/en/llm-gateway-connect
//! - ANTHROPIC_BASE_URL is the gateway root; Claude Code appends /v1/messages.
//! - ANTHROPIC_AUTH_TOKEN -> Authorization: Bearer; ANTHROPIC_API_KEY -> x-api-key.
//! - settings-file env wins over shell/user env.
//! - CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1 adds gateway models to /model;
//!   a modelPicker with replaceBuiltInOptions hides them, so we use one or the other.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

/// Description we put on our modelPicker entries; marks the list as ours.
pub const PICKER_MARK: &str = "From gateway (Gateswitch)";
/// Marks written by earlier builds (before the rename to Gateswitch); still recognised as ours.
const LEGACY_PICKER_MARKS: &[&str] = &["From gateway (Model Switcher)", "Từ gateway (Model Switcher)"];

/// Env keys that decide routing. Cleared before every setup so an old gateway,
/// a stale model pin or a cloud-provider switch can't win over the new one.
pub(crate) const ROUTING_ENV: &[&str] = &[
    "ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL", "CLAUDE_CODE_EFFORT_LEVEL",
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY",
    "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY", "CLAUDE_CODE_USE_ANTHROPIC_AWS",
];

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub provider_id: String,
    pub model: String,
    /// "bearer" (default) or "x-api-key".
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Replace the /model menu with the gateway's models.
    #[serde(default)]
    pub picker: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Current {
    pub model: String,
    pub base_url: String,
    /// "bearer", "x-api-key" or "" (no gateway credential in settings).
    pub auth: String,
    pub effort: String,
    pub picker: bool,
    /// Things that may stop the setup from taking effect.
    pub warnings: Vec<String>,
}

pub fn claude_home() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".claude"))
}

pub fn settings_path() -> PathBuf {
    claude_home().join("settings.json")
}

/// Provider URLs are stored OpenAI-style (…/v1). Claude Code wants the root.
pub fn base_root(base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b).to_string()
}

fn parse(text: &str) -> Result<Map<String, Value>, String> {
    let t = text.trim_start_matches('\u{feff}');
    if t.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(t) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err("Claude's settings.json must be a JSON object".into()),
        Err(e) => Err(format!("Claude's settings.json has a syntax error (line {}). Fix or delete the file, then try again.", e.line())),
    }
}

pub(crate) fn is_owned_picker(v: Option<&Value>) -> bool {
    v.and_then(|p| p.get("options"))
        .and_then(|o| o.as_array())
        .map(|a| !a.is_empty() && a.iter().all(|e| e.get("description").and_then(|d| d.as_str()) .map(|d| d == PICKER_MARK || LEGACY_PICKER_MARKS.contains(&d)) == Some(true)))
        .unwrap_or(false)
}

/// `user_env` looks up a user-level environment variable (registry on Windows).
pub fn read_current(text: &str, user_env: &dyn Fn(&str) -> Option<String>) -> Current {
    let doc = parse(text).unwrap_or_default();
    let env = doc.get("env").and_then(|e| e.as_object()).cloned().unwrap_or_default();
    let s = |k: &str| env.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let auth = if env.contains_key("ANTHROPIC_AUTH_TOKEN") {
        "bearer"
    } else if env.contains_key("ANTHROPIC_API_KEY") {
        "x-api-key"
    } else {
        ""
    };
    let mut warnings = vec![];
    if parse(text).is_err() {
        warnings.push("settings.json has a syntax error; Claude Code will ignore this file.".into());
    }
    // Settings env wins over user env for the same name, but a credential in
    // the *other* variable still triggers Claude's auth-conflict warning.
    for (name, ours) in [("ANTHROPIC_API_KEY", "bearer"), ("ANTHROPIC_AUTH_TOKEN", "x-api-key")] {
        if auth == ours && user_env(name).is_some() {
            warnings.push(format!("The Windows environment variable {name} is set; Claude Code will report a key conflict. Remove it if unused."));
        }
    }
    if auth.is_empty() && !s("ANTHROPIC_BASE_URL").is_empty() {
        warnings.push("A base URL is set but settings.json has no key; Claude Code will use your claude.ai account.".into());
    }
    Current {
        model: doc.get("model").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| s("ANTHROPIC_MODEL")),
        base_url: s("ANTHROPIC_BASE_URL"),
        auth: auth.into(),
        effort: s("CLAUDE_CODE_EFFORT_LEVEL"),
        picker: is_owned_picker(doc.get("modelPicker")),
        warnings,
    }
}

/// ANTHROPIC_CUSTOM_HEADERS is ours only when it equals what a saved provider
/// would write; a value the user typed by hand is left alone.
pub(crate) fn is_owned_headers(v: Option<&Value>, owned: &[String]) -> bool {
    v.and_then(|v| v.as_str()).map(|s| owned.iter().any(|o| o == s)).unwrap_or(false)
}

/// New settings.json text. `models` feeds the /model menu when `req.picker`.
/// `headers` is the provider's custom headers; `owned` the header values any
/// saved provider would write (so switching gateways drops the previous one's).
pub fn render(before: &str, req: &Request, base_url: &str, key: &str, models: &[String], headers: &crate::headers::Headers, owned: &[String]) -> Result<String, String> {
    crate::codex::validate_model(&req.model)?;
    if key.trim().is_empty() {
        return Err("This provider has no key".into());
    }
    let mut doc = parse(before)?;
    let mut env = match doc.remove("env") {
        Some(Value::Object(m)) => m,
        None | Some(Value::Null) => Map::new(),
        Some(_) => return Err("The env entry in settings.json must be an object".into()),
    };
    for k in ROUTING_ENV {
        env.remove(*k);
    }
    if !headers.is_empty() || is_owned_headers(env.get("ANTHROPIC_CUSTOM_HEADERS"), owned) {
        env.remove("ANTHROPIC_CUSTOM_HEADERS");
    }
    if !headers.is_empty() {
        env.insert("ANTHROPIC_CUSTOM_HEADERS".into(), json!(crate::headers::to_env(headers)));
    }
    env.insert("ANTHROPIC_BASE_URL".into(), json!(base_root(base_url)));
    let cred = if req.auth.as_deref() == Some("x-api-key") { "ANTHROPIC_API_KEY" } else { "ANTHROPIC_AUTH_TOKEN" };
    env.insert(cred.into(), json!(key.trim()));
    match req.effort.as_deref() {
        Some(e @ ("low" | "medium" | "high" | "xhigh" | "max")) => {
            env.insert("CLAUDE_CODE_EFFORT_LEVEL".into(), json!(e));
        }
        None | Some("") | Some("default") => {}
        Some(other) => return Err(format!("Invalid effort: {other}")),
    }
    if !req.picker {
        env.insert("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".into(), json!("1"));
    }
    doc.insert("env".into(), Value::Object(env));
    // Top-level `model` (not ANTHROPIC_MODEL) so a later /model choice still sticks.
    doc.insert("model".into(), json!(req.model));

    if req.picker {
        let mut ids: Vec<&str> = models.iter().map(String::as_str).collect();
        if !ids.contains(&req.model.as_str()) {
            ids.insert(0, &req.model);
        }
        for m in &ids {
            crate::codex::validate_model(m)?;
        }
        let options: Vec<Value> = ids.iter().map(|m| json!({ "model": m, "label": m, "description": PICKER_MARK })).collect();
        doc.insert("modelPicker".into(), json!({ "replaceBuiltInOptions": true, "options": options }));
    } else if is_owned_picker(doc.get("modelPicker")) {
        doc.remove("modelPicker");
    }
    let mut out = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(picker: bool) -> Request {
        Request { provider_id: "9router".into(), model: "cc/claude-opus-5.5[1m]".into(), auth: None, effort: None, picker }
    }
    const NONE: &dyn Fn(&str) -> Option<String> = &|_| None;

    #[test]
    fn keeps_unrelated_settings_and_order() {
        let before = r#"{
  "permissions": {"allow": ["Bash(ls)"]},
  "env": {"FOO": "1", "ANTHROPIC_MODEL": "old", "CLAUDE_CODE_USE_BEDROCK": "1", "ANTHROPIC_API_KEY": "old-key"},
  "hooks": {}
}"#;
        let out = render(before, &req(false), "http://localhost:20128/v1/", "sk-new", &[], &Default::default(), &[]).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(&keys[..2], &["permissions", "hooks"]);
        let env = &v["env"];
        assert_eq!(env["FOO"], "1");
        assert_eq!(env["ANTHROPIC_BASE_URL"], "http://localhost:20128");
        assert_eq!(env["ANTHROPIC_AUTH_TOKEN"], "sk-new");
        assert_eq!(env["CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"], "1");
        for gone in ["ANTHROPIC_MODEL", "CLAUDE_CODE_USE_BEDROCK", "ANTHROPIC_API_KEY"] {
            assert!(env.get(gone).is_none(), "{gone}");
        }
        assert_eq!(v["model"], "cc/claude-opus-5.5[1m]");
        let cur = read_current(&out, NONE);
        assert_eq!((cur.auth.as_str(), cur.picker), ("bearer", false));
        assert!(cur.warnings.is_empty());
    }

    #[test]
    fn picker_owned_and_removed() {
        let models = vec!["cc/claude-sonnet-5".to_string(), "cx/gpt-6".to_string()];
        let on = render("", &req(true), "https://gw.example/v1", "k", &models, &Default::default(), &[]).unwrap();
        let v: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(v["modelPicker"]["options"].as_array().unwrap().len(), 3);
        assert!(v["env"].get("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY").is_none());
        assert!(read_current(&on, NONE).picker);
        let off = render(&on, &req(false), "https://gw.example/v1", "k", &models, &Default::default(), &[]).unwrap();
        assert!(serde_json::from_str::<Value>(&off).unwrap().get("modelPicker").is_none());
        // A picker the user wrote stays.
        let mine = r#"{"modelPicker":{"options":[{"model":"x","label":"x"}]}}"#;
        let kept = render(mine, &req(false), "https://gw.example/v1", "k", &[], &Default::default(), &[]).unwrap();
        assert!(serde_json::from_str::<Value>(&kept).unwrap().get("modelPicker").is_some());
    }

    #[test]
    fn custom_headers_written_and_switched() {
        let h: crate::headers::Headers = [("User-Agent".to_string(), "example-client/1".to_string()), ("x-app".to_string(), "demo".to_string())].into();
        let on = render("", &req(false), "https://gw/v1", "k", &[], &h, &[]).unwrap();
        let v: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(v["env"]["ANTHROPIC_CUSTOM_HEADERS"], "User-Agent: example-client/1
x-app: demo");
        // Switching to a provider without headers drops ours...
        let owned = vec![crate::headers::to_env(&h)];
        let off = render(&on, &req(false), "https://other/v1", "k", &[], &Default::default(), &owned).unwrap();
        assert!(serde_json::from_str::<Value>(&off).unwrap()["env"].get("ANTHROPIC_CUSTOM_HEADERS").is_none());
        // ...but keeps a value the user wrote by hand.
        let mine = r#"{"env":{"ANTHROPIC_CUSTOM_HEADERS":"X-Mine: 1"}}"#;
        let kept = render(mine, &req(false), "https://other/v1", "k", &[], &Default::default(), &owned).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&kept).unwrap()["env"]["ANTHROPIC_CUSTOM_HEADERS"], "X-Mine: 1");
    }

    #[test]
    fn x_api_key_effort_and_errors() {
        let mut r = req(false);
        r.auth = Some("x-api-key".into());
        r.effort = Some("high".into());
        let out = render("\u{feff}{}", &r, "https://gw/v1", "k", &[], &Default::default(), &[]).unwrap();
        let cur = read_current(&out, &|n| (n == "ANTHROPIC_AUTH_TOKEN").then(|| "x".to_string()));
        assert_eq!(cur.auth, "x-api-key");
        assert_eq!(cur.effort, "high");
        assert_eq!(cur.warnings.len(), 1);
        assert!(render("[1]", &req(false), "u", "k", &[], &Default::default(), &[]).is_err());
        assert!(render("{bad", &req(false), "u", "k", &[], &Default::default(), &[]).is_err());
        assert!(render(r#"{"env":"x"}"#, &req(false), "u", "k", &[], &Default::default(), &[]).is_err());
        r.effort = Some("turbo".into());
        assert!(render("{}", &r, "u", "k", &[], &Default::default(), &[]).is_err());
    }
}
