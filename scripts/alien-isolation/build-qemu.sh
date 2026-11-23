#!/usr/bin/env bash
# Build the Alien isolation QEMU image.
#
# Options:
#   --smp N      number of harts (default: 2)
#   --log LEVEL  domain log level; the kernel stays quiet (default: INFO)
#   --out DIR    output directory (default: <alien>/build/smp-<N>)
#   --crash      build the block domain with fault injection (test only)
#
# Environment variables are also accepted: QEMU_SMP, LOG, OUT_DIR, ALIEN_DIR.
#
# The script follows the upstream Makefile for the kernel, domains, user
# programs and disk image. It adds three things the upstream Makefile does not
# have: artifact checks after `cargo domain build-all`, a lock seed for the
# yanked `core2` crate, and a separate output tree per hart count.

set -euo pipefail

usage() {
    sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
QEMU_SMP=${QEMU_SMP:-2}
LOG=${LOG:-INFO}
OUT_DIR=${OUT_DIR:-}
CRASH=${CRASH:-0}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)
            QEMU_SMP=$2
            shift 2
            ;;
        --log)
            LOG=$2
            shift 2
            ;;
        --out)
            OUT_DIR=$2
            shift 2
            ;;
        --crash)
            CRASH=1
            shift
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

if ! [[ "$QEMU_SMP" =~ ^[1-9][0-9]*$ ]]; then
    echo "SMP must be a positive integer: $QEMU_SMP" >&2
    exit 2
fi
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien isolation source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build/smp-$QEMU_SMP}
mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)

for cmd in cargo rustc cpio gzip mkfs.fat mcopy mmd python3; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        echo "run this script inside nix develop" >&2
        exit 2
    fi
done

USER_APPS="dvfs dlog dtest dtask dnet"
USER_BINARIES="$ALIEN_DIR/target/riscv64gc-unknown-none-elf/release"

echo "[1/6] building domains (LOG=$LOG)"
# The vfs workspaces depend on core2 0.4.0, which is yanked from crates.io.
# A lock that already contains it lets Cargo keep using it.
for workspace in common/vfs common/vfs2; do
    if [ ! -f "$ALIEN_DIR/domains/$workspace/Cargo.lock" ]; then
        cp "$ALIEN_DIR/Cargo.lock" "$ALIEN_DIR/domains/$workspace/Cargo.lock"
    fi
done
# Remove old artifacts so that a failed build cannot pass the check below by
# leaving a previously built ELF in place.
rm -rf "$OUT_DIR/init" "$OUT_DIR/disk"
DOMAIN_FEATURE_ARGS=()
if [ "$CRASH" = "1" ]; then
    # Fault injection lives in the virtio block domain. The feature is only
    # enabled for test images; normal images keep the block domain intact.
    DOMAIN_FEATURE_ARGS=(--features virtio_mmio_block=crash)
fi
(
    cd "$ALIEN_DIR/domains"
    # Domain ELFs index per-hart arrays with MAX_CPU_NUM, so they do not depend
    # on the hart count and can be reused across SMP builds.
    unset SMP
    CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback \
        LOG="$LOG" \
        cargo domain build-all -l "$LOG" -o "$OUT_DIR" "${DOMAIN_FEATURE_ARGS[@]}"
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

echo "[2/6] building kernel"
(
    cd "$ALIEN_DIR"
    PLATFORM=qemu_riscv SMP="$QEMU_SMP" \
        RUSTFLAGS='--cfg getrandom_backend="custom"' \
        cargo build --release -p kernel --target ./tools/riscv64.json \
        -Zbuild-std=core,alloc \
        -Zbuild-std-features=compiler-builtins-mem
) > "$OUT_DIR/kernel.log" 2>&1
cp "$ALIEN_DIR/target/riscv64/release/kernel" "$OUT_DIR/kernel"

echo "[3/6] building user programs"
(
    cd "$ALIEN_DIR"
    RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
        cargo build --release --target riscv64gc-unknown-none-elf \
        -p init $(printf -- '-p %s ' $USER_APPS)
) > "$OUT_DIR/user.log" 2>&1
for app in $USER_APPS; do
    if [ ! -f "$USER_BINARIES/$app" ]; then
        echo "missing user program: $app" >&2
        exit 1
    fi
done

echo "[4/6] creating initramfs"
rm -rf "$OUT_DIR/initrd" "$OUT_DIR/rootfs"
mkdir -p "$OUT_DIR/initrd/bin" "$OUT_DIR/rootfs"
cp "$USER_BINARIES/init" "$OUT_DIR/rootfs/init"
cp "$ALIEN_DIR/user/bin/bash" "$OUT_DIR/rootfs/bash"
cp "$ALIEN_DIR/user/bin/bash" "$OUT_DIR/initrd/bin/bash"
ln -s bash "$OUT_DIR/initrd/bin/sh"
cp "$OUT_DIR"/init/g* "$OUT_DIR/initrd/"
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -o --format=newc 2>/dev/null | gzip -9
) > "$OUT_DIR/initramfs.cpio.gz"

echo "[5/6] copying user programs"
for app in $USER_APPS; do
    cp "$USER_BINARIES/$app" "$OUT_DIR/rootfs/$app"
done

echo "[6/6] creating FAT disk"
rm -f "$OUT_DIR/sdcard.img"
dd if=/dev/zero of="$OUT_DIR/sdcard.img" bs=1M count=72 status=none
mkfs.fat -F 32 "$OUT_DIR/sdcard.img" >/dev/null
mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/init" "::/init"
mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/bash" "::/bash"
printf 'alien isolation block read ok\n' > "$OUT_DIR/rootfs/hello.txt"
mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/hello.txt" "::/hello.txt"
for app in $USER_APPS; do
    mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR/rootfs/$app" "::$app"
done
if [ -n "$(find "$OUT_DIR/disk" -maxdepth 1 -name 'g*' -print -quit 2>/dev/null)" ]; then
    # The loader tools use /tests/domains/<name>, while the older tools use
    # /tests/<name>. Keep both layouts on the FAT root.
    mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR"/disk/g* "::/"
    mmd -i "$OUT_DIR/sdcard.img" ::/domains
    mcopy -i "$OUT_DIR/sdcard.img" "$OUT_DIR"/disk/g* "::/domains/"
fi

cat > "$OUT_DIR/build-info" <<EOF
SMP=$QEMU_SMP
LOG=$LOG
CRASH=$CRASH
INIT_DOMAINS=$(find "$OUT_DIR/init" -type f | wc -l)
DISK_DOMAINS=$(find "$OUT_DIR/disk" -type f | wc -l)
USER_PROGRAMS=$USER_APPS
EOF

echo
echo "built (SMP=$QEMU_SMP, LOG=$LOG):"
ls -lh "$OUT_DIR/kernel" "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/sdcard.img"
echo
echo "run:"
echo "  $ROOT/scripts/alien-isolation/run-qemu.sh --smp $QEMU_SMP --out $OUT_DIR"
