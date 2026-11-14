#!/usr/bin/env bash
set -euo pipefail

# Builds the applemedia plugin (vtdec) statically for arm64 iOS from the gst-plugins-bad source
# matching the GStreamer.xcframework, with patches/gst-plugins-bad/series-macos applied, and the
# EAGL GL window from gst-plugins-base with patches/gst-plugins-base/series-ios applied.
# Leaves libgstapplemedia.a and gstglwindow_eagl.o in OUT_DIR for package-ios.sh.
#   build-patched-ios.sh GStreamer.xcframework [OUT_DIR]

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PATCHES="$REPO/scripts/gstreamer/patches"
XCF="$(cd "${1:?GStreamer.xcframework required}" && pwd)"
OUT="${2:-$(mktemp -d)}"
MIN_IOS="${MIN_IOS:-15.0}"
SLICE="$XCF/ios-arm64"
HEADERS="$SLICE/Headers"
SDK="$(xcrun --sdk iphoneos --show-sdk-path)"

for t in glib-mkenums glib-genmarshal gobject-query meson ninja; do
  command -v "$t" >/dev/null || { echo "$t not on PATH (brew install glib meson ninja)" >&2; exit 1; }
done

version() {
  awk -v re="$2" '$1 == "#define" && $2 ~ re { gsub(/[()]/, "", $3); v[$2] = $3 }
    END { for (k in v) if (k ~ /MAJOR/) a = v[k]; else if (k ~ /MINOR/) b = v[k]; else c = v[k]
          print a "." b "." c }' "$1"
}
GST_VERSION="$(version "$HEADERS/gst/gstversion.h" '^GST_VERSION_(MAJOR|MINOR|MICRO)$')"
GLIB_VERSION="$(version "$HEADERS/arm64/glibconfig.h" '^GLIB_(MAJOR|MINOR|MICRO)_VERSION$')"
SRC_URL="https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${GST_VERSION}.tar.xz"
BASE_URL="https://gstreamer.freedesktop.org/src/gst-plugins-base/gst-plugins-base-${GST_VERSION}.tar.xz"

echo "Building patched applemedia for iOS GStreamer $GST_VERSION"

# The xcframework carries headers but no .pc files. The plugin is only compiled here and linked
# into the framework by package-ios.sh, so the .pc files only hand meson the include paths.
PC="$(mktemp -d)"
for pc in gstreamer-1.0 gstreamer-base-1.0 gstreamer-net-1.0 gstreamer-controller-1.0 \
  gstreamer-pbutils-1.0 gstreamer-allocators-1.0 gstreamer-app-1.0 gstreamer-audio-1.0 \
  gstreamer-fft-1.0 gstreamer-riff-1.0 gstreamer-rtp-1.0 gstreamer-rtsp-1.0 gstreamer-sdp-1.0 \
  gstreamer-tag-1.0 gstreamer-video-1.0 gstreamer-gl-1.0 gstreamer-gl-prototypes-1.0; do
  {
    echo "Name: $pc"
    echo "Description: $pc from the xcframework"
    echo "Version: $GST_VERSION"
    echo "Cflags: -I$HEADERS -I$HEADERS/arm64"
    if [ "$pc" = gstreamer-gl-1.0 ]; then
      echo "gl_apis=gles2"
      echo "gl_winsys=eagl"
      echo "gl_platforms=eagl"
    fi
  } > "$PC/$pc.pc"
done
for pc in glib-2.0 gobject-2.0 gio-2.0 gmodule-2.0 gmodule-no-export-2.0; do
  {
    echo "glib_genmarshal=$(command -v glib-genmarshal)"
    echo "gobject_query=$(command -v gobject-query)"
    echo "glib_mkenums=$(command -v glib-mkenums)"
    echo "Name: $pc"
    echo "Description: $pc from the xcframework"
    echo "Version: $GLIB_VERSION"
    echo "Cflags: -I$HEADERS -I$HEADERS/arm64"
  } > "$PC/$pc.pc"
done

apply_series() {
  local dir="$PATCHES/$1"
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    patch -p1 -i "$dir/$p"
  done < "$dir/$2"
}

fetch() {
  curl -fsSL -o "$2" "$1"
  echo "$(curl -fsSL "$1.sha256sum" | cut -d' ' -f1)  $2" | shasum -a 256 -c -
}

