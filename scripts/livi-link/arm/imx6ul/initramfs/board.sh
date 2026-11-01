# i.MX6ULL: what the shared initramfs init (common/initramfs/init) does on this board.
BOARD_NAME="LIVI Link i.MX6ULL"
RESCUE_USB_DELAY=0

# The Wi-Fi driver is built into the kernel, so the Wi-Fi AP is a second way in next to USB.
board_rescue() { ( wifi-up > /dev/console 2>&1 ) & }

board_help() {
    cat <<'EOT'

no rootfs taken over. 10.10.10.1 (telnet :23, :2323) over USB-NCM and the Wi-Fi AP LIVI-Link. Tools:
  net-up [ncm|acm] / net-down    USB gadget, one function at a time
  flashcp / flash_eraseall       write and erase an MTD partition (cat /proc/mtd)
  nc -l -p 9000 > /tmp/f   receive a file over the network, or rx /tmp/f over the UART (xmodem)
  devmem ADDR       read a register
EOT
}
