# V821B + AIC8800D80: what rcS and livi-radio do on this board and not on the others.

# The MFi chip on twi1, the DT alias makes that /dev/i2c-1.
MFI_I2C=1

# The kernel powers the module (reg_wlan, wlan_pwrseq) and finds it on the SDIO slot while booting.
board_wifi() {
    if ls /sys/bus/sdio/devices/* >/dev/null 2>&1; then
        echo '[livi] insmod aic8800_bsp'
        insmod /lib/modules/$(uname -r)/aic8800_bsp.ko 2>&1 | tail -1
        sleep 1
        echo '[livi] insmod aic8800_fdrv'
        insmod /lib/modules/$(uname -r)/aic8800_fdrv.ko aicwf_dbg_level=1 2>&1 | tail -1
    else
        echo '[livi] no SDIO card, WiFi skipped'
    fi
}
