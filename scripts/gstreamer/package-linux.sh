#!/usr/bin/env bash
set -euo pipefail

OUT="${1:-assets/gstreamer/linux-x64}"

copy_required() {
  local src="$1"
  local dst="$2"

  if [[ ! -e "$src" ]]; then
    echo "missing required file: $src" >&2
    exit 1
  fi

  cp -p "$src" "$dst"
}

copy_if_exists() {
  local src="$1"
  local dst="$2"

  if [[ -e "$src" ]]; then
    cp -p "$src" "$dst"
  fi
}

real_path() {
  python3 - <<'PY' "$1"
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
}

is_system_excluded() {
  local p="$1"
  case "$p" in
    /lib64/ld-linux-*|/lib/ld-linux-*|/lib/x86_64-linux-gnu/ld-linux-*|/lib/aarch64-linux-gnu/ld-linux-*|\
        */libc.so.*|*/libm.so.*|*/libmvec.so.*|*/libpthread.so.*|*/libdl.so.*|*/librt.so.*|\
    */libgcc_s.so.*|*/libstdc++.so.*|\
    */libpulse.so.*|*/libpulsecommon-*.so|*/libasound.so.*|\
    */libdbus-1.so.*|*/libsystemd.so.*|\
    */libX*.so.*|*/libxcb*.so.*|*/libwayland-*.so.*|\
    */libdrm.so.*|*/libgbm.so.*|*/libGL.so.*|*/libEGL.so.*|*/libGLESv2.so.*|\
    */libGLdispatch.so.*|*/libOpenGL.so.*|*/libglapi.so.*|*/libgallium*.so.*|\
    */libva.so.*|*/libva-drm.so.*|*/libva-x11.so.*|*/libv4l2.so.*|*/libv4lconvert.so.*|\
    */libvulkan.so.*|*/libudev.so.*|*/libgudev-1.0.so.*|\
    */libssh.so.*|*/libssh2.so.*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

queue_dep() {
  local dep="$1"
  [[ -n "$dep" ]] || return 0
  [[ -f "$dep" ]] || return 0

  local real
  real="$(real_path "$dep")"

  if is_system_excluded "$real"; then
    return 0
  fi

  local link_name
  link_name="$(basename "$dep")"

  if [[ -n "${SEEN_LIBS[$dep]:-}" ]]; then
    return 0
  fi

  SEEN_LIBS["$dep"]=1
  PENDING_LIBS+=("$dep")
}

scan_deps() {
  local file="$1"

  ldd "$file" 2>/dev/null \
    | awk '
        /=> \// { print $3 }
        /^\// { print $1 }
      ' \
    | sort -u \
    | while read -r dep; do
        [[ -n "$dep" ]] || continue
        [[ -f "$dep" ]] || continue
        echo "$dep"
      done
}

copy_binary_and_deps() {
  local src="$1"
  local name="${2:-$(basename "$src")}"

  copy_required "$src" "$OUT/bin/$name"

  while read -r dep; do
    queue_dep "$dep"
  done < <(scan_deps "$src")
}

copy_libexec_and_deps() {
  local src="$1"
  local name="${2:-$(basename "$src")}"

  copy_required "$src" "$OUT/libexec/gstreamer-1.0/$name"

  while read -r dep; do
    queue_dep "$dep"
  done < <(scan_deps "$src")
}

copy_plugin_and_deps() {
  local src="$1"
  local name="${2:-$(basename "$src")}"

  copy_required "$src" "$OUT/lib/gstreamer-1.0/$name"

  while read -r dep; do
    queue_dep "$dep"
  done < <(scan_deps "$src")
}

# HW-specific plugins (v4l2/kms/va)
copy_plugin_optional_and_deps() {
  local src="$1"
  local name="${2:-$(basename "$src")}"

  if [[ ! -e "$src" ]]; then
    echo "skip optional plugin: $src" >&2
    return 0
  fi

  cp -p "$src" "$OUT/lib/gstreamer-1.0/$name"

  while read -r dep; do
    queue_dep "$dep"
  done < <(scan_deps "$src")
}

copy_all_pending_libs() {
  local idx=0

  while [[ $idx -lt ${#PENDING_LIBS[@]} ]]; do
    local lib="${PENDING_LIBS[$idx]}"
    idx=$((idx + 1))

    local link_name real_name real_base
    link_name="$(basename "$lib")"
    real_name="$(real_path "$lib")"
    real_base="$(basename "$real_name")"

    if [[ ! -e "$OUT/lib/$real_base" ]]; then
      copy_required "$real_name" "$OUT/lib/$real_base"
    fi

    if [[ "$link_name" != "$real_base" && ! -e "$OUT/lib/$link_name" ]]; then
      ln -s "$real_base" "$OUT/lib/$link_name"
    fi

    while read -r dep; do
      queue_dep "$dep"
    done < <(scan_deps "$real_name")
  done
}

find_plugin_dir() {
  local arch_libdir
  arch_libdir="$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || true)"

  local candidates=()

  if [[ -n "$arch_libdir" ]]; then
    candidates+=(
      "/usr/lib/${arch_libdir}/gstreamer-1.0"
      "/lib/${arch_libdir}/gstreamer-1.0"
    )
  fi

  candidates+=(
    "/usr/lib/gstreamer-1.0"
    "/lib/gstreamer-1.0"
  )

  for p in "${candidates[@]}"; do
    if [[ -d "$p" ]]; then
      echo "$p"
      return 0
    fi
  done

  return 1
}

find_scanner() {
  local arch_libdir
  arch_libdir="$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || true)"

  local candidates=()

  if [[ -n "$arch_libdir" ]]; then
    candidates+=(
      "/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
      "/usr/lib/${arch_libdir}/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"
      "/usr/lib/${arch_libdir}/gstreamer-1.0/gst-plugin-scanner"
    )
  fi

  candidates+=(
    "/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"
    "/usr/lib/gstreamer-1.0/gst-plugin-scanner"
  )

  for p in "${candidates[@]}"; do
    if [[ -x "$p" ]]; then
      echo "$p"
      return 0
    fi
  done

  return 1
}

GST_LAUNCH="$(command -v gst-launch-1.0 || true)"
GST_INSPECT="$(command -v gst-inspect-1.0 || true)"
GST_DEVICE_MONITOR="$(command -v gst-device-monitor-1.0 || true)"
PLUGIN_DIR="$(find_plugin_dir || true)"
SCANNER="$(find_scanner || true)"

if [[ -z "$GST_LAUNCH" ]]; then
  echo "gst-launch-1.0 not found" >&2
  exit 1
fi

if [[ -z "$GST_INSPECT" ]]; then
  echo "gst-inspect-1.0 not found" >&2
  exit 1
fi

if [[ -z "$GST_DEVICE_MONITOR" ]]; then
  echo "gst-device-monitor-1.0 not found" >&2
  exit 1
fi

if [[ -z "$PLUGIN_DIR" ]]; then
  echo "gstreamer plugin directory not found" >&2
  exit 1
fi

echo "Using gst-launch:         $GST_LAUNCH"
echo "Using gst-inspect:        $GST_INSPECT"
echo "Using gst-device-monitor: $GST_DEVICE_MONITOR"
echo "Using plugin dir:         $PLUGIN_DIR"
if [[ -n "$SCANNER" ]]; then
  echo "Using scanner:            $SCANNER"
else
  echo "No gst-plugin-scanner found; continuing without it"
fi

rm -rf "$OUT"
mkdir -p \
  "$OUT/bin" \
  "$OUT/lib" \
  "$OUT/lib/gstreamer-1.0" \
  "$OUT/libexec/gstreamer-1.0"

declare -A SEEN_LIBS=()
declare -a PENDING_LIBS=()

# bin
copy_binary_and_deps "$GST_LAUNCH" "gst-launch-1.0"
copy_binary_and_deps "$GST_INSPECT" "gst-inspect-1.0"
copy_binary_and_deps "$GST_DEVICE_MONITOR" "gst-device-monitor-1.0"

# libexec
if [[ -n "$SCANNER" ]]; then
  copy_libexec_and_deps "$SCANNER" "gst-plugin-scanner"
fi

plugins=(
  # core
  libgstapp.so
  libgstcoreelements.so
  libgsttypefindfunctions.so
  libgstautodetect.so
  # audio
  libgstaudioconvert.so
  libgstaudiofx.so
  libgstaudiomixer.so
  libgstaudioparsers.so
  libgstaudiorate.so
  libgstaudioresample.so
  libgstaudiotestsrc.so
  libgstequalizer.so
  libgstinterleave.so
  libgstlevel.so
  libgstrawparse.so
  libgstvolume.so
  libgstpulseaudio.so
  libgstalsa.so
  libgstfaad.so
  libgstopus.so
  libgstrtp.so
  libgstudp.so
  libgstrtpmanager.so
  # video parse + scale + GL sink (cross-platform)
  libgstvideoparsersbad.so
  libgstvideoconvertscale.so
  libgstopengl.so
  libgstlibav.so
)

for plugin in "${plugins[@]}"; do
  copy_plugin_and_deps "$PLUGIN_DIR/$plugin"
done

# video HW decode + DRM/KMS sink — host-dependent
optional_plugins=(
  libgstvideo4linux2.so  # v4l2h264dec/v4l2h265dec (Pi 4 stateful M2M)
  libgstv4l2codecs.so    # v4l2slh264dec/v4l2slh265dec (Pi 5 stateless HEVC)
  libgstkms.so           # kmssink (DRM overlay plane, kiosk)
  libgstva.so            # vah264dec/vah265dec (x86 VA-API)
  libgstwaylandsink.so   # waylandsink (wlroots/cage)
)

for plugin in "${optional_plugins[@]}"; do
  copy_plugin_optional_and_deps "$PLUGIN_DIR/$plugin"
done

# All libs
copy_all_pending_libs

# Make the bundle relocatabl
if ! command -v patchelf >/dev/null 2>&1; then
  echo "ERROR: patchelf not found. The bundle would not be relocatable and video would silently fail on hosts without a system GStreamer (e.g. Raspberry Pi OS Lite). Install patchelf and re-run." >&2
  exit 1
fi
echo "Patching RPATHs to \$ORIGIN (relocatable bundle)"
for f in "$OUT"/lib/*.so*;               do [ -f "$f" ] && patchelf --set-rpath '$ORIGIN' "$f" 2>/dev/null || true; done
for f in "$OUT"/lib/gstreamer-1.0/*.so;  do [ -f "$f" ] && patchelf --set-rpath '$ORIGIN/..' "$f" 2>/dev/null || true; done
for f in "$OUT"/bin/*;                   do [ -f "$f" ] && patchelf --set-rpath '$ORIGIN/../lib' "$f" 2>/dev/null || true; done
for f in "$OUT"/libexec/gstreamer-1.0/*; do [ -f "$f" ] && patchelf --set-rpath '$ORIGIN/../../lib' "$f" 2>/dev/null || true; done

# Record the Debian package versions that fed this bundle.
{
  dpkg-query -W -f='${binary:Package} ${Version}\n' 2>/dev/null \
    | grep -E '^(gstreamer1\.0-|libgstreamer)' | grep -v '^libgstreamer-plugins-bad' || true
  cat /usr/share/gst-build/meson-builds.txt 2>/dev/null || true
} | sort > "$OUT/packages.txt"
echo "Wrote provenance: $OUT/packages.txt ($(wc -l < "$OUT/packages.txt") packages)"

echo "Created linux-x64/linux-arm64 GStreamer bundle at: $OUT"
echo "Bundle size:"
du -sh "$OUT"
echo "Top-level contents:"
find "$OUT" -maxdepth 3 | sort
