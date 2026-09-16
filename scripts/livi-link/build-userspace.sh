#!/usr/bin/env bash
# Cross-build the V821B userspace bits (musl, busybox, libnl3, hostapd)
# from upstream sources. Andes gcc as the C compiler. musl runtime is
# copied into $OUT/lib so busybox/hostapd link against it dynamically.
#
# Layout matches what build-mtd3.sh expects from $STOCK_ROOTFS:
#   $OUT/bin/busybox
#   $OUT/lib/{libc.so, ld-musl-riscv32.so.1}
#   $OUT/usr/sbin/hostapd
#   $OUT/usr/lib/libnl-3.so.200, libnl-genl-3.so.200
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
BUILD=${BUILD:-$HOME/LocalDev/livi-userspace-build}
SRC=$BUILD/src
STAGE=$BUILD/stage
OUT=${OUT:-$BUILD/out}
JOBS=${JOBS:-$(nproc)}
LOGS=$BUILD/logs
mkdir -p "$LOGS"

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

TC_ROOT=${TC_ROOT:-$HOME/LocalDev/LIVI-Link/build/tmp/sysroots-components/x86_64/sunxi-nds32le-native}
TC_BIN=${TC_BIN:-$(echo "$TC_ROOT"/usr/lib/sunxi-toolchains/nds32le-linux-glibc-v5d*/bin | awk '{print $1}')}
[[ -x "$TC_BIN/riscv32-linux-gcc" ]] || { echo "no riscv32-linux-gcc under $TC_BIN"; exit 2; }
export PATH=$TC_BIN:$PATH
CC=riscv32-linux-gcc
CROSS=riscv32-linux-
MUSL_PREFIX=$STAGE/musl-riscv32
LIBNL_PREFIX=$STAGE/libnl3

log(){ printf '\033[1;35m[userspace]\033[0m %s\n' "$*"; }

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
    log "$(sha256sum "$out" | awk '{print $1}')  $out"
}

mkdir -p "$SRC" "$STAGE" "$OUT"/{bin,lib,usr/sbin,usr/lib}

# ---------------------------------------------------------------------------
# 1) musl → riscv32-linux
# ---------------------------------------------------------------------------
if [[ ! -f "$MUSL_PREFIX/lib/libc.so" ]]; then
    fetch "$MUSL_URL" "$SRC/musl-${MUSL_VER}.tar.gz" "$MUSL_SHA"
    rm -rf "$SRC/musl-${MUSL_VER}"
    tar -xzf "$SRC/musl-${MUSL_VER}.tar.gz" -C "$SRC"
    log "configure + build musl ${MUSL_VER}"
    (
        cd "$SRC/musl-${MUSL_VER}"
        quiet musl-configure  env CC=$CC CROSS_COMPILE=$CROSS ./configure \
            --prefix="$MUSL_PREFIX" \
            --target=riscv32-linux \
            --enable-wrapper=gcc
        quiet musl-build      make -j"$JOBS"
        quiet musl-install    make install
    )
fi

MUSL_GCC=$MUSL_PREFIX/bin/musl-gcc
[[ -x "$MUSL_GCC" ]] || { log "no musl-gcc at $MUSL_GCC"; exit 3; }

KERNEL_SRC=${KERNEL_SRC:-$HOME/LocalDev/tina-test/tina-v821-v1.3-kernel/linux-5.4-ansc}
if [[ ! -f "$MUSL_PREFIX/include/linux/kd.h" && -d "$KERNEL_SRC" ]]; then
    log "install kernel UAPI headers into musl sysroot"
    (cd "$KERNEL_SRC" && make ARCH=riscv INSTALL_HDR_PATH="$MUSL_PREFIX" headers_install >/dev/null)
fi

cp -f "$MUSL_PREFIX/lib/libc.so"                "$OUT/lib/libc.so"
cp -f "$MUSL_PREFIX/lib/ld-musl-riscv32.so.1"   "$OUT/lib/ld-musl-riscv32.so.1" 2>/dev/null || \
    ln -sf libc.so "$OUT/lib/ld-musl-riscv32.so.1"

