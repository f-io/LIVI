#!/usr/bin/env bash
# LIVI-Link V821B+AIC8800D80 dongle kernel + bootimg build.
# One command: clone Tina sources, apply LIVI patches, build, wrap into ANDROID! bootimg
# that fits into Stock LY_U-Boot's mtd1 slot (0x60000, 0x310000 bytes).

set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
TOP=${TOP:-$HOME/LocalDev/tina-test}
TC_ROOT=${TC_ROOT:-$HOME/LocalDev/LIVI-Link/build/tmp/sysroots-components/x86_64/sunxi-nds32le-native}
JOBS=${JOBS:-$(nproc)}
: ${KBUILD_BUILD_USER:=LIVI}
: ${KBUILD_BUILD_HOST:=Link}
export KBUILD_BUILD_USER KBUILD_BUILD_HOST

TINA_ORG=${TINA_ORG:-https://github.com/sam-yangjj}
BRANCH=${BRANCH:-main}
REPOS=(kernel bsp device platform tools)

KDIR=$TOP/tina-v821-v1.3-kernel/linux-5.4-ansc
BSP=$TOP/tina-v821-v1.3-bsp
DEV=$TOP/tina-v821-v1.3-device
OUT=$TOP/out
mkdir -p "$OUT"

log(){ printf '\033[1;36m[livi-build]\033[0m %s\n' "$*"; }

# ---------------------------------------------------------------------------
# 1) Sources
# ---------------------------------------------------------------------------
mkdir -p "$TOP"
for r in "${REPOS[@]}"; do
  d=$TOP/tina-v821-v1.3-$r
  if [[ ! -d $d/.git ]]; then
    log "clone tina-v821-v1.3-$r"
    git clone --depth 1 --branch "$BRANCH" "$TINA_ORG/tina-v821-v1.3-$r.git" "$d"
  fi
done

# ---------------------------------------------------------------------------
# 2) BSP patches (idempotent)
# ---------------------------------------------------------------------------
log "patch bsp/Makefile (drop gpu, nand)"
sed -i \
  -e 's|^obj-y += modules/gpu/|# obj-y += modules/gpu/ (livi: no GPU)|' \
  -e 's|^obj-y += modules/nand/|# obj-y += modules/nand/ (livi: NOR only)|' \
  "$BSP/Makefile"

log "rewrite bsp/drivers/Makefile — minimal driver set"
cat > "$BSP/drivers/Makefile" <<'EOF'
# SPDX-License-Identifier: GPL-2.0
# LIVI-Link minimal V821B driver set (headless dongle, no GPU/NAND/media).
obj-y += clk/ pinctrl/ uart/ timer/ dma/ rtc/ bus/ dumpreg/ mbus/ aw_trace/
obj-y += mmc/ mtd/ sid/ phy/ usb/ twi/ spi/ spi-ng/
obj-y += input/ irqchip/ nvmem/ misc/ vendor_hooks/ chips/ net/ bluetooth/
obj-y += debug/ ansc_adaptor/
EOF

log "apply BSP patches (patch -p1 -N, idempotent)"
shopt -s nullglob; for p in "$HERE"/patches/v821b/*.patch; do
  # -N silently skips a hunk that's already applied; -r- prevents .rej files.
  # If the live apply fails, verify the patch IS already applied (dry-run reverse).
  ( cd "$BSP" && patch -p1 -N -r- -s < "$p" ) \
    || ( cd "$BSP" && patch -p1 -R --dry-run -s < "$p" >/dev/null ) \
    || { log "patch $(basename "$p") failed"; exit 5; }
done

log "generate bsp/include/sunxi-autogen.h"
mkdir -p "$BSP/include"
cat > "$BSP/include/sunxi-autogen.h" <<EOF
#define AW_BSP_VERSION "LIVI-Link V821B, $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
EOF

# ---------------------------------------------------------------------------
# 2b) AIC8800 driver: replace the sam-yangjj mirror's partial copy (no SDIO BT)
#     with the radxa full SDK — keeps Allwinner/sunxi support and ships aic_btsdio.
# ---------------------------------------------------------------------------
AIC_ORG=${AIC_ORG:-https://github.com/radxa-pkg/aic8800}
AIC_REF=${AIC_REF:-516e3b087763d80c44f5e3b6d2dd63e0d925c91d}
AIC_CACHE=$TOP/radxa-aic8800
AIC_SUB=src/SDIO/driver_fw/driver/aic8800
if [[ ! -e $AIC_CACHE/$AIC_SUB/aic8800_fdrv/aic_btsdio.c ]]; then
  log "fetch radxa aic8800 driver ($AIC_REF)"
  rm -rf "$AIC_CACHE"
  git clone --filter=blob:none --sparse "$AIC_ORG" "$AIC_CACHE"
  git -C "$AIC_CACHE" sparse-checkout set "$AIC_SUB"
  git -C "$AIC_CACHE" checkout -q "$AIC_REF"
fi
[[ -e $AIC_CACHE/$AIC_SUB/aic8800_fdrv/aic_btsdio.c ]] || { log "radxa aic_btsdio.c missing"; exit 6; }

log "overlay radxa AIC8800 driver into BSP"
DRV=$BSP/drivers/net/wireless/aic8800
rm -rf "$DRV"
cp -a "$AIC_CACHE/$AIC_SUB" "$DRV"

# radxa's aic8800/Kconfig sources its sub-Kconfigs with plain paths; the BSP is included
# via $(BSP_TOP), so prefix it or the fdrv/btlpm symbols never get defined.
sed -i 's|source "drivers/net/wireless/aic8800/|source "$(BSP_TOP)drivers/net/wireless/aic8800/|' \
  "$DRV/Kconfig"

# radxa's Makefiles are out-of-tree/Ubuntu oriented; force the in-tree Allwinner build,
# turn on SDIO Bluetooth (aic_btsdio.o + btsdio.o), off the Ubuntu platform shim.
for mk in "$DRV/Makefile" "$DRV/aic8800_bsp/Makefile" "$DRV/aic8800_fdrv/Makefile"; do
  [[ -f $mk ]] || continue
  sed -i \
    -e 's|^\([[:space:]]*export[[:space:]]*\)\?CONFIG_SDIO_BT[[:space:]]*=.*|CONFIG_SDIO_BT = y|' \
    -e 's|^\([[:space:]]*export[[:space:]]*\)\?CONFIG_PLATFORM_ALLWINNER[[:space:]]*?\?=.*|CONFIG_PLATFORM_ALLWINNER = y|' \
    -e 's|^\([[:space:]]*export[[:space:]]*\)\?CONFIG_PLATFORM_UBUNTU[[:space:]]*?\?=.*|CONFIG_PLATFORM_UBUNTU = n|' \
    "$mk"
done

# aic_btsdio picks its BT transport by CONFIG_BLUEDROID: 0 = BlueZ (kernel BT subsystem, what
# we want with CONFIG_BT=y — hci_register_dev + mgmt), 1 = bluedroid/standalone (defines its own
# bdaddr_t/HCI enums → clashes with <net/bluetooth/hci.h>). The header ties that to
# CONFIG_PLATFORM_UBUNTU; force BlueZ regardless of platform.
# Also silence the per-packet AICBT_DBG spam (aic_btsdio: hci0 / skb type / btsdio_send_frame /
# btsdio_work) that floods the console every few ms; AICBT_INFO/WARN/ERR (init/errors) stay.
sed -i \
  -e 's|#define CONFIG_BLUEDROID\([[:space:]]\{1,\}\)1|#define CONFIG_BLUEDROID\10|' \
  -e 's|#define AICBT_DBG_FLAG\([[:space:]]\{1,\}\)1|#define AICBT_DBG_FLAG\10|' \
  "$DRV/aic8800_fdrv/aic_btsdio.h"

log "apply AIC8800 driver patches"
for p in "$HERE"/patches/aic8800/*.patch; do
  ( cd "$DRV" && patch -p1 -s < "$p" ) || { log "patch $(basename "$p") failed"; exit 6; }
done

# ---------------------------------------------------------------------------
# 3) Kernel patches: symlinks, DTSIs, board DTS
# ---------------------------------------------------------------------------
log "wire BSP into kernel tree"
ln -sfnr "$BSP" "$KDIR/bsp"
mkdir -p "$KDIR/include/dt-bindings"
for d in "$BSP/include/dt-bindings"/*; do
  n=$(basename "$d")
  [[ -e $KDIR/include/dt-bindings/$n ]] || ln -sfnr "$d" "$KDIR/include/dt-bindings/$n"
done

log "install canonical LIVI-Link DTS (decompiled from Stock, LED node = linux,spidev)"
DTS_DIR=$KDIR/arch/riscv/boot/dts/allwinner
mkdir -p "$DTS_DIR"
cp -f "$HERE/dts/livi-link-v821b.dts" "$DTS_DIR/livi-link-v821b.dts"
echo 'dtb-y += livi-link-v821b.dtb' > "$DTS_DIR/Makefile"
grep -q 'subdir-y += allwinner' "$KDIR/arch/riscv/boot/dts/Makefile" \
  || echo 'subdir-y += allwinner' >> "$KDIR/arch/riscv/boot/dts/Makefile"

# ---------------------------------------------------------------------------
# 4) Defconfig: start from BSP baseline, layer LIVI overrides
# ---------------------------------------------------------------------------
log "assemble defconfig from BSP + LIVI overrides"
BASE=$BSP/configs/linux-5.4-ansc/sun300iw1p1_min_defconfig
DEST=$KDIR/arch/riscv/configs/sun300iw1p1_min_defconfig
cp -f "$BASE" "$DEST"

disable(){ for k in "$@"; do sed -i "s|^CONFIG_$k=.*|# CONFIG_$k is not set|" "$DEST"; done; }
disable IKCONFIG IKCONFIG_PROC CGROUPS CGROUP_SCHED CFS_BANDWIDTH CGROUP_BPF \
        NAMESPACES USER_NS CHECKPOINT_RESTORE BLK_DEV_INITRD BPF_SYSCALL SMP \
        IP_MULTICAST NET_9P ATA USB USB_EHCI_HCD USB_EHCI_HCD_PLATFORM \
        USB_OHCI_HCD USB_OHCI_HCD_PLATFORM USB_STORAGE USB_UAS \
        NFS_FS NFS_V4 NFS_V4_1 9P_FS CRYPTO_USER_API_HASH DEBUG_FS \
        MTD_SPI_NOR
sed -i 's|CONFIG_INITRAMFS_SOURCE=.*|CONFIG_INITRAMFS_SOURCE=""|' "$DEST"

cat >> "$DEST" <<'EOF'
CONFIG_CC_OPTIMIZE_FOR_SIZE=y
CONFIG_PREEMPT=y

# The perf2b defconfig defaults to CHIP_V821. Our silicon is V821B.
# CONFIG_CHIP_V821 is not set
CONFIG_CHIP_V821B=y

# Bootloader-supplied cmdline is loglevel=3; force ours so we see everything.
# uses the SBI HVC console (registered by hvc_sbi_init). The Sunxi
# UART driver does not register a tty on this SoC, so ttyAS0/ttyS0 leave /init
# without a working /dev/console.
CONFIG_CMDLINE_FORCE=y
CONFIG_CMDLINE="earlyprintk=sunxi-uart,0x42500000 keep_bootcon loglevel=8 usbcore.autosuspend=-1 nohz=off root=/dev/mtdblock3 rootfstype=squashfs rootwait mbr_offset=376832 gpt=1 partitions=boot@mtdblock1:recovery@mtdblock2:rootfs@mtdblock3:customer@mtdblock4:env@mtdblock5:env-redund@mtdblock6:private@mtdblock7:logo@mtdblock8:UDISK@mtdblock9 init=/init"

# SPI-NOR (xt25p1288 via sunxi_spif @ 0x44f00000) + AW MTD + sunxipart
CONFIG_AW_MTD=y
CONFIG_AW_MTD_SPI_NOR_5_4=y
CONFIG_AW_MTD_SUNXI_PARTS=y
CONFIG_AW_SPI_NG=y
CONFIG_MTD=y
CONFIG_MTD_BLOCK=y
CONFIG_MTD_CMDLINE_PARTS=y
CONFIG_MTD_OF_PARTS=y
CONFIG_SPI=y
CONFIG_SPI_MASTER=y
CONFIG_SPI_SPIDEV=y
CONFIG_SPIF_SUNXI=y

# Squashfs+XZ (Stock rootfs). BCJ filters for x86/ARM are useless on RISC-V.
CONFIG_SQUASHFS=y
CONFIG_SQUASHFS_XZ=y
CONFIG_XZ_DEC=y
# CONFIG_XZ_DEC_X86 is not set
# CONFIG_XZ_DEC_ARM is not set
# CONFIG_XZ_DEC_ARMTHUMB is not set
# CONFIG_XZ_DEC_IA64 is not set
# CONFIG_XZ_DEC_POWERPC is not set
# CONFIG_XZ_DEC_SPARC is not set

# I2C for MFi coprocessor. AW_TWI needs AW_DMA (Kconfig dependency).
CONFIG_I2C=y
CONFIG_I2C_CHARDEV=y
CONFIG_AW_DMA=y
CONFIG_AW_TWI=y

# MMC / SDIO for AIC8800 (WiFi+BT combo)
CONFIG_MMC=y
CONFIG_MMC_SDIO=y
CONFIG_AW_MMC_HSQ=y
CONFIG_AW_MMC=y
CONFIG_AW_MMC_V5P3X=y
CONFIG_GPIO_SYSFS=y

# USB gadget (NCM to host)
CONFIG_USB_GADGET=y
CONFIG_USB_LIBCOMPOSITE=y
CONFIG_USB_U_ETHER=y
CONFIG_USB_F_NCM=y
CONFIG_USB_G_NCM=y

# Sunxi USB stack (UDC0)
CONFIG_USB_SUNXI_USB=y
CONFIG_USB_SUNXI_UDC0=y
CONFIG_USB_SUNXI_PHY=y
CONFIG_USB_SUNXI_HCI=y
CONFIG_USB_SUNXI_EXTCON=y
CONFIG_USB_SUNXI_USB_MANAGER=y

# WiFi core + AIC8800 D80 SDIO combo (WiFi + BT). Firmware blobs live in mtd3.
CONFIG_WLAN=y
CONFIG_CFG80211=y
CONFIG_FW_LOADER=y
CONFIG_NETDEVICES=y
CONFIG_AW_RFKILL=y
CONFIG_AIC_WLAN_SUPPORT=y
CONFIG_AIC8800_WLAN_SUPPORT=m
CONFIG_AIC8800_BTLPM_SUPPORT=m
# radxa driver reads the firmware dir from this Kconfig string; our blobs live in aic8800D80/.
CONFIG_AIC_FW_PATH="/lib/firmware/aic8800D80"
# SDIO interface + SDIO Bluetooth are hard-set in the radxa Makefiles (patched in step 2b), not
# Kconfig; the OOB host-wake IRQ is gated by CONFIG_GPIO_WAKEUP (default n) so it is skipped.

# Bluetooth stack for AIC8800 combo (BT+WiFi share SDIO).
CONFIG_BT=y
CONFIG_BT_BREDR=y
CONFIG_BT_LE=y
CONFIG_BT_RFCOMM=y
CONFIG_BT_MGMT=y
CONFIG_CRYPTO_ECDH=y
# CONFIG_BT_HS is not set
# CONFIG_BT_DEBUGFS is not set
# CONFIG_BT_BNEP is not set
# CONFIG_BT_HIDP is not set
# CONFIG_BT_HCIVHCI is not set

# No module signing (extract-cert.c needs OpenSSL 1.1 engine API, missing on OpenSSL 3.x hosts).
# CONFIG_MODULE_SIG is not set
# CONFIG_SYSTEM_TRUSTED_KEYRING is not set
# CONFIG_SYSTEM_EXTRA_CERTIFICATE is not set
# CONFIG_WATCHDOG is not set

# Diagnostics only (ss --bluetooth / -0). nl80211 and our DHCP server need neither.
CONFIG_INET_DIAG=y
CONFIG_INET_UDP_DIAG=y
# CONFIG_NETFILTER is not set
# CONFIG_NF_CONNTRACK is not set
CONFIG_BRIDGE=y

# CONFIG_BRIDGE_IGMP_SNOOPING is not set
# CONFIG_BRIDGE_VLAN_FILTERING is not set
EOF

# ---------------------------------------------------------------------------
# 5) Build
# ---------------------------------------------------------------------------
TC_BIN=${TC_BIN:-$(echo "$TC_ROOT"/usr/lib/sunxi-toolchains/nds32le-linux-glibc-v5d*/bin | awk '{print $1}')}
[[ -x $TC_BIN/riscv32-linux-gcc ]] || { log "no riscv32-linux-gcc under $TC_BIN"; exit 3; }
export PATH=$TC_BIN:$PATH
export ARCH=riscv
export CROSS_COMPILE=riscv32-linux-
export BSP_TOP=bsp/
export KCFLAGS="${KCFLAGS:-} -fno-asynchronous-unwind-tables -fno-unwind-tables"

