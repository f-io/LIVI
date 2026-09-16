#!/usr/bin/env bash
# Cross-builds the LIVI Link V821B dongle-side Rust binary (livid, riscv32gc,
# nightly + build-std) into an asset directory: <out>/livid.gz + MANIFEST.md5.
# Deterministic gzip (-9 -n). Needs Rust nightly with rust-src and a
# riscv32-linux-gnu cross toolchain on PATH (Andes locally, Bootlin in CI).
set -euo pipefail

HERE=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:-$HERE/assets/livi-link/v821b_aic8800d80}
CROSS=${CROSS:-riscv32-linux-}
TARGET=${TARGET:-riscv32gc-unknown-linux-gnu}
HELPERD=$HERE/native/livi-helperd
BIN=livid

command -v "${CROSS}gcc" >/dev/null || { echo "no ${CROSS}gcc in PATH" >&2; exit 1; }
mkdir -p "$OUT"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> $BIN (cargo +nightly $TARGET, build-std)"
(
  cd "$HELPERD"
  cargo +nightly build -p "$BIN" --release \
    --target "$TARGET" \
    -Z build-std=std,panic_abort
)
cp "$HELPERD/target/$TARGET/release/$BIN" "$WORK/$BIN"
"${CROSS}strip" "$WORK/$BIN"

echo "==> packing"
if file "$WORK/$BIN" | grep -q 'dynamically linked'; then
  echo "$BIN is dynamically linked; it will not run on the dongle" >&2
  exit 1
fi
gzip -9 -n -c "$WORK/$BIN" > "$OUT/$BIN.gz"
for gz in "$OUT"/*.gz; do
  [ -e "$gz" ] || continue
  [ "$(basename "$gz")" = "$BIN.gz" ] || { echo "   dropping stale $(basename "$gz")"; rm -f "$gz"; }
done

hashes() { if command -v md5sum >/dev/null; then md5sum "$@"; else command md5 -r "$@"; fi; }
(cd "$OUT" && hashes ./*.gz | awk '{ sub(/^\.\//, "", $2); print $1 "  " $2 }' | sort -k2 > MANIFEST.md5)

ls -la "$OUT"
cat "$OUT/MANIFEST.md5"
