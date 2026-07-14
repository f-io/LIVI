#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Installer & Shortcut Creator
# ----------------------------------------

# 0) Variables
USER_HOME="$HOME"
APPIMAGE_PATH="$USER_HOME/LIVI/LIVI.AppImage"
APPIMAGE_DIR="$(dirname "$APPIMAGE_PATH")"

echo "→ Creating target directory: $APPIMAGE_DIR"
mkdir -p "$APPIMAGE_DIR"

# Ensure required tools are installed
echo "→ Checking for required tools: curl, xdg-user-dir, pkexec"
for tool in curl xdg-user-dir pkexec; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "   $tool not found, installing…"
    sudo apt-get update
    case "$tool" in
      xdg-user-dir) sudo apt-get --yes install xdg-user-dirs ;;
      pkexec)       sudo apt-get --yes install policykit-1 ;;
      *)            sudo apt-get --yes install "$tool" ;;
    esac
  else
    echo "   $tool found"
  fi
done

# Ensure the runtime deps LIVI needs: GStreamer video pipeline,
# wireless AP (hostapd/dnsmasq) and Bluetooth pairing stack
echo "→ Ensuring GStreamer, wireless AP and Bluetooth runtime packages"
sudo apt-get update
sudo apt-get install -y \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-gl \
  gstreamer1.0-libav \
  gstreamer1.0-tools \
  python3-dbus python3-gi \
  bluez hostapd dnsmasq-base iw rfkill

# ICON INSTALLATION
ICON_URL="https://raw.githubusercontent.com/f-io/LIVI/main/assets/icons/linux/livi.png"
ICON_DEST="$USER_HOME/.local/share/icons/livi.png"

echo "→ Installing icon to $ICON_DEST"
mkdir -p "$(dirname "$ICON_DEST")"

echo "   Downloading icon from $ICON_URL..."
if curl -fL "$ICON_URL" -o "$ICON_DEST"; then
  echo "   App icon downloaded and installed successfully."
  HICOLOR_ICON="$USER_HOME/.local/share/icons/hicolor/256x256/apps/livi.png"
  mkdir -p "$(dirname "$HICOLOR_ICON")"
  cp -f "$ICON_DEST" "$HICOLOR_ICON" 2>/dev/null || true
  gtk-update-icon-cache -f -t "$USER_HOME/.local/share/icons/hicolor" 2>/dev/null || true
else
  echo "   Failed to download icon from $ICON_URL. Skipping icon install."
  ICON_DEST=""
fi

# Fetch latest ARM64 AppImage from GitHub
echo "→ Fetching latest LIVI release"
latest_url=$(curl -s https://api.github.com/repos/f-io/LIVI/releases/latest \
  | grep "browser_download_url" \
  | grep "arm64.AppImage" \
  | cut -d '"' -f 4)

if [ -z "$latest_url" ]; then
  echo "Error: Could not find ARM64 AppImage URL" >&2
  exit 1
fi

echo "   Download URL: $latest_url"
if ! curl -L "$latest_url" --output "$APPIMAGE_PATH"; then
  echo "Error: Download failed" >&2
  exit 1
fi
echo "   Download complete: $APPIMAGE_PATH"

# Mark AppImage as executable
echo "→ Setting executable flag"
chmod +x "$APPIMAGE_PATH"

# Create per-user autostart entry
echo "→ Creating autostart entry"
AUTOSTART_DIR="$USER_HOME/.config/autostart"
mkdir -p "$AUTOSTART_DIR"

AUTOSTART_LOG="$APPIMAGE_DIR/LIVI.log"
cat > "$AUTOSTART_DIR/LIVI.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=LIVI
Exec=sh -c '"$APPIMAGE_PATH" >"$AUTOSTART_LOG" 2>&1'
Icon=${ICON_DEST:-livi}
Terminal=false
X-GNOME-Autostart-enabled=true
Categories=AudioVideo;
StartupWMClass=dev.f-io.livi
EOF
echo "Autostart entry at $AUTOSTART_DIR/LIVI.desktop"
echo "Autostart log at $AUTOSTART_LOG"

