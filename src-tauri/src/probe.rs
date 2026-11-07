//! "Kiểm tra model": send one tiny request per model and classify the reply.
//!
//! Only runs when the user clicks. Each request asks for at most 16 tokens.
//! The API key never appears in a result: every message goes through `mask`.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub const WORKERS: usize = 4;
pub const TIMEOUT_SECS: u64 = 30;
const MAX_TOKENS: u32 = 16;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Quota,
    Auth,
    Missing,
    Upstream,
    Timeout,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Probe {
    pub status: Status,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    /// Short reason from the gateway, key masked. Empty when ok.
    #[serde(default)]
    pub message: String,
    pub checked_at: String,
}

/// Hide the key and anything that looks like a bearer token.
pub fn mask(text: &str, key: &str) -> String {
    let mut s = text.to_string();
    let key = key.trim();
    if key.len() >= 4 {
        s = s.replace(key, &short(key));
    }
    // Generic tokens: sk-..., Bearer xxx, long opaque runs.
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let core = word.trim_matches(|c: char| "\"'.,;:()[]{}<>".contains(c));
        let tokenish = core.len() >= 20
            && core.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
            && core.chars().any(|c| c.is_ascii_digit())
            && core.chars().any(|c| c.is_ascii_alphabetic());
        if (core.starts_with("sk-") && core.len() > 8) || tokenish {
            out.push_str(&word.replace(core, &short(core)));
        } else {
            out.push_str(word);
        }
    }
    out
}

fn short(token: &str) -> String {
    let tail: String = token.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("•••{tail}")
}

fn is_claude(model: &str) -> bool {
    model.to_lowercase().contains("claude")
}

fn quota_words(msg: &str) -> bool {
    let m = msg.to_lowercase();
    ["quota", "rate limit", "rate_limit", "ratelimit", "exhausted", "insufficient", "usage limit", "limit reached", "limit exceeded", "credit", "balance"]
        .iter()
        .any(|w| m.contains(w))
}

/// Pull the human-readable error out of a JSON or plain body.
fn error_text(body: &str) -> String {
    let v: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let msg = v.as_ref().and_then(|v| {
        let e = v.get("error").unwrap_or(v);
        e.get("message").and_then(|m| m.as_str()).map(String::from).or_else(|| e.as_str().map(String::from))
    });
    let msg = msg.unwrap_or_else(|| body.trim().to_string());
    let msg: String = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    if msg.chars().count() > 200 {
        format!("{}…", msg.chars().take(200).collect::<String>())
    } else {
        msg
    }
}

/// Classify a finished HTTP exchange.
pub fn classify(code: u16, body: &str) -> (Status, String) {
    let msg = error_text(body);
    if (200..300).contains(&code) {
        // Some gateways answer 200 with an error object.
        let has_error = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .map(|v| v.get("error").map(|e| !e.is_null()).unwrap_or(false))
            .unwrap_or(false);
        if !has_error {
            return (Status::Ok, String::new());
        }
        return (if quota_words(&msg) { Status::Quota } else { Status::Error }, msg);
    }
    let status = match code {
        429 | 402 => Status::Quota,
        _ if quota_words(&msg) => Status::Quota,
        401 | 403 => Status::Auth,
        404 => Status::Missing,
        500..=599 => Status::Upstream,
        _ => Status::Error,
    };
    let msg = if msg.is_empty() { format!("HTTP {code}") } else { format!("HTTP {code}: {msg}") };
    (status, msg)
}

enum Reply {
    Http(u16, String),
    Timeout,
    Failed(String),
}

