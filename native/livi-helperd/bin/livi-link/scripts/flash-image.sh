#!/bin/sh
# flash-image.sh <partition> <expected sha256> [image]
#
# The one place that erases flash, so the guards live here and not at each caller: rootfs only,
# addressed by name from /proc/mtd, exact partition size, matching sha256 — all before the erase.
# The tools move to tmpfs first, because the write takes out the rootfs they live on.
# LIVI_FLASH_DRY_RUN=1 checks everything and prints the write instead of doing it.
PATH=/bin:/sbin:/usr/bin:/usr/sbin; export PATH

part=$1
want=$2
img=${3:-/tmp/restore.img}
stage=/tmp/flash

[ -n "$part" ] && [ -n "$want" ] || { echo "usage: flash-image.sh rootfs <sha256> [image]"; exit 2; }
[ -f "$img" ] || { echo "no $img - upload an image first"; exit 1; }

case "$part" in
  rootfs) ;;
  *) echo "refusing $part - only the rootfs can be written from here"; exit 1 ;;
esac

line=$(grep -w "$part" /proc/mtd)
[ -n "$line" ] || { echo "no partition named $part in /proc/mtd"; exit 1; }
dev=/dev/${line%%:*}
psize=$((0x$(echo "$line" | cut -d" " -f2)))
ersize=$((0x$(echo "$line" | cut -d" " -f3)))
isize=$(wc -c < "$img")

if [ "$isize" -ne "$psize" ]; then
  echo "image is $isize bytes, $part ($dev) is $psize - refusing"
  exit 1
fi
got=$(sha256sum "$img" | cut -d" " -f1)
if [ "$got" != "$want" ]; then
  echo "sha256 mismatch - have $got"
  exit 1
fi

# Busybox applets, copied under their own name so they still dispatch by argv[0] — nothing below
# reaches back into the rootfs it is erasing. cat, not dd: this busybox has no dd applet and the
# flash is byte-addressable (writesize 1), so a plain copy programs it.
mkdir -p "$stage"
for t in sh cat mount sync reboot sleep; do cp "$(command -v $t)" "$stage/" 2>/dev/null; done
cp /usr/sbin/flash_erase "$stage/" 2>/dev/null
for t in sh cat mount sync reboot sleep flash_erase; do
  [ -x "$stage/$t" ] || { echo "could not stage $t in $stage - refusing"; exit 1; }
done

blocks=$((psize / ersize))
if [ -n "$LIVI_FLASH_DRY_RUN" ]; then
  echo "would write $isize bytes to $dev: remount ro, flash_erase $dev 0 $blocks, cat $img > $dev"
  exit 0
fi
# Red and blue alternating for as long as the write runs, the signal the vendor updater gives too.
cat > "$stage/blink.sh" <<EOF
PATH=$stage; export PATH
for g in 2 9; do
  [ -e /sys/class/gpio/gpio\$g ] || echo \$g > /sys/class/gpio/export
  echo out > /sys/class/gpio/gpio\$g/direction
done
while :; do
  echo 0 > /sys/class/gpio/gpio2/value; echo 1 > /sys/class/gpio/gpio9/value; sleep 0.25
  echo 1 > /sys/class/gpio/gpio2/value; echo 0 > /sys/class/gpio/gpio9/value; sleep 0.25
done
EOF
killall colorLightDaemon 2>/dev/null

echo "writing $isize bytes to $dev ($blocks blocks of $ersize), then rebooting"
echo "if it does not come back, $stage.log has the reason"
setsid "$stage/sh" -c "$stage/sh $stage/blink.sh & \
  $stage/sleep 1; \
  $stage/mount -o remount,ro /; \
  $stage/flash_erase $dev 0 $blocks && \
  $stage/cat $img > $dev && \
  $stage/sync; $stage/reboot -f" > "$stage.log" 2>&1 &
