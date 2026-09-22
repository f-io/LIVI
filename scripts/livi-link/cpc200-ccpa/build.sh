#!/usr/bin/env bash
# Cross-builds the LIVI Link cpc200-ccpa dongle stack (armv7, static) into an
# asset directory: <out>/cpc200-ccpa.gz plus MANIFEST.md5. The gzip is
# deterministic (-9 -n), so an unchanged binary means nothing for CI to
# commit. Needs the rustup target and an arm-linux-gnueabihf toolchain.
set -euo pipefail

HERE=$(cd "$(dirname "$0")/../../.." && pwd)
OUT=${1:-$HERE/assets/livi-link/cpc200-ccpa}
CROSS=${CROSS:-arm-linux-gnueabihf-}
TARGET=${TARGET:-armv7-unknown-linux-gnueabihf}
HELPERD=$HERE/native/livi-helperd
PKG=livi-link
source "$HERE/scripts/livi-link/version.sh"
BIN=cpc200-ccpa

command -v "${CROSS}gcc" >/dev/null || { echo "no ${CROSS}gcc in PATH" >&2; exit 1; }
mkdir -p "$OUT"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> $BIN (cargo $TARGET)"
(
  cd "$HELPERD"
  export "CARGO_TARGET_$(echo "$TARGET" | tr 'a-z-' 'A-Z_')_LINKER=${CROSS}gcc"
  export "CC_${TARGET//-/_}=${CROSS}gcc"
  export "AR_${TARGET//-/_}=${CROSS}ar"
  export RUSTFLAGS="-C target-feature=+crt-static"
  cargo build -p "$PKG" --release --target "$TARGET"
)
cp "$HELPERD/target/$TARGET/release/$BIN" "$WORK/$BIN"
"${CROSS}strip" "$WORK/$BIN"

echo "==> packing"
# The dongle's glibc is 2.20, so the binary must carry its own: `file` spells a static build
# either "statically linked" or "static-pie linked", so check for the failure instead.
if file "$WORK/$BIN" | grep -q 'dynamically linked'; then
  echo "$BIN is dynamically linked; it will not run on the dongle" >&2
  exit 1
fi
gzip -9 -n -c "$WORK/$BIN" > "$OUT/$BIN.gz"
# What the stack no longer ships must leave the assets too, or it stays in the repo forever.
for gz in "$OUT"/*.gz; do
  [ -e "$gz" ] || continue
  [ "$(basename "$gz")" = "$BIN.gz" ] || { echo "   dropping stale $(basename "$gz")"; rm -f "$gz"; }
done

hashes() { if command -v md5sum >/dev/null; then md5sum "$@"; else command md5 -r "$@"; fi; }
(cd "$OUT" && hashes ./*.gz | awk '{ sub(/^\.\//, "", $2); print $1 "  " $2 }' | sort -k2 > MANIFEST.md5)
livi_firmware_json "$OUT/$BIN.gz"

ls -la "$OUT"
cat "$OUT/MANIFEST.md5"
