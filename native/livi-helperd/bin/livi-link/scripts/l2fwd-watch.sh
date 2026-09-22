#!/bin/sh
# Keeps l2fwd bridging ncm0 (the host) to whichever side carries a phone: the AP while nothing is
# on the OTG port, and the iPhone's CarPlay network function while there is. One bridge at a time,
# because running both would put every wired frame out over the air as well.
#
# Interface selection is not a guess: the iAP2 identification announces
# car_play_interface_number 3, and a working session on a Linux host (reference capture
# 2026-09-05) runs CarPlay on exactly that USB interface — bInterfaceNumber 03, driver
# cdc_ncm. The dongle's old kernel binds the phone's second NCM function (interface 5) too,
# so selecting by carrier picks the wrong netdev.
#
# The vendor cdc_ncm binds at enumeration and leaves the data interface on alt 0, where it has
# no endpoints ("Delay load NCM" / "link is not ready"), so the link never comes up. Re-binding
# after the CarPlay config switch fixes it — measured: alt 0 -> 1, carrier 0 -> 1, 100 mbit/s.
PATH=/bin:/sbin:/usr/bin:/usr/sbin; export PATH
RUN=/tmp/livi; LAST=""; REBIND_AT=0; REBINDS=0
# Repeated unbind/rebind is hard on this kernel's cdc_ncm, so give up after a few tries and
# wait for the phone to re-enumerate. The counter resets when the phone goes or the link comes up.
MAX_REBINDS=3

# Netdev of the CarPlay NCM function, by USB interface number.
carplay_iface() {
  for n in /sys/class/net/*; do
    d=$(readlink -f "$n/device" 2>/dev/null) || continue
    [ "$(cat "$d/bInterfaceNumber" 2>/dev/null)" = "03" ] || continue
    basename "$n"
    return 0
  done
  return 1
}

# Its USB control interface (e.g. 1-1:6.3) on an Apple device.
carplay_ctrl() {
  for i in /sys/bus/usb/devices/*:*; do
    [ "$(cat "$i/bInterfaceNumber" 2>/dev/null)" = "03" ] || continue
    [ "$(cat "$i/bInterfaceClass" 2>/dev/null)" = "02" ] || continue
    [ "$(cat "${i%%:*}/idVendor" 2>/dev/null)" = "05ac" ] || continue
    basename "$i"
    return 0
  done
  return 1
}

rebind_ncm() {
  ctrl=$1; base=${ctrl%.*}; num=${ctrl##*.}; data="$base.$((num + 1))"
  for x in "$data" "$ctrl"; do echo -n "$x" > /sys/bus/usb/drivers/cdc_ncm/unbind 2>/dev/null; done
  sleep 1
  echo -n "$ctrl" > /sys/bus/usb/drivers/cdc_ncm/bind 2>/dev/null
  echo "$(date +%T) rebound $ctrl" >> /tmp/l2fwd-watch.log
}

while true; do
  SEL=$(carplay_iface)
  [ -z "$SEL" ] && REBINDS=0   # phone gone: the next one starts with a fresh budget
  if [ -n "$SEL" ]; then
    ip link set "$SEL" up 2>/dev/null
    echo 0 > /proc/sys/net/ipv6/conf/$SEL/accept_dad 2>/dev/null
    echo 0 > /proc/sys/net/ipv6/conf/ncm0/accept_dad 2>/dev/null

    if [ "$(cat /sys/class/net/$SEL/carrier 2>/dev/null)" = "1" ]; then
      REBINDS=0
    else
      # Data path asleep: re-bind, at most every 10 s and only a few times.
      NOW=$(cut -d. -f1 /proc/uptime)
      if [ $((NOW - REBIND_AT)) -ge 10 ] && [ $REBINDS -lt $MAX_REBINDS ]; then
        REBIND_AT=$NOW
        REBINDS=$((REBINDS + 1))
        CTRL=$(carplay_ctrl) && rebind_ncm "$CTRL"
        sleep 2
        SEL=$(carplay_iface)
        [ -n "$SEL" ] && ip link set "$SEL" up 2>/dev/null
      fi
    fi
  fi

  # A phone on the OTG port takes the bridge, otherwise it belongs to the AP.
  PARTNER=${SEL:-wlan0}
  IDX=$(cat /sys/class/net/$PARTNER/ifindex 2>/dev/null)
  if [ -n "$IDX" ]; then
    # l2fwd binds its AF_PACKET sockets by ifindex, and a re-bind gives the netdev a new one,
    # so key the restart on name AND index.
    if [ "$PARTNER:$IDX" != "$LAST" ] || ! ps | grep -v grep | grep -q "[l]2fwd $PARTNER"; then
      pkill -f "$RUN/l2fwd" 2>/dev/null; sleep 0.3
      setsid "$RUN/l2fwd" "$PARTNER" ncm0 >/tmp/l2fwd.log 2>&1 &
      LAST="$PARTNER:$IDX"
      echo "$(date +%T) l2fwd on $PARTNER($IDX) carrier=$(cat /sys/class/net/$PARTNER/carrier 2>/dev/null)" >> /tmp/l2fwd-watch.log
    fi
  fi
  sleep 1
done
