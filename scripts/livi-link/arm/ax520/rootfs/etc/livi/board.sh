# AX520 + AIC8800D80: what rcS and livi-radio do on this board and not on the others.

# The MFi chip sits on the i2c-gpio bus (stock softi2c2: SCL GPIO0_26, SDA GPIO0_27), the DT alias
# makes that /dev/i2c-3.
MFI_I2C=3

# WiFi power: the module enable (GPIO0_24) goes high first.
# "high" as the direction sets the level together with the output, so the pin never drives low: low on
# this pin reset the whole board once. Only then the SDIO controller comes on (an overlay), and the
# polled slot finds the module.
board_wifi() {
    GB=
    for c in /sys/class/gpio/gpiochip*; do
        case "$(readlink -f "$c")" in *9040000*) GB=$(cat "$c/base") ;; esac
    done
    if [ -n "$GB" ]; then
        g=$((GB + 24))
        [ -e /sys/class/gpio/gpio$g ] || echo $g > /sys/class/gpio/export
        echo high > /sys/class/gpio/gpio$g/direction
        echo "[livi] wifi_en GPIO0_24 (gpio$g) high"
        sleep 0.2
        echo sdio0 > /sys/firmware/ax520/overlay && echo '[livi] sdio0 on'
        for i in $(seq 1 20); do
            ls /sys/bus/sdio/devices/* >/dev/null 2>&1 && break
            sleep 0.25
        done
        echo "[livi] sdio: $(ls /sys/bus/sdio/devices 2>/dev/null | tr '\n' ' ')"
    fi
    if [ -e /sys/class/mmc_host/mmc0 ]; then
        echo '[livi] insmod aic8800_bsp'
        insmod /lib/modules/$(uname -r)/aic8800_bsp.ko 2>&1 | tail -1
        sleep 1
        echo '[livi] insmod aic8800_fdrv'
        insmod /lib/modules/$(uname -r)/aic8800_fdrv.ko aicwf_dbg_level=1 2>&1 | tail -1
    else
        echo '[livi] no SDIO host, WiFi skipped'
    fi
}
