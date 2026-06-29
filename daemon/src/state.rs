//! Authoritative in-memory blocklist + session state, mirrored to SQLite.
//!
//! [`StateManager`] owns the canonical state and serializes all mutations
//! behind a mutex. Every mutation is validated, applied to the in-memory sets,
//! and persisted via [`crate::persist::Store`] before being acknowledged, so a
//! restart reloads exactly what callers were told succeeded.
//!
//! Enforcement (pf / DNS) is **not** wired here yet — M2 only owns the truth.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::info;

use crate::persist::Store;

/// How many recent blocked queries to retain for the statistics view.
const RECENT_CAP: usize = 50;
/// How many top entries to report in a stats snapshot.
const TOP_CAP: usize = 20;

/// An active blocking session. Stored from M5; in M2 it is only ever read back
/// from persistence (no command sets it yet).
#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub until_unix: i64,
    pub committed: bool,
}

/// A consistent point-in-time copy of all state.
#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub domains: BTreeSet<String>,
    pub cidrs: BTreeSet<String>,
    pub session: Option<Session>,
}

/// Result of a mutating command: how many entries actually changed, and the
/// resulting set size.
#[derive(Debug)]
pub struct MutationResult {
    pub changed: usize,
    pub total: usize,
}

/// One blocked query, as shown in the statistics "recent activity" feed.
#[derive(Debug, Clone, Serialize)]
pub struct RecentBlock {
    /// The fully-qualified name that was queried (lowercased, no trailing dot).
    pub name: String,
    /// When it was blocked (seconds since the Unix epoch).
    pub unix: i64,
}

/// Per-entry hit count for the statistics "top blocked" view.
#[derive(Debug, Clone, Serialize)]
pub struct DomainStat {
    /// The blocklist entry that matched (in its stored form, e.g. `*.ads.com`).
    pub entry: String,
    pub count: u64,
}

/// A point-in-time copy of blocking statistics for `GetStats`.
#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    /// When stat collection started (daemon start), seconds since the epoch.
    pub since_unix: i64,
    /// Total blocked DNS queries since `since_unix`.
    pub total_blocked: u64,
    /// Number of distinct blocklist entries that have been hit at least once.
    pub unique_domains: u64,
    /// Most-hit entries, descending (capped at [`TOP_CAP`]).
    pub top: Vec<DomainStat>,
    /// Most recent blocked queries, newest first (capped at [`RECENT_CAP`]).
    pub recent: Vec<RecentBlock>,
}

/// In-memory blocking statistics. Reset on restart (enforcement only runs while
/// the daemon is up), kept cheap so recording a hit never stalls a DNS reply.
#[derive(Default)]
struct Stats {
    since_unix: i64,
    total: u64,
    per_entry: HashMap<String, u64>,
    recent: VecDeque<RecentBlock>,
}

/// Errors from state mutations.
#[derive(thiserror::Error, Debug)]
pub enum StateError {
    /// Client-supplied input was invalid. The message is safe to echo back.
    #[error("{0}")]
    Invalid(String),
    /// Persistence failure. Details are logged, not returned to the client.
    #[error("storage error: {0}")]
    Storage(#[from] rusqlite::Error),
}

fn invalid(msg: impl Into<String>) -> StateError {
    StateError::Invalid(msg.into())
}

/// Thread-safe owner of the authoritative state.
pub struct StateManager {
    inner: Mutex<Inner>,
    /// Whether the loopback block-page server is serving (set once at startup).
    /// Surfaced in `GetStatus` so the GUI can show how blocks are presented.
    block_page: AtomicBool,
}

struct Inner {
    snapshot: Snapshot,
    store: Store,
    stats: Stats,
}

impl StateManager {
    /// Build a manager from a store, loading any persisted state into memory.
    pub fn new(store: Store) -> Result<Self, StateError> {
        let snapshot = store.load()?;
        info!(
            domains = snapshot.domains.len(),
            cidrs = snapshot.cidrs.len(),
            session = snapshot.session.is_some(),
            "loaded persisted state"
        );
        let stats = Stats { since_unix: now_unix(), ..Stats::default() };
        Ok(Self {
            inner: Mutex::new(Inner { snapshot, store, stats }),
            block_page: AtomicBool::new(false),
        })
    }

