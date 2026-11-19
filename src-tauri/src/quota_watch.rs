//! "Báo khi hồi quota": after a gateway says a model is out of quota, check
//! again later and notify once it works.
//!
//! Cheap by design:
//! - Quota is per upstream account, not per model (every `cx/...` model on
//!   9router runs out at the same moment), so one model per group is watched.
//! - When the error says when the limit resets ("reset after 5m 34s"), only
//!   one check is made, just after that time. Otherwise checks back off
//!   5 → 10 → 20 → 30 minutes and then stay at 30.
//! - A 429 is refused before the model runs, so a failed check costs no
//!   tokens; the one that succeeds asks for at most 16 (see `probe::probe_one`).
//! - Gives up after 24 hours unless the gateway named a later reset time.
//!
//! - Never for out-of-credit errors (balance, billing, HTTP 402): waiting does
//!   not top up an account.
//!
//! The top of this file holds the pure rules (tested below). The scheduler at
//! the bottom is one thread that exists only while there are watches: it
//! sleeps until the earliest check, runs one probe at a time and notifies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::probe::{Probe, Status};
use crate::providers::{ModelInfo, Provider};

/// When the first check is already due, make it this soon.
const SOON: Duration = Duration::from_secs(5);
/// The scheduler re-reads the clock at least this often (a laptop that slept
/// must not miss a reset).
const MAX_SLEEP: Duration = Duration::from_secs(60);
pub const BILLING_HINT: &str = "Out of credit: top up, waiting won't help";

/// Wait after the announced reset before checking, so the gateway has rolled over.
const RESET_SLACK: Duration = Duration::from_secs(30);
const BACKOFF_MIN: [u64; 4] = [5, 10, 20, 30];
/// Without an announced reset, stop after this long.
const GIVE_UP: Duration = Duration::from_secs(24 * 3600);
/// Ignore announced resets further away than this (weekly limits included).
const MAX_RESET: Duration = Duration::from_secs(8 * 24 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Watch {
    pub provider_id: String,
    /// Upstream account group (9router `owned_by`, else the id prefix).
    pub group: String,
    /// The model the checks use (the one Codex/Claude Code is set to, when possible).
    pub model: String,
    /// When the watch started, RFC 3339.
    pub since: String,
    /// Next check, RFC 3339.
    pub next_at: String,
    #[serde(default)]
    pub attempts: u32,
    /// Last gateway message (key masked), for the UI.
    #[serde(default)]
    pub message: String,
    /// Why it was started: "auto" (seen while checking models) or "manual".
    #[serde(default)]
    pub origin: String,
    /// Last failed check (the probe that started the watch, then each re-check), RFC 3339.
    #[serde(default)]
    pub last_check: String,
}

/// What to do after a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Works again: notify and drop the watch.
    Recovered,
    /// Still limited, or a transient failure: check again after this long.
    Later(Duration),
    /// Stop without a "recovered" notification (bad key, model gone, too long).
    Stop(String),
}

/// Group that shares one quota. `owned_by` comes from the gateway's /models.
pub fn group_of(owned_by: Option<&str>, model: &str) -> String {
    if let Some(o) = owned_by.filter(|o| !o.is_empty()) {
        return o.to_string();
    }
    match model.split_once('/') {
        Some((prefix, _)) if !prefix.is_empty() => prefix.to_string(),
        _ => model.to_string(),
    }
}

/// Reset time announced in a gateway error, if any:
/// "reset after 5m 34s", "resets in 2h 13m", "try again in 20s",
/// "retry after 30 seconds", "Please retry in 1.5 hours".
pub fn parse_reset(msg: &str) -> Option<Duration> {
    let lower = msg.to_lowercase();
    let start = ["reset after", "resets after", "reset in", "resets in", "try again in", "try again after", "retry after", "retry in"]
        .iter()
        .filter_map(|k| lower.find(k).map(|i| i + k.len()))
        .min()?;
    let words: Vec<&str> = lower[start..]
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|w| w.trim_matches(|c: char| "()[].;:".contains(c)))
        .filter(|w| !w.is_empty())
        .take(8)
        .collect();
    let mut total = 0f64;
    let mut found = false;
    let mut i = 0;
    'words: while i < words.len() {
        // A word is pairs of number + unit ("5m34s", "5m", "1.5") ; a bare number
        // takes its unit from the next word ("30 seconds").
        let mut rest = words[i];
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        while !rest.is_empty() {
            let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
            let Ok(n) = rest[..end].parse::<f64>() else { break 'words };
            rest = &rest[end..];
            let ulen = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
            let unit = if ulen == 0 {
                i += 1;
                words.get(i).copied().unwrap_or("")
            } else {
                &rest[..ulen]
            };
            rest = &rest[ulen..];
            let Some(secs) = unit_secs(unit) else { break 'words };
            total += n * secs;
            found = true;
        }
        i += 1;
    }
    (found && total > 0.0).then(|| Duration::from_secs_f64(total)).filter(|d| *d <= MAX_RESET)
}

