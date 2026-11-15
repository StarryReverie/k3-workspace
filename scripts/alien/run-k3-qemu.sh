#!/usr/bin/env bash
# Run the k3_qemu regression kernel built by build-k3-qemu.sh.
#
# Optional:
#   QEMU_SMP      number of harts passed to QEMU (default: $OUT_DIR/smp, else 1)
#   QEMU_AIA      set to aplic-imsic to enable IMSIC/APLIC (default: off)
#   QEMU_TIMEOUT  seconds before QEMU is stopped (default: 60; 0 = forever)

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
QEMU_SMP=${QEMU_SMP:-$(cat "$OUT_DIR/smp" 2>/dev/null || echo 1)}
QEMU_AIA=${QEMU_AIA:-}
if [ -n "$QEMU_AIA" ]; then
    QEMU_MACHINE="virt,aia=$QEMU_AIA"
else
    QEMU_MACHINE="virt"
fi

if [ ! -f "$OUT_DIR/kernel-k3-qemu" ]; then
    echo "missing $OUT_DIR/kernel-k3-qemu; run build-k3-qemu.sh first" >&2
    exit 2
fi

QEMU_ARGS=(
    qemu-system-riscv64
    -M "$QEMU_MACHINE"
    -bios default
    -m 1024M
    -smp "$QEMU_SMP"
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
