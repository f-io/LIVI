: "${BOARD:?set BOARD before sourcing riscv/common.sh}"
RISCV=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(cd "$RISCV/../common" && pwd)
REPO=$(cd "$RISCV/../../.." && pwd)
TOP=${TOP:-$HOME/LocalDev/$BOARD-kernel}
JOBS=${JOBS:-$(nproc)}
# A riscv64 toolchain builds the rv32 kernel as well.
CROSS_COMPILE=${CROSS_COMPILE:-riscv64-linux-gnu-}
# The Andes gcc builds the userspace and links livid. CI unpacks it under ~/andes.
TC_BIN=${TC_BIN:-$HOME/andes/nds32le-linux-glibc-v5d/bin}

source "$COMMON/kernel.sh"

# Rust ships no std for riscv32 Linux, so nightly builds it, and the Andes gcc links livid
# statically (.cargo/config.toml).
build_livid() {
  local native=$REPO/native rtarget=riscv32gc-unknown-linux-gnu livid
  local PATH=$TC_BIN:$PATH
  command -v riscv32-linux-gcc >/dev/null || { log "no riscv32-linux-gcc in PATH (set TC_BIN)"; exit 3; }
  log "cargo +nightly build livid ($rtarget)"
  ( cd "$native" && cargo +nightly build --profile embedded -p livid --target "$rtarget" -Z build-std=std,panic_abort )
  livid=${CARGO_TARGET_DIR:-$native/target}/$rtarget/embedded/livid
  [[ -x $livid ]] || { log "missing $livid"; exit 4; }
  cp -f "$livid" "$OUT/livid"
  log "  livid: $(stat -c%s "$OUT/livid") B"
}
