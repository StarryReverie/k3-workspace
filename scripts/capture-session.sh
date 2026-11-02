#!/usr/bin/env bash
# Connect to a K3 board and keep the full pty log under logs/.
#
# ostool only forwards serial input when stdin/stdout are a real tty, so
# `script(1)` is used to provide a pty while also recording the session.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname -- "$script_dir")"

BOARD_TYPE="${BOARD_TYPE:-BananaPi}"
LOG_DIR="${LOG_DIR:-$root/logs}"
mkdir -p "$LOG_DIR"

if ! command -v ostool >/dev/null 2>&1; then
  echo "ostool not found in PATH; enter the dev shell first: nix develop" >&2
  exit 1
fi

stamp="$(date +%Y%m%d-%H%M%S)"
log="$LOG_DIR/session-$stamp.log"

printf 'board type : %s\n' "$BOARD_TYPE"
printf 'session log: %s\n' "$log"
printf 'interrupt  : Ctrl+C during "Autoboot in 3 seconds"\n'
printf 'exit       : Ctrl+A, release, then x\n\n'

exec script -qefc "ostool board connect -b $BOARD_TYPE" "$log"
