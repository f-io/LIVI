#!/bin/sh
# LIVI bridge bring-up (IW416 / i.MX6UL)
# NCM-over-USB: accessory gadget = ncm as the SOLE/first gadget (=> ncm0), brought up EARLY
# with UDiskPassThroughMode set, fixed MAC, and udhcpd so the host auto-configures.
# rcS has already mounted proc/sys/tmp + mdev before this runs.

PATH=/bin:/sbin:/usr/bin:/usr/sbin:/tmp/bin; export PATH
log(){ echo "[livi] $*" > /dev/console 2>/dev/null; echo "[livi] $*"; }
# The name the AP falls back to when this boot gave up, so there is a known one to look for.
AP_DEFAULT='LIVI Link'

rm -f /dev/random && ln -s /dev/urandom /dev/random
mkdir -p /tmp/bin

# Root shell first, before anything that can hang: it binds on whatever interface turns up later
# and is the way back in if the rest of this script fails.
busybox telnetd -l /bin/sh -p 2323

# Access watchdog. If neither the USB link nor the AP is up there is no way back in, so put the
# previous boot script back and reboot. Measured 12s from boot to access, so 30s leaves room
# without making anyone wait. While either one works it does nothing, and without a saved boot
# script it does nothing either, rather than reboot in a loop.
( sleep 30
  [ -e /tmp/livi_ok ] && exit 0
  [ -e /script/start_main_service.sh.orig ] || exit 0
  # The AP carries whatever name was last saved, which is no help to someone looking for a dongle
  # that just gave up. Put the default back before handing over.
  if [ -f /etc/hostapd.conf ]; then
    { grep -v '^ssid=' /etc/hostapd.conf; echo "ssid=$AP_DEFAULT"; } > /etc/hostapd.conf.new \
      && mv /etc/hostapd.conf.new /etc/hostapd.conf
  fi
  cp /script/start_main_service.sh.orig /script/start_main_service.sh
  sync; reboot -f
) &
WATCHDOG=$!

# --- GPIO: BT reset (gpio1), charge (gpio6/7), power LED (gpio2) ---
echo 1 > /sys/class/gpio/export 2>/dev/null
echo out > /sys/class/gpio/gpio1/direction 2>/dev/null
echo 1 > /sys/class/gpio/gpio1/value 2>/dev/null; sleep 0.1; echo 0 > /sys/class/gpio/gpio1/value 2>/dev/null
for g in 6 7 2; do echo $g > /sys/class/gpio/export 2>/dev/null; echo out > /sys/class/gpio/gpio$g/direction 2>/dev/null; done
echo 0 > /sys/class/gpio/gpio6/value 2>/dev/null; sleep 0.1; echo 1 > /sys/class/gpio/gpio6/value 2>/dev/null
echo 1 > /sys/class/gpio/gpio7/value 2>/dev/null
echo 0 > /sys/class/gpio/gpio2/value 2>/dev/null

# --- NCM over USB (host IP link) FIRST, so it is the sole gadget => ncm0 ---
log "NCM gadget (accessory=ncm, ncm0)"
tar -xzf /script/ko.tar.gz -C /tmp 2>/dev/null
touch /tmp/UDiskPassThroughMode
grep -q storage_common /proc/modules || insmod /tmp/storage_common.ko 2>/dev/null
grep -q g_android_accessory /proc/modules || insmod /tmp/g_android_accessory.ko 2>/dev/null
A=/sys/class/android_usb_accessory/android0
i=0; while [ ! -e "$A/enable" ] && [ $i -lt 50 ]; do i=$((i+1)); sleep 0.1; done
if [ -e "$A/enable" ]; then
  echo 0 > "$A/enable"
  printf f-io.dev > "$A/iManufacturer"
  printf 'LIVI Link' > "$A/iProduct"
  printf 1314 > "$A/idVendor"
  printf 1520 > "$A/idProduct"
  echo 239 > "$A/bDeviceClass"; echo 2 > "$A/bDeviceSubClass"; echo 1 > "$A/bDeviceProtocol"
  echo ncm > "$A/functions"
  echo 1 > "$A/enable"
  sleep 1
  [ -e /sys/class/net/ncm0 ] && ifconfig ncm0 hw ether c2:8e:30:53:48:01 2>/dev/null
  ifconfig ncm0 10.10.10.1 netmask 255.255.255.0 mtu 1500 up
  cat > /tmp/udhcpd_ncm.conf <<CFG
start 10.10.10.100
end 10.10.10.149
interface ncm0
opt subnet 255.255.255.0
opt lease 86400
lease_file /tmp/udhcpd_ncm.leases
pidfile /tmp/udhcpd_ncm.pid
max_leases 100
CFG
  touch /tmp/udhcpd_ncm.leases
  busybox udhcpd -f /tmp/udhcpd_ncm.conf >/tmp/udhcpd_ncm.log 2>&1 &
  log "ncm0 10.10.10.1 + udhcpd; state=$(cat $A/state)"
fi

# --- LIVI Link relay stack: seedrng, mfid (:5000), wifid (:5001), usbproxy (:5003), bridge ---
[ -f /script/livi/livi-link.sh ] && { sh /script/livi/livi-link.sh; log "LIVI Link stack started"; }

# --- WiFi (IW416, sdioCardID 0x9159): mlan + moal ---
log "WiFi mlan/moal"
tar -xf /lib/firmware/nxp/iw416_ko.tar.gz -C /tmp 2>/dev/null
insmod /tmp/mlan.ko 2>/dev/null
insmod /tmp/moal.ko mod_para=nxp/wifi_mod_para.conf 2>/dev/null
i=0; while [ ! -e /sys/class/net/wlan0 ] && [ $i -lt 60 ]; do sleep 0.1; i=$((i+1)); done

# --- WiFi AP ---
# Own address and own DHCP for the phones, disjoint from the range ncm0 serves.
ifconfig wlan0 10.10.10.2 netmask 255.255.255.0 up 2>/dev/null
hostapd /etc/hostapd.conf -B 2>/dev/null
cat > /tmp/udhcpd_ap.conf <<CFG
start 10.10.10.150
end 10.10.10.199
interface wlan0
opt subnet 255.255.255.0
opt lease 86400
lease_file /tmp/udhcpd_ap.leases
pidfile /tmp/udhcpd_ap.pid
max_leases 50
CFG
touch /tmp/udhcpd_ap.leases
busybox udhcpd -f /tmp/udhcpd_ap.conf >/tmp/udhcpd_ap.log 2>&1 &
log "wlan0 10.10.10.2 + udhcpd for the AP"

if [ -e /sys/class/net/ncm0 ] || ps | grep -v grep | grep -q hostapd; then
  touch /tmp/livi_ok
  kill $WATCHDOG 2>/dev/null
  log "access up (ncm0 and/or AP), watchdog stood down"
else
  log "WARNING: neither ncm0 nor the AP came up, watchdog will restore the previous boot script"
fi

# --- BT (IW416): hci_uart + firmware + hciattach ---
log "BT hci_uart+fw+hciattach"
insmod /tmp/hci_uart.ko 2>/dev/null
fw_loader_linux /dev/ttymxc2 115200 1 /lib/firmware/nxp/uartiw416_bt_v0.bin 3000000
hciattach /dev/ttymxc2 any 3000000 flow
hciconfig hci0 up 2>/dev/null
hciconfig hci0 reset 2>/dev/null
hciconfig hci0 scomtu 240:32 2>/dev/null
hcitool -i hci0 cmd 0x3f 0x1d 0x00 2>/dev/null

log "bring-up complete (no CarlinKit userspace)"
