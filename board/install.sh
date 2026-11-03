#!/bin/sh
# K3 Alpine incremental install: add OpenRC + services to the existing rootfs.
#
# Run on the board, after extracting the offline bundle to /root:
#
#   wget -O /root/k.tar.gz 'http://192.168.1.2:2999/share/sessions/<SID>/boot/k.tar.gz'
#   tar xzf /root/k.tar.gz -C /root
#   sh /root/offline/install.sh
#   sync
#   reboot -f
#
# The script refuses to run unless /etc/os-release says ID=alpine.

set -u
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

LOG=/root/k3-install.log
: > "$LOG"
log() { echo "$*" | tee -a "$LOG"; }
die() { log "ERROR: $*"; exit 1; }

# shellcheck disable=SC1091
[ -f /etc/os-release ] && . /etc/os-release
[ "${ID:-}" = "alpine" ] || die "not Alpine (ID=${ID:-unknown})"

mount -o remount,rw / 2>/dev/null || true
dmesg -n 1 2>/dev/null || true

[ -f /root/etc-backup.tar.gz ] || tar czf /root/etc-backup.tar.gz /etc 2>/dev/null || true

REPO=/root/offline/apks
[ -f "$REPO/riscv64/APKINDEX.tar.gz" ] || die "missing $REPO/riscv64/APKINDEX.tar.gz"

if [ -f /etc/apk/repositories ]; then
    cp /etc/apk/repositories /root/repositories.orig
fi

# Use only the local bundle while installing. Keep the original mirrors
# commented out so they can be re-enabled when the board gets internet.
{
    printf '%s\n' "$REPO"
    if [ -f /root/repositories.orig ]; then
        sed 's/^/# /' /root/repositories.orig
    fi
} > /etc/apk/repositories

log "== apk update =="
apk update || die "apk update failed"

log "== apk add =="
apk add \
    alpine-base openrc ifupdown-ng busybox-openrc busybox-mdev-openrc \
    openssh openssh-server-common-openrc openssh-keygen \
    dhcpcd dhcpcd-openrc chrony chrony-openrc \
    e2fsprogs e2fsprogs-extra doas ca-certificates-bundle tzdata wget \
    || die "apk add failed"

# Write inittab after packages: alpine-baselayout may ship its own copy.
[ -f /etc/inittab ] && cp -a /etc/inittab /etc/inittab.pre-k3 2>/dev/null || true
cat > /etc/inittab <<'EOF'
# /etc/inittab
::sysinit:/bin/mount -o remount,rw /
::sysinit:/bin/dmesg -n 1
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6

ttyS0::respawn:/sbin/getty -L 115200 ttyS0 vt100

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOF

echo k3-alpine > /etc/hostname
grep -q '^127.0.0.1[[:space:]].*k3-alpine' /etc/hosts 2>/dev/null || \
    echo '127.0.0.1 localhost k3-alpine' >> /etc/hosts

if [ -s /root/offline/root.hash ]; then
    HASH=$(cat /root/offline/root.hash)
    awk -F: -v h="$HASH" 'BEGIN{OFS=":"} $1=="root"{$2=h} {print}' \
        /etc/shadow > /etc/shadow.new
    cat /etc/shadow.new > /etc/shadow
    rm -f /etc/shadow.new
    chmod 640 /etc/shadow
    log "root password set"
fi

ssh-keygen -A >/dev/null 2>&1 || true
if [ -s /root/offline/authorized_keys ]; then
    mkdir -p /root/.ssh
    cp /root/offline/authorized_keys /root/.ssh/authorized_keys
    chmod 700 /root/.ssh
    chmod 600 /root/.ssh/authorized_keys
    log "authorized_keys installed"
fi

if [ -f /etc/ssh/sshd_config ]; then
    sed -i 's/^#*PermitRootLogin.*/PermitRootLogin prohibit-password/' /etc/ssh/sshd_config
    grep -q '^PermitRootLogin' /etc/ssh/sshd_config || \
        echo 'PermitRootLogin prohibit-password' >> /etc/ssh/sshd_config
fi

if [ -f /etc/securetty ] && ! grep -qx 'ttyS0' /etc/securetty; then
    echo ttyS0 >> /etc/securetty
fi

mkdir -p /etc/runlevels/sysinit /etc/runlevels/boot \
    /etc/runlevels/default /etc/runlevels/shutdown

addrun() {
    svc="$1"
    level="$2"
    [ -x "/etc/init.d/$svc" ] || return 0
    rc-update add "$svc" "$level" >/dev/null 2>&1 || true
}

for s in devfs procfs sysfs dmesg mdev hwdrivers modules; do
    addrun "$s" sysinit
done
for s in bootmisc hostname hwclock root swap sysctl syslog urandom seedrng mtab; do
    addrun "$s" boot
done
for s in dhcpcd sshd chronyd crond local; do
    addrun "$s" default
done
for s in killprocs mount-ro savecache; do
    addrun "$s" shutdown
done

sync
log "== INSTALL_DONE =="
log "log: $LOG"
