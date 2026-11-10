#!/usr/bin/env bash
# Run the real K3 kernel ELF on QEMU virt with the K3 address layout.
#
# The kernel is linked at 0x102200000 and expects memory up to
# 0x300000000. QEMU RAM starts at 0x80000000, so 10 GiB is the smallest
# memory size that covers the K3 range exactly.
#
# This is a pre-board check for addresses above 4 GiB, the 8 GiB page
# table mapping, and the large frame allocator range. It is not a K3
# firmware or UART test.
#
# Optional:
#   OUT_DIR        directory containing kernel-k3 (default: out-k3-diag)
#   QEMU_MEM       QEMU memory size (default: 10G)
#   QEMU_TIMEOUT   seconds before QEMU is stopped (default: 60; 0 = run forever)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3-diag}
QEMU_MEM=${QEMU_MEM:-10G}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}

if [ ! -f "$OUT_DIR/kernel-k3" ]; then
    echo "missing $OUT_DIR/kernel-k3; run K3_DIAG=1 build-k3.sh first" >&2
    exit 2
fi

QEMU_ARGS=(
    qemu-system-riscv64
    -M virt
    -bios default
    -m "$QEMU_MEM"
    -smp 1
    -nographic
    -kernel "$OUT_DIR/kernel-k3"
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