cd "$KDIR"
# OpenSSL 3.x dropped the ENGINE API's engine.h; kernel 5.4's extract-cert.c
# includes it unconditionally. Neutralise the include — the ENGINE_* code path
# is only used with '-e <engine>' which we never pass.
log "patch extract-cert: drop openssl/engine.h include"
sed -i 's|^#include <openssl/engine.h>|/* engine.h removed for OpenSSL 3 compat */|'   "$KDIR/scripts/extract-cert.c"

# LIVI identifies the dongle by the "LIVI Link" product string, not by
# vendor/product ids. Keep g_ncm's default 0525:a4a1 (NetChip/Linux gadget)
# and only rewrite the descriptor strings — no hijacking of another
# vendor's registered ids.
NCM=$KDIR/drivers/usb/gadget/legacy/ncm.c
if [[ -f "$NCM" ]]; then
    log "patch g_ncm: LIVI Link descriptor strings"
    sed -i \
        -e 's|^#define DRIVER_DESC.*"NCM Gadget"|#define DRIVER_DESC "LIVI Link"|' \
        -e 's|\[USB_GADGET_MANUFACTURER_IDX\]\.s = "",|[USB_GADGET_MANUFACTURER_IDX].s = "LIVI",|' \
        "$NCM"
fi

log "make sun300iw1p1_min_defconfig"
make -j"$JOBS" sun300iw1p1_min_defconfig >/tmp/livi-cfg.log 2>&1

