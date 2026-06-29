//! blockerd — privileged macOS daemon that enforces website/IP blocking.
//!
//! Milestone 1 scope: the daemon skeleton and IPC surface only. It binds a Unix
//! domain socket, speaks versioned line-delimited JSON, and answers
//! `GetStatus`. There is no enforcement (pf / DNS) yet, so there is nothing
//! system-wide to restore — the only resource owned here is the socket file,
//! which is removed on every exit path including panic.

mod block_page;
mod dns;
mod ipc;
mod netconfig;
mod persist;
mod protocol;
mod state;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

/// Default production socket path. Lives under /var/run, so creating it requires
/// root. Override with `--socket` or `BLOCKERD_SOCKET` for non-root smoke tests.
const DEFAULT_SOCKET: &str = "/var/run/com.aslonkhamidov.blockerd.sock";

/// Default production state database. Under /var/db, so it requires root.
/// Override with `--state` or `BLOCKERD_STATE`.
const DEFAULT_STATE: &str = "/var/db/com.aslonkhamidov.blockerd/state.db";

fn main() {
    init_tracing();

    let (socket_path, state_path, capture_dns) = match parse_args() {
        Args::Run { socket, state, capture_dns } => (socket, state, capture_dns),
        Args::Help => {
            print_help();
            return;
        }
        Args::BadUsage(msg) => {
            eprintln!("blockerd: {msg}");
            eprintln!("try 'blockerd --help'");
            std::process::exit(2);
        }
    };


    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("blockerd: failed to start tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = runtime.block_on(run(socket_path, state_path, capture_dns)) {
        eprintln!("blockerd: exited with error: {e}");
        std::process::exit(1);
    }
}

/// Open the state store, bind the socket, start DNS enforcement, and serve until
/// a shutdown signal.
async fn run(
    socket_path: PathBuf,
    state_path: PathBuf,
    capture_dns: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let is_root = nix::unistd::Uid::effective().is_root();
    if !is_root {
        warn!(
            "running without root; DNS sinkhole (binding :53 + system DNS) is \
             unavailable — IPC-only mode (fine for smoke tests)"
        );
    }

    ensure_state_dir(&state_path)?;
    let store: persist::Store = persist::Store::open(&state_path)?;
    let state: Arc<state::StateManager> = Arc::new(state::StateManager::new(store)?);
    info!(state = %state_path.display(), "opened state store");

    let listener = ipc::bind(&socket_path)?;
    // Removes the socket file on every exit path (signal *or* panic), so a stale
    // socket never blocks the next start.
    let _socket_guard = SocketGuard::new(socket_path.clone());

    // DNS sinkhole enforcement (root only). Order matters for safety: bind :53
    // first, and only *then* repoint system DNS at it. `_dns_guard` restores the
    // original DNS on every exit path (it drops when this function returns).
    let mut _dns_guard: Option<netconfig::DnsGuard> = None;
    if is_root {
        // Block-page server on loopback :80. If it binds, blocked names resolve
        // to loopback so the browser shows the page; otherwise we fall back to
        // NXDOMAIN for blocked names.
        let block_page = match block_page::bind(SocketAddr::from(([127, 0, 0, 1], 80))).await {
            Ok(http) => {
                tokio::spawn(block_page::serve(http));
                info!("block page served on 127.0.0.1:80");
                true
            }
            Err(e) => {
                warn!(error = %e, "could not bind 127.0.0.1:80; block page disabled (NXDOMAIN fallback)");
                false
            }
        };
        state.set_block_page_active(block_page);

        let dns_addr = SocketAddr::from(([127, 0, 0, 1], 53));
        match dns::bind(dns_addr).await {
            Ok(listeners) => {
                let upstream = netconfig::capture_upstream();
                let dns_state = Arc::clone(&state);
                tokio::spawn(async move { listeners.serve(dns_state, upstream, block_page).await });
                if capture_dns {
                    _dns_guard = Some(netconfig::capture_and_redirect());
                    info!("system DNS redirected to the sinkhole (restored on exit)");
                } else {
                    info!(
                        "DNS sinkhole on 127.0.0.1:53; system DNS NOT captured \
                         (--no-capture). Test with: dig <name> @127.0.0.1"
                    );
                }
            }
            Err(e) => warn!(error = %e, "could not bind 127.0.0.1:53; DNS enforcement disabled"),
        }
    }

    info!(
        socket = %socket_path.display(),
        pid = std::process::id(),
        version = env!("CARGO_PKG_VERSION"),
        "blockerd listening"
    );

    tokio::select! {
        _ = ipc::serve(listener, state) => warn!("accept loop ended unexpectedly"),
        signal = wait_for_shutdown() => info!(signal, "shutdown signal received"),
    }

    info!("blockerd stopped");
    Ok(())
}

