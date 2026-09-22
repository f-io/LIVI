#!/usr/bin/env bash
# Pack the AX520 firmware images (boot uImage + rootfs squashfs) into one .lfwb
# bundle, same container format as the V821B one:
#   Header (8 B):    magic "LFWB" | version:u8=1 | count:u8 | reserved:u16
#   Desc (N*12 B):   type:u8 | flags:u8 | reserved:u16 | length:u32 | crc32:u32
#   Payload:         images concatenated in descriptor order (no padding)
#
# Type is the MTD index in ax520.dts: 3 = boot (kernel uImage), 6 = rootfs.
set -euo pipefail

LOG_TAG=bundle
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

BUNDLE="$OUT/livi-link-ax520.lfwb"
BOOT="$OUT/livi-link-ax520-boot.uimg"
ROOTFS="$OUT/livi-link-ax520-rootfs.bin"

[[ -f "$BOOT" ]]   || { log "missing $BOOT"; exit 1; }
[[ -f "$ROOTFS" ]] || { log "missing $ROOTFS"; exit 1; }

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
  for entry in "3:$BOOT" "6:$ROOTFS"; do
    typ=${entry%%:*} img=${entry#*:}
    # shellcheck disable=SC2059
    printf "$(printf '\\x%02x' "$typ")\x00\x00\x00"
    le32 "$(wc -c < "$img" | tr -d ' ')"
    crc32le "$img" | tee "$WORK/crc$typ"
  done
  cat "$BOOT" "$ROOTFS"
} > "$BUNDLE"

log "wrote $BUNDLE: $(wc -c < "$BUNDLE" | tr -d ' ') B"
for entry in "3:$BOOT" "6:$ROOTFS"; do
  typ=${entry%%:*} img=${entry#*:}
  crc=$(od -An -tx1 "$WORK/crc$typ" | awk '{ print $4 $3 $2 $1 }')
  log "  type=$typ  len=$(wc -c < "$img" | tr -d ' ')  crc32=$crc"
done
