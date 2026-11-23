# F133-B XFH head unit (C906, rv64) + AIC8800D40L, which the AIC8800D80 driver and firmware run.
BOARD=f133
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(cd "$HERE/../../../firmware" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
TOP=${TOP:-${XDG_CACHE_HOME:-$HOME/.cache}/livi/$BOARD-kernel}
JOBS=${JOBS:-$(nproc)}
CROSS_COMPILE=${CROSS_COMPILE:-riscv64-linux-gnu-}
US_ARCH=riscv
US_LINK=static
MUSL_TARGET=riscv64-linux
MUSL_WRAPPER=1
AIC_BT=0
source "$COMMON/kernel.sh"
source "$COMMON/aic8800.sh"
source "$HERE/g2d.sh"
source "$HERE/gst.sh"

# Fedora's cross gcc links -latomic_asneeded but ships no libatomic for the target. Rust links -ldl on
# musl and GStreamer -lm, -lpthread and -lrt, whose functions live in libc.a.
STUB=$TOP/host-stub
if [[ ! -e $STUB/librt.a ]]; then
  mkdir -p "$STUB"
  for lib in atomic_asneeded dl m pthread rt; do ${CROSS_COMPILE}ar rc "$STUB/lib$lib.a"; done
  printf '#!/bin/sh\nexec %sgcc "$@" -L"%s"\n' "$CROSS_COMPILE" "$STUB" >"$STUB/gcc"
  chmod +x "$STUB/gcc"
fi
export REALGCC=$STUB/gcc

BUSYBOX=$USERSPACE/bin/busybox
# Under memory pressure every page of code read back costs one block's decompression: zstd in 32K
# blocks a fraction of xz in 128K ones, 16K halves that again. lz4 would be cheaper still but does not
# fit the slot.
SQUASHFS_COMP="-comp zstd -Xcompression-level 19 -b 16K"
HOSTAPD=$USERSPACE/usr/sbin/hostapd
PRODUCT=$(cd "$HERE/../.." && pwd)
PRODUCT_APPLETS="livid livi-httpd livi-mdnsd livi-mfid"
LIVID=$OUT/livi
LIVID_APPLETS="LIVI livi-core livi-helperd livi-output"

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

# NOR layout (sun20i-f133b-xfh.dts): mtd0 boot0, mtd1 boot (the sunxi-package at 0x10000, where boot0
# looks), mtd2 rootfs, mtd3 data, mtd4 customer, mtd5 bootstate.
BOOT0IMG=$OUT/livi-os-f133-boot0.bin
BOOTIMG=$OUT/livi-os-f133-mtd1.bin
BOOT_SIZE=$((0x410000))
ROOTFS_IMG=$OUT/livi-os-f133-mtd2.bin
ROOTFS_SIZE=$((0xb40000))
BUNDLE=$OUT/livi-os-f133.lfwb
BUNDLE_TARGET=f133b_xfh
BUNDLE_IMAGES=("1:$BOOTIMG" "2:$ROOTFS_IMG")

rootfs_payload() {
  aic8800_rootfs "$1"
  mkdir -p "$1/mnt/data"
}
