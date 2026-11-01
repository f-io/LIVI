# X1600 + AIC8800D80: what rcS and livi-radio do on this board and not on the others.
MFI_I2C=0

board_wifi() {
  for _ in $(seq 1 20); do
    ls /sys/bus/sdio/devices/* >/dev/null 2>&1 && break
    sleep 0.25
  done
  if [ -e /sys/class/mmc_host/mmc1 ]; then
    echo "[livi] sdio: $(ls /sys/bus/sdio/devices 2>/dev/null | tr '\n' ' ')"
    echo '[livi] insmod aic8800_bsp'
    insmod "/lib/modules/$(uname -r)/aic8800_bsp.ko" 2>&1 | tail -1
    sleep 1
    echo '[livi] insmod aic8800_fdrv'
    insmod "/lib/modules/$(uname -r)/aic8800_fdrv.ko" aicwf_dbg_level=1 2>&1 | tail -1
  else
    echo '[livi] no SDIO host, WiFi skipped'
  fi
}
