#!/usr/bin/env bash
set -euo pipefail

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

rm -rf "$OUT"
mkdir -p \
  "$OUT/bin" \
  "$OUT/lib" \
  "$OUT/lib/gstreamer-1.0" \
  "$OUT/libexec/gstreamer-1.0"

# bin
copy_required "$GST_ROOT/bin/gst-launch-1.0" "$OUT/bin/"

# libexec
copy_required \
  "$GST_ROOT/libexec/gstreamer-1.0/gst-plugin-scanner" \
  "$OUT/libexec/gstreamer-1.0/"

# libraries
libs=(
  GStreamer
  libffi.7.dylib
  libglib-2.0.0.dylib
  libgmodule-2.0.0.dylib
  libgobject-2.0.0.dylib
  libgstapp-1.0.0.dylib
  libgstaudio-1.0.0.dylib
  libgstbase-1.0.0.dylib
  libgstfft-1.0.0.dylib
  libgstpbutils-1.0.0.dylib
  libgstreamer-1.0.0.dylib
  libgsttag-1.0.0.dylib
  libgstvideo-1.0.0.dylib
  libintl.8.dylib
  liborc-0.4.0.dylib
  libpcre2-8.0.dylib
  libz.1.dylib
)

for lib in "${libs[@]}"; do
  copy_required "$GST_ROOT/lib/$lib" "$OUT/lib/"
done

# plugins
plugins=(
  libgstapp.dylib
  libgstaudioconvert.dylib
  libgstaudiofx.dylib
  libgstaudiomixer.dylib
  libgstaudioparsers.dylib
  libgstaudiorate.dylib
  libgstaudioresample.dylib
  libgstaudiotestsrc.dylib
  libgstautodetect.dylib
  libgstcoreelements.dylib
  libgstequalizer.dylib
  libgstinterleave.dylib
  libgstlevel.dylib
  libgstosxaudio.dylib
  libgstrawparse.dylib
  libgstvolume.dylib
)

for plugin in "${plugins[@]}"; do
  copy_required "$GST_ROOT/lib/gstreamer-1.0/$plugin" "$OUT/lib/gstreamer-1.0/"
done

echo "Created macOS GStreamer bundle at: $OUT"
