//! Custom HTTP headers a gateway asks for, stored per provider.
//!
//! Some gateways pick a code path from request headers. For example, one sends Claude
//! Code requests to a branch that accepts the `reasoning_effort` field only when a
//! client header is present. The values belong to the gateway, so the tool never
//! ships any; the user (or a preset from the gateway) supplies them.
//!
//! Written to:
//! - Claude Code: `env.ANTHROPIC_CUSTOM_HEADERS` in settings.json, one
//!   `Name: value` per line.
//! - Claude Desktop (3P): `inferenceCustomHeaders` as an object in the
//!   configLibrary entry; the app turns it into the same env var for the
//!   Claude Code it runs.
//!
//! Header names that look like credentials are refused. A key belongs in the
//! keychain, and Claude Desktop also warns about such names.

use std::collections::BTreeMap;

pub type Headers = BTreeMap<String, String>;

pub const MAX: usize = 20;

/// Header name shapes we refuse: these carry credentials or decide routing,
/// which the tool already sets (base URL, key header).
const BLOCKED: &[&str] = &["authorization", "x-api-key", "api-key", "cookie", "proxy-authorization", "host", "content-length", "content-type"];
const SECRET_WORDS: &[&str] = &["token", "secret", "passw", "cookie", "apikey", "api-key", "api_key"];

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+.^_`|~-".contains(&b))
}

pub fn validate(h: &Headers) -> Result<(), String> {
    if h.len() > MAX {
        return Err(format!("At most {MAX} headers"));
    }
    for (name, value) in h {
        if !valid_name(name) {
            return Err(format!("Invalid header name: {name}"));
        }
        let low = name.to_ascii_lowercase();
        if BLOCKED.contains(&low.as_str()) || SECRET_WORDS.iter().any(|w| low.contains(w)) {
            return Err(format!("Header {name} looks like a credential or is set by the tool. Put the key in the provider's API key field."));
        }
        if value.trim().is_empty() || value.len() > 512 || value.chars().any(|c| c.is_control()) {
            return Err(format!("Invalid value for header {name}"));
        }
    }
    Ok(())
}

/// Parse `Name: value` lines. Blank lines and lines starting with # are skipped.
/// A repeated name is an error, not a silent overwrite.
pub fn parse(text: &str) -> Result<Headers, String> {
    let mut out = Headers::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once(':').ok_or_else(|| format!("Line {}: use Name: value", i + 1))?;
        let (name, value) = (name.trim().to_string(), value.trim().to_string());
        if out.keys().any(|k| k.eq_ignore_ascii_case(&name)) {
            return Err(format!("Header {name} appears twice"));
        }
        out.insert(name, value);
    }
    validate(&out)?;
    Ok(out)
}

/// Value of ANTHROPIC_CUSTOM_HEADERS.
pub fn to_env(h: &Headers) -> String {
    h.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_env() {
        let h = parse("User-Agent: example-client/1\n\n# note\nx-app: demo\n").unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(to_env(&h), "User-Agent: example-client/1\nx-app: demo");
    }

    #[test]
    fn refuses_credentials_and_bad_input() {
        assert!(parse("Authorization: Bearer x").is_err());
        assert!(parse("X-Auth-Token: x").is_err());
        assert!(parse("x-api-key: x").is_err());
        assert!(parse("no colon").is_err());
        assert!(parse("Bad Name: x").is_err());
        assert!(parse("a: 1\nA: 2").is_err());
        assert!(parse("a:   ").is_err());
        assert!(parse("").unwrap().is_empty());
    }
}
