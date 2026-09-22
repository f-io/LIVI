#!/usr/bin/env bash
# Dongle firmware from assets/livi-link, named for a release's flat asset list.
#
# usage: stage-firmware.sh <dest-dir>
set -euo pipefail
HERE=$(cd "$(dirname "$0")/../.." && pwd)
DEST=${1:?usage: stage-firmware.sh <dest-dir>}
mkdir -p "$DEST"

for dir in "$HERE"/assets/livi-link/*/; do
  target=$(basename "$dir")
  [ -f "$dir/firmware.json" ] || { echo "skip $target: no firmware.json yet" >&2; continue; }
  bundle=$dir/$(sed -n 's/^  "file": "\(.*\)",$/\1/p' "$dir/firmware.json")
  [ -f "$bundle" ] || { echo "$target: firmware.json names a bundle that is not there" >&2; exit 1; }
  cp "$bundle" "$DEST/"
  cp "$dir/firmware.json" "$DEST/$target.firmware.json"
  echo "staged $target: $(basename "$bundle")"
done
