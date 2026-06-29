# Blocker (GUI)

The unprivileged desktop GUI for the blocker daemon — a **view/controller** over
`blockerd`. It owns no enforcement and no authoritative state; it bridges the
WebView to the daemon over a Unix-domain socket and re-fetches status after every
change. See the [repo README](../../README.md) for the overall architecture.

**Stack:** Tauri v2 (Rust backend) + SvelteKit (Svelte 5 runes, TypeScript,
`adapter-static` SPA, Vite), `bun` package manager.

## Routes

| Route        | What it does                                                                 |
| ------------ | --------------------------------------------------------------------------- |
| `/`          | **Status** — daemon connection, block counts, manual blocklist editor, session controls. |
| `/stats`     | **Statistics** — total blocked queries, top blocked entries, recent activity (`get_stats`). |
| `/library`   | **Library** — reusable site items (domains + CIDRs). Domains accept `example.com`, `*.example.com` (subdomains), and `=example.com` (exact). |
| `/sessions`  | **Sessions** — groups of library items on a recurrence + time-window schedule. |
| `/calendar`  | **Calendar** — a week view of when each session is active.                  |

## Menu-bar item

A macOS tray icon provides **Show / Statistics… / Reconcile schedule now / Quit**.
Closing the window **hides** it rather than quitting, so the app stays in the menu
bar and the background scheduler keeps running. Quit from the tray menu.

## Develop

```sh
# point at a running daemon's socket (see the repo README for starting blockerd)
BLOCKERD_SOCKET=/tmp/blockerd-dev.sock bun run tauri dev
```

## Checks & build

```sh
bun run check        # svelte-check (types)
bun run build        # static export to ../build
bun run tauri build  # frontend + Rust release, then macOS bundle
```

## Backend layout (`src-tauri/src`)

| File              | Responsibility                                                            |
| ----------------- | ------------------------------------------------------------------------- |
| `commands.rs`     | `#[tauri::command]` wrappers — one per daemon request (incl. `get_stats`). |
| `ipc_client.rs`   | Short-lived Unix-socket client with typed errors (offline vs. rejected).  |
| `config_store.rs` | Local JSON persistence for the library / sessions / managed set.          |
| `scheduler.rs`    | Background loop that reconciles active sessions into the daemon blocklist. |
| `tray.rs`         | Menu-bar item + actions.                                                   |

## Recommended IDE setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
