#!/usr/bin/env bash
# Cross-build the mipsel userspace bits (musl, busybox, libnl3, hostapd) from
# upstream sources, all statically linked, so there is no libc.so to ship or
# keep in sync. A board whose board.sh names RESCUE_APPLETS also gets a small
# busybox for its initramfs, where the kernel budget has no room for the full one.
#
#   $USERSPACE/bin/busybox
#   $USERSPACE/usr/sbin/hostapd
#   $USERSPACE/rescue/busybox   (RESCUE_APPLETS only)
#
#   build-userspace.sh <board dir>
set -euo pipefail

LOG_TAG=userspace
source "$(cd "${1:?usage: build-userspace.sh <board dir>}" && pwd)/board.sh"

BUILD=$TOP/userspace
SRC=$BUILD/src
STAGE=$BUILD/stage
LOGS=$BUILD/logs
mkdir -p "$SRC" "$STAGE" "$LOGS" "$USERSPACE"/{bin,usr/sbin}

quiet() {
  local logname=$1
  shift
  if ! "$@" >"$LOGS/$logname.log" 2>&1; then
    echo "=== $logname failed, last 60 lines: ==="
    tail -60 "$LOGS/$logname.log"
    return 1
  fi
}

MUSL_VER=1.2.6
MUSL_SHA=d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a
MUSL_URL=https://musl.libc.org/releases/musl-${MUSL_VER}.tar.gz

BUSYBOX_VER=1.36.1
BUSYBOX_SHA=b8cc24c9574d809e7279c3be349795c5d5ceb6fdf19ca709f80cde50e47de314
BUSYBOX_URL=https://busybox.net/downloads/busybox-${BUSYBOX_VER}.tar.bz2

LIBNL_VER=3.7.0
LIBNL_SHA=9fe43ccbeeea72c653bdcf8c93332583135cda46a79507bfd0a483bb57f65939
LIBNL_URL=https://github.com/thom311/libnl/releases/download/libnl${LIBNL_VER//./_}/libnl-${LIBNL_VER}.tar.gz

HOSTAPD_VER=2.10
HOSTAPD_SHA=206e7c799b678572c2e3d12030238784bc4a9f82323b0156b4c9466f1498915d
HOSTAPD_URL=https://w1.fi/releases/hostapd-${HOSTAPD_VER}.tar.gz

command -v "${CROSS_COMPILE}gcc" >/dev/null || {
  log "no ${CROSS_COMPILE}gcc in PATH"
  exit 2
}
CC=${CROSS_COMPILE}gcc
MUSL_PREFIX=$STAGE/musl-mips
LIBNL_PREFIX=$STAGE/libnl3

fetch() {
  local url=$1 out=$2 sha=${3:-}
  if [[ -f "$out" ]]; then
    [[ -z "$sha" ]] && return 0
    echo "$sha  $out" | sha256sum -c --status 2>/dev/null && return 0
    rm -f "$out"
  fi
  log "fetch $(basename "$out")"
  curl -sSL "$url" -o "$out"
  if [[ -n "$sha" ]]; then
    echo "$sha  $out" | sha256sum -c || {
      rm -f "$out"
      log "$(basename "$out") does not match its hash"
      exit 3
    }
  fi
}

if [[ ! -f "$MUSL_PREFIX/lib/libc.a" ]]; then
  fetch "$MUSL_URL" "$SRC/musl-${MUSL_VER}.tar.gz" "$MUSL_SHA"
  rm -rf "$SRC/musl-${MUSL_VER}"
  tar -xzf "$SRC/musl-${MUSL_VER}.tar.gz" -C "$SRC"
  apply_patches "$COMMON/patches/musl" "$SRC/musl-${MUSL_VER}"
  log "configure + build musl ${MUSL_VER}"
  (
    cd "$SRC/musl-${MUSL_VER}"
    quiet musl-configure env CC="$CC" CROSS_COMPILE="$CROSS_COMPILE" ./configure \
      --prefix="$MUSL_PREFIX" \
      --target=mipsel-linux-musl
    quiet musl-build make -j"$JOBS"
    quiet musl-install make install
  )
fi

MUSL_CC="${CROSS_COMPILE}gcc -isystem $MUSL_PREFIX/include -B$MUSL_PREFIX/lib -L$MUSL_PREFIX/lib"

if [[ ! -f "$MUSL_PREFIX/include/linux/nl80211.h" ]]; then
  fetch_kernel
  log "install kernel UAPI headers into musl sysroot"
  (cd "$KDIR" && make ARCH=mips INSTALL_HDR_PATH="$MUSL_PREFIX" headers_install >/dev/null)
fi

