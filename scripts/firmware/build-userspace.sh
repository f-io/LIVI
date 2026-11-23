#!/usr/bin/env bash
# Cross-build the userspace bits (musl, busybox, libnl3, hostapd) from upstream sources. The arch's
# common.sh decides how: US_LINK=static links everything static, so there is no libc.so to ship or
# keep in sync; US_LINK=shared links musl and libnl dynamically and ships them. A board whose
# board.sh names RESCUE_APPLETS also gets a small static busybox for its initramfs, where the kernel
# budget has no room for the full one.
#
#   $USERSPACE/bin/busybox
#   $USERSPACE/usr/sbin/hostapd
#   $USERSPACE/lib/{libc.so, $MUSL_LDSO}        (shared only)
#   $USERSPACE/usr/lib/libnl-{3,genl-3}.so.200  (shared only)
#   $USERSPACE/rescue/busybox                   (RESCUE_APPLETS only)
#
#   build-userspace.sh <board dir>
set -euo pipefail

LOG_TAG=userspace
source "$(cd "${1:?usage: build-userspace.sh <board dir>}" && pwd)/board.sh"
: "${US_ARCH:?the arch common.sh sets US_ARCH}"
US_CROSS=${US_CROSS:-$CROSS_COMPILE}
LIBNL_HOST=${LIBNL_HOST:-$MUSL_TARGET}
[[ -n ${US_PATH:-} ]] && export PATH=$US_PATH:$PATH

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

BUSYBOX_VER=1.38.0
BUSYBOX_SHA=34f9ea6ff8636f2c9241153b9114eefa9e65674a45318ae1ef95bb5f31c53bb2
BUSYBOX_URL=https://busybox.net/downloads/busybox-${BUSYBOX_VER}.tar.bz2

LIBNL_VER=3.12.0
LIBNL_SHA=fc51ca7196f1a3f5fdf6ffd3864b50f4f9c02333be28be4eeca057e103c0dd18
LIBNL_URL=https://github.com/thom311/libnl/releases/download/libnl${LIBNL_VER//./_}/libnl-${LIBNL_VER}.tar.gz

HOSTAPD_VER=2.12
HOSTAPD_SHA=f43502561c28ba47ab77e18e1a973d07361c68cc8b14178e619bd5796b70eabd
HOSTAPD_URL=https://w1.fi/releases/hostapd-${HOSTAPD_VER}.tar.gz

command -v "${US_CROSS}gcc" >/dev/null || {
  log "no ${US_CROSS}gcc in PATH"
  exit 2
}
MUSL_PREFIX=$STAGE/musl-$US_ARCH
LIBNL_PREFIX=$STAGE/libnl3
HOSTAPD_CONFIG=$(sha256sum "$COMMON/hostapd-build.config" | cut -c1-12)
PINS="$US_LINK musl-$MUSL_VER busybox-$BUSYBOX_VER libnl-$LIBNL_VER hostapd-$HOSTAPD_VER-$HOSTAPD_CONFIG"
if [[ "$(cat "$BUILD/pins" 2>/dev/null)" != "$PINS" ]]; then
  rm -rf "$MUSL_PREFIX" "$LIBNL_PREFIX" "$USERSPACE/rescue" \
    "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd" \
    "$USERSPACE/lib/libc.so" "$USERSPACE/usr/lib"/libnl-*
fi

fetch() {
  local url=$1 out=$2 sha=$3
  if [[ -f "$out" ]] && echo "$sha  $out" | sha256sum -c --status 2>/dev/null; then
    return 0
  fi
  rm -f "$out"
  log "fetch $(basename "$out")"
  download "$url" -o "$out"
  echo "$sha  $out" | sha256sum -c || {
    rm -f "$out"
    log "$(basename "$out") does not match its hash"
    exit 3
  }
}

if [[ ! -f "$MUSL_PREFIX/lib/libc.a" ]]; then
  fetch "$MUSL_URL" "$SRC/musl-${MUSL_VER}.tar.gz" "$MUSL_SHA"
  rm -rf "$SRC/musl-${MUSL_VER}"
  tar -xzf "$SRC/musl-${MUSL_VER}.tar.gz" -C "$SRC"
  # CVE-2026-40200 (qsort stack overflow, "32-bit users should patch"): not in any tagged musl
  # release yet (1.2.7 will have it), only as upstream's own mailed patch, applied here as our own
  # maintained patch until it is.
  apply_patches "$COMMON/patches/musl" "$SRC/musl-${MUSL_VER}"
  log "configure + build musl ${MUSL_VER}"
  (
    cd "$SRC/musl-${MUSL_VER}"
    quiet musl-configure env CC="${US_CROSS}gcc" CROSS_COMPILE="$US_CROSS" ./configure \
      --prefix="$MUSL_PREFIX" \
      --target="$MUSL_TARGET" \
      ${MUSL_WRAPPER:+--enable-wrapper=gcc}
    quiet musl-build make -j"$JOBS"
    quiet musl-install make install
  )
fi

if [[ -n ${MUSL_WRAPPER:-} ]]; then
  MUSL_CC=$MUSL_PREFIX/bin/musl-gcc
  [[ -x "$MUSL_CC" ]] || {
    log "no musl-gcc at $MUSL_CC"
    exit 3
  }
else
  MUSL_CC="${US_CROSS}gcc -isystem $MUSL_PREFIX/include -B$MUSL_PREFIX/lib -L$MUSL_PREFIX/lib"
fi

