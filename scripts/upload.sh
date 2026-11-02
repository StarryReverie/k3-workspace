#!/usr/bin/env bash
# Upload one file into the current board session and print its tftp_url.
#
# Usage:
#   source scripts/session-env.sh [log]
#   ./scripts/upload.sh <local-file> [remote-path]
#
# Remote path defaults to boot/<basename>.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <local-file> [remote-path]" >&2
  exit 2
fi

file="$1"
remote="${2:-boot/$(basename -- "$file")}"

if [[ ! -f "$file" ]]; then
  echo "error: no such file: $file" >&2
  exit 1
fi

if [[ -z "${SID:-}" ]]; then
  echo "error: SID is not set; run: source scripts/session-env.sh <log>" >&2
  exit 1
fi

if [[ -z "${TOK:-}" ]]; then
  echo "error: TOK is not set; export OSTOOL_BOARD_ACCESS_TOKEN, or run inside the dev shell so secret-tool can read the keyring" >&2
  exit 1
fi

server="${SERVER:-https://ostool.muxai.net:14321}"
server="${server%/}"

size="$(stat -c %s -- "$file")"
limit=$((64 * 1024 * 1024))
if (( size > limit )); then
  echo "error: $file is $size bytes, above the 64 MiB upload limit; split it first" >&2
  exit 1
fi

response="$(
  curl -sS --fail-with-body -X PUT \
    -H "Authorization: Bearer $TOK" \
    -H "X-File-Path: $remote" \
    --data-binary "@$file" \
    "$server/api/v1/sessions/$SID/files"
)" || {
  echo "error: upload failed; if this is 401, run 'ostool board ls' to refresh the token, then re-source scripts/session-env.sh" >&2
  exit 1
}

printf '%s\n' "$response"

python3 -c '
import json, sys
try:
    data = json.load(sys.stdin)
except json.JSONDecodeError:
    raise SystemExit(0)
url = data.get("tftp_url")
if url:
    print("tftp_url:", url)
' <<<"$response"
