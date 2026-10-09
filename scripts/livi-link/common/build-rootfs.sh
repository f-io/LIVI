#!/usr/bin/env bash
# Assemble a board's rootfs squashfs: the shared overlay (common/rootfs), busybox, hostapd and livid
# laid out the same way on every board, then what the board adds on top (<board dir>/rootfs and its
# rootfs_payload: runtime libraries, kernel modules, firmware).
#   build-rootfs.sh <board dir>
set -euo pipefail

COMMON=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
LOG_TAG=rootfs
source "$(cd "${1:?usage: build-rootfs.sh <board dir>}" && pwd)/board.sh"

need() { [[ -e $1 ]] || { log "missing $1 — $2"; exit 1; }; }
need "$BUSYBOX" "run the userspace build first"
need "$HOSTAPD" "run the userspace build first"
need "$LIVID"   "run the board's build.sh first"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

log "assembling in $WORK"
mkdir -p "$WORK"/{bin,sbin,lib,usr/{bin,sbin,lib},etc/init.d,dev,proc,sys,tmp,root,mnt}
ln -s /proc/mounts "$WORK/etc/mtab"

log "cp busybox + hostapd"
cp "$BUSYBOX" "$WORK/bin/busybox"
cp "$HOSTAPD" "$WORK/usr/sbin/hostapd"

log "busybox applet symlinks"
BB_BIN_APPLETS="sh ash cat chmod cp dd df echo grep head ln ls mkdir more mount mv ps rm sed setsid stty sync tail touch umount"
BB_SBIN_APPLETS="brctl dmesg ifconfig init insmod ip killall reboot rmmod"
BB_USR_BIN_APPLETS="awk basename cut dirname env find hexdump id kill less md5sum nc netstat readlink pgrep pidof pkill seq sha256sum sleep sort strings tee tr uname uniq wc which xargs xxd"
BB_USR_SBIN_APPLETS="devmem flash_eraseall flashcp hostname i2cdetect i2cdump i2cget i2cset telnetd"
for a in $BB_BIN_APPLETS;       do ln -sf busybox           "$WORK/bin/$a";      done
for a in $BB_SBIN_APPLETS;      do ln -sf ../bin/busybox    "$WORK/sbin/$a";     done
for a in $BB_USR_BIN_APPLETS;   do ln -sf ../../bin/busybox "$WORK/usr/bin/$a";  done
for a in $BB_USR_SBIN_APPLETS;  do ln -sf ../../bin/busybox "$WORK/usr/sbin/$a"; done

log "overlay: common/rootfs, then the board's rootfs (init, inittab, rcS, board.sh, ...)"
cp -a "$COMMON/rootfs/." "$WORK/"
[[ -d $HERE/rootfs ]] && cp -a "$HERE/rootfs/." "$WORK/"
chmod 755 "$WORK/init" "$WORK/etc/init.d/rcS" "$WORK/usr/bin/livi-radio"
need "$WORK/etc/livi/board.sh" "every board needs rootfs/etc/livi/board.sh"
# A syntax error there stops init or rcS before the ways in are up.
for s in init etc/init.d/rcS etc/livi/board.sh usr/bin/livi-radio; do
  sh -n "$WORK/$s" || { log "syntax error in /$s"; exit 4; }
done

log "livid + applet symlinks"
cp "$LIVID" "$WORK/usr/bin/livid"
for name in livi-netd livi-httpd livi-wifid livi-ledd livi-bt-up livi-mfid livi-btd livi-accessoryd; do
  ln -sf livid "$WORK/usr/bin/$name"
done

rootfs_payload "$WORK"

log "size breakdown (uncompressed, KiB):"
for p in bin/busybox usr/sbin/hostapd usr/bin/livid lib usr/lib; do
  printf "  %6s  %s\n" "$(du -sk "$WORK/$p" | cut -f1)" "$p"
done
printf "  %6s  %s\n" "$(du -sk "$WORK" | cut -f1)" "TOTAL (uncompressed)"

mkdir -p "$(dirname "$ROOTFS_IMG")"
rm -f "$ROOTFS_IMG"
# No xattrs: a build host with SELinux would otherwise put its labels into the image.
mksquashfs "$WORK" "$ROOTFS_IMG" -comp xz -no-progress -all-root -no-xattrs -noappend 2>&1 | tail -3

SIZE=$(stat -c%s "$ROOTFS_IMG")
FREE=$((ROOTFS_SIZE - SIZE))
log "rootfs: $SIZE B ($((SIZE/1024)) KiB), slot $ROOTFS_SIZE B ($((ROOTFS_SIZE/1024)) KiB), FREE $FREE B ($((FREE/1024)) KiB)"
[[ $SIZE -le $ROOTFS_SIZE ]] || { log "OVERFLOW by $(( SIZE - ROOTFS_SIZE )) B"; exit 3; }
md5sum "$ROOTFS_IMG"