    /// Record whether the loopback block-page server bound successfully.
    pub fn set_block_page_active(&self, active: bool) {
        self.block_page.store(active, Ordering::Relaxed);
    }

    /// Whether blocked domains resolve to the loopback block page (vs. NXDOMAIN).
    pub fn block_page_active(&self) -> bool {
        self.block_page.load(Ordering::Relaxed)
    }

    /// Lock the state, recovering from a poisoned mutex so one panicked
    /// connection task can't wedge the whole daemon.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A clone of the current state, for `GetStatus`.
    pub fn summary(&self) -> Snapshot {
        self.lock().snapshot.clone()
    }

    /// Evaluate a DNS query against the blocklist and, if blocked, record the hit
    /// for statistics. Returns the matching blocklist entry (in stored form).
    ///
    /// This is the hot path on every DNS lookup, so it takes the state lock once
    /// and does only cheap bookkeeping under it.
    pub fn on_query(&self, qname: &str) -> Option<String> {
        let mut g = self.lock();
        let entry = matching_entry(&g.snapshot.domains, qname)?.to_string();
        let name = normalize_query(qname);
        let stats = &mut g.stats;
        stats.total += 1;
        *stats.per_entry.entry(entry.clone()).or_insert(0) += 1;
        stats.recent.push_front(RecentBlock { name, unix: now_unix() });
        stats.recent.truncate(RECENT_CAP);
        Some(entry)
    }

    /// A consistent copy of current blocking statistics, for `GetStats`.
    pub fn stats_snapshot(&self) -> StatsSnapshot {
        let g = self.lock();
        let s = &g.stats;
        let mut top: Vec<DomainStat> = s
            .per_entry
            .iter()
            .map(|(entry, count)| DomainStat { entry: entry.clone(), count: *count })
            .collect();
        // Highest count first; ties broken by entry name for a stable order.
        top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.entry.cmp(&b.entry)));
        top.truncate(TOP_CAP);
        StatsSnapshot {
            since_unix: s.since_unix,
            total_blocked: s.total,
            unique_domains: s.per_entry.len() as u64,
            top,
            recent: s.recent.iter().cloned().collect(),
        }
    }

    pub fn add_domains(&self, raw: &[String]) -> Result<MutationResult, StateError> {
        let normalized = normalize_all(raw, normalize_domain)?;
        let mut g = self.lock();
        let added: Vec<String> = normalized
            .into_iter()
            .filter(|d| !g.snapshot.domains.contains(d))
            .collect();
        if !added.is_empty() {
            g.store.add_domains(&added)?;
            for d in &added {
                g.snapshot.domains.insert(d.clone());
            }
            info!(added = added.len(), total = g.snapshot.domains.len(), "added domains");
        }
        let total = g.snapshot.domains.len();
        Ok(MutationResult { changed: added.len(), total })
    }

    pub fn remove_domains(&self, raw: &[String]) -> Result<MutationResult, StateError> {
        // M5: reject this while a committed session is active.
        let normalized = normalize_all(raw, normalize_domain)?;
        let mut g = self.lock();
        let removed: Vec<String> = normalized
            .into_iter()
            .filter(|d| g.snapshot.domains.contains(d))
            .collect();
        if !removed.is_empty() {
            g.store.remove_domains(&removed)?;
            for d in &removed {
                g.snapshot.domains.remove(d);
            }
            info!(removed = removed.len(), total = g.snapshot.domains.len(), "removed domains");
        }
        let total = g.snapshot.domains.len();
        Ok(MutationResult { changed: removed.len(), total })
    }

    pub fn add_cidrs(&self, raw: &[String]) -> Result<MutationResult, StateError> {
        let normalized = normalize_all(raw, normalize_cidr)?;
        let mut g = self.lock();
        let added: Vec<String> = normalized
            .into_iter()
            .filter(|c| !g.snapshot.cidrs.contains(c))
            .collect();
        if !added.is_empty() {
            g.store.add_cidrs(&added)?;
            for c in &added {
                g.snapshot.cidrs.insert(c.clone());
            }
            info!(added = added.len(), total = g.snapshot.cidrs.len(), "added cidrs");
        }
        let total = g.snapshot.cidrs.len();
        Ok(MutationResult { changed: added.len(), total })
    }

    pub fn remove_cidrs(&self, raw: &[String]) -> Result<MutationResult, StateError> {
        // M5: reject this while a committed session is active.
        let normalized = normalize_all(raw, normalize_cidr)?;
        let mut g = self.lock();
        let removed: Vec<String> = normalized
            .into_iter()
            .filter(|c| g.snapshot.cidrs.contains(c))
            .collect();
        if !removed.is_empty() {
            g.store.remove_cidrs(&removed)?;
            for c in &removed {
                g.snapshot.cidrs.remove(c);
            }
            info!(removed = removed.len(), total = g.snapshot.cidrs.len(), "removed cidrs");
        }
        let total = g.snapshot.cidrs.len();
        Ok(MutationResult { changed: removed.len(), total })
    }
}

