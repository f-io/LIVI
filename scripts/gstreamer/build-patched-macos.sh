#!/usr/bin/env bash
set -euo pipefail

# Builds the applemedia plugin (vtdec) from the gst-plugins-bad source matching the
# installed GStreamer.framework, with patches/gst-plugins-bad/series-macos applied.
# Prints the path of the patched libgstapplemedia.dylib for package-macos.sh.
#   build-patched-macos.sh [OUT_DIR]      (GST_VERSION and GST_ROOT overridable via env)

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PATCHES="$REPO/scripts/gstreamer/patches/gst-plugins-bad"
GST_ROOT="${GST_ROOT:-/Library/Frameworks/GStreamer.framework/Versions/1.0}"
OUT="${1:-$(mktemp -d)}"

export PKG_CONFIG_PATH="$GST_ROOT/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
GST_VERSION="${GST_VERSION:-$(pkg-config --modversion gstreamer-1.0)}"
SRC_URL="https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${GST_VERSION}.tar.xz"

# The framework's glib-2.0.pc points its generator tools at bin/ scripts that do not run
# here (gobject-query missing, glib-mkenums shebang dead). Take the three from Homebrew glib
# in an override .pc; prefix stays the framework so linking is unchanged.
for t in glib-mkenums glib-genmarshal gobject-query; do
  command -v "$t" >/dev/null || { echo "$t not on PATH (brew install glib)" >&2; exit 1; }
done
PC_OVERRIDE="$(mktemp -d)"
sed \
  -e "s|^prefix=.*|prefix=$GST_ROOT|" \
  -e "s|^glib_genmarshal=.*|glib_genmarshal=$(command -v glib-genmarshal)|" \
  -e "s|^gobject_query=.*|gobject_query=$(command -v gobject-query)|" \
  -e "s|^glib_mkenums=.*|glib_mkenums=$(command -v glib-mkenums)|" \
  "$GST_ROOT/lib/pkgconfig/glib-2.0.pc" > "$PC_OVERRIDE/glib-2.0.pc"
export PKG_CONFIG_PATH="$PC_OVERRIDE:$PKG_CONFIG_PATH"

echo "Building patched applemedia for GStreamer $GST_VERSION"

apply_series() {
  local series="$1"
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    patch -p1 -i "$PATCHES/$p"
  done < "$series"
}

work="$(mktemp -d)"
(
  cd "$work"
  curl -fsSL -o src.tar.xz "$SRC_URL"
  tar -xf src.tar.xz
  cd "gst-plugins-bad-${GST_VERSION}"
  apply_series "$PATCHES/series-macos"
  grep -q "parsed HEVC SPS max_num_reorder_pics" sys/applemedia/vtdec.c || {
    echo "0007 did not apply: SPS reorder parsing absent in vtdec.c" >&2
    exit 1
  }
  grep -q "420YpCbCr8BiPlanarFullRange" sys/applemedia/vtdec.c || {
    echo "0008 did not apply: full-range output absent in vtdec.c" >&2
    exit 1
  }
  # gl on: applemedia includes gst/gl unconditionally and vtdec outputs GL textures, and the
  # generated gstglconfig.h only comes via the gstreamer-gl-1.0 include path.
  meson setup _build \
    -Dauto_features=disabled \
    -Dapplemedia=enabled \
    -Dgl=enabled \
    -Dtests=disabled \
    -Dexamples=disabled \
    -Ddoc=disabled
  meson compile -C _build
  mkdir -p "$OUT"
  cp "$(find _build -name libgstapplemedia.dylib | head -1)" "$OUT/libgstapplemedia.dylib"
)
PLUGIN="$OUT/libgstapplemedia.dylib"

# Drop the build-tree and absolute framework rpaths: the shipped plugin must resolve inside
# the bundle only, like the prebuilt ones. package-macos.sh adds the bundle rpath.
while read -r rp; do
  case "$rp" in /*|*gst-libs*) install_name_tool -delete_rpath "$rp" "$PLUGIN" ;; esac
done < <(otool -l "$PLUGIN" | awk '/LC_RPATH/{getline;getline;print $2}')
codesign --force --sign - "$PLUGIN" >/dev/null 2>&1 || true

# The patch is compiled in and the plugin loads and registers vtdec_hw. Read the whole
# output before matching: grep -q closing the pipe early trips pipefail on the producer.
strings -a "$PLUGIN" | grep "parsed HEVC SPS max_num_reorder_pics" >/dev/null || {
  echo "built plugin lacks the SPS reorder parsing" >&2
  exit 1
}
inspect="$("$GST_ROOT/bin/gst-inspect-1.0" "$PLUGIN" 2>&1)"
grep "vtdec_hw" <<<"$inspect" >/dev/null || {
  echo "built plugin does not load or register vtdec_hw:" >&2
  echo "$inspect" >&2
  exit 1
}

echo "Patched applemedia: $PLUGIN"
