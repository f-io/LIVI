#!/usr/bin/env bash
# Pack V821B firmware images (mtd1 bootimg + mtd3 squashfs) into one .lfwb
# bundle. Format:
#   Header (8 B):    magic "LFWB" | version:u8=1 | count:u8 | reserved:u16
#   Desc (N*12 B):   type:u8 | flags:u8 | reserved:u16 | length:u32 | crc32:u32
#   Payload:         images concatenated in descriptor order (no padding)
#
# Types: 1 = mtd1 (bootimg), 3 = mtd3 (rootfs).
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
OUT_DIR=${OUT_DIR:-$HOME/LocalDev/tina-test/out}
BUNDLE="$OUT_DIR/livi-link-v821b.lfwb"
MTD1="$OUT_DIR/livi-link-v821b-mtd1.bin"
MTD3="$OUT_DIR/livi-link-v821b-mtd3.bin"

log(){ printf "\033[1;36m[livi-bundle]\033[0m %s\n" "$*"; }
[[ -f "$MTD1" ]] || { log "missing $MTD1"; exit 1; }
[[ -f "$MTD3" ]] || { log "missing $MTD3"; exit 1; }

le32() {
  local v=$1
  # shellcheck disable=SC2059
  printf "$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
    $((v & 255)) $((v >> 8 & 255)) $((v >> 16 & 255)) $((v >> 24 & 255)))"
}

# CRC-32 as the gzip trailer carries it, little-endian
crc32le() { gzip -c "$1" | tail -c 8 | head -c 4; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

{
  printf 'LFWB\x01\x02\x00\x00'
  for entry in "1:$MTD1" "3:$MTD3"; do
    typ=${entry%%:*} img=${entry#*:}
    # shellcheck disable=SC2059
    printf "$(printf '\\x%02x' "$typ")\x00\x00\x00"
    le32 "$(wc -c < "$img" | tr -d ' ')"
    crc32le "$img" | tee "$WORK/crc$typ"
  done
  cat "$MTD1" "$MTD3"
} > "$BUNDLE"

log "wrote $BUNDLE: $(wc -c < "$BUNDLE" | tr -d ' ') B"
for entry in "1:$MTD1" "3:$MTD3"; do
  typ=${entry%%:*} img=${entry#*:}
  crc=$(od -An -tx1 "$WORK/crc$typ" | awk '{ print $4 $3 $2 $1 }')
  log "  type=$typ  len=$(wc -c < "$img" | tr -d ' ')  crc32=$crc"
done
