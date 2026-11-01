#!/usr/bin/env bash
# LIVI-Link AX520+AIC8800D80 dongle kernel build.
# Order: ../build-userspace.sh <this dir>, build.sh, ../../common/build-rootfs.sh <this dir>,
# ../../common/pack-bundle.sh <this dir>.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
: ${KBUILD_BUILD_USER:=LIVI}
: ${KBUILD_BUILD_HOST:=Link}
export KBUILD_BUILD_USER KBUILD_BUILD_HOST

[[ -x $USERSPACE/bin/busybox ]] || { log "no busybox at $USERSPACE — run build-userspace.sh first"; exit 1; }
fetch_kernel

# ---------------------------------------------------------------------------
# 1) Our own Axera AX520 platform glue (mainline has no SoC-specific driver
#    for it — none is needed: single Cortex-A7, real ARM GIC, stock DW IP,
#    all handled by the generic ARM multiplatform boot path).
# ---------------------------------------------------------------------------
log "install mach-axera (idempotent)"
mkdir -p "$KDIR/arch/arm/mach-axera"
cp -f "$HERE/mach.c"       "$KDIR/arch/arm/mach-axera/axera.c"
cp -f "$HERE/mach.Kconfig" "$KDIR/arch/arm/mach-axera/Kconfig"
cat > "$KDIR/arch/arm/mach-axera/Makefile" <<'EOF'
# SPDX-License-Identifier: GPL-2.0-only
obj-$(CONFIG_ARCH_AXERA)	+= axera.o
EOF
grep -q 'mach-axera/Kconfig' "$KDIR/arch/arm/Kconfig" || \
  sed -i '/^source "arch\/arm\/mach-at91\/Kconfig"/a\
\
source "arch/arm/mach-axera/Kconfig"' "$KDIR/arch/arm/Kconfig"
# Without this line arch/arm/Makefile never descends into mach-axera, axera.c is not built, and the
# kernel still boots (as the generic DT machine), which hides that our time setup is missing.
grep -q 'CONFIG_ARCH_AXERA' "$KDIR/arch/arm/Makefile" || \
  sed -i '/^machine-\$(CONFIG_ARCH_AT91)/a machine-$(CONFIG_ARCH_AXERA)\t\t+= axera' "$KDIR/arch/arm/Makefile"
grep -q 'CONFIG_ARCH_AXERA' "$KDIR/arch/arm/Makefile" || { log "could not hook mach-axera into arch/arm/Makefile"; exit 3; }

shopt -s nullglob
log "apply kernel patches (fotg210 udc: second interrupt line, pullup, polarity, IRQs before bind, status bits that clear, padding instead of zero length packets; dw spi: wait for the last frame; spidev for the LED)"
apply_patches "$HERE/kernel-patches" "$KDIR"

log "install LIVI-Link AX520 DTS"
mkdir -p "$KDIR/arch/arm/boot/dts/axera"
cp -f "$HERE/ax520.dts" "$KDIR/arch/arm/boot/dts/axera/ax520-livi-link.dts"
OVERLAYS=$(cd "$HERE/overlays" && ls *.dtso | sed 's/\.dtso$//')
rm -f "$KDIR"/arch/arm/boot/dts/axera/ax520-*.dtbo
{
  echo '# SPDX-License-Identifier: GPL-2.0'
  echo 'dtb-$(CONFIG_ARCH_AXERA) += ax520-livi-link.dtb'
  for o in $OVERLAYS; do
    cp -f "$HERE/overlays/$o.dtso" "$KDIR/arch/arm/boot/dts/axera/ax520-$o.dtso"
    echo "dtb-\$(CONFIG_ARCH_AXERA) += ax520-$o.dtbo"
  done
  echo 'DTC_FLAGS_ax520-livi-link := -@'
} > "$KDIR/arch/arm/boot/dts/axera/Makefile"
grep -q 'subdir-y += axera' "$KDIR/arch/arm/boot/dts/Makefile" \
  || echo 'subdir-y += axera' >> "$KDIR/arch/arm/boot/dts/Makefile"

# ---------------------------------------------------------------------------
# 2) AIC8800 driver: radxa SDK, Bluetooth through aic_btsdio.
# ---------------------------------------------------------------------------
aic8800_install

