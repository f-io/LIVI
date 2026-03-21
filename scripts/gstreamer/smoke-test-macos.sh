#!/usr/bin/env bash
set -euo pipefail

ROOT="${1:?bundle path required}"

export GST_PLUGIN_SYSTEM_PATH=""
export GST_PLUGIN_PATH="$ROOT/lib/gstreamer-1.0"
export GST_PLUGIN_SCANNER="$ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"
export DYLD_LIBRARY_PATH="$ROOT/lib:${DYLD_LIBRARY_PATH:-}"

"$ROOT/bin/gst-launch-1.0" --version
"$ROOT/bin/gst-launch-1.0" fakesrc num-buffers=1 ! fakesink
