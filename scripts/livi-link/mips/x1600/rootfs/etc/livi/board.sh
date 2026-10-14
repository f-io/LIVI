# Ingenic X1600 + AIC8800D80: what rcS and livi-radio do on this board and not on the others.

# The DT alias makes the real I2C0 controller /dev/i2c-0 — not confirmed this is where the MFi
# chip actually sits (see x1600-livi-link.dts's &i2c0 comment), might be a soft-i2c GPIO pair
# instead, like AX520's. Unset this (and drop &i2c0 from the DTS) if nothing answers there.
MFI_I2C=0

# Unlike AX520, WiFi power (PA29/PA30) is sequenced by the kernel itself (mmc-pwrseq-simple on
# mmc1 in x1600-livi-link.dts), not toggled here: the SDIO host just needs a moment to scan once
# the pwrseq has brought the chip up.
board_wifi() {
    for i in $(seq 1 20); do
        ls /sys/bus/sdio/devices/* >/dev/null 2>&1 && break
        sleep 0.25
    done
    if [ -e /sys/class/mmc_host/mmc1 ]; then
        echo "[livi] sdio: $(ls /sys/bus/sdio/devices 2>/dev/null | tr '\n' ' ')"
        echo '[livi] insmod aic8800_bsp'
        insmod /lib/modules/$(uname -r)/aic8800_bsp.ko 2>&1 | tail -1
        sleep 1
        echo '[livi] insmod aic8800_fdrv'
        insmod /lib/modules/$(uname -r)/aic8800_fdrv.ko aicwf_dbg_level=1 2>&1 | tail -1
    else
        echo '[livi] no SDIO host, WiFi skipped'
    fi
}