/// Canonicalize a query name for matching: trimmed, lowercased, no trailing dot.
fn normalize_query(qname: &str) -> String {
    qname.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Seconds since the Unix epoch (saturating to 0 on a pre-1970 clock).
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The first blocklist entry that covers `qname`, if any. `qname` must already be
/// normalized via [`normalize_query`].
fn matching_entry<'a>(domains: &'a BTreeSet<String>, qname: &str) -> Option<&'a String> {
    let name = normalize_query(qname);
    if name.is_empty() {
        return None;
    }
    domains.iter().find(|entry| entry_matches(entry, &name))
}

/// Whether a single stored `entry` matches the already-normalized `name`,
/// honoring the entry's matching mode:
///
/// - `=host`  — **exact**: matches only `host` itself, never a subdomain.
/// - `*.host` — **subdomains only**: matches `sub.host` (any depth) but not `host`.
/// - `host`   — **host + subdomains**: matches `host` and any `sub.host`.
fn entry_matches(entry: &str, name: &str) -> bool {
    if let Some(exact) = entry.strip_prefix('=') {
        name == exact
    } else if let Some(base) = entry.strip_prefix("*.") {
        name.ends_with(&format!(".{base}"))
    } else {
        name == entry || name.ends_with(&format!(".{entry}"))
    }
}

/// Validate and canonicalize every entry, rejecting the whole batch on the first
/// bad one (so a command is all-or-nothing). Input duplicates collapse via the
/// returned set.
fn normalize_all<F>(raw: &[String], normalize: F) -> Result<BTreeSet<String>, StateError>
where
    F: Fn(&str) -> Result<String, StateError>,
{
    if raw.is_empty() {
        return Err(invalid("empty input list"));
    }
    let mut out = BTreeSet::new();
    for item in raw {
        out.insert(normalize(item)?);
    }
    Ok(out)
}

/// Canonicalize a domain: lowercase, drop a trailing dot, and require a
/// syntactically valid FQDN. An optional matching-mode prefix is preserved:
/// `=` (exact host) or `*.` (subdomains only); bare entries match host +
/// subdomains. See [`entry_matches`].
fn normalize_domain(raw: &str) -> Result<String, StateError> {
    let s = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if s.is_empty() {
        return Err(invalid("empty domain"));
    }
    let (prefix, host) = if let Some(rest) = s.strip_prefix('=') {
        ("=", rest)
    } else if let Some(rest) = s.strip_prefix("*.") {
        ("*.", rest)
    } else {
        ("", s.as_str())
    };
    if host.is_empty() || host.len() > 253 {
        return Err(invalid(format!("invalid domain '{raw}'")));
    }
    if !host.contains('.') {
        return Err(invalid(format!("'{raw}' is not a fully-qualified domain name")));
    }
    for label in host.split('.') {
        let valid = !label.is_empty()
            && label.len() <= 63
            && label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-');
        if !valid {
            return Err(invalid(format!("invalid domain '{raw}'")));
        }
    }
    Ok(format!("{prefix}{host}"))
}

