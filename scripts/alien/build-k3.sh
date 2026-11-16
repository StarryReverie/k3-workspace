#!/usr/bin/env bash
# Build the minimal Alien image for SpacemiT K3.
#
# Optional:
#   ALIEN_DIR   Alien source directory (default: <workspace>/alien)
#   OUT_DIR     output directory (default: $ALIEN_DIR/out-k3[-diag])
#   K3_DIAG     set to 1 to build a diagnostic image without bash
#   K3_UARTTEST set to 1 with K3_DIAG=1 to add a UART echo test
#   K3_CPUS     number of harts: 1, 2, 4, 8, or 16 (default: 1)
#   K3_SMP      legacy alias; set to 1 for two harts
#
# The kernel embeds the initramfs. The initramfs is created before the
# kernel build because subsystems/platform/src/k3_riscv/mod.rs uses
# include_bytes! on tools/initrd/initramfs.cpio.gz.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    echo "set ALIEN_DIR to a different checkout if needed" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)

K3_DIAG=${K3_DIAG:-0}
if [ "$K3_DIAG" = "1" ]; then
    OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3-diag}
    FIT_NAME=alien-k3-diag.itb
else
    OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3}
    FIT_NAME=alien-k3.itb
fi

TARGET=riscv64gc-unknown-none-elf

for cmd in cargo rustc mkimage cpio gzip; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/build-info"

echo "[1/7] building init"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build -p init --release --target "$TARGET"
)

echo "[2/7] preparing initramfs"
rm -rf "$OUT_DIR/initrd"
mkdir -p "$OUT_DIR/initrd/bin" "$OUT_DIR/initrd/tests"
cp "$ALIEN_DIR/target/$TARGET/release/init" "$OUT_DIR/initrd/tests/init"
if [ "$K3_DIAG" = "1" ]; then
    touch "$OUT_DIR/initrd/tests/k3-diag"
    if [ "${K3_UARTTEST:-0}" = "1" ]; then
        touch "$OUT_DIR/initrd/tests/k3-uarttest"
    fi
else
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

echo "[3/7] building kernel"
# include_bytes! does not always trigger a rebuild when only the included
# file changes, so touch the platform module.
touch "$ALIEN_DIR/subsystems/platform/src/k3_riscv/mod.rs"
FEATURES=k3
CPUS=${K3_CPUS:-1}
if [ -z "${K3_CPUS:-}" ] && [ "${K3_SMP:-0}" = "1" ]; then
    CPUS=2
fi
case "$CPUS" in
    1|2|4|8|16) ;;
    *)
        echo "invalid K3_CPUS: $CPUS (expected 1, 2, 4, 8, or 16)" >&2
        exit 2
        ;;
esac
if [ "$CPUS" -gt 1 ]; then
    FEATURES="k3,k3_smp"
fi
# 存储阶段需要 FAT 挂载支持。
FEATURES="$FEATURES,fat"
echo "$CPUS" > "$OUT_DIR/smp"
(
    cd "$ALIEN_DIR"
    SMP="$CPUS" cargo build --release -p kernel --target "$TARGET" \
        --no-default-features --features "$FEATURES"
)

echo "[4/7] linking kernel-k3"
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | sed -n 's/^host: //p')
RUST_LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"
"$RUST_LLD" -flavor gnu \
    -T "$ALIEN_DIR/tools/linker-k3.ld" \
    -o "$OUT_DIR/kernel-k3" \
    "$ALIEN_DIR/target/$TARGET/release/libkernel.a"

echo "[5/7] creating raw binary"
LLVM_OBJCOPY="$SYSROOT/lib/rustlib/$HOST/bin/llvm-objcopy"
"$LLVM_OBJCOPY" --strip-all -O binary \
    "$OUT_DIR/kernel-k3" "$ALIEN_DIR/alien-k3.bin"
gzip -9 -c "$ALIEN_DIR/alien-k3.bin" > "$ALIEN_DIR/alien-k3.bin.gz"

echo "[6/7] creating FIT image"
mkimage -f "$ALIEN_DIR/tools/k3.its" "$OUT_DIR/$FIT_NAME"

UARTTEST_INFO=0
if [ "$K3_DIAG" = "1" ] && [ "${K3_UARTTEST:-0}" = "1" ]; then
    UARTTEST_INFO=1
fi
cat > "$OUT_DIR/build-info" <<EOF
CPUS=$CPUS
DIAG=$K3_DIAG
UARTTEST=$UARTTEST_INFO
EOF

echo "[7/7] summary"
ls -lh "$OUT_DIR/kernel-k3" "$ALIEN_DIR/alien-k3.bin" "$ALIEN_DIR/alien-k3.bin.gz" "$OUT_DIR/$FIT_NAME"
echo
echo "U-Boot:"
echo "  tftpboot 0x140000000 $FIT_NAME"
echo "  bootm 0x140000000 - \$fdtcontroladdr"
