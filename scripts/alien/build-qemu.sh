#!/usr/bin/env bash
# Build the Alien kernel and a minimal QEMU disk/initramfs.
#
# Optional:
#   ALIEN_DIR   Alien source directory (default: <workspace>/alien)
#   OUT_DIR     output directory (default: $ALIEN_DIR/out-qemu)
#
# The script does not use the Alien Makefile. It builds the kernel
# static library, links it with the QEMU linker script, builds the
# user init, and creates a FAT32 disk image plus an initramfs.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    echo "set ALIEN_DIR to a different checkout if needed" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-qemu}

TARGET=riscv64gc-unknown-none-elf
FEATURES=qemu,slab,fat

for cmd in cargo rustc mkfs.fat mcopy cpio gzip; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"

echo "[1/6] building kernel"
(
    cd "$ALIEN_DIR"
    cargo build --release -p kernel --target "$TARGET" --features "$FEATURES"
)

echo "[2/6] linking kernel-qemu"
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | sed -n 's/^host: //p')
RUST_LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"
"$RUST_LLD" -flavor gnu \
    -T "$ALIEN_DIR/tools/linker-qemu.ld" \
    -o "$OUT_DIR/kernel-qemu" \
    "$ALIEN_DIR/target/$TARGET/release/libkernel.a"

echo "[3/6] building init"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build -p init --release --target "$TARGET"
)

echo "[4/6] preparing disk files"
rm -rf "$OUT_DIR/diskfs" "$OUT_DIR/initrd"
mkdir -p "$OUT_DIR/diskfs" "$OUT_DIR/initrd/bin"
cp "$ALIEN_DIR/target/$TARGET/release/init" "$OUT_DIR/diskfs/init"
cp "$ALIEN_DIR/tests/testbin-second-stage/bash" "$OUT_DIR/diskfs/bash"
cp "$ALIEN_DIR/tests/testbin-second-stage/libc.so" "$OUT_DIR/diskfs/libc.so"
cp "$ALIEN_DIR/tests/testbin-second-stage/busybox" "$OUT_DIR/initrd/bin/busybox"
ln -s busybox "$OUT_DIR/initrd/bin/sh"

# kallsyms is optional. The kernel now prints a message and continues
# when /tests/kallsyms is absent.
if command -v gen_ksym >/dev/null 2>&1; then
    echo "      generating kallsyms"
    nm -n -C "$OUT_DIR/kernel-qemu" \
        | grep ' [Tt] ' \
        | grep -v '\.L' \
        | grep -v '\$x' \
        | gen_ksym > "$OUT_DIR/diskfs/kallsyms"
else
    echo "      gen_ksym not found; skipping kallsyms"
fi

echo "[5/6] creating initramfs"
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -ov --format=newc 2>/dev/null | gzip -9
) > "$OUT_DIR/initramfs.cpio.gz"

echo "[6/6] creating FAT32 disk image"
rm -f "$OUT_DIR/sdcard.img"
dd if=/dev/zero of="$OUT_DIR/sdcard.img" bs=1M count=72 status=none
mkfs.fat -F 32 "$OUT_DIR/sdcard.img" >/dev/null
for file in "$OUT_DIR"/diskfs/*; do
    mcopy -i "$OUT_DIR/sdcard.img" "$file" "::/$(basename "$file")"
done

echo
echo "built:"
echo "  $OUT_DIR/kernel-qemu"
echo "  $OUT_DIR/sdcard.img"
echo "  $OUT_DIR/initramfs.cpio.gz"
echo
echo "run:"
echo "  $ROOT/scripts/alien/run-qemu.sh"
