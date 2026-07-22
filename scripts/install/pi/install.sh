#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Installer & Shortcut Creator (desktop session)
# ----------------------------------------
# For a host that already has a desktop session, so it adds an autostart entry,
# a desktop shortcut and an application entry. Everything it shares with the
# headless installer lives in scripts/install/common.sh.
#
# Re-runnable.

LIVI_LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../common.sh"
if [ ! -f "$LIVI_LIB" ]; then
  LIVI_LIB="$(mktemp)"
  curl -fsSL \
    "https://raw.githubusercontent.com/${LIVI_REPO:-f-io/LIVI}/${LIVI_INSTALLER_BRANCH:-main}/scripts/install/common.sh" \
    -o "$LIVI_LIB" || { echo "Error: cannot obtain common.sh" >&2; exit 1; }
fi
# shellcheck source=../common.sh
. "$LIVI_LIB"

USER_HOME="$HOME"
APPIMAGE_PATH="$USER_HOME/LIVI/LIVI.AppImage"
APPIMAGE_DIR="$(dirname "$APPIMAGE_PATH")"

echo "→ Creating target directory: $APPIMAGE_DIR"
mkdir -p "$APPIMAGE_DIR"

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

# A desktop session is already present, so only the core packages are needed here.
echo "→ Ensuring GStreamer, wireless AP and Bluetooth runtime packages"
sudo apt-get update
sudo apt-get install -y $(livi_packages core | tr '\n' ' ')

livi_install_pymobiledevice3

ICON_URL="$LIVI_RAW/assets/icons/linux/livi.png"
ICON_DEST="$USER_HOME/.local/share/icons/livi.png"

echo "→ Installing icon to $ICON_DEST"
mkdir -p "$(dirname "$ICON_DEST")"
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

# Optional positional arg: local AppImage file path or http(s) URL
APPIMAGE_SRC="${1:-}"

livi_pick_channel "$APPIMAGE_SRC"
livi_ask_mfi
livi_ask_splash

livi_fetch_appimage "$APPIMAGE_PATH" "$APPIMAGE_SRC"
echo "   Download complete: $APPIMAGE_PATH"

livi_apply_mfi
livi_apply_splash

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

echo "✅ Installation complete!"
