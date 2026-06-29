//! Shared wire protocol between the blocker GUI (unprivileged) and `blockerd`.
//!
//! This mirrors the **real daemon** (`dp_course`): line-delimited JSON over a
//! Unix-domain socket, where every *request* carries a protocol version `v` and
//! a `cmd` tag, and every *response* is either
//! `{ "ok": true, "data": ... }` or `{ "ok": false, "error": "..." }`.
//!
//! ```text
//! GUI -> daemon:  {"v":1,"cmd":"AddDomains","domains":["x.com"]}\n
//! daemon -> GUI:  {"ok":true,"data":{"changed":1,"total":5}}\n
//! daemon -> GUI:  {"ok":false,"error":"... not implemented ..."}\n
//! ```
//!
//! The **daemon is the source of truth**. `GetStatus` returns *counts* and daemon
//! metadata — not the blocklist contents — so the GUI shows totals, never an
//! authoritative local list.

use serde::{Deserialize, Serialize};

pub mod config;
pub mod schedule;

/// IPC protocol version. The daemon rejects requests with any other `v`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Default daemon socket path; override via [`SOCKET_ENV`] for dev.
pub const DEFAULT_SOCKET_PATH: &str = "/var/run/com.aslonkhamidov.blockerd.sock";

/// Environment variable that overrides [`DEFAULT_SOCKET_PATH`] on both sides.
pub const SOCKET_ENV: &str = "BLOCKERD_SOCKET";

/// Resolve the socket path: `$BLOCKERD_SOCKET` if set, else [`DEFAULT_SOCKET_PATH`].
pub fn socket_path() -> String {
    std::env::var(SOCKET_ENV).unwrap_or_else(|_| DEFAULT_SOCKET_PATH.to_string())
}

/// A request from the GUI to the daemon, internally tagged by `cmd`.
///
/// Field names and shapes match the daemon's `Command` enum exactly. Sent inside
/// an [`Envelope`] so the wire form is `{"v":1,"cmd":...,...}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum Request {
    GetStatus,
    /// Blocking statistics (see [`Stats`]).
    GetStats,
    AddDomains { domains: Vec<String> },
    RemoveDomains { domains: Vec<String> },
    AddAddrs { cidrs: Vec<String> },
    RemoveAddrs { cidrs: Vec<String> },
    /// `until_unix` is signed to match the daemon. Not implemented daemon-side yet.
    StartSession { until_unix: i64, committed: bool },
    StopSession,
}

/// Versioned envelope used for **requests** (`{"v":1, ...}`). Responses are not
/// enveloped — see [`Reply`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u32,
    #[serde(flatten)]
    pub body: T,
}

impl<T> Envelope<T> {
    pub fn new(body: T) -> Self {
        Envelope { v: PROTOCOL_VERSION, body }
    }
}

impl<T: Serialize> Envelope<T> {
    /// Serialize to one newline-terminated JSON line.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(self)?;
        s.push('\n');
        Ok(s)
    }
}

impl<T: for<'de> Deserialize<'de>> Envelope<T> {
    /// Parse one request line, rejecting an unsupported protocol version.
    pub fn from_line(line: &str) -> Result<Self, ProtocolError> {
        let env: Envelope<T> = serde_json::from_str(line.trim_end())?;
        if env.v != PROTOCOL_VERSION {
            return Err(ProtocolError::VersionMismatch {
                expected: PROTOCOL_VERSION,
                got: env.v,
            });
        }
        Ok(env)
    }
}

/// A daemon response: `{ "ok": true, "data": ... }` or `{ "ok": false, "error": ... }`.
/// `data` is intentionally untyped here — it is a [`Status`] for `GetStatus`, a
/// [`MutationOutcome`] for add/remove, etc. — and decoded by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Reply {
    /// Successful reply carrying a JSON payload.
    pub fn ok(data: serde_json::Value) -> Self {
        Reply { ok: true, data: Some(data), error: None }
    }

    /// Failed reply carrying a human-readable message.
    pub fn error(msg: impl Into<String>) -> Self {
        Reply { ok: false, data: None, error: Some(msg.into()) }
    }

    /// Serialize to one newline-terminated JSON line.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        let mut s = serde_json::to_string(self)?;
        s.push('\n');
        Ok(s)
    }

    /// Collapse to `Ok(data)` on success, `Err(message)` on failure.
    pub fn into_result(self) -> Result<serde_json::Value, String> {
        if self.ok {
            Ok(self.data.unwrap_or(serde_json::Value::Null))
        } else {
            Err(self
                .error
                .unwrap_or_else(|| "daemon reported failure without a message".to_string()))
        }
    }
}

