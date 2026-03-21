#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GST_ROOT="$ROOT/assets/gstreamer/macos-arm64"

export DYLD_LIBRARY_PATH="$GST_ROOT/lib"
export GST_PLUGIN_SYSTEM_PATH_1_0="$GST_ROOT/lib/gstreamer-1.0"
export GST_PLUGIN_SCANNER="$GST_ROOT/libexec/gstreamer-1.0/gst-plugin-scanner"

"$GST_ROOT/bin/gst-launch-1.0" --version

TMP_RAW="/tmp/test-gstreamer.raw"

if ! command -v sox >/dev/null 2>&1; then
  echo "sox is required for the PCM test. Install it with: brew install sox"
  exit 1
fi

sox -n -r 16000 -c 1 -b 16 "$TMP_RAW" synth 1 sine 440

"$GST_ROOT/bin/gst-launch-1.0" \
  fdsrc fd=0 ! \
  rawaudioparse format=pcm pcm-format=s16le sample-rate=16000 num-channels=1 ! \
  audioconvert ! \
  audioresample ! \
  autoaudiosink \
  < "$TMP_RAW"
