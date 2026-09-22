#!/bin/sh
# The way into a stock dongle, sourced from /etc/profile because a file written over the wire has
# no execute bit. It has to claim the gadget before the vendor does, or the host never sees NCM.
(
  # rcS mounts /tmp right after it sources the profile.
  sleep 4
  # The vendor's own switch against starting its projection.
  touch /tmp/UDiskPassThroughMode

  tar -xf /script/ko.tar.gz -C /tmp 2>/dev/null
  grep -q g_android_accessory /proc/modules || {
    insmod /tmp/storage_common.ko 2>/dev/null
    insmod /tmp/g_android_accessory.ko 2>/dev/null
  }

  A=/sys/class/android_usb_accessory/android0
  i=0
  while [ ! -e "$A/enable" ] && [ $i -lt 50 ]; do i=$((i + 1)); sleep 0.1; done
  [ -e "$A/enable" ] || exit 0

  echo 0 > "$A/enable"

  printf f-io.dev > "$A/iManufacturer"
  printf 'LIVI Link' > "$A/iProduct"
  echo 239 > "$A/bDeviceClass"
  echo 2 > "$A/bDeviceSubClass"
  echo 1 > "$A/bDeviceProtocol"
  echo ncm > "$A/functions"
  echo 1 > "$A/enable"
  sleep 2

  [ -e /sys/class/net/ncm0 ] && ifconfig ncm0 hw ether c2:8e:30:53:48:01
  ifconfig ncm0 10.10.10.1 netmask 255.255.255.0 mtu 1500 up
  printf 'start 10.10.10.100\nend 10.10.10.200\ninterface ncm0\nopt subnet 255.255.255.0\nopt lease 86400\nlease_file /tmp/livi-udhcpd.leases\npidfile /tmp/livi-udhcpd.pid\nmax_leases 20\n' > /tmp/livi-udhcpd.conf
  touch /tmp/livi-udhcpd.leases
  busybox udhcpd /tmp/livi-udhcpd.conf
  busybox telnetd -l /bin/sh -p 2323
) &
