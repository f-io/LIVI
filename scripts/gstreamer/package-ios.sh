#!/usr/bin/env bash
set -euo pipefail

# CI only, via gstreamer-assets.yml.
# Upstream ships iOS as one static archive with every plugin. This links the plugins LIVI uses
# and the API gstreamer-rs binds out of it into one GStreamer.framework the app embeds, the
# iOS counterpart of libgstreamer_android.so. gst_init_static_plugins registers the plugins.
# PATCHED_APPLEMEDIA and PATCHED_GL_WINDOW (from build-patched-ios.sh) replace the archive's
# applemedia plugin (vtdec low latency, full-range HEVC) and EAGL GL window (layer refcount).

XCF="$(cd "${1:?GStreamer.xcframework required}" && pwd)"
OUT="${2:-assets/gstreamer/ios-arm64}"
MIN_IOS="${MIN_IOS:-15.0}"
SLICE="$XCF/ios-arm64"
SDK="$(xcrun --sdk iphoneos --show-sdk-path)"
CC=(xcrun --sdk iphoneos clang -target "arm64-apple-ios$MIN_IOS" -isysroot "$SDK")

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
  osxaudio
  opus
  rtp
  udp
  rtpmanager
  # video parse + scale + GL, VideoToolbox decoders
  videoparsersbad
  videoconvertscale
  jpeg
  videofilter
  opengl
  libav
  applemedia
  # video LIVI fetches from a URL: playbin3, hlsdemux2, souphttpsrc, qtdemux, tsdemux
  playback
  adaptivedemux2
  soup
  isomp4
  mpegtsdemux
)

# What gstreamer-rs binds. Their objects are linked whole so the framework exports the full API,
# everything else comes out of the archive only as far as the plugins need it.
api='^lib(gstreamer|gstbase|gstapp|gstaudio|gstvideo|gstcontroller|gstpbutils|gsttag)-1_0_a-|^lib(glib|gobject|gio|gmodule)-2_0_a-'
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

frameworks=(
  AVFoundation AssetsLibrary AudioToolbox CoreAudio CoreFoundation CoreGraphics CoreMedia CoreText CoreVideo
  Foundation IOSurface Metal OpenGLES QuartzCore Security SystemConfiguration UIKit VideoToolbox
)

[[ -f "$SLICE/libGStreamer.a" ]] || { echo "$SLICE/libGStreamer.a missing" >&2; exit 1; }
for patched in PATCHED_APPLEMEDIA PATCHED_GL_WINDOW; do
  [[ -f "${!patched:-}" ]] || { echo "$patched not set, run build-patched-ios.sh first" >&2; exit 1; }
done

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

members="$(ar -t "$SLICE/libGStreamer.a")"
for p in "${plugins[@]}"; do
  grep -q "^libgst${p}_a-" <<<"$members" || { echo "archive lacks plugin $p" >&2; exit 1; }
done

archive="$work/libGStreamer.a"
cp "$SLICE/libGStreamer.a" "$archive"
grep "^libgstapplemedia_a-" <<<"$members" | xargs ar -d "$archive"
gl_window="$(grep "^libgstgl-1_0_a-.*gstglwindow_eagl" <<<"$members")" || {
  echo "archive lacks the EAGL GL window" >&2
  exit 1
}
xargs ar -d "$archive" <<<"$gl_window"

mkdir -p "$work/api"
(cd "$work/api" && grep -E "$api" <<<"$members" | xargs ar -x "$SLICE/libGStreamer.a")

{
  echo '#include <gst/gst.h>'
  echo 'void g_io_openssl_load(gpointer data);'
  for p in "${plugins[@]}"; do echo "GST_PLUGIN_STATIC_DECLARE($p);"; done
  echo 'void gst_init_static_plugins(void) {'
  echo '  static gsize done = 0;'
  echo '  if (!g_once_init_enter(&done)) return;'
  echo '  g_io_openssl_load(NULL);'
  for p in "${plugins[@]}"; do echo "  GST_PLUGIN_STATIC_REGISTER($p);"; done
  echo '  g_once_init_leave(&done, 1);'
  echo '}'
} > "$work/init.c"
"${CC[@]}" -O2 -fvisibility=default -I"$SLICE/Headers" -c "$work/init.c" -o "$work/init.o"

fw="$OUT/lib/GStreamer.framework"
rm -rf "$OUT"
mkdir -p "$fw" "$OUT/lib/pkgconfig"

link=()
for f in "${frameworks[@]}"; do link+=(-framework "$f"); done
"${CC[@]}" -dynamiclib -o "$fw/GStreamer" \
  -install_name @rpath/GStreamer.framework/GStreamer \
  -Wl,-dead_strip \
  "$work/init.o" "$work"/api/*.o "$PATCHED_GL_WINDOW" "$PATCHED_APPLEMEDIA" "$archive" \
  "${link[@]}" -lc++ -liconv -lresolv -lz

version() {
  awk -v re="$2" '$1 == "#define" && $2 ~ re { gsub(/[()]/, "", $3); v[$2] = $3 }
    END { for (k in v) if (k ~ /MAJOR/) a = v[k]; else if (k ~ /MINOR/) b = v[k]; else c = v[k]
          print a "." b "." c }' "$1"
}
GST_VERSION="$(version "$SLICE/Headers/gst/gstversion.h" '^GST_VERSION_(MAJOR|MINOR|MICRO)$')"
GLIB_VERSION="$(version "$SLICE/Headers/arm64/glibconfig.h" '^GLIB_(MAJOR|MINOR|MICRO)_VERSION$')"

cat > "$fw/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>GStreamer</string>
  <key>CFBundleIdentifier</key><string>org.freedesktop.gstreamer</string>
  <key>CFBundleName</key><string>GStreamer</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>$GST_VERSION</string>
  <key>CFBundleVersion</key><string>$GST_VERSION</string>
  <key>MinimumOSVersion</key><string>$MIN_IOS</string>
  <key>CFBundleSupportedPlatforms</key><array><string>iPhoneOS</string></array>
</dict>
</plist>
EOF

for pc in "${deps[@]}"; do
  case "$pc" in
    glib-2.0|gobject-2.0|gio-2.0) ver="$GLIB_VERSION" ;;
    *) ver="$GST_VERSION" ;;
  esac
  cat > "$OUT/lib/pkgconfig/$pc.pc" <<EOF
libdir=\${pcfiledir}/..

Name: $pc
Description: $pc, linked into GStreamer.framework
Version: $ver
Libs: -F\${libdir} -framework GStreamer
Cflags:
EOF
done

{
  echo "gstreamer-xcframework $GST_VERSION"
  echo "glib-2.0 $GLIB_VERSION"
} | sort > "$OUT/packages.txt"
printf '%s\n' "${plugins[@]}" > "$OUT/plugins.txt"
printf '%s\n' "$GST_VERSION" > "$OUT/version.txt"

echo "Created ios-arm64 GStreamer bundle at: $OUT"
echo "Bundle size:"
du -sh "$OUT"
echo "Top-level contents:"
find "$OUT" -maxdepth 3 | sort