# ---------------------------------------------------------------------------
# 3) Config: built from allnoconfig, not multi_v7_defconfig. multi_v7 is the
#    kitchen-sink config for every ARMv7 SoC (DRM, media, sound, dozens of
#    other vendors' platform drivers), ~6000 objects of which this dongle
#    uses a few hundred. Everything listed here is something the DTS, the
#    boot path or a planned userspace feature actually needs.
# ---------------------------------------------------------------------------
INITRAMFS_LIST=$OUT/initramfs.list
{
  initramfs_common "$USERSPACE/bin/busybox"
  cat <<EOF
dir /dtbo 0755 0 0
file /sbin/ovl $HERE/initramfs/ovl 0755 0 0
file /sbin/diag $HERE/initramfs/diag 0755 0 0
file /sbin/flash-boot $HERE/initramfs/flash-boot 0755 0 0
file /sbin/flash-mtd $HERE/initramfs/flash-mtd 0755 0 0
file /sbin/sfc-sr $HERE/initramfs/sfc-sr 0755 0 0
file /sbin/led-test $HERE/initramfs/led-test 0755 0 0
EOF
} > "$INITRAMFS_LIST"
for o in $OVERLAYS; do
  echo "file /dtbo/ax520-$o.dtbo $KDIR/arch/arm/boot/dts/axera/ax520-$o.dtbo 0644 0 0" >> "$INITRAMFS_LIST"
done
check_initramfs_scripts "$INITRAMFS_LIST"

log "make allnoconfig"
cd "$KDIR"
make ARCH=arm allnoconfig >/dev/null

log "layer LIVI AX520 config onto allnoconfig"
# COMPILE_TEST below is required: USB_FOTG210 depends on ARCH_GEMINI || COMPILE_TEST.
# muboot passes no DTB: the one appended to the zImage is used, and the command line and memory
# size arrive as ATAGS that ARM_ATAG_DTB_COMPAT folds into it.
./scripts/config \
  --enable MMU \
  --enable ARCH_MULTIPLATFORM \
  --enable ARCH_MULTI_V7 \
  --enable ARCH_AXERA \
  --enable ARM_APPENDED_DTB \
  --enable ARM_ATAG_DTB_COMPAT \
  --enable ARM_ATAG_DTB_COMPAT_CMDLINE_EXTEND \
  --enable VFP \
  --enable VFPv3 \
  --enable NEON \
  --disable SMP \
  --enable PREEMPT \
  --enable CC_OPTIMIZE_FOR_SIZE \
  --enable KERNEL_XZ \
  --enable MODULES \
  --enable MODULE_UNLOAD \
  --enable EXPERT \
  --disable IO_URING \
  --disable ETHTOOL_NETLINK \
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
  --enable COMPAT_32BIT_TIME \
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
  --enable OF_OVERLAY \
  --enable BLK_DEV_INITRD \
  --enable INITRAMFS_COMPRESSION_NONE \
  --set-str INITRAMFS_SOURCE "$INITRAMFS_LIST" \
  \
  --enable TTY \
  --enable UNIX98_PTYS \
  --enable SERIAL_8250 \
  --enable SERIAL_8250_CONSOLE \
  --enable SERIAL_8250_DW \
  --enable SERIAL_OF_PLATFORM \
  --enable SERIAL_EARLYCON \
  \
  --enable GPIOLIB \
  --enable GPIO_DWAPB \
  --enable GPIO_SYSFS \
  \
  --enable MMC \
  --enable MMC_DW \
  --enable MMC_DW_PLTFM \
  --enable PWRSEQ_SIMPLE \
  \
  --enable MTD \
  --enable MTD_BLOCK \
  --enable MTD_CHAR \
  --enable MTD_OF_PARTS \
  --enable MTD_SPI_NOR \
  --disable MTD_SPI_NOR_USE_4K_SECTORS \
  --enable SPI \
  --enable SPI_DESIGNWARE \
  --enable SPI_DW_MMIO \
  --enable SPI_SPIDEV \
  --enable I2C \
  --enable I2C_CHARDEV \
  --enable I2C_DESIGNWARE_CORE \
  --enable I2C_DESIGNWARE_PLATFORM \
  --enable I2C_GPIO \
  --enable PINCTRL \
  --enable PINCTRL_SINGLE \
  --enable MISC_FILESYSTEMS \
  --enable SQUASHFS \
  --enable SQUASHFS_XZ \
  \
  --enable USB_SUPPORT \
  --enable COMPILE_TEST \
  --enable USB_FOTG210 \
  --enable USB_FOTG210_UDC \
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
  --enable NETDEVICES \
  $(aic8800_config) \
  \
  --enable DEBUG_KERNEL \
  --enable DEBUG_FS \
  --enable DEVMEM \
  --disable STRICT_DEVMEM \
  --enable MAGIC_SYSRQ \
  --set-val CONSOLE_LOGLEVEL_DEFAULT 8

