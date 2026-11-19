//! "Usage": read token counts out of the logs the apps already keep.
//!
//! Read only, and only on demand: when the Usage page asks for numbers, by the
//! opt-in watcher (`usage_watch`) while the page or the mini window is open, and
//! by the optional tray icon, mini window and block alerts (`usage_live`), all
//! off by default. No other background work.
//! - Claude Code: ~/.claude/projects/**/*.jsonl (subagents under <session>/subagents/),
//!   the `usage` block of assistant messages
//! - Codex: ~/.codex/sessions/**/rollout-*.jsonl, `token_count` events
//! - Orca: the same rollout format under its private Codex home
//!
//! Every app is normalised to four disjoint kinds, so sums and prices work the
//! same everywhere:
//! - `input`: input tokens not served from cache
//! - `cached`: input tokens read from the prompt cache
//! - `cache_write`: input tokens written to the prompt cache
//! - `output`: output tokens
//!
//! Claude's `input_tokens` already excludes both cache kinds; Codex's
//! `input_tokens` includes `cached_input_tokens`, so Codex subtracts it.
//!
//! Logs are append-only, so each file keeps a byte offset in the cache and only
//! the new complete lines are read next time. Times are stored as UTC hours and
//! turned into local days/hours when a report is built.
//!
//! Privacy: project labels and paths stay in the cache and in replies to the
//! page. They never go into logs, the support report or error strings; cache
//! keys are hashes of the file path.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum App {
    Claude,
    Codex,
    Orca,
}

impl App {
    pub const ALL: [App; 3] = [App::Claude, App::Codex, App::Orca];
    pub fn idx(self) -> usize {
        self as usize
    }
    fn from_idx(i: u8) -> App {
        match i {
            0 => App::Claude,
            1 => App::Codex,
            _ => App::Orca,
        }
    }
    pub fn name(self) -> &'static str {
        ["claude", "codex", "orca"][self.idx()]
    }
    pub fn title(self) -> &'static str {
        ["Claude Code", "Codex", "Orca"][self.idx()]
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Totals {
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Totals {
    pub fn sum(&self) -> u64 {
        self.input + self.cached + self.cache_write + self.output
    }
    pub fn add(&mut self, o: &Totals) {
        self.input += o.input;
        self.cached += o.cached;
        self.cache_write += o.cache_write;
        self.output += o.output;
    }
}

/// Totals plus the number of model calls.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
pub struct Counted {
    #[serde(flatten)]
    pub t: Totals,
    pub turns: u64,
}

impl Counted {
    pub fn add(&mut self, t: &Totals, turns: u64) {
        self.t.add(t);
        self.turns += turns;
    }
}

// ------------------------------------------------------------------ on-disk cache

/// One Claude assistant message: key hash (0 = no key), UTC seconds, thread
/// (index into the file's threads), model (index into the file's models),
/// input, cache read, cache write, output. A tuple keeps the cache small.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Msg(u64, i64, u32, u32, u64, u64, u64, u64);

/// Codex tokens of one (UTC hour, model) in the file's thread.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Bk {
    /// Unix hour (seconds / 3600).
    h: i64,
    m: u32,
    /// Turns.
    n: u64,
    i: u64,
    r: u64,
    w: u64,
    o: u64,
    /// Largest single-turn context.
    c: u64,
    /// Cache expiries.
    x: u64,
    /// Cache-write tokens of those expiry turns.
    #[serde(default)]
    xw: u64,
}

/// A thread (main conversation or subagent/worker) seen in a file.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Th {
    /// Claude: `sessionId` or `sessionId/agentId`. Codex: the thread id.
    id: String,
    /// Claude: `sessionId`. Codex: the thread id (sessions are resolved via `parent`).
    sess: String,
    #[serde(default)]
    parent: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    sub: bool,
    /// Subagent description or worker nickname.
    #[serde(default)]
    label: String,
    /// Claude custom title.
    #[serde(default)]
    title: String,
    #[serde(default)]
    first: i64,
    #[serde(default)]
    last: i64,
}

