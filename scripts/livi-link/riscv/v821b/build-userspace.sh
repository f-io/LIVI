#!/usr/bin/env bash
# Cross-build the V821B userspace bits (musl, busybox, libnl3, hostapd)
# from upstream sources. Andes gcc as the C compiler. musl runtime is
# copied into $USERSPACE/lib so busybox/hostapd link against it dynamically.
#
# Layout matches what common/build-rootfs.sh and the initramfs expect:
#   $USERSPACE/bin/busybox
#   $USERSPACE/lib/{libc.so, ld-musl-riscv32.so.1}
#   $USERSPACE/usr/sbin/hostapd
#   $USERSPACE/usr/lib/libnl-3.so.200, libnl-genl-3.so.200
#   $USERSPACE/rescue/busybox   (RESCUE_APPLETS only)
set -euo pipefail

LOG_TAG=userspace
source "$(dirname "${BASH_SOURCE[0]}")/board.sh"
BUILD=$TOP/userspace
SRC=$BUILD/src
STAGE=$BUILD/stage
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

[[ -x "$TC_BIN/riscv32-linux-gcc" ]] || { echo "no riscv32-linux-gcc under $TC_BIN (set TC_BIN)"; exit 2; }
export PATH=$TC_BIN:$PATH
CC=riscv32-linux-gcc
CROSS=riscv32-linux-
MUSL_PREFIX=$STAGE/musl-riscv32
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
        echo "$sha  $out" | sha256sum -c || { rm -f "$out"; log "$(basename "$out") does not match its hash"; exit 3; }
    fi
    log "$(sha256sum "$out" | awk '{print $1}')  $out"
}

mkdir -p "$SRC" "$STAGE" "$USERSPACE"/{bin,lib,usr/sbin,usr/lib}

# ---------------------------------------------------------------------------
# 1) musl → riscv32-linux
# ---------------------------------------------------------------------------
if [[ ! -f "$MUSL_PREFIX/lib/libc.so" ]]; then
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

if [[ ! -f "$MUSL_PREFIX/include/linux/kd.h" ]]; then
    fetch_kernel
    log "install kernel UAPI headers into musl sysroot"
    (cd "$KDIR" && make ARCH=riscv INSTALL_HDR_PATH="$MUSL_PREFIX" headers_install >/dev/null)
fi

cp -f "$MUSL_PREFIX/lib/libc.so"                "$USERSPACE/lib/libc.so"
cp -f "$MUSL_PREFIX/lib/ld-musl-riscv32.so.1"   "$USERSPACE/lib/ld-musl-riscv32.so.1" 2>/dev/null || \
    ln -sf libc.so "$USERSPACE/lib/ld-musl-riscv32.so.1"

# ---------------------------------------------------------------------------
# 2) busybox
# ---------------------------------------------------------------------------
BB_SRC=$SRC/busybox-${BUSYBOX_VER}
if [[ ! -x "$USERSPACE/bin/busybox" ]]; then
    fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
    rm -rf "$BB_SRC"
    tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
    log "configure busybox ${BUSYBOX_VER}"
    (
        cd "$BB_SRC"
        make CROSS_COMPILE=$CROSS defconfig >/dev/null </dev/null
        sed -i -f "$COMMON/busybox.sed" .config
        (yes "" 2>/dev/null || true) | make CROSS_COMPILE=$CROSS oldconfig >/dev/null
        quiet busybox-build make -j"$JOBS" CROSS_COMPILE=$CROSS CC="$MUSL_GCC"
    )
    cp -f "$BB_SRC/busybox" "$USERSPACE/bin/busybox"
fi

