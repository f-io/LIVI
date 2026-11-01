# AX520: what the shared initramfs init (common/initramfs/init) does on this board.
BOARD_NAME=AX520
RESCUE_USB_DELAY=15

board_rescue() { :; }

board_help() {
    cat <<'EOT'

no rootfs taken over. Shell on the UART, 10.10.10.1 (telnet :23, :2323) over USB-NCM after 15 s. Tools:
  ovl [NAME]        peripheral overlays: sdio0 (only once wifi_en, GPIO0_24, is high)
  led-test RRGGBB   send a colour to the RGB LED
  net-up [ncm|acm]  USB gadget, one function at a time (four endpoints), started 15 s after boot, net-down first to switch
  nousb             skip that automatic net-up (within the 15 s)
  flash-mtd boot|rootfs FILE [-y]   write an image to that MTD, verified (without -y: dry run), flash-boot FILE = boot
  sfc-sr [qe]       flash status registers, qe sets the quad enable bit the bootloader needs
  nc -l -p 9000 > /tmp/f   receive a file over the network, or rx /tmp/f over the UART (xmodem)
  devmem ADDR       read a register, e.g. devmem 0x0b500000
  diag              state overview; dmesg, /proc/uptime, /sys/kernel/debug
EOT
}
