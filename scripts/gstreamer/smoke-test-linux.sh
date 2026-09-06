#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?bundle path required}"

export GST_PLUGIN_SYSTEM_PATH=""
export GST_PLUGIN_PATH="$ROOT/lib/gstreamer-1.0"
export LD_LIBRARY_PATH="$ROOT/lib:${LD_LIBRARY_PATH:-}"

if [[ -x "$ROOT/libexec/gstreamer-1.0/gst-plugin-scanner" ]]; then
  export GST_PLUGIN_SCANNER="$ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"
fi

LAUNCH="$ROOT/bin/gst-launch-1.0"
INSPECT="$ROOT/bin/gst-inspect-1.0"

# Reject symbols newer than the deploy target's glibc. CI passes the target's version in;
# the fallback is for local runs (never derive it from the build host, which may be newer).
GLIBC_CEILING="${GLIBC_CEILING:-2.41}"
worst=$(find "$ROOT" -type f \( -name '*.so*' -o -perm -u+x \) -exec sh -c \
  'strings "$1" 2>/dev/null | grep -oE "GLIBC_2\.[0-9]+"' _ {} \; | sort -t. -k2 -n -u | tail -1)
if [[ -n "$worst" ]]; then
  need="${worst#GLIBC_}"
  if [[ $(printf '%s\n' "$GLIBC_CEILING" "$need" | sort -t. -k2 -n | tail -1) != "$GLIBC_CEILING" ]]; then
    echo "FAIL bundle requires $worst, ceiling is GLIBC_$GLIBC_CEILING" >&2
    exit 1
  fi
  echo "ok   glibc requirement $worst <= $GLIBC_CEILING"
fi

"$LAUNCH" --version
"$LAUNCH" fakesrc num-buffers=1 ! fakesink
"$ROOT/bin/gst-device-monitor-1.0" --version

# Cross-platform video elements must load
for el in h264parse h265parse videoconvert videoscale glimagesink; do
  if "$INSPECT" "$el" >/dev/null 2>&1; then
    echo "ok   $el"
  else
    echo "FAIL $el" >&2
    exit 1
  fi
done

# HW decode + DRM sink are host-dependent
for el in v4l2h264dec v4l2h265dec v4l2slh264dec v4l2slh265dec vah264dec vah265dec kmssink waylandsink; do
  if "$INSPECT" "$el" >/dev/null 2>&1; then
    echo "hw   $el"
  else
    echo "--   $el (not present)"
  fi
done