BB_SRC=$SRC/busybox-${BUSYBOX_VER}
if [[ ! -x "$USERSPACE/bin/busybox" ]]; then
  fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
  rm -rf "$BB_SRC"
  tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
  log "configure busybox ${BUSYBOX_VER}"
  (
    cd "$BB_SRC"
    make CROSS_COMPILE="$CROSS_COMPILE" defconfig >/dev/null </dev/null
    sed -i -e 's|^# CONFIG_STATIC is not set|CONFIG_STATIC=y|' -f "$COMMON/busybox.sed" .config
    (yes "" 2>/dev/null || true) | make CROSS_COMPILE="$CROSS_COMPILE" oldconfig >/dev/null
    quiet busybox-build make -j"$JOBS" CROSS_COMPILE="$CROSS_COMPILE" CC="$MUSL_CC"
  )
  cp -f "$BB_SRC/busybox" "$USERSPACE/bin/busybox"
fi

if [[ ! -f "$LIBNL_PREFIX/lib/libnl-3.a" ]]; then
  fetch "$LIBNL_URL" "$SRC/libnl-${LIBNL_VER}.tar.gz" "$LIBNL_SHA"
  rm -rf "$SRC/libnl-${LIBNL_VER}"
  tar -xzf "$SRC/libnl-${LIBNL_VER}.tar.gz" -C "$SRC"
  log "configure + build libnl ${LIBNL_VER}"
  (
    cd "$SRC/libnl-${LIBNL_VER}"
    quiet libnl-configure ./configure \
      --host=mipsel-linux-musl \
      --prefix="$LIBNL_PREFIX" \
      --enable-static --disable-shared \
      --disable-cli \
      --disable-pthreads \
      CC="$MUSL_CC" \
      AR="${CROSS_COMPILE}ar" RANLIB="${CROSS_COMPILE}ranlib"
    quiet libnl-build make -j"$JOBS"
    quiet libnl-install make install
  )
fi

HA_SRC=$SRC/hostapd-${HOSTAPD_VER}
if [[ ! -x "$USERSPACE/usr/sbin/hostapd" ]]; then
  fetch "$HOSTAPD_URL" "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" "$HOSTAPD_SHA"
  rm -rf "$HA_SRC"
  tar -xzf "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" -C "$SRC"
  log "build hostapd ${HOSTAPD_VER}"
  (
    cd "$HA_SRC/hostapd"
    cp "$COMMON/hostapd-build.config" .config
    cat >>.config <<EOF
LIBS += -L$LIBNL_PREFIX/lib
LDFLAGS += -static
EOF
    export PKG_CONFIG_PATH=$LIBNL_PREFIX/lib/pkgconfig
    export PKG_CONFIG_LIBDIR=$LIBNL_PREFIX/lib/pkgconfig
    quiet hostapd-build make -j"$JOBS" CC="$MUSL_CC"
  )
  cp -f "$HA_SRC/hostapd/hostapd" "$USERSPACE/usr/sbin/hostapd"
fi

log "strip"
"${CROSS_COMPILE}strip" "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd"

RESCUE=$USERSPACE/rescue
if [[ -n ${RESCUE_APPLETS:-} ]] \
  && [[ ! -x $RESCUE/busybox || "$(cat "$RESCUE/busybox.applets" 2>/dev/null)" != "$RESCUE_APPLETS" ]]; then
  log "rescue busybox (allnoconfig + RESCUE_APPLETS)"
  mkdir -p "$RESCUE"
  (
    cd "$BB_SRC"
    make -s distclean >/dev/null
    make -s allnoconfig >/dev/null </dev/null
    sed -i -e 's|^# CONFIG_STATIC is not set|CONFIG_STATIC=y|' \
      -e 's|^CONFIG_SH_IS_NONE=y|# CONFIG_SH_IS_NONE is not set|' .config
    for sym in $RESCUE_APPLETS; do
      sed -i "s|^# CONFIG_$sym is not set|CONFIG_$sym=y|" .config
    done
    (yes "" 2>/dev/null || true) | make -s oldconfig >/dev/null
    missing=
    for sym in STATIC $RESCUE_APPLETS; do
      grep -q "^CONFIG_$sym=y" .config || missing="$missing $sym"
    done
    [[ -z $missing ]] || {
      log "busybox symbols that did not stick:$missing"
      exit 4
    }
    quiet busybox-rescue make -j"$JOBS" CROSS_COMPILE="$CROSS_COMPILE" CC="$MUSL_CC"
  )
  "${CROSS_COMPILE}strip" -o "$RESCUE/busybox" "$BB_SRC/busybox"
  printf '%s' "$RESCUE_APPLETS" >"$RESCUE/busybox.applets"
  (cd "$BB_SRC" && make -s distclean >/dev/null)
fi

log "sizes:"
du -h "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd" ${RESCUE_APPLETS:+"$RESCUE/busybox"}
file "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd" ${RESCUE_APPLETS:+"$RESCUE/busybox"} \
  | grep -q 'dynamically linked' && {
  log "a userspace binary came out dynamically linked"
  exit 4
}
log "userspace done at $USERSPACE"
