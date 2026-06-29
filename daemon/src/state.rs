//! Authoritative in-memory blocklist + session state, mirrored to SQLite.
//!
//! [`StateManager`] owns the canonical state and serializes all mutations
//! behind a mutex. Every mutation is validated, applied to the in-memory sets,
//! and persisted via [`crate::persist::Store`] before being acknowledged, so a
//! restart reloads exactly what callers were told succeeded.
//!
//! Enforcement (pf / DNS) is **not** wired here yet — M2 only owns the truth.

use std::collections::BTreeSet;
use std::sync::Mutex;

use serde::Serialize;
use tracing::info;

use crate::persist::Store;

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
}

struct Inner {
    snapshot: Snapshot,
    store: Store,
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
        Ok(Self { inner: Mutex::new(Inner { snapshot, store }) })
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

    /// Whether a DNS query name should be sinkholed, per the current blocklist.
    /// A stored entry blocks its apex and any subdomain; a `*.` prefix is
    /// matched the same way (so `*.d` and `d` both cover `d` and `sub.d`).
    pub fn is_blocked(&self, qname: &str) -> bool {
        name_matches(&self.lock().snapshot.domains, qname)
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

/// Whether `qname` is covered by any entry in `domains`. An entry `d` (or `*.d`)
/// matches the apex `d` and any subdomain `sub.d`. Case- and trailing-dot-insensitive.
fn name_matches(domains: &BTreeSet<String>, qname: &str) -> bool {
    let name = qname.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() {
        return false;
    }
    domains.iter().any(|entry| {
        let base = entry.strip_prefix("*.").unwrap_or(entry);
        name == base || name.ends_with(&format!(".{base}"))
    })
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

/// Canonicalize a domain: lowercase, drop a trailing dot, allow an optional
/// `*.` wildcard prefix, and require a syntactically valid FQDN.
fn normalize_domain(raw: &str) -> Result<String, StateError> {
    let s = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if s.is_empty() {
        return Err(invalid("empty domain"));
    }
    let host = s.strip_prefix("*.").unwrap_or(s.as_str());
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
    Ok(s)
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
    }

    #[test]
    fn domain_rejections() {
        for bad in ["", "localhost", "no_dot_here", "a..b.com", "-bad.com", "bad-.com"] {
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

    #[test]
    fn name_matching_covers_apex_subdomains_and_wildcards() {
        let mut d = BTreeSet::new();
        d.insert("tauri.app".to_string());
        d.insert("*.reddit.com".to_string());

        // Apex entry covers the apex and its subdomains, case/dot-insensitive.
        assert!(name_matches(&d, "tauri.app"));
        assert!(name_matches(&d, "www.tauri.app"));
        assert!(name_matches(&d, "TAURI.APP."));
        // Wildcard entry covers the base and its subdomains.
        assert!(name_matches(&d, "reddit.com"));
        assert!(name_matches(&d, "old.reddit.com"));
        // Non-matches: not on a label boundary, different TLD, empty.
        assert!(!name_matches(&d, "nottauri.app"));
        assert!(!name_matches(&d, "tauri.apps"));
        assert!(!name_matches(&d, "example.com"));
        assert!(!name_matches(&d, ""));
    }
}
