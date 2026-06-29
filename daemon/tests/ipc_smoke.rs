//! Milestone 1–2 IPC smoke test.
//!
//! Spawns the real `blockerd` binary on a throwaway socket + state db, exercises
//! the IPC surface and the state store, and confirms state survives a restart
//! and the socket is cleaned up on SIGTERM. Privilege-free (temp paths, no
//! pf/DNS), so it runs anywhere — unlike the milestone 3+ system tests gated
//! behind the `system-tests` feature.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// A unique temp directory, removed on drop.
struct TmpDir(PathBuf);

impl TmpDir {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("blockerd-smoke-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TmpDir(dir)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A spawned daemon process; killed and its socket removed on drop.
struct Daemon {
    child: Child,
    socket: PathBuf,
}

impl Daemon {
    fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Start a daemon on the given paths and wait until it actually answers.
fn spawn(socket: &Path, state: &Path) -> Daemon {
    let child = Command::new(env!("CARGO_BIN_EXE_blockerd"))
        .arg("--socket")
        .arg(socket)
        .arg("--state")
        .arg(state)
        .env("BLOCKERD_LOG", "warn")
        .spawn()
        .expect("spawn blockerd binary");

    let daemon = Daemon { child, socket: socket.to_path_buf() };
    wait_until_ready(socket);
    daemon
}

/// Poll until a GetStatus succeeds — robust against a stale socket file left by
/// a previously killed daemon (the new one removes and rebinds it).
fn wait_until_ready(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(v) = try_request(socket, r#"{"v":1,"cmd":"GetStatus"}"#) {
            if v["ok"] == true {
                return;
            }
        }
        assert!(Instant::now() < deadline, "daemon never became ready");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Send one request line; return the parsed response, or None on any I/O error.
fn try_request(socket: &Path, line: &str) -> Option<serde_json::Value> {
    let stream = UnixStream::connect(socket).ok()?;
    let mut writer = stream.try_clone().ok()?;
    writer.write_all(line.as_bytes()).ok()?;
    writer.write_all(b"\n").ok()?;
    writer.flush().ok()?;
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).ok()?;
    serde_json::from_str(&response).ok()
}

/// Send one request line; panic on any failure.
fn request(socket: &Path, line: &str) -> serde_json::Value {
    try_request(socket, line).unwrap_or_else(|| panic!("request failed: {line}"))
}

#[test]
fn ipc_and_state_round_trip() {
    let tmp = TmpDir::new();
    let socket = tmp.join("d.sock");
    let state = tmp.join("state.db");
    let _daemon = spawn(&socket, &state);

    // Fresh daemon: empty blocklist.
    let resp = request(&socket, r#"{"v":1,"cmd":"GetStatus"}"#);
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["data"]["blocked_domains"], 0);
    assert_eq!(resp["data"]["blocked_cidrs"], 0);
    assert_eq!(resp["data"]["session"], serde_json::Value::Null);

    // AddDomains now actually mutates state. The duplicate collapses -> changed 2.
    let resp = request(
        &socket,
        r#"{"v":1,"cmd":"AddDomains","domains":["YouTube.com","*.reddit.com","youtube.com"]}"#,
    );
    assert_eq!(resp["ok"], true, "{resp}");
    assert_eq!(resp["data"]["changed"], 2);
    assert_eq!(resp["data"]["total"], 2);

    // Re-adding an existing domain is idempotent (changed 0).
    let resp = request(&socket, r#"{"v":1,"cmd":"AddDomains","domains":["youtube.com"]}"#);
    assert_eq!(resp["data"]["changed"], 0, "{resp}");
    assert_eq!(resp["data"]["total"], 2);

    // CIDRs canonicalize: 142.250.1.5/16 -> 142.250.0.0/16, so two equal inputs
    // dedupe to one entry.
    let resp = request(
        &socket,
        r#"{"v":1,"cmd":"AddAddrs","cidrs":["142.250.1.5/16","142.250.0.0/16"]}"#,
    );
    assert_eq!(resp["data"]["changed"], 1, "{resp}");
    assert_eq!(resp["data"]["total"], 1);

    // Invalid input is rejected atomically — nothing is added.
    let resp = request(&socket, r#"{"v":1,"cmd":"AddDomains","domains":["ok.com","not a domain"]}"#);
    assert_eq!(resp["ok"], false, "{resp}");
    let resp = request(&socket, r#"{"v":1,"cmd":"GetStatus"}"#);
    assert_eq!(resp["data"]["blocked_domains"], 2, "rejected batch must not add: {resp}");

    // RemoveDomains mutates state.
    let resp = request(&socket, r#"{"v":1,"cmd":"RemoveDomains","domains":["youtube.com"]}"#);
    assert_eq!(resp["data"]["changed"], 1, "{resp}");
    assert_eq!(resp["data"]["total"], 1);

    // StartSession / StopSession remain unimplemented in M2.
    let resp = request(&socket, r#"{"v":1,"cmd":"StartSession","until_unix":9999999999,"committed":true}"#);
    assert_eq!(resp["ok"], false, "sessions are M5: {resp}");
}

#[test]
fn state_persists_across_restart() {
    let tmp = TmpDir::new();
    let socket = tmp.join("d.sock");
    let state = tmp.join("state.db");

    {
        let _daemon = spawn(&socket, &state);
        let resp = request(
            &socket,
            r#"{"v":1,"cmd":"AddDomains","domains":["persist.com","keep.org"]}"#,
        );
        assert_eq!(resp["data"]["total"], 2, "{resp}");
        request(&socket, r#"{"v":1,"cmd":"AddAddrs","cidrs":["10.0.0.0/8"]}"#);
        // _daemon dropped here -> killed, socket removed.
    }

    // Restart against the same state db: the blocklist must be intact.
    let _daemon = spawn(&socket, &state);
    let resp = request(&socket, r#"{"v":1,"cmd":"GetStatus"}"#);
    assert_eq!(resp["data"]["blocked_domains"], 2, "domains should survive restart: {resp}");
    assert_eq!(resp["data"]["blocked_cidrs"], 1, "cidrs should survive restart: {resp}");
}

#[test]
fn socket_removed_on_sigterm() {
    let tmp = TmpDir::new();
    let socket = tmp.join("d.sock");
    let state = tmp.join("state.db");
    let daemon = spawn(&socket, &state);
    assert!(socket.exists());

    // SIGTERM should trigger graceful shutdown and socket cleanup.
    unsafe {
        libc_kill(daemon.pid() as i32, 15 /* SIGTERM */);
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while socket.exists() {
        assert!(Instant::now() < deadline, "socket was not removed after SIGTERM");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// Minimal kill(2) shim so the test needs no extra dependency.
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}