fn unit_secs(u: &str) -> Option<f64> {
    Some(match u {
        "ms" => 0.001,
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        _ => return None,
    })
}

/// Delay before the next check.
pub fn next_delay(attempts: u32, reset: Option<Duration>) -> Duration {
    match reset {
        Some(r) => r + RESET_SLACK,
        None => Duration::from_secs(60 * BACKOFF_MIN[(attempts as usize).min(BACKOFF_MIN.len() - 1)]),
    }
}

/// Decide from one check. `elapsed` = time since the watch started.
pub fn decide(probe: &Probe, attempts: u32, elapsed: Duration) -> Next {
    match probe.status {
        Status::Ok => Next::Recovered,
        Status::Auth => Next::Stop("The gateway refused the key".into()),
        Status::Missing => Next::Stop("The gateway no longer has this model".into()),
        Status::Quota => {
            let reset = parse_reset(&probe.message);
            if reset.is_none() && elapsed >= GIVE_UP {
                return Next::Stop("Still out of quota after 24 hours".into());
            }
            Next::Later(next_delay(attempts, reset))
        }
        // Timeout / upstream / other: not a quota answer, try again later.
        _ if elapsed >= GIVE_UP => Next::Stop("No clear answer from the gateway after 24 hours".into()),
        _ => Next::Later(next_delay(attempts, None)),
    }
}

// ---------------------------------------------------------------- storage

pub fn path(data: &Path) -> PathBuf {
    data.join("quota-watch.json")
}

pub fn load(data: &Path) -> Vec<Watch> {
    std::fs::read_to_string(path(data)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(data: &Path, watches: &[Watch]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(watches).map_err(|e| e.to_string())?;
    crate::store::atomic_write(&path(data), text.as_bytes())
}

/// Add or refresh the watch for (provider, group). Keeps an existing one's
/// start time so the 24-hour limit counts from the first time.
pub fn upsert(list: &mut Vec<Watch>, w: Watch) {
    match list.iter_mut().find(|x| x.provider_id == w.provider_id && x.group == w.group) {
        Some(x) => {
            x.model = w.model;
            x.next_at = w.next_at;
            x.message = w.message;
            x.last_check = w.last_check;
            if w.origin == "manual" {
                x.origin = w.origin;
            }
        }
        None => list.push(w),
    }
}

// ---------------------------------------------------------------- arming

/// Out-of-credit errors look like quota errors to `probe::classify`, but no
/// amount of waiting fixes them, so they are never watched.
pub fn is_billing(msg: &str) -> bool {
    let m = msg.to_lowercase();
    ["balance", "credit", "insufficient", "payment", "billing", "http 402"].iter().any(|w| m.contains(w))
}

/// Model id without a context suffix: "cx/gpt-6-sol[1m]" → "cx/gpt-6-sol".
pub fn base_model(model: &str) -> &str {
    match model.rfind('[') {
        Some(i) if model.ends_with(']') && i > 0 => &model[..i],
        _ => model,
    }
}

/// Quota group of `model` on a gateway, using its /models metadata.
pub fn group_in(info: &BTreeMap<String, ModelInfo>, model: &str) -> String {
    let owned = info.get(model).or_else(|| info.get(base_model(model))).and_then(|i| i.owned_by.as_deref());
    group_of(owned, base_model(model))
}

/// The id to send when checking: the configured one if the gateway lists it,
/// else the same model without its context suffix (a check against an id the
/// gateway does not know would end the watch with "model gone").
pub fn check_model(models: &[String], configured: &str) -> String {
    let base = base_model(configured);
    if models.is_empty() || models.iter().any(|m| m == configured) || !models.iter().any(|m| m == base) {
        configured.to_string()
    } else {
        base.to_string()
    }
}

fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&Utc))
}

fn to_d(d: Duration) -> chrono::Duration {
    chrono::Duration::from_std(d).unwrap_or(chrono::Duration::MAX)
}