log "make Image dtbs modules (-j$JOBS)"
make -j"$JOBS" Image dtbs modules

IMG=$KDIR/arch/riscv/boot/Image
DTB=$KDIR/arch/riscv/boot/dts/allwinner/livi-link-v821b.dtb
[[ -f $IMG && -f $DTB ]] || { log "build did not produce Image + DTB"; exit 4; }
log "Image: $(stat -c%s "$IMG") B   DTB: $(stat -c%s "$DTB") B"

# AIC8800 modules
log "collect AIC8800 modules (bsp, fdrv, btlpm)"
MODOUT=$OUT/modules
rm -rf "$MODOUT"; mkdir -p "$MODOUT"
for m in aic8800_bsp aic8800_fdrv aic8800_btlpm; do
  ko=$(find "$BSP" -name "$m.ko" -print -quit)
  [[ -n $ko ]] || { log "module $m.ko not built"; exit 4; }
  riscv32-linux-strip --strip-debug "$ko" -o "$MODOUT/$m.ko"
  log "  $m.ko: $(stat -c%s "$MODOUT/$m.ko") B"
done

# ---------------------------------------------------------------------------
# 6) Wrap into ANDROID! bootimg (host-side Rust tool)
# ---------------------------------------------------------------------------
HELPERD=$(cd "$HERE/../../native/livi-helperd" && pwd)
source "$HERE/version.sh"
log "cargo build -p mkbootimg-v821b (host)"
( cd "$HELPERD" && cargo build --release -p mkbootimg-v821b )

log "wrap kernel + DTB into Stock-compatible ANDROID! bootimg"
"$HELPERD/target/release/mkbootimg-v821b" "$IMG" "$DTB" "$OUT/livi-link-v821b-mtd1.bin"

log "done — $OUT/livi-link-v821b-mtd1.bin"
log ""
log "flash to a LIVI-Link (V821B) in FEL mode:"
log "  sudo ~/LocalDev/xfel/xfel spinor write 0x60000 $OUT/livi-link-v821b-mtd1.bin"

# ---------------------------------------------------------------------------
# 7) Cross-build dongle-side Rust binaries (riscv32gc-unknown-linux-gnu, nightly)
# ---------------------------------------------------------------------------
log "cargo +nightly build livid (riscv32 multi-call)"
( cd "$HELPERD" && cargo +nightly build --profile embedded -p livid \
    --target riscv32gc-unknown-linux-gnu \
    -Z build-std=std,panic_abort )
DBIN=$HELPERD/target/riscv32gc-unknown-linux-gnu/embedded
[[ -x "$DBIN/livid" ]] || { log "missing $DBIN/livid"; exit 4; }
log "  livid: $(stat -c%s "$DBIN/livid") B"
cp -f "$DBIN/livid" "$OUT/livid"
