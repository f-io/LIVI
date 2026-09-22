#!/usr/bin/env bash
# Assemble the AX520 rootfs squashfs (the "rootfs" MTD) from our own builds:
# static busybox + hostapd from build-userspace.sh, livid and the AIC8800
# modules from build.sh, AIC8800 firmware from the pinned radxa checkout that
# build.sh already fetched for the driver.
set -euo pipefail

LOG_TAG=rootfs
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

MTD_SIZE=$((0x440000))  # 4456448 B, "rootfs" partition in ax520.dts
FW_SRC=$TOP/radxa-aic8800/src/SDIO/driver_fw/fw/aic8800D80
# What the driver actually request_firmware()s on this board (stock dmesg), plus
# the u04 patch pair and the two config texts, which are tiny.
FW_FILES="aic_powerlimit_8800d80.txt aic_userconfig_8800d80.txt
          fmacfw_8800d80_h_u02.bin fw_adid_8800d80_u02.bin
          fw_patch_8800d80_u02.bin fw_patch_8800d80_u02_ext0.bin fw_patch_8800d80_u04.bin
          fw_patch_table_8800d80_u02.bin fw_patch_table_8800d80_u04.bin"

need() { [[ -e $1 ]] || { log "missing $1 — $2"; exit 1; }; }
need "$USERSPACE/bin/busybox"       "run build-userspace.sh first"
need "$USERSPACE/usr/sbin/hostapd"  "run build-userspace.sh first"
need "$OUT/livid"                   "run build.sh first"
need "$OUT/modules/aic8800_bsp.ko"  "run build.sh first"
need "$OUT/modules/aic8800_fdrv.ko" "run build.sh first"
need "$FW_SRC"                      "run build.sh first (it fetches the firmware next to the driver)"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

log "assembling in $WORK"
mkdir -p "$WORK"/{bin,sbin,lib,usr/{bin,sbin},etc/init.d,dev,proc,sys,tmp,root,mnt}
ln -s /proc/mounts "$WORK/etc/mtab"

log "cp busybox + hostapd"
cp "$USERSPACE/bin/busybox"       "$WORK/bin/busybox"
cp "$USERSPACE/usr/sbin/hostapd"  "$WORK/usr/sbin/hostapd"

log "busybox applet symlinks"
BB_BIN_APPLETS="sh ash cat chmod cp dd df echo grep head ln ls mkdir more mount mv ps rm sed setsid stty sync tail touch umount"
BB_SBIN_APPLETS="brctl dmesg ifconfig init insmod killall mdev reboot rmmod route sysctl"
BB_USR_BIN_APPLETS="awk basename cut dirname env find hexdump id kill less md5sum nc netstat readlink pgrep pidof pkill seq sleep sort strings tee tr uname uniq wc which xargs xxd"
BB_USR_SBIN_APPLETS="chroot devmem flash_eraseall flashcp hostname httpd i2cdetect i2cdump i2cget i2cset nslookup telnetd udhcpc"
for a in $BB_BIN_APPLETS;       do ln -sf busybox        "$WORK/bin/$a";      done
for a in $BB_SBIN_APPLETS;      do ln -sf ../bin/busybox "$WORK/sbin/$a";     done
for a in $BB_USR_BIN_APPLETS;   do ln -sf ../../bin/busybox "$WORK/usr/bin/$a";  done
for a in $BB_USR_SBIN_APPLETS;  do ln -sf ../../bin/busybox "$WORK/usr/sbin/$a"; done

log "overlay repo rootfs (init, inittab, rcS, passwd, hostname, profile, hostapd.conf)"
cp -a "$HERE/rootfs-overlay/." "$WORK/"
chmod 755 "$WORK/init" "$WORK/etc/init.d/rcS"

log "flash tools from the initramfs (flash-mtd, sfc-sr), so a running system can be updated over USB-NCM"
cp "$HERE/initramfs/flash-mtd" "$HERE/initramfs/sfc-sr" "$WORK/usr/sbin/"
chmod 755 "$WORK/usr/sbin/flash-mtd" "$WORK/usr/sbin/sfc-sr"

log "livid + applet symlinks"
cp "$OUT/livid" "$WORK/usr/bin/livid"
for name in livi-tinyshell livi-netd livi-httpd livi-wifid livi-ledd; do
  ln -sf livid "$WORK/usr/bin/$name"
done

log "device-tree overlays (rcS switches sdio0 on after the WiFi enable)"
mkdir -p "$WORK/dtbo"
for o in "$HERE"/overlays/*.dtso; do
  n=$(basename "$o" .dtso)
  cp "$KDIR/arch/arm/boot/dts/axera/ax520-$n.dtbo" "$WORK/dtbo/"
done

log "AIC8800 modules"
mkdir -p "$WORK/lib/modules/$KVER"
cp "$OUT/modules"/aic8800_bsp.ko "$OUT/modules"/aic8800_fdrv.ko "$WORK/lib/modules/$KVER/"

log "AIC8800 firmware"
mkdir -p "$WORK/lib/firmware/aic8800d80"
for f in $FW_FILES; do
  [[ -f $FW_SRC/$f ]] || { log "firmware $f not in $FW_SRC"; exit 2; }
  cp "$FW_SRC/$f" "$WORK/lib/firmware/aic8800d80/$f"
done

log "size breakdown (uncompressed, KiB):"
{
  printf "  %6s  %s\n" "$(du -sk "$WORK/bin/busybox"              | cut -f1)" "bin/busybox"
  printf "  %6s  %s\n" "$(du -sk "$WORK/usr/sbin/hostapd"         | cut -f1)" "usr/sbin/hostapd"
  printf "  %6s  %s\n" "$(du -sk "$WORK/usr/bin/livid"            | cut -f1)" "usr/bin/livid"
  printf "  %6s  %s\n" "$(du -sk "$WORK/lib/modules"              | cut -f1)" "lib/modules"
  printf "  %6s  %s\n" "$(du -sk "$WORK/lib/firmware"             | cut -f1)" "lib/firmware"
  printf "  %6s  %s\n" "$(du -sk "$WORK"                          | cut -f1)" "TOTAL (uncompressed)"
}

IMG=$OUT/livi-link-ax520-rootfs.bin
rm -f "$IMG"
mksquashfs "$WORK" "$IMG" -comp xz -no-progress -all-root -noappend 2>&1 | tail -3

SIZE=$(stat -c%s "$IMG")
FREE=$((MTD_SIZE - SIZE))
log "rootfs: $SIZE B ($((SIZE/1024)) KiB), slot $MTD_SIZE B ($((MTD_SIZE/1024)) KiB), FREE $FREE B ($((FREE/1024)) KiB)"
[[ $SIZE -le $MTD_SIZE ]] || { log "OVERFLOW by $(( SIZE - MTD_SIZE )) B"; exit 3; }
md5sum "$IMG"
