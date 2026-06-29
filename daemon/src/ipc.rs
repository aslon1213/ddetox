//! Unix-domain-socket IPC server: bind, accept, and dispatch line-delimited
//! JSON requests to responses.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, warn};

use crate::protocol::{self, Command, Response, Status, PROTOCOL_VERSION};
use crate::state::{MutationResult, StateError, StateManager};

/// Bind the IPC listener at `path`, clearing a stale socket from a previous run
/// and tightening the socket to owner-only permissions.
///
/// A stale socket file makes `bind(2)` fail with `EADDRINUSE`, so we remove it
/// first — but only if it really is a socket, so we never clobber a regular
/// file a caller pointed us at by mistake.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::FileTypeExt;

    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_socket() {
            std::fs::remove_file(path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} exists and is not a socket; refusing to remove it",
                    path.display()
                ),
            ));
        }
    }

    let listener = UnixListener::bind(path)?;

    // Owner-only. The daemon is the sole writer; granting the unprivileged GUI
    // access (which runs as a different user than root) is an IPC-authentication
    // concern for a later milestone, not a reason to widen the socket now.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;

    Ok(listener)
}

/// Accept connections until the listener is dropped, handling each on its own
/// task.
pub async fn serve(listener: UnixListener, state: Arc<StateManager>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        debug!(error = %e, "connection closed with error");
                    }
                });
            }
            Err(e) => {
                // accept(2) errors are typically transient (e.g. EMFILE). Back
                // off briefly so a persistent error can't spin the CPU.
                error!(error = %e, "accept failed; backing off");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Read newline-delimited requests until EOF, replying with one JSON line each.
async fn handle_connection(stream: UnixStream, state: Arc<StateManager>) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = dispatch(&line, &state);
        let mut bytes = serde_json::to_vec(&response).unwrap_or_else(|_| {
            br#"{"ok":false,"error":"failed to serialize response"}"#.to_vec()
        });
        bytes.push(b'\n');
        write_half.write_all(&bytes).await?;
        write_half.flush().await?;
    }
    Ok(())
}

/// Map one raw request line to a response.
fn dispatch(line: &str, state: &StateManager) -> Response {
    let command = match protocol::parse_request(line) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "rejected malformed request");
            return Response::error(e);
        }
    };

    match command {
        Command::GetStatus => status_response(state),
        Command::GetStats => stats_response(state),
        Command::AddDomains { domains } => mutation_response(state.add_domains(&domains)),
        Command::RemoveDomains { domains } => mutation_response(state.remove_domains(&domains)),
        Command::AddAddrs { cidrs } => mutation_response(state.add_cidrs(&cidrs)),
        Command::RemoveAddrs { cidrs } => mutation_response(state.remove_cidrs(&cidrs)),
        // Sessions (scheduling + committed lock) land in M5.
        cmd @ (Command::StartSession { .. } | Command::StopSession) => {
            warn!(command = cmd.name(), "command not implemented in this build");
            Response::error(format!(
                "command '{}' is not implemented in this build",
                cmd.name()
            ))
        }
    }
}

/// Turn a state mutation result into a response, keeping storage-error details
/// out of the client-facing message (they are logged instead).
fn mutation_response(result: Result<MutationResult, StateError>) -> Response {
    match result {
        Ok(m) => Response::ok(json!({ "changed": m.changed, "total": m.total })),
        Err(StateError::Invalid(msg)) => {
            warn!(error = %msg, "rejected invalid request");
            Response::error(msg)
        }
        Err(e @ StateError::Storage(_)) => {
            error!(error = %e, "storage error while handling request");
            Response::error("internal storage error")
        }
    }
}

/// Build the current status snapshot from live state.
fn status_response(state: &StateManager) -> Response {
    let snapshot = state.summary();
    let status = Status {
        daemon_version: env!("CARGO_PKG_VERSION"),
        protocol_version: PROTOCOL_VERSION,
        pid: std::process::id(),
        privileged: nix::unistd::Uid::effective().is_root(),
        blocked_domains: snapshot.domains.len(),
        blocked_cidrs: snapshot.cidrs.len(),
        block_page: state.block_page_active(),
        session: snapshot
            .session
            .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null)),
    };
    match serde_json::to_value(status) {
        Ok(v) => Response::ok(v),
        Err(e) => Response::error(format!("failed to encode status: {e}")),
    }
}

/// Build a blocking-statistics snapshot from live state.
fn stats_response(state: &StateManager) -> Response {
    match serde_json::to_value(state.stats_snapshot()) {
        Ok(v) => Response::ok(v),
        Err(e) => Response::error(format!("failed to encode stats: {e}")),
    }
}
