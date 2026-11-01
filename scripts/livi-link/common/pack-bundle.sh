#!/usr/bin/env bash
# Pack a board's firmware images into one .lfwb bundle, the same container on every board:
#   Header (8 B):    magic "LFWB" | version:u8=1 | count:u8 | reserved:u16
#   Desc (N*12 B):   type:u8 | flags:u8 | reserved:u16 | length:u32 | crc32:u32
#   Payload:         images concatenated in descriptor order (no padding)
# The type is the MTD index the image goes to, BUNDLE_IMAGES in the board's board.sh lists them.
#   pack-bundle.sh <board dir>
set -euo pipefail

LOG_TAG=bundle
source "$(cd "${1:?usage: pack-bundle.sh <board dir>}" && pwd)/board.sh"

for entry in "${BUNDLE_IMAGES[@]}"; do
  [[ -f ${entry#*:} ]] || { log "missing ${entry#*:}"; exit 1; }
done

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

mkdir -p "$(dirname "$BUNDLE")"
{
  # shellcheck disable=SC2059
  printf "LFWB\x01$(printf '\\x%02x' "${#BUNDLE_IMAGES[@]}")\x00\x00"
  for entry in "${BUNDLE_IMAGES[@]}"; do
    typ=${entry%%:*} img=${entry#*:}
    # shellcheck disable=SC2059
    printf "$(printf '\\x%02x' "$typ")\x00\x00\x00"
    le32 "$(wc -c < "$img" | tr -d ' ')"
    crc32le "$img" | tee "$WORK/crc$typ"
  done
  for entry in "${BUNDLE_IMAGES[@]}"; do cat "${entry#*:}"; done
} > "$BUNDLE"

log "wrote $BUNDLE: $(wc -c < "$BUNDLE" | tr -d ' ') B"
for entry in "${BUNDLE_IMAGES[@]}"; do
  typ=${entry%%:*} img=${entry#*:}
  crc=$(od -An -tx1 "$WORK/crc$typ" | awk '{ print $4 $3 $2 $1 }')
  log "  type=$typ  len=$(wc -c < "$img" | tr -d ' ')  crc32=$crc"
done
