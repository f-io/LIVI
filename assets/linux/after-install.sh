#!/bin/bash
PROFILE_DIR="/etc/apparmor.d"
PROFILE_FILE="$PROFILE_DIR/livi"
PROFILE_SOURCE="/opt/LIVI/resources/livi.apparmor"

if [ -f "$PROFILE_SOURCE" ]; then
  mkdir -p "$PROFILE_DIR"
  cp "$PROFILE_SOURCE" "$PROFILE_FILE"
  if command -v apparmor_parser &>/dev/null; then
    apparmor_parser -r "$PROFILE_FILE" 2>/dev/null || true
  fi
fi

chown root:root /opt/LIVI/chrome-sandbox
chmod 4755 /opt/LIVI/chrome-sandbox
