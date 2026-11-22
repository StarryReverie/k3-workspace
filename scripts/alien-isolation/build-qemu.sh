#!/usr/bin/env bash
# Build the Alien isolation QEMU baseline.
#
# Optional:
#   ALIEN_DIR  source directory (default: <workspace>/alien-isolation)
#   OUT_DIR    output directory (default: $ALIEN_DIR/build)
#   QEMU_SMP   number of harts (default: 2)
#   LOG        domain log level (default: INFO)
#
# The script follows the upstream Makefile for the kernel, init, domains
# and disk image. It adds two things the upstream Makefile does not have:
# artifact checks after `cargo domain build-all`, and a lock seed for the
# yanked `core2` crate used by the vfs domains.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien isolation source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build}
QEMU_SMP=${QEMU_SMP:-2}
LOG=${LOG:-INFO}

for cmd in cargo rustc cpio gzip mkfs.fat mcopy python3; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        echo "run this script inside nix develop" >&2
        exit 2
    fi
done

mkdir -p "$OUT_DIR"
CPU_FILE="$ALIEN_DIR/domain-lib/config/src/lib.rs"
CPU_BAK="$OUT_DIR/config-lib.rs.bak"
cp "$CPU_FILE" "$CPU_BAK"
restore_cpu() {
    cp "$CPU_BAK" "$CPU_FILE"
}
trap restore_cpu EXIT

echo "[1/5] building domains"
# The vfs workspaces depend on core2 0.4.0, which is yanked from crates.io.
# A lock that already contains it lets Cargo keep using it.
for workspace in common/vfs common/vfs2; do
    if [ ! -f "$ALIEN_DIR/domains/$workspace/Cargo.lock" ]; then
        cp "$ALIEN_DIR/Cargo.lock" "$ALIEN_DIR/domains/$workspace/Cargo.lock"
    fi
done
(
    cd "$ALIEN_DIR/domains"
    CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback \
        cargo domain build-all -l "$LOG" -o "$OUT_DIR"
) > "$OUT_DIR/domains.log" 2>&1

python3 - "$ALIEN_DIR" "$OUT_DIR" <<'PY'
import pathlib
import sys
import tomllib

alien = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
config = tomllib.loads((alien / "domains/domain-list.toml").read_text())["domains"]
failed = False
for group, directory in (("init_members", "init"), ("disk_members", "disk")):
    expected = {f"g{name}" for name in config[group]}
    path = out / directory
    actual = {entry.name for entry in path.glob("g*")} if path.is_dir() else set()
    missing = sorted(expected - actual)
    if missing:
        failed = True
        print(f"missing {directory} domains: {', '.join(missing)}", file=sys.stderr)
if failed:
    raise SystemExit(1)
print(f"domains: {len(config['init_members'])} init, {len(config['disk_members'])} disk")
PY

echo "[2/5] building kernel"
(
    cd "$ALIEN_DIR"
    PLATFORM=qemu_riscv SMP="$QEMU_SMP" \
        RUSTFLAGS='--cfg getrandom_backend="custom"' \
        cargo build --release -p kernel --target ./tools/riscv64.json \
        -Zbuild-std=core,alloc \
        -Zbuild-std-features=compiler-builtins-mem
) > "$OUT_DIR/kernel.log" 2>&1

echo "[3/5] building init"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build --release -p init --target riscv64gc-unknown-none-elf
) > "$OUT_DIR/init.log" 2>&1

echo "[4/5] creating initramfs"
rm -rf "$OUT_DIR/initrd" "$OUT_DIR/rootfs"
mkdir -p "$OUT_DIR/initrd/bin" "$OUT_DIR/rootfs"
cp "$ALIEN_DIR/target/riscv64gc-unknown-none-elf/release/init" "$OUT_DIR/rootfs/init"
cp "$ALIEN_DIR/user/bin/bash" "$OUT_DIR/rootfs/bash"
cp "$ALIEN_DIR/user/bin/bash" "$OUT_DIR/initrd/bin/bash"
ln -s bash "$OUT_DIR/initrd/bin/sh"
cp "$OUT_DIR"/init/g* "$OUT_DIR/initrd/"
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -o --format=newc 2>/dev/null | gzip -9
) > "$OUT_DIR/initramfs.cpio.gz"

echo "[5/5] creating FAT disk"
rm -f "$OUT_DIR/sdcard.img"
dd if=/dev/zero of="$OUT_DIR/sdcard.img" bs=1M count=72 status=none
mkfs.fat -F 32 "$OUT_DIR/sdcard.img" >/dev/null
mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/init" "::/init"
mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/bash" "::/bash"
if [ -n "$(find "$OUT_DIR/disk" -maxdepth 1 -name 'g*' -print -quit 2>/dev/null)" ]; then
    mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR"/disk/g* "::/"
fi

cat > "$OUT_DIR/build-info" <<EOF
SMP=$QEMU_SMP
INIT_DOMAINS=$(find "$OUT_DIR/init" -type f | wc -l)
DISK_DOMAINS=$(find "$OUT_DIR/disk" -type f | wc -l)
EOF

echo
echo "built:"
ls -lh "$ALIEN_DIR/target/riscv64/release/kernel"
ls -lh "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/sdcard.img"
echo
echo "run:"
echo "  $ROOT/scripts/alien-isolation/run-qemu.sh"
