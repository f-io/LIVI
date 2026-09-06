#!/usr/bin/env bash
set -euo pipefail

# Modes:
#   (default)        build the full bundle into OUT (CI only, via gstreamer-assets.yml)
#   --relocate-node  only retarget gst_video.node at the committed bundle, never
#                    touch assets/ (used by build:mac; the bundle is built+committed
#                    in CI, regenerating it locally pollutes file owners/flags)
MODE="full"
if [[ "${1:-}" == "--relocate-node" ]]; then MODE="node"; shift || true; fi

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
  python3 - <<'PY' "$1"
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
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

    # Preserve versioned aliases (e.g. libjpeg.8.dylib -> libjpeg.8.3.2.dylib)
    if [[ "$link_name" != "$real_base" && ! -e "$OUT/lib/$link_name" ]]; then
      ln -s "$real_base" "$OUT/lib/$link_name"
    fi

    while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$real_name")
  done
}

# rpath/signing helpers + addon relocation, shared by both modes
resign() {
  [[ -e "$1" ]] || return 0
  command -v codesign >/dev/null 2>&1 && codesign --force --sign - "$1" >/dev/null 2>&1 || true
}
add_rpath() {
  local rp="$1" f="$2"
  [[ -e "$f" ]] || return 0
  if install_name_tool -add_rpath "$rp" "$f" 2>/dev/null; then resign "$f"; fi
}
# Point gst_video.node at the bundle. It ships asar-unpacked, so from
# .../node_modules/gst-video/build/Release/ the bundle sits 5 levels up at
# Contents/Resources/gstreamer/macos-arm64/lib. Bundle rpath FIRST (self-contained),
# the system framework kept as a dev fallback.
relocate_node() {
  local REPO_ROOT NODE BUNDLE_RPATH rp
  REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  NODE="$REPO_ROOT/native/gst-video/build/Release/gst_video.node"
  BUNDLE_RPATH="@loader_path/../../../../../gstreamer/macos-arm64/lib"
  if [[ ! -e "$NODE" ]]; then
    echo "WARN: $NODE not built yet; build the addon before packaging" >&2
    return 0
  fi
  echo "==> Relocating gst_video.node rpath -> bundle (system framework kept as dev fallback)"
  while read -r rp; do
    case "$rp" in
      "$BUNDLE_RPATH" | *GStreamer.framework*)
        install_name_tool -delete_rpath "$rp" "$NODE" 2>/dev/null || true ;;
    esac
  done < <(otool -l "$NODE" 2>/dev/null | awk '/LC_RPATH/{getline;getline;print $2}')
  add_rpath "$BUNDLE_RPATH" "$NODE"
  add_rpath "$GST_ROOT/lib" "$NODE"
}

# --relocate-node: only the addon, never regenerate or touch the committed bundle
if [[ "$MODE" == "node" ]]; then
  relocate_node
  exit 0
fi

rm -rf "$OUT"
mkdir -p \
  "$OUT/bin" \
  "$OUT/lib" \
  "$OUT/lib/gstreamer-1.0" \
  "$OUT/libexec/gstreamer-1.0"

PENDING_LIBS=()

# bin
copy_bin_and_deps "$GST_ROOT/bin/gst-launch-1.0"
copy_bin_and_deps "$GST_ROOT/bin/gst-inspect-1.0"
copy_bin_and_deps "$GST_ROOT/bin/gst-device-monitor-1.0"

# libexec
copy_libexec_and_deps "$GST_ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"

plugins=(
  # core
  libgstapp.dylib
  libgstcoreelements.dylib
  libgsttypefindfunctions.dylib
  libgstautodetect.dylib
  # audio
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
  # video parse + decode + scale
  libgstvideoparsersbad.dylib
  libgstapplemedia.dylib
  libgstlibav.dylib
  libgstvideoconvertscale.dylib
  # video sinks
  libgstopengl.dylib
  libgstosxvideo.dylib
)

# PATCHED_APPLEMEDIA (from build-patched-macos.sh) replaces the prebuilt applemedia plugin
# with the locally built, patched one (vtdec low-latency + full-range HEVC). Same @rpath deps,
# so dep scanning and the later rpath/signing pass treat it like any other plugin.
for plugin in "${plugins[@]}"; do
  src="$GST_ROOT/lib/gstreamer-1.0/$plugin"
  if [[ "$plugin" == libgstapplemedia.dylib && -n "${PATCHED_APPLEMEDIA:-}" ]]; then
    copy_required "$PATCHED_APPLEMEDIA" "$OUT/lib/gstreamer-1.0/$plugin"
    while read -r dep; do queue_dep "$dep"; done < <(scan_deps "$PATCHED_APPLEMEDIA")
    continue
  fi
  copy_plugin_and_deps "$src"
done

# Umbrella framework binary (kept for parity with prior bundles)
copy_required "$GST_ROOT/lib/GStreamer" "$OUT/lib/GStreamer"

# all transitive libs
copy_all_pending_libs

# Make the bundle self-contained (most framework Mach-Os already carry @loader_path
# rpaths; add the few that are missing and ad-hoc re-sign).
echo "==> Relocating rpaths to @loader_path + ad-hoc signing where needed"
for f in "$OUT"/lib/*.dylib "$OUT/lib/GStreamer"; do add_rpath "@loader_path" "$f"; done
for f in "$OUT"/lib/gstreamer-1.0/*.dylib; do add_rpath "@loader_path/.." "$f"; done
for f in "$OUT"/bin/*; do add_rpath "@loader_path/../lib" "$f"; done
add_rpath "@loader_path/../../lib" "$OUT/libexec/gstreamer-1.0/gst-plugin-scanner"

relocate_node

echo "Created macOS GStreamer bundle at: $OUT"
echo "Bundle size:"
du -sh "$OUT"
echo "Top-level contents:"
find "$OUT" -maxdepth 3 | sort