# Create Desktop shortcut
echo "→ Creating desktop shortcut"
if command -v xdg-user-dir >/dev/null 2>&1; then
  DESKTOP_DIR="$(xdg-user-dir DESKTOP)"
else
  DESKTOP_DIR="$USER_HOME/Desktop"
fi

mkdir -p "$DESKTOP_DIR"
cat > "$DESKTOP_DIR/LIVI.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=LIVI
Comment=Launch LIVI AppImage
Exec=$APPIMAGE_PATH
Icon=${ICON_DEST:-livi}
Terminal=false
Categories=AudioVideo;
StartupNotify=false
StartupWMClass=dev.f-io.livi
EOF

chmod +x "$DESKTOP_DIR/LIVI.desktop"
echo "Desktop shortcut at $DESKTOP_DIR/LIVI.desktop"

# Application entry so the panel/compositor can resolve the window icon from app_id.
echo "→ Creating application entry"
APPLICATIONS_DIR="$USER_HOME/.local/share/applications"
mkdir -p "$APPLICATIONS_DIR"
rm -f "$APPLICATIONS_DIR/livi.desktop"
cat > "$APPLICATIONS_DIR/dev.f-io.livi.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=LIVI
Exec=$APPIMAGE_PATH
Icon=livi
Terminal=false
Categories=AudioVideo;
StartupWMClass=dev.f-io.livi
EOF
update-desktop-database "$APPLICATIONS_DIR" 2>/dev/null || true
echo "Application entry at $APPLICATIONS_DIR/dev.f-io.livi.desktop"

# Remove the USB re-enumerate service from earlier installs, it cycles port power and disrupts mouse/touch
if [ -f /etc/systemd/system/livi-usb-rescan.service ]; then
  echo "→ Removing legacy USB re-enumerate service"
  sudo systemctl disable --now livi-usb-rescan.service 2>/dev/null || true
  sudo rm -f /etc/systemd/system/livi-usb-rescan.service /usr/local/bin/livi-usb-rescan.sh
  sudo systemctl daemon-reload
fi

# Raspberry Pi 1080p HEVC needs the v4l2codecs SAND-crop fix. GStreamer 1.26.x
# (< 1.26.11, which the Pi currently ships) detiles the 1088-padded 1080p frame into
# SystemMemory. Waylandsink rejects that and the main video layer hard-fails (black).
# Build the crop-fixed plugin from the distro source on-device. Upstream-fixed in
# 1.26.11 / 1.28.x, where this detects the version and skips.
apply_pi_hevc_crop_patch() {
  local ver here patch tmp
  ver="$(dpkg-query -W -f='${Version}' gstreamer1.0-plugins-bad 2>/dev/null \
    | grep -oE '^[0-9]+\.[0-9]+\.[0-9]+' || true)"
  if [ -z "$ver" ]; then
    echo "→ gstreamer1.0-plugins-bad not present; skipping HEVC crop patch"
    return 0
  fi
  if dpkg --compare-versions "$ver" lt 1.26.0 || dpkg --compare-versions "$ver" ge 1.26.11; then
    echo "→ GStreamer $ver is not affected by the 1080p-HEVC crop bug; skipping patch"
    return 0
  fi
  echo "→ GStreamer $ver has the 1080p-HEVC SAND-crop bug; building the patched v4l2codecs plugin"

  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  patch="$here/../../gstreamer/patch-pi-v4l2codecs.sh"
  if [ ! -f "$patch" ]; then
    tmp="$(mktemp)"
    if curl -fsSL "https://raw.githubusercontent.com/f-io/LIVI/main/scripts/gstreamer/patch-pi-v4l2codecs.sh" -o "$tmp"; then
      patch="$tmp"
    else
      echo "   could not obtain patch-pi-v4l2codecs.sh; apply it manually later" >&2
      return 0
    fi
  fi

  if bash "$patch"; then
    echo "   v4l2codecs crop patch applied"
  else
    echo "   v4l2codecs crop patch did not complete; 1080p HEVC may not display." >&2
    echo "   Re-run manually: bash scripts/gstreamer/patch-pi-v4l2codecs.sh" >&2
  fi
}

apply_pi_hevc_crop_patch

echo "✅ Installation complete!"
