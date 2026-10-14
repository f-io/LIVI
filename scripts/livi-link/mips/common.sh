: "${BOARD:?set BOARD before sourcing mips/common.sh}"
MIPS=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(cd "$MIPS/../common" && pwd)
REPO=$(cd "$MIPS/../../.." && pwd)
TOP=${TOP:-$HOME/LocalDev/$BOARD-kernel}
JOBS=${JOBS:-$(nproc)}
# Debian dropped mipsel entirely from trixie on, so this is Bootlin's toolchain
# (toolchains.bootlin.com/releases_mips32el.html), not a distro package; see the CI workflow for
# how it's fetched.
CROSS_COMPILE=${CROSS_COMPILE:-mipsel-buildroot-linux-musl-}

source "$COMMON/kernel.sh"

# mipsel-unknown-linux-musl is Tier 3 (no prebuilt std), same as riscv32gc-unknown-linux-gnu, so
# nightly builds it (-Z build-std). The Bootlin gcc links livid statically (.cargo/config.toml).
build_livid() {
  local native=$REPO/native rtarget=mipsel-unknown-linux-musl livid
  command -v mipsel-buildroot-linux-musl-gcc >/dev/null || { log "no mipsel-buildroot-linux-musl-gcc in PATH"; exit 3; }
  log "cargo +nightly build livid ($rtarget)"
  ( cd "$native" && cargo +nightly build --profile embedded -p livid --target "$rtarget" -Z build-std=std,panic_abort )
  livid=${CARGO_TARGET_DIR:-$native/target}/$rtarget/embedded/livid
  [[ -x $livid ]] || { log "missing $livid"; exit 4; }
  cp -f "$livid" "$OUT/livid"
  log "  livid: $(stat -c%s "$OUT/livid") B"
}