/// `GetStatus` payload — daemon metadata plus blocklist **counts**. The daemon
/// does not expose the actual entries, so there are no `domains`/`cidrs` lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub daemon_version: String,
    pub protocol_version: u32,
    pub pid: u32,
    pub privileged: bool,
    pub blocked_domains: u64,
    pub blocked_cidrs: u64,
    /// Whether blocked names resolve to the loopback block page (vs. NXDOMAIN).
    /// Defaulted for compatibility with daemons that predate this field.
    #[serde(default)]
    pub block_page: bool,
    /// Active session details, or `null`. Shape is daemon-defined; opaque until
    /// the daemon implements sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<serde_json::Value>,
}

/// `GetStats` payload — a snapshot of blocking activity since the daemon started.
/// Counts reset on daemon restart (enforcement only runs while it is up).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// When collection started (daemon start), seconds since the epoch.
    pub since_unix: i64,
    /// Total blocked DNS queries since `since_unix`.
    pub total_blocked: u64,
    /// Distinct blocklist entries hit at least once.
    pub unique_domains: u64,
    /// Most-hit entries, descending.
    #[serde(default)]
    pub top: Vec<DomainStat>,
    /// Most recent blocked queries, newest first.
    #[serde(default)]
    pub recent: Vec<RecentBlock>,
}

/// One row of the "top blocked" table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainStat {
    /// The blocklist entry that matched, in stored form (e.g. `*.ads.com`).
    pub entry: String,
    pub count: u64,
}

/// One entry of the "recent activity" feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentBlock {
    /// The queried name that was blocked.
    pub name: String,
    /// When it was blocked, seconds since the epoch.
    pub unix: i64,
}

/// Payload of a successful add/remove mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutationOutcome {
    /// How many entries actually changed (added/removed).
    pub changed: u64,
    /// Total entries of that kind after the mutation.
    pub total: u64,
}

/// Errors decoding a request frame.
#[derive(Debug)]
pub enum ProtocolError {
    Json(serde_json::Error),
    VersionMismatch { expected: u32, got: u32 },
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::Json(e) => write!(f, "malformed request frame: {e}"),
            ProtocolError::VersionMismatch { expected, got } => {
                write!(f, "protocol version mismatch: expected v{expected}, got v{got}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<serde_json::Error> for ProtocolError {
    fn from(e: serde_json::Error) -> Self {
        ProtocolError::Json(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_with_version_and_cmd() {
        let line = Envelope::new(Request::AddDomains { domains: vec!["*.b.com".into()] })
            .to_line()
            .unwrap();
        assert!(line.contains("\"v\":1"));
        assert!(line.contains("\"cmd\":\"AddDomains\""));
    }

    #[test]
    fn request_roundtrips() {
        let req = Request::StartSession { until_unix: 1_700_000_000, committed: true };
        let line = Envelope::new(req.clone()).to_line().unwrap();
        assert_eq!(Envelope::<Request>::from_line(&line).unwrap().body, req);
    }

    #[test]
    fn ok_reply_yields_data() {
        let line = r#"{"ok":true,"data":{"changed":1,"total":5}}"#;
        let reply: Reply = serde_json::from_str(line).unwrap();
        let data = reply.into_result().unwrap();
        let outcome: MutationOutcome = serde_json::from_value(data).unwrap();
        assert_eq!(outcome, MutationOutcome { changed: 1, total: 5 });
    }

    #[test]
    fn error_reply_yields_message() {
        let line = r#"{"ok":false,"error":"command 'StartSession' is not implemented in this build"}"#;
        let reply: Reply = serde_json::from_str(line).unwrap();
        let err = reply.into_result().unwrap_err();
        assert!(err.contains("not implemented"));
    }

    #[test]
    fn status_decodes_counts_and_null_session() {
        let line = r#"{"ok":true,"data":{"daemon_version":"0.1.0","protocol_version":1,"pid":90830,"privileged":true,"blocked_domains":5,"blocked_cidrs":0,"session":null}}"#;
        let reply: Reply = serde_json::from_str(line).unwrap();
        let status: Status = serde_json::from_value(reply.into_result().unwrap()).unwrap();
        assert_eq!(status.blocked_domains, 5);
        assert_eq!(status.blocked_cidrs, 0);
        assert!(status.session.is_none());
        assert!(status.privileged);
    }

    #[test]
    fn rejects_wrong_request_version() {
        let err = Envelope::<Request>::from_line(r#"{"v":2,"cmd":"GetStatus"}"#).unwrap_err();
        assert!(matches!(err, ProtocolError::VersionMismatch { got: 2, .. }));
    }
}
