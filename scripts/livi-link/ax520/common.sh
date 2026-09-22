# Sourced by the AX520 build scripts: pinned kernel source, shared paths, log().
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
TOP=${TOP:-$HOME/LocalDev/ax520-kernel}
JOBS=${JOBS:-$(nproc)}
CROSS_COMPILE=${CROSS_COMPILE:-arm-linux-gnu-}

KVER=6.18.53
KMAJOR=${KVER%%.*}
KURL="https://cdn.kernel.org/pub/linux/kernel/v${KMAJOR}.x/linux-${KVER}.tar.xz"
KDIR=$TOP/linux-$KVER
OUT=$TOP/out
USERSPACE=${USERSPACE:-$TOP/userspace/out}
mkdir -p "$TOP" "$OUT"

log(){ printf '\033[1;36m[ax520-%s]\033[0m %s\n' "${LOG_TAG:-build}" "$*"; }

# Vanilla kernel.org tarball, checked against kernel.org's own published
# sha256sums rather than a hash we would have to keep in sync by hand.
ax520_fetch_kernel() {
  [[ -d $KDIR ]] && return 0
  log "fetch linux-$KVER"
  curl -sSL "$KURL" -o "$TOP/linux-$KVER.tar.xz"
  curl -sSL "https://cdn.kernel.org/pub/linux/kernel/v${KMAJOR}.x/sha256sums.asc" -o "$TOP/sha256sums.asc"
  local expected
  expected=$(grep " linux-$KVER.tar.xz\$" "$TOP/sha256sums.asc" | awk '{print $1}')
  [[ -n $expected ]] || { log "linux-$KVER.tar.xz not found in kernel.org sha256sums"; exit 3; }
  echo "$expected  $TOP/linux-$KVER.tar.xz" | sha256sum -c -
  tar -xJf "$TOP/linux-$KVER.tar.xz" -C "$TOP"
}