# ---------------------------------------------------------------------------
# 2b) rescue busybox for the initramfs: static, only what the ways in, the switch to the rootfs and the
#     provisioner use, since the boot image has no room for the full one and musl next to it.
#     Rebuilt whenever the list changes.
# ---------------------------------------------------------------------------
RESCUE_APPLETS="
  LFS BUSYBOX ASH SH_IS_ASH ASH_JOB_CONTROL ASH_ECHO ASH_PRINTF ASH_TEST ASH_CMDCMD ASH_OPTIMIZE_FOR_SIZE
  FEATURE_SH_MATH FEATURE_EDITING FEATURE_TAB_COMPLETION FEATURE_INSTALLER FEATURE_DEVPTS
  CTTYHACK SETSID STTY SWITCH_ROOT
  CAT CHMOD CP CUT DATE DD DF DIRNAME BASENAME DU ECHO ENV FALSE TRUE HEAD TAIL LN LS MKDIR MV
  OD PRINTF RM SEQ SLEEP SYNC TEST TEST1 TEST2 TOUCH TR UNAME UPTIME WC MD5SUM SHA256SUM
  GREP SED AWK HEXDUMP XXD VI
  PS KILL KILLALL PKILL PGREP PIDOF FREE
  MOUNT UMOUNT DMESG FLASHCP FLASH_ERASEALL DEVMEM HALT REBOOT POWEROFF
  IFCONFIG FEATURE_IFCONFIG_STATUS BRCTL FEATURE_BRCTL_FANCY FEATURE_BRCTL_SHOW NC NC_SERVER TELNETD
  FEATURE_TELNETD_STANDALONE UDHCPD PING
"
RESCUE=$USERSPACE/rescue
if [[ ! -x $RESCUE/busybox || "$(cat "$RESCUE/busybox.applets" 2>/dev/null)" != "$RESCUE_APPLETS" ]]; then
    if [[ ! -d $BB_SRC ]]; then
        fetch "$BUSYBOX_URL" "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" "$BUSYBOX_SHA"
        tar -xjf "$SRC/busybox-${BUSYBOX_VER}.tar.bz2" -C "$SRC"
    fi
    log "rescue busybox (allnoconfig + RESCUE_APPLETS, static)"
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
        [[ -z $missing ]] || { log "busybox symbols that did not stick:$missing"; exit 4; }
        quiet busybox-rescue make -j"$JOBS" CROSS_COMPILE=$CROSS CC="$MUSL_GCC"
    )
    "${CROSS}strip" -o "$RESCUE/busybox" "$BB_SRC/busybox"
    printf '%s' "$RESCUE_APPLETS" > "$RESCUE/busybox.applets"
    file "$RESCUE/busybox" | grep -q 'statically linked' || { log "rescue busybox is not static"; exit 4; }
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
    cp -fL "$LIBNL_PREFIX/lib/${l}.so.200"      "$USERSPACE/usr/lib/${l}.so.200"
    ln -sf "${l}.so.200"                        "$USERSPACE/usr/lib/${l}.so"
done

# ---------------------------------------------------------------------------
# 4) hostapd
# ---------------------------------------------------------------------------
HA_SRC=$SRC/hostapd-${HOSTAPD_VER}
if [[ ! -x "$USERSPACE/usr/sbin/hostapd" ]]; then
    fetch "$HOSTAPD_URL" "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" "$HOSTAPD_SHA"
    rm -rf "$HA_SRC"
    tar -xzf "$SRC/hostapd-${HOSTAPD_VER}.tar.gz" -C "$SRC"
    (
        cd "$HA_SRC/hostapd"
        cp "$COMMON/hostapd-build.config" .config
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
    cp -f "$HA_SRC/hostapd/hostapd" "$USERSPACE/usr/sbin/hostapd"
fi

# Strip everything, the biggest win is hostapd.
log "strip"
"${CROSS}strip" \
    "$USERSPACE/bin/busybox" \
    "$USERSPACE/usr/sbin/hostapd" \
    "$USERSPACE/usr/lib"/libnl-*.so.200 \
    "$USERSPACE/lib/libc.so" 2>/dev/null || true

log "sizes:"
du -h "$USERSPACE/bin/busybox" \
      "$USERSPACE/usr/sbin/hostapd" \
      "$USERSPACE/usr/lib"/libnl-*.so.200 \
      "$USERSPACE/lib"/{libc.so,ld-musl-riscv32.so.1}
log "userspace done at $USERSPACE"