/// Canonicalize a CIDR to its network form (host bits zeroed) so equivalent
/// inputs deduplicate, e.g. `142.250.1.5/16` -> `142.250.0.0/16`.
fn normalize_cidr(raw: &str) -> Result<String, StateError> {
    let net: ipnet::IpNet = raw
        .trim()
        .parse()
        .map_err(|_| invalid(format!("invalid CIDR '{raw}'")))?;
    Ok(net.trunc().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_canonicalization() {
        assert_eq!(normalize_domain("YouTube.com.").unwrap(), "youtube.com");
        assert_eq!(normalize_domain("  *.Reddit.COM ").unwrap(), "*.reddit.com");
        assert_eq!(normalize_domain(" =News.Example.com ").unwrap(), "=news.example.com");
    }

    #[test]
    fn domain_rejections() {
        // Including bare/dangling mode prefixes and single-label wildcards.
        for bad in [
            "", "localhost", "no_dot_here", "a..b.com", "-bad.com", "bad-.com", "*.com", "=com",
            "*.", "=",
        ] {
            assert!(normalize_domain(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn cidr_canonicalization() {
        assert_eq!(normalize_cidr("142.250.1.5/16").unwrap(), "142.250.0.0/16");
        assert_eq!(normalize_cidr("1.1.1.1/32").unwrap(), "1.1.1.1/32");
        assert_eq!(normalize_cidr("2001:db8::1/32").unwrap(), "2001:db8::/32");
    }

    #[test]
    fn cidr_rejections() {
        for bad in ["", "not-an-ip", "1.2.3.4", "1.2.3.4/33", "999.0.0.0/8"] {
            assert!(normalize_cidr(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn empty_batch_rejected() {
        assert!(normalize_all(&[], normalize_domain).is_err());
    }

    /// Convenience: does any entry in `d` cover `name`?
    fn blocks(d: &BTreeSet<String>, name: &str) -> bool {
        matching_entry(d, name).is_some()
    }

    #[test]
    fn name_matching_honors_per_entry_modes() {
        let mut d = BTreeSet::new();
        d.insert("tauri.app".to_string()); // host + subdomains
        d.insert("*.reddit.com".to_string()); // subdomains only
        d.insert("=news.example.com".to_string()); // exact host only

        // Bare entry covers the apex and its subdomains, case/dot-insensitive.
        assert!(blocks(&d, "tauri.app"));
        assert!(blocks(&d, "www.tauri.app"));
        assert!(blocks(&d, "a.b.tauri.app"));
        assert!(blocks(&d, "TAURI.APP."));

        // Wildcard covers subdomains at any depth, but NOT the apex.
        assert!(blocks(&d, "old.reddit.com"));
        assert!(blocks(&d, "i.redd.reddit.com"));
        assert!(!blocks(&d, "reddit.com"));

        // Exact covers only the host itself, never a subdomain.
        assert!(blocks(&d, "news.example.com"));
        assert!(!blocks(&d, "sport.news.example.com"));
        assert!(!blocks(&d, "example.com"));

        // Non-matches: not on a label boundary, different TLD, empty.
        assert!(!blocks(&d, "nottauri.app"));
        assert!(!blocks(&d, "tauri.apps"));
        assert!(!blocks(&d, "other.com"));
        assert!(!blocks(&d, ""));
    }

    #[test]
    fn on_query_records_stats_for_blocked_only() {
        let dir = std::env::temp_dir()
            .join(format!("blockerd-stats-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let mgr = StateManager::new(Store::open(&dir).unwrap()).unwrap();
        mgr.add_domains(&["ads.com".into(), "*.track.net".into()]).unwrap();

        assert_eq!(mgr.on_query("ads.com").as_deref(), Some("ads.com"));
        assert_eq!(mgr.on_query("x.ads.com").as_deref(), Some("ads.com"));
        assert_eq!(mgr.on_query("a.track.net").as_deref(), Some("*.track.net"));
        assert!(mgr.on_query("allowed.com").is_none()); // not blocked → no hit

        let s = mgr.stats_snapshot();
        assert_eq!(s.total_blocked, 3);
        assert_eq!(s.unique_domains, 2);
        // Most-hit entry first.
        assert_eq!(s.top[0].entry, "ads.com");
        assert_eq!(s.top[0].count, 2);
        // Newest recent first.
        assert_eq!(s.recent[0].name, "a.track.net");

        let _ = std::fs::remove_file(&dir);
    }
}
