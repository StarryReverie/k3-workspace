#!/usr/bin/env bash
# Build an Alien isolation image for SpacemiT K3.
#
# Options:
#   --smp N        number of harts (default: 1)
#   --mode MODE    diag or shell (default: diag)
#   --out DIR      output directory (default: <alien>/build/k3[-shell]/smp-<N>)
#   --kernel-base  kernel link address (default: 0x102200000)
#
# Environment variables are also accepted: K3_SMP, K3_MODE, OUT_DIR,
# ALIEN_DIR, KERNEL_BASE.
#
# diag:  TCB only, no domains, no userspace. Outputs K3_BOOT_OK.
# shell: builds K3 domains, user init and bash, and boots to an interactive
#        polling UART shell. External interrupts are not enabled yet.

set -euo pipefail

usage() {
    sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
K3_SMP=${K3_SMP:-1}
K3_MODE=${K3_MODE:-diag}
KERNEL_BASE=${KERNEL_BASE:-0x102200000}
OUT_DIR=${OUT_DIR:-}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)
            K3_SMP=$2
            shift 2
            ;;
        --mode)
            K3_MODE=$2
            shift 2
            ;;
        --out)
            OUT_DIR=$2
            shift 2
            ;;
        --kernel-base)
            KERNEL_BASE=$2
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

if ! [[ "$K3_SMP" =~ ^[1-9][0-9]*$ ]]; then
    echo "SMP must be a positive integer: $K3_SMP" >&2
    exit 2
fi
case "$K3_MODE" in
    diag|shell) ;;
    *)
        echo "mode must be diag or shell: $K3_MODE" >&2
        exit 2
        ;;
esac
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien isolation source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
if [ -z "$OUT_DIR" ]; then
    if [ "$K3_MODE" = "shell" ]; then
        OUT_DIR="$ALIEN_DIR/build/k3-shell/smp-$K3_SMP"
    else
        OUT_DIR="$ALIEN_DIR/build/k3/smp-$K3_SMP"
    fi
fi
mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)

for cmd in cargo rustc llvm-objcopy gzip cpio mkimage readelf; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        echo "run this script inside nix develop" >&2
        exit 2
    fi
done

TARGET=riscv64gc-unknown-none-elf
USER_BINARIES="$ALIEN_DIR/target/$TARGET/release"

if [ "$K3_MODE" = "shell" ]; then
    echo "[1/6] building K3 domains"
    for workspace in common/vfs common/vfs2; do
        if [ ! -f "$ALIEN_DIR/domains/$workspace/Cargo.lock" ]; then
            cp "$ALIEN_DIR/Cargo.lock" "$ALIEN_DIR/domains/$workspace/Cargo.lock"
        fi
    done
    rm -rf "$OUT_DIR/domains"
    (
        cd "$ALIEN_DIR/domains"
        PLATFORM=k3 CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback \
            LOG=WARN cargo domain build-all -l WARN -o "$OUT_DIR/domains"
    ) > "$OUT_DIR/domains.log" 2>&1
    python3 - "$ALIEN_DIR" "$OUT_DIR" <<'PY'
import pathlib
import sys
import tomllib
alien = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2]) / "domains"
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
PY
else
    echo "[1/6] diagnostic mode: skip domains"
fi

if [ "$K3_MODE" = "shell" ]; then
    echo "[2/6] building user init"
    (
        cd "$ALIEN_DIR"
        RUSTFLAGS="-C link-arg=-T$ALIEN_DIR/user/apps/linker.ld" \
            cargo build --release -p init --target "$TARGET"
    ) > "$OUT_DIR/init.log" 2>&1
else
    echo "[2/6] diagnostic mode: skip user init"
fi

echo "[3/6] creating initramfs"
rm -rf "$OUT_DIR/initrd"
if [ "$K3_MODE" = "diag" ]; then
    mkdir -p "$OUT_DIR/initrd/tests"
    echo "K3 diagnostic initramfs" > "$OUT_DIR/initrd/tests/init"
else
    mkdir -p "$OUT_DIR/initrd/bin" "$OUT_DIR/initrd/tests"
    cp "$OUT_DIR"/domains/init/g* "$OUT_DIR/initrd/"
    # loopback supplies the NIC that net_stack and vfs expect.
    cp "$OUT_DIR/domains/disk/gloopback" "$OUT_DIR/initrd/"
    cp "$USER_BINARIES/init" "$OUT_DIR/initrd/tests/init"
    cp "$ALIEN_DIR/user/bin/bash" "$OUT_DIR/initrd/bin/bash"
    ln -sf bash "$OUT_DIR/initrd/bin/sh"
