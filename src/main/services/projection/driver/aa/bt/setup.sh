#!/usr/bin/env bash
# setup.sh — Install system packages for LIVI Android Auto stack
#
# This is a ONE-TIME package installer only.
# All WiFi AP + BT configuration happens at runtime in aa-bluetooth.py.
#
# Usage: sudo ./bt/setup.sh

set -euo pipefail

if [ "$EUID" -ne 0 ]; then
    echo "ERROR: run as root: sudo ./bt/setup.sh"
    exit 1
fi

echo "=== LIVI — installing packages ==="
apt-get update -qq
apt-get install -y \
    hostapd \
    dnsmasq \
    bluetooth \
    bluez \
    python3-dbus \
    python3-gi \
    gir1.2-glib-2.0 \
    rfkill \
    iproute2
echo "=== done ==="
echo ""
echo "Run the stack: sudo ./bt/start.sh"
