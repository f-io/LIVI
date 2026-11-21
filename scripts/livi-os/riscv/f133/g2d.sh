# Sourced by build.sh: Allwinner's G2D RCQ driver from the radxa BSP (mainline has none), built in.
# Needs TOP, KDIR, HERE, log(), apply_patches().
G2D_ORG=${G2D_ORG:-https://github.com/radxa/allwinner-bsp}
G2D_REF=${G2D_REF:-a842bb8dc74608ba13975c97c31b9518f1976c01}
G2D_CACHE=$TOP/radxa-allwinner-bsp
G2D_UAPI=include/uapi/linux/sunxi-g2d.h
G2D_DRV=$KDIR/drivers/misc/sunxi-g2d

g2d_install() {
  if [[ ! -e $G2D_CACHE/drivers/g2d/g2d_rcq/g2d.c || ! -e $G2D_CACHE/$G2D_UAPI
        || $(git -C "$G2D_CACHE" rev-parse HEAD 2>/dev/null) != "$G2D_REF" ]]; then
    log "fetch radxa allwinner-bsp G2D driver ($G2D_REF)"
    rm -rf "$G2D_CACHE"
    git clone --filter=blob:none --no-checkout "$G2D_ORG" "$G2D_CACHE"
    git -C "$G2D_CACHE" sparse-checkout set --no-cone /drivers/g2d/ "/$G2D_UAPI"
    git -C "$G2D_CACHE" checkout -q "$G2D_REF"
  fi

  log "overlay the G2D driver into drivers/misc/sunxi-g2d"
  rm -rf "$G2D_DRV"
  cp -a "$G2D_CACHE/drivers/g2d" "$G2D_DRV"
  cp "$G2D_CACHE/$G2D_UAPI" "$KDIR/$G2D_UAPI"
  apply_patches "$HERE/g2d-patches" "$G2D_DRV"

  grep -q 'sunxi-g2d/Kconfig' "$KDIR/drivers/misc/Kconfig" \
    || sed -i '$s|^endmenu$|source "drivers/misc/sunxi-g2d/Kconfig"\nendmenu|' "$KDIR/drivers/misc/Kconfig"
  grep -q 'sunxi-g2d/' "$KDIR/drivers/misc/Makefile" \
    || echo 'obj-$(CONFIG_AW_G2D) += sunxi-g2d/' >>"$KDIR/drivers/misc/Makefile"
}
