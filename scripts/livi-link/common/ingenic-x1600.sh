# Sourced by mips/x1600/build.sh: X1600 SoC bring-up not in kernel.org mainline, from the
# letux-kernel fork (github.com/goldelico/letux-kernel). Needs TOP, KDIR, COMMON, log().
LETUX_ORG=${LETUX_ORG:-https://github.com/goldelico/letux-kernel}
LETUX_REF=${LETUX_REF:-letux/x1600-v15}
LETUX_CACHE=$TOP/letux-kernel

# pinctrl-ingenic.c and dwc2 params.c are not here: both already have X1600 entries in mainline.
LETUX_FILES="arch/mips/boot/dts/ingenic/x1600.dtsi
             drivers/clk/ingenic/x1600-cgu.c
             drivers/phy/ingenic/phy-ingenic-usb.c
             drivers/spi/spi-ingenic.c
             include/dt-bindings/clock/ingenic,x1600-cgu.h
             include/dt-bindings/interrupt-controller/x1600-irq.h
             include/dt-bindings/interrupt-controller/mips-irq.h
             include/dt-bindings/dma/x1600-dma.h
             include/dt-bindings/net/ingenic_gmac.h
             include/dt-bindings/sound/ingenic-baic.h"

ingenic_x1600_install() {
  if [[ ! -e $LETUX_CACHE/arch/mips/boot/dts/ingenic/x1600.dtsi ]]; then
    log "fetch letux-kernel X1600 sources ($LETUX_REF)"
    rm -rf "$LETUX_CACHE"
    # Not --sparse: that flag starts cone mode, which rejects a pattern list of individual files
    # (it wants directories). --no-cone takes the file list as-is.
    git clone --filter=blob:none --no-checkout "$LETUX_ORG" "$LETUX_CACHE"
    git -C "$LETUX_CACHE" sparse-checkout init --no-cone
    git -C "$LETUX_CACHE" sparse-checkout set $LETUX_FILES
    git -C "$LETUX_CACHE" checkout -q "$LETUX_REF"
  fi

  log "overlay the X1600 SoC dtsi, CGU driver and DT bindings"
  cp "$LETUX_CACHE/arch/mips/boot/dts/ingenic/x1600.dtsi" "$KDIR/arch/mips/boot/dts/ingenic/"
  # The board dtb is built unconditionally (dtb-y), unlike the per-board Kconfig symbols upstream
  # boards use (CONFIG_X1000_CU1000_NEO and friends): this tree only ever has the one board.
  grep -q 'x1600-livi-link.dtb' "$KDIR/arch/mips/boot/dts/ingenic/Makefile" || \
    echo 'dtb-y += x1600-livi-link.dtb' >> "$KDIR/arch/mips/boot/dts/ingenic/Makefile"
  cp "$LETUX_CACHE/drivers/clk/ingenic/x1600-cgu.c" "$KDIR/drivers/clk/ingenic/"
  # Taken whole, not a patch: the PHY tuning values are register magic from Ingenic's own reference
  # code, not ours to transcribe by hand.
  cp "$LETUX_CACHE/drivers/phy/ingenic/phy-ingenic-usb.c" "$KDIR/drivers/phy/ingenic/"
  # Also whole: includes a clock-rate fix, a DMA-direction fix and an SSICDR register poke (a
  # divider-specific stop bit our x1600-cgu.c's gate clock doesn't cover). Safe unconditional: this
  # tree only ever builds for the X1600.
  cp "$LETUX_CACHE/drivers/spi/spi-ingenic.c" "$KDIR/drivers/spi/"
  # x1600.dtsi's own #includes, none in mainline (not found by eye — a real dtc run turned these up).
  mkdir -p "$KDIR/include/dt-bindings/clock" "$KDIR/include/dt-bindings/interrupt-controller" \
    "$KDIR/include/dt-bindings/dma" "$KDIR/include/dt-bindings/net" "$KDIR/include/dt-bindings/sound"
  cp "$LETUX_CACHE/include/dt-bindings/clock/ingenic,x1600-cgu.h" "$KDIR/include/dt-bindings/clock/"
  cp "$LETUX_CACHE/include/dt-bindings/interrupt-controller/x1600-irq.h" \
    "$LETUX_CACHE/include/dt-bindings/interrupt-controller/mips-irq.h" \
    "$KDIR/include/dt-bindings/interrupt-controller/"
  cp "$LETUX_CACHE/include/dt-bindings/dma/x1600-dma.h" "$KDIR/include/dt-bindings/dma/"
  cp "$LETUX_CACHE/include/dt-bindings/net/ingenic_gmac.h" "$KDIR/include/dt-bindings/net/"
  cp "$LETUX_CACHE/include/dt-bindings/sound/ingenic-baic.h" "$KDIR/include/dt-bindings/sound/"

  grep -q 'INGENIC_CGU_X1600' "$KDIR/drivers/clk/ingenic/Makefile" || \
    sed -i '/^obj-\$(CONFIG_INGENIC_CGU_X1830)/a obj-$(CONFIG_INGENIC_CGU_X1600)\t\t+= x1600-cgu.o' \
      "$KDIR/drivers/clk/ingenic/Makefile"

  # Same shape as X1000/X1830 above it; "default MACH_X1600" is harmless, nothing defines that
  # symbol, so --enable INGENIC_CGU_X1600 below does the real work.
  grep -q 'INGENIC_CGU_X1600' "$KDIR/drivers/clk/ingenic/Kconfig" || \
    sed -i '/^config INGENIC_TCU_CLK$/i config INGENIC_CGU_X1600\n\tbool "Ingenic X1600 CGU driver"\n\tdefault MACH_X1600\n\tselect INGENIC_CGU_COMMON\n\thelp\n\t  Support the clocks provided by the CGU hardware on Ingenic X1600\n\t  and compatible SoCs.\n\n\t  If building for a X1600 SoC, you want to say Y here.\n' \
      "$KDIR/drivers/clk/ingenic/Kconfig"
}
