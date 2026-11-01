#!/usr/bin/env bash
# LIVI-Link i.MX6ULL dongle kernel for the module RADIO names (board.sh): zImage with the initramfs
# built in and our DTB appended, which the provisioner stages for the vendor U-Boot (dongle arm
# imx6ul kernel <image> --write), and the Bluetooth modules and livid for the rootfs.
# Order: ../build-userspace.sh <this dir>, build.sh, ../../common/build-rootfs.sh <this dir>,
# ../../common/pack-bundle.sh <this dir>.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
: ${KBUILD_BUILD_USER:=LIVI}
: ${KBUILD_BUILD_HOST:=Link}
export KBUILD_BUILD_USER KBUILD_BUILD_HOST

for b in rescue/busybox usr/sbin/hostapd; do
  [[ -x $USERSPACE/$b ]] || { log "no $b at $USERSPACE, run build-userspace.sh first"; exit 1; }
done
fetch_kernel
fetch_firmware

log "apply kernel patches (bus clocks on the ULL, USB gadget without BSV wait, no charger detection, MX25L128 without SFDP, mwifiex AP receive rate)"
apply_patches "$HERE/kernel-patches" "$KDIR"

log "install the board DTS with the $RADIO part"
DTS_DIR=$KDIR/arch/arm/boot/dts/nxp/imx
cp -f "$HERE/imx6ull.dts" "$DTS_DIR/imx6ull-livi-link.dts"
cp -f "$HERE/imx6ull-$RADIO.dtsi" "$DTS_DIR/imx6ull-livi-link-radio.dtsi"
grep -q 'imx6ull-livi-link.dtb' "$DTS_DIR/Makefile" \
  || echo 'dtb-$(CONFIG_SOC_IMX6UL) += imx6ull-livi-link.dtb' >> "$DTS_DIR/Makefile"

INITRAMFS_LIST=$OUT/initramfs.list
{
  initramfs_common "$USERSPACE/rescue/busybox"
  cat <<EOF
dir /lib 0755 0 0
dir /lib/firmware 0755 0 0
file /usr/sbin/hostapd $HOSTAPD 0755 0 0
file /sbin/wifi-up $HERE/initramfs/wifi-up 0755 0 0
file /lib/firmware/regulatory.db $REGDB 0644 0 0
EOF
  wifi_firmware_initramfs
} > "$INITRAMFS_LIST"
check_initramfs_scripts "$INITRAMFS_LIST"

log "make allnoconfig"
cd "$KDIR"
make ARCH=arm allnoconfig >/dev/null

# The module's Wi-Fi driver is built in, the Wi-Fi AP is one of the two ways in. Its Bluetooth
# driver is a module in the rootfs.
case $RADIO in
  iw416)
    RADIO_CONFIG=(--enable WLAN_VENDOR_MARVELL --enable MWIFIEX --enable MWIFIEX_SDIO --module BT_NXPUART)
    RADIO_BUILTIN=(MWIFIEX_SDIO)
    RADIO_MODULES=(BT_NXPUART)
    ;;
  rtl8822cs)
    RADIO_CONFIG=(
      --enable MAC80211 --enable WLAN_VENDOR_REALTEK --enable RTW88 --enable RTW88_8822CS
      --enable BT_HCIUART_SERDEV --enable BT_HCIUART_3WIRE --enable BT_HCIUART_RTL
    )
    RADIO_BUILTIN=(MAC80211 RTW88_8822CS BT_HCIUART_SERDEV BT_HCIUART_RTL)
    RADIO_MODULES=(BT_HCIUART BT_RTL)
    ;;
  rtl8822bs)
    RADIO_CONFIG=(--enable MAC80211 --enable WLAN_VENDOR_REALTEK --enable RTW88 --enable RTW88_8822BS)
    RADIO_BUILTIN=(MAC80211 RTW88_8822BS)
    RADIO_MODULES=()
    ;;
  bcm4354)
    RADIO_CONFIG=(--enable WLAN_VENDOR_BROADCOM --enable BRCMFMAC --enable BRCMFMAC_SDIO)
    RADIO_BUILTIN=(BRCMFMAC BRCMFMAC_SDIO)
    RADIO_MODULES=()
    ;;
