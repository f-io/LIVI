#!/usr/bin/env bash
# Order: build-userspace.sh, build.sh, ../../common/build-rootfs.sh <this dir>,
# ../../common/pack-bundle.sh <this dir>.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
: ${KBUILD_BUILD_USER:=LIVI}
: ${KBUILD_BUILD_HOST:=Link}
export KBUILD_BUILD_USER KBUILD_BUILD_HOST

BOOTIMG=$OUT/livi-link-v821b-mtd1.bin

[[ -x $USERSPACE/rescue/busybox ]] || { log "no rescue/busybox at $USERSPACE, run build-userspace.sh first"; exit 1; }
fetch_kernel

log "apply kernel patches (Andes MMU erratum and cache, V821 platform and driver hooks, MMC, USB PHY and musb, spidev for the LED, uncached DMA alias, awbase heartbeat, console without input)"
apply_patches "$HERE/kernel-patches" "$KDIR"

log "install the V821 sources (clocks, pins, flash controller, SoC dtsi) and the board DTS"
cp -r "$HERE/kernel/." "$KDIR/"
DTS_DIR=$KDIR/arch/riscv/boot/dts/allwinner
cp -f "$HERE/sun300i-v821b-livi-link.dts" "$DTS_DIR/"
grep -q 'sun300i-v821b-livi-link.dtb' "$DTS_DIR/Makefile" \
  || echo 'dtb-$(CONFIG_ARCH_SUNXI) += sun300i-v821b-livi-link.dtb' >> "$DTS_DIR/Makefile"

aic8800_install

INITRAMFS_LIST=$OUT/initramfs.list
initramfs_common "$USERSPACE/rescue/busybox" > "$INITRAMFS_LIST"
check_initramfs_scripts "$INITRAMFS_LIST"

log "make allnoconfig"
cd "$KDIR"
make ARCH=riscv allnoconfig >/dev/null

# The console is SBI since OpenSBI owns uart0. clk_ignore_unused keeps its clock running, and
# noinput stops a UART that hears its own TX from feeding every line back.
log "layer the LIVI V821B config onto allnoconfig"
./scripts/config \
  --enable NONPORTABLE \
  --enable ARCH_RV32I \
  --disable ARCH_RV64I \
  --enable MMU \
  --enable ARCH_SUNXI \
  --disable ERRATA_THEAD_CMO \
  --enable ARCH_SUNXI_V821 \
  --disable SUN20I_D1_CCU \
  --disable SUN20I_D1_R_CCU \
  --disable PINCTRL_SUN20I_D1 \
  --disable SUN6I_RTC_CCU \
  --disable SUN8I_DE2_CCU \
  --disable SUNXI_SRAM \
  --enable ERRATA_ANDES \
  --enable ANDES_CACHE \
  --enable FPU \
  --enable RISCV_ISA_C \
  --disable RISCV_ISA_ZICBOM \
  --disable RISCV_ISA_ZICBOZ \
  --disable RISCV_ISA_V \
  --disable RISCV_ISA_ZAWRS \
  --disable RISCV_ISA_ZBA \
  --disable RISCV_ISA_ZBB \
  --disable RISCV_ISA_ZBC \
  --disable RISCV_ISA_ZBKB \
  --disable SMP \
  --enable PREEMPT \
  --enable CC_OPTIMIZE_FOR_SIZE \
  --enable RISCV_SBI \
  --enable RISCV_SBI_V01 \
  --enable HVC_RISCV_SBI \
  --enable SERIAL_EARLYCON_RISCV_SBI \
  --enable CMDLINE_FORCE \
  --set-str CMDLINE "earlycon=sbi console=hvc0 hvc_riscv_sbi.noinput=1 clk_ignore_unused rdinit=/init" \
  --enable MODULES \
  --enable MODULE_UNLOAD \
  --enable EXPERT \
  --disable IO_URING \
  --disable VT \
  --disable INPUT \
  --disable ETHERNET \
  --disable NET_NS \
  --disable ETHTOOL_NETLINK \
  --disable NET_RX_BUSY_POLL \
  --disable NET_PTP_CLASSIFY \
  --enable IPV6 \
  --disable IPV6_SIT \
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
  --enable BINFMT_ELF \
  --enable BINFMT_SCRIPT \
  --enable PROC_FS \
  --enable PROC_SYSCTL \
  --enable SYSFS \
  --enable TMPFS \
  --enable DEVTMPFS \
  --enable CONFIGFS_FS \
  --enable BLK_DEV_INITRD \
  --enable INITRAMFS_COMPRESSION_NONE \
  --set-str INITRAMFS_SOURCE "$INITRAMFS_LIST" \
  \
  --enable TTY \
  --enable UNIX98_PTYS \
  --enable SERIAL_EARLYCON \
  \
  --enable COMMON_CLK \
  --enable SUN300I_V821_CCU \
  --enable SUN300I_V821_AON_CCU \
  --enable RESET_CONTROLLER \
  --enable PINCTRL \
  --enable PINCTRL_SUN300I_V821B \
  --enable GPIOLIB \
  --enable GPIO_SYSFS \
  --enable REGULATOR \
  --enable REGULATOR_FIXED_VOLTAGE \
  \
  --enable MTD \
  --enable MTD_BLOCK \
  --enable MTD_CHAR \
  --enable MTD_OF_PARTS \
  --enable MTD_SPI_NOR \
  --disable MTD_SPI_NOR_USE_4K_SECTORS \
  --enable SPI \
  --enable SPI_MEM \
  --enable SPI_SUN300I_SPIF \
  --enable SPI_SUN6I \
  --enable SPI_SPIDEV \
  --enable I2C \
  --enable I2C_CHARDEV \
  --enable I2C_MV64XXX \
  --enable MISC_FILESYSTEMS \
  --enable SQUASHFS \
  --enable SQUASHFS_XZ \
  \
  --enable MMC \
  --enable MMC_SUNXI \
  --enable PWRSEQ_SIMPLE \
  \
  --enable POWER_SUPPLY \
  --enable EXTCON \
  --enable USB_SUPPORT \
  --enable USB_PHY \
  --enable NOP_USB_XCEIV \
  --enable GENERIC_PHY \
  --enable PHY_SUN4I_USB \
  --enable USB_MUSB_HDRC \
  --enable USB_MUSB_GADGET \
  --enable USB_MUSB_SUNXI \
  --enable USB_GADGET \
  --enable USB_LIBCOMPOSITE \
  --enable USB_CONFIGFS \
  --enable USB_CONFIGFS_NCM \
  --enable USB_CONFIGFS_ACM \
  \
  --enable NET \
  --enable INET \
  --enable PACKET \
  --enable UNIX \
  --enable BRIDGE \
  --disable BRIDGE_IGMP_SNOOPING \
  --enable NETDEVICES \
  $(aic8800_config) \
  \
  --enable DEBUG_KERNEL \
  --enable DEBUG_FS \
  --enable DEVMEM \
  --disable STRICT_DEVMEM \
  --enable MAGIC_SYSRQ \
  --set-val CONSOLE_LOGLEVEL_DEFAULT 8

