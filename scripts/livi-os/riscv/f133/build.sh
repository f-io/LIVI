#!/usr/bin/env bash
# Order: ../../../livi-link/common/build-userspace.sh <this dir>, build.sh,
# ../../../livi-link/common/build-rootfs.sh <this dir>.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
: ${KBUILD_BUILD_USER:=LIVI}
: ${KBUILD_BUILD_HOST:=OS}
export KBUILD_BUILD_USER KBUILD_BUILD_HOST
OPENSBI_VER=${OPENSBI_VER:-1.7}
BOOT0_REV=d21a737
DTB=sun20i-f133b-xfh.dtb

[[ -x $USERSPACE/rescue/busybox ]] || { log "no rescue/busybox at $USERSPACE, run build-userspace.sh first"; exit 1; }
fetch_kernel
apply_patches "$HERE/kernel-patches" "$KDIR"
aic8800_install
g2d_install

log "board DTS, panel and touch driver"
cp "$HERE/$(basename "$DTB" .dtb).dts" "$KDIR/arch/riscv/boot/dts/allwinner/"
cp "$HERE/panel-livi-he0801a068.c" "$KDIR/drivers/gpu/drm/panel/"
grep -q panel-livi-he0801a068 "$KDIR/drivers/gpu/drm/panel/Makefile" \
  || echo 'obj-$(CONFIG_DRM_PANEL_LIVI_HE0801A068) += panel-livi-he0801a068.o' >>"$KDIR/drivers/gpu/drm/panel/Makefile"
grep -q DRM_PANEL_LIVI_HE0801A068 "$KDIR/drivers/gpu/drm/panel/Kconfig" \
  || sed -i 's|^endmenu$|config DRM_PANEL_LIVI_HE0801A068\n\ttristate "HE0801A068 MIPI-DSI panel"\n\tdepends on OF \&\& DRM_MIPI_DSI \&\& BACKLIGHT_CLASS_DEVICE\n\nendmenu|' \
       "$KDIR/drivers/gpu/drm/panel/Kconfig"
cp "$HERE/touchscreen-livi-he0801a068.c" "$KDIR/drivers/input/touchscreen/"
grep -q touchscreen-livi-he0801a068 "$KDIR/drivers/input/touchscreen/Makefile" \
  || echo 'obj-$(CONFIG_TOUCHSCREEN_LIVI_HE0801A068) += touchscreen-livi-he0801a068.o' >>"$KDIR/drivers/input/touchscreen/Makefile"
grep -q TOUCHSCREEN_LIVI_HE0801A068 "$KDIR/drivers/input/touchscreen/Kconfig" \
  || sed -i '$s|^endif$|config TOUCHSCREEN_LIVI_HE0801A068\n\ttristate "HE0801A068 panel touchscreen"\n\tdepends on I2C\n\nendif|' \
       "$KDIR/drivers/input/touchscreen/Kconfig"
grep -q "$DTB" "$KDIR/arch/riscv/boot/dts/allwinner/Makefile" \
  || echo "dtb-\$(CONFIG_ARCH_SUNXI) += $DTB" >>"$KDIR/arch/riscv/boot/dts/allwinner/Makefile"

INITRAMFS_LIST=$OUT/initramfs.list
initramfs_common "$USERSPACE/rescue/busybox" >"$INITRAMFS_LIST"
check_initramfs_scripts "$INITRAMFS_LIST"

