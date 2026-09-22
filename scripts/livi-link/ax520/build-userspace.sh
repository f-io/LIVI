#!/usr/bin/env bash
# Cross-build the AX520 userspace bits (musl, busybox, libnl3, hostapd) from
# upstream sources, all statically linked: the same busybox serves the
# initramfs baked into the kernel and the rootfs squashfs, so there is no
# libc.so to ship or keep in sync.
#
#   $USERSPACE/bin/busybox
#   $USERSPACE/usr/sbin/hostapd
set -euo pipefail

LOG_TAG=userspace
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

BUILD=$TOP/userspace
SRC=$BUILD/src
STAGE=$BUILD/stage
LOGS=$BUILD/logs
mkdir -p "$SRC" "$STAGE" "$LOGS" "$USERSPACE"/{bin,usr/sbin}

# Run a command silently, only show its last 60 lines on failure.
quiet(){
    local logname=$1; shift
    if ! "$@" >"$LOGS/$logname.log" 2>&1; then
        echo "=== $logname failed, last 60 lines: ==="
        tail -60 "$LOGS/$logname.log"
        return 1
    fi
}

MUSL_VER=1.2.5
MUSL_SHA=a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4
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

command -v "${CROSS_COMPILE}gcc" >/dev/null || { log "no ${CROSS_COMPILE}gcc in PATH"; exit 2; }
CC=${CROSS_COMPILE}gcc
MUSL_PREFIX=$STAGE/musl-arm
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
        echo "$sha  $out" | sha256sum -c || log "WARN sha mismatch, continuing"
    fi
}

# ---------------------------------------------------------------------------
# 1) musl → arm hard-float
# ---------------------------------------------------------------------------
if [[ ! -f "$MUSL_PREFIX/lib/libc.a" ]]; then
    fetch "$MUSL_URL" "$SRC/musl-${MUSL_VER}.tar.gz" "$MUSL_SHA"
    rm -rf "$SRC/musl-${MUSL_VER}"
    tar -xzf "$SRC/musl-${MUSL_VER}.tar.gz" -C "$SRC"
    log "configure + build musl ${MUSL_VER}"
    (
        cd "$SRC/musl-${MUSL_VER}"
        quiet musl-configure  env CC=$CC CROSS_COMPILE=$CROSS_COMPILE ./configure \
            --prefix="$MUSL_PREFIX" \
            --target=arm-linux-gnueabihf \
            --enable-wrapper=gcc
        quiet musl-build      make -j"$JOBS"
        quiet musl-install    make install
    )
fi

MUSL_GCC=$MUSL_PREFIX/bin/musl-gcc
[[ -x "$MUSL_GCC" ]] || { log "no musl-gcc at $MUSL_GCC"; exit 3; }

# musl ships no kernel UAPI headers; busybox, libnl and hostapd need linux/*.h.
if [[ ! -f "$MUSL_PREFIX/include/linux/nl80211.h" ]]; then
    ax520_fetch_kernel
    log "install kernel UAPI headers into musl sysroot"
    (cd "$KDIR" && make ARCH=arm INSTALL_HDR_PATH="$MUSL_PREFIX" headers_install >/dev/null)
fi