esac

# Built from allnoconfig: only what the board, the boot path, the two ways in and the rootfs need.
# No cpufreq: the CPU keeps the clock U-Boot set. Bluetooth and the crypto it selects do not fit
# into the bytes U-Boot reads, they are modules in the rootfs.
log "layer the LIVI i.MX6ULL config onto allnoconfig"
./scripts/config \
  --enable MMU \
  --enable ARCH_MULTIPLATFORM \
  --enable ARCH_MULTI_V7 \
  --enable ARCH_MXC \
  --enable SOC_IMX6UL \
  --enable ARM_APPENDED_DTB \
  --enable VFP \
  --enable VFPv3 \
  --enable NEON \
  --disable SMP \
  --enable PREEMPT \
  --enable CC_OPTIMIZE_FOR_SIZE \
  --enable KERNEL_XZ \
  --enable MODULES \
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
  --enable MISC_FILESYSTEMS \
  --enable SQUASHFS \
  --enable SQUASHFS_XZ \
  \
  --enable TTY \
  --enable UNIX98_PTYS \
  --enable SERIAL_IMX \
  --enable SERIAL_IMX_CONSOLE \
  --enable SERIAL_DEV_BUS \
  --enable SERIAL_DEV_CTRL_TTYPORT \
  --enable I2C \
  --enable I2C_CHARDEV \
  --enable I2C_IMX \
  \
  --enable GPIOLIB \
  --enable GPIO_MXC \
  --enable GPIO_SYSFS \
  --enable NEW_LEDS \
  --enable LEDS_CLASS \
  --enable LEDS_GPIO \
  --enable LEDS_TRIGGERS \
  --enable LEDS_TRIGGER_HEARTBEAT \
  --enable LEDS_TRIGGER_TIMER \
  --enable REGULATOR \
  --enable REGULATOR_FIXED_VOLTAGE \
  --enable REGULATOR_ANATOP \
  --enable WATCHDOG \
  --enable WATCHDOG_HANDLE_BOOT_ENABLED \
  --enable IMX2_WDT \
  --enable NVMEM \
  --enable NVMEM_IMX_OCOTP \
  \
  --enable MMC \
  --enable MMC_SDHCI \
  --enable MMC_SDHCI_PLTFM \
  --enable MMC_SDHCI_ESDHC_IMX \
  \
  --enable MTD \
  --enable MTD_BLOCK \
  --enable MTD_CHAR \
  --enable MTD_OF_PARTS \
  --enable MTD_SPI_NOR \
  --disable MTD_SPI_NOR_USE_4K_SECTORS \
  --enable SPI \
  --enable SPI_MEM \
  --enable SPI_FSL_QUADSPI \
  \
  --enable USB_SUPPORT \
  --enable USB_PHY \
  --enable USB_MXS_PHY \
  --enable USB_CHIPIDEA \
  --enable USB_CHIPIDEA_UDC \
  --disable USB_CHIPIDEA_HOST \
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
  --enable WIRELESS \
  --enable WLAN \
  --enable CFG80211 \
  --enable CFG80211_CERTIFICATION_ONUS \
  --disable CFG80211_REQUIRE_SIGNED_REGDB \
  --disable CFG80211_CRDA_SUPPORT \
  --enable FW_LOADER \
  \
  --module BT \
  --enable BT_BREDR \
  --enable BT_LE \
  --module BT_RFCOMM \
  --module BT_HCIUART \
  --disable IMX_MU_MSI \
  "${RADIO_CONFIG[@]}" \
  \
  --enable DEVMEM \
  --disable STRICT_DEVMEM \
  --enable MAGIC_SYSRQ \
  --set-val CONSOLE_LOGLEVEL_DEFAULT 7

make ARCH=arm CROSS_COMPILE="$CROSS_COMPILE" olddefconfig

