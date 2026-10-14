#!/usr/bin/env bash
set -euo pipefail

# CI only, via gstreamer-assets.yml.

OUT="${1:-assets/gstreamer/macos-arm64}"
GST_ROOT="/Library/Frameworks/GStreamer.framework/Versions/1.0"

copy_required() {
  local src="$1"
  local dst="$2"

  if [[ ! -e "$src" ]]; then
    echo "missing required file: $src" >&2
    exit 1
  fi

  cp -p "$src" "$dst"
}

real_path() {
  local p=$1 link dir
  while [ -L "$p" ]; do
    link=$(readlink "$p")
    case "$link" in
      /*) p=$link ;;
      *) dir=$(dirname "$p"); p=${dir%/}/$link ;;
    esac
  done
  dir=$(cd -P "$(dirname "$p")" 2>/dev/null && pwd -P) || { printf '%s\n' "$p"; return; }
  printf '%s/%s\n' "${dir%/}" "$(basename "$p")"
}

# Only follow @rpath deps, system libs (/usr/lib, /System) are absolute and skipped
scan_deps() {
  local file="$1"
  otool -L "$file" 2>/dev/null \
    | awk '/^\t@rpath\// { sub(/^@rpath\//, "", $1); print $1 }' \
    | sort -u
}

SEEN_LIBS=""

queue_dep() {
  local name="$1"
  [[ -n "$name" ]] || return 0
  [[ -e "$GST_ROOT/lib/$name" ]] || return 0
  case " $SEEN_LIBS " in *" $name "*) return 0 ;; esac
  SEEN_LIBS="$SEEN_LIBS $name"
  PENDING_LIBS+=("$name")
}

copy_bin_and_deps() {
  copy_required "$1" "$OUT/bin/$(basename "$1")"
  while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$1")
}

copy_libexec_and_deps() {
  copy_required "$1" "$OUT/libexec/gstreamer-1.0/$(basename "$1")"
  while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$1")
}

copy_plugin_and_deps() {
  copy_required "$1" "$OUT/lib/gstreamer-1.0/$(basename "$1")"
  while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$1")
}

copy_all_pending_libs() {
  local idx=0
  while [[ $idx -lt ${#PENDING_LIBS[@]} ]]; do
    local link_name="${PENDING_LIBS[$idx]}"
    idx=$((idx + 1))

    local real_name real_base
    real_name="$(real_path "$GST_ROOT/lib/$link_name")"
    real_base="$(basename "$real_name")"

    if [[ ! -e "$OUT/lib/$real_base" ]]; then
      copy_required "$real_name" "$OUT/lib/$real_base"
    fi

    if [[ "$link_name" != "$real_base" && ! -e "$OUT/lib/$link_name" ]]; then
      ln -s "$real_base" "$OUT/lib/$link_name"
    fi

    while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$real_name")
  done
}

resign() {
  [[ -e "$1" ]] || return 0
  command -v codesign >/dev/null 2>&1 && codesign --force --sign - "$1" >/dev/null 2>&1 || true
}
add_rpath() {
  local rp="$1" f="$2"
  [[ -e "$f" ]] || return 0
  if install_name_tool -add_rpath "$rp" "$f" 2>/dev/null; then resign "$f"; fi
}

rm -rf "$OUT"
mkdir -p \
  "$OUT/bin" \
  "$OUT/lib" \
  "$OUT/lib/gstreamer-1.0" \
  "$OUT/libexec/gstreamer-1.0"

PENDING_LIBS=()

copy_bin_and_deps "$GST_ROOT/bin/gst-launch-1.0"
copy_bin_and_deps "$GST_ROOT/bin/gst-inspect-1.0"
copy_bin_and_deps "$GST_ROOT/bin/gst-device-monitor-1.0"

copy_libexec_and_deps "$GST_ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"

plugins=(
  libgstapp.dylib
  libgstcoreelements.dylib
  libgsttypefindfunctions.dylib
  libgstautodetect.dylib
  libgstaudioconvert.dylib
  libgstaudiofx.dylib
  libgstaudiomixer.dylib
  libgstaudioparsers.dylib
  libgstaudiorate.dylib
  libgstaudioresample.dylib
  libgstaudiotestsrc.dylib
  libgstequalizer.dylib
  libgstinterleave.dylib
  libgstlevel.dylib
  libgstosxaudio.dylib
  libgstrawparse.dylib
  libgstvolume.dylib
  libgstopus.dylib
  libgstrtp.dylib
  libgstudp.dylib
  libgstrtpmanager.dylib
  libgstvideoparsersbad.dylib
  libgstapplemedia.dylib
  libgstlibav.dylib
  libgstvideoconvertscale.dylib
  libgstopengl.dylib
  libgstosxvideo.dylib
  # video LIVI fetches from a URL: playbin3, hlsdemux2, souphttpsrc, qtdemux, tsdemux
  libgstplayback.dylib
  libgstadaptivedemux2.dylib
  libgstsoup.dylib
  libgstisomp4.dylib
  libgstmpegtsdemux.dylib
)

# PATCHED_APPLEMEDIA (from build-patched-macos.sh) replaces the prebuilt applemedia plugin
# with our patched build (vtdec low latency, full-range HEVC).
for plugin in "${plugins[@]}"; do
  src="$GST_ROOT/lib/gstreamer-1.0/$plugin"
  if [[ "$plugin" == libgstapplemedia.dylib && -n "${PATCHED_APPLEMEDIA:-}" ]]; then
    copy_required "$PATCHED_APPLEMEDIA" "$OUT/lib/gstreamer-1.0/$plugin"
    while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$PATCHED_APPLEMEDIA")
    continue
  fi
  copy_plugin_and_deps "$src"
done

# Both soup plugins dlopen libsoup, so otool never names it.
if [[ ! -e "$GST_ROOT/lib/libsoup-3.0.0.dylib" ]]; then
  echo "libsoup-3.0.0.dylib not found, souphttpsrc and hlsdemux2 would not load" >&2
  exit 1
fi
queue_dep libsoup-3.0.0.dylib

copy_required "$GST_ROOT/lib/GStreamer" "$OUT/lib/GStreamer"

copy_all_pending_libs

echo "==> Relocating rpaths to @loader_path + ad-hoc signing where needed"
for f in "$OUT"/lib/*.dylib "$OUT/lib/GStreamer"; do add_rpath "@loader_path" "$f"; done
for f in "$OUT"/lib/gstreamer-1.0/*.dylib; do add_rpath "@loader_path/.." "$f"; done
for f in "$OUT"/bin/*; do add_rpath "@loader_path/../lib" "$f"; done
add_rpath "@loader_path/../../lib" "$OUT/libexec/gstreamer-1.0/gst-plugin-scanner"

echo "Created macOS GStreamer bundle at: $OUT"
echo "Bundle size:"
du -sh "$OUT"
echo "Top-level contents:"
find "$OUT" -maxdepth 3 | sort