# musl ships no kernel UAPI headers; busybox, libnl and hostapd need linux/*.h.
if [[ ! -f "$MUSL_PREFIX/include/linux/nl80211.h" ]]; then
  fetch_kernel
  log "install kernel UAPI headers into musl sysroot"
  (cd "$KDIR" && make ARCH="$US_ARCH" INSTALL_HDR_PATH="$MUSL_PREFIX" headers_install >/dev/null)
fi

if [[ $US_LINK == shared ]]; then
  mkdir -p "$USERSPACE"/{lib,usr/lib}
  cp -f "$MUSL_PREFIX/lib/libc.so" "$USERSPACE/lib/libc.so"
  ln -sf libc.so "$USERSPACE/lib/$MUSL_LDSO"
  STATIC_BUSYBOX=
  LIBNL_LINK=(--disable-static)
else
  STATIC_BUSYBOX='s|^# CONFIG_STATIC is not set|CONFIG_STATIC=y|'
  LIBNL_LINK=(--enable-static --disable-shared)
fi

BB_SRC=$SRC/busybox-${BUSYBOX_VER}
if [[ ! -x "$USERSPACE/bin/busybox" ]]; then
  fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
  rm -rf "$BB_SRC"
  tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
  log "configure busybox ${BUSYBOX_VER}"
  (
    cd "$BB_SRC"
    make CROSS_COMPILE="$US_CROSS" defconfig >/dev/null </dev/null
    sed -i -e "$STATIC_BUSYBOX" -f "$COMMON/busybox.sed" .config
    (yes "" 2>/dev/null || true) | make CROSS_COMPILE="$US_CROSS" oldconfig >/dev/null
    quiet busybox-build make -j"$JOBS" CROSS_COMPILE="$US_CROSS" CC="$MUSL_CC"
  )
  cp -f "$BB_SRC/busybox" "$USERSPACE/bin/busybox"
fi

if [[ ! -f "$LIBNL_PREFIX/lib/libnl-3.a" && ! -f "$LIBNL_PREFIX/lib/libnl-3.so" ]]; then
  fetch "$LIBNL_URL" "$SRC/libnl-${LIBNL_VER}.tar.gz" "$LIBNL_SHA"
  rm -rf "$SRC/libnl-${LIBNL_VER}"
  tar -xzf "$SRC/libnl-${LIBNL_VER}.tar.gz" -C "$SRC"
  log "configure + build libnl ${LIBNL_VER}"
  (
    cd "$SRC/libnl-${LIBNL_VER}"
    quiet libnl-configure ./configure \
      --host="$LIBNL_HOST" \
      --prefix="$LIBNL_PREFIX" \
      "${LIBNL_LINK[@]}" \
      --disable-cli \
      --disable-pthreads \
      CC="$MUSL_CC" \
      AR="${US_CROSS}ar" RANLIB="${US_CROSS}ranlib"
    quiet libnl-build make -j"$JOBS"
    quiet libnl-install make install
  )
fi

if [[ $US_LINK == shared ]]; then
  for l in libnl-3 libnl-genl-3; do
    cp -fL "$LIBNL_PREFIX/lib/${l}.so.200" "$USERSPACE/usr/lib/${l}.so.200"
    ln -sf "${l}.so.200" "$USERSPACE/usr/lib/${l}.so"
  done
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
    # CONFIG_LIBNL32 only appends -lnl-3 -lnl-genl-3; point the linker at our
    # staged libnl3.
    echo "LIBS += -L$LIBNL_PREFIX/lib" >>.config
    [[ $US_LINK == shared ]] || echo "LDFLAGS += -static" >>.config
    export PKG_CONFIG_PATH=$LIBNL_PREFIX/lib/pkgconfig
    export PKG_CONFIG_LIBDIR=$LIBNL_PREFIX/lib/pkgconfig
    quiet hostapd-build make -j"$JOBS" CC="$MUSL_CC"
  )
  cp -f "$HA_SRC/hostapd/hostapd" "$USERSPACE/usr/sbin/hostapd"
fi

SHIPPED=("$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd")
[[ $US_LINK == shared ]] && SHIPPED+=("$USERSPACE/lib/libc.so" "$USERSPACE/usr/lib"/libnl-*.so.200)
log "strip"
"${US_CROSS}strip" "${SHIPPED[@]}"

# The rescue busybox is rebuilt whenever the list changes, since a stale binary would lack the new
# applets.
RESCUE=$USERSPACE/rescue
if [[ -n ${RESCUE_APPLETS:-} ]] \
  && [[ ! -x $RESCUE/busybox || "$(cat "$RESCUE/busybox.applets" 2>/dev/null)" != "$RESCUE_APPLETS" ]]; then
  if [[ ! -d $BB_SRC ]]; then
    fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
    tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
  fi
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
    quiet busybox-rescue make -j"$JOBS" CROSS_COMPILE="$US_CROSS" CC="$MUSL_CC"
  )
  "${US_CROSS}strip" -o "$RESCUE/busybox" "$BB_SRC/busybox"
  printf '%s' "$RESCUE_APPLETS" >"$RESCUE/busybox.applets"
  # The full busybox is built from the same tree, leave it clean for the next run.
  (cd "$BB_SRC" && make -s distclean >/dev/null)
  file "$RESCUE/busybox" | grep -q 'statically linked' || {
    log "rescue busybox is not static"
    exit 4
  }
fi

log "sizes:"
du -h "${SHIPPED[@]}" ${RESCUE_APPLETS:+"$RESCUE/busybox"}
if [[ $US_LINK != shared ]] && file "${SHIPPED[@]}" | grep -q 'dynamically linked'; then
  log "a userspace binary came out dynamically linked"
  exit 4
fi
printf '%s' "$PINS" >"$BUILD/pins"
log "userspace done at $USERSPACE"
