#!/usr/bin/env bash
# Build the Alien isolation K3 diagnostic image.
#
# Options:
#   --smp N        number of harts (default: 1)
#   --out DIR      output directory (default: <alien>/build/k3/smp-<N>)
#   --kernel-base  kernel link address (default: 0x102200000)
#
# Environment variables are also accepted: K3_SMP, OUT_DIR, ALIEN_DIR,
# KERNEL_BASE.
#
# The diagnostic kernel stops after DTB, memory, paging and secondary harts
# are initialized. It does not start the domain set, which still assumes PLIC,
# virtio and the QEMU UART. The output contains the ELF, a gzipped raw binary
# and a U-Boot FIT image.

set -euo pipefail

usage() {
    sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
}

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ALIEN_DIR=${ALIEN_DIR:-$ROOT/alien-isolation}
K3_SMP=${K3_SMP:-1}
KERNEL_BASE=${KERNEL_BASE:-0x102200000}
OUT_DIR=${OUT_DIR:-}

while [ $# -gt 0 ]; do
    case "$1" in
        --smp)
            K3_SMP=$2
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
if [ ! -d "$ALIEN_DIR" ]; then
    echo "Alien isolation source not found: $ALIEN_DIR" >&2
    exit 2
fi
ALIEN_DIR=$(cd "$ALIEN_DIR" && pwd)
OUT_DIR=${OUT_DIR:-$ALIEN_DIR/build/k3/smp-$K3_SMP}
mkdir -p "$OUT_DIR"
OUT_DIR=$(cd "$OUT_DIR" && pwd)

for cmd in cargo rustc llvm-objcopy gzip cpio mkimage readelf; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "missing command: $cmd" >&2
        echo "run this script inside nix develop" >&2
        exit 2
    fi
done

echo "[1/4] building diagnostic kernel (SMP=$K3_SMP, base=$KERNEL_BASE)"
(
    cd "$ALIEN_DIR"
    PLATFORM=k3 SMP="$K3_SMP" KERNEL_BASE="$KERNEL_BASE" \
        RUSTFLAGS='--cfg getrandom_backend="custom"' \
        cargo build --release -p kernel --target ./tools/riscv64.json \
        -Zbuild-std=core,alloc \
        -Zbuild-std-features=compiler-builtins-mem \
        --features k3-diag
) > "$OUT_DIR/kernel.log" 2>&1
cp "$ALIEN_DIR/target/riscv64/release/kernel" "$OUT_DIR/kernel-k3"

echo "[2/4] creating diagnostic initramfs"
rm -rf "$OUT_DIR/initrd"
mkdir -p "$OUT_DIR/initrd/tests"
echo "K3 diagnostic initramfs" > "$OUT_DIR/initrd/tests/init"
(
    cd "$OUT_DIR/initrd"
    find . -print0 | cpio --null -o --format=newc 2>/dev/null | gzip -9
) > "$OUT_DIR/initramfs.cpio.gz"

echo "[3/4] creating raw binary and FIT"
llvm-objcopy --strip-all -O binary "$OUT_DIR/kernel-k3" "$OUT_DIR/kernel-k3.bin"
gzip -9 -c "$OUT_DIR/kernel-k3.bin" > "$OUT_DIR/kernel-k3.bin.gz"
BASE_HI=$(printf '0x%x' $((KERNEL_BASE >> 32)))
BASE_LO=$(printf '0x%x' $((KERNEL_BASE & 0xffffffff)))
cat > "$OUT_DIR/k3.its" <<ITS
/dts-v1/;

/ {
    description = "Alien isolation diagnostic image for SpacemiT K3";
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
mkimage -f "$OUT_DIR/k3.its" "$OUT_DIR/alien-k3-diag.itb" >/dev/null

echo "[4/4] checking addresses"
ENTRY_ADDR=$(readelf -h "$OUT_DIR/kernel-k3" | sed -n 's/.*Entry point address:[[:space:]]*//p')
if [ "$ENTRY_ADDR" != "$(printf '0x%x' "$KERNEL_BASE")" ]; then
    echo "unexpected entry point: $ENTRY_ADDR (expected $KERNEL_BASE)" >&2
    exit 1
fi

cat > "$OUT_DIR/build-info" <<EOF
PLATFORM=k3
MODE=diag
SMP=$K3_SMP
KERNEL_BASE=$KERNEL_BASE
ENTRY=$ENTRY_ADDR
EOF

echo
echo "built (SMP=$K3_SMP):"
ls -lh "$OUT_DIR/kernel-k3" "$OUT_DIR/kernel-k3.bin.gz" \
    "$OUT_DIR/initramfs.cpio.gz" "$OUT_DIR/alien-k3-diag.itb"
echo
echo "run on QEMU:"
echo "  $ROOT/scripts/alien-isolation/run-k3-qemu.sh --smp $K3_SMP --out $OUT_DIR"
echo
echo "U-Boot:"
echo "  tftpboot 0x140000000 alien-k3-diag.itb"
echo "  bootm 0x140000000 - \$fdtcontroladdr"
