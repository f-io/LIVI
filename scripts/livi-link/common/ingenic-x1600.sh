# X1600 SoC sources from letux-kernel that mainline lacks, sourced by mips/x1600/build.sh.
LETUX_ORG=${LETUX_ORG:-https://github.com/goldelico/letux-kernel}
LETUX_REF=${LETUX_REF:-letux/x1600-v15}
LETUX_CACHE=$TOP/letux-kernel

LETUX_FILES=(
  arch/mips/boot/dts/ingenic/x1600.dtsi
  drivers/clk/ingenic/x1600-cgu.c
  drivers/phy/ingenic/phy-ingenic-usb.c
  drivers/spi/spi-ingenic.c
  include/dt-bindings/clock/ingenic,x1600-cgu.h
  include/dt-bindings/interrupt-controller/x1600-irq.h
  include/dt-bindings/interrupt-controller/mips-irq.h
  include/dt-bindings/dma/x1600-dma.h
  include/dt-bindings/net/ingenic_gmac.h
  include/dt-bindings/sound/ingenic-baic.h
)

ingenic_x1600_install() {
  local f
  if [[ ! -e $LETUX_CACHE/arch/mips/boot/dts/ingenic/x1600.dtsi ]]; then
    log "fetch letux-kernel X1600 sources ($LETUX_REF)"
    rm -rf "$LETUX_CACHE"
    git clone --filter=blob:none --no-checkout "$LETUX_ORG" "$LETUX_CACHE"
    git -C "$LETUX_CACHE" sparse-checkout init --no-cone
    git -C "$LETUX_CACHE" sparse-checkout set "${LETUX_FILES[@]}"
    git -C "$LETUX_CACHE" checkout -q "$LETUX_REF"
  fi

  log "overlay the X1600 SoC dtsi, CGU driver and DT bindings"
  for f in "${LETUX_FILES[@]}"; do
    mkdir -p "$KDIR/$(dirname "$f")"
    cp "$LETUX_CACHE/$f" "$KDIR/$f"
  done

  grep -q 'x1600-livi-link.dtb' "$KDIR/arch/mips/boot/dts/ingenic/Makefile" \
    || echo 'dtb-y += x1600-livi-link.dtb' >>"$KDIR/arch/mips/boot/dts/ingenic/Makefile"

  grep -q 'INGENIC_CGU_X1600' "$KDIR/drivers/clk/ingenic/Makefile" \
    || sed -i '/^obj-\$(CONFIG_INGENIC_CGU_X1830)/a obj-$(CONFIG_INGENIC_CGU_X1600)\t\t+= x1600-cgu.o' \
      "$KDIR/drivers/clk/ingenic/Makefile"

  grep -q 'INGENIC_CGU_X1600' "$KDIR/drivers/clk/ingenic/Kconfig" \
    || sed -i '/^config INGENIC_TCU_CLK$/i config INGENIC_CGU_X1600\n\tbool "Ingenic X1600 CGU driver"\n\tdefault MACH_X1600\n\tselect INGENIC_CGU_COMMON\n\thelp\n\t  Support the clocks provided by the CGU hardware on Ingenic X1600\n\t  and compatible SoCs.\n\n\t  If building for a X1600 SoC, you want to say Y here.\n' \
      "$KDIR/drivers/clk/ingenic/Kconfig"
}
