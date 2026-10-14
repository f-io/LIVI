# Ingenic X1600: what the shared initramfs init (common/initramfs/init) does on this board.
BOARD_NAME="LIVI Link X1600"
RESCUE_USB_DELAY=15

# AIC8800D80 is modules, not built in (common/aic8800.sh), so there is nothing to bring up for a
# rescue-only WiFi AP here, the same reason ax520 has no board_rescue of its own either.
board_rescue() { :; }

board_help() {
    cat <<'EOT'

no rootfs taken over. Shell on the UART, 10.10.10.1 (telnet :23, :2323) over USB-NCM after 15 s. Tools:
  net-up [ncm|acm] / net-down    USB gadget, one function at a time
  flashcp / flash_eraseall       write and erase an MTD partition (cat /proc/mtd)
  nc -l -p 9000 > /tmp/f   receive a file over the network, or rx /tmp/f over the UART (xmodem)
  devmem ADDR       read a register
EOT
}