make ARCH=arm CROSS_COMPILE="$CROSS_COMPILE" olddefconfig

# Everything the ways in hang on has to have survived olddefconfig, a dropped dependency is silent.
for sym in ARCH_AXERA ARM_APPENDED_DTB SERIAL_8250_DW USB_FOTG210_UDC USB_CONFIGFS_NCM USB_CONFIGFS_ACM \
           MMC_DW_PLTFM SPI_DW_MMIO MTD_SPI_NOR SPI_SPIDEV I2C_GPIO SQUASHFS SQUASHFS_XZ BLK_DEV_INITRD \
           BRIDGE CFG80211 BT AIC_WLAN_SUPPORT; do
  grep -q "^CONFIG_$sym=y" .config || { log "CONFIG_$sym did not make it into .config"; exit 4; }
done

# ---------------------------------------------------------------------------
# 4) Build
# ---------------------------------------------------------------------------
command -v "${CROSS_COMPILE}gcc" >/dev/null || { log "no ${CROSS_COMPILE}gcc in PATH"; exit 3; }
"${CROSS_COMPILE}gcc" --version | head -1

log "make dtbs, then zImage modules (-j$JOBS)"
make ARCH=arm CROSS_COMPILE="$CROSS_COMPILE" -j"$JOBS" dtbs
make ARCH=arm CROSS_COMPILE="$CROSS_COMPILE" -j"$JOBS" zImage modules

ZIMAGE=$KDIR/arch/arm/boot/zImage
DTB=$KDIR/arch/arm/boot/dts/axera/ax520-livi-link.dtb
[[ -f $ZIMAGE && -f $DTB ]] || { log "build did not produce zImage + DTB"; exit 4; }
log "zImage: $(stat -c%s "$ZIMAGE") B   DTB: $(stat -c%s "$DTB") B"

aic8800_collect

# ---------------------------------------------------------------------------
# 5) Wrap into this device's uImage variant: the legacy U-Boot header layout,
#    but muboot reads every field little-endian (the magic in a real
#    mtd3_boot.img is 56 19 05 27). mkimage only writes big-endian.
# ---------------------------------------------------------------------------
le32() {
  local v=$1
  # shellcheck disable=SC2059
  printf "$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
    $((v & 255)) $((v >> 8 & 255)) $((v >> 16 & 255)) $((v >> 24 & 255)))"
}

# CRC-32 as the gzip trailer carries it, little-endian
crc32le() { gzip -c "$1" | tail -c 8 | head -c 4; }

# make_uimage <image> <out> <name>, load and entry address 0x10008000
make_uimage() {
  local src=$1 dst=$2 name=${3:0:32} hdr=$2.hdr
  {
    le32 $((0x27051956))
    le32 0  # header CRC, computed over this header and filled in below
    le32 "$(date +%s)"
    le32 "$(wc -c < "$src")"
    le32 $((0x10008000))
    le32 $((0x10008000))
    crc32le "$src"
    printf '\x05\x02\x02\x00'  # Linux, ARM, kernel, uncompressed
    printf '%s' "$name"
    head -c $((32 - ${#name})) /dev/zero
  } > "$hdr"
  { head -c 4 "$hdr"; crc32le "$hdr"; tail -c +9 "$hdr"; cat "$src"; } > "$dst"
  rm -f "$hdr"
}

log "cat zImage + DTB, wrap as little-endian legacy uImage"
cat "$ZIMAGE" "$DTB" > "$OUT/zImage_w_dtb.bin"
make_uimage "$OUT/zImage_w_dtb.bin" "$OUT/livi-link-ax520-boot.uimg" "LIVI-Link AX520 $KVER"

BOOT_PART_SIZE=$((3 * 1024 * 1024))  # 3 MiB — the "boot" mtd region's real size.
UIMG_SIZE=$(wc -c < "$OUT/livi-link-ax520-boot.uimg")
(( UIMG_SIZE <= BOOT_PART_SIZE )) || { log "uImage is $UIMG_SIZE B, the boot partition only $BOOT_PART_SIZE B"; exit 4; }
log "boot uImage: $UIMG_SIZE B of $BOOT_PART_SIZE B"

log "done — $OUT/livi-link-ax520-boot.uimg"

build_livid
