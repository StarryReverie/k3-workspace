#!/usr/bin/env bash
# Build the K3 platform code with QEMU memory/linker settings.
#
# This is a regression build: it uses the K3 platform module and SBI
# console, but runs on QEMU virt with 0x80200000 as the entry address.
#
# Optional:
#   K3_QEMU_SHELL  set to 1 to include bash and test interactive I/O
#   K3_SMP         set to 1 to enable CPU_NUM=2 and SBI HSM startup

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3-qemu}

TARGET=riscv64gc-unknown-none-elf

for cmd in cargo rustc cpio gzip; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"

echo "[1/5] building init"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build -p init --release --target "$TARGET"
)

echo "[2/5] preparing initramfs"
rm -rf "$OUT_DIR/initrd"
mkdir -p "$OUT_DIR/initrd/bin" "$OUT_DIR/initrd/tests"
cp "$ALIEN_DIR/target/$TARGET/release/init" "$OUT_DIR/initrd/tests/init"
if [ "${K3_QEMU_SHELL:-0}" = "1" ]; then
    cp "$ALIEN_DIR/tests/testbin-second-stage/bash" "$OUT_DIR/initrd/tests/bash"
    cp "$ALIEN_DIR/tests/testbin-second-stage/libc.so" "$OUT_DIR/initrd/tests/libc.so"
fi
cp "$ALIEN_DIR/tests/testbin-second-stage/busybox" "$OUT_DIR/initrd/bin/busybox"
ln -s busybox "$OUT_DIR/initrd/bin/sh"

mkdir -p "$ALIEN_DIR/tools/initrd"
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -ov --format=newc 2>/dev/null | gzip -9
) > "$ALIEN_DIR/tools/initrd/initramfs.cpio.gz"

echo "[3/5] building kernel (k3_qemu)"
touch "$ALIEN_DIR/subsystems/platform/src/k3_riscv/mod.rs"
FEATURES=k3_qemu
SMP_VALUE=1
if [ "${K3_SMP:-0}" = "1" ]; then
    FEATURES="k3_qemu,k3_smp"
    SMP_VALUE=2
fi
(
    cd "$ALIEN_DIR"
    SMP="$SMP_VALUE" cargo build --release -p kernel --target "$TARGET" \
        --no-default-features --features "$FEATURES"
)

echo "[4/5] linking kernel-k3-qemu"
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | sed -n 's/^host: //p')
RUST_LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"
"$RUST_LLD" -flavor gnu \
    -T "$ALIEN_DIR/tools/linker-qemu.ld" \
    -o "$OUT_DIR/kernel-k3-qemu" \
    "$ALIEN_DIR/target/$TARGET/release/libkernel.a"

echo "[5/5] summary"
ls -lh "$OUT_DIR/kernel-k3-qemu"
echo
echo "run:"
echo "  $ROOT/scripts/alien/run-k3-qemu.sh"
