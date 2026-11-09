#!/usr/bin/env bash
# Clone Alien at the pinned commit and apply the K3 workspace patches.
#
# Usage:
#   ./scripts/alien/apply-patches.sh [destination]
#
# Default destination: $ALIEN_DIR, or <workspace>/../Alien if unset.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DEST=${1:-${ALIEN_DIR:-$ROOT/../Alien}}
COMMIT=181b263

if [ ! -d "$DEST/.git" ]; then
    echo "cloning Alien into $DEST"
    git clone https://github.com/Godones/Alien.git "$DEST"
fi

if [ -n "$(git -C "$DEST" status --porcelain)" ]; then
    echo "destination is not clean: $DEST" >&2
    echo "remove it or choose another destination" >&2
    exit 1
fi

echo "checking out $COMMIT"
git -C "$DEST" checkout "$COMMIT"

for patch in "$ROOT"/docs/agents/alien/patches/*.patch; do
    echo "applying $(basename "$patch")"
    git -C "$DEST" apply --check "$patch"
    git -C "$DEST" apply "$patch"
done

echo "done: $DEST"
