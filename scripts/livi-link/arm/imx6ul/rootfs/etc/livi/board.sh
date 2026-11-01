# i.MX6ULL: what rcS and livi-radio do on this board and not on the others.

# The MFi chip answers at 0x11 on the bus of the UART5 pads.
MFI_I2C=1

# The Wi-Fi driver is built into the kernel and powers the module itself.
board_wifi() {
    for i in $(seq 1 20); do
        { [ -e /sys/class/net/uap0 ] || [ -e /sys/class/net/wlan0 ]; } && break
        sleep 0.5
    done
    [ -e /sys/class/net/uap0 ] && ip link set uap0 name wlan0 && echo '[livi] uap0 is wlan0'
    M=/lib/modules/$(uname -r)
    for m in $(cat $M/load); do
        insmod $M/$m.ko && echo "[livi] insmod $m"
    done
}
