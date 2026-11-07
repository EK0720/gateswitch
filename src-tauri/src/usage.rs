//! "Sử dụng": read token counts out of the logs the apps already keep.
//!
//! Read only, and only when the page is opened. No background work.
//! - Claude Code: ~/.claude/projects/**/*.jsonl, the `usage` block of assistant messages
//! - Codex: ~/.codex/sessions/**/rollout-*.jsonl, `token_count` events
//! - Orca: the same rollout format under its private Codex home

use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Totals {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DayStat {
    pub day: String,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Usage {
    pub claude: Totals,
    pub codex: Totals,
    pub orca: Totals,
    /// Oldest day first.
    pub days: Vec<DayStat>,
    pub by_model: BTreeMap<String, Totals>,
    /// Sessions read per app, so the page can say how much it covered.
    pub files: [u32; 3],
}

/// Assistant messages' usage. Claude Code writes one line per content block and
/// repeats the same `usage` on each, so a message counts once per `message.id`
/// (shared across files: resumed sessions copy earlier lines).
pub fn add_claude(text: &str, seen: &mut HashSet<String>, into: &mut Usage) {
    for line in text.lines() {
        if !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(u) = v.pointer("/message/usage") else { continue };
        let input = u.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
        let cached = u.get("cache_read_input_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
        let output = u.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0);
        if input == 0 && cached == 0 && output == 0 {
            continue;
        }
        let id = v.pointer("/message/id").and_then(|m| m.as_str()).unwrap_or("");
        let req = v.get("requestId").and_then(|m| m.as_str()).unwrap_or("");
        if !(id.is_empty() && req.is_empty()) && !seen.insert(format!("{id}|{req}")) {
            continue;
        }
        let model = v.pointer("/message/model").and_then(|m| m.as_str()).unwrap_or("").to_string();
        bump(into, 0, &model, v.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""), input, cached, output);
    }
}

/// Codex `token_count` events. `total_token_usage` is cumulative for the
/// session, so each event adds its delta over the previous one; that puts the
/// tokens on the day (and model) they were spent. The model comes from the most
/// recent `turn_context`, since `/model` can switch mid-session.
pub fn add_codex(text: &str, app: usize, into: &mut Usage) {
    let mut model = String::from("codex");
    let mut prev = (0u64, 0u64, 0u64);
    for line in text.lines() {
        let is_ctx = line.contains("\"turn_context\"");
        if !is_ctx && !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let p = v.get("payload").unwrap_or(&v);
        if v.get("type").and_then(|t| t.as_str()) == Some("turn_context") {
            if let Some(m) = p.get("model").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
                model = m.to_string();
            }
            continue;
        }
        if p.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(u) = p.pointer("/info/total_token_usage") else { continue };
        let n = |k: &str| u.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
        let cur = (n("input_tokens"), n("cached_input_tokens"), n("output_tokens"));
        // A smaller total means the counter restarted (compaction, new thread).
        let base = if cur.0 < prev.0 || cur.2 < prev.2 { (0, 0, 0) } else { prev };
        let d = (cur.0 - base.0, cur.1.saturating_sub(base.1), cur.2 - base.2);
        prev = cur;
        if d == (0, 0, 0) {
            continue;
        }
        let ts = v.get("timestamp").and_then(|t| t.as_str()).unwrap_or("");
        bump(into, app, &model, ts, d.0, d.1, d.2);
    }
}

fn bump(into: &mut Usage, app: usize, model: &str, ts: &str, input: u64, cached: u64, output: u64) {
    let totals = match app {
        0 => &mut into.claude,
        1 => &mut into.codex,
        _ => &mut into.orca,
    };
    totals.input += input;
    totals.cached += cached;
    totals.output += output;
    if !model.is_empty() && model != "<synthetic>" {
        let m = into.by_model.entry(model.to_string()).or_default();
        m.input += input;
        m.cached += cached;
        m.output += output;
    }
    if let Some(day) = ts.get(..10).filter(|d| d.chars().all(|c| c.is_ascii_digit() || c == '-')) {
        if let Some(d) = into.days.iter_mut().find(|d| d.day == day) {
            d.input += input;
            d.cached += cached;
            d.output += output;
        } else {
            into.days.push(DayStat { day: day.to_string(), input, cached, output });
        }
    }
}

// ------------------------------------------------------------------ file cache
//
// The logs reach many GB (one Orca/Codex rollout can be hundreds of MB), so a
// full read takes seconds in release and minutes in a debug build. Each file's
// contribution is cached by path + size + mtime; only new or changed files are
// read again. Claude records keep their message key so cross-file dedupe still
// works; Codex files are independent and stored pre-summed per model and day.

