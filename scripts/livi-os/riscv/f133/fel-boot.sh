#!/usr/bin/env bash
# Recovery from FEL: writes boot0 and the boot package to NOR and resets into them. The rootfs (mtd2) is
# flashed from the running system.
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
X=${XFEL:-xfel}
VERIFY=$TOP/fel-verify.bin

# A boot that does not reach the rootfs returns to FEL by itself (boot0's rtc[3] mark and watchdog), a
# running system reboots there with
#   devmem 0x07090108 32 0x5AA5A55A; reboot -f
lsusb -d 1f3a:efe8 >/dev/null || { echo "no device in FEL"; exit 1; }
sudo "$X" version
for img in "0x0 $BOOT0IMG" "0x10000 $BOOTIMG"; do
  set -- $img
  sudo "$X" spinor write "$1" "$2" | tr '\r' '\n' | tail -1
  sudo rm -f "$VERIFY"
  sudo "$X" spinor read "$1" "$(stat -c%s "$2")" "$VERIFY" >/dev/null
  cmp "$VERIFY" "$2"
done
sudo "$X" reset
