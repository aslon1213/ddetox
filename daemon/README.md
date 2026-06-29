# blockerd

A privileged macOS root daemon (Rust) that enforces website / IP blocking via a
DNS sinkhole and `pf` packet-filter rules, controlled over a local IPC socket by
a separate unprivileged GUI. This repo is the **daemon only**.

> Status: **Milestone 3 — DNS sinkhole**. Domain blocking is now enforced via a
> local DNS resolver; IP/CIDR blocking via `pf` is still a later milestone.

## What works today (M1–M2)

- A Tokio Unix-domain-socket server speaking **line-delimited JSON**.
- A **versioned** request protocol (`v: 1`) with a tagged `cmd` field.
- An authoritative **in-memory blocklist** (domains + CIDRs) mirrored to
  **SQLite**, so state survives a daemon restart.
- `AddDomains` / `RemoveDomains` / `AddAddrs` / `RemoveAddrs` mutate state with
  validation + canonicalization (lowercased FQDNs incl. `*.` wildcards; CIDRs
  normalized to network form). Batches are atomic — one bad entry rejects the
  whole command. Responses report `{ changed, total }`.
- `GetStatus` reports live blocklist counts.
- `StartSession` / `StopSession` are parsed and validated but return an explicit
  *"not implemented"* error — they land in M5 with the committed-session lock.
- Clean shutdown on SIGINT/SIGTERM, and the socket file is removed on every exit
  path including panic (`SocketGuard`).

The build order for the remaining milestones (`pf` enforcement, sessions +
committed lock, `launchd` packaging) is tracked in the project spec.

## DNS sinkhole (M3)

When run as root, the daemon binds a resolver on `127.0.0.1:53`:

- **Blocked** names (apex + subdomains; a stored `*.d` and `d` both match `d` and
  `sub.d`) get an **NXDOMAIN** reply.
- **Everything else** is forwarded verbatim to the real upstream resolvers
  (captured from `/etc/resolv.conf` before the switch; falls back to `1.1.1.1` /
  `8.8.8.8`), so non-blocked traffic is unaffected.

By default it then repoints every active network service's DNS at `127.0.0.1`
(via `networksetup`) and **restores the originals on every exit** — clean, signal,
or panic — via a `Drop` guard. Only domains are enforced; `cidrs` await the `pf`
milestone.

```sh
# Safe test — run the sinkhole but DON'T touch system DNS:
sudo ./target/debug/blockerd --no-capture
dig tauri.app  @127.0.0.1     # -> status: NXDOMAIN  (if tauri.app is blocked)
dig apple.com  @127.0.0.1     # -> real answer (forwarded)

# Full enforcement — repoints system DNS (restored on Ctrl-C):
sudo ./target/debug/blockerd
```

**Recovery:** if the daemon is ever killed (e.g. `kill -9`) without restoring DNS,
your machine's DNS may be left pointing at `127.0.0.1`. Fix it with:

```sh
sudo networksetup -setdnsservers Wi-Fi Empty   # repeat per active service
sudo dscacheutil -flushcache; sudo killall -HUP mDNSResponder
```

## Protocol

Each request is one JSON line; each response is one JSON line.

```jsonc
// requests
{ "v": 1, "cmd": "GetStatus" }
{ "v": 1, "cmd": "AddDomains", "domains": ["youtube.com", "*.reddit.com"] }

// responses
{ "ok": true,  "data": { /* ... */ } }
{ "ok": false, "error": "..." }
```

`GetStatus` returns daemon/protocol versions, pid, whether the daemon holds
root, and (placeholder) blocklist counts + session, e.g.:

```json
{ "ok": true, "data": {
  "daemon_version": "0.1.0", "protocol_version": 1, "pid": 12345,
  "privileged": false, "blocked_domains": 0, "blocked_cidrs": 0, "session": null
} }
```

## Build & test

```sh
cargo build
cargo test          # unit tests + the privilege-free IPC integration test
```

## Run it

The production socket (`/var/run/...`) and state db (`/var/db/...`) both need
root. For local testing, point them somewhere writable:

```sh
# explicit flags
cargo run -- --socket /tmp/blockerd.sock --state /tmp/blockerd-state.db

# or via env
BLOCKERD_SOCKET=/tmp/blockerd.sock BLOCKERD_STATE=/tmp/blockerd-state.db cargo run
```

Then, from another terminal:

```sh
printf '{"v":1,"cmd":"GetStatus"}\n' | nc -U /tmp/blockerd.sock -w 1
printf '{"v":1,"cmd":"AddDomains","domains":["youtube.com","*.reddit.com"]}\n' | nc -U /tmp/blockerd.sock -w 1
```

Or run the end-to-end manual check (builds, starts on a temp socket, sends a few
requests, tears down):

```sh
./scripts/smoke_test.sh
```

## Configuration

| Setting       | Source                         | Default                                       |
| ------------- | ------------------------------ | --------------------------------------------- |
| Socket path   | `--socket` › `BLOCKERD_SOCKET` | `/var/run/com.aslonkhamidov.blockerd.sock`    |
| State db path | `--state` › `BLOCKERD_STATE`   | `/var/db/com.aslonkhamidov.blockerd/state.db` |
| Log filter    | `BLOCKERD_LOG`                 | `info`                                        |