/// One contribution: (dedupe key or "", model, day, input, cached, output).
type Rec = (String, String, String, u64, u64, u64);

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Cache {
    v: u32,
    files: BTreeMap<String, Cached>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct Cached {
    size: u64,
    mtime: u64,
    app: usize,
    recs: Vec<Rec>,
}

const CACHE_VERSION: u32 = 2;

fn cache_path() -> std::path::PathBuf {
    crate::data_dir().join("usage-cache.json")
}

/// Records from one file, without cross-file dedupe (done when summing).
fn file_recs(text: &str, app: usize) -> Vec<Rec> {
    let mut out = Vec::new();
    if app == 0 {
        for line in text.lines() {
            if !line.contains("\"usage\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Some(us) = v.pointer("/message/usage") else { continue };
            let n = |k: &str| us.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
            let (i, c, o) = (n("input_tokens"), n("cache_read_input_tokens"), n("output_tokens"));
            if i == 0 && c == 0 && o == 0 {
                continue;
            }
            let id = v.pointer("/message/id").and_then(|m| m.as_str()).unwrap_or("");
            let req = v.get("requestId").and_then(|m| m.as_str()).unwrap_or("");
            let key = if id.is_empty() && req.is_empty() { String::new() } else { format!("{id}|{req}") };
            let model = v.pointer("/message/model").and_then(|m| m.as_str()).unwrap_or("").to_string();
            let day = day_of(v.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""));
            out.push((key, model, day, i, c, o));
        }
    } else {
        // Reuse the delta logic, then flatten its per-model / per-day sums.
        // A file's days and models are not crossed, so sum per (model, day) directly.
        let mut per: BTreeMap<(String, String), (u64, u64, u64)> = BTreeMap::new();
        let mut model = String::from("codex");
        let mut prev = (0u64, 0u64, 0u64);
        for line in text.lines() {
            let is_ctx = line.contains("\"turn_context\"");
            if !is_ctx && !line.contains("\"token_count\"") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let p = v.get("payload").unwrap_or(&v);
            if v.get("type").and_then(|t| t.as_str()) == Some("turn_context") {
                if let Some(m) = p.get("model").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
                    model = m.to_string();
                }
                continue;
            }
            if p.get("type").and_then(|t| t.as_str()) != Some("token_count") {
                continue;
            }
            let Some(us) = p.pointer("/info/total_token_usage") else { continue };
            let n = |k: &str| us.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
            let cur = (n("input_tokens"), n("cached_input_tokens"), n("output_tokens"));
            let base = if cur.0 < prev.0 || cur.2 < prev.2 { (0, 0, 0) } else { prev };
            let d = (cur.0 - base.0, cur.1.saturating_sub(base.1), cur.2 - base.2);
            prev = cur;
            if d == (0, 0, 0) {
                continue;
            }
            let day = day_of(v.get("timestamp").and_then(|t| t.as_str()).unwrap_or(""));
            let e = per.entry((model.clone(), day)).or_default();
            e.0 += d.0;
            e.1 += d.1;
            e.2 += d.2;
        }
        out = per.into_iter().map(|((m, d), (i, c, o))| (String::new(), m, d, i, c, o)).collect();
    }
    out
}

fn day_of(ts: &str) -> String {
    ts.get(..10).filter(|d| d.chars().all(|c| c.is_ascii_digit() || c == '-')).unwrap_or("").to_string()
}

fn walk(dir: &Path, suffix: &str, mut on_file: impl FnMut(&Path)) {
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0u32;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some(suffix) && seen < 4000 {
                seen += 1;
                on_file(&p);
            }
        }
    }
}

