//! Reports for the Usage page, built from the in-memory `usage::Index`.
//! Hours are stored in UTC and turned into local days/hours here.

use crate::usage::{App, Counted, IBucket, Index, Totals};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageFilter {
    /// Inclusive local days "YYYY-MM-DD".
    pub from: Option<String>,
    pub to: Option<String>,
    pub apps: Option<Vec<App>>,
    pub project: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageRow {
    pub key: String,
    pub label: String,
    pub app: Option<App>,
    pub project: Option<String>,
    pub last: String,
    pub subagents: u32,
    /// Per-model split of this row.
    pub models: BTreeMap<String, Totals>,
    #[serde(flatten)]
    pub c: Counted,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageBucket {
    pub start: String,
    #[serde(flatten)]
    pub t: Totals,
    pub by_app: BTreeMap<App, Totals>,
    pub by_model: BTreeMap<String, Totals>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageReport {
    pub filter: UsageFilter,
    pub total: Counted,
    pub prev_total: Counted,
    pub bucket: &'static str,
    pub buckets: Vec<UsageBucket>,
    pub by_app: Vec<UsageRow>,
    pub by_project: Vec<UsageRow>,
    pub by_model: Vec<UsageRow>,
    pub sessions: Vec<UsageRow>,
    pub projects: Vec<String>,
    pub models: Vec<String>,
    pub files: [u32; 3],
    pub skipped: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionDetail {
    pub key: String,
    pub label: String,
    pub app: App,
    pub project: Option<String>,
    pub threads: Vec<UsageRow>,
    pub by_model: Vec<UsageRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UsageBlock {
    pub start: String,
    pub end: String,
    pub active: bool,
    pub tokens_per_min: Option<f64>,
    pub projected: Option<u64>,
    #[serde(flatten)]
    pub c: Counted,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlocksReport {
    pub blocks: Vec<UsageBlock>,
    pub current: Option<UsageBlock>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Insight {
    pub level: &'static str,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Share {
    pub input: f64,
    pub cached: f64,
    pub cache_write: f64,
    pub output: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContextRow {
    #[serde(flatten)]
    pub row: UsageRow,
    pub max_context: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageAnalytics {
    pub share: Share,
    pub by_hour: Vec<u64>,
    pub main: Counted,
    pub subagents: Counted,
    pub cache_expiries: u64,
    pub biggest_context: Vec<ContextRow>,
    pub insights: Vec<Insight>,
}

// ------------------------------------------------------------------ helpers

pub fn rfc3339(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    DateTime::<Utc>::from_timestamp(secs, 0).map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)).unwrap_or_default()
}

fn local<Tz: TimeZone>(tz: &Tz, h: i64) -> DateTime<Tz> {
    tz.from_utc_datetime(&DateTime::<Utc>::from_timestamp(h * 3600, 0).unwrap_or_default().naive_utc())
}

fn parse_day(s: &Option<String>) -> Result<Option<NaiveDate>, String> {
    match s.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d").map(Some).map_err(|_| "Invalid date, expected YYYY-MM-DD".to_string()),
    }
}

/// App/project/model part of a filter, plus the local-day range.
struct Sel<'a, Tz: TimeZone> {
    idx: &'a Index,
    tz: &'a Tz,
    apps: [bool; 3],
    project: Option<&'a str>,
    /// Some(None): a model that has no data, so nothing matches.
    model: Option<Option<u32>>,
}

impl<'a, Tz: TimeZone> Sel<'a, Tz> {
    fn new(idx: &'a Index, f: &'a UsageFilter, tz: &'a Tz) -> Self {
        let apps = match f.apps.as_deref() {
            Some(a) if !a.is_empty() => App::ALL.map(|x| a.contains(&x)),
            _ => [true; 3],
        };
        let model = f.model.as_deref().filter(|m| !m.is_empty()).map(|m| idx.model_id(m));
        Sel { idx, tz, apps, project: f.project.as_deref().filter(|p| !p.is_empty()), model }
    }
    fn ok(&self, b: &IBucket) -> bool {
        self.apps[b.app]
            && self.project.is_none_or(|p| self.idx.sessions[b.session as usize].project == p)
            && self.model.is_none_or(|m| m == Some(b.model))
    }
    fn day(&self, b: &IBucket) -> NaiveDate {
        local(self.tz, b.h).date_naive()
    }
    fn matching(&self) -> impl Iterator<Item = &'a IBucket> + '_ {
        self.idx.buckets.iter().filter(move |b| self.ok(b))
    }
    fn in_range(&self, range: Option<(NaiveDate, NaiveDate)>) -> impl Iterator<Item = &'a IBucket> + '_ {
        self.matching().filter(move |b| range.is_some_and(|(f, t)| (f..=t).contains(&self.day(b))))
    }
}

/// Resolved inclusive local-day range. None when there is no bound and no data.
fn range<Tz: TimeZone>(sel: &Sel<Tz>, f: &UsageFilter, today: NaiveDate) -> Result<Option<(NaiveDate, NaiveDate)>, String> {
    let (from, to) = (parse_day(&f.from)?, parse_day(&f.to)?);
    let data = || {
        let mut it = sel.matching();
        let first = it.next().map(|b| sel.day(b));
        let last = sel.matching().last().map(|b| sel.day(b));
        first.zip(last)
    };
    let r = match (from, to) {
        (Some(a), Some(b)) => Some((a.min(b), a.max(b))),
        (Some(a), None) => Some((a, today.max(a))),
        (None, Some(b)) => Some((data().map(|(a, _)| a.min(b)).unwrap_or(b), b)),
        (None, None) => data(),
    };
    Ok(r)
}

#[derive(Default)]
struct Acc {
    c: Counted,
    last: i64,
    apps: u8,
    max_ctx: u64,
    models: HashMap<u32, Totals>,
}

impl Acc {
    fn add(&mut self, idx: &Index, b: &IBucket) {
        self.c.add(&b.t, b.turns);
        // The thread's last call, but not past the end of this hour.
        let tl = idx.threads[b.thread as usize].last;
        self.last = self.last.max(tl.min(b.h * 3600 + 3599));
        self.apps |= 1 << b.app;
        self.max_ctx = self.max_ctx.max(b.ctx);
        self.models.entry(b.model).or_default().add(&b.t);
    }
    fn models(&self, idx: &Index) -> BTreeMap<String, Totals> {
        self.models.iter().map(|(m, t)| (idx.models[*m as usize].clone(), *t)).collect()
    }
    fn app(&self) -> Option<App> {
        App::ALL.into_iter().find(|a| self.apps == 1 << a.idx())
    }
}

fn opt(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn sorted(mut rows: Vec<UsageRow>) -> Vec<UsageRow> {
    rows.sort_by(|a, b| b.c.t.sum().cmp(&a.c.t.sum()).then_with(|| a.key.cmp(&b.key)));
    rows
}

fn session_row(idx: &Index, si: u32, a: &Acc) -> UsageRow {
    let s = &idx.sessions[si as usize];
    UsageRow { key: s.key.clone(), label: s.label.clone(), app: Some(s.app), project: opt(&s.project), last: rfc3339(a.last), subagents: s.subagents, models: a.models(idx), c: a.c }
}

// ------------------------------------------------------------------ query

pub fn query<Tz: TimeZone>(idx: &Index, filter: &UsageFilter, tz: &Tz, now: DateTime<Utc>) -> Result<UsageReport, String> {
    let sel = Sel::new(idx, filter, tz);
    let today = now.with_timezone(tz).date_naive();
    let r = range(&sel, filter, today)?;
    let hourly = r.is_some_and(|(a, b)| a == b);

    let mut total = Counted::default();
    let mut bks: BTreeMap<String, UsageBucket> = BTreeMap::new();
    let mut by_app: BTreeMap<usize, Acc> = BTreeMap::new();
    let mut by_project: HashMap<&str, Acc> = HashMap::new();
    let mut by_model: HashMap<u32, Acc> = HashMap::new();
    let mut by_session: HashMap<u32, Acc> = HashMap::new();
    let key = |b: &IBucket| {
        let l = local(tz, b.h);
        if hourly { format!("{}T{:02}", l.date_naive(), l.hour()) } else { l.date_naive().to_string() }
    };
    let empty = |start: String| UsageBucket { start, t: Totals::default(), by_app: BTreeMap::new(), by_model: BTreeMap::new() };
    if let Some((a, b)) = r {
        // Every bucket in the range, empty ones included.
        if hourly {
            for h in 0..24 {
                let k = format!("{a}T{h:02}");
                bks.insert(k.clone(), empty(k));
            }
        } else {
            let mut d = a;
            let mut n = 0;
            while d <= b && n < 20_000 {
                bks.insert(d.to_string(), empty(d.to_string()));
                d += Duration::days(1);
                n += 1;
            }
        }
    }
    for b in sel.in_range(r) {
        total.add(&b.t, b.turns);
        let k = key(b);
        let e = bks.entry(k.clone()).or_insert_with(|| empty(k));
        e.t.add(&b.t);
        e.by_app.entry(App::ALL[b.app]).or_default().add(&b.t);
        e.by_model.entry(idx.models[b.model as usize].clone()).or_default().add(&b.t);
        by_app.entry(b.app).or_default().add(idx, b);
        by_project.entry(idx.sessions[b.session as usize].project.as_str()).or_default().add(idx, b);
        by_model.entry(b.model).or_default().add(idx, b);
        by_session.entry(b.session).or_default().add(idx, b);
    }

    let mut prev_total = Counted::default();
    if let (Some(_), Some((a, b))) = (&filter.from, r) {
        let len = (b - a).num_days() + 1;
        let prev = Some((a - Duration::days(len), a - Duration::days(1)));
        for x in sel.in_range(prev) {
            prev_total.add(&x.t, x.turns);
        }
    }

    let by_app = App::ALL
        .into_iter()
        .filter_map(|app| {
            let a = by_app.get(&app.idx())?;
            Some(UsageRow { key: app.name().into(), label: app.title().into(), app: Some(app), project: None, last: rfc3339(a.last), subagents: 0, models: a.models(idx), c: a.c })
        })
        .collect();
    let by_project = by_project.iter().map(|(p, a)| UsageRow { key: p.to_string(), label: p.to_string(), app: a.app(), project: opt(p), last: rfc3339(a.last), subagents: 0, models: a.models(idx), c: a.c }).collect();
    let by_model = by_model
        .iter()
        .map(|(m, a)| {
            let name = idx.models[*m as usize].clone();
            UsageRow { key: name.clone(), label: name, app: a.app(), project: None, last: rfc3339(a.last), subagents: 0, models: a.models(idx), c: a.c }
        })
        .collect();
    let mut sessions = sorted(by_session.iter().map(|(s, a)| session_row(idx, *s, a)).collect());
    sessions.truncate(100);

    Ok(UsageReport {
        filter: filter.clone(),
        total,
        prev_total,
        bucket: if hourly { "hour" } else { "day" },
        buckets: bks.into_values().collect(),
        by_app,
        by_project: sorted(by_project),
        by_model: sorted(by_model),
        sessions,
        projects: idx.project_names.clone(),
        models: idx.model_names.clone(),
        files: idx.files,
        skipped: idx.skipped,
    })
}

// ------------------------------------------------------------------ session

pub fn session(idx: &Index, key: &str) -> Result<SessionDetail, String> {
    let si = *idx.session_by_key.get(key).ok_or("Session not found")?;
    let s = &idx.sessions[si as usize];
    let mut threads: HashMap<u32, Acc> = HashMap::new();
    let mut models: HashMap<u32, Acc> = HashMap::new();
    for b in idx.buckets.iter().filter(|b| b.session == si) {
        threads.entry(b.thread).or_default().add(idx, b);
        models.entry(b.model).or_default().add(idx, b);
    }
    let mut rows: Vec<(bool, UsageRow)> = threads
        .iter()
        .map(|(t, a)| {
            let th = &idx.threads[*t as usize];
            let label = if th.sub { if th.label.is_empty() { th.id.rsplit('/').next().unwrap_or("").chars().take(12).collect() } else { th.label.clone() } } else { s.label.clone() };
            (th.sub, UsageRow { key: th.id.clone(), label, app: Some(th.app), project: opt(&th.project), last: rfc3339(a.last), subagents: 0, models: a.models(idx), c: a.c })
        })
        .collect();
    // Main thread first, then subagents by size.
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.c.t.sum().cmp(&a.1.c.t.sum())).then_with(|| a.1.key.cmp(&b.1.key)));
    let by_model = models
        .iter()
        .map(|(m, a)| {
            let name = idx.models[*m as usize].clone();
            UsageRow { key: name.clone(), label: name, app: Some(s.app), project: None, last: rfc3339(a.last), subagents: 0, models: a.models(idx), c: a.c }
        })
        .collect();
    Ok(SessionDetail { key: s.key.clone(), label: s.label.clone(), app: s.app, project: opt(&s.project), threads: rows.into_iter().map(|r| r.1).collect(), by_model: sorted(by_model) })
}

// ------------------------------------------------------------------ blocks

const BLOCK_HOURS: i64 = 5;

/// 5-hour windows like Claude plans: a window starts at the hour of the first
/// call after the previous window ended. Starts are whole hours, so hourly
/// buckets split exactly. Oldest first.
pub fn blocks<Tz: TimeZone>(idx: &Index, filter: &UsageFilter, tz: &Tz, now: DateTime<Utc>) -> Result<BlocksReport, String> {
    let sel = Sel::new(idx, filter, tz);
    let bounded = filter.from.is_some() || filter.to.is_some();
    let r = if bounded { range(&sel, filter, now.with_timezone(tz).date_naive())? } else { None };
    let mut hours: BTreeMap<i64, Counted> = BTreeMap::new();
    for b in sel.matching().filter(|b| !bounded || r.is_some_and(|(f, t)| (f..=t).contains(&sel.day(b)))) {
        hours.entry(b.h).or_default().add(&b.t, b.turns);
    }
    let blocks = split_blocks(&hours, now);
    let current = blocks.last().filter(|b| b.active).cloned();
    Ok(BlocksReport { blocks, current })
}

fn split_blocks(hours: &BTreeMap<i64, Counted>, now: DateTime<Utc>) -> Vec<UsageBlock> {
    let mut out: Vec<(i64, Counted)> = vec![];
    for (h, c) in hours {
        match out.last_mut() {
            Some((start, acc)) if *h < *start + BLOCK_HOURS => acc.add(&c.t, c.turns),
            _ => out.push((*h, *c)),
        }
    }
    let now_s = now.timestamp();
    out.into_iter()
        .map(|(h, c)| {
            let (start, end) = (h * 3600, (h + BLOCK_HOURS) * 3600);
            let active = now_s >= start && now_s < end;
            let (tpm, projected) = if active {
                let mins = ((now_s - start) as f64 / 60.0).max(1.0);
                let tpm = c.t.sum() as f64 / mins;
                (Some((tpm * 10.0).round() / 10.0), Some(c.t.sum() + (tpm * (end - now_s) as f64 / 60.0).round() as u64))
            } else {
                (None, None)
            };
            UsageBlock { start: rfc3339(start), end: rfc3339(end), active, tokens_per_min: tpm, projected, c }
        })
        .collect()
}

// ------------------------------------------------------------------ analytics

pub fn analytics<Tz: TimeZone>(idx: &Index, filter: &UsageFilter, tz: &Tz, now: DateTime<Utc>) -> Result<UsageAnalytics, String> {
    let sel = Sel::new(idx, filter, tz);
    let r = range(&sel, filter, now.with_timezone(tz).date_naive())?;
    let mut total = Counted::default();
    let (mut main, mut subs) = (Counted::default(), Counted::default());
    let mut by_hour = vec![0u64; 24];
    let mut expiries = 0;
    let mut sessions: HashMap<u32, Acc> = HashMap::new();
    let mut sub_models: HashMap<u32, u64> = HashMap::new();
    let mut rewrite = 0;
    for b in sel.in_range(r) {
        total.add(&b.t, b.turns);
        let sub = idx.threads[b.thread as usize].sub;
        if sub { &mut subs } else { &mut main }.add(&b.t, b.turns);
        by_hour[local(tz, b.h).hour() as usize] += b.t.sum();
        expiries += b.expiries;
        rewrite += b.rewrite;
        sessions.entry(b.session).or_default().add(idx, b);
        if sub {
            *sub_models.entry(b.model).or_default() += b.t.sum();
        }
    }
    let all = total.t.sum();
    let f = |x: u64| if all == 0 { 0.0 } else { x as f64 / all as f64 };
    let share = Share { input: f(total.t.input), cached: f(total.t.cached), cache_write: f(total.t.cache_write), output: f(total.t.output) };
    let mut ctx: Vec<ContextRow> = sessions.iter().filter(|(_, a)| a.max_ctx > 0).map(|(s, a)| ContextRow { row: session_row(idx, *s, a), max_context: a.max_ctx }).collect();
    ctx.sort_by(|a, b| b.max_context.cmp(&a.max_context).then_with(|| a.row.key.cmp(&b.row.key)));
    ctx.truncate(10);
    let sub_model = sub_models.iter().max_by_key(|(m, n)| (**n, std::cmp::Reverse(**m))).map(|(m, _)| idx.models[*m as usize].as_str());
    let max_context = ctx.first().map(|c| c.max_context).unwrap_or(0);
    let insights = insights(&Facts { total, subs, expiries, rewrite, max_context, by_hour: &by_hour, sub_model });
    Ok(UsageAnalytics { share, by_hour, main, subagents: subs, cache_expiries: expiries, biggest_context: ctx, insights })
}

struct Facts<'a> {
    total: Counted,
    subs: Counted,
    expiries: u64,
    /// Cache-write tokens of the expiry turns.
    rewrite: u64,
    /// Largest single-turn context of any session.
    max_context: u64,
    by_hour: &'a [u64],
    /// Model that used most of the subagent tokens.
    sub_model: Option<&'a str>,
}

fn pct(x: u64, of: u64) -> u64 {
    if of == 0 { 0 } else { ((x as f64 / of as f64) * 100.0).round() as u64 }
}

/// Token amount in the page's style: "950", "12k", "1.2M", "3.4B".
pub fn fmt_tokens(n: u64) -> String {
    let k = (n as f64 / 1e3).round() as u64;
    if n < 1_000 {
        n.to_string()
    } else if k < 1_000 {
        format!("{k}k")
    } else if n < 999_950_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else {
        format!("{:.1}B", n as f64 / 1e9)
    }
}

/// Contexts above this make the "long context" insight.
const LONG_CONTEXT: u64 = 150_000;

/// Plain-English findings. The page translates them by these exact templates,
/// so the wording must not change without updating its i18n keys.
fn insights(f: &Facts) -> Vec<Insight> {
    let all = f.total.t.sum();
    let mut out = vec![];
    if all == 0 {
        return out;
    }
    if f.expiries >= 3 && f.rewrite > 0 {
        out.push(Insight {
            level: "warn",
            title: format!("{} cache rewrites after idle gaps", f.expiries),
            detail: format!("A pause longer than 5 minutes lets the prompt cache expire, so the next turn writes the whole context again. These rewrites cost {} tokens.", fmt_tokens(f.rewrite)),
        });
    }
    if f.max_context >= LONG_CONTEXT {
        out.push(Insight {
            level: "warn",
            title: format!("One session reached {} tokens of context", fmt_tokens(f.max_context)),
            detail: "Long contexts make every turn expensive. Start a new session or compact when the task changes.".into(),
        });
    }
    let s = pct(f.subs.t.sum(), all);
    if s >= 30 {
        if let Some(m) = f.sub_model.filter(|m| !m.is_empty()) {
            out.push(Insight { level: "info", title: format!("Subagents used {s}% of tokens"), detail: format!("Most of it went to {m}.") });
        }
    }
    if let Some((h, n)) = f.by_hour.iter().enumerate().max_by_key(|(h, n)| (**n, std::cmp::Reverse(*h))).filter(|(_, n)| **n > 0) {
        out.push(Insight {
            level: "info",
            title: format!("Busiest hour: {h:02}:00"),
            detail: format!("{}% of tokens were spent between {h:02}:00 and {:02}:00.", pct(*n, all), (h + 1) % 24),
        });
    }
    out
}

// ------------------------------------------------------------------ export

#[derive(Serialize)]
struct ExportRow<'a> {
    date: String,
    app: &'static str,
    project: &'a str,
    session: &'a str,
    model: &'a str,
    turns: u64,
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// One row per (local day, app, session, model) in the filter.
pub fn export_text<Tz: TimeZone>(idx: &Index, filter: &UsageFilter, tz: &Tz, now: DateTime<Utc>, format: &str) -> Result<String, String> {
    let sel = Sel::new(idx, filter, tz);
    let r = range(&sel, filter, now.with_timezone(tz).date_naive())?;
    let mut rows: BTreeMap<(NaiveDate, usize, u32, u32), Counted> = BTreeMap::new();
    for b in sel.in_range(r) {
        rows.entry((sel.day(b), b.app, b.session, b.model)).or_default().add(&b.t, b.turns);
    }
    let rows: Vec<ExportRow> = rows
        .into_iter()
        .map(|((d, app, s, m), c)| {
            let s = &idx.sessions[s as usize];
            ExportRow { date: d.to_string(), app: App::ALL[app].name(), project: &s.project, session: &s.key, model: &idx.models[m as usize], turns: c.turns, input: c.t.input, cached: c.t.cached, cache_write: c.t.cache_write, output: c.t.output }
        })
        .collect();
    match format {
        "json" => serde_json::to_string_pretty(&rows).map_err(|e| e.to_string()),
        "csv" => {
            let mut s = String::from("date,app,project,session,model,turns,input,cached,cache_write,output\n");
            for r in &rows {
                s += &format!("{},{},{},{},{},{},{},{},{},{}\n", r.date, r.app, csv_field(r.project), csv_field(r.session), csv_field(r.model), r.turns, r.input, r.cached, r.cache_write, r.output);
            }
            Ok(s)
        }
        _ => Err("Unknown export format".into()),
    }
}

// ------------------------------------------------------------------ cheap helpers (tray / overlay)

/// Today's local totals.
pub fn today<Tz: TimeZone>(idx: &Index, tz: &Tz, now: DateTime<Utc>) -> Counted {
    let d = now.with_timezone(tz).date_naive();
    let mut c = Counted::default();
    // Buckets are sorted by hour: only the last ~day needs looking at.
    let from_h = now.timestamp().div_euclid(3600) - 48;
    let start = idx.buckets.partition_point(|b| b.h < from_h);
    for b in &idx.buckets[start..] {
        if local(tz, b.h).date_naive() == d {
            c.add(&b.t, b.turns);
        }
    }
    c
}

/// The 5-hour block running now, over all apps.
pub fn current_block(idx: &Index, now: DateTime<Utc>) -> Option<UsageBlock> {
    // A block's start depends on earlier blocks, but a quiet gap of 5 hours
    // always starts a new one, so walking back to the last such gap is enough.
    let mut hours: BTreeMap<i64, Counted> = BTreeMap::new();
    let mut prev: Option<i64> = None;
    for b in idx.buckets.iter().rev() {
        if prev.is_some_and(|p| p - b.h >= BLOCK_HOURS) {
            break;
        }
        hours.entry(b.h).or_default().add(&b.t, b.turns);
        prev = Some(b.h);
    }
    split_blocks(&hours, now).pop().filter(|b| b.active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::tests::{t, Fixture};
    use crate::usage::Shared;
    use chrono::FixedOffset;

    fn claude_line(ts: &str, sid: &str, id: &str, cwd: &str, model: &str, i: u64, o: u64) -> String {
        format!(r#"{{"timestamp":"{ts}","sessionId":"{sid}","cwd":"{cwd}","requestId":"{id}","message":{{"id":"{id}","model":"{model}","usage":{{"input_tokens":{i},"output_tokens":{o}}}}}}}"#)
    }

    fn fixture() -> (Fixture, Shared) {
        let fx = Fixture::new();
        let lines = [
            claude_line("2026-11-05T20:00:00Z", "s1", "a", "/w/shop", "m1", 10, 1),
            claude_line("2026-11-06T01:00:00Z", "s1", "b", "/w/shop", "m1", 20, 2),
            claude_line("2026-11-06T09:00:00Z", "s1", "c", "/w/shop", "m2", 40, 4),
        ];
        fx.write("claude/p/s1.jsonl", &format!("{}\n", lines.join("\n")));
        fx.write("claude/p/s2.jsonl", &format!("{}\n", claude_line("2026-11-03T03:00:00Z", "s2", "d", "/w/blog", "m1", 100, 10)));
        fx.write("codex/rollout-1.jsonl", crate::usage::tests::CODEX);
        let mut s = fx.shared();
        s.refresh();
        (fx, s)
    }

    fn filter(from: Option<&str>, to: Option<&str>) -> UsageFilter {
        UsageFilter { from: from.map(Into::into), to: to.map(Into::into), ..Default::default() }
    }

    fn now() -> DateTime<Utc> {
        "2026-11-06T10:30:00Z".parse().unwrap()
    }

    #[test]
    fn days_and_hours_are_local() {
        let (_fx, mut s) = fixture();
        let idx = s.index(false);
        let vn = FixedOffset::east_opt(7 * 3600).unwrap();
        // 2026-11-05T20:00Z is 03:00 on the 6th in Vietnam; 2026-11-06T09:00Z is 16:00.
        let r = query(idx, &UsageFilter { apps: Some(vec![App::Claude]), ..filter(Some("2026-11-06"), Some("2026-11-06")) }, &vn, now()).unwrap();
        assert_eq!(r.bucket, "hour");
        assert_eq!(r.buckets.len(), 24);
        assert_eq!(r.buckets[3].start, "2026-11-06T03");
        assert_eq!(r.buckets[3].t, t(10, 0, 0, 1));
        assert_eq!(r.buckets[8].t, t(20, 0, 0, 2));
        assert_eq!(r.buckets[16].by_model["m2"], t(40, 0, 0, 4));
        assert_eq!(r.total.t, t(70, 0, 0, 7));
        assert_eq!(r.total.turns, 3);
        // In UTC the first call falls on the 5th.
        let r = query(idx, &UsageFilter { apps: Some(vec![App::Claude]), ..filter(Some("2026-11-06"), Some("2026-11-06")) }, &Utc, now()).unwrap();
        assert_eq!(r.total.t, t(60, 0, 0, 6));
    }

    #[test]
    fn filters_range_and_prev_total() {
        let (_fx, mut s) = fixture();
        let idx = s.index(false);
        // No bounds: first..last day with data, every day filled.
        let r = query(idx, &filter(None, None), &Utc, now()).unwrap();
        assert_eq!(r.bucket, "day");
        assert_eq!(r.buckets.first().unwrap().start, "2026-11-03");
        assert_eq!(r.buckets.last().unwrap().start, "2026-11-06");
        assert_eq!(r.buckets.len(), 4);
        assert_eq!(r.prev_total, Counted::default());
        assert_eq!(r.by_app.len(), 2);
        assert_eq!(r.projects, vec!["blog".to_string(), "repo".into(), "shop".into()]);
        // Project filter.
        let r = query(idx, &UsageFilter { project: Some("blog".into()), ..filter(None, None) }, &Utc, now()).unwrap();
        assert_eq!(r.total.t, t(100, 0, 0, 10));
        assert_eq!(r.sessions.len(), 1);
        assert_eq!(r.sessions[0].key, "claude:s2");
        // Model filter, including a model with no data.
        let r = query(idx, &UsageFilter { model: Some("m2".into()), ..filter(None, None) }, &Utc, now()).unwrap();
        assert_eq!(r.total.t, t(40, 0, 0, 4));
        let r = query(idx, &UsageFilter { model: Some("nope".into()), ..filter(None, None) }, &Utc, now()).unwrap();
        assert_eq!(r.total, Counted::default());
        // prev_total: the 2 days before 05..06 are 03..04.
        let r = query(idx, &UsageFilter { apps: Some(vec![App::Claude]), ..filter(Some("2026-11-05"), Some("2026-11-06")) }, &Utc, now()).unwrap();
        assert_eq!(r.total.t, t(70, 0, 0, 7));
        assert_eq!(r.prev_total.t, t(100, 0, 0, 10));
        assert_eq!(r.prev_total.turns, 1);
        // from only: up to today.
        let r = query(idx, &filter(Some("2026-11-04"), None), &Utc, now()).unwrap();
        assert_eq!(r.buckets.len(), 3);
        assert!(query(idx, &filter(Some("Nov 4"), None), &Utc, now()).is_err());
    }

    #[test]
    fn session_detail_and_rows_serialize_like_the_contract() {
        let (_fx, mut s) = fixture();
        let idx = s.index(false);
        let d = session(idx, "claude:s1").unwrap();
        assert_eq!(d.threads.len(), 1);
        assert_eq!(d.by_model[0].key, "m2");
        assert!(session(idx, "claude:none").is_err());
        let r = query(idx, &filter(None, None), &Utc, now()).unwrap();
        let v = serde_json::to_value(&r).unwrap();
        for k in ["filter", "total", "prev_total", "bucket", "buckets", "by_app", "by_project", "by_model", "sessions", "projects", "models", "files", "skipped"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let row = &v["sessions"][0];
        let s1 = v["sessions"].as_array().unwrap().iter().find(|r| r["key"] == "claude:s1").unwrap();
        assert_eq!(s1["models"]["m1"]["input"], 30, "per-model split of the row");
        assert_eq!(s1["models"]["m2"]["output"], 4);
        for k in ["key", "label", "app", "project", "last", "subagents", "models", "input", "cached", "cache_write", "output", "turns"] {
            assert!(row.get(k).is_some(), "{k}");
        }
        assert_eq!(v["by_app"][0]["app"], "claude");
        assert!(v["buckets"][0]["by_app"].is_object());
        assert_eq!(v["filter"]["apps"], serde_json::Value::Null);
        let f: UsageFilter = serde_json::from_str(r#"{"from":null,"to":null,"apps":["codex"],"project":null,"model":null}"#).unwrap();
        assert_eq!(f.apps, Some(vec![App::Codex]));
    }

    #[test]
    fn blocks_split_on_five_hours() {
        let c = |n: u64| Counted { t: t(n, 0, 0, 0), turns: 1 };
        let h0 = "2026-11-06T00:00:00Z".parse::<DateTime<Utc>>().unwrap().timestamp() / 3600;
        // Calls at 00, 02, 04 (one block 00-05), 05 (new block 05-10), 12 (new block 12-17).
        let hours: BTreeMap<i64, Counted> = [(h0, c(1)), (h0 + 2, c(2)), (h0 + 4, c(3)), (h0 + 5, c(4)), (h0 + 12, c(5))].into();
        let now: DateTime<Utc> = "2026-11-06T13:00:00Z".parse().unwrap();
        let b = split_blocks(&hours, now);
        assert_eq!(b.len(), 3);
        assert_eq!((b[0].start.as_str(), b[0].end.as_str(), b[0].c.t.input), ("2026-11-06T00:00:00Z", "2026-11-06T05:00:00Z", 6));
        assert_eq!(b[1].c.t.input, 4);
        assert!(!b[1].active && b[2].active);
        assert_eq!(b[2].tokens_per_min, Some(0.1));
        // 5 tokens in 60 min, 240 min left: 5 + 20 = 25.
        assert_eq!(b[2].projected, Some(25));
    }

    #[test]
    fn current_block_and_today() {
        let (_fx, mut s) = fixture();
        let idx = s.index(false);
        // 2026-11-06T09:00Z call starts a block 09-14 that covers 10:30.
        let b = current_block(idx, now()).unwrap();
        assert_eq!(b.start, "2026-11-06T09:00:00Z");
        assert_eq!(b.c.t, t(140, 1900, 0, 44), "claude 09:00 plus codex 12:00");
        let r = blocks(idx, &filter(None, None), &Utc, now()).unwrap();
        assert_eq!(r.current, Some(b));
        assert_eq!(today(idx, &Utc, now()).t, t(160, 1900, 0, 46), "claude 20+40 plus codex's 6th");
    }

    #[test]
    fn analytics_and_insights() {
        let (_fx, mut s) = fixture();
        let idx = s.index(false);
        let a = analytics(idx, &filter(None, None), &Utc, now()).unwrap();
        let sum = a.share.input + a.share.cached + a.share.cache_write + a.share.output;
        assert!((sum - 1.0).abs() < 1e-9);
        assert_eq!(a.by_hour.len(), 24);
        assert_eq!(a.subagents, Counted::default());
        assert!(!a.insights.is_empty() && a.insights.len() <= 6);
        assert_eq!(a.biggest_context[0].row.key, "codex:th1");
        let v = serde_json::to_value(&a).unwrap();
        assert!(v["biggest_context"][0].get("max_context").is_some() && v["biggest_context"][0].get("key").is_some());
        let csv = export_text(idx, &filter(None, None), &Utc, now(), "csv").unwrap();
        assert!(csv.starts_with("date,app,project,session,model"));
        assert!(csv.contains("2026-11-03,claude,blog,claude:s2,m1,1,100,0,0,10"));
        assert!(export_text(idx, &filter(None, None), &Utc, now(), "xml").is_err());
    }

    #[test]
    fn insight_templates() {
        let mut hours = [0u64; 24];
        hours[9] = 50;
        let f = |expiries: u64, rewrite: u64, max_context: u64, subs: u64| {
            insights(&Facts { total: Counted { t: t(100, 0, 0, 0), turns: 10 }, subs: Counted { t: t(subs, 0, 0, 0), turns: 1 }, expiries, rewrite, max_context, by_hour: &hours, sub_model: Some("m1") })
        };
        let i = f(12, 1_234_567, 200_000, 40);
        assert_eq!(i[0].title, "12 cache rewrites after idle gaps");
        assert!(i[0].detail.ends_with("These rewrites cost 1.2M tokens."));
        assert_eq!(fmt_tokens(999_950), "1.0M", "no 1000k");
        assert_eq!(i[1].title, "One session reached 200k tokens of context");
        assert_eq!((i[2].title.as_str(), i[2].detail.as_str()), ("Subagents used 40% of tokens", "Most of it went to m1."));
        assert_eq!((i[3].title.as_str(), i[3].detail.as_str()), ("Busiest hour: 09:00", "50% of tokens were spent between 09:00 and 10:00."));
        assert_eq!(f(0, 0, 0, 0).len(), 1, "only the busiest hour");
        assert_eq!((fmt_tokens(950), fmt_tokens(12_345), fmt_tokens(3_000_000_000)), ("950".to_string(), "12k".to_string(), "3.0B".to_string()));
        let all = f(0, 0, 0, 0);
        assert!(all.iter().all(|x| !x.title.is_empty()) && insights(&Facts { total: Counted::default(), subs: Counted::default(), expiries: 0, rewrite: 0, max_context: 0, by_hour: &hours, sub_model: None }).is_empty());
    }

    /// `cargo test --lib -- --ignored usage_live --nocapture`
    /// Reads the real logs into a temporary cache (the app's own cache is not touched).
    #[test]
    #[ignore]
    fn usage_live() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("usage-cache.json");
        let mut timings = vec![];
        for run in ["cold", "warm"] {
            let mut s = Shared::new(crate::usage::Roots::real(), cache.clone());
            let t0 = std::time::Instant::now();
            s.refresh();
            s.flush();
            let scan = t0.elapsed();
            let t1 = std::time::Instant::now();
            let r = query(s.index(false), &UsageFilter::default(), &chrono::Local, Utc::now()).unwrap();
            let q = t1.elapsed();
            let t2 = std::time::Instant::now();
            let _ = analytics(s.index(false), &UsageFilter::default(), &chrono::Local, Utc::now()).unwrap();
            let _ = blocks(s.index(false), &UsageFilter::default(), &chrono::Local, Utc::now()).unwrap();
            let a = t2.elapsed();
            println!("{run}: scan+load {scan:?}, query {q:?}, analytics+blocks {a:?}; files {:?} skipped {}", r.files, r.skipped);
            for row in &r.by_app {
                println!("  {}: {:?} turns {}", row.key, row.c.t, row.c.turns);
            }
            println!("  buckets {} sessions(top) {} projects {} models {}", r.buckets.len(), r.sessions.len(), r.projects.len(), r.models.len());
            timings.push(scan);
        }
        println!("cache file {} KB", std::fs::metadata(&cache).map(|m| m.len() / 1024).unwrap_or(0));
    }
}