/// First check time, counted from when the gateway answered (not from now), so
/// a probe taken a while ago does not wait the whole reset again. Never sooner
/// than a few seconds from now.
pub fn first_check(probe: &Probe, now: DateTime<Utc>) -> DateTime<Utc> {
    let base = parse_time(&probe.checked_at).filter(|t| *t <= now).unwrap_or(now);
    let at = base + to_d(next_delay(0, parse_reset(&probe.message)));
    at.max(now + to_d(SOON))
}

/// A new watch for (provider, group). `probe` is the latest result for the
/// group, when there is one. Refuses out-of-credit errors.
pub fn arm(provider_id: &str, group: &str, model: &str, probe: Option<&Probe>, origin: &str, now: DateTime<Utc>) -> Result<Watch, String> {
    let quota = probe.filter(|p| p.status == Status::Quota);
    if quota.is_some_and(|p| is_billing(&p.message)) {
        return Err(BILLING_HINT.into());
    }
    let next = quota.map(|p| first_check(p, now)).unwrap_or(now + to_d(SOON));
    // Without a named reset the first wait was back-off step 0, so the next one is step 1.
    let attempts = u32::from(quota.is_some_and(|p| parse_reset(&p.message).is_none()));
    Ok(Watch {
        provider_id: provider_id.into(),
        group: group.into(),
        model: model.into(),
        since: now.to_rfc3339(),
        next_at: next.to_rfc3339(),
        attempts,
        message: quota.map(|p| p.message.clone()).unwrap_or_default(),
        origin: origin.into(),
        last_check: quota.map(|p| p.checked_at.clone()).unwrap_or_else(|| now.to_rfc3339()),
    })
}

/// Watches to start after "Kiểm tra model" on `provider`: one per quota group
/// that a tool (Codex, Claude Code) is set to on this provider, checked with
/// the tool's own model. `tools` = (provider id, model) per tool.
pub fn auto_targets(provider: &Provider, results: &[(String, Probe)], tools: &[(String, String)]) -> Vec<(String, String, Probe)> {
    let mut out: Vec<(String, String, Probe)> = vec![];
    for (pid, tool_model) in tools {
        if *pid != provider.id || tool_model.is_empty() {
            continue;
        }
        let group = group_in(&provider.info, tool_model);
        if out.iter().any(|(g, _, _)| *g == group) {
            continue;
        }
        let in_group: Vec<&(String, Probe)> =
            results.iter().filter(|(m, p)| p.status == Status::Quota && group_in(&provider.info, m) == group).collect();
        // The tool's own model speaks for the group when it was checked.
        let own = in_group.iter().find(|(m, _)| m == tool_model || base_model(m) == base_model(tool_model));
        let Some((_, probe)) = own.or(in_group.first()) else { continue };
        if is_billing(&probe.message) {
            continue;
        }
        out.push((group, check_model(&provider.models, tool_model), probe.clone()));
    }
    out
}

/// Groups that answered ok in this run: watching them is pointless now.
pub fn ok_groups(provider: &Provider, results: &[(String, Probe)]) -> Vec<String> {
    let mut g: Vec<String> = results.iter().filter(|(_, p)| p.status == Status::Ok).map(|(m, _)| group_in(&provider.info, m)).collect();
    g.sort();
    g.dedup();
    g
}

// ---------------------------------------------------------------- one step

/// OS notification text.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The watch was cancelled or re-armed while checking: ignore the result.
    Gone,
    /// Dropped from the list; tell the user.
    Ended(Watch, Notice),
    /// Still waiting; next check updated.
    Waiting,
}

/// The watch due next and its key, or how long until one is due.
pub fn due(list: &[Watch], now: DateTime<Utc>) -> Result<Watch, Duration> {
    let next = list.iter().min_by_key(|w| parse_time(&w.next_at).unwrap_or(now)).ok_or(MAX_SLEEP)?;
    let at = parse_time(&next.next_at).unwrap_or(now);
    if at <= now {
        Ok(next.clone())
    } else {
        Err((at - now).to_std().unwrap_or(MAX_SLEEP))
    }
}

