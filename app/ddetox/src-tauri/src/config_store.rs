//! App-owned persistence for the website library, sessions, and the scheduler's
//! "session-managed" blocklist — plain JSON files under the app data dir.
//!
//! This is configuration the **app** owns (the daemon never sees it). The store
//! is a thin directory wrapper so the read/write logic is testable against a
//! temp dir without a running Tauri app.

use std::path::{Path, PathBuf};

use protocol::config::{Session, WebsiteItem};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

const LIBRARY_FILE: &str = "library.json";
const SESSIONS_FILE: &str = "sessions.json";
const MANAGED_FILE: &str = "managed.json";

/// The domains/CIDRs the scheduler has currently pushed to the daemon on behalf
/// of active sessions — tracked separately so it never disturbs entries the user
/// added manually to the blocklist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedSet {
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
}

/// A configuration directory holding the JSON files above.
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    /// Open (creating if needed) a store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("create config dir: {e}"))?;
        Ok(Self { dir })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Load and parse a JSON file, returning `T::default()` if it does not exist.
    fn load<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T, String> {
        let path = self.path(name);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("parse {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
            Err(e) => Err(format!("read {}: {e}", path.display())),
        }
    }

    /// Write `value` as pretty JSON, atomically (write temp + rename).
    fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        let path = self.path(name);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(value).map_err(|e| format!("encode {name}: {e}"))?;
        std::fs::write(&tmp, &json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("commit {}: {e}", path.display()))?;
        Ok(())
    }

    pub fn load_library(&self) -> Result<Vec<WebsiteItem>, String> {
        self.load(LIBRARY_FILE)
    }
    pub fn save_library(&self, items: &[WebsiteItem]) -> Result<(), String> {
        self.save(LIBRARY_FILE, &items)
    }

    pub fn load_sessions(&self) -> Result<Vec<Session>, String> {
        self.load(SESSIONS_FILE)
    }
    pub fn save_sessions(&self, sessions: &[Session]) -> Result<(), String> {
        self.save(SESSIONS_FILE, &sessions)
    }

    pub fn load_managed(&self) -> Result<ManagedSet, String> {
        self.load(MANAGED_FILE)
    }
    pub fn save_managed(&self, managed: &ManagedSet) -> Result<(), String> {
        self.save(MANAGED_FILE, managed)
    }
}

/// Resolve the store rooted at `dir` (used by commands via the app data dir).
pub fn open(dir: &Path) -> Result<ConfigStore, String> {
    ConfigStore::new(dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::config::{Recurrence, ScheduleRule, Session, WebsiteItem};

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ddetox-store-test-{}", std::process::id()));
        p.push(format!("{:?}", std::time::SystemTime::now()));
        p
    }

    #[test]
    fn library_roundtrip_and_missing_is_empty() {
        let store = ConfigStore::new(temp_dir()).unwrap();
        assert!(store.load_library().unwrap().is_empty());

        let items = vec![WebsiteItem {
            id: "i1".into(),
            label: "Reddit".into(),
            domains: vec!["reddit.com".into(), "*.reddit.com".into()],
            cidrs: vec![],
        }];
        store.save_library(&items).unwrap();
        assert_eq!(store.load_library().unwrap(), items);
    }

    #[test]
    fn sessions_and_managed_roundtrip() {
        let store = ConfigStore::new(temp_dir()).unwrap();
        let sessions = vec![Session {
            id: "s1".into(),
            name: "Focus".into(),
            item_ids: vec!["i1".into()],
            enabled: true,
            rules: vec![ScheduleRule { recurrence: Recurrence::Everyday, windows: vec![] }],
        }];
        store.save_sessions(&sessions).unwrap();
        assert_eq!(store.load_sessions().unwrap(), sessions);

        let managed = ManagedSet { domains: vec!["reddit.com".into()], cidrs: vec![] };
        store.save_managed(&managed).unwrap();
        assert_eq!(store.load_managed().unwrap(), managed);
    }
}
