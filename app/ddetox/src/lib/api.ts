// Typed bridge to the Tauri backend — one wrapper per `#[tauri::command]`.
//
// These mirror the shared `protocol` crate's types. The GUI treats `Status` as
// read-only truth from the daemon; it never edits a local copy and re-fetches
// after every mutation.

import { invoke } from "@tauri-apps/api/core";

/**
 * Active session details. The daemon hasn't finalized this shape (sessions
 * aren't implemented yet), so it's treated as opaque with a couple of
 * forward-looked-for fields.
 */
export interface SessionInfo {
  committed?: boolean;
  until_unix?: number;
  [key: string]: unknown;
}

/**
 * Mirror of the daemon's `GetStatus` payload. Note this carries blocklist
 * **counts**, not the entries themselves — the daemon does not expose the lists.
 */
export interface Status {
  daemon_version: string;
  protocol_version: number;
  pid: number;
  privileged: boolean;
  blocked_domains: number;
  blocked_cidrs: number;
  session: SessionInfo | null;
}

/**
 * Thrown when the daemon can't be reached at all (vs. reached-and-rejected).
 * The UI shows a retrying banner for this rather than a one-off error.
 */
export class DaemonOfflineError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DaemonOfflineError";
  }
}

// The backend formats unreachable errors with this stable prefix
// (see ipc_client::IpcError::Unreachable).
const OFFLINE_PREFIX = "daemon unreachable";

function normalize(e: unknown): never {
  const msg =
    typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  if (msg.toLowerCase().startsWith(OFFLINE_PREFIX)) {
    throw new DaemonOfflineError(msg);
  }
  // Daemon rejections (e.g. a committed-session lock) propagate verbatim.
  throw new Error(msg);
}

export async function getStatus(): Promise<Status> {
  try {
    return await invoke<Status>("get_status");
  } catch (e) {
    normalize(e);
  }
}

export async function addDomains(domains: string[]): Promise<void> {
  try {
    await invoke("add_domains", { domains });
  } catch (e) {
    normalize(e);
  }
}

export async function removeDomains(domains: string[]): Promise<void> {
  try {
    await invoke("remove_domains", { domains });
  } catch (e) {
    normalize(e);
  }
}

export async function addAddrs(cidrs: string[]): Promise<void> {
  try {
    await invoke("add_addrs", { cidrs });
  } catch (e) {
    normalize(e);
  }
}

export async function removeAddrs(cidrs: string[]): Promise<void> {
  try {
    await invoke("remove_addrs", { cidrs });
  } catch (e) {
    normalize(e);
  }
}

/** Start a session ending at `untilUnix` (seconds since epoch). */
export async function startSession(
  untilUnix: number,
  committed: boolean,
): Promise<void> {
  try {
    await invoke("start_session", { untilUnix, committed });
  } catch (e) {
    normalize(e);
  }
}

export async function stopSession(): Promise<void> {
  try {
    await invoke("stop_session");
  } catch (e) {
    normalize(e);
  }
}
