#!/bin/sh
# GStreamer bundles and dongle firmware, kept on the "assets" release instead of in git.
#
# usage: assets.sh fetch gstreamer <platform>...
#        assets.sh fetch livi-link
#        assets.sh publish gstreamer <platform> <archive>
#        assets.sh publish livi-link
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=assets
URL=${LIVI_ASSETS_URL:-https://github.com/f-io/LIVI/releases/download/$TAG}

sha() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

short() {
  printf '%s' "$1" | cut -c1-16
}

download() {
  curl -fsSL --retry 3 --retry-all-errors -o "$2" "$URL/$1"
  [ "$(sha "$2")" = "$3" ] || { echo "$1: checksum mismatch" >&2; rm -f "$2"; exit 1; }
}

upload() {
  if gh release view "$TAG" --json assets -q '.assets[].name' | grep -qx "$2"; then
    echo "$2 already published"
    return
  fi
  tmp=$(mktemp -d)
  cp "$1" "$tmp/$2"
  gh release upload "$TAG" "$tmp/$2"
  rm -rf "$tmp"
  echo "published $2"
}

gst_asset() {
  echo "gstreamer-$1-$(short "$2").tar.gz"
}

fw_field() {
  sed -n "s/^  \"$2\": \"\(.*\)\",\{0,1\}$/\1/p" "$1"
}

fw_asset() {
  echo "${1%.lfwb}-$(short "$2").lfwb"
}

fetch_gstreamer() {
  dir=$ROOT/assets/gstreamer/$1
  want=$(cat "$dir/sha256.txt")
  [ "$(cat "$dir/.fetched" 2>/dev/null || true)" = "$want" ] && return
  tmp=$(mktemp)
  download "$(gst_asset "$1" "$want")" "$tmp" "$want"
  find "$dir" -mindepth 1 -maxdepth 1 ! -name sha256.txt -exec rm -rf {} +
  tar -xzf "$tmp" -C "$ROOT/assets/gstreamer"
  rm -f "$tmp"
  echo "$want" > "$dir/.fetched"
  echo "fetched gstreamer $1"
}

fetch_livi_link() {
  for json in "$ROOT"/assets/livi-link/*/firmware.json; do
    [ -f "$json" ] || continue
    dir=$(dirname "$json")
    file=$(fw_field "$json" file)
    want=$(fw_field "$json" sha256)
    if [ -f "$dir/$file" ] && [ "$(sha "$dir/$file")" = "$want" ]; then continue; fi
    download "$(fw_asset "$file" "$want")" "$dir/$file.part" "$want"
    mv "$dir/$file.part" "$dir/$file"
    echo "fetched $file"
  done
}

publish_gstreamer() {
  dir=$ROOT/assets/gstreamer/$1
  sum=$(sha "$2")
  upload "$2" "$(gst_asset "$1" "$sum")"
  mkdir -p "$dir"
  for meta in version.txt packages.txt; do
    tar -xzf "$2" -O "$1/$meta" > "$dir/$meta" 2>/dev/null || rm -f "$dir/$meta"
  done
  echo "$sum" > "$dir/sha256.txt"
}

publish_livi_link() {
  for json in "$ROOT"/assets/livi-link/*/firmware.json; do
    [ -f "$json" ] || continue
    dir=$(dirname "$json")
    file=$(fw_field "$json" file)
    [ -f "$dir/$file" ] || continue
    want=$(fw_field "$json" sha256)
    [ "$(sha "$dir/$file")" = "$want" ] || { echo "$file: does not match firmware.json" >&2; exit 1; }
    upload "$dir/$file" "$(fw_asset "$file" "$want")"
  done
}

case "${1:-} ${2:-}" in
  "fetch gstreamer") shift 2; for p in "$@"; do fetch_gstreamer "$p"; done ;;
  "fetch livi-link") fetch_livi_link ;;
  "publish gstreamer") publish_gstreamer "$3" "$4" ;;
  "publish livi-link") publish_livi_link ;;
  *) sed -n '4,7s/^# //p' "$0" >&2; exit 2 ;;
esac
