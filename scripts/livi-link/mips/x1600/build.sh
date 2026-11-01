#!/usr/bin/env bash
# LIVI-Link X1600+AIC8800D80 dongle kernel build.
# Order: ../build-userspace.sh <this dir>, build.sh, ../../common/build-rootfs.sh <this dir>,
# ../../common/pack-bundle.sh <this dir>.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
: "${KBUILD_BUILD_USER:=LIVI}"
: "${KBUILD_BUILD_HOST:=Link}"
export KBUILD_BUILD_USER KBUILD_BUILD_HOST

KERNEL_IMG=$OUT/livi-link-x1600-kernel.uimg

[[ -x $USERSPACE/rescue/busybox ]] || {
  log "no rescue/busybox at $USERSPACE, run build-userspace.sh first"
  exit 1
}
command -v "${CROSS_COMPILE}gcc" >/dev/null || {
  log "no ${CROSS_COMPILE}gcc in PATH"
  exit 2
}
"${CROSS_COMPILE}gcc" --version | head -1
command -v mkimage >/dev/null || {
  log "no mkimage in PATH (u-boot-tools)"
  exit 2
}
fetch_kernel

log "install the X1600 SoC sources (x1600.dtsi, CGU driver, USB PHY driver, DT bindings) from letux-kernel"
ingenic_x1600_install

log "apply kernel patches (X1600 entries 8250_ingenic.c and spidev.c don't carry yet)"
apply_patches "$HERE/kernel-patches" "$KDIR"

log "install the board DTS"
cp -f "$HERE/x1600-livi-link.dts" "$KDIR/arch/mips/boot/dts/ingenic/"

aic8800_install

INITRAMFS_LIST=$OUT/initramfs.list
initramfs_common "$USERSPACE/rescue/busybox" \
  | sed "s|$COMMON/initramfs/init|$HERE/initramfs/init|" >"$INITRAMFS_LIST"
check_initramfs_scripts "$INITRAMFS_LIST"

log "make allnoconfig"
cd "$KDIR"
make ARCH=mips allnoconfig >/dev/null

log "layer the LIVI X1600 config onto allnoconfig"
./scripts/config \
  --enable MIPS_GENERIC_KERNEL \
  --enable BOARD_INGENIC \
  --enable CPU_MIPS32_R2 \
  --enable CPU_MIPSR2_IRQ_EI \
  --enable CPU_MIPSR2_IRQ_VI \
  --enable CEVT_R4K \
  --enable CSRC_R4K \
  --enable MIPS_CPU_SCACHE \
  --enable SYS_HAS_CPU_MIPS32_R2 \
  --enable SYS_SUPPORTS_32BIT_KERNEL \
  --enable SYS_SUPPORTS_LITTLE_ENDIAN \
  --enable CPU_LITTLE_ENDIAN \
  --enable DMA_NONCOHERENT \
  --enable IRQ_MIPS_CPU \
  --enable USE_OF \
  --enable BOOT_RAW \
  --enable MIPS_AUTO_PFN_OFFSET \
  --disable MIPS_ELF_APPENDED_DTB \
  --enable MIPS_RAW_APPENDED_DTB \
  --enable PREEMPT \
  --enable CC_OPTIMIZE_FOR_SIZE \
  --enable KERNEL_GZIP \
  \
  --enable COMMON_CLK \
  --enable GENERIC_IRQ_CHIP \
  --enable PINCTRL \
  --enable PINCTRL_INGENIC \
  --enable GPIOLIB \
  --enable GPIO_SYSFS \
  \
  --enable MTD \
  --enable MTD_BLOCK \
  --enable MTD_CHAR \
  --enable MTD_OF_PARTS \
  --enable MTD_SPI_NOR \
  --disable MTD_SPI_NOR_USE_4K_SECTORS \
  --enable SPI \
  --enable SPI_MEM \
  --enable SPI_INGENIC \
  --enable SPI_SPIDEV \
  --enable I2C \
  --enable I2C_CHARDEV \
  --enable I2C_JZ4780 \
  --enable I2C_GPIO \
  --enable MISC_FILESYSTEMS \
  --enable SQUASHFS \
  --enable SQUASHFS_XZ \
  \
  --enable MMC \
  --enable MMC_JZ4740 \
  --enable PWRSEQ_SIMPLE \
  --enable REGULATOR \
  --enable REGULATOR_FIXED_VOLTAGE \
  \
  --enable TTY \
  --enable SERIAL_8250 \
  --enable SERIAL_8250_CONSOLE \
  --enable SERIAL_8250_INGENIC \
  --enable SERIAL_EARLYCON \
  --enable UNIX98_PTYS \
  --enable CMDLINE_FORCE \
  --set-str CMDLINE "console=ttyS2,3000000 earlycon rdinit=/init" \
  \
  --enable MODULES \
  --enable MODULE_UNLOAD \
  --enable EXPERT \
  --disable VT \
  --disable INPUT \
  --disable ETHERNET \
  \
  --enable NET \
  --enable INET \
  --enable PACKET \
  --enable UNIX \
  --enable BRIDGE \
  --disable BRIDGE_IGMP_SNOOPING \
  --enable NETDEVICES \
  --enable WIRELESS \
  --enable WLAN \
  --enable IPV6 \
  --disable IPV6_SIT \
  $(aic8800_config) \
  \
  --enable USB_SUPPORT \
  --enable GENERIC_PHY \
  --enable PHY_INGENIC_USB \
  --enable USB_DWC2 \
  --enable USB_DWC2_PERIPHERAL \
  --enable USB_GADGET \
  --enable USB_LIBCOMPOSITE \
  --enable USB_CONFIGFS \
  --enable USB_CONFIGFS_NCM \
  --enable USB_CONFIGFS_ACM \
  \
  --enable PRINTK \
  --enable PRINTK_TIME \
  --enable BUG \
  --enable FUTEX \
  --enable EPOLL \
  --enable EVENTFD \
  --enable SIGNALFD \
  --enable TIMERFD \
  --enable POSIX_TIMERS \
  --enable MULTIUSER \
  --enable FILE_LOCKING \
  --enable ADVISE_SYSCALLS \
  --enable SHMEM \
  --enable HIGH_RES_TIMERS \
  --enable NO_HZ_IDLE \
  --enable SYSVIPC \
  --enable INOTIFY_USER \
  --enable KALLSYMS \
  \
  --enable BLOCK \
  --enable BLK_DEV_WRITE_MOUNTED \
  --enable TMPFS \
  --enable DEVTMPFS \
  --enable CONFIGFS_FS \
  --enable PROC_FS \
  --enable PROC_SYSCTL \
  --enable SYSFS \
  --enable BLK_DEV_INITRD \
  --enable INITRAMFS_COMPRESSION_NONE \
  --set-str INITRAMFS_SOURCE "$INITRAMFS_LIST" \
  --enable BINFMT_ELF \
  --enable BINFMT_SCRIPT \
  --enable POWER_SUPPLY \
  --enable EXTCON \
  --enable DEBUG_KERNEL \
  --enable DEBUG_FS \
  --enable DEVMEM \
  --disable STRICT_DEVMEM \
  --enable MAGIC_SYSRQ \
  --set-val CONSOLE_LOGLEVEL_DEFAULT 8

