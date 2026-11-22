#!/usr/bin/env bash
# Run the Alien isolation QEMU image built by build-qemu.sh.
#
# Options:
#   --smp N        number of harts (default: 2)
#   --memory SIZE  memory size (default: 2048M)
#   --timeout SEC  stop after SEC seconds; 0 runs until Ctrl+A x (default: 60)
#   --out DIR      build directory (default: <alien>/build/smp-<N>)
#
# Environment variables are also accepted: QEMU_SMP, QEMU_MEMORY,
# QEMU_TIMEOUT, OUT_DIR, ALIEN_DIR.

set -euo pipefail

usage() {
    sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
QEMU_SMP=${QEMU_SMP:-2}
QEMU_MEMORY=${QEMU_MEMORY:-2048M}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}
OUT_DIR=${OUT_DIR:-}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)
            QEMU_SMP=$2
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
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build/smp-$QEMU_SMP}
OUT_DIR=$(cd "$OUT_DIR" && pwd)

KERNEL=${KERNEL:-$OUT_DIR/kernel}
for file in "$KERNEL" "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/sdcard.img"; do
    if [ ! -f "$file" ]; then
        echo "missing $file; run build-qemu.sh first" >&2
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
    -smp "$QEMU_SMP"
    -nographic
    -kernel "$KERNEL"
    -initrd "$OUT_DIR/initramfs.cpio.gz"
    -append "rdinit=/init"
    -drive "file=$OUT_DIR/sdcard.img,if=none,format=raw,id=x0"
    -device virtio-blk-device,drive=x0
    -device virtio-net-device,netdev=net0
    -netdev "user,id=net0,hostfwd=tcp::55555-:55555,hostfwd=udp::5555-:5555"
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
