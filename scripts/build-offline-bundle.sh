#!/usr/bin/env bash
# Build an offline Alpine bundle for the K3 board.
#
# Usage:
#   K3_ROOT_PASSWORD='<password>' ./scripts/build-offline-bundle.sh
#
# Optional:
#   K3_AUTHORIZED_KEYS=/path/to/authorized_keys
#   K3_ALPINE_VERSION=3.23.7
#   K3_ALPINE_SHA256=<sha256 of alpine-uboot-<version>-riscv64.tar.gz>
#
# The result is dist/k3-offline.tar.gz. It stays below the platform's
# 64 MiB per-file upload limit.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
dist="$root/dist"
cache="$dist/.cache"
work="$dist/.work"

version="${K3_ALPINE_VERSION:-3.23.7}"
url="https://dl-cdn.alpinelinux.org/alpine/v3.23/releases/riscv64/alpine-uboot-${version}-riscv64.tar.gz"
sha="${K3_ALPINE_SHA256:-c45287adaa0effde4d357abc04569184b46c02a4f7e056bfa2b6b9a6d1874250}"

if [[ -z "${K3_ROOT_PASSWORD:-}" ]]; then
  echo "error: set K3_ROOT_PASSWORD for the board's root account" >&2
  exit 2
fi
if ! command -v openssl >/dev/null 2>&1; then
  echo "error: openssl not found (needed for the \$6\$ password hash)" >&2
  exit 2
fi

mkdir -p "$dist" "$cache"
tarball="$cache/alpine-uboot-${version}-riscv64.tar.gz"
if [[ ! -f "$tarball" ]]; then
  echo "downloading $url"
  curl -fL --retry 3 --connect-timeout 20 -o "$tarball.part" "$url"
  mv "$tarball.part" "$tarball"
fi
printf '%s  %s\n' "$sha" "$tarball" | sha256sum -c -

rm -rf "$work"
mkdir -p "$work/offline"
echo "extracting apks"
tar xzf "$tarball" -C "$work/offline" ./apks

cp "$root/board/install.sh" "$work/offline/install.sh"
chmod +x "$work/offline/install.sh"
openssl passwd -6 "$K3_ROOT_PASSWORD" > "$work/offline/root.hash"

if [[ -n "${K3_AUTHORIZED_KEYS:-}" ]]; then
  if [[ ! -f "$K3_AUTHORIZED_KEYS" ]]; then
    echo "error: no such file: $K3_AUTHORIZED_KEYS" >&2
    exit 2
  fi
  cp "$K3_AUTHORIZED_KEYS" "$work/offline/authorized_keys"
fi

cat > "$work/offline/README.txt" <<'EOF'
K3 Alpine offline bundle

1. Upload dist/k3-offline.tar.gz to the board session as boot/k.tar.gz.
2. On the board (init=/bin/sh):

   stty echo icanon
   dmesg -n 1
   wget -O /root/k.tar.gz 'http://192.168.1.2:2999/share/sessions/<SID>/boot/k.tar.gz'
   tar xzf /root/k.tar.gz -C /root
   sh /root/offline/install.sh
   sync
   reboot -f

3. Reconnect without interrupting autoboot; log in on ttyS0 as root.
EOF

out="$dist/k3-offline.tar.gz"
echo "creating $out"
tar czf "$out" -C "$work" offline
rm -rf "$work"

size="$(stat -c %s "$out")"
limit=$((64 * 1024 * 1024))
if (( size > limit )); then
  echo "error: bundle is $size bytes, above the 64 MiB upload limit" >&2
  exit 1
fi

echo
echo "bundle : $out"
echo "size   : $((size / 1024 / 1024)) MiB ($size bytes)"
sha256sum "$out"
