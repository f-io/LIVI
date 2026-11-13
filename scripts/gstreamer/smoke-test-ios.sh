#!/usr/bin/env bash
set -euo pipefail

# The bundle cannot run on the build host, so this checks the linked framework instead: an iOS
# binary, only system libraries as dependencies, every plugin registered, the bound API exported.

ROOT="${1:?bundle path required}"
LIB="$ROOT/lib/GStreamer.framework/GStreamer"

[[ -f "$LIB" ]] || { echo "FAIL $LIB missing" >&2; exit 1; }

platform="$(otool -l "$LIB" | awk '/LC_BUILD_VERSION/ { f = 1 } f && $1 == "platform" { print $2; exit }')"
[[ "$platform" == 2 ]] || { echo "FAIL built for platform $platform, not iOS" >&2; exit 1; }
echo "ok   iOS arm64"

while read -r lib; do
  case "$lib" in
    @rpath/GStreamer.framework/GStreamer|/usr/lib/*|/System/Library/Frameworks/*) echo "ok   needs $lib" ;;
    *) echo "FAIL needs $lib, not part of the system" >&2; exit 1 ;;
  esac
done < <(otool -L "$LIB" | tail -n +2 | awk '{ print $1 }')

syms="$(mktemp)"
trap 'rm -f "$syms"' EXIT
nm -gU "$LIB" | awk '{ print $NF }' | sed 's/^_//' | sort -u > "$syms"

check() {
  if grep -qx "$1" "$syms"; then
    echo "ok   $1"
  else
    echo "FAIL $1" >&2
    exit 1
  fi
}

while read -r plugin; do
  check "gst_plugin_${plugin}_register"
done < "$ROOT/plugins.txt"

for sym in gst_init gst_init_static_plugins g_io_openssl_load \
  gst_app_src_push_buffer gst_app_sink_pull_sample gst_base_src_get_type gst_audio_info_from_caps \
  gst_video_info_from_caps gst_interpolation_control_source_new gst_pb_utils_init gst_tag_list_new_empty \
  g_unix_fd_add_full g_socket_new; do
  check "$sym"
done

for f in lib/GStreamer.framework/Info.plist lib/pkgconfig/gstreamer-1.0.pc; do
  [[ -f "$ROOT/$f" ]] || { echo "FAIL $f missing" >&2; exit 1; }
  echo "ok   $f"
done