/// Apply one check result to the watch `checked` (as it was when the check
/// started). `probe` = Err(reason) when the check could not run at all.
pub fn step(list: &mut Vec<Watch>, checked: &Watch, probe: Result<&Probe, String>, now: DateTime<Utc>) -> Outcome {
    let Some(i) = list.iter().position(|w| w.provider_id == checked.provider_id && w.group == checked.group && w.next_at == checked.next_at) else {
        return Outcome::Gone;
    };
    let w = &mut list[i];
    let elapsed = parse_time(&w.since).map(|s| (now - s).to_std().unwrap_or_default()).unwrap_or_default();
    let next = match &probe {
        Err(reason) => Next::Stop(reason.clone()),
        Ok(p) if p.status == Status::Quota && is_billing(&p.message) => Next::Stop(BILLING_HINT.into()),
        Ok(p) => decide(p, w.attempts, elapsed),
    };
    let stopped = |w: Watch, title: String, body: String| Outcome::Ended(w, Notice { title, body });
    match next {
        Next::Recovered => {
            let w = list.remove(i);
            let title = format!("Gateswitch: {} quota is back", w.group);
            let body = format!("{} works again.", w.model);
            stopped(w, title, body)
        }
        Next::Stop(reason) => {
            let w = list.remove(i);
            let title = format!("Gateswitch: stopped watching {}", w.group);
            stopped(w, title, reason)
        }
        Next::Later(d) => {
            w.attempts += 1;
            w.next_at = (now + to_d(d)).to_rfc3339();
            w.last_check = now.to_rfc3339();
            if let Ok(p) = &probe {
                w.message = p.message.clone();
            }
            Outcome::Waiting
        }
    }
}

// ---------------------------------------------------------------- passive

/// Watches the user has already worked around: Codex/Orca logged a reply
/// (`used` = model id + unix hour of a bucket with tokens) from a model of
/// the watched group in an hour after the last failed check. Only exact ids
/// the provider lists count. Hour buckets make this conservative: a reply in
/// the same hour as the check is not counted.
pub fn used_since(list: &[Watch], provider: &dyn Fn(&str) -> Option<Provider>, used: &[(String, i64)]) -> Vec<(String, String)> {
    let mut gone = vec![];
    for w in list {
        let Some(p) = provider(&w.provider_id) else { continue };
        let Some(last) = parse_time(&w.last_check) else { continue };
        let hour = last.timestamp().div_euclid(3600);
        if used.iter().any(|(m, h)| *h > hour && p.models.iter().any(|x| x == m) && group_in(&p.info, m) == w.group) {
            gone.push((w.provider_id.clone(), w.group.clone()));
        }
    }
    gone
}

// ---------------------------------------------------------------- scheduler

#[derive(Default)]
struct Sched {
    list: Vec<Watch>,
    running: bool,
}

/// Tauri state: the watch list and the scheduler thread's wake-up signal.
#[derive(Default, Clone)]
pub struct QuotaWatch(Arc<(Mutex<Sched>, Condvar)>);

fn lock(m: &Mutex<Sched>) -> std::sync::MutexGuard<'_, Sched> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub const EVENT: &str = "quota-watch-changed";

impl QuotaWatch {
    pub fn list(&self) -> Vec<Watch> {
        lock(&self.0 .0).list.clone()
    }

    /// Change the list, save it, wake the scheduler (starting it if needed)
    /// and tell the windows. `f` returns whether it changed anything.
    pub fn edit(&self, app: &tauri::AppHandle, f: impl FnOnce(&mut Vec<Watch>) -> bool) -> Vec<Watch> {
        use tauri::Emitter;
        let (m, cv) = &*self.0;
        let mut g = lock(m);
        if !f(&mut g.list) {
            return g.list.clone();
        }
        if let Err(e) = save(&crate::data_dir(), &g.list) {
            eprintln!("quota watch: {e}");
        }
        self.ensure_running(app, &mut g);
        cv.notify_all();
        let out = g.list.clone();
        drop(g);
        let _ = app.emit(EVENT, &out);
        out
    }

    /// Load saved watches at start; the thread starts only if there are any.
    pub fn start(&self, app: &tauri::AppHandle) {
        let mut g = lock(&self.0 .0);
        g.list = load(&crate::data_dir());
        self.ensure_running(app, &mut g);
    }

    fn ensure_running(&self, app: &tauri::AppHandle, g: &mut Sched) {
        if g.running || g.list.is_empty() {
            return;
        }
        g.running = true;
        let (me, app) = (self.clone(), app.clone());
        std::thread::spawn(move || me.run(app));
    }

