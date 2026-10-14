# Ingenic X1600 + AIC8800D80: what the shared rootfs and bundle scripts (common/) take from this
# board. Dual bank, the vendor's own: the existing bootloader stays exactly as it is and already
# reads "ota" to pick which bank to boot, so flashing goes through livi-link-provision's own
# flash() (the same vendor HTTP bank-flip install-shell uses), not a raw MTD write.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
BOARD=x1600
source "$HERE/../common.sh"
source "$COMMON/aic8800.sh"
source "$COMMON/ingenic-x1600.sh"

BUSYBOX=$USERSPACE/bin/busybox
HOSTAPD=$USERSPACE/usr/sbin/hostapd
LIVID=$OUT/livid
ROOTFS_IMG=$OUT/livi-link-x1600-rootfs.squashfs
# The "rootfs"/"rootfs2" partition in x1600::MTDS (livi-link-provision): 16 MiB, a real /proc/mtd
# off real hardware (see x1600-livi-link.dts for where that is confirmed, not read off the code).
ROOTFS_SIZE=$((0x01000000))
# Type 0/1, not an MTD index: there is no single MTD index for "the kernel", it alternates between
# mtd1 and mtd3 by bank. x1600::flash_lfwb_bytes reads these two out and builds the vendor's own
# OTA container around them, picking the inactive bank itself.
BUNDLE=$OUT/livi-link-x1600.lfwb
BUNDLE_IMAGES=("0:$OUT/livi-link-x1600-kernel.uimg" "1:$ROOTFS_IMG")

# The "kernel"/"kernel2" partition is only 4 MiB, with the initramfs (rootfs discovery + USB-NCM
# rescue shell) baked in alongside vmlinux itself, so the rescue busybox gets the same trim
# imx6ul's does rather than the full one ax520 can afford.
RESCUE_APPLETS="
  LFS BUSYBOX ASH SH_IS_ASH ASH_JOB_CONTROL ASH_ECHO ASH_PRINTF ASH_TEST ASH_CMDCMD ASH_OPTIMIZE_FOR_SIZE
  FEATURE_SH_MATH FEATURE_EDITING FEATURE_TAB_COMPLETION FEATURE_INSTALLER FEATURE_DEVPTS
  CTTYHACK SETSID STTY SWITCH_ROOT
  CAT CHMOD CP CUT DATE DD DF DIRNAME BASENAME DU ECHO ENV FALSE TRUE HEAD TAIL LN LS MKDIR MV
  OD PRINTF RM SEQ SLEEP SYNC TEST TEST1 TEST2 TOUCH TR UNAME UPTIME WC MD5SUM SHA256SUM
  GREP SED AWK HEXDUMP XXD VI
  PS KILL KILLALL PKILL PGREP PIDOF FREE
  MOUNT UMOUNT DMESG FLASHCP FLASH_ERASEALL DEVMEM HALT REBOOT POWEROFF RX
  IFCONFIG FEATURE_IFCONFIG_STATUS BRCTL FEATURE_BRCTL_FANCY FEATURE_BRCTL_SHOW NC NC_SERVER TELNETD
  FEATURE_TELNETD_STANDALONE UDHCPD PING
"

# busybox, hostapd and libnl are all statically linked (mips/build-userspace.sh),
rootfs_payload() {
  local work=$1
  aic8800_rootfs "$work"
}
