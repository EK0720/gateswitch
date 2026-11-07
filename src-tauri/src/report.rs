//! Support report: a JSON snapshot the user can copy into a bug report.
//!
//! Never contains a key. Configs are read through the same `read_current`
//! helpers the pages use (they never return secrets), provider records hold no
//! key, and as a last guard every stored key and anything shaped like one is
//! replaced with `***` before the text leaves this module.

use crate::secrets::{Env, Vault};
use serde_json::{json, Value};

pub fn build() -> String {
    let scan = crate::detect::scan();
    let claude_text = std::fs::read_to_string(crate::claude::settings_path()).unwrap_or_default();
    let claude = crate::claude::read_current(&claude_text, &|n| crate::secrets::UserEnv.get(n));
    let codex = crate::codex::read_current(&std::fs::read_to_string(crate::codex::config_path()).unwrap_or_default());
    let desktop = crate::desktop::read_current(crate::desktop::library_dir().as_deref());
    let providers = crate::providers().list(&crate::secrets::Keychain);
    let history: Vec<Value> = crate::store()
        .history()
        .into_iter()
        .take(15)
        .map(|c| json!({ "time": c.time, "title": c.title, "detail": c.detail, "undone": c.undone }))
        .collect();
    let report = json!({
        "app": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "time": chrono::Local::now().to_rfc3339(),
        "tools": scan.tools,
        "doctor": crate::doctor::run(),
        "claude_code": claude,
        "codex": codex,
        "claude_desktop": desktop,
        "orca": crate::orca::status(),
        "providers": providers.iter().map(|p| json!({
            "name": p.name, "base_url": p.base_url, "models": p.models.len(), "has_key": p.has_key,
            "probes": p.probes.iter().map(|(m, r)| (m.clone(), json!(r.status))).collect::<serde_json::Map<_, _>>(),
        })).collect::<Vec<_>>(),
        "history": history,
        "restart_log": crate::rlog::tail(60),
    });
    let text = serde_json::to_string_pretty(&report).unwrap_or_default();
    let keys: Vec<String> = providers
        .iter()
        .filter_map(|p| crate::secrets::Keychain.get(&crate::providers::account(&p.id)))
        .collect();
    scrub(&text, &keys)
}

/// Replace known secrets, then anything that looks like an API key, then the
/// user's home folder (it usually contains their real name).
pub fn scrub(text: &str, known: &[String]) -> String {
    let mut out = text.to_string();
    for k in known.iter().filter(|k| k.len() >= 8) {
        out = out.replace(k.as_str(), "***");
    }
    out = mask_key_like(&out);
    if let Some(home) = dirs::home_dir() {
        let h = home.display().to_string();
        if h.len() > 3 {
            out = out.replace(&h.replace('\\', "\\\\"), "~").replace(&h, "~");
        }
    }
    out
}

/// `sk-…`, `sk-ant-…` and long bearer-looking tokens.
fn mask_key_like(text: &str) -> String {
    let token = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |w: &mut String, out: &mut String| {
        let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
        let looks_key = w.starts_with("sk-") && w.len() >= 12
            || (w.len() >= 32 && digits >= 4 && w.chars().any(|c| c.is_ascii_uppercase()) && w.chars().any(|c| c.is_ascii_lowercase()));
        out.push_str(if looks_key { "***" } else { w });
        w.clear();
    };
    for c in text.chars() {
        if token(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_key_like_values_are_masked() {
        let t = r#"{"a":"my-secret-value-123","b":"sk-abcdefghijklmnop","c":"cc/claude-opus-5-5","d":"AbCdEf1234GhIjKl5678MnOpQr9012StUv"}"#;
        let s = scrub(t, &["my-secret-value-123".into()]);
        assert!(!s.contains("my-secret"));
        assert!(!s.contains("sk-abc"));
        assert!(!s.contains("AbCdEf"));
        assert!(s.contains("cc/claude-opus-5-5"));
    }

    #[test]
    #[ignore]
    fn print_report() {
        println!("{}", build());
    }
}