# ---------------------------------------------------------------------------
# 2) busybox (static)
# ---------------------------------------------------------------------------
BB_SRC=$SRC/busybox-${BUSYBOX_VER}
if [[ ! -x "$USERSPACE/bin/busybox" ]]; then
    fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
    rm -rf "$BB_SRC"
    tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
    log "configure busybox ${BUSYBOX_VER}"
    (
        cd "$BB_SRC"
        make CROSS_COMPILE=$CROSS_COMPILE defconfig >/dev/null </dev/null
        # tc does not build against current kernel headers, the rest is
        # console/VT tooling this headless dongle has no use for.
        sed -i \
            -e 's|^# CONFIG_STATIC is not set|CONFIG_STATIC=y|' \
            -e 's|^# CONFIG_FLASHCP is not set|CONFIG_FLASHCP=y|' \
            -e 's|^# CONFIG_FLASH_ERASEALL is not set|CONFIG_FLASH_ERASEALL=y|' \
            -e 's|^CONFIG_TC=y|# CONFIG_TC is not set|' \
            -e 's|^CONFIG_FEATURE_TC.*|# &|' \
            -e 's|^CONFIG_KBD_MODE=y|# CONFIG_KBD_MODE is not set|' \
            -e 's|^CONFIG_LOADFONT=y|# CONFIG_LOADFONT is not set|' \
            -e 's|^CONFIG_LOADKMAP=y|# CONFIG_LOADKMAP is not set|' \
            -e 's|^CONFIG_OPENVT=y|# CONFIG_OPENVT is not set|' \
            -e 's|^CONFIG_SETCONSOLE=y|# CONFIG_SETCONSOLE is not set|' \
            -e 's|^CONFIG_SETFONT=y|# CONFIG_SETFONT is not set|' \
            -e 's|^CONFIG_SHOWKEY=y|# CONFIG_SHOWKEY is not set|' \
            -e 's|^CONFIG_CHVT=y|# CONFIG_CHVT is not set|' \
            -e 's|^CONFIG_DEALLOCVT=y|# CONFIG_DEALLOCVT is not set|' \
            -e 's|^CONFIG_RESET=y|# CONFIG_RESET is not set|' \
            -e 's|^CONFIG_FGCONSOLE=y|# CONFIG_FGCONSOLE is not set|' \
            -e 's|^CONFIG_HWCLOCK=y|# CONFIG_HWCLOCK is not set|' \
            -e 's|^CONFIG_RTCWAKE=y|# CONFIG_RTCWAKE is not set|' \
            .config
        (yes "" 2>/dev/null || true) | make CROSS_COMPILE=$CROSS_COMPILE oldconfig >/dev/null
        quiet busybox-build make -j"$JOBS" CROSS_COMPILE=$CROSS_COMPILE CC="$MUSL_GCC"
    )
    cp -f "$BB_SRC/busybox" "$USERSPACE/bin/busybox"
fi

# ---------------------------------------------------------------------------
# 3) libnl3 (static)
# ---------------------------------------------------------------------------
if [[ ! -f "$LIBNL_PREFIX/lib/libnl-3.a" ]]; then
    fetch "$LIBNL_URL" "$SRC/libnl-${LIBNL_VER}.tar.gz" "$LIBNL_SHA"
    rm -rf "$SRC/libnl-${LIBNL_VER}"
    tar -xzf "$SRC/libnl-${LIBNL_VER}.tar.gz" -C "$SRC"
    log "configure + build libnl ${LIBNL_VER}"
    (
        cd "$SRC/libnl-${LIBNL_VER}"
        quiet libnl-configure ./configure \
            --host=arm-linux-gnueabihf \
            --prefix="$LIBNL_PREFIX" \
            --enable-static --disable-shared \
            --disable-cli \
            --disable-pthreads \
            CC="$MUSL_GCC" \
            AR="${CROSS_COMPILE}ar" RANLIB="${CROSS_COMPILE}ranlib"
        quiet libnl-build   make -j"$JOBS"
        quiet libnl-install make install
    )
fi

# ---------------------------------------------------------------------------
# 4) hostapd (static)
# ---------------------------------------------------------------------------
HA_SRC=$SRC/hostapd-${HOSTAPD_VER}
if [[ ! -x "$USERSPACE/usr/sbin/hostapd" ]]; then
    fetch "$HOSTAPD_URL" "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" "$HOSTAPD_SHA"
    rm -rf "$HA_SRC"
    tar -xzf "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" -C "$SRC"
    log "build hostapd ${HOSTAPD_VER}"
    (
        cd "$HA_SRC/hostapd"
        cp "$HERE/hostapd-build.config" .config
        # CONFIG_LIBNL32 only appends -lnl-3 -lnl-genl-3; point the linker at our
        # staged static libnl3 and link the whole thing static.
        cat >>.config <<EOF
LIBS += -L$LIBNL_PREFIX/lib
LDFLAGS += -static
EOF
        export PKG_CONFIG_PATH=$LIBNL_PREFIX/lib/pkgconfig
        export PKG_CONFIG_LIBDIR=$LIBNL_PREFIX/lib/pkgconfig
        quiet hostapd-build make -j"$JOBS" CC="$MUSL_GCC"
    )
    cp -f "$HA_SRC/hostapd/hostapd" "$USERSPACE/usr/sbin/hostapd"
fi

log "strip"
"${CROSS_COMPILE}strip" "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd"

log "sizes:"
du -h "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd"
file "$USERSPACE/bin/busybox" "$USERSPACE/usr/sbin/hostapd" | grep -q 'dynamically linked' \
    && { log "a userspace binary came out dynamically linked"; exit 4; }
log "userspace done at $USERSPACE"