log "kernel config"
KM=(make -C "$KDIR" ARCH=riscv CROSS_COMPILE=$CROSS_COMPILE -j"$JOBS")
"${KM[@]}" allnoconfig >/dev/null
{
  cat "$HERE/f133.config"
  echo "CONFIG_INITRAMFS_SOURCE=\"$INITRAMFS_LIST\""
  cmdline=()
  # MEMDIAG=1: memory accounting, for measuring only. No kallsyms (overflows the boot slot): the traces
  # give addresses for this build's System.map.
  if [[ ${MEMDIAG:-0} == 1 ]]; then
    cmdline+=(page_owner=on slub_debug=U)
    printf '%s\n' '# CONFIG_SLUB_TINY is not set' CONFIG_SLUB_DEBUG=y CONFIG_DEBUG_FS=y \
      CONFIG_CMA_DEBUGFS=y CONFIG_PAGE_OWNER=y
  fi
  if ((${#cmdline[@]})); then printf 'CONFIG_CMDLINE="%s"\nCONFIG_CMDLINE_EXTEND=y\n' "${cmdline[*]}"; fi
} >"$TOP/frag.config"
(cd "$KDIR" && ARCH=riscv scripts/kconfig/merge_config.sh -m .config "$TOP/frag.config" >/dev/null)
(cd "$KDIR" && ./scripts/config $(aic8800_config))
# The AB5301B on the UART is the head unit's bluetooth, the AIC8800's own goes unused. The firmware
# stays the combined one, only fdrv's hci device is left out.
(cd "$KDIR" && ./scripts/config --disable BT)
sed -i 's/^CONFIG_SDIO_BT = y/CONFIG_SDIO_BT = n/' "$AIC_DRV/aic8800_fdrv/Makefile"
"${KM[@]}" olddefconfig >/dev/null
# A symbol with an unmet dependency is dropped silently.
while read -r line; do
  [[ ${MEMDIAG:-0} == 1 && $line == CONFIG_SLUB_TINY=y ]] && continue
  grep -qxF "$line" "$KDIR/.config" || { log "config not applied: $line"; exit 3; }
done < <(grep -E '^CONFIG_.*=y' "$HERE/f133.config")

log "make Image $DTB modules"
"${KM[@]}" Image "allwinner/$DTB" modules >/dev/null
cp "$KDIR/arch/riscv/boot/Image" "$KDIR/arch/riscv/boot/dts/allwinner/$DTB" "$OUT/"
aic8800_collect

SBI=$TOP/opensbi-$OPENSBI_VER
if [[ ! -d $SBI ]]; then
  download "https://github.com/riscv-software-src/opensbi/archive/refs/tags/v$OPENSBI_VER.tar.gz" -o "$TOP/opensbi-$OPENSBI_VER.tar.gz"
  tar -xzf "$TOP/opensbi-$OPENSBI_VER.tar.gz" -C "$TOP"
fi
log "opensbi $OPENSBI_VER"
# A changed FW_* value does not relink on its own.
rm -rf "$SBI/build"
make -C "$SBI" -j"$JOBS" CROSS_COMPILE=$CROSS_COMPILE PLATFORM=generic FW_TEXT_START=0x40000000 >/dev/null

B0=$TOP/sun20i_d1_spl
if [[ ! -d $B0 ]]; then
  git clone -q https://github.com/smaeul/sun20i_d1_spl "$B0"
  git -C "$B0" checkout -q "$BOOT0_REV"
  git -C "$B0" apply "$HERE/boot0.patch"
fi
log "boot0"
make -C "$B0" CROSS_COMPILE=$CROSS_COMPILE p=sun20iw1p1 clean >/dev/null
make -C "$B0" CROSS_COMPILE=$CROSS_COMPILE p=sun20iw1p1 spinor >/dev/null
cp "$B0/nboot/boot0_spinor_sun20iw1p1.bin" "$BOOT0IMG"
python3 "$HERE/boot0-head.py" "$BOOT0IMG"

# boot0 copies OpenSBI and the dtb to their run addresses and unpacks the LZ4 frame of the kernel,
# OpenSBI (fw_dynamic) then starts the kernel at the u-boot run address.
log "boot package for mtd1"
python3 - "$OUT/Image" "$OUT/Image.lz4" <<'PY'
import ctypes, sys
lz4 = ctypes.CDLL("liblz4.so.1")
class Prefs(ctypes.Structure):
    _fields_ = [("blockSizeID", ctypes.c_int), ("blockMode", ctypes.c_int), ("contentChecksumFlag", ctypes.c_int),
                ("frameType", ctypes.c_int), ("contentSize", ctypes.c_ulonglong), ("dictID", ctypes.c_uint),
                ("blockChecksumFlag", ctypes.c_int), ("compressionLevel", ctypes.c_int),
                ("autoFlush", ctypes.c_uint), ("favorDecSpeed", ctypes.c_uint), ("reserved", ctypes.c_uint * 3)]
# boot0 only takes independent blocks.
prefs = Prefs(blockSizeID=7, blockMode=1, compressionLevel=12)
src = open(sys.argv[1], "rb").read()
lz4.LZ4F_compressFrameBound.restype = ctypes.c_size_t
lz4.LZ4F_compressFrame.restype = ctypes.c_size_t
dst = ctypes.create_string_buffer(lz4.LZ4F_compressFrameBound(ctypes.c_size_t(len(src)), ctypes.byref(prefs)))
n = lz4.LZ4F_compressFrame(dst, ctypes.c_size_t(len(dst)), src, ctypes.c_size_t(len(src)), ctypes.byref(prefs))
assert 0 < n < len(dst)
open(sys.argv[2], "wb").write(dst.raw[:n])
PY
(( 0x40200000 + $(stat -c%s "$OUT/Image") <= 0x41000000 )) || { log "Image reaches the package buffer at 0x41000000"; exit 3; }
python3 "$HERE/mktoc.py" "$BOOTIMG" opensbi@0x40000000="$SBI/build/platform/generic/firmware/fw_dynamic.bin" \
  dtb@0x43000000="$OUT/$DTB" u-boot@0x40200000="$OUT/Image.lz4"
SIZE=$(stat -c%s "$BOOTIMG")
log "boot: $SIZE B, slot $BOOT_SIZE B, FREE $((BOOT_SIZE - SIZE)) B"
(( SIZE <= BOOT_SIZE )) || { log "boot image overflows mtd1"; exit 3; }

gst_build
log "livi (riscv64gc-unknown-linux-musl)"
RT=riscv64gc-unknown-linux-musl
# The gstreamer sys crates read their link lines from the .pc files once, a new plugin set would not link.
if [[ ${GST_REBUILT:-0} == 1 ]]; then
  for sys in gstreamer-sys gstreamer-app-sys gstreamer-audio-sys gstreamer-base-sys gstreamer-controller-sys \
             gstreamer-video-sys; do
    (cd "$REPO/native" && cargo clean -q -p $sys --profile embedded --target $RT)
  done
fi
( cd "$REPO/native" && gst_env && \
  CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_LINKER=$REALGCC \
  CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_MUSL_RUSTFLAGS="-C target-feature=+crt-static" \
  CC_riscv64gc_unknown_linux_musl=$TOP/userspace/stage/musl-$US_ARCH/bin/musl-gcc \
  AR_riscv64gc_unknown_linux_musl=${CROSS_COMPILE}ar \
  CARGO_PROFILE_EMBEDDED_STRIP=false \
  cargo build --profile embedded -p livi-os --features ab5301 --target $RT )
# With panic=abort nothing unwinds, the unwind tables are 1.3 MB of dead weight.
${CROSS_COMPILE}objcopy --strip-all -R .eh_frame -R .eh_frame_hdr -R .gcc_except_table \
  "${CARGO_TARGET_DIR:-$REPO/native/target}/$RT/embedded/livi" "$LIVID"
log "done: $BOOT0IMG, $BOOTIMG, then build-rootfs.sh for $ROOTFS_IMG"