make ARCH=riscv CROSS_COMPILE="$CROSS_COMPILE" olddefconfig

# A symbol with an unmet dependency is dropped silently, so the ones this board needs are checked.
for s in ARCH_RV32I ARCH_SUNXI_V821 ERRATA_ANDES ANDES_CACHE RISCV_DMA_NONCOHERENT RISCV_TIMER HVC_RISCV_SBI SUN300I_AWBASE \
         SUN300I_V821_CCU SUN300I_V821_AON_CCU PINCTRL_SUN300I_V821B \
         SPI_SUN300I_SPIF MTD_SPI_NOR MTD_OF_PARTS SQUASHFS MMC_SUNXI PHY_SUN4I_USB \
         USB_MUSB_SUNXI USB_CONFIGFS_NCM SPI_SUN6I SPI_SPIDEV I2C_MV64XXX BLK_DEV_INITRD \
         CFG80211 BT AIC_WLAN_SUPPORT; do
  grep -q "^CONFIG_$s=y" .config || { log "CONFIG_$s did not end up =y"; exit 3; }
done

log "make Image dtbs modules (-j$JOBS)"
make ARCH=riscv CROSS_COMPILE="$CROSS_COMPILE" -j"$JOBS" Image dtbs modules

IMG=$KDIR/arch/riscv/boot/Image
DTB=$DTS_DIR/sun300i-v821b-livi-link.dtb
[[ -f $IMG && -f $DTB ]] || { log "build did not produce Image + DTB"; exit 4; }
log "Image: $(stat -c%s "$IMG") B   DTB: $(stat -c%s "$DTB") B"

aic8800_collect

log "wrap kernel + DTB into the boot image for mtd1"
NATIVE=$REPO/native
( cd "$NATIVE" && cargo build --release -p mkbootimg-v821b )
"${CARGO_TARGET_DIR:-$NATIVE/target}/release/mkbootimg-v821b" "$IMG" "$DTB" "$BOOTIMG"
log "done: $BOOTIMG"

build_livid
