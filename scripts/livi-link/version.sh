# Sourced by the firmware builds. CI passes both values in, a local build reads them off the tree.
_livi_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
: "${LIVI_VERSION:=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$_livi_root/package.json" | head -1)}"
: "${LIVI_BUILD:=$(git -C "$_livi_root" rev-parse --short=8 HEAD 2>/dev/null || echo unknown)}"
export LIVI_VERSION LIVI_BUILD
unset _livi_root

livi_firmware_json() {
  local bundle=$1 sum size
  if command -v sha256sum >/dev/null; then sum=$(sha256sum "$bundle"); else sum=$(shasum -a 256 "$bundle"); fi
  size=$(wc -c < "$bundle" | tr -d ' ')
  printf '{\n  "version": "%s",\n  "build": "%s",\n  "file": "%s",\n  "size": %s,\n  "sha256": "%s"\n}\n' \
    "$LIVI_VERSION" "$LIVI_BUILD" "$(basename "$bundle")" "$size" "${sum%% *}" \
    > "$(dirname "$bundle")/firmware.json"
}
