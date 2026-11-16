#!/usr/bin/env bash
# Build the K3 platform code with QEMU memory/linker settings.
#
# This is a regression build: it uses the K3 platform module and SBI
# console, but runs on QEMU virt with 0x80200000 as the entry address.
#
# Optional:
#   K3_QEMU_SHELL  set to 1 to include bash and test interactive I/O
#   K3_QEMU_DISK   set to 1 to load init/bash/libc from a virtio-blk image
#   K3_CPUS        number of harts: 1, 2, 4, 8, or 16 (default: 1)
#   K3_SMP         legacy alias; set to 1 for two harts

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/out-k3-qemu}
DISK=${K3_QEMU_DISK:-0}

TARGET=riscv64gc-unknown-none-elf

REQUIRED="cargo rustc cpio gzip"
if [ "$DISK" = "1" ]; then
    REQUIRED="$REQUIRED mkfs.fat mcopy"
fi
for cmd in $REQUIRED; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR/build-info"

echo "[1/6] building init"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build -p init --release --target "$TARGET"
)

echo "[2/6] preparing initramfs"
rm -rf "$OUT_DIR/initrd"
mkdir -p "$OUT_DIR/initrd/bin"
if [ "$DISK" != "1" ]; then
    mkdir -p "$OUT_DIR/initrd/tests"
    cp "$ALIEN_DIR/target/$TARGET/release/init" "$OUT_DIR/initrd/tests/init"
fi
if [ "${K3_QEMU_SHELL:-0}" = "1" ] && [ "$DISK" != "1" ]; then
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

echo "[3/6] building kernel (k3_qemu)"
touch "$ALIEN_DIR/subsystems/platform/src/k3_riscv/mod.rs"
FEATURES=k3_qemu
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
    FEATURES="k3_qemu,k3_smp"
fi
if [ "$DISK" = "1" ]; then
    FEATURES="$FEATURES,fat"
fi
echo "$CPUS" > "$OUT_DIR/smp"
(
    cd "$ALIEN_DIR"
    SMP="$CPUS" cargo build --release -p kernel --target "$TARGET" \
        --no-default-features --features "$FEATURES"
)

echo "[4/6] linking kernel-k3-qemu"
SYSROOT=$(rustc --print sysroot)
HOST=$(rustc -vV | sed -n 's/^host: //p')
RUST_LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"
"$RUST_LLD" -flavor gnu \
    -T "$ALIEN_DIR/tools/linker-qemu.ld" \
    -o "$OUT_DIR/kernel-k3-qemu" \
    "$ALIEN_DIR/target/$TARGET/release/libkernel.a"

echo "[5/6] creating disk image"
if [ "$DISK" = "1" ]; then
    rm -rf "$OUT_DIR/diskfs"
    mkdir -p "$OUT_DIR/diskfs"
    cp "$ALIEN_DIR/target/$TARGET/release/init" "$OUT_DIR/diskfs/init"
    cp "$ALIEN_DIR/tests/testbin-second-stage/bash" "$OUT_DIR/diskfs/bash"
    cp "$ALIEN_DIR/tests/testbin-second-stage/libc.so" "$OUT_DIR/diskfs/libc.so"
    printf 'alien qemu nvme mount ok\n' > "$OUT_DIR/diskfs/HELLO.TXT"
    if command -v gen_ksym >/dev/null 2>&1; then
        nm -n -C "$OUT_DIR/kernel-k3-qemu" \
            | grep ' [Tt] ' | grep -v '\.L' | grep -v '\$x' \
            | gen_ksym > "$OUT_DIR/diskfs/kallsyms"
    fi
    rm -f "$OUT_DIR/sdcard.img"
    dd if=/dev/zero of="$OUT_DIR/sdcard.img" bs=1M count=72 status=none
    mkfs.fat -F 32 "$OUT_DIR/sdcard.img" >/dev/null
    for file in "$OUT_DIR"/diskfs/*; do
        mcopy -i "$OUT_DIR/sdcard.img" "$file" "::/$(basename "$file")"
    done
    echo "      $OUT_DIR/sdcard.img"
else
    echo "      skipped (set K3_QEMU_DISK=1 to enable)"
fi

SHELL_INFO=0
if [ "$DISK" != "1" ] && [ "${K3_QEMU_SHELL:-0}" = "1" ]; then
    SHELL_INFO=1
fi
cat > "$OUT_DIR/build-info" <<EOF
CPUS=$CPUS
DISK=$DISK
SHELL=$SHELL_INFO
EOF

echo "[6/6] summary"
ls -lh "$OUT_DIR/kernel-k3-qemu"
if [ "$DISK" = "1" ]; then
    ls -lh "$OUT_DIR/sdcard.img"
fi
echo
echo "run:"
if [ "$DISK" = "1" ]; then
    echo "  QEMU_DISK=1 $ROOT/scripts/alien/run-k3-qemu.sh"
else
    echo "  $ROOT/scripts/alien/run-k3-qemu.sh"
fi
