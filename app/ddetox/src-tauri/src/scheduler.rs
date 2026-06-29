//! App-orchestrated scheduler: every tick, work out which sessions are active
//! *now*, union their library items into a desired blocklist, and reconcile that
//! against the daemon by pushing only the delta versus what we last pushed.
//!
//! The "session-managed" set is persisted separately from manual blocklist
//! entries, so sessions and hand-added blocks never clobber each other. Daemon
//! add/remove are idempotent, so a partial failure (e.g. daemon mid-restart)
//! self-heals on the next tick — we only advance the managed set on full success.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use protocol::config::{Session, WebsiteItem};
use protocol::schedule::LocalMoment;
use protocol::Request;
use serde::Serialize;

use crate::config_store::{self, ConfigStore, ManagedSet};
use crate::ipc_client;

/// How often the background reconcile loop runs. Schedules are minute-granular,
/// so 30s is plenty responsive.
const TICK: Duration = Duration::from_secs(30);

/// The current local wall-clock as a [`LocalMoment`].
pub fn local_moment_now() -> LocalMoment {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    LocalMoment {
        year: now.year(),
        month: now.month() as u8,
        day: now.day() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
    }
}

/// What blocking *should* be in force right now, derived from active sessions.
pub struct Desired {
    pub domains: BTreeSet<String>,
    pub cidrs: BTreeSet<String>,
    pub active_session_ids: Vec<String>,
}

/// Union the items of every currently-active session.
pub fn compute_desired(
    library: &[WebsiteItem],
    sessions: &[Session],
    now: &LocalMoment,
) -> Desired {
    let by_id: HashMap<&str, &WebsiteItem> =
        library.iter().map(|i| (i.id.as_str(), i)).collect();

    let mut domains = BTreeSet::new();
    let mut cidrs = BTreeSet::new();
    let mut active_session_ids = Vec::new();

    for session in sessions {
        if !session.is_active_at(now) {
            continue;
        }
        active_session_ids.push(session.id.clone());
        for item_id in &session.item_ids {
            if let Some(item) = by_id.get(item_id.as_str()) {
                domains.extend(item.domains.iter().cloned());
                cidrs.extend(item.cidrs.iter().cloned());
            }
        }
    }

    Desired { domains, cidrs, active_session_ids }
}

/// The add/remove deltas to move the daemon from `managed` to `desired`.
pub struct Delta {
    pub domains_add: Vec<String>,
    pub domains_remove: Vec<String>,
    pub cidrs_add: Vec<String>,
    pub cidrs_remove: Vec<String>,
}

impl Delta {
    pub fn is_empty(&self) -> bool {
        self.domains_add.is_empty()
            && self.domains_remove.is_empty()
            && self.cidrs_add.is_empty()
            && self.cidrs_remove.is_empty()
    }
}

pub fn diff(desired: &Desired, managed: &ManagedSet) -> Delta {
    let md: BTreeSet<&String> = managed.domains.iter().collect();
    let mc: BTreeSet<&String> = managed.cidrs.iter().collect();
    Delta {
        domains_add: desired.domains.iter().filter(|d| !md.contains(d)).cloned().collect(),
        domains_remove: managed.domains.iter().filter(|d| !desired.domains.contains(*d)).cloned().collect(),
        cidrs_add: desired.cidrs.iter().filter(|c| !mc.contains(c)).cloned().collect(),
        cidrs_remove: managed.cidrs.iter().filter(|c| !desired.cidrs.contains(*c)).cloned().collect(),
    }
}

/// Reported back to the UI after a reconcile (or a plain read of schedule state).
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleState {
    pub active_session_ids: Vec<String>,
    pub managed_domains: Vec<String>,
    pub managed_cidrs: Vec<String>,
}

/// Reconcile once: compute desired, push the delta to the daemon, persist the
/// new managed set. Idempotent; safe to call on every config change.
pub async fn reconcile(store: &ConfigStore, socket_path: &str) -> Result<ScheduleState, String> {
    let library = store.load_library()?;
    let sessions = store.load_sessions()?;
    let managed = store.load_managed()?;

    let now = local_moment_now();
    let desired = compute_desired(&library, &sessions, &now);
    let delta = diff(&desired, &managed);

    // Removes before adds. Each call propagates a daemon/transport error, which
    // aborts before we persist the managed set so the next tick retries.
    if !delta.domains_remove.is_empty() {
        ipc_client::request_ok(socket_path, Request::RemoveDomains { domains: delta.domains_remove })
            .await
            .map_err(|e| e.to_string())?;
    }
    if !delta.cidrs_remove.is_empty() {
        ipc_client::request_ok(socket_path, Request::RemoveAddrs { cidrs: delta.cidrs_remove })
            .await
            .map_err(|e| e.to_string())?;
    }
    if !delta.domains_add.is_empty() {
        ipc_client::request_ok(socket_path, Request::AddDomains { domains: delta.domains_add })
            .await
            .map_err(|e| e.to_string())?;
    }
    if !delta.cidrs_add.is_empty() {
        ipc_client::request_ok(socket_path, Request::AddAddrs { cidrs: delta.cidrs_add })
            .await
            .map_err(|e| e.to_string())?;
    }

    let new_managed = ManagedSet {
        domains: desired.domains.into_iter().collect(),
        cidrs: desired.cidrs.into_iter().collect(),
    };
    store.save_managed(&new_managed)?;

    Ok(ScheduleState {
        active_session_ids: desired.active_session_ids,
        managed_domains: new_managed.domains,
        managed_cidrs: new_managed.cidrs,
    })
}

