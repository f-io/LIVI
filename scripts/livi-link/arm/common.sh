: "${BOARD:?set BOARD before sourcing arm/common.sh}"
ARM=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(cd "$ARM/../common" && pwd)
REPO=$(cd "$ARM/../../.." && pwd)
TOP=${TOP:-$HOME/LocalDev/$BOARD-kernel}
JOBS=${JOBS:-$(nproc)}
CROSS_COMPILE=${CROSS_COMPILE:-arm-linux-gnu-}

source "$COMMON/kernel.sh"

# The musl target keeps livid fully static like everything else on the rootfs.
build_livid() {
  local native=$REPO/native rtarget=armv7-unknown-linux-musleabihf livid
  log "cargo build livid ($rtarget)"
  (
    cd "$native"
    export "CARGO_TARGET_$(echo "$rtarget" | tr 'a-z-' 'A-Z_')_LINKER=${CROSS_COMPILE}gcc"
    export "CC_${rtarget//-/_}=${CROSS_COMPILE}gcc"
    export "AR_${rtarget//-/_}=${CROSS_COMPILE}ar"
    cargo build --profile embedded -p livid --target "$rtarget"
  )
  livid=${CARGO_TARGET_DIR:-$native/target}/$rtarget/embedded/livid
  [[ -x $livid ]] || { log "missing $livid"; exit 4; }
  cp -f "$livid" "$OUT/livid"
  log "  livid: $(stat -c%s "$OUT/livid") B"
}
