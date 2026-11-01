# V821B: what the shared initramfs init (common/initramfs/init) does on this board.
BOARD_NAME=V821B
RESCUE_USB_DELAY=0

board_rescue() { :; }

board_help() {
    cat <<'EOT'

no rootfs taken over. The console (hvc0) only prints, the shell is 10.10.10.1 (telnet :23, :2323)
over USB-NCM. Tools:
  net-up [ncm|acm]  USB gadget, one function at a time, net-down first to switch
  nc -l -p 9000 > /tmp/f   receive a file over the network
  devmem ADDR       read a register
  dmesg, /proc/mtd, /sys/kernel/debug
EOT
}