/// Create the state database's parent directory (owner-only) if missing.
fn ensure_state_dir(state_path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if let Some(parent) = state_path.parent() {
        if !parent.as_os_str().is_empty() {
            // Apply 0700 only to directories we create. `recursive(true)` is a
            // no-op that leaves permissions untouched when the directory already
            // exists, so we never chmod a pre-existing system dir like /tmp —
            // which would fail with EPERM for a non-root user.
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
    }
    Ok(())
}

/// Wait for SIGINT or SIGTERM, returning which one fired.
async fn wait_for_shutdown() -> &'static str {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => "SIGTERM",
        _ = sigint.recv() => "SIGINT",
    }
}

/// Removes the socket file when dropped, leaving the filesystem clean on
/// shutdown or panic.
struct SocketGuard {
    path: PathBuf,
}

impl SocketGuard {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => info!(socket = %self.path.display(), "removed socket file"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                warn!(socket = %self.path.display(), error = %e, "failed to remove socket file")
            }
        }
    }
}

/// Parsed command line.
enum Args {
    Run { socket: PathBuf, state: PathBuf, capture_dns: bool },
    Help,
    BadUsage(String),
}

/// Resolve paths: `--socket`/`--state` win, then `BLOCKERD_SOCKET`/
/// `BLOCKERD_STATE`, then the defaults under /var. Overridable paths are what
/// make non-root smoke testing possible without touching /var.
fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut socket: Option<PathBuf> = None;
    let mut state: Option<PathBuf> = None;
    let mut no_capture = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => match args.next() {
                Some(p) => socket = Some(PathBuf::from(p)),
                None => return Args::BadUsage("--socket requires a path argument".into()),
            },
            "--state" => match args.next() {
                Some(p) => state = Some(PathBuf::from(p)),
                None => return Args::BadUsage("--state requires a path argument".into()),
            },
            // Run the :53 sinkhole but leave system DNS alone (safe for testing
            // via `dig @127.0.0.1`). Also settable with BLOCKERD_DNS_CAPTURE=off.
            "--no-capture" => no_capture = true,
            "-h" | "--help" => return Args::Help,
            other => return Args::BadUsage(format!("unknown argument: {other}")),
        }
    }

    let socket = socket
        .or_else(|| std::env::var_os("BLOCKERD_SOCKET").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET));
    let state = state
        .or_else(|| std::env::var_os("BLOCKERD_STATE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE));

    let env_off = std::env::var("BLOCKERD_DNS_CAPTURE").map(|v| v == "off").unwrap_or(false);
    let capture_dns = !no_capture && !env_off;

    Args::Run { socket, state, capture_dns }
}

fn print_help() {
    println!(
        "blockerd — website/IP blocking daemon (milestone 3: DNS sinkhole)\n\n\
         USAGE:\n    \
             blockerd [--socket <path>] [--state <path>] [--no-capture]\n\n\
         OPTIONS:\n    \
             --socket <path>   Unix socket path to bind\n    \
             --state <path>    SQLite state database path\n    \
             --no-capture      Run the :53 sinkhole but do NOT repoint system DNS\n    \
             \x20                 (test with: dig <name> @127.0.0.1)\n    \
             -h, --help        Print this help\n\n\
         ENVIRONMENT:\n    \
             BLOCKERD_SOCKET       socket path (overridden by --socket)\n    \
             BLOCKERD_STATE        state db path (overridden by --state)\n    \
             BLOCKERD_DNS_CAPTURE  set to 'off' for --no-capture behavior\n    \
             BLOCKERD_LOG          tracing filter, e.g. debug (default: info)\n\n\
         NOTE: the DNS sinkhole binds :53 and (by default) repoints system DNS at\n    \
             127.0.0.1, restoring it on exit. Requires root; skipped if non-root.\n\n\
         DEFAULTS:\n    \
             socket  {DEFAULT_SOCKET}\n    \
             state   {DEFAULT_STATE}"
    );
}

/// Initialize structured logging. Honors `BLOCKERD_LOG` (a tracing-subscriber
/// env filter), defaulting to `info`.
fn init_tracing() {
    let filter = EnvFilter::try_from_env("BLOCKERD_LOG")
        .or_else(|_| EnvFilter::try_new("info"))
        .expect("static default filter is valid");

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
