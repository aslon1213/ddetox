//! Wire protocol for the blockerd IPC socket.
//!
//! The transport is line-delimited JSON over a Unix domain socket. Every
//! request carries a protocol version `v` and a `cmd` tag. Responses are either
//! `{ "ok": true, "data": ... }` or `{ "ok": false, "error": "..." }`.
//!
//! The full command surface is defined here from milestone 1 so that field
//! validation is complete and stable, even though only [`Command::GetStatus`]
//! is functional in this build.

use serde::{Deserialize, Serialize};

/// Current IPC protocol version. Requests with any other `v` are rejected so
/// that a newer GUI talking to an older daemon (or vice-versa) fails loudly
/// instead of silently misbehaving.
pub const PROTOCOL_VERSION: u32 = 1;

/// A command parsed from a client request line, tagged by the `cmd` field.
///
/// Variants that weaken an active committed session (`StopSession`,
/// `RemoveDomains`, `RemoveAddrs`) exist here but are not yet enforced — the
/// committed-session authorization rule lands with the session milestone.
///
/// The payload fields below are deserialized and validated now but only read
/// once their commands are implemented (M2+), so we silence the interim
/// "field never read" lints rather than carry partial, churning definitions.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd")]
pub enum Command {
    AddDomains { domains: Vec<String> },
    RemoveDomains { domains: Vec<String> },
    AddAddrs { cidrs: Vec<String> },
    RemoveAddrs { cidrs: Vec<String> },
    StartSession { until_unix: i64, committed: bool },
    StopSession,
    GetStatus,
    GetStats,
}

impl Command {
    /// Stable, human-readable name for logs and error messages.
    pub fn name(&self) -> &'static str {
        match self {
            Command::AddDomains { .. } => "AddDomains",
            Command::RemoveDomains { .. } => "RemoveDomains",
            Command::AddAddrs { .. } => "AddAddrs",
            Command::RemoveAddrs { .. } => "RemoveAddrs",
            Command::StartSession { .. } => "StartSession",
            Command::StopSession => "StopSession",
            Command::GetStatus => "GetStatus",
            Command::GetStats => "GetStats",
        }
    }
}

/// Parse and validate a single request line.
///
/// Returns a human-readable error string (suitable for the `error` field of a
/// response) when the line is not a JSON object, is missing or carries an
/// unsupported protocol version, or names an unknown command / malformed
/// fields. Validating the version *before* decoding the command gives a precise
/// error instead of a confusing serde failure.
pub fn parse_request(line: &str) -> Result<Command, String> {
    let value: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("invalid JSON: {e}"))?;

    if !value.is_object() {
        return Err("request must be a JSON object".to_string());
    }

    match value.get("v").and_then(serde_json::Value::as_u64) {
        Some(v) if v == u64::from(PROTOCOL_VERSION) => {}
        Some(v) => {
            return Err(format!(
                "unsupported protocol version {v}; this daemon speaks v{PROTOCOL_VERSION}"
            ))
        }
        None => return Err("missing or non-integer protocol version field 'v'".to_string()),
    }

    // The `v` field is ignored by the internally-tagged enum decode below.
    serde_json::from_value(value).map_err(|e| format!("invalid '{}' command: {e}", "cmd"))
}

/// A response written back to the client, one JSON line per request.
#[derive(Debug, Serialize)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    /// Successful response carrying a JSON payload.
    pub fn ok(data: serde_json::Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    /// Failed response carrying a human-readable error message.
    pub fn error(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

/// Snapshot of daemon state returned by `GetStatus`.
///
/// The blocklist counts and `session` are placeholders in this build; they are
/// wired to the real state store and session manager in later milestones.
#[derive(Debug, Serialize)]
pub struct Status {
    /// Daemon binary version (from Cargo).
    pub daemon_version: &'static str,
    /// IPC protocol version this daemon speaks.
    pub protocol_version: u32,
    /// Daemon process id.
    pub pid: u32,
    /// Whether the daemon holds root (required for pf, DNS, and binding :53 in
    /// later milestones).
    pub privileged: bool,
    /// Number of blocked domains. Always 0 until the state store lands (M2).
    pub blocked_domains: usize,
    /// Number of blocked CIDRs. Always 0 until the state store lands (M2).
    pub blocked_cidrs: usize,
    /// Whether blocked names resolve to the loopback block page (vs. NXDOMAIN).
    pub block_page: bool,
    /// Active session details, if any. Always `null` until sessions land (M5).
    pub session: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_status() {
        let cmd = parse_request(r#"{"v":1,"cmd":"GetStatus"}"#).unwrap();
        assert!(matches!(cmd, Command::GetStatus));
    }

    #[test]
    fn parses_struct_variant() {
        let cmd = parse_request(r#"{"v":1,"cmd":"AddDomains","domains":["a.com","*.b.com"]}"#)
            .unwrap();
        match cmd {
            Command::AddDomains { domains } => assert_eq!(domains, ["a.com", "*.b.com"]),
            other => panic!("wrong variant: {}", other.name()),
        }
    }

    #[test]
    fn rejects_wrong_version() {
        let err = parse_request(r#"{"v":2,"cmd":"GetStatus"}"#).unwrap_err();
        assert!(err.contains("unsupported protocol version"), "{err}");
    }

    #[test]
    fn rejects_missing_version() {
        let err = parse_request(r#"{"cmd":"GetStatus"}"#).unwrap_err();
        assert!(err.contains("'v'"), "{err}");
    }

    #[test]
    fn rejects_unknown_command() {
        let err = parse_request(r#"{"v":1,"cmd":"Nope"}"#).unwrap_err();
        assert!(err.contains("command"), "{err}");
    }

    #[test]
    fn rejects_non_object() {
        assert!(parse_request("42").is_err());
        assert!(parse_request("not json").is_err());
    }
}
