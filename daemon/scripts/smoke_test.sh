#!/usr/bin/env bash
# Milestone 1–2 manual smoke test for blockerd's IPC + state surface.
#
# Builds the daemon, runs it on a throwaway socket + state db (no root needed),
# and pokes it with a few requests via `nc -U`. There is no enforcement yet, so
# nothing system-wide is changed; the temp state db is discarded on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMPDIR_RUN="$(mktemp -d)"
SOCK="$TMPDIR_RUN/blockerd.sock"
STATE="$TMPDIR_RUN/state.db"
DAEMON_PID=""

cleanup() {
  [ -n "$DAEMON_PID" ] && kill "$DAEMON_PID" 2>/dev/null || true
  rm -rf "$TMPDIR_RUN"
}
trap cleanup EXIT

echo "==> building (debug)"
cargo build --quiet --manifest-path "$ROOT/Cargo.toml"

echo "==> starting daemon on $SOCK (state: $STATE)"
BLOCKERD_LOG=info "$ROOT/target/debug/blockerd" --socket "$SOCK" --state "$STATE" &
DAEMON_PID=$!

# Wait for the socket to appear.
for _ in $(seq 1 100); do
  [ -S "$SOCK" ] && break
  sleep 0.05
done
[ -S "$SOCK" ] || { echo "FAIL: socket never appeared"; exit 1; }

send() {
  echo "--> $1"
  printf '%s\n' "$1" | nc -U "$SOCK" -w 1
  echo
}

send '{"v":1,"cmd":"GetStatus"}'                                   # expect 0 domains
send '{"v":2,"cmd":"GetStatus"}'                                   # expect ok:false (version)
send '{"v":1,"cmd":"Nope"}'                                        # expect ok:false (unknown cmd)
send '{"v":1,"cmd":"AddDomains","domains":["youtube.com","*.reddit.com"]}'  # changed:2
send '{"v":1,"cmd":"AddAddrs","cidrs":["142.250.1.5/16"]}'         # canonicalized -> /16
send '{"v":1,"cmd":"AddDomains","domains":["bad domain"]}'         # expect ok:false (invalid)
send '{"v":1,"cmd":"GetStatus"}'                                   # expect 2 domains, 1 cidr
send '{"v":1,"cmd":"RemoveDomains","domains":["youtube.com"]}'     # changed:1
send '{"v":1,"cmd":"StartSession","until_unix":9999999999,"committed":true}'  # ok:false (M5)

echo "==> done (daemon will be stopped and socket removed; temp state discarded)"