    fn run(&self, app: tauri::AppHandle) {
        use tauri::Emitter;
        let (m, cv) = &*self.0;
        loop {
            let watch = {
                let mut g = lock(m);
                loop {
                    if g.list.is_empty() {
                        g.running = false;
                        return;
                    }
                    match due(&g.list, Utc::now()) {
                        Ok(w) => break w,
                        Err(wait) => g = cv.wait_timeout(g, wait.min(MAX_SLEEP)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0),
                    }
                }
            };
            // No lock while the request runs (up to 30 s).
            let probe = check(&watch);
            let outcome = {
                let mut g = lock(m);
                let o = step(&mut g.list, &watch, probe.as_ref().map_err(|e| e.clone()), Utc::now());
                if o != Outcome::Gone {
                    if let Err(e) = save(&crate::data_dir(), &g.list) {
                        eprintln!("quota watch: {e}");
                    }
                }
                o
            };
            if outcome == Outcome::Gone {
                continue;
            }
            if let Ok(p) = &probe {
                // The latest answer is the truth for the Codex / Claude Code cards.
                let _ = crate::providers().set_probes(&watch.provider_id, vec![(watch.model.clone(), p.clone())], &crate::secrets::Keychain);
            }
            if let Outcome::Ended(_, n) = &outcome {
                crate::usage_live::toast(&app, &n.title, &n.body);
            }
            let _ = app.emit(EVENT, &self.list());
        }
    }
}

/// One cheap request (see `probe::probe_one`) with the provider's key.
fn check(w: &Watch) -> Result<Probe, String> {
    use crate::secrets::Vault;
    let p = crate::providers().get(&w.provider_id, &crate::secrets::Keychain).map_err(|_| "The provider was removed".to_string())?;
    let key = crate::secrets::Keychain.get(&crate::providers::account(&p.id)).ok_or("The provider has no key")?;
    Ok(crate::probe::probe_one(&crate::probe::agent(), &p.base_url, &key, &w.model))
}

/// After "Kiểm tra model": drop watches whose group answered ok, and (when the
/// user turned it on) watch the groups Codex / Claude Code are set to that
/// came back out of quota.
pub fn after_probe(app: &tauri::AppHandle, provider: &Provider, results: &[(String, Probe)], auto: bool, tools: &[(String, String)]) {
    use tauri::Manager;
    let ok = ok_groups(provider, results);
    let targets = if auto { auto_targets(provider, results, tools) } else { vec![] };
    if ok.is_empty() && targets.is_empty() {
        return;
    }
    let now = Utc::now();
    app.state::<QuotaWatch>().edit(app, |list| {
        let before = list.clone();
        list.retain(|w| !(w.provider_id == provider.id && ok.contains(&w.group)));
        for (group, model, probe) in &targets {
            if let Ok(w) = arm(&provider.id, group, model, Some(probe), "auto", now) {
                upsert(list, w);
            }
        }
        *list != before
    });
}