pub fn collect() -> Usage {
    let cache_file = cache_path();
    let old: Cache = std::fs::read(&cache_file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .filter(|c: &Cache| c.v == CACHE_VERSION)
        .unwrap_or_default();
    let mut next = Cache { v: CACHE_VERSION, files: BTreeMap::new() };

    let mut dirs: Vec<(std::path::PathBuf, usize)> = vec![
        (crate::claude::claude_home().join("projects"), 0),
        (crate::codex::codex_home().join("sessions"), 1),
    ];
    if let Some(home) = crate::orca::home() {
        dirs.push((home.join("sessions"), 2));
    }
    let mut files = [0u32; 3];
    for (dir, app) in dirs {
        walk(&dir, "jsonl", |p| {
            let Ok(meta) = std::fs::metadata(p) else { return };
            let size = meta.len();
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            // Hash, not the path: folder names often carry project or user names.
            let key = { use sha2::Digest; let h = sha2::Sha256::digest(p.display().to_string().as_bytes()); h.iter().take(12).map(|b| format!("{b:02x}")).collect::<String>() };
            let hit = old.files.get(&key).filter(|c| c.size == size && c.mtime == mtime && c.app == app).cloned();
            let entry = match hit {
                Some(c) => c,
                None => {
                    // Lossy: one bad byte must not drop a whole session.
                    let Ok(bytes) = std::fs::read(p) else { return };
                    Cached { size, mtime, app, recs: file_recs(&String::from_utf8_lossy(&bytes), app) }
                }
            };
            files[app] += 1;
            next.files.insert(key, entry);
        });
    }

    let mut u = Usage { claude: Totals::default(), codex: Totals::default(), orca: Totals::default(), days: vec![], by_model: BTreeMap::new(), files };
    let mut seen = HashSet::new();
    for c in next.files.values() {
        for (key, model, day, i, cached, o) in &c.recs {
            if c.app == 0 && !key.is_empty() && !seen.insert(key.as_str()) {
                continue;
            }
            bump(&mut u, c.app, model, day, *i, *cached, *o);
        }
    }
    u.days.sort_by(|a, b| a.day.cmp(&b.day));

    if let Ok(bytes) = serde_json::to_vec(&next) {
        let _ = std::fs::create_dir_all(cache_file.parent().unwrap_or(Path::new(".")));
        let tmp = cache_file.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &cache_file);
        }
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_usage_sums_and_skips_empty() {
        let text = r#"{"timestamp":"2026-11-06T03:00:00Z","message":{"model":"cc/claude-sonnet","usage":{"input_tokens":100,"cache_read_input_tokens":40,"output_tokens":10}}}
{"timestamp":"2026-11-06T04:00:00Z","message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}
{"timestamp":"2026-11-05T04:00:00Z","message":{"model":"cc/claude-sonnet","usage":{"input_tokens":5,"cache_read_input_tokens":0,"output_tokens":2}}}"#;
        let mut u = empty();
        add_claude(text, &mut HashSet::new(), &mut u);
        assert_eq!(u.claude, Totals { input: 105, cached: 40, output: 12 });
        assert_eq!(u.by_model["cc/claude-sonnet"].input, 105);
        assert_eq!(u.days.len(), 2);
    }

    #[test]
    fn codex_counts_deltas_per_day_and_model() {
        let text = r#"{"timestamp":"2026-11-05T01:00:00Z","type":"turn_context","payload":{"model":"cx/a"}}
{"timestamp":"2026-11-05T01:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":20}}}}
{"timestamp":"2026-11-05T01:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":20}}}}
{"timestamp":"2026-11-06T02:00:00Z","type":"turn_context","payload":{"model":"cx/b"}}
{"timestamp":"2026-11-06T02:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2800,"output_tokens":60}}}}"#;
        let mut u = empty();
        add_codex(text, 1, &mut u);
        assert_eq!(u.codex, Totals { input: 3000, cached: 2800, output: 60 });
        assert_eq!(u.by_model["cx/a"].input, 1000);
        assert_eq!(u.by_model["cx/b"].input, 2000);
        assert_eq!(u.days.len(), 2);
    }

    #[test]
    fn claude_repeated_message_counts_once() {
        let line = r#"{"timestamp":"2026-11-06T03:00:00Z","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":7,"output_tokens":3}}}"#;
        let mut u = empty();
        let mut seen = HashSet::new();
        add_claude(&format!("{line}
{line}"), &mut seen, &mut u);
        add_claude(line, &mut seen, &mut u);
        assert_eq!(u.claude, Totals { input: 7, cached: 0, output: 3 });
    }

    fn empty() -> Usage {
        Usage { claude: Totals::default(), codex: Totals::default(), orca: Totals::default(), days: vec![], by_model: BTreeMap::new(), files: [0, 0, 0] }
    }

    #[test]
    fn cached_records_match_direct_sums() {
        let claude = r#"{"timestamp":"2026-11-06T03:00:00Z","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":7,"output_tokens":3}}}
{"timestamp":"2026-11-06T03:00:00Z","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":7,"output_tokens":3}}}"#;
        let recs = file_recs(claude, 0);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].0, recs[1].0, "same key so the sum dedupes them");
        let codex = r#"{"timestamp":"2026-11-05T01:00:00Z","type":"turn_context","payload":{"model":"cx/a"}}
{"timestamp":"2026-11-05T01:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":20}}}}
{"timestamp":"2026-11-06T02:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2800,"output_tokens":60}}}}"#;
        let mut direct = empty();
        add_codex(codex, 1, &mut direct);
        let recs = file_recs(codex, 1);
        let sum = recs.iter().fold((0, 0, 0), |a, r| (a.0 + r.3, a.1 + r.4, a.2 + r.5));
        assert_eq!(sum, (direct.codex.input, direct.codex.cached, direct.codex.output));
        assert_eq!(recs.len(), 2, "one per (model, day)");
    }

    /// `cargo test --release -- --ignored usage_live --nocapture`
    #[test]
    #[ignore]
    fn usage_live() {
        for _ in 0..2 {
            let t = std::time::Instant::now();
            let u = collect();
            println!("files {:?} in {:?}; in: claude {} codex {} orca {}", u.files, t.elapsed(), u.claude.input, u.codex.input, u.orca.input);
        }
    }
}
