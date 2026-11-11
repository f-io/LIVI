# V821B + AIC8800D80: what the shared rootfs and bundle scripts (common/) take from this board.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BOARD=v821b
source "$HERE/../common.sh"
source "$COMMON/aic8800.sh"

BUSYBOX=$USERSPACE/bin/busybox
HOSTAPD=$USERSPACE/usr/sbin/hostapd
LIVID=$OUT/livid

# The boot image has no room for the full busybox and musl next to it, so the initramfs gets a static
# one with only what the ways in, the switch to the rootfs and the provisioner use.
RESCUE_APPLETS="
  LFS BUSYBOX ASH SH_IS_ASH ASH_JOB_CONTROL ASH_ECHO ASH_PRINTF ASH_TEST ASH_CMDCMD ASH_OPTIMIZE_FOR_SIZE
  FEATURE_SH_MATH FEATURE_EDITING FEATURE_TAB_COMPLETION FEATURE_INSTALLER FEATURE_DEVPTS
  CTTYHACK SETSID STTY SWITCH_ROOT
  CAT CHMOD CP CUT DATE DD DF DIRNAME BASENAME DU ECHO ENV FALSE TRUE HEAD TAIL LN LS MKDIR MV
  OD PRINTF RM SEQ SLEEP SYNC TEST TEST1 TEST2 TOUCH TR UNAME UPTIME WC MD5SUM SHA256SUM
  GREP SED AWK HEXDUMP XXD VI
  PS KILL KILLALL PKILL PGREP PIDOF FREE
  MOUNT UMOUNT DMESG FLASHCP FLASH_ERASEALL DEVMEM HALT REBOOT POWEROFF
  IFCONFIG FEATURE_IFCONFIG_STATUS BRCTL FEATURE_BRCTL_FANCY FEATURE_BRCTL_SHOW NC NC_SERVER TELNETD
  FEATURE_TELNETD_STANDALONE UDHCPD PING
"

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
