: "${BOARD:?set BOARD before sourcing mips/common.sh}"
MIPS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(cd "$MIPS/../common" && pwd)
REPO=$(cd "$MIPS/../../.." && pwd)
TOP=${TOP:-$HOME/LocalDev/$BOARD-kernel}
JOBS=${JOBS:-$(nproc)}
CROSS_COMPILE=${CROSS_COMPILE:-mipsel-buildroot-linux-musl-}
RUST_NIGHTLY=$(<"$COMMON/rust-nightly")

source "$COMMON/kernel.sh"

build_livid() {
  local native=$REPO/native rtarget=mipsel-unknown-linux-musl livid
  command -v mipsel-buildroot-linux-musl-gcc >/dev/null || {
    log "no mipsel-buildroot-linux-musl-gcc in PATH"
    exit 3
  }
  log "cargo +$RUST_NIGHTLY build livid ($rtarget)"
  (cd "$native" && cargo "+$RUST_NIGHTLY" build --profile embedded -p livid --target "$rtarget" -Z build-std=std,panic_abort)
  livid=${CARGO_TARGET_DIR:-$native/target}/$rtarget/embedded/livid
  [[ -x $livid ]] || {
    log "missing $livid"
    exit 4
  }
  cp -f "$livid" "$OUT/livid"
  log "  livid: $(stat -c%s "$OUT/livid") B"
}
