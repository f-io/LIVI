BOARD_NAME=F133
RESCUE_USB_DELAY=0

# EHCI0 claims the USB-A port as host when it probes, the NCM gadget needs it back.
for m in /sys/devices/platform/soc/4100000.usb/musb-hdrc.*/mode; do echo peripheral > $m; done

board_rescue() { :; }

board_help() {
    cat <<'EOT'

no rootfs taken over. The shell is 10.10.10.1 (telnet :23, :2323) over USB-NCM. Tools:
  net-up [ncm|acm]  USB gadget, one function at a time, net-down first to switch
  nc -l -p 9000 > /tmp/f   receive a file over the network
  devmem 0x07090108 32 0x5AA5A55A; reboot -f   boot0 enters FEL
  dmesg, /proc/mtd, /sys/kernel/debug
EOT
}