/// Passive check after the log watcher re-scanned (the index is fresh, so
/// this costs a pass over the hour buckets and nothing else).
pub fn after_usage(app: &tauri::AppHandle) {
    use tauri::Manager;
    let qw = app.state::<QuotaWatch>();
    let list = qw.list();
    if list.is_empty() {
        return;
    }
    let state = app.state::<crate::usage::UsageState>().inner().clone();
    let mut shared = crate::usage::lock(&state);
    let index = shared.index(false);
    // Codex and Orca log the id they sent ("cx/gpt-6.2-sol"); Claude Code logs
    // what the gateway answered (often without the prefix), so it is skipped.
    let mut used: BTreeMap<u32, i64> = BTreeMap::new();
    for b in index.buckets.iter().filter(|b| b.app != 0 && b.turns > 0 && b.t.sum() > 0) {
        let h = used.entry(b.model).or_insert(b.h);
        *h = (*h).max(b.h);
    }
    let used: Vec<(String, i64)> = used.into_iter().filter_map(|(m, h)| index.models.get(m as usize).map(|s| (s.clone(), h))).collect();
    drop(shared);
    let ps = crate::providers().list(&crate::secrets::Keychain);
    let find = |id: &str| ps.iter().find(|p| p.id == id).cloned();
    let gone = used_since(&list, &find, &used);
    if !gone.is_empty() {
        qw.edit(app, |l| {
            l.retain(|w| !gone.contains(&(w.provider_id.clone(), w.group.clone())));
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(status: Status, message: &str) -> Probe {
        Probe { status, latency_ms: None, message: message.into(), checked_at: String::new() }
    }

    #[test]
    fn reset_times_from_real_gateways() {
        let s = |n| Some(Duration::from_secs(n));
        assert_eq!(parse_reset("HTTP 503: [codex/gpt-6-sol] [429]: The usage limit has been reached (reset after 5m 34s)"), s(334));
        assert_eq!(parse_reset("limit reached, resets in 2h 13m"), s(2 * 3600 + 13 * 60));
        assert_eq!(parse_reset("Rate limit. Please try again in 20s."), s(20));
        assert_eq!(parse_reset("retry after 30 seconds"), s(30));
        assert_eq!(parse_reset("reset after 5m34s"), s(334));
        assert_eq!(parse_reset("resets in 1.5 hours"), s(5400));
        assert_eq!(parse_reset("weekly limit, resets in 3 days"), s(3 * 86400));
        assert_eq!(parse_reset("{\"error\":{\"type\":\"quota_error\"}}"), None);
        assert_eq!(parse_reset("reset after soon"), None);
        assert_eq!(parse_reset("resets in 30 days"), None, "too far to be useful");
    }

    #[test]
    fn groups_share_one_quota() {
        assert_eq!(group_of(Some("cx"), "cx/gpt-6-sol"), "cx");
        assert_eq!(group_of(None, "cx/gpt-6-sol[1m]"), "cx");
        assert_eq!(group_of(Some(""), "gpt-6"), "gpt-6");
    }

    #[test]
    fn one_check_when_reset_is_known_else_back_off() {
        let p = probe(Status::Quota, "The usage limit has been reached (reset after 5m 34s)");
        assert_eq!(decide(&p, 0, Duration::ZERO), Next::Later(Duration::from_secs(334 + 30)));
        let p = probe(Status::Quota, "quota_error");
        let mins: Vec<u64> = (0..6)
            .map(|a| match decide(&p, a, Duration::ZERO) { Next::Later(d) => d.as_secs() / 60, n => panic!("{n:?}") })
            .collect();
        assert_eq!(mins, [5, 10, 20, 30, 30, 30]);
    }

    #[test]
    fn stops_and_recovers() {
        assert_eq!(decide(&probe(Status::Ok, ""), 3, Duration::ZERO), Next::Recovered);
        assert!(matches!(decide(&probe(Status::Auth, "HTTP 401"), 0, Duration::ZERO), Next::Stop(_)));
        assert!(matches!(decide(&probe(Status::Quota, "quota"), 9, GIVE_UP), Next::Stop(_)));
        // A named reset later than 24 h is still honoured (weekly limits).
        assert!(matches!(decide(&probe(Status::Quota, "resets in 2 days"), 9, GIVE_UP), Next::Later(_)));
        // Transient failures retry quietly.
        assert!(matches!(decide(&probe(Status::Upstream, "HTTP 502"), 0, Duration::ZERO), Next::Later(_)));
    }

    #[test]
    fn upsert_keeps_first_start() {
        let w = |since: &str, origin: &str| Watch { provider_id: "p".into(), group: "cx".into(), model: "cx/a".into(), since: since.into(), next_at: "n".into(), attempts: 0, message: String::new(), origin: origin.into(), last_check: String::new() };
        let mut list = vec![w("first", "auto")];
        upsert(&mut list, w("second", "manual"));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].since, "first");
        assert_eq!(list[0].origin, "manual");
    }

    fn at(s: &str) -> DateTime<Utc> {
        parse_time(s).unwrap()
    }

    fn probe_at(status: Status, message: &str, checked_at: &str) -> Probe {
        Probe { status, latency_ms: None, message: message.into(), checked_at: checked_at.into() }
    }

    fn gateway() -> Provider {
        serde_json::from_value(serde_json::json!({
            "id": "nr", "name": "9router", "base_url": "http://localhost:20128/v1",
            "models": ["cx/gpt-6-astra", "cx/gpt-6.2-sol", "cx/gpt-6-sol[1m]", "cc/claude-opus-5-5", "gpt-6"],
            "info": {
                "cx/gpt-6-astra": { "owned_by": "cx" }, "cx/gpt-6.2-sol": { "owned_by": "cx" },
                "cc/claude-opus-5-5": { "owned_by": "cc" }, "gpt-6": { "owned_by": "combo" }
            }
        }))
        .unwrap()
    }

    #[test]
    fn out_of_credit_is_not_worth_waiting_for() {
        assert!(is_billing("HTTP 402: Payment Required"));
        assert!(is_billing("HTTP 429: Your credit balance is too low"));
        assert!(is_billing("insufficient_quota: check your plan and billing details"));
        assert!(!is_billing("HTTP 503: [codex/gpt-6-sol] [429]: The usage limit has been reached (reset after 5m 34s)"));
        assert!(!is_billing("HTTP 429: rate limit"));
        let now = at("2026-11-19T10:00:00Z");
        let p = probe_at(Status::Quota, "HTTP 402: insufficient balance", "2026-11-19T09:59:00Z");
        assert_eq!(arm("nr", "cx", "cx/a", Some(&p), "manual", now), Err(BILLING_HINT.to_string()));
        let gw = gateway();
        assert!(auto_targets(&gw, &[("cx/gpt-6-astra".into(), p.clone())], &[("nr".into(), "cx/gpt-6.2-sol".into())]).is_empty(), "never auto-watched");
    }

    #[test]
    fn first_check_counts_from_the_probe_not_from_now() {
        let now = at("2026-11-19T10:00:00Z");
        let msg = "The usage limit has been reached (reset after 5m 34s)";
        // Fresh probe: reset + 30 s slack.
        assert_eq!(first_check(&probe_at(Status::Quota, msg, "2026-11-19T10:00:00Z"), now), now + chrono::Duration::seconds(364));
        // Probe from 4 minutes ago (local offset): only the remaining time.
        assert_eq!(first_check(&probe_at(Status::Quota, msg, "2026-11-19T16:56:00+07:00"), now), now + chrono::Duration::seconds(124));
        // Reset already passed: check in a few seconds.
        assert_eq!(first_check(&probe_at(Status::Quota, msg, "2026-11-19T08:00:00Z"), now), now + chrono::Duration::seconds(5));
        // No reset named: first back-off step, counted from the probe.
        assert_eq!(first_check(&probe_at(Status::Quota, "quota_error", "2026-11-19T09:58:00Z"), now), now + chrono::Duration::minutes(3));
        // Unreadable time: from now.
        assert_eq!(first_check(&probe_at(Status::Quota, "quota_error", ""), now), now + chrono::Duration::minutes(5));
        // Manual watch with no quota result: check soon.
        let w = arm("nr", "cx", "cx/a", None, "manual", now).unwrap();
        assert_eq!(at(&w.next_at), now + chrono::Duration::seconds(5));
        assert_eq!(w.attempts, 0);
        // No reset named: the wait before the first check was the 5-minute step,
        // so a failed first check waits 10 (5, 10, 20, 30 overall).
        let mut list = vec![arm("nr", "cx", "cx/a", Some(&probe_at(Status::Quota, "quota_error", "2026-11-19T10:00:00Z")), "auto", now).unwrap()];
        let w = list[0].clone();
        assert_eq!(w.attempts, 1);
        assert_eq!(step(&mut list, &w, Ok(&probe_at(Status::Quota, "quota_error", "")), now), Outcome::Waiting);
        assert_eq!(at(&list[0].next_at), now + chrono::Duration::minutes(10));
    }

    #[test]
    fn auto_arms_only_the_groups_the_tools_use() {
        let gw = gateway();
        let q = probe_at(Status::Quota, "usage limit (reset after 10m)", "2026-11-19T10:00:00Z");
        let ok = probe_at(Status::Ok, "", "2026-11-19T10:00:00Z");
        let results = vec![("cx/gpt-6-astra".to_string(), q.clone()), ("cc/claude-opus-5-5".to_string(), ok.clone()), ("gpt-6".to_string(), q.clone())];
        // Codex on cx (an id with [1m] the gateway lists), Claude Code on cc (ok), another provider ignored.
        let tools = vec![("nr".to_string(), "cx/gpt-6-sol[1m]".to_string()), ("nr".to_string(), "cc/claude-opus-5-5".to_string()), ("other".to_string(), "gpt-6".to_string())];
        let t = auto_targets(&gw, &results, &tools);
        assert_eq!(t.len(), 1, "{t:?}");
        assert_eq!((t[0].0.as_str(), t[0].1.as_str()), ("cx", "cx/gpt-6-sol[1m]"));
        // Two tools on the same group: one watch, checked with the first tool's model.
        let t = auto_targets(&gw, &results, &[("nr".into(), "cx/gpt-6.2-sol".into()), ("nr".into(), "cx/gpt-6-astra".into())]);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "cx/gpt-6.2-sol");
        // Nobody uses the combo group: not watched although it is out of quota.
        assert!(auto_targets(&gw, &results, &[]).is_empty());
        assert_eq!(ok_groups(&gw, &results), vec!["cc".to_string()]);
    }

    #[test]
    fn check_model_avoids_ids_the_gateway_does_not_list() {
        let models: Vec<String> = ["cx/gpt-6.2-sol", "cx/gpt-6-sol[1m]"].iter().map(|s| s.to_string()).collect();
        assert_eq!(base_model("cx/gpt-6.2-sol[1m]"), "cx/gpt-6.2-sol");
        assert_eq!(base_model("[1m]"), "[1m]");
        assert_eq!(check_model(&models, "cx/gpt-6.2-sol[1m]"), "cx/gpt-6.2-sol");
        assert_eq!(check_model(&models, "cx/gpt-6-sol[1m]"), "cx/gpt-6-sol[1m]");
        assert_eq!(check_model(&[], "cx/x[1m]"), "cx/x[1m]");
        assert_eq!(group_in(&gateway().info, "cx/gpt-6.2-sol[1m]"), "cx");
    }

    #[test]
    fn scheduler_step() {
        let now = at("2026-11-19T10:00:00Z");
        let w = arm("nr", "cx", "cx/gpt-6.2-sol", Some(&probe_at(Status::Quota, "reset after 1m", "2026-11-19T10:00:00Z")), "manual", now).unwrap();
        let mut list = vec![w.clone()];
        assert_eq!(due(&list, now), Err(Duration::from_secs(90)));
        let later = now + chrono::Duration::seconds(90);
        assert_eq!(due(&list, later).unwrap(), w);
        assert_eq!(due(&[], now), Err(MAX_SLEEP));

        // Still limited, no reset named: back off and remember the check.
        let o = step(&mut list, &w, Ok(&probe_at(Status::Quota, "HTTP 429", "")), later);
        assert_eq!(o, Outcome::Waiting);
        assert_eq!((list[0].attempts, list[0].message.as_str()), (1, "HTTP 429"));
        assert_eq!(at(&list[0].next_at), later + chrono::Duration::minutes(5));
        assert_eq!(at(&list[0].last_check), later);

        // A result for a watch re-armed (or cancelled) meanwhile is ignored.
        assert_eq!(step(&mut list, &w, Ok(&probe_at(Status::Ok, "", "")), later), Outcome::Gone);
        assert_eq!(list.len(), 1);

        // Works again: dropped, with one notification.
        let cur = list[0].clone();
        match step(&mut list, &cur, Ok(&probe_at(Status::Ok, "", "")), later) {
            Outcome::Ended(_, n) => {
                assert_eq!(n.title, "Gateswitch: cx quota is back");
                assert_eq!(n.body, "cx/gpt-6.2-sol works again.");
            }
            o => panic!("{o:?}"),
        }
        assert!(list.is_empty());

        // The account ran out of credit meanwhile: stop, do not back off for a day.
        let mut list = vec![w.clone()];
        match step(&mut list, &w, Ok(&probe_at(Status::Quota, "HTTP 402: insufficient balance", "")), later) {
            Outcome::Ended(_, n) => assert_eq!((n.title.as_str(), n.body.as_str()), ("Gateswitch: stopped watching cx", BILLING_HINT)),
            o => panic!("{o:?}"),
        }
        // The check could not run (provider removed).
        let mut list = vec![w.clone()];
        match step(&mut list, &w, Err("The provider was removed".into()), later) {
            Outcome::Ended(_, n) => assert_eq!(n.body, "The provider was removed"),
            o => panic!("{o:?}"),
        }
        assert!(list.is_empty());
    }

    #[test]
    fn passive_needs_a_later_hour_and_a_listed_model() {
        let find = |id: &str| (id == "nr").then(gateway);
        let mut w = arm("nr", "cx", "cx/gpt-6.2-sol", None, "auto", at("2026-11-19T10:20:00Z")).unwrap();
        w.last_check = "2026-11-19T10:20:00Z".into();
        let hour = |s: &str| at(s).timestamp() / 3600;
        let list = vec![w];
        // Same hour as the check: not counted (could be before it).
        assert!(used_since(&list, &find, &[("cx/gpt-6-astra".into(), hour("2026-11-19T10:00:00Z"))]).is_empty());
        // Another group, or an id the gateway does not list: not counted.
        assert!(used_since(&list, &find, &[("cc/claude-opus-5-5".into(), hour("2026-11-19T11:00:00Z")), ("gpt-6-astra".into(), hour("2026-11-19T11:00:00Z"))]).is_empty());
        // A later hour, same group: the user is already using it.
        assert_eq!(used_since(&list, &find, &[("cx/gpt-6-astra".into(), hour("2026-11-19T11:00:00Z"))]), vec![("nr".to_string(), "cx".to_string())]);
    }
}
