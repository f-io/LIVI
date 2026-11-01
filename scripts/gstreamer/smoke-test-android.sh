#!/usr/bin/env bash
set -euo pipefail

# The bundle cannot run on the build host, so this checks the linked library instead: only
# platform libraries as dependencies, every plugin registered, the bound API exported.

ROOT="${1:?bundle path required}"
NDK="${ANDROID_NDK_HOME:?ANDROID_NDK_HOME not set}"
TOOLCHAIN="$(echo "$NDK"/toolchains/llvm/prebuilt/*)"
LIB="$ROOT/lib/libgstreamer_android.so"

[[ -f "$LIB" ]] || { echo "FAIL $LIB missing" >&2; exit 1; }

needed="$("$TOOLCHAIN/bin/llvm-readelf" -d "$LIB" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')"
for lib in $needed; do
  case "$lib" in
    libc.so|libm.so|libdl.so|liblog.so|libandroid.so|libEGL.so|libGLESv2.so|libOpenSLES.so)
      echo "ok   needs $lib" ;;
    *) echo "FAIL needs $lib, not part of the platform" >&2; exit 1 ;;
  esac
done

syms="$(mktemp)"
trap 'rm -f "$syms"' EXIT
"$TOOLCHAIN/bin/llvm-nm" -D --defined-only "$LIB" | awk '{ print $NF }' | sort -u > "$syms"

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

for sym in JNI_OnLoad gst_init gst_init_static_plugins gst_android_get_java_vm g_io_openssl_load \
  gst_app_src_push_buffer gst_app_sink_pull_sample gst_base_src_get_type gst_audio_info_from_caps \
  gst_video_info_from_caps gst_interpolation_control_source_new g_unix_fd_add_full; do
  check "$sym"
done

for f in assets/ssl/certs/ca-certificates.crt java/org/freedesktop/gstreamer/GStreamer.java \
  lib/pkgconfig/gstreamer-1.0.pc; do
  [[ -f "$ROOT/$f" ]] || { echo "FAIL $f missing" >&2; exit 1; }
  echo "ok   $f"
done
