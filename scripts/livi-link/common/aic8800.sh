# Sourced by the boards with an AIC8800D80 on SDIO (ax520, v821b): the radxa driver, overlaid into the
# kernel tree with Bluetooth over SDIO, and the firmware it loads. Needs TOP, KDIR, OUT, COMMON, log().
AIC_ORG=${AIC_ORG:-https://github.com/radxa-pkg/aic8800}
AIC_REF=${AIC_REF:-516e3b087763d80c44f5e3b6d2dd63e0d925c91d}
AIC_CACHE=$TOP/radxa-aic8800
AIC_SUB=src/SDIO/driver_fw/driver/aic8800
AIC_FW=$AIC_CACHE/src/SDIO/driver_fw/fw/aic8800D80
AIC_DRV=$KDIR/drivers/net/wireless/aic8800
# What the driver request_firmware()s with Bluetooth over SDIO, plus the u04 patch pair and the two
# config texts, which are tiny.
AIC_FW_FILES="aic_powerlimit_8800d80.txt aic_userconfig_8800d80.txt
              fmacfwbt_8800d80_h_u02.bin fw_adid_8800d80_u02.bin
              fw_patch_8800d80_u02.bin fw_patch_8800d80_u02_ext0.bin fw_patch_8800d80_u04.bin
              fw_patch_table_8800d80_u02.bin fw_patch_table_8800d80_u04.bin"

# In-tree rather than out-of-tree: the vendor Makefiles' obj-m/obj-y have to read our real Kconfig, and
# the CFG80211/BT symbols only resolve against Module.symvers of a vmlinux built with them.
aic8800_install() {
  if [[ ! -e $AIC_CACHE/$AIC_SUB/aic8800_fdrv/aic_btsdio.c || ! -d $AIC_FW ]]; then
    log "fetch radxa aic8800 driver ($AIC_REF)"
    rm -rf "$AIC_CACHE"
    git clone --filter=blob:none --sparse "$AIC_ORG" "$AIC_CACHE"
    git -C "$AIC_CACHE" sparse-checkout set "$AIC_SUB" "${AIC_FW#$AIC_CACHE/}"
    git -C "$AIC_CACHE" checkout -q "$AIC_REF"
  fi

  log "overlay the AIC8800 driver into drivers/net/wireless/aic8800"
  rm -rf "$AIC_DRV"
  cp -a "$AIC_CACHE/$AIC_SUB" "$AIC_DRV"
  find "$AIC_DRV" -name '*.o' -o -name '*.ko' -o -name '.*.cmd' -o -name Module.symvers -o -name modules.order \
    | xargs rm -f

  log "apply AIC8800 driver patches (BT tweaks, mainline API)"
  apply_patches "$COMMON/patches/aic8800" "$AIC_DRV"
  apply_patches "$COMMON/patches/aic8800-mainline" "$AIC_DRV"

  grep -q 'aic8800/Kconfig' "$KDIR/drivers/net/wireless/Kconfig" || \
    sed -i '/^source "drivers\/net\/wireless\/virtual\/Kconfig"/i source "drivers/net/wireless/aic8800/Kconfig"' \
      "$KDIR/drivers/net/wireless/Kconfig"
  grep -q 'AIC_WLAN_SUPPORT' "$KDIR/drivers/net/wireless/Makefile" || \
    echo 'obj-$(CONFIG_AIC_WLAN_SUPPORT) += aic8800/' >> "$KDIR/drivers/net/wireless/Makefile"

  # The top-level vendor Makefile sets these three to m, which overrides our .config the moment Kbuild
  # reads it.
  sed -i '/^CONFIG_AIC8800_BTLPM_SUPPORT := m$/d;/^CONFIG_AIC8800_WLAN_SUPPORT := m$/d;/^CONFIG_AIC_WLAN_SUPPORT := m$/d' \
    "$AIC_DRV/Makefile"

  # bsp and fdrv carry the same SDIO and message code under different file names with the same symbol
  # names, so they only link as two separate modules, never built in.

  # aic8800_bsp tells the firmware which port Bluetooth uses, so bsp and fdrv both need SDIO BT on.
  # With fdrv alone the firmware stays on UART and asserts on the first HCI reset.
  local mk
  for mk in "$AIC_DRV/aic8800_bsp/Makefile" "$AIC_DRV/aic8800_fdrv/Makefile"; do
    sed -i 's|^\([[:space:]]*export[[:space:]]*\)\?CONFIG_SDIO_BT[[:space:]]*=.*|CONFIG_SDIO_BT = y|' "$mk"
  done
  grep -q '^CONFIG_SDIO_BT = y' "$AIC_DRV/aic8800_bsp/Makefile" && grep -q '^CONFIG_SDIO_BT = y' "$AIC_DRV/aic8800_fdrv/Makefile" \
    || { log "CONFIG_SDIO_BT not found in the AIC8800 Makefiles"; exit 6; }
  sed -i 's|#define AICBT_DBG_FLAG\([[:space:]]\{1,\}\)1|#define AICBT_DBG_FLAG\10|' "$AIC_DRV/aic8800_fdrv/aic_btsdio.h"
}

# The scripts/config arguments for WiFi, Bluetooth and the driver.
aic8800_config() {
  echo --enable WIRELESS --enable WLAN --enable CFG80211 --enable FW_LOADER \
       --enable CFG80211_CERTIFICATION_ONUS --disable CFG80211_REQUIRE_SIGNED_REGDB \
       --disable CFG80211_CRDA_SUPPORT --enable CFG80211_INTERNAL_REGDB \
       --enable AIC_WLAN_SUPPORT --module AIC8800_WLAN_SUPPORT --disable AIC8800_BTLPM_SUPPORT \
       --set-str AIC_FW_PATH /lib/firmware/aic8800d80 \
       --enable BT --enable BT_BREDR --enable BT_LE --enable BT_RFCOMM --enable CRYPTO_ECDH
}

# bsp and fdrv, stripped, to $OUT/modules for the rootfs.
aic8800_collect() {
  local m ko
  log "collect AIC8800 modules (bsp, fdrv)"
  rm -rf "$OUT/modules"; mkdir -p "$OUT/modules"
  for m in aic8800_bsp aic8800_fdrv; do
    ko=$(find "$AIC_DRV" -name "$m.ko" -print -quit)
    [[ -n $ko ]] || { log "module $m.ko not built"; exit 4; }
    "${CROSS_COMPILE}strip" --strip-debug "$ko" -o "$OUT/modules/$m.ko"
    log "  $m.ko: $(stat -c%s "$OUT/modules/$m.ko") B"
  done
}

# Modules and firmware into a rootfs being assembled.
aic8800_rootfs() {
  local work=$1 f
  [[ -f $OUT/modules/aic8800_bsp.ko && -f $OUT/modules/aic8800_fdrv.ko ]] || { log "no AIC8800 modules in $OUT/modules, run the kernel build first"; exit 1; }
  [[ -d $AIC_FW ]] || { log "no AIC8800 firmware at $AIC_FW, run the kernel build first"; exit 1; }
  log "AIC8800 modules and firmware"
  local modules
  modules=$work/lib/modules/$(krelease)
  mkdir -p "$modules" "$work/lib/firmware/aic8800d80"
  cp "$OUT/modules"/aic8800_bsp.ko "$OUT/modules"/aic8800_fdrv.ko "$modules/"
  for f in $AIC_FW_FILES; do
    [[ -f $AIC_FW/$f ]] || { log "firmware $f not in $AIC_FW"; exit 2; }
    cp "$AIC_FW/$f" "$work/lib/firmware/aic8800d80/$f"
  done
}
