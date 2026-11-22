#!/usr/bin/env bash
# Run the Alien isolation QEMU baseline built by build-qemu.sh.
#
# Optional:
#   ALIEN_DIR     source directory (default: <workspace>/alien-isolation)
#   OUT_DIR       build directory (default: $ALIEN_DIR/build)
#   QEMU_SMP      number of harts (default: 2)
#   QEMU_MEMORY   memory size (default: 2048M)
#   QEMU_TIMEOUT  seconds before stopping QEMU (default: 60; 0 = run forever)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build}
QEMU_SMP=${QEMU_SMP:-2}
QEMU_MEMORY=${QEMU_MEMORY:-2048M}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}

for file in "$ALIEN_DIR/target/riscv64/release/kernel" \
            "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/sdcard.img"; do
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
    -kernel "$ALIEN_DIR/target/riscv64/release/kernel"
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