work="$(mktemp -d)"
(
  cd "$work"
  fetch "$SRC_URL" src.tar.xz
  tar -xf src.tar.xz
  cd "gst-plugins-bad-${GST_VERSION}"
  apply_series gst-plugins-bad series-macos
  grep -q "Using HEVC SPS max_num_reorder_pics" sys/applemedia/vtdec.c || {
    echo "0008 did not apply: SPS reorder parsing absent in vtdec.c" >&2
    exit 1
  }
  grep -q "420YpCbCr8BiPlanarFullRange" sys/applemedia/vtdec.c || {
    echo "0009 did not apply: full-range output absent in vtdec.c" >&2
    exit 1
  }
  target=(-target "arm64-apple-ios$MIN_IOS" -isysroot "$SDK")
  args="$(printf "'%s', " "${target[@]}")"
  cat > cross-ios.ini <<EOF
[binaries]
c = ['xcrun', '--sdk', 'iphoneos', 'clang']
cpp = ['xcrun', '--sdk', 'iphoneos', 'clang++']
objc = ['xcrun', '--sdk', 'iphoneos', 'clang']
objcpp = ['xcrun', '--sdk', 'iphoneos', 'clang++']
ar = ['xcrun', '--sdk', 'iphoneos', 'ar']
strip = ['xcrun', '--sdk', 'iphoneos', 'strip']
pkg-config = 'pkg-config'

[built-in options]
c_args = [${args%, }]
cpp_args = [${args%, }]
objc_args = [${args%, }]
objcpp_args = [${args%, }]
c_link_args = [${args%, }]
cpp_link_args = [${args%, }]
objc_link_args = [${args%, }]
objcpp_link_args = [${args%, }]

[properties]
needs_exe_wrapper = true

[host_machine]
system = 'darwin'
subsystem = 'ios'
kernel = 'xnu'
cpu_family = 'aarch64'
cpu = 'arm64'
endian = 'little'
EOF
  PKG_CONFIG_LIBDIR="$PC" PKG_CONFIG_PATH="" meson setup _build --cross-file "$PWD/cross-ios.ini" \
    -Ddefault_library=static \
    -Dauto_features=disabled \
    -Dapplemedia=enabled \
    -Dgl=enabled \
    -Dtests=disabled \
    -Dexamples=disabled \
    -Ddoc=disabled
  meson compile -C _build gstapplemedia
  mkdir -p "$OUT"
  cp "$(find _build -name libgstapplemedia.a | head -1)" "$OUT/libgstapplemedia.a"
)
PLUGIN="$OUT/libgstapplemedia.a"

# The patch is compiled in and the archive registers the plugin statically. Read the whole
# output before matching: grep -q closing the pipe early trips pipefail on the producer.
strings -a "$PLUGIN" | grep "Using HEVC SPS max_num_reorder_pics" >/dev/null || {
  echo "built plugin lacks the SPS reorder parsing" >&2
  exit 1
}
nm -gU "$PLUGIN" 2>/dev/null | grep "_gst_plugin_applemedia_register" >/dev/null || {
  echo "built plugin does not register applemedia statically" >&2
  exit 1
}

echo "Patched applemedia: $PLUGIN"

echo "Building patched EAGL GL window for iOS GStreamer $GST_VERSION"
(
  cd "$work"
  fetch "$BASE_URL" base.tar.xz
  tar -xf base.tar.xz "gst-plugins-base-${GST_VERSION}/gst-libs"
  cd "gst-plugins-base-${GST_VERSION}"
  apply_series gst-plugins-base series-ios
  grep -q "update_layer() releases the layer" gst-libs/gst/gl/eagl/gstglwindow_eagl.m || {
    echo "0006 did not apply: layer retain absent in gstglwindow_eagl.m" >&2
    exit 1
  }
  # The flags meson gives libgstgl's Objective-C sources.
  xcrun --sdk iphoneos clang -target "arm64-apple-ios$MIN_IOS" -isysroot "$SDK" \
    -O2 -fobjc-arc -fvisibility=hidden \
    -DBUILDING_GST_GL -DGST_USE_UNSTABLE_API '-DG_LOG_DOMAIN="GStreamer-GL"' \
    -I"$HEADERS" -I"$HEADERS/arm64" -Igst-libs -Igst-libs/gst/gl/eagl \
    -c gst-libs/gst/gl/eagl/gstglwindow_eagl.m -o "$OUT/gstglwindow_eagl.o"
)
echo "Patched EAGL window: $OUT/gstglwindow_eagl.o"
