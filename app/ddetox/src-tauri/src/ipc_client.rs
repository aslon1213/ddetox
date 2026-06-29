//! Unprivileged IPC client to `blockerd`.
//!
//! One short-lived Unix-socket connection per call: connect (with capped
//! backoff, because a failed `connect` usually means the daemon is mid-restart
//! under its `KeepAlive`), write one request line `{"v":1,"cmd":...}`, read one
//! response line `{"ok":...}`, decode.
//!
//! Errors are typed so the frontend can tell "the daemon is offline" (banner,
//! keep retrying) apart from "the daemon rejected this" (surface verbatim —
//! e.g. `{"ok":false,"error":"... not implemented ..."}`).

use std::time::Duration;

use protocol::{Envelope, Reply, Request, Status};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Failure modes of a single daemon request, surfaced to the UI as strings.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Could not reach the daemon at all — treat as "daemon restarting", not fatal.
    #[error("daemon unreachable — is blockerd running? ({0})")]
    Unreachable(String),
    /// Reached the daemon but the socket conversation failed mid-flight.
    #[error("daemon I/O error: {0}")]
    Io(String),
    /// The daemon replied with something we could not decode.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The daemon understood the request and deliberately rejected it
    /// (`{"ok":false,"error":...}`).
    #[error("{0}")]
    Daemon(String),
}

/// Connection backoff tuning. Total worst-case wait ≈ 150+300+600 = 1.05s.
const CONNECT_ATTEMPTS: u32 = 4;
const INITIAL_BACKOFF: Duration = Duration::from_millis(150);
const MAX_BACKOFF: Duration = Duration::from_secs(2);

async fn connect_with_backoff(path: &str) -> Result<UnixStream, IpcError> {
    let mut delay = INITIAL_BACKOFF;
    let mut last_err = String::new();

    for attempt in 1..=CONNECT_ATTEMPTS {
        match UnixStream::connect(path).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                last_err = e.to_string();
                tracing::debug!(attempt, error = %last_err, "connect failed, backing off");
                if attempt < CONNECT_ATTEMPTS {
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(MAX_BACKOFF);
                }
            }
        }
    }

    Err(IpcError::Unreachable(last_err))
}

/// Decode one response line into the daemon's `data` payload, mapping a
/// `{"ok":false,...}` reply to [`IpcError::Daemon`] so a rejection is never
/// silently treated as success.
fn decode(line: &str) -> Result<serde_json::Value, IpcError> {
    let trimmed = line.trim_end();
    let reply: Reply = serde_json::from_str(trimmed).map_err(|e| {
        IpcError::Protocol(format!("could not decode daemon reply ({e}): {trimmed}"))
    })?;
    reply.into_result().map_err(IpcError::Daemon)
}

/// Send one [`Request`] and return the daemon's `data` payload (or a typed error).
pub async fn request(path: &str, req: Request) -> Result<serde_json::Value, IpcError> {
    let stream = connect_with_backoff(path).await?;
    let (read_half, mut write_half) = stream.into_split();

    let line = Envelope::new(req)
        .to_line()
        .map_err(|e| IpcError::Protocol(e.to_string()))?;
    write_half
        .write_all(line.as_bytes())
        .await
        .map_err(|e| IpcError::Io(e.to_string()))?;
    write_half
        .flush()
        .await
        .map_err(|e| IpcError::Io(e.to_string()))?;

    let mut reader = BufReader::new(read_half);
    let mut resp_line = String::new();
    let n = reader
        .read_line(&mut resp_line)
        .await
        .map_err(|e| IpcError::Io(e.to_string()))?;
    if n == 0 {
        return Err(IpcError::Unreachable(
            "daemon closed the connection without responding".into(),
        ));
    }

    decode(&resp_line)
}

/// Send `GetStatus` (or any request whose `data` is a [`Status`]) and decode it.
pub async fn request_status(path: &str, req: Request) -> Result<Status, IpcError> {
    let data = request(path, req).await?;
    serde_json::from_value(data)
        .map_err(|e| IpcError::Protocol(format!("status payload did not decode: {e}")))
}

/// Send a mutating request; success is enough, the payload is discarded. A
/// daemon rejection propagates as [`IpcError::Daemon`] so the UI reflects it.
pub async fn request_ok(path: &str, req: Request) -> Result<(), IpcError> {
    request(path, req).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_status_data() {
        let line = r#"{"ok":true,"data":{"daemon_version":"0.1.0","protocol_version":1,"pid":1,"privileged":true,"blocked_domains":5,"blocked_cidrs":0,"session":null}}"#;
        let status: Status = serde_json::from_value(decode(line).unwrap()).unwrap();
        assert_eq!(status.blocked_domains, 5);
    }

    #[test]
    fn maps_ok_false_to_daemon_error() {
        let line = r#"{"ok":false,"error":"command 'StartSession' is not implemented in this build"}"#;
        match decode(line) {
            Err(IpcError::Daemon(msg)) => assert!(msg.contains("not implemented")),
            other => panic!("expected Daemon error, got {other:?}"),
        }
    }

    #[test]
    fn maps_garbage_to_protocol_error_with_raw_line() {
        match decode(r#"{"totally":"unexpected"}"#) {
            Err(IpcError::Protocol(msg)) => assert!(msg.contains("totally")),
            other => panic!("expected Protocol error, got {other:?}"),
        }
    }
}