/// Codex running state, persisted with the offset so deltas survive incremental reads.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
struct Cx {
    /// Cumulative input, cached, cache write, output at the offset.
    prev: [u64; 4],
    started: bool,
    model: String,
    last_turn: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct FileState {
    size: u64,
    mtime: u64,
    app: u8,
    /// Bytes consumed (always just after a '\n').
    off: u64,
    /// Length and hash of the first bytes, to notice a file replaced by another.
    hl: u64,
    head: u64,
    models: Vec<String>,
    th: Vec<Th>,
    #[serde(default)]
    msgs: Vec<Msg>,
    #[serde(default)]
    bk: Vec<Bk>,
    #[serde(default)]
    cx: Cx,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    v: u32,
    files: BTreeMap<String, FileState>,
}

/// 4: UTC hours, threads, byte offsets, per-message Claude rows.
const CACHE_VERSION: u32 = 4;
const HEAD: u64 = 64;
/// A turn more than this long after the previous one in the same thread finds the cache gone.
const CACHE_TTL: i64 = 300;

fn fnv(b: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn parse_ts(v: Option<&serde_json::Value>) -> Option<i64> {
    let s = v?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp())
}

fn intern(models: &mut Vec<String>, m: &str) -> u32 {
    if let Some(i) = models.iter().position(|x| x == m) {
        return i as u32;
    }
    models.push(m.to_string());
    (models.len() - 1) as u32
}

fn touch(t: &mut Th, ts: i64) {
    if t.first == 0 || ts < t.first {
        t.first = ts;
    }
    t.last = t.last.max(ts);
}

fn short(s: &str, n: usize) -> String {
    s.trim().chars().take(n).collect()
}

/// Project label from a working directory: its last component, with git
/// worktrees collapsed to the repository name when that is cheap to see.
pub fn project_label(cwd: &str) -> String {
    if cwd.trim().is_empty() {
        return String::new();
    }
    if let Some(repo) = git_worktree_repo(Path::new(cwd)) {
        return repo;
    }
    label_from_path(cwd)
}

/// `<cwd>/.git` is a file `gitdir: <repo>/.git/worktrees/<name>` inside a git worktree.
fn git_worktree_repo(cwd: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(cwd.join(".git")).ok()?;
    if !meta.is_file() || meta.len() > 4096 {
        return None;
    }
    let text = std::fs::read_to_string(cwd.join(".git")).ok()?;
    let dir = text.trim().strip_prefix("gitdir:")?.trim().to_string();
    let parts: Vec<&str> = dir.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let w = parts.iter().rposition(|p| *p == "worktrees")?;
    let mut i = w.checked_sub(1)?;
    if parts[i].eq_ignore_ascii_case(".git") || parts[i].eq_ignore_ascii_case(".bare") {
        i = i.checked_sub(1)?;
    }
    let name = parts[i].trim_end_matches(".git");
    (!name.is_empty() && !name.contains(':')).then(|| name.to_string())
}

/// Pure part of `project_label`: path patterns only.
fn label_from_path(cwd: &str) -> String {
    let parts: Vec<&str> = cwd.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n == 0 {
        return String::new();
    }
    // <repo>/.claude/worktrees/<x>, <repo>/.git/worktrees/<x>
    if n >= 4 && parts[n - 2].eq_ignore_ascii_case("worktrees") && matches!(parts[n - 3], ".claude" | ".git" | ".codex" | ".orca") {
        return parts[n - 4].to_string();
    }
    // <repo>/.worktrees/<x>
    if n >= 3 && parts[n - 2] == ".worktrees" {
        return parts[n - 3].to_string();
    }
    let last = parts[n - 1];
    // <repo>-worktree, <repo>-worktree-<x>, <repo>.worktrees
    for pat in ["-worktree", ".worktree"] {
        if let Some(i) = last.to_ascii_lowercase().find(pat) {
            if i > 0 {
                return last[..i].to_string();
            }
        }
    }
    last.to_string()
}

/// Subagent description from the sibling `agent-<id>.meta.json`.
fn agent_description(path: &Path, agent: &str) -> Option<String> {
    let meta = path.parent()?.join(format!("agent-{agent}.meta.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(meta).ok()?).ok()?;
    let d = v.get("description").and_then(|d| d.as_str()).or_else(|| v.get("agentType").and_then(|d| d.as_str()))?;
    Some(short(d, 100)).filter(|d| !d.is_empty())
}

fn claude_thread(fs: &mut FileState, sid: &str, agent: Option<&str>, path: &Path) -> usize {
    let id = match agent {
        Some(a) => format!("{sid}/{a}"),
        None => sid.to_string(),
    };
    if let Some(i) = fs.th.iter().position(|t| t.id == id) {
        return i;
    }
    let label = agent.and_then(|a| agent_description(path, a)).unwrap_or_default();
    fs.th.push(Th { id, sess: sid.to_string(), sub: agent.is_some(), label, ..Default::default() });
    fs.th.len() - 1
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Claude Code writes one line per content block and repeats the same `usage`
/// on each, so a message counts once per `message.id|requestId`. Resumed
/// sessions copy earlier lines into new files, so the key is kept and dedupe
/// happens again across files when the index is built.
fn claude_lines(fs: &mut FileState, text: &str, path: &Path) {
    let mut seen: HashSet<u64> = fs.msgs.iter().map(|m| m.0).filter(|k| *k != 0).collect();
    let file_sid = stem(path);
    for line in text.split('\n') {
        let title = line.contains("\"custom-title\"");
        if !title && !line.contains("\"usage\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let sid = v.get("sessionId").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).unwrap_or(&file_sid).to_string();
        if v.get("type").and_then(|t| t.as_str()) == Some("custom-title") {
            if let Some(t) = v.get("customTitle").and_then(|t| t.as_str()).filter(|t| !t.trim().is_empty()) {
                let i = claude_thread(fs, &sid, None, path);
                fs.th[i].title = short(t, 120);
            }
            continue;
        }
        let Some(us) = v.pointer("/message/usage") else { continue };
        let n = |k: &str| us.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
        let (i, r, w, o) = (n("input_tokens"), n("cache_read_input_tokens"), n("cache_creation_input_tokens"), n("output_tokens"));
        if i == 0 && r == 0 && w == 0 && o == 0 {
            continue;
        }
        let Some(ts) = parse_ts(v.get("timestamp")) else { continue };
        let id = v.pointer("/message/id").and_then(|m| m.as_str()).unwrap_or("");
        let req = v.get("requestId").and_then(|m| m.as_str()).unwrap_or("");
        let k = if id.is_empty() && req.is_empty() { 0 } else { fnv(format!("{id}|{req}").as_bytes()).max(1) };
        if k != 0 && !seen.insert(k) {
            continue;
        }
        let agent = v.get("agentId").and_then(|a| a.as_str()).filter(|a| !a.is_empty()).map(str::to_string);
        let side = v.get("isSidechain").and_then(|s| s.as_bool()).unwrap_or(false);
        let agent = agent.or_else(|| side.then(|| "sidechain".to_string()));
        let ti = claude_thread(fs, &sid, agent.as_deref(), path);
        let t = &mut fs.th[ti];
        if t.project.is_empty() {
            if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
                t.project = project_label(cwd);
            }
        }
        touch(t, ts);
        let model = v.pointer("/message/model").and_then(|m| m.as_str()).unwrap_or("");
        let m = intern(&mut fs.models, model);
        fs.msgs.push(Msg(k, ts, ti as u32, m, i, r, w, o));
    }
}

fn quad(u: &serde_json::Value) -> [u64; 4] {
    let n = |k: &str| u.get(k).and_then(|n| n.as_u64()).unwrap_or(0);
    [n("input_tokens"), n("cached_input_tokens"), n("cache_write_input_tokens"), n("output_tokens")]
}

/// Codex `token_count` events. `total_token_usage` is cumulative for the
/// thread, so each event adds its delta over the previous one; that puts the
/// tokens on the hour (and model) they were spent. The model comes from the
/// most recent `turn_context`, since `/model` can switch mid-thread.
///
/// The first event of a file and an event after a counter reset use
/// `last_token_usage` instead: a forked thread's first total still carries
/// everything its parent spent.
fn codex_lines(fs: &mut FileState, text: &str, path: &Path) {
    for line in text.split('\n') {
        let meta = line.contains("\"session_meta\"");
        if !meta && !line.contains("\"turn_context\"") && !line.contains("\"token_count\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let p = v.get("payload").unwrap_or(&v);
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") => {
                // Forked/sub threads copy their parent's meta after their own: keep the first.
                if fs.th.is_empty() {
                    let s = |k: &str| p.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let id = [s("id"), s("session_id"), stem(path)].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
                    let parent = s("parent_thread_id");
                    fs.th.push(Th {
                        sess: id.clone(),
                        id,
                        sub: !parent.is_empty(),
                        parent,
                        project: project_label(&s("cwd")),
                        label: short(&s("agent_nickname"), 100),
                        ..Default::default()
                    });
                }
                continue;
            }
            Some("turn_context") => {
                if let Some(m) = p.get("model").and_then(|m| m.as_str()).filter(|m| !m.is_empty()) {
                    fs.cx.model = m.to_string();
                }
                if let (Some(t), Some(cwd)) = (fs.th.first_mut(), p.get("cwd").and_then(|c| c.as_str())) {
                    if t.project.is_empty() {
                        t.project = project_label(cwd);
                    }
                }
                continue;
            }
            _ => {}
        }
        if p.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = p.get("info").filter(|i| i.is_object()) else { continue };
        let Some(tot) = info.get("total_token_usage") else { continue };
        let cur = quad(tot);
        let last = info.get("last_token_usage").filter(|l| l.is_object()).map(quad);
        let cx = &mut fs.cx;
        // A smaller total means the counter restarted (compaction, new thread).
        let d = if !cx.started || cur[0] < cx.prev[0] || cur[3] < cx.prev[3] {
            last.unwrap_or(cur)
        } else {
            [0, 1, 2, 3].map(|k| cur[k].saturating_sub(cx.prev[k]))
        };
        cx.prev = cur;
        cx.started = true;
        if d == [0; 4] {
            continue;
        }
        let Some(ts) = parse_ts(v.get("timestamp")) else { continue };
        if fs.th.is_empty() {
            let id = stem(path);
            fs.th.push(Th { sess: id.clone(), id, ..Default::default() });
        }
        let cx = &mut fs.cx;
        // input_tokens includes the cached (and written) part; keep the kinds disjoint.
        let r = d[1].min(d[0]);
        let w = d[2];
        let i = d[0].saturating_sub(r).saturating_sub(w);
        let ctx = d[0].max(r + w);
        let x = (w > 0 && cx.last_turn > 0 && ts - cx.last_turn > CACHE_TTL) as u64;
        cx.last_turn = ts;
        let model = if cx.model.is_empty() { "codex".to_string() } else { cx.model.clone() };
        let m = intern(&mut fs.models, &model);
        let h = ts.div_euclid(3600);
        let bi = match fs.bk.iter().rposition(|b| b.h == h && b.m == m) {
            Some(bi) => bi,
            None => {
                fs.bk.push(Bk { h, m, ..Default::default() });
                fs.bk.len() - 1
            }
        };
        let b = &mut fs.bk[bi];
        b.n += 1;
        b.i += i;
        b.r += r;
        b.w += w;
        b.o += d[3];
        b.c = b.c.max(ctx);
        b.x += x;
        b.xw += x * w;
        touch(&mut fs.th[0], ts);
    }
}

/// Read what is new in one file. A file that shrank or whose first bytes
/// changed is read again from the start.
fn ingest(fs: &mut FileState, path: &Path, app: App, size: u64, mtime: u64) -> std::io::Result<()> {
    let mut f = std::fs::File::open(path)?;
    let hl = size.min(HEAD);
    let mut head = vec![0u8; hl as usize];
    f.read_exact(&mut head)?;
    let same = fs.app == app as u8 && fs.off > 0 && size >= fs.off && fs.hl > 0 && fs.hl <= hl && fnv(&head[..fs.hl as usize]) == fs.head;
    if !same {
        *fs = FileState { app: app as u8, ..Default::default() };
    }
    fs.hl = hl;
    fs.head = fnv(&head);
    f.seek(SeekFrom::Start(fs.off))?;
    let mut buf = Vec::with_capacity((size - fs.off) as usize);
    (&mut f).take(size - fs.off).read_to_end(&mut buf)?;
    fs.size = size;
    fs.mtime = mtime;
    // Only complete lines: the app may be halfway through writing the last one.
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return Ok(()) };
    // Lossy: one bad byte must not drop a whole session.
    let text = String::from_utf8_lossy(&buf[..end]);
    match app {
        App::Claude => claude_lines(fs, &text, path),
        _ => codex_lines(fs, &text, path),
    }
    fs.off += end as u64 + 1;
    Ok(())
}

// ------------------------------------------------------------------ scanning

/// Where to look. Injectable so tests never touch the real logs or cache.
#[derive(Clone, Debug, Default)]
pub struct Roots {
    pub dirs: Vec<(PathBuf, App)>,
    /// Codex-style `session_index.jsonl` files with thread names.
    pub titles: Vec<(PathBuf, App)>,
}

impl Roots {
    pub fn real() -> Roots {
        let codex = crate::codex::codex_home();
        let mut r = Roots {
            dirs: vec![(crate::claude::claude_home().join("projects"), App::Claude), (codex.join("sessions"), App::Codex)],
            titles: vec![(codex.join("session_index.jsonl"), App::Codex)],
        };
        if let Some(home) = crate::orca::home() {
            r.dirs.push((home.join("sessions"), App::Orca));
            r.titles.push((home.join("session_index.jsonl"), App::Orca));
        }
        r
    }
}

fn walk(dir: &Path, suffix: &str, mut on_file: impl FnMut(&Path)) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            // file_type does not follow symlinks, so a link loop cannot recurse forever.
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some(suffix) {
                on_file(&p);
            }
        }
    }
}

