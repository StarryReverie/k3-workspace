#!/usr/bin/env bash
# Run the k3_qemu regression kernel built by build-k3-qemu.sh.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3-qemu}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}

if [ ! -f "$OUT_DIR/kernel-k3-qemu" ]; then
    echo "missing $OUT_DIR/kernel-k3-qemu; run build-k3-qemu.sh first" >&2
    exit 2
fi

QEMU_ARGS=(
    qemu-system-riscv64
    -M virt
    -bios default
    -m 1024M
    -smp 1
    -nographic
    -kernel "$OUT_DIR/kernel-k3-qemu"
)

if [ "$QEMU_TIMEOUT" = "0" ]; then
    exec "${QEMU_ARGS[@]}"
fi

set +e
timeout "$QEMU_TIMEOUT" "${QEMU_ARGS[@]}"
rc=$?
set -e
if [ "$rc" -eq 124 ]; then
    echo
    echo "QEMU stopped after ${QEMU_TIMEOUT}s"
    exit 0
fi
exit "$rc"