/// Resolve the config store rooted at the app data dir.
pub fn store(app: &tauri::AppHandle) -> Result<ConfigStore, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolve app data dir: {e}"))?;
    config_store::open(&dir)
}

/// Spawn the background reconcile loop for the lifetime of the app.
pub fn spawn(app: tauri::AppHandle, socket_path: String) {
    tauri::async_runtime::spawn(async move {
        loop {
            match store(&app) {
                Ok(store) => match reconcile(&store, &socket_path).await {
                    Ok(state) => {
                        tracing::debug!(active = state.active_session_ids.len(), "scheduler reconciled")
                    }
                    Err(e) => tracing::warn!(error = %e, "scheduler reconcile failed"),
                },
                Err(e) => tracing::warn!(error = %e, "scheduler store unavailable"),
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::config::{Recurrence, ScheduleRule};

    fn item(id: &str, domains: &[&str]) -> WebsiteItem {
        WebsiteItem {
            id: id.into(),
            label: id.into(),
            domains: domains.iter().map(|s| s.to_string()).collect(),
            cidrs: vec![],
        }
    }

    fn always_on(id: &str, item_ids: &[&str], enabled: bool) -> Session {
        Session {
            id: id.into(),
            name: id.into(),
            item_ids: item_ids.iter().map(|s| s.to_string()).collect(),
            enabled,
            rules: vec![ScheduleRule { recurrence: Recurrence::Everyday, windows: vec![] }],
        }
    }

    fn now() -> LocalMoment {
        LocalMoment { year: 2024, month: 6, day: 14, hour: 10, minute: 0 }
    }

    #[test]
    fn desired_unions_active_sessions_only() {
        let lib = vec![item("i1", &["a.com"]), item("i2", &["b.com"])];
        let sessions = vec![
            always_on("s1", &["i1"], true),
            always_on("s2", &["i2"], false), // disabled -> excluded
        ];
        let d = compute_desired(&lib, &sessions, &now());
        assert_eq!(d.domains.iter().cloned().collect::<Vec<_>>(), vec!["a.com"]);
        assert_eq!(d.active_session_ids, vec!["s1"]);
    }

    #[test]
    fn diff_computes_add_and_remove_sets() {
        let lib = vec![item("i1", &["a.com"]), item("i2", &["b.com"])];
        let sessions = vec![always_on("s1", &["i2"], true)];
        let desired = compute_desired(&lib, &sessions, &now()); // wants b.com
        let managed = ManagedSet { domains: vec!["a.com".into()], cidrs: vec![] }; // has a.com
        let delta = diff(&desired, &managed);
        assert_eq!(delta.domains_add, vec!["b.com"]);
        assert_eq!(delta.domains_remove, vec!["a.com"]);
    }

    #[test]
    fn diff_empty_when_in_sync() {
        let lib = vec![item("i1", &["a.com"])];
        let sessions = vec![always_on("s1", &["i1"], true)];
        let desired = compute_desired(&lib, &sessions, &now());
        let managed = ManagedSet { domains: vec!["a.com".into()], cidrs: vec![] };
        assert!(diff(&desired, &managed).is_empty());
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    /// A throwaway daemon that records each request line and acks it, so the test
    /// can assert what `reconcile` actually pushed over the real ipc_client path.
    async fn mock_daemon(
        listener: tokio::net::UnixListener,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        loop {
            let Ok((stream, _)) = listener.accept().await else { break };
            let seen = std::sync::Arc::clone(&seen);
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut lines = BufReader::new(read_half).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    seen.lock().unwrap().push(line);
                    let _ = write_half
                        .write_all(b"{\"ok\":true,\"data\":{\"changed\":1,\"total\":1}}\n")
                        .await;
                    let _ = write_half.flush().await;
                }
            });
        }
    }

    #[tokio::test]
    async fn reconcile_pushes_active_session_then_removes_when_inactive() {
        use std::sync::{Arc, Mutex};

        let dir = std::env::temp_dir().join(format!("ddetox-recon-{}", unique_suffix()));
        let store = ConfigStore::new(dir).unwrap();
        store.save_library(&[item("i1", &["a.com"])]).unwrap();
        store.save_sessions(&[always_on("s1", &["i1"], true)]).unwrap();

        let sock = std::env::temp_dir().join(format!("ddetox-recon-{}.sock", unique_suffix()));
        let _ = std::fs::remove_file(&sock);
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        tokio::spawn(mock_daemon(listener, Arc::clone(&seen)));
        let sock_str = sock.to_string_lossy().to_string();

        // Active (always-on) session -> AddDomains pushed, managed reflects it.
        let state = reconcile(&store, &sock_str).await.unwrap();
        assert_eq!(state.managed_domains, vec!["a.com".to_string()]);
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("AddDomains") && l.contains("a.com")));

        // Disable the session -> RemoveDomains pushed, managed emptied.
        store.save_sessions(&[always_on("s1", &["i1"], false)]).unwrap();
        let state = reconcile(&store, &sock_str).await.unwrap();
        assert!(state.managed_domains.is_empty());
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("RemoveDomains") && l.contains("a.com")));

        let _ = std::fs::remove_file(&sock);
    }
}
