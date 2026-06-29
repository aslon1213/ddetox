# app_blocker

A distraction blocker split into a **privileged daemon** (the source of truth that
enforces blocking) and an **unprivileged desktop GUI** (a thin view/controller).

```
┌─────────────────────────────┐        line-delimited JSON         ┌──────────────────────────┐
│ GUI  (app/ddetox)           │   over a Unix-domain socket        │ blockerd (root)          │
│ Tauri v2 + SvelteKit        │ ─────────────────────────────────▶ │ enforces pf/DNS, owns    │
│ unprivileged view/controller│ ◀───────────────────────────────── │ authoritative state      │
└─────────────────────────────┘  req {"v":1,...} / resp {"ok":...}  └──────────────────────────┘
                    │                                                          ▲
                    └──────────────── both depend on ──▶  protocol/ ◀──────────┘
```

The **daemon is the source of truth.** The GUI never enforces anything and never
trusts its own optimistic state — it re-fetches `get_status` after every change.
The committed-session lock (no early stop / no un-blocking once committed) is
enforced **daemon-side**; the GUI only disables those controls for honesty.

The wire protocol mirrors the real daemon (`dp_course`), not the original brief:
`GetStatus` returns blocklist **counts** (not the entries), and the daemon's own
committed-session command is not implemented yet. See [Wire protocol](#wire-protocol).

## Website library & scheduled sessions

On top of the manual blocklist, the app owns two **local** features (the daemon
never sees their config):

- **Library** (`/library`) — reusable "site" items, each a label plus domains
  (incl. `*.` wildcards) and optional CIDRs.
- **Sessions** (`/sessions`) — named groups of library items with a **schedule**:
  one or more rules, each a recurrence (every day · specific weekdays · a
  day-of-month range like the 1st–10th · every-N-days · a fixed date range) and
  optional daily time windows. The `/calendar` view paints active windows per week.

An **app-orchestrated scheduler** (a tokio loop in the Tauri backend, `scheduler.rs`)
ticks ~every 30s and on every config change: it unions the items of the sessions
active *now* and reconciles that against the daemon by pushing only the delta via
the existing `Add*/Remove*` commands. The session-managed set is tracked
separately (`managed.json`) so it never disturbs manually-added blocklist entries.

Two deliberate properties: schedules apply **while the app runs** (the chosen
trade-off), and because the daemon doesn't enforce yet, the scheduler drives the
daemon's *blocklist* (visible via counts) — real network blocking arrives with the
daemon's pf/DNS milestone. The model + recurrence engine live in the shared
`protocol` crate (`config` + `schedule`) so they can migrate into the daemon later.

## Layout

| Path          | What it is                                                                                  |
| ------------- | ------------------------------------------------------------------------------------------- |
| `protocol/`   | Shared types: wire protocol (`Request`, `Reply`, `Status`, `Envelope`) **and** the app config model + recurrence engine (`config`, `schedule`). Both sides depend on it so nothing drifts. |
| `app/ddetox/` | The GUI: Tauri v2 (Rust) backend (IPC client, config store, scheduler) + SvelteKit frontend (Status / Library / Sessions / Calendar). |
| `daemon/`     | **`blockerd-stub`** — an in-memory stub kept faithful to the real daemon (`dp_course`) for development. Enforces nothing; it mirrors the daemon's *current* capabilities (status + add/remove; sessions return "not implemented"). The real privileged daemon is a separate build. |

## Wire protocol

Line-delimited JSON over a Unix socket. **Requests** carry a protocol version `v`
and a `cmd` tag. **Responses** are `{"ok":true,"data":...}` or `{"ok":false,"error":...}`.

```jsonc
// GUI -> daemon
{"v":1,"cmd":"GetStatus"}
{"v":1,"cmd":"AddDomains","domains":["*.reddit.com"]}

// daemon -> GUI
// GetStatus: counts + metadata (NOT the blocklist entries; there is no list endpoint)
{"ok":true,"data":{"daemon_version":"0.1.0","protocol_version":1,"pid":90830,"privileged":true,"blocked_domains":5,"blocked_cidrs":0,"session":null}}
// Add/Remove: how many changed + the new total
{"ok":true,"data":{"changed":1,"total":6}}
// A rejection, surfaced verbatim in the UI
{"ok":false,"error":"command 'StartSession' is not implemented in this build"}
```

The socket path is `BLOCKERD_SOCKET` if set, otherwise
`/var/run/com.aslonkhamidov.blockerd.sock` (see `protocol::socket_path`). Note the
real daemon currently binds this socket `0o600` (root-only); reaching it from the
unprivileged GUI is a documented daemon-side milestone.

## Develop

The GUI needs a daemon to talk to. For development, run the stub on a dev socket
and point the GUI at the same path:

```sh
# Terminal 1 — the stub daemon (no root needed; enforces nothing)
BLOCKERD_SOCKET=/tmp/blockerd-dev.sock cargo run -p blockerd-stub

# Terminal 2 — the GUI (Tauri dev: launches Vite + the native window)
cd app/ddetox
BLOCKERD_SOCKET=/tmp/blockerd-dev.sock bun run tauri dev
```

With no daemon running, the GUI shows a "Daemon offline — retrying…" banner and
keeps reconnecting with backoff — that is the expected behavior, not a crash.

### Checks

```sh
cargo test -p protocol            # protocol round-trips + version guard
cd app/ddetox && bun run check    # svelte-check (types)
cd app/ddetox && bun run build    # static export to app/ddetox/build/
```

There is also an end-to-end socket smoke test of the daemon protocol (used during
development) that exercises the committed-session lock; see the project history.

## Build & sign (macOS)

```sh
cd app/ddetox
bun run tauri build               # builds frontend + Rust release, then bundles
```

Code signing / notarization credentials come from the environment, never the
repo. Set these before `tauri build` for a Developer-ID build:

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="app-specific-password"   # or APPLE_API_KEY/APPLE_API_ISSUER
export APPLE_TEAM_ID="TEAMID"
```

`src-tauri/tauri.conf.json` sets `bundle.macOS.signingIdentity: null` so the
identity is taken from `APPLE_SIGNING_IDENTITY` (or ad-hoc if unset). Tighten the
WebView CSP (`app.security.csp`) before shipping a production build.