make ARCH=mips olddefconfig

for s in MIPS_GENERIC_KERNEL BOARD_INGENIC CPU_MIPS32_R2 PINCTRL_INGENIC SPI_INGENIC SPI_SPIDEV MTD_SPI_NOR \
  MMC_JZ4740 SERIAL_8250_INGENIC SQUASHFS MIPS_RAW_APPENDED_DTB CFG80211 BT AIC_WLAN_SUPPORT \
  PHY_INGENIC_USB USB_DWC2 USB_CONFIGFS_NCM BRIDGE BLK_DEV_INITRD \
  CC_OPTIMIZE_FOR_SIZE SYSVIPC KALLSYMS DEVMEM IPV6 SYS_SUPPORTS_ZBOOT KERNEL_GZIP; do
  grep -q "^CONFIG_$s=y" .config || {
    log "CONFIG_$s did not end up =y"
    exit 3
  }
done

log "make vmlinux dtbs modules vmlinuz.bin (-j$JOBS)"
make ARCH=mips CROSS_COMPILE="$CROSS_COMPILE" -j"$JOBS" vmlinux dtbs modules vmlinuz.bin

VMLINUZ=$KDIR/vmlinuz
VMLINUZ_BIN=$KDIR/arch/mips/boot/vmlinuz.bin
DTB=$KDIR/arch/mips/boot/dts/ingenic/x1600-livi-link.dtb
[[ -f $VMLINUZ && -f $VMLINUZ_BIN && -f $DTB ]] || {
  log "build did not produce vmlinuz + DTB"
  exit 4
}

log "append the DTB (MIPS_RAW_APPENDED_DTB, also documented for vmlinuz.bin), wrap into a legacy uImage"
cat "$VMLINUZ_BIN" "$DTB" >"$OUT/vmlinuz-with-dtb.bin"
VMLINUZ_ADDR=$("${CROSS_COMPILE}readelf" -h "$VMLINUZ" | awk '/Entry point address:/{print $NF}')
[[ -n $VMLINUZ_ADDR ]] || {
  log "could not read vmlinuz's entry address"
  exit 4
}
mkimage -A mips -O linux -T kernel -C none -a "$VMLINUZ_ADDR" -e "$VMLINUZ_ADDR" \
  -n "LIVI-Link X1600" -d "$OUT/vmlinuz-with-dtb.bin" "$KERNEL_IMG"

KERNEL_SIZE=$(stat -c%s "$KERNEL_IMG")
KERNEL_SLOT=$((0x00400000))
log "kernel.uimg: $KERNEL_SIZE B, slot $KERNEL_SLOT B, free $((KERNEL_SLOT - KERNEL_SIZE)) B"
[[ $KERNEL_SIZE -le $KERNEL_SLOT ]] || {
  log "OVERFLOW by $((KERNEL_SIZE - KERNEL_SLOT)) B"
  exit 5
}

aic8800_collect
build_livid
