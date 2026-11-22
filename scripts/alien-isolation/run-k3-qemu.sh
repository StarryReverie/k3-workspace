#!/usr/bin/env bash
# Run the K3 diagnostic kernel on QEMU with the K3 address layout.
#
# Options:
#   --smp N        number of harts (default: 1)
#   --memory SIZE  RAM size (default: 10G, the smallest size that covers K3)
#   --timeout SEC  stop after SEC seconds; 0 runs until Ctrl+A x (default: 40)
#   --out DIR      build directory (default: <alien>/build/k3/smp-<N>)
#
# The kernel is linked at 0x102200000 and expects memory up to 0x300000000.
# QEMU RAM starts at 0x80000000, so 10 GiB covers the K3 range exactly. This
# is not a K3 firmware or UART test.

set -euo pipefail

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
K3_SMP=${K3_SMP:-1}
QEMU_MEMORY=${QEMU_MEMORY:-10G}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-40}
OUT_DIR=${OUT_DIR:-}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)
            K3_SMP=$2
            shift 2
            ;;
        --memory)
            QEMU_MEMORY=$2
            shift 2
            ;;
        --timeout)
            QEMU_TIMEOUT=$2
            shift 2
            ;;
        --out)
            OUT_DIR=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build/k3/smp-$K3_SMP}
OUT_DIR=$(cd "$OUT_DIR" && pwd)

for file in "$OUT_DIR/kernel-k3" "$OUT_DIR/initramfs.cpio.gz"; do
    if [ ! -f "$file" ]; then
        echo "missing $file; run build-k3.sh first" >&2
        exit 2
    fi
done

if ! command -v qemu-system-riscv64 >/dev/null 2>&1; then
    echo "qemu-system-riscv64 not found; run inside nix develop" >&2
    exit 2
fi

QEMU_ARGS=(
    qemu-system-riscv64
    -M virt
    -bios default
    -m "$QEMU_MEMORY"
    -smp "$K3_SMP"
    -nographic
    -kernel "$OUT_DIR/kernel-k3"
    -initrd "$OUT_DIR/initramfs.cpio.gz"
    -append "rdinit=/init"
)

if [ "$QEMU_TIMEOUT" = "0" ]; then
    exec "${QEMU_ARGS[@]}"
fi

set +e
timeout "$QEMU_TIMEOUT" "${QEMU_ARGS[@]}"
rc=$?
set -e
if [ "$rc" -eq 124 ]; then
    exit 0
fi
exit "$rc"
