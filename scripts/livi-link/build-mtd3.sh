#!/usr/bin/env bash
# Assemble the LIVI-Link (V821B) rootfs squashfs (mtd3) from our own builds:
# userspace binaries from build-userspace.sh ($USERSPACE), Rust livid from
# native/livi-helperd, kernel modules + AIC8800 firmware from rootfs-overlay/.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
USERSPACE=${USERSPACE:-$HOME/LocalDev/livi-userspace-build/out}
HELPERD=$(cd "$HERE/../../native/livi-helperd" && pwd)
OUT_DIR=${OUT_DIR:-$HOME/LocalDev/tina-test/out}
MTD3_SIZE=$((0x480000))  # 4718592 B = 4.5 MiB

log(){ printf '\033[1;36m[livi-mtd3]\033[0m %s\n' "$*"; }
if [[ ! -x $USERSPACE/bin/busybox ]]; then
    log "USERSPACE not built at $USERSPACE — run build-userspace.sh first"
    exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

log "assembling in $WORK"
mkdir -p "$WORK"/{bin,sbin,lib,usr/{bin,sbin,lib},etc/init.d,dev,proc,sys,tmp,root,mnt}

# --- Our own userspace: built from source by build-userspace.sh ---
log "cp busybox + musl runtime (from $USERSPACE)"
cp    "$USERSPACE/bin/busybox"              "$WORK/bin/busybox"
cp    "$USERSPACE/lib/libc.so"              "$WORK/lib/libc.so"
cp -P "$USERSPACE/lib/ld-musl-riscv32.so.1" "$WORK/lib/"

log "cp hostapd + libnl3"
cp    "$USERSPACE/usr/sbin/hostapd"          "$WORK/usr/sbin/"
mkdir -p "$WORK/usr/lib"
cp -P "$USERSPACE/usr/lib"/libnl-3.so*       "$WORK/usr/lib/"
cp -P "$USERSPACE/usr/lib"/libnl-genl-3.so*  "$WORK/usr/lib/"

# --- Busybox applet symlinks (only what our init and shell need) ---
log "busybox applet symlinks"
BB_BIN_APPLETS="sh ash cat cp dd df echo grep head ln ls mkdir more mount mv ps rm sed sync tail touch umount"
BB_SBIN_APPLETS="brctl dmesg ifconfig init insmod killall mdev reboot rmmod route swapoff swapon sysctl"
BB_USR_BIN_APPLETS="awk basename cut dirname env find hexdump id kill less nc netstat pgrep pidof pkill seq sleep sort strings tee tr uname uniq wc which xargs"
BB_USR_SBIN_APPLETS="chroot hostname httpd nslookup ntpd sendmail udhcpc"
for a in $BB_BIN_APPLETS;       do ln -sf busybox    "$WORK/bin/$a";      done
for a in $BB_SBIN_APPLETS;      do ln -sf ../bin/busybox "$WORK/sbin/$a"; done
for a in $BB_USR_BIN_APPLETS;   do ln -sf ../../bin/busybox "$WORK/usr/bin/$a";  done
for a in $BB_USR_SBIN_APPLETS;  do ln -sf ../../bin/busybox "$WORK/usr/sbin/$a"; done

# --- LIVI overlay: our /init, /etc/*, /etc/init.d/rcS ---
log "overlay repo rootfs (init, inittab, rcS, passwd, hostname, profile)"
cp -a "$HERE/rootfs-overlay/." "$WORK/"
chmod 755 "$WORK/init" "$WORK/etc/init.d/rcS"

# --- LIVI Rust binaries (glibc-static, self-contained) ---
DBIN="$HELPERD/target/riscv32gc-unknown-linux-gnu/embedded"
[[ -x "$DBIN/livid" ]] || { log "missing $DBIN/livid — run build-v821b.sh first"; exit 2; }
cp "$DBIN/livid" "$WORK/usr/bin/livid"
for name in livi-tinyshell livi-netd livi-httpd livi-bt-up livi-ledd livi-mfid livi-wifid livi-btd livi-iapd; do
  ln -sf livid "$WORK/usr/bin/$name"
done
log "installed: livid + 5 symlinks"

# --- AIC8800 kernel modules --
MODULES=${MODULES:-$OUT_DIR/modules}
KREL=5.4.220
mkdir -p "$WORK/lib/modules/$KREL"
for m in aic8800_bsp aic8800_fdrv aic8800_btlpm; do
  [[ -f "$MODULES/$m.ko" ]] || { log "missing $MODULES/$m.ko — run build-v821b.sh first"; exit 2; }
  cp "$MODULES/$m.ko" "$WORK/lib/modules/$KREL/$m.ko"
done
log "installed AIC8800 modules: $(du -sk "$WORK/lib/modules/$KREL" | cut -f1) KiB"

# The from-source AIC driver (tag 20250410) requests firmware under lowercase aic8800d80/, but
# the blobs live in aic8800D80/. Add a case alias here — the build host is case-sensitive, the
# Mac repo is not, so this can't live in the committed overlay.
if [ -d "$WORK/lib/firmware/aic8800D80" ] && [ ! -e "$WORK/lib/firmware/aic8800d80" ]; then
  ln -s aic8800D80 "$WORK/lib/firmware/aic8800d80"
  log "firmware case alias: aic8800d80 -> aic8800D80"
fi

# --- Budget: what does each component weigh? ---
log "size breakdown (uncompressed, KiB):"
{
  printf "  %6s  %s\n" "$(du -sk "$WORK/bin/busybox"          | cut -f1)" "bin/busybox"
  printf "  %6s  %s\n" "$(du -sk "$WORK/lib/libc.so" "$WORK/lib/ld-musl-riscv32.so.1" | awk '{s+=$1} END {print s}')" "lib/{libc.so, ld-musl}"
  printf "  %6s  %s\n" "$(du -sLk "$WORK/usr/sbin/hostapd" "$WORK/usr/lib"/libnl-*.so.200 | awk '{s+=$1} END {print s}')" "usr/sbin/hostapd + libnl3"
  printf "  %6s  %s\n" "$(du -sk "$WORK/lib/firmware"| cut -f1)" "AIC8800 firmware"
  printf "  %6s  %s\n" "$(du -sk "$WORK/usr/bin/livid" | cut -f1)" "usr/bin/livid (Rust, multi-call)"
  printf "  %6s  %s\n" "$(du -sk "$WORK"                        | cut -f1)" "TOTAL (uncompressed)"
}

# --- Pack squashfs+xz ---
mkdir -p "$OUT_DIR"
OUT="$OUT_DIR/livi-link-v821b-mtd3.bin"
rm -f "$OUT"
mksquashfs "$WORK" "$OUT" -comp xz -no-progress -all-root -noappend 2>&1 | tail -3

SIZE=$(stat -c%s "$OUT")
FREE=$((MTD3_SIZE - SIZE))
log "mtd3: $SIZE B ($((SIZE/1024)) KiB), slot $MTD3_SIZE B ($((MTD3_SIZE/1024)) KiB), FREE $FREE B ($((FREE/1024)) KiB)"
[[ $SIZE -le $MTD3_SIZE ]] || { log "OVERFLOW by $(( SIZE - MTD3_SIZE )) B"; exit 3; }
md5sum "$OUT"


# --- Optional: pack mtd1 + mtd3 into a .lfwb firmware bundle ---
if [[ -x "$HERE/pack-bundle.sh" ]]; then
    "$HERE/pack-bundle.sh"
fi
