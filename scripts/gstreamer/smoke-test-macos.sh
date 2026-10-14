#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?bundle path required}"

export GST_PLUGIN_SYSTEM_PATH=""
export GST_PLUGIN_PATH="$ROOT/lib/gstreamer-1.0"
export GST_PLUGIN_SCANNER="$ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"
export DYLD_LIBRARY_PATH="$ROOT/lib:${DYLD_LIBRARY_PATH:-}"

LAUNCH="$ROOT/bin/gst-launch-1.0"
INSPECT="$ROOT/bin/gst-inspect-1.0"

"$LAUNCH" --version
"$LAUNCH" fakesrc num-buffers=1 ! fakesink
"$ROOT/bin/gst-device-monitor-1.0" --version

# Video elements must load self-contained
for el in h264parse h265parse vtdec vtdec_hw videoconvert videoscale glimagesink osxvideosink \
  playbin3 hlsdemux2 souphttpsrc qtdemux; do
  if "$INSPECT" "$el" >/dev/null 2>&1; then
    echo "ok   $el"
  else
    echo "FAIL $el" >&2
    exit 1
  fi
done
