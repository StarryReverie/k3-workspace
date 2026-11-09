#!/usr/bin/env bash
# Run the QEMU baseline built by build-qemu.sh.
#
# Optional:
#   ALIEN_DIR     Alien source directory (default: <workspace>/alien)
#   OUT_DIR       output directory (default: $ALIEN_DIR/out-qemu)
#   QEMU_TIMEOUT  seconds before stopping QEMU (default: 60, 0 disables)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-qemu}
QEMU_TIMEOUT=${QEMU_TIMEOUT:-60}

for file in kernel-qemu sdcard.img initramfs.cpio.gz; do
    if [ ! -f "$OUT_DIR/$file" ]; then
        echo "missing $OUT_DIR/$file; run build-qemu.sh first" >&2
        exit 2
    fi
done

QEMU_ARGS=(
    qemu-system-riscv64
    -M virt
    -bios default
    -m 1024M
    -smp 1
    -nographic
    -kernel "$OUT_DIR/kernel-qemu"
    -drive "file=$OUT_DIR/sdcard.img,if=none,format=raw,id=x0"
    -device virtio-blk-device,drive=x0
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
    echo
    echo "QEMU stopped after ${QEMU_TIMEOUT}s"
    exit 0
fi
exit "$rc"