# Everything the ways in hang on has to have survived olddefconfig, a dropped dependency is silent.
for sym in SOC_IMX6UL ARM_APPENDED_DTB SERIAL_IMX USB_CHIPIDEA_UDC USB_CHIPIDEA_IMX USB_MXS_PHY USB_CONFIGFS_NCM \
           MMC_SDHCI_ESDHC_IMX CFG80211 BRIDGE SPI_FSL_QUADSPI MTD_SPI_NOR NVMEM_IMX_OCOTP \
           LEDS_TRIGGER_HEARTBEAT IMX2_WDT MTD_BLOCK SQUASHFS SQUASHFS_XZ I2C_IMX I2C_CHARDEV \
           SERIAL_DEV_CTRL_TTYPORT BLK_DEV_WRITE_MOUNTED "${RADIO_BUILTIN[@]}"; do
  grep -q "^CONFIG_$sym=y" .config || { log "CONFIG_$sym did not make it into .config"; exit 4; }
done
for sym in BT BT_RFCOMM "${RADIO_MODULES[@]}"; do
  grep -q "^CONFIG_$sym=m" .config || { log "CONFIG_$sym did not make it into .config as a module"; exit 4; }
done

command -v "${CROSS_COMPILE}gcc" >/dev/null || { log "no ${CROSS_COMPILE}gcc in PATH"; exit 3; }
log "make dtbs zImage modules (-j$JOBS)"
make ARCH=arm CROSS_COMPILE="$CROSS_COMPILE" -j"$JOBS" dtbs zImage modules

# The rootfs has no modprobe, board_wifi loads every module up front in the order of the file "load":
# modules.order (crypto before net, so what Bluetooth asks the crypto API for by name is there), and
# each one after the modules it links against.
log "collect the modules for the rootfs"
MODOUT=$OUT/modules
rm -rf "$MODOUT"; mkdir -p "$MODOUT"
declare -A file deps
names=()
while read -r o; do
  ko=${o%.*}.ko
  n=$(basename "$ko" .ko); n=${n//-/_}
  names+=("$n"); file[$n]=$ko
  # depends= names a module by its file name (crc-ccitt), the kernel by its module name (crc_ccitt).
  deps[$n]=$("${CROSS_COMPILE}readelf" -p .modinfo "$ko" | sed -n 's/.*depends=//p' | tr ,- ' _')
done < modules.order
loaded=" "
while (( ${#names[@]} )); do
  next=
  for n in "${names[@]}"; do
    ready=1
    for d in ${deps[$n]}; do [[ $loaded == *" $d "* ]] || ready=; done
    [[ $ready ]] && { next=$n; break; }
  done
  [[ $next ]] || { log "the dependencies of ${names[*]} do not resolve"; exit 4; }
  f=$(basename "${file[$next]}")
  "${CROSS_COMPILE}strip" --strip-debug "${file[$next]}" -o "$MODOUT/$f"
  echo "${f%.ko}" >> "$MODOUT/load"
  loaded+="$next "
  rest=(); for n in "${names[@]}"; do [[ $n == "$next" ]] || rest+=("$n"); done; names=("${rest[@]}")
done
log "  $(wc -l < "$MODOUT/load") modules, $(cat "$MODOUT"/*.ko | wc -c) B"

ZIMAGE=$KDIR/arch/arm/boot/zImage
DTB=$DTS_DIR/imx6ull-livi-link.dtb
[[ -f $ZIMAGE && -f $DTB ]] || { log "build did not produce zImage + DTB"; exit 4; }
cat "$ZIMAGE" "$DTB" > "$KERNEL_IMG"
SIZE=$(stat -c%s "$KERNEL_IMG")
log "zImage $(stat -c%s "$ZIMAGE") B + DTB $(stat -c%s "$DTB") B = $SIZE B of $KERNEL_MAX B U-Boot reads"
(( SIZE <= KERNEL_MAX )) || { log "image is $((SIZE - KERNEL_MAX)) B too large for the vendor U-Boot"; exit 6; }
log "done: $KERNEL_IMG"

build_livid
