# blockerd

A privileged macOS root daemon (Rust) that enforces website / IP blocking via a
DNS sinkhole and `pf` packet-filter rules, controlled over a local IPC socket by
a separate unprivileged GUI. This repo is the **daemon only**.

> Status: **Milestone 3 — DNS sinkhole**, plus a loopback **block page** and
> in-memory **statistics**. Domain blocking is enforced via a local DNS resolver;
> IP/CIDR blocking via `pf` and sessions are still later milestones.

## What works today

- A Tokio Unix-domain-socket server speaking **line-delimited JSON**.
- A **versioned** request protocol (`v: 1`) with a tagged `cmd` field.
- An authoritative **in-memory blocklist** (domains + CIDRs) mirrored to
  **SQLite**, so state survives a daemon restart.
- `AddDomains` / `RemoveDomains` / `AddAddrs` / `RemoveAddrs` mutate state with
  validation + canonicalization. Domains are lowercased FQDNs and may carry a
  match-mode prefix (see below); CIDRs normalize to network form. Batches are
  atomic — one bad entry rejects the whole command. Responses report
  `{ changed, total }`.
- `GetStatus` reports live blocklist counts plus `block_page` (whether blocked
  names resolve to the loopback block page).
- `GetStats` reports blocking activity since startup: `{ since_unix,
  total_blocked, unique_domains, top[], recent[] }` (in-memory; resets on restart).
- `StartSession` / `StopSession` are parsed and validated but return an explicit
  *"not implemented"* error — they land in M5 with the committed-session lock.

## Domain match modes

A stored domain entry matches a queried name in one of three ways:

| Entry            | Matches                                   | Example                                |
| ---------------- | ----------------------------------------- | -------------------------------------- |
| `example.com`    | the host **and all subdomains** (default) | `example.com`, `www.example.com`       |
| `*.example.com`  | **subdomains only** (not the apex)        | `a.example.com`, not `example.com`     |
| `=example.com`   | the **exact host only**                   | `example.com`, not `www.example.com`   |

Matching is case- and trailing-dot-insensitive. A bare TLD or a single-label
wildbase (e.g. `*.com`) is rejected.
- Clean shutdown on SIGINT/SIGTERM, and the socket file is removed on every exit
  path including panic (`SocketGuard`).

The build order for the remaining milestones (`pf` enforcement, sessions +
committed lock, `launchd` packaging) is tracked in the project spec.

## DNS sinkhole (M3)

When run as root, the daemon binds a resolver on `127.0.0.1:53`:

- **Blocked** names (per the [match modes](#domain-match-modes)) are intercepted.
  If the block-page server is up, `A`/`AAAA` queries resolve to loopback
  (`127.0.0.1` / `::1`) so the browser reaches the block page; otherwise (or for
  other record types) they get an **NXDOMAIN** reply.
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
dig tauri.app  @127.0.0.1     # -> 127.0.0.1 (block page), or NXDOMAIN if :80 is taken
dig apple.com  @127.0.0.1     # -> real answer (forwarded)

# Full enforcement — repoints system DNS (restored on Ctrl-C):
sudo ./target/debug/blockerd
```

## Block page

Alongside the sinkhole (root only), the daemon serves a small built-in HTTP page
on `127.0.0.1:80`. Because blocked `A`/`AAAA` lookups resolve to loopback, a
browser visiting a blocked site lands on a styled "site blocked" page instead of
a connection error. If `:80` can't be bound, the daemon logs a warning and falls
back to NXDOMAIN for blocked names. This works for plain **HTTP** only — HTTPS
sites terminate TLS before we can answer, so those simply fail to connect (the
standard limitation of DNS-based blocking).

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
{ "v": 1, "cmd": "GetStats" }
{ "v": 1, "cmd": "AddDomains", "domains": ["youtube.com", "*.reddit.com", "=old.reddit.com"] }

// responses
{ "ok": true,  "data": { /* ... */ } }
{ "ok": false, "error": "..." }
```

`GetStatus` returns daemon/protocol versions, pid, whether the daemon holds root,
blocklist counts, and `block_page` (whether blocked names resolve to the loopback
block page):

```json
{ "ok": true, "data": {
  "daemon_version": "0.1.0", "protocol_version": 1, "pid": 12345,
  "privileged": false, "blocked_domains": 0, "blocked_cidrs": 0,
  "block_page": false, "session": null
} }
```

`GetStats` returns blocking activity since the daemon started (in-memory; resets
on restart):

```json
{ "ok": true, "data": {
  "since_unix": 1782700000, "total_blocked": 42, "unique_domains": 3,
  "top": [{ "entry": "*.reddit.com", "count": 30 }],
  "recent": [{ "name": "i.redd.it", "unix": 1782700123 }]
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
