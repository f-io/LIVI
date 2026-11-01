# V821B + AIC8800D80: what the shared rootfs and bundle scripts (common/) take from this board.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BOARD=v821b
source "$HERE/../common.sh"
source "$COMMON/aic8800.sh"

BUSYBOX=$USERSPACE/bin/busybox
HOSTAPD=$USERSPACE/usr/sbin/hostapd
LIVID=$OUT/livid
ROOTFS_IMG=$OUT/livi-link-v821b-mtd3.bin
ROOTFS_SIZE=$((0x480000))  # mtd3, 4.5 MiB
# The MTD index: 1 = mtd1 (boot image), 3 = mtd3 (rootfs).
BUNDLE=$OUT/livi-link-v821b.lfwb
BUNDLE_IMAGES=("1:$OUT/livi-link-v821b-mtd1.bin" "3:$ROOTFS_IMG")

# busybox and hostapd link musl and libnl dynamically here.
rootfs_payload() {
  local work=$1
  log "musl runtime + libnl3 (from $USERSPACE)"
  cp    "$USERSPACE/lib/libc.so"              "$work/lib/libc.so"
  cp -P "$USERSPACE/lib/ld-musl-riscv32.so.1" "$work/lib/"
  cp -P "$USERSPACE/usr/lib"/libnl-3.so*       "$work/usr/lib/"
  cp -P "$USERSPACE/usr/lib"/libnl-genl-3.so*  "$work/usr/lib/"

  aic8800_rootfs "$work"
}
