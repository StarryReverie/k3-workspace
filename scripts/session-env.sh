#!/usr/bin/env bash
# Source this file to export the current board-session credentials:
#
#   source scripts/session-env.sh                 # newest logs/session-*.log
#   source scripts/session-env.sh logs/foo.log
#
# Exports:
#   SID     - board session id
#   TOK     - bearer token (OSTOOL_BOARD_ACCESS_TOKEN wins if set)
#   SERVER  - board server base URL
#
# This file is meant to be sourced; it intentionally does not touch `set -e`.

_k3_script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
_k3_root="$(dirname -- "$_k3_script_dir")"

_k3_extract_sid() {
  local log="$1"
  grep -aoiE 'session[_ -]?id[^A-Za-z0-9]+[A-Za-z0-9_-]+' "$log" 2>/dev/null \
    | head -n1 \
    | sed -E 's/.*[^A-Za-z0-9_-]//' \
    | tr -d '\r'
}

_k3_extract_token() {
  if [[ -n "${OSTOOL_BOARD_ACCESS_TOKEN:-}" ]]; then
    printf '%s' "$OSTOOL_BOARD_ACCESS_TOKEN"
    return 0
  fi

  local file_token
  file_token="$(python3 - "${HOME}/.config/ostool/hosts.json" <<'PY'
import json
import sys

path = sys.argv[1]

try:
    with open(path, encoding="utf-8") as fh:
        data = json.load(fh)
except FileNotFoundError:
    raise SystemExit(0)

def find(node):
    if isinstance(node, dict):
        for key, value in node.items():
            if key in ("access_token", "token") and isinstance(value, str) and value:
                return value
            found = find(value)
            if found:
                return found
    elif isinstance(node, list):
        for value in node:
            found = find(value)
            if found:
                return found
    return None

print(find(data) or "", end="")
PY
)"
  if [[ -n "$file_token" ]]; then
    printf '%s' "$file_token"
    return 0
  fi

  # keyring v4 on Linux uses the Secret Service via zbus. ostool stores one
  # item per server: service=ostool, username=board-server:<sha256(base_url)>.
  command -v secret-tool >/dev/null 2>&1 || return 0

  local base="${SERVER:-https://ostool.muxai.net:14321}"
  base="${base%/}"
  local url digest secret
  for url in "$base/" "$base"; do
    digest="$(printf '%s' "$url" | sha256sum | cut -d' ' -f1)"
    secret="$(secret-tool lookup service ostool username "board-server:$digest" 2>/dev/null || true)"
    if [[ -n "$secret" ]]; then
      printf '%s' "$secret" | python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except json.JSONDecodeError:
    raise SystemExit(0)
print(data.get("access_token") or data.get("token") or "", end="")
'
      return 0
    fi
  done
}

_k3_log="${1:-}"
if [[ -z "$_k3_log" ]]; then
  _k3_log="$(ls -1t "$_k3_root"/logs/session-*.log 2>/dev/null | head -n1 || true)"
fi

SERVER="${OSTOOL_SERVER:-https://ostool.muxai.net:14321}"
SERVER="${SERVER%/}"

if [[ -z "$_k3_log" || ! -f "$_k3_log" ]]; then
  echo "warning: no session log found; pass one explicitly" >&2
  SID=""
else
  SID="$(_k3_extract_sid "$_k3_log")"
fi

TOK="$(_k3_extract_token)"

export SID TOK SERVER

echo "log     : ${_k3_log:-<none>}"
echo "session : ${SID:-<missing>}"
echo "token   : $([[ -n "$TOK" ]] && echo found || echo '<missing>')"
echo "server  : $SERVER"

if [[ -z "$TOK" ]]; then
  echo "hint: export OSTOOL_BOARD_ACCESS_TOKEN=... if credentials live in the keyring" >&2
fi

unset _k3_script_dir _k3_root _k3_log
unset -f _k3_extract_sid _k3_extract_token