fn path_key(p: &Path) -> String {
    use sha2::Digest;
    // Hash, not the path: folder names often carry project or user names.
    let h = sha2::Sha256::digest(p.display().to_string().as_bytes());
    h.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

struct Scan {
    changed: bool,
    files: [u32; 3],
    skipped: u32,
}

fn scan(cache: &mut Cache, roots: &Roots) -> Scan {
    let mut old = std::mem::take(&mut cache.files);
    let mut s = Scan { changed: false, files: [0; 3], skipped: 0 };
    for (dir, app) in &roots.dirs {
        let app = *app;
        walk(dir, "jsonl", |p| {
            let Ok(meta) = std::fs::metadata(p) else {
                s.skipped += 1;
                return;
            };
            let size = meta.len();
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            let key = path_key(p);
            let mut st = old.remove(&key).unwrap_or_default();
            let known = st.app == app as u8 && (st.hl > 0 || st.size > 0 || size == 0);
            if !(known && st.size == size && st.mtime == mtime) {
                let mut next = st.clone();
                match ingest(&mut next, p, app, size, mtime) {
                    Ok(()) => {
                        s.changed = true;
                        st = next;
                    }
                    Err(_) => {
                        s.skipped += 1;
                        if !known {
                            return;
                        }
                    }
                }
            }
            s.files[app.idx()] += 1;
            cache.files.insert(key, st);
        });
    }
    s.changed |= !old.is_empty();
    s
}

fn read_titles(roots: &Roots) -> HashMap<(App, String), String> {
    let mut out = HashMap::new();
    for (p, app) in &roots.titles {
        let Ok(text) = std::fs::read_to_string(p) else { continue };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let (Some(id), Some(name)) = (v.get("id").and_then(|x| x.as_str()), v.get("thread_name").and_then(|x| x.as_str())) else { continue };
            if !name.trim().is_empty() {
                // Later lines are newer renames.
                out.insert((*app, id.to_string()), short(name, 120));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ index

/// Tokens of one (thread, UTC hour, model).
#[derive(Debug, Clone, Default)]
pub struct IBucket {
    pub app: usize,
    pub thread: u32,
    pub session: u32,
    /// Unix hour (UTC seconds / 3600).
    pub h: i64,
    pub model: u32,
    pub turns: u64,
    pub t: Totals,
    /// Largest single-turn context (input + cache read + cache write).
    pub ctx: u64,
    /// Turns that found the prompt cache expired and wrote it again.
    pub expiries: u64,
    /// Cache-write tokens of those turns.
    pub rewrite: u64,
}

#[derive(Debug, Clone)]
pub struct IThread {
    pub id: String,
    pub app: App,
    pub session: u32,
    pub sub: bool,
    pub label: String,
    pub project: String,
    pub last: i64,
}

#[derive(Debug, Clone)]
pub struct ISession {
    pub key: String,
    pub app: App,
    pub project: String,
    pub label: String,
    #[allow(dead_code)]
    pub first: i64,
    pub last: i64,
    pub subagents: u32,
}

/// Everything a report needs, built from the cache after each scan.
#[derive(Debug, Default)]
pub struct Index {
    /// Sorted by hour.
    pub buckets: Vec<IBucket>,
    pub threads: Vec<IThread>,
    pub sessions: Vec<ISession>,
    pub models: Vec<String>,
    /// Filter options: distinct non-empty project labels / model ids, sorted.
    pub project_names: Vec<String>,
    pub model_names: Vec<String>,
    pub session_by_key: HashMap<String, u32>,
    pub files: [u32; 3],
    pub skipped: u32,
}

impl Index {
    pub fn model_id(&self, m: &str) -> Option<u32> {
        self.models.iter().position(|x| x == m).map(|i| i as u32)
    }
}

fn build(cache: &Cache, titles: &HashMap<(App, String), String>, files: [u32; 3], skipped: u32) -> Index {
    struct T {
        id: String,
        app: App,
        sess: String,
        parent: String,
        sub: bool,
        label: String,
        title: String,
        project: String,
        first: i64,
        last: i64,
    }
    let mut threads: Vec<T> = vec![];
    let mut tmap: HashMap<(App, String), u32> = HashMap::new();
    let mut models: Vec<String> = vec![];
    let mut mmap: HashMap<String, u32> = HashMap::new();
    let mut acc: HashMap<(u32, i64, u32), IBucket> = HashMap::new();
    let mut claude_rows: HashMap<u32, Vec<(i64, u32, Totals)>> = HashMap::new();
    let mut seen: HashSet<u64> = HashSet::new();

    for fs in cache.files.values() {
        let app = App::from_idx(fs.app);
        let gid: Vec<u32> = fs
            .th
            .iter()
            .map(|th| {
                let g = *tmap.entry((app, th.id.clone())).or_insert_with(|| {
                    threads.push(T { id: th.id.clone(), app, sess: th.sess.clone(), parent: String::new(), sub: th.sub, label: String::new(), title: String::new(), project: String::new(), first: 0, last: 0 });
                    (threads.len() - 1) as u32
                });
                let t = &mut threads[g as usize];
                for (dst, src) in [(&mut t.label, &th.label), (&mut t.title, &th.title), (&mut t.project, &th.project), (&mut t.parent, &th.parent)] {
                    if dst.is_empty() && !src.is_empty() {
                        *dst = src.clone();
                    }
                }
                if th.first > 0 && (t.first == 0 || th.first < t.first) {
                    t.first = th.first;
                }
                t.last = t.last.max(th.last);
                g
            })
            .collect();
        let gm: Vec<u32> = fs
            .models
            .iter()
            .map(|m| {
                *mmap.entry(m.clone()).or_insert_with(|| {
                    models.push(m.clone());
                    (models.len() - 1) as u32
                })
            })
            .collect();
        for m in &fs.msgs {
            if m.0 != 0 && !seen.insert(m.0) {
                continue;
            }
            let (Some(&t), Some(&model)) = (gid.get(m.2 as usize), gm.get(m.3 as usize)) else { continue };
            claude_rows.entry(t).or_default().push((m.1, model, Totals { input: m.4, cached: m.5, cache_write: m.6, output: m.7 }));
        }
        if let Some(&t) = gid.first() {
            for b in &fs.bk {
                let Some(&model) = gm.get(b.m as usize) else { continue };
                let e = acc.entry((t, b.h, model)).or_insert_with(|| IBucket { app: app.idx(), thread: t, h: b.h, model, ..Default::default() });
                e.turns += b.n;
                e.t.add(&Totals { input: b.i, cached: b.r, cache_write: b.w, output: b.o });
                e.ctx = e.ctx.max(b.c);
                e.expiries += b.x;
                e.rewrite += b.xw;
            }
        }
    }

    // Claude: per thread in time order, for context size and cache expiries.
    for (t, mut rows) in claude_rows {
        rows.sort_by_key(|r| r.0);
        let mut prev = 0i64;
        for (ts, model, tot) in rows {
            let h = ts.div_euclid(3600);
            let e = acc.entry((t, h, model)).or_insert_with(|| IBucket { app: 0, thread: t, h, model, ..Default::default() });
            e.turns += 1;
            e.t.add(&tot);
            e.ctx = e.ctx.max(tot.input + tot.cached + tot.cache_write);
            if tot.cache_write > 0 && prev > 0 && ts - prev > CACHE_TTL {
                e.expiries += 1;
                e.rewrite += tot.cache_write;
            }
            prev = ts;
        }
    }

    // Sessions: Claude by sessionId, Codex/Orca by the root of the parent chain.
    let parent_of: HashMap<(App, &str), &str> = threads.iter().map(|t| ((t.app, t.id.as_str()), t.parent.as_str())).collect();
    let mut sessions: Vec<ISession> = vec![];
    let mut smap: HashMap<String, u32> = HashMap::new();
    let mut out_threads: Vec<IThread> = Vec::with_capacity(threads.len());
    for t in &threads {
        let root = if t.app == App::Claude {
            t.sess.clone()
        } else {
            let mut cur = t.id.as_str();
            for _ in 0..32 {
                match parent_of.get(&(t.app, cur)).copied() {
                    Some(p) if !p.is_empty() => cur = p,
                    _ => break,
                }
            }
            cur.to_string()
        };
        let key = format!("{}:{root}", t.app.name());
        let si = *smap.entry(key.clone()).or_insert_with(|| {
            let label = titles.get(&(t.app, root.clone())).cloned().unwrap_or_else(|| root.chars().take(8).collect());
            sessions.push(ISession { key, app: t.app, project: String::new(), label, first: 0, last: 0, subagents: 0 });
            (sessions.len() - 1) as u32
        });
        let s = &mut sessions[si as usize];
        if !t.sub {
            if !t.project.is_empty() {
                s.project = t.project.clone();
            }
            if !t.title.is_empty() {
                s.label = t.title.clone();
            }
        } else {
            s.subagents += 1;
        }
        if s.project.is_empty() {
            s.project = t.project.clone();
        }
        if t.first > 0 && (s.first == 0 || t.first < s.first) {
            s.first = t.first;
        }
        s.last = s.last.max(t.last);
        let label = if t.sub { t.label.clone() } else { String::new() };
        out_threads.push(IThread { id: t.id.clone(), app: t.app, session: si, sub: t.sub, label, project: t.project.clone(), last: t.last });
    }

    let mut buckets: Vec<IBucket> = acc.into_values().collect();
    for b in &mut buckets {
        b.session = out_threads[b.thread as usize].session;
    }
    buckets.sort_by_key(|b| (b.h, b.app, b.thread, b.model));
    let project_names: BTreeSet<String> = sessions.iter().map(|s| s.project.clone()).filter(|p| !p.is_empty()).collect();
    let mut used = vec![false; models.len()];
    for b in &buckets {
        used[b.model as usize] = true;
    }
    let model_names: BTreeSet<String> = models.iter().zip(&used).filter(|(m, u)| **u && !m.is_empty() && *m != "<synthetic>").map(|(m, _)| m.clone()).collect();
    Index {
        buckets,
        threads: out_threads,
        session_by_key: smap,
        sessions,
        models,
        project_names: project_names.into_iter().collect(),
        model_names: model_names.into_iter().collect(),
        files,
        skipped,
    }
}

// ------------------------------------------------------------------ shared state

/// Re-scan when a report is asked for and the index is older than this.
const STALE: Duration = Duration::from_secs(10);
/// Rewrite the cache file at most this often while logs keep changing.
const SAVE_EVERY: Duration = Duration::from_secs(30);

pub struct Shared {
    roots: Roots,
    cache_path: PathBuf,
    cache: Option<Cache>,
    index: Index,
    scanned: Option<Instant>,
    saved: Option<Instant>,
    dirty: bool,
}

/// Managed by Tauri; cloned into blocking tasks and the watcher.
pub type UsageState = Arc<Mutex<Shared>>;

pub fn new_state() -> UsageState {
    Arc::new(Mutex::new(Shared::new(Roots::real(), crate::data_dir().join("usage-cache.json"))))
}

/// Today's local totals, re-scanning only when stale (for a tray or overlay).
pub fn today_totals(state: &UsageState) -> Counted {
    let mut s = lock(state);
    crate::usage_report::today(s.index(false), &chrono::Local, chrono::Utc::now())
}

/// The 5-hour block running now over all apps, re-scanning only when stale.
pub fn current_block(state: &UsageState) -> Option<crate::usage_report::UsageBlock> {
    let mut s = lock(state);
    crate::usage_report::current_block(s.index(false), chrono::Utc::now())
}

pub fn lock(state: &UsageState) -> std::sync::MutexGuard<'_, Shared> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    pub fn new(roots: Roots, cache_path: PathBuf) -> Shared {
        Shared { roots, cache_path, cache: None, index: Index::default(), scanned: None, saved: None, dirty: false }
    }

    /// The index, re-scanned incrementally when `force` or stale.
    pub fn index(&mut self, force: bool) -> &Index {
        if force || self.scanned.is_none_or(|t| t.elapsed() >= STALE) {
            self.refresh();
        }
        &self.index
    }

    /// Incremental re-scan. Returns whether anything changed.
    pub fn refresh(&mut self) -> bool {
        let path = self.cache_path.clone();
        let cache = self.cache.get_or_insert_with(|| {
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Cache>(&b).ok()).filter(|c| c.v == CACHE_VERSION).unwrap_or_default()
        });
        cache.v = CACHE_VERSION;
        let s = scan(cache, &self.roots);
        if s.changed || self.scanned.is_none() {
            self.index = build(cache, &read_titles(&self.roots), s.files, s.skipped);
        } else {
            self.index.files = s.files;
            self.index.skipped = s.skipped;
        }
        self.scanned = Some(Instant::now());
        self.dirty |= s.changed;
        if self.dirty && self.saved.is_none_or(|t| t.elapsed() >= SAVE_EVERY) {
            self.save();
        }
        s.changed
    }

    /// Write the cache if it has unsaved changes.
    pub fn flush(&mut self) {
        if self.dirty {
            self.save();
        }
    }

    fn save(&mut self) {
        let Some(cache) = &self.cache else { return };
        if let Ok(bytes) = serde_json::to_vec(cache) {
            if crate::store::atomic_write(&self.cache_path, &bytes).is_ok() {
                self.dirty = false;
            }
        }
        self.saved = Some(Instant::now());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    pub fn t(input: u64, cached: u64, cache_write: u64, output: u64) -> Totals {
        Totals { input, cached, cache_write, output }
    }

    /// Temp log roots with one folder per app and a private cache path.
    pub struct Fixture {
        pub dir: tempfile::TempDir,
    }

    impl Fixture {
        pub fn new() -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            for a in ["claude", "codex", "orca"] {
                std::fs::create_dir_all(dir.path().join(a)).unwrap();
            }
            Fixture { dir }
        }
        pub fn path(&self, rel: &str) -> PathBuf {
            self.dir.path().join(rel)
        }
        pub fn write(&self, rel: &str, text: &str) -> PathBuf {
            let p = self.path(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }
        pub fn append(&self, rel: &str, text: &str) {
            let mut f = std::fs::OpenOptions::new().append(true).open(self.path(rel)).unwrap();
            f.write_all(text.as_bytes()).unwrap();
        }
        pub fn shared(&self) -> Shared {
            let roots = Roots {
                dirs: vec![(self.path("claude"), App::Claude), (self.path("codex"), App::Codex), (self.path("orca"), App::Orca)],
                titles: vec![(self.path("codex/session_index.jsonl"), App::Codex)],
            };
            Shared::new(roots, self.path("cache/usage-cache.json"))
        }
    }

    pub fn app_total(idx: &Index, app: App) -> Totals {
        let mut x = Totals::default();
        for b in idx.buckets.iter().filter(|b| b.app == app.idx()) {
            x.add(&b.t);
        }
        x
    }

    fn model_total(idx: &Index, m: &str) -> Totals {
        let id = idx.model_id(m).unwrap();
        let mut x = Totals::default();
        for b in idx.buckets.iter().filter(|b| b.model == id) {
            x.add(&b.t);
        }
        x
    }

    fn index_of(fx: &Fixture) -> Shared {
        let mut s = fx.shared();
        s.refresh();
        s
    }

    #[test]
    fn claude_keeps_four_kinds_apart() {
        let fx = Fixture::new();
        fx.write("claude/p/s1.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","sessionId":"s1","cwd":"/w/app","message":{"model":"cc/claude-sonnet","usage":{"input_tokens":100,"cache_read_input_tokens":4000,"cache_creation_input_tokens":300,"output_tokens":10}}}
{"timestamp":"2026-11-06T12:00:00Z","message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}
{"timestamp":"2026-11-03T12:00:00Z","sessionId":"s1","message":{"model":"cc/claude-sonnet","usage":{"input_tokens":5,"cache_read_input_tokens":0,"output_tokens":2}}}
"#);
        let mut s = index_of(&fx);
        let idx = s.index(false);
        // input_tokens already excludes cache read and write: nothing is subtracted.
        assert_eq!(app_total(idx, App::Claude), t(105, 4000, 300, 12));
        assert_eq!(model_total(idx, "cc/claude-sonnet"), t(105, 4000, 300, 12));
        assert_eq!(idx.sessions.len(), 1);
        assert_eq!(idx.sessions[0].key, "claude:s1");
        assert_eq!(idx.sessions[0].project, "app");
        assert_eq!(idx.model_names, vec!["cc/claude-sonnet".to_string()]);
    }

    pub const CODEX: &str = r#"{"timestamp":"2026-11-03T12:00:00Z","type":"session_meta","payload":{"id":"th1","cwd":"C:\\w\\repo"}}
{"timestamp":"2026-11-03T12:00:00Z","type":"turn_context","payload":{"model":"cx/a"}}
{"timestamp":"2026-11-03T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":20}}}}
{"timestamp":"2026-11-03T12:00:01Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":20}}}}
{"timestamp":"2026-11-06T12:00:00Z","type":"turn_context","payload":{"model":"cx/b"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2700,"output_tokens":60}}}}
"#;

    #[test]
    fn codex_input_excludes_cached() {
        let fx = Fixture::new();
        fx.write("codex/2026/rollout-1.jsonl", CODEX);
        let mut s = index_of(&fx);
        let idx = s.index(false);
        assert_eq!(app_total(idx, App::Codex), t(300, 2700, 0, 60));
        assert_eq!(model_total(idx, "cx/a"), t(200, 800, 0, 20));
        assert_eq!(model_total(idx, "cx/b"), t(100, 1900, 0, 40));
        assert_eq!(idx.buckets.iter().map(|b| b.turns).sum::<u64>(), 2, "the repeated event is not a turn");
        assert_eq!(idx.sessions[0].project, "repo");
    }

    #[test]
    fn codex_counter_restart_starts_a_new_delta() {
        let fx = Fixture::new();
        fx.write("orca/rollout-1.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5000,"cached_input_tokens":4000,"output_tokens":50}}}}
{"timestamp":"2026-11-06T12:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":700,"cached_input_tokens":600,"output_tokens":7}}}}
"#);
        let mut s = index_of(&fx);
        assert_eq!(app_total(s.index(false), App::Orca), t(1100, 4600, 0, 57));
    }

    #[test]
    fn forked_codex_thread_does_not_count_its_parent() {
        let fx = Fixture::new();
        fx.write("codex/rollout-f.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","type":"session_meta","payload":{"id":"f1","forked_from_id":"p0"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"session_meta","payload":{"id":"p0"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":900000,"cached_input_tokens":800000,"output_tokens":900},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":9}}}}
{"timestamp":"2026-11-06T12:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":902000,"cached_input_tokens":801000,"output_tokens":910},"last_token_usage":{"input_tokens":2000,"cached_input_tokens":1000,"output_tokens":10}}}}
"#);
        let mut s = index_of(&fx);
        let idx = s.index(false);
        assert_eq!(app_total(idx, App::Codex), t(1400, 1600, 0, 19));
        assert_eq!(idx.sessions[0].key, "codex:f1", "the first session_meta is the thread's own");
    }

    #[test]
    fn claude_message_repeated_across_files_counts_once() {
        let fx = Fixture::new();
        let line = r#"{"timestamp":"2026-11-06T12:00:00Z","sessionId":"a","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":7,"cache_read_input_tokens":90,"output_tokens":3}}}"#;
        fx.write("claude/p/a.jsonl", &format!("{line}\n{line}\n"));
        fx.write("claude/p/b.jsonl", &format!("{}\n", line.replace("\"a\"", "\"b\"")));
        let mut s = index_of(&fx);
        let idx = s.index(false);
        assert_eq!(app_total(idx, App::Claude), t(7, 90, 0, 3));
        assert_eq!(idx.buckets.iter().map(|b| b.turns).sum::<u64>(), 1);
    }

    #[test]
    fn incremental_read_waits_for_the_end_of_a_line() {
        let fx = Fixture::new();
        let l1 = r#"{"timestamp":"2026-11-06T12:00:00Z","sessionId":"s","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        let l2 = r#"{"timestamp":"2026-11-06T12:10:00Z","sessionId":"s","requestId":"r2","message":{"id":"m2","model":"x","usage":{"input_tokens":2,"output_tokens":2}}}"#;
        fx.write("claude/p/s.jsonl", &format!("{l1}\n"));
        let mut s = fx.shared();
        s.refresh();
        let (a, b) = l2.split_at(40);
        fx.append("claude/p/s.jsonl", a);
        s.refresh();
        assert_eq!(app_total(s.index(false), App::Claude), t(1, 0, 0, 1), "half a line is not read");
        fx.append("claude/p/s.jsonl", &format!("{b}\n"));
        s.refresh();
        assert_eq!(app_total(s.index(false), App::Claude), t(3, 0, 0, 3));
        // A fresh state from the saved cache agrees and has nothing left to read.
        s.flush();
        let mut s2 = fx.shared();
        assert!(!s2.refresh(), "nothing changed since the cache was written");
        assert_eq!(app_total(s2.index(false), App::Claude), t(3, 0, 0, 3));
        // A file replaced by a shorter one is read again from the start.
        fx.write("claude/p/s.jsonl", &format!("{l1}\n"));
        s2.refresh();
        assert_eq!(app_total(s2.index(false), App::Claude), t(1, 0, 0, 1));
    }

    #[test]
    fn codex_running_total_survives_incremental_reads() {
        let fx = Fixture::new();
        let lines: Vec<&str> = CODEX.lines().collect();
        fx.write("codex/rollout-1.jsonl", &format!("{}\n", lines[..3].join("\n")));
        let mut s = fx.shared();
        s.refresh();
        s.flush();
        // A new process: state comes back from the cache file.
        let mut s = fx.shared();
        fx.append("codex/rollout-1.jsonl", &format!("{}\n", lines[3..].join("\n")));
        s.refresh();
        let idx = s.index(false);
        assert_eq!(app_total(idx, App::Codex), t(300, 2700, 0, 60));
        assert_eq!(model_total(idx, "cx/b"), t(100, 1900, 0, 40), "model carried across reads");
    }

    #[test]
    fn subagents_join_their_session() {
        let fx = Fixture::new();
        fx.write("claude/p/s1.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","sessionId":"s1","requestId":"r1","message":{"id":"m1","model":"x","usage":{"input_tokens":1,"output_tokens":1}}}
{"type":"custom-title","customTitle":"Fix the bug","sessionId":"s1"}
"#);
        fx.write("claude/p/s1/subagents/agent-ab.jsonl", r#"{"timestamp":"2026-11-06T12:01:00Z","sessionId":"s1","agentId":"ab","isSidechain":true,"requestId":"r2","message":{"id":"m2","model":"x","usage":{"input_tokens":5,"output_tokens":5}}}
"#);
        fx.write("claude/p/s1/subagents/agent-ab.meta.json", r#"{"agentType":"Explore","description":"Find the parser"}"#);
        fx.write("codex/rollout-p.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","type":"session_meta","payload":{"id":"P"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":1}}}}
"#);
        fx.write("codex/rollout-c.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","type":"session_meta","payload":{"id":"C","parent_thread_id":"P","agent_nickname":"Ada"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":1}}}}
"#);
        fx.write("codex/rollout-g.jsonl", r#"{"timestamp":"2026-11-06T12:00:00Z","type":"session_meta","payload":{"id":"G","parent_thread_id":"C"}}
{"timestamp":"2026-11-06T12:00:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":10,"output_tokens":1}}}}
"#);
        fx.write("codex/session_index.jsonl", "{\"id\":\"P\",\"thread_name\":\"Parent work\"}\n");
        let mut s = index_of(&fx);
        let idx = s.index(false);
        assert_eq!(idx.sessions.len(), 2);
        let c = &idx.sessions[idx.session_by_key["claude:s1"] as usize];
        assert_eq!((c.label.as_str(), c.subagents), ("Fix the bug", 1));
        let x = &idx.sessions[idx.session_by_key["codex:P"] as usize];
        assert_eq!((x.label.as_str(), x.subagents), ("Parent work", 2));
        assert!(idx.threads.iter().any(|t| t.label == "Find the parser" && t.sub));
        assert!(idx.threads.iter().any(|t| t.label == "Ada" && t.sub));
    }

    #[test]
    fn cache_expiry_counts_turns_after_a_long_pause() {
        let fx = Fixture::new();
        let l = |ts: &str, id: &str, w: u64| format!(r#"{{"timestamp":"{ts}","sessionId":"s","requestId":"{id}","message":{{"id":"{id}","model":"x","usage":{{"input_tokens":1,"cache_creation_input_tokens":{w},"cache_read_input_tokens":100,"output_tokens":1}}}}}}"#);
        let text = [l("2026-11-06T12:00:00Z", "a", 500), l("2026-11-06T12:03:00Z", "b", 10), l("2026-11-06T12:20:00Z", "c", 900), l("2026-11-06T12:40:00Z", "d", 0)].join("\n");
        fx.write("claude/p/s.jsonl", &format!("{text}\n"));
        let mut s = index_of(&fx);
        let idx = s.index(false);
        assert_eq!(idx.buckets.iter().map(|b| b.expiries).sum::<u64>(), 1, "only c: long pause and a cache write");
        assert_eq!(idx.buckets.iter().map(|b| b.rewrite).sum::<u64>(), 900);
        assert_eq!(idx.buckets.iter().map(|b| b.ctx).max(), Some(1001));
    }

    #[test]
    fn project_labels() {
        assert_eq!(label_from_path("C:\\Users\\me\\Desktop\\shop"), "shop");
        assert_eq!(label_from_path("/home/me/shop/"), "shop");
        assert_eq!(label_from_path("/home/me/shop/.claude/worktrees/fix-x"), "shop");
        assert_eq!(label_from_path("/home/me/shop/.worktrees/fix-x"), "shop");
        assert_eq!(label_from_path("/home/me/shop-worktree-2"), "shop");
        assert_eq!(label_from_path("/home/me/shop-worktree"), "shop");
        assert_eq!(label_from_path(""), "");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".git"), "gitdir: D:/src/shop/.git/worktrees/feature-a\n").unwrap();
        assert_eq!(project_label(&dir.path().display().to_string()), "shop");
    }
}
