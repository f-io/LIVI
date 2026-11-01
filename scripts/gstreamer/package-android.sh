#!/usr/bin/env bash
set -euo pipefail

# CI only, via gstreamer-assets.yml.

GST_ROOT="$(cd "${1:?GStreamer Android SDK arm64 root required}" && pwd)"
OUT="${2:-assets/gstreamer/android-arm64}"
ANDROID_API="${ANDROID_API:-24}"
NDK="${ANDROID_NDK_HOME:?ANDROID_NDK_HOME not set}"
ABI=arm64-v8a

if [[ -z "${FAAD_DIR:-}" || ! -f "$FAAD_DIR/libgstfaad.a" || ! -f "$FAAD_DIR/libfaad.a" ]]; then
  echo "FAAD_DIR not set or incomplete, run build-faad-android.sh first" >&2
  exit 1
fi
FAAD_DIR="$(cd "$FAAD_DIR" && pwd)"
command -v pkg-config >/dev/null || { echo "pkg-config not on PATH" >&2; exit 1; }

plugins=(
  # core
  app
  coreelements
  typefindfunctions
  autodetect
  # audio
  audioconvert
  audiofx
  audiomixer
  audioparsers
  audiorate
  audioresample
  audiotestsrc
  equalizer
  interleave
  level
  rawparse
  volume
  opensles
  faad
  opus
  rtp
  udp
  rtpmanager
  # video parse + scale + GL sink, MediaCodec decoders
  videoparsersbad
  videoconvertscale
  jpeg
  videofilter
  opengl
  libav
  androidmedia
  # video LIVI fetches from a URL: playbin3, hlsdemux2, souphttpsrc, qtdemux, tsdemux
  playback
  adaptivedemux2
  soup
  isomp4
  mpegtsdemux
)

# What gstreamer-rs binds, plus faad's own libraries.
deps=(
  glib-2.0
  gobject-2.0
  gio-2.0
  gstreamer-1.0
  gstreamer-base-1.0
  gstreamer-app-1.0
  gstreamer-audio-1.0
  gstreamer-video-1.0
  gstreamer-controller-1.0
  gstreamer-pbutils-1.0
  gstreamer-tag-1.0
)

for p in "${plugins[@]}"; do
  [[ "$p" == faad ]] && continue
  [[ -f "$GST_ROOT/lib/gstreamer-1.0/libgst$p.a" ]] || { echo "SDK lacks plugin $p" >&2; exit 1; }
done

work="$(mktemp -d)"
stage="$work/stage"
mkdir -p "$work/jni" "$stage"

cat > "$work/jni/Application.mk" <<EOF
APP_ABI := $ABI
APP_PLATFORM := android-$ANDROID_API
APP_STL := c++_static
EOF

cat > "$work/jni/Android.mk" <<EOF
LOCAL_PATH := \$(call my-dir)

include \$(CLEAR_VARS)
LOCAL_MODULE := livi_gst_link
LOCAL_SRC_FILES := link.c
LOCAL_SHARED_LIBRARIES := gstreamer_android
include \$(BUILD_SHARED_LIBRARY)

GSTREAMER_ROOT := $GST_ROOT
GSTREAMER_PLUGINS := ${plugins[*]}
GSTREAMER_EXTRA_DEPS := ${deps[*]}
GSTREAMER_EXTRA_LIBS := -L$FAAD_DIR -lfaad
G_IO_MODULES := openssl
GSTREAMER_INCLUDE_FONTS := no
GSTREAMER_INCLUDE_CA_CERTIFICATES := yes
GSTREAMER_JAVA_SRC_DIR := $stage/java
GSTREAMER_ASSETS_DIR := $stage/assets
GLOBAL_LDFLAGS += -static-libstdc++
include \$(GSTREAMER_ROOT)/share/gst-android/ndk-build/gstreamer-1.0.mk
EOF

printf '#include <gst/gst.h>\nvoid livi_gst_link(void) { gst_init(NULL, NULL); }\n' > "$work/jni/link.c"

"$NDK/ndk-build" -C "$work" NDK_PROJECT_PATH="$work" -j"$(getconf _NPROCESSORS_ONLN)"

libs="$work/libs/$ABI"
[[ -f "$libs/libgstreamer_android.so" ]] || { echo "ndk-build produced no libgstreamer_android.so" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/lib/pkgconfig" "$OUT/java" "$OUT/assets"
cp -p "$libs/libgstreamer_android.so" "$OUT/lib/"
cp -R "$stage/java/." "$OUT/java/"
cp -R "$stage/assets/." "$OUT/assets/"

export PKG_CONFIG_LIBDIR="$GST_ROOT/lib/pkgconfig"
unset PKG_CONFIG_PATH
for pc in "${deps[@]}"; do
  cat > "$OUT/lib/pkgconfig/$pc.pc" <<EOF
libdir=\${pcfiledir}/..

Name: $pc
Description: $pc, linked into libgstreamer_android.so
Version: $(pkg-config --modversion "$pc")
Libs: -L\${libdir} -lgstreamer_android
Cflags:
EOF
done

GST_VERSION="$(pkg-config --modversion gstreamer-1.0)"
{
  echo "gstreamer-1.0-android-universal $GST_VERSION"
  echo "glib-2.0 $(pkg-config --modversion glib-2.0)"
  echo "faad2 ${FAAD2_VERSION:-2.11.4}"
} | sort > "$OUT/packages.txt"
printf '%s\n' "${plugins[@]}" > "$OUT/plugins.txt"
printf '%s\n' "$GST_VERSION" > "$OUT/version.txt"

rm -rf "$work"

echo "Created android-arm64 GStreamer bundle at: $OUT"
echo "Bundle size:"
du -sh "$OUT"
echo "Top-level contents:"
find "$OUT" -maxdepth 3 | sort
