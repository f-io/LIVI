# AX520 + AIC8800D80: what the shared rootfs and bundle scripts (common/) take from this board.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BOARD=ax520
source "$HERE/../common.sh"
source "$COMMON/aic8800.sh"

BUSYBOX=$USERSPACE/bin/busybox
HOSTAPD=$USERSPACE/usr/sbin/hostapd
LIVID=$OUT/livid
ROOTFS_IMG=$OUT/livi-link-ax520-rootfs.bin
ROOTFS_SIZE=$((0x440000))  # "rootfs" partition in ax520.dts
# The MTD index in ax520.dts: 3 = boot (kernel uImage), 6 = rootfs.
BUNDLE=$OUT/livi-link-ax520.lfwb
BUNDLE_IMAGES=("3:$OUT/livi-link-ax520-boot.uimg" "6:$ROOTFS_IMG")

rootfs_payload() {
  local work=$1 o n

  log "flash tools from the initramfs (flash-mtd, sfc-sr), so a running system can be updated over USB-NCM"
  cp "$HERE/initramfs/flash-mtd" "$HERE/initramfs/sfc-sr" "$work/usr/sbin/"
  chmod 755 "$work/usr/sbin/flash-mtd" "$work/usr/sbin/sfc-sr"

  log "device-tree overlays (board_wifi switches sdio0 on after the WiFi enable)"
  mkdir -p "$work/dtbo"
  for o in "$HERE"/overlays/*.dtso; do
    n=$(basename "$o" .dtso)
    cp "$KDIR/arch/arm/boot/dts/axera/ax520-$n.dtbo" "$work/dtbo/"
  done

  aic8800_rootfs "$work"
}
