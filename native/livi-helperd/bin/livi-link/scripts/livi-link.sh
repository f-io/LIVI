#!/bin/sh
# LIVI Link relay stack. Installed at /script/livi/livi-link.sh, run by livi-bringup.sh at boot;
# re-run by hand to restart the stack without a reboot (--fresh re-unpacks the binary).
# One binary lives gzipped on jffs2 (the per-device stack .gz), unpacked into tmpfs (/tmp/livi)
# and linked under each tool name, busybox-style — it picks its job from argv[0]:
#   seedrng        feeds the kernel entropy pool (3.14 has no getrandom; TLS blocks without it)
#   mfid           MFi coprocessor on i2c-1, served over TCP :5000
#   wifid          the access point, configured by the host over TCP :5001
#   btd            the Bluetooth controller, handed to the host over TCP :5002
#   iapd           the Bluetooth accessory itself: pairs, serves iAP over TCP :5004
#   livi-usbproxy  the iPhone's USB side (enumerate, config, bulk pipes) on TCP :5003
#   l2fwd          L2 bridge iPhone-NCM (usbN) <-> ncm0 (host); started by l2fwd-watch.sh
#   mdnsd          answers livi-link.local on ncm0 (host) and wlan0 (AP), each with its own address
#   httpd          the LIVI web UI + control API on :80 (shared livi-web); vendor boa is the fallback
PATH=/bin:/sbin:/usr/bin:/usr/sbin; export PATH
SRC=/script/livi; RUN=/tmp/livi
log(){ echo "[link] $*" > /dev/console 2>/dev/null; echo "[link] $*"; }
alive(){ ps | grep -v grep | grep -q "$1"; }

# pkill only signals; on this single-core box the daemons can still be scheduled out when the
# next check runs, which used to make start() skip them as "already alive" right before they
# died. So wait until they are really gone.
reap(){ i=0
  while [ $i -lt 30 ] && ps | grep -v grep | grep -q "$1"; do
    pkill -f "$1" 2>/dev/null; i=$((i+1)); sleep 0.2
  done; }

fresh=
[ "$1" = "--fresh" ] && { fresh=1; reap "$RUN/"; reap l2fwd-watch; rm -rf "$RUN"; }
mkdir -p "$RUN"
if [ ! -x "$RUN/livi-link" ]; then
  gz=$(ls "$SRC"/*.gz 2>/dev/null | head -n1)
  if [ -n "$gz" ]; then
    gunzip -c "$gz" > "$RUN/livi-link.tmp" && chmod 755 "$RUN/livi-link.tmp" \
      && mv "$RUN/livi-link.tmp" "$RUN/livi-link" || log "unpack $gz failed"
  else
    log "no stack .gz in $SRC"
  fi
fi

if [ -n "$fresh" ] && [ -x "$RUN/livi-link" ]; then
  "$RUN/livi-link" sync-scripts && exec sh "$SRC/livi-link.sh"
fi

for b in seedrng mfid wifid btd iapd ledd livi-usbproxy l2fwd mdnsd httpd; do
  [ -L "$RUN/$b" ] || ln -sf livi-link "$RUN/$b"
done

# iPhone NCM (config 6) needs the kernel cdc_ncm; it ships in the vendor module tarball.
grep -q cdc_ncm /proc/modules || {
  [ -f /tmp/cdc_ncm.ko ] || tar -xzf /script/ko.tar.gz -C /tmp cdc_ncm.ko 2>/dev/null
  insmod /tmp/cdc_ncm.ko 2>/dev/null
}

# Bring the stack to a known state
start(){ name=$1; shift; reap "$1"
  plain=$(echo "$name" | tr -d '[]')
  for try in 1 2 3; do
    setsid "$@" </dev/null >/tmp/$(basename "$1").log 2>&1 &
    sleep 1
    alive "$name" && { log "$plain up"; return 0; }
  done
  log "$plain FAILED to start"; return 1; }

start '[s]eedrng' "$RUN/seedrng"
start '[m]fid' "$RUN/mfid" /dev/i2c-1
start '[w]ifid' "$RUN/wifid"
start '[b]td' "$RUN/btd"
start '[i]apd' "$RUN/iapd"
start '[l]edd' "$RUN/ledd"
start '[m]dnsd' "$RUN/mdnsd" livi-link ncm0 wlan0

# Web UI
reap boa
if ! start '[h]ttpd' "$RUN/httpd"; then
  log "httpd down — falling back to vendor boa so a recovery UI stays up"
  mkdir -p /tmp/boa/logs
  [ -d /tmp/boa/www ] || cp -r /etc/boa/www /tmp/boa/
  [ -d /tmp/boa/cgi-bin ] || cp -r /etc/boa/cgi-bin /tmp/boa/
  start '[b]oa' /usr/sbin/boa
fi

# usbproxy and the bridge watcher are restarted on every run; a running host session reconnects.
reap "$RUN/livi-usbproxy"; reap l2fwd-watch; reap "$RUN/l2fwd"
start '[l]ivi-usbproxy' "$RUN/livi-usbproxy"
setsid sh "$SRC/l2fwd-watch.sh" </dev/null >/dev/null 2>&1 &
log "stack: $(ps | grep -v grep | grep -c -E "$RUN/|l2fwd-watch") processes"