# ---------------------------------------------------------------------------
# 2) busybox
# ---------------------------------------------------------------------------
BB_SRC=$SRC/busybox-${BUSYBOX_VER}
if [[ ! -x "$OUT/bin/busybox" ]]; then
    fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
    rm -rf "$BB_SRC"
    tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
    log "configure busybox ${BUSYBOX_VER}"
    (
        cd "$BB_SRC"
        make CROSS_COMPILE=$CROSS defconfig >/dev/null </dev/null
        sed -i \
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
        (yes "" 2>/dev/null || true) | make CROSS_COMPILE=$CROSS oldconfig >/dev/null
        quiet busybox-build make -j"$JOBS" CROSS_COMPILE=$CROSS CC="$MUSL_GCC"
    )
    cp -f "$BB_SRC/busybox" "$OUT/bin/busybox"
fi

# ---------------------------------------------------------------------------
# 3) libnl3
# ---------------------------------------------------------------------------
if [[ ! -f "$LIBNL_PREFIX/lib/libnl-3.so" ]]; then
    fetch "$LIBNL_URL" "$SRC/libnl-${LIBNL_VER}.tar.gz" "$LIBNL_SHA"
    rm -rf "$SRC/libnl-${LIBNL_VER}"
    tar -xzf "$SRC/libnl-${LIBNL_VER}.tar.gz" -C "$SRC"
    log "configure + build libnl ${LIBNL_VER}"
    (
        cd "$SRC/libnl-${LIBNL_VER}"
        quiet libnl-configure ./configure \
            --host=riscv32-linux-musl \
            --prefix="$LIBNL_PREFIX" \
            --disable-static \
            --disable-cli \
            --disable-pthreads \
            CC="$MUSL_GCC" \
            AR="${CROSS}ar" RANLIB="${CROSS}ranlib"
        quiet libnl-build   make -j"$JOBS"
        quiet libnl-install make install
    )
fi

# Ship the two libnl3 SOs the dongle needs, dereferencing the version symlink
# so both the SONAME (libnl-3.so.200) and the compat symlink (libnl-3.so) land.
for l in libnl-3 libnl-genl-3; do
    cp -fL "$LIBNL_PREFIX/lib/${l}.so.200"      "$OUT/usr/lib/${l}.so.200"
    ln -sf "${l}.so.200"                        "$OUT/usr/lib/${l}.so"
done

# ---------------------------------------------------------------------------
# 4) hostapd
# ---------------------------------------------------------------------------
HA_SRC=$SRC/hostapd-${HOSTAPD_VER}
if [[ ! -x "$OUT/usr/sbin/hostapd" ]]; then
    fetch "$HOSTAPD_URL" "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" "$HOSTAPD_SHA"
    rm -rf "$HA_SRC"
    tar -xzf "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" -C "$SRC"
    (
        cd "$HA_SRC/hostapd"
        cp "$HERE/hostapd-build.config" .config
        # hostapd's Makefile just adds `LIBS += -lnl-3 -lnl-genl-3` under
        # CONFIG_LIBNL32 — no pkg-config, no -L. Append -L via .config so
        # the linker can find our staged libnl3.
        cat >>.config <<EOF
LIBS += -L$LIBNL_PREFIX/lib
EOF
        export PKG_CONFIG_PATH=$LIBNL_PREFIX/lib/pkgconfig
        export PKG_CONFIG_LIBDIR=$LIBNL_PREFIX/lib/pkgconfig
        quiet hostapd-build make -j"$JOBS" CC="$MUSL_GCC"
    )
    cp -f "$HA_SRC/hostapd/hostapd" "$OUT/usr/sbin/hostapd"
fi

# Strip everything (matches Stock: -Os + stripped). Big wins on hostapd.
log "strip"
"${CROSS}strip" \
    "$OUT/bin/busybox" \
    "$OUT/usr/sbin/hostapd" \
    "$OUT/usr/lib"/libnl-*.so.200 \
    "$OUT/lib/libc.so" 2>/dev/null || true

log "sizes:"
du -h "$OUT/bin/busybox" \
      "$OUT/usr/sbin/hostapd" \
      "$OUT/usr/lib"/libnl-*.so.200 \
      "$OUT/lib"/{libc.so,ld-musl-riscv32.so.1}
log "userspace done at $OUT"
