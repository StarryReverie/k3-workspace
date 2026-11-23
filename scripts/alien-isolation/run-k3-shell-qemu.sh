#!/usr/bin/env bash
# Run the K3 isolation shell image on QEMU with the K3 address layout.
#
# Options:
#   --smp N        number of harts (default: 2)
#   --memory SIZE  RAM size (default: 10G)
#   --timeout SEC  stop after SEC seconds; 0 runs until Ctrl+A x (default: 60)
#   --out DIR      build directory (default: <alien>/build/k3-shell/smp-<N>)
#   --no-aia       use the default QEMU machine without IMSIC/APLIC
#   --nvme         attach <out>/nvme.img as a PCIe NVMe drive
#
# AIA is enabled by default: QEMU exposes an S-level IMSIC at 0x28000000 and
# an S-level APLIC at 0x0d000000, matching the addresses the K3 kernel maps.

set -euo pipefail

usage() {
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
K3_SMP=${K3_SMP:-2}
QEMU_MEMORY=${QEMU_MEMORY:-10G}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}
QEMU_AIA=${QEMU_AIA:-aplic-imsic}
QEMU_NVME=${QEMU_NVME:-0}
OUT_DIR=${OUT_DIR:-}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp) K3_SMP=$2; shift 2 ;;
        --memory) QEMU_MEMORY=$2; shift 2 ;;
        --timeout) QEMU_TIMEOUT=$2; shift 2 ;;
        --out) OUT_DIR=$2; shift 2 ;;
        --no-aia) QEMU_AIA=""; shift ;;
        --nvme) QEMU_NVME=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build/k3-shell/smp-$K3_SMP}
OUT_DIR=$(cd "$OUT_DIR" && pwd)

for file in "$OUT_DIR/kernel-k3" "$OUT_DIR/initramfs.cpio.gz"; do
    if [ ! -f "$file" ]; then
        echo "missing $file; run build-k3.sh --mode shell first" >&2
        exit 2
    fi
done
if [ "$QEMU_NVME" = "1" ] && [ ! -f "$OUT_DIR/nvme.img" ]; then
    echo "missing $OUT_DIR/nvme.img; run build-k3.sh --mode shell --disk first" >&2
    exit 2
fi

for cmd in qemu-system-riscv64; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "$cmd not found; run inside nix develop" >&2
        exit 2
    fi
done

MACHINE=virt
if [ -n "$QEMU_AIA" ]; then
    MACHINE="virt,aia=$QEMU_AIA"
fi

QEMU_ARGS=(
    qemu-system-riscv64
    -M "$MACHINE"
    -bios default
    -m "$QEMU_MEMORY"
    -smp "$K3_SMP"
    -nographic
    -kernel "$OUT_DIR/kernel-k3"
    -initrd "$OUT_DIR/initramfs.cpio.gz"
    -append "rdinit=/init"
)

if [ "$QEMU_NVME" = "1" ]; then
    QEMU_ARGS+=(
        -drive "file=$OUT_DIR/nvme.img,if=none,id=nvme0,format=raw"
        -device "nvme,serial=deadbeef,drive=nvme0"
    )
fi

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