fn post(agent: &ureq::Agent, url: &str, key: &str, body: serde_json::Value) -> Reply {
    let res = agent
        .post(url)
        .header("Authorization", &format!("Bearer {key}"))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("Accept", "application/json, text/event-stream")
        // Some gateways sit behind Cloudflare rules that block unknown clients.
        .header("User-Agent", concat!("model-switcher/", env!("CARGO_PKG_VERSION")))
        .send_json(body);
    match res {
        Ok(mut r) => {
            let code = r.status().as_u16();
            let text = r.body_mut().with_config().limit(256 * 1024).read_to_string().unwrap_or_default();
            Reply::Http(code, text)
        }
        Err(ureq::Error::Timeout(_)) => Reply::Timeout,
        Err(e) => {
            let s = e.to_string();
            if s.to_lowercase().contains("timed out") || s.to_lowercase().contains("timeout") {
                Reply::Timeout
            } else {
                Reply::Failed(s)
            }
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
        .http_status_as_error(false)
        .build()
        .into()
}

/// Probe one model. Claude ids use /messages; others /chat/completions, then
/// /responses if the gateway does not route that model through chat.
pub fn probe_one(agent: &ureq::Agent, base_url: &str, key: &str, model: &str) -> Probe {
    let ping = serde_json::json!([{ "role": "user", "content": "ping" }]);
    let started = Instant::now();
    let reply = if is_claude(model) {
        post(agent, &format!("{base_url}/messages"), key, serde_json::json!({ "model": model, "max_tokens": MAX_TOKENS, "messages": ping }))
    } else {
        let first = post(agent, &format!("{base_url}/chat/completions"), key, serde_json::json!({ "model": model, "max_tokens": MAX_TOKENS, "messages": ping }));
        match first {
            Reply::Http(400 | 404 | 405, ref body) if !quota_words(body) => {
                match post(agent, &format!("{base_url}/responses"), key, serde_json::json!({ "model": model, "max_output_tokens": MAX_TOKENS, "input": "ping" })) {
                    r @ Reply::Http(200..=299, _) => r,
                    _ => first,
                }
            }
            r => r,
        }
    };
    let ms = started.elapsed().as_millis() as u64;
    let (status, message, latency_ms) = match reply {
        Reply::Http(code, body) => {
            let (s, m) = classify(code, &body);
            (s, m, if s == Status::Ok { Some(ms) } else { None })
        }
        Reply::Timeout => (Status::Timeout, format!("No response after {TIMEOUT_SECS} seconds"), None),
        Reply::Failed(e) => (Status::Error, format!("Could not connect: {e}"), None),
    };
    Probe { status, latency_ms, message: mask(&message, key), checked_at: chrono::Local::now().to_rfc3339() }
}

// ---------------------------------------------------------------- capabilities

/// One capability result: None = not tested (e.g. not applicable).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Check {
    pub ok: bool,
    #[serde(default)]
    pub latency_ms: Option<u64>,
    /// Short reason, key masked. Empty when ok.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Caps {
    /// The endpoint the app uses: /responses for Codex models, /messages for Claude.
    pub endpoint: Option<Check>,
    pub streaming: Option<Check>,
    pub tools: Option<Check>,
    /// Second identical request reports cached tokens (> 0).
    pub caching: Option<Check>,
    pub images: Option<Check>,
}

/// 1x1 transparent PNG.
const PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn check_of(reply: Reply, started: Instant, key: &str, ok_if: impl Fn(&str) -> Result<(), String>) -> Check {
    let ms = started.elapsed().as_millis() as u64;
    match reply {
        Reply::Http(200..=299, body) => match ok_if(&body) {
            Ok(()) => Check { ok: true, latency_ms: Some(ms), note: String::new() },
            Err(why) => Check { ok: false, latency_ms: None, note: mask(&why, key) },
        },
        Reply::Http(code, body) => Check { ok: false, latency_ms: None, note: mask(&classify(code, &body).1, key) },
        Reply::Timeout => Check { ok: false, latency_ms: None, note: format!("No response after {TIMEOUT_SECS} seconds") },
        Reply::Failed(e) => Check { ok: false, latency_ms: None, note: mask(&format!("Could not connect: {e}"), key) },
    }
}

/// A long, stable prefix so a second identical call can hit the prompt cache
/// (providers only cache prompts above ~1024 tokens).
fn cache_prompt() -> String {
    let line = "Model Switcher cache probe. This paragraph is repeated to exceed the minimum cacheable prompt length. ";
    line.repeat(70)
}

fn cached_tokens(body: &str) -> u64 {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let u = v.get("usage").cloned().unwrap_or_default();
    u.get("cache_read_input_tokens")
        .or_else(|| u.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| u.pointer("/prompt_tokens_details/cached_tokens"))
        .and_then(|n| n.as_u64())
        .unwrap_or(0)
}

pub fn caps_one(agent: &ureq::Agent, base: &str, key: &str, model: &str) -> Caps {
    let claude = is_claude(model);
    let user = |content: serde_json::Value| serde_json::json!([{ "role": "user", "content": content }]);
    let tool_def = if claude {
        serde_json::json!([{ "name": "get_time", "description": "Current time", "input_schema": { "type": "object", "properties": {} } }])
    } else {
        serde_json::json!([{ "type": "function", "name": "get_time", "description": "Current time", "parameters": { "type": "object", "properties": {} } }])
    };
    // Claude ids go to /messages, the rest to /responses (what Codex uses).
    let call = |extra: serde_json::Value, input: serde_json::Value| -> Reply {
        let mut body = if claude {
            serde_json::json!({ "model": model, "max_tokens": 64, "messages": input })
        } else {
            serde_json::json!({ "model": model, "max_output_tokens": 64, "input": input })
        };
        if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                b.insert(k.clone(), v.clone());
            }
        }
        post(agent, &format!("{base}/{}", if claude { "messages" } else { "responses" }), key, body)
    };

    let t = Instant::now();
    let endpoint = check_of(call(serde_json::json!({}), user(serde_json::json!("Reply OK."))), t, key, |_| Ok(()));
    if !endpoint.ok {
        // No point trying the rest when the basic call fails.
        return Caps { endpoint: Some(endpoint), ..Default::default() };
    }

    let t = Instant::now();
    let streaming = check_of(call(serde_json::json!({ "stream": true }), user(serde_json::json!("Reply OK."))), t, key, |b| {
        if b.contains("data:") || b.contains("event:") { Ok(()) } else { Err("Gateway did not return a stream (SSE)".into()) }
    });

    let t = Instant::now();
    let tools = check_of(
        call(serde_json::json!({ "tools": tool_def, "tool_choice": if claude { serde_json::json!({ "type": "any" }) } else { serde_json::json!("required") } }), user(serde_json::json!("What time is it? Use the tool."))),
        t,
        key,
        |b| if b.contains("tool_use") || b.contains("function_call") { Ok(()) } else { Err("Model did not call the tool".into()) },
    );

    let prompt = cache_prompt();
    let cache_input = |p: &str| {
        if claude {
            user(serde_json::json!([{ "type": "text", "text": p, "cache_control": { "type": "ephemeral" } }, { "type": "text", "text": "Reply OK." }]))
        } else {
            user(serde_json::json!(format!("{p}\nReply OK.")))
        }
    };
    let _ = call(serde_json::json!({}), cache_input(&prompt));
    let t = Instant::now();
    let caching = check_of(call(serde_json::json!({}), cache_input(&prompt)), t, key, |b| {
        if cached_tokens(b) > 0 { Ok(()) } else { Err("The second call had no cached tokens: the gateway breaks prompt caching".into()) }
    });

    let image = if claude {
        serde_json::json!([{ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": PIXEL } }, { "type": "text", "text": "What color is this pixel? One word." }])
    } else {
        serde_json::json!([{ "type": "input_image", "image_url": format!("data:image/png;base64,{PIXEL}") }, { "type": "input_text", "text": "What color is this pixel? One word." }])
    };
    let t = Instant::now();
    let images = check_of(call(serde_json::json!({}), user(image)), t, key, |_| Ok(()));

    Caps { endpoint: Some(endpoint), streaming: Some(streaming), tools: Some(tools), caching: Some(caching), images: Some(images) }
}

/// Capability test for a handful of models, two at a time.
pub fn capabilities(base_url: &str, key: &str, models: Vec<String>) -> Vec<(String, Caps)> {
    let agent = agent();
    let mut out = vec![];
    for chunk in models.chunks(2) {
        let handles: Vec<_> = chunk
            .iter()
            .map(|m| {
                let (agent, base, key, m) = (agent.clone(), base_url.to_string(), key.to_string(), m.clone());
                std::thread::spawn(move || {
                    let c = caps_one(&agent, &base, &key, &m);
                    (m, c)
                })
            })
            .collect();
        for h in handles {
            if let Ok(r) = h.join() {
                out.push(r);
            }
        }
    }
    out
}

/// Probe many models with at most `WORKERS` requests in flight.
pub fn probe_all(base_url: &str, key: &str, models: Vec<String>) -> Vec<(String, Probe)> {
    let queue = Arc::new(Mutex::new(models.into_iter().collect::<VecDeque<_>>()));
    let (tx, rx) = mpsc::channel();
    let agent = agent();
    let mut handles = vec![];
    for _ in 0..WORKERS {
        let (queue, tx, agent) = (queue.clone(), tx.clone(), agent.clone());
        let (base, key) = (base_url.to_string(), key.to_string());
        handles.push(std::thread::spawn(move || loop {
            let next = queue.lock().unwrap().pop_front();
            let Some(model) = next else { break };
            let r = probe_one(&agent, &base, &key, &model);
            let _ = tx.send((model, r));
        }));
    }
    drop(tx);
    let out: Vec<_> = rx.into_iter().collect();
    for h in handles {
        let _ = h.join();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const KEY: &str = "sk-test-SECRET1234567890abcd";

    #[test]
    fn classify_covers_spec() {
        assert_eq!(classify(200, r#"{"choices":[]}"#).0, Status::Ok);
        assert_eq!(classify(429, "").0, Status::Quota);
        assert_eq!(classify(402, "").0, Status::Quota);
        assert_eq!(classify(400, r#"{"error":{"message":"Quota exhausted for account"}}"#).0, Status::Quota);
        assert_eq!(classify(500, r#"{"error":"insufficient balance"}"#).0, Status::Quota);
        assert_eq!(classify(200, r#"{"error":{"message":"usage limit reached"}}"#).0, Status::Quota);
        assert_eq!(classify(401, "").0, Status::Auth);
        assert_eq!(classify(403, "forbidden").0, Status::Auth);
        assert_eq!(classify(404, "").0, Status::Missing);
        assert_eq!(classify(502, "bad gateway").0, Status::Upstream);
        assert_eq!(classify(400, r#"{"error":{"message":"bad param"}}"#), (Status::Error, "HTTP 400: bad param".into()));
    }

    #[test]
    fn mask_hides_keys() {
        let m = mask(&format!("invalid key {KEY} (Bearer {KEY})"), KEY);
        assert!(!m.contains("SECRET"), "{m}");
        assert!(m.contains("•••abcd"));
        let other = mask("token sk-otherkey12345678 and abcdefghij0123456789xyz", KEY);
        assert!(!other.contains("otherkey"), "{other}");
        assert!(!other.contains("abcdefghij0123456789"), "{other}");
        assert_eq!(mask("model gpt-6.1-sol not found", KEY), "model gpt-6.1-sol not found");
    }

    /// Minimal HTTP server: answers by model name and counts concurrency.
    fn serve(peak: Arc<AtomicUsize>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        std::thread::spawn(move || {
            for s in l.incoming() {
                let mut s = s.unwrap();
                let (live, peak) = (live.clone(), peak.clone());
                std::thread::spawn(move || {
                    let n = live.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(n, Ordering::SeqCst);
                    let mut r = BufReader::new(s.try_clone().unwrap());
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    let path = line.split(' ').nth(1).unwrap_or("").to_string();
                    let mut len = 0;
                    loop {
                        let mut h = String::new();
                        r.read_line(&mut h).unwrap();
                        if h == "\r\n" || h.is_empty() { break; }
                        if let Some(v) = h.to_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap(); }
                    }
                    let mut body = vec![0; len];
                    r.read_exact(&mut body).unwrap();
                    let body = String::from_utf8(body).unwrap();
                    std::thread::sleep(Duration::from_millis(80));
                    let model = serde_json::from_str::<serde_json::Value>(&body).unwrap()["model"].as_str().unwrap().to_string();
                    let (code, out) = match (model.as_str(), path.as_str()) {
                        ("claude-ok", "/v1/messages") => (200, r#"{"content":[]}"#.to_string()),
                        ("gpt-ok", "/v1/chat/completions") => (200, r#"{"choices":[]}"#.to_string()),
                        ("codex-only", "/v1/chat/completions") => (400, r#"{"error":{"message":"use responses"}}"#.to_string()),
                        ("codex-only", "/v1/responses") => (200, r#"{"output":[]}"#.to_string()),
                        ("cx-quota", _) => (429, r#"{"error":{"message":"rate limited"}}"#.to_string()),
                        ("leak", _) => (401, format!(r#"{{"error":{{"message":"bad key {KEY}"}}}}"#)),
                        _ => (404, r#"{"error":{"message":"model not found"}}"#.to_string()),
                    };
                    let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
                    live.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        format!("http://{addr}/v1")
    }

    #[test]
    fn probe_all_against_mock() {
        let peak = Arc::new(AtomicUsize::new(0));
        let base = serve(peak.clone());
        let mut models: Vec<String> = ["claude-ok", "gpt-ok", "codex-only", "cx-quota", "leak", "nope"].map(String::from).to_vec();
        for i in 0..6 { models.push(format!("gpt-ok-{i}")); }
        let res: std::collections::HashMap<_, _> = probe_all(&base, KEY, models).into_iter().collect();
        assert_eq!(res.len(), 12);
        assert_eq!(res["claude-ok"].status, Status::Ok);
        assert!(res["claude-ok"].latency_ms.is_some());
        assert_eq!(res["gpt-ok"].status, Status::Ok);
        assert_eq!(res["codex-only"].status, Status::Ok);
        assert_eq!(res["cx-quota"].status, Status::Quota);
        assert_eq!(res["nope"].status, Status::Missing);
        assert_eq!(res["leak"].status, Status::Auth);
        assert!(!res["leak"].message.contains("SECRET"), "{}", res["leak"].message);
        assert!(peak.load(Ordering::SeqCst) <= WORKERS, "peak {}", peak.load(Ordering::SeqCst));
    }
}