fi
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -o --format=newc 2>/dev/null | gzip -9
) > "$OUT_DIR/initramfs.cpio.gz"

echo "[4/6] building kernel (MODE=$K3_MODE, SMP=$K3_SMP, base=$KERNEL_BASE)"
FEATURES=()
if [ "$K3_MODE" = "diag" ]; then
    FEATURES=(--features k3-diag)
else
    # The U-Boot control DTB does not reliably carry a ramdisk, so the shell
    # image embeds the initramfs in the kernel.
    mkdir -p "$ALIEN_DIR/build"
    cp "$OUT_DIR/initramfs.cpio.gz" "$ALIEN_DIR/build/k3-initramfs.cpio.gz"
    FEATURES=(--features k3-initrd)
fi
(
    cd "$ALIEN_DIR"
    PLATFORM=k3 SMP="$K3_SMP" KERNEL_BASE="$KERNEL_BASE" \
        RUSTFLAGS='--cfg getrandom_backend="custom"' \
        cargo build --release -p kernel --target ./tools/riscv64.json \
        -Zbuild-std=core,alloc \
        -Zbuild-std-features=compiler-builtins-mem \
        "${FEATURES[@]}"
) > "$OUT_DIR/kernel.log" 2>&1
cp "$ALIEN_DIR/target/riscv64/release/kernel" "$OUT_DIR/kernel-k3"

echo "[5/6] creating raw binary and FIT"
llvm-objcopy --strip-all -O binary "$OUT_DIR/kernel-k3" "$OUT_DIR/kernel-k3.bin"
gzip -9 -c "$OUT_DIR/kernel-k3.bin" > "$OUT_DIR/kernel-k3.bin.gz"
BASE_HI=$(printf '0x%x' $((KERNEL_BASE >> 32)))
BASE_LO=$(printf '0x%x' $((KERNEL_BASE & 0xffffffff)))
cat > "$OUT_DIR/k3.its" <<ITS
/dts-v1/;

/ {
    description = "Alien isolation ${K3_MODE} image for SpacemiT K3";
    #address-cells = <2>;

    images {
        kernel {
            description = "Alien isolation kernel for SpacemiT K3";
            data = /incbin/("kernel-k3.bin.gz");
            type = "kernel";
            arch = "riscv";
            os = "linux";
            compression = "gzip";
            load = <$BASE_HI $BASE_LO>;
            entry = <$BASE_HI $BASE_LO>;
        };
    };

    configurations {
        default = "conf";
        conf {
            description = "Boot Alien isolation on SpacemiT K3";
            kernel = "kernel";
        };
    };
};
ITS
FIT_NAME=alien-k3-${K3_MODE}.itb
mkimage -f "$OUT_DIR/k3.its" "$OUT_DIR/$FIT_NAME" >/dev/null

echo "[6/6] checking addresses"
ENTRY_ADDR=$(readelf -h "$OUT_DIR/kernel-k3" | sed -n 's/.*Entry point address:[[:space:]]*//p')
if [ "$ENTRY_ADDR" != "$(printf '0x%x' "$KERNEL_BASE")" ]; then
    echo "unexpected entry point: $ENTRY_ADDR (expected $KERNEL_BASE)" >&2
    exit 1
fi

cat > "$OUT_DIR/build-info" <<EOF
PLATFORM=k3
MODE=$K3_MODE
SMP=$K3_SMP
KERNEL_BASE=$KERNEL_BASE
ENTRY=$ENTRY_ADDR
EOF

echo
echo "built (MODE=$K3_MODE, SMP=$K3_SMP):"
ls -lh "$OUT_DIR/kernel-k3" "$OUT_DIR/kernel-k3.bin.gz" \
    "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/$FIT_NAME"
echo
echo "run on QEMU:"
if [ "$K3_MODE" = "diag" ]; then
    echo "  $ROOT/scripts/alien-isolation/run-k3-qemu.sh --smp $K3_SMP --out $OUT_DIR"
else
    echo "  $ROOT/scripts/alien-isolation/run-k3-shell-qemu.sh --smp $K3_SMP --out $OUT_DIR"
fi
echo
echo "U-Boot:"
echo "  tftpboot 0x140000000 $FIT_NAME"
echo "  bootm 0x140000000 - \$fdtcontroladdr"
