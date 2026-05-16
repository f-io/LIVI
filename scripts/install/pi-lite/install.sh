#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Lite Installer (Raspberry Pi OS Lite)
# ----------------------------------------
# Kiosk-style install for headless Pi OS Lite.
#
#   - Installs Cage (Wayland kiosk compositor), seatd, PipeWire
#   - Downloads the latest LIVI AppImage
#   - Extracts the bundled udev rule template from the AppImage and writes
#     /etc/udev/rules.d/99-LIVI.rules with the matching version marker so the
#     in-app pkexec dialog stays silent on first launch
#   - Configures tty1 autologin and a Cage autostart in ~/.bash_profile
#
# Re-runnable. Refuses to run as root (sudo is used internally).

if [[ $EUID -eq 0 ]]; then
  echo "Run as a regular user. sudo is used internally where needed." >&2
  exit 1
fi

USER_HOME="$HOME"
APPIMAGE_PATH="$USER_HOME/LIVI/LIVI.AppImage"
APPIMAGE_DIR="$(dirname "$APPIMAGE_PATH")"
RULE_FILE="/etc/udev/rules.d/99-LIVI.rules"
TEMPLATE_NAME="99-LIVI.rules.template"

echo "→ Installing required packages"
sudo apt-get update
sudo apt-get install -y \
  curl \
  xdg-user-dirs \
  cage \
  seatd \
  pipewire wireplumber pipewire-pulse

echo "→ Adding $USER to required groups"
WANTED_GROUPS=(video render input plugdev)
EXISTING_GROUPS=()
for g in "${WANTED_GROUPS[@]}"; do
  if getent group "$g" >/dev/null; then
    EXISTING_GROUPS+=("$g")
  else
    echo "   skipping group '$g' (not present on this system)"
  fi
done
if [ ${#EXISTING_GROUPS[@]} -gt 0 ]; then
  sudo usermod -aG "$(IFS=,; echo "${EXISTING_GROUPS[*]}")" "$USER"
fi

echo "→ Creating target directory: $APPIMAGE_DIR"
mkdir -p "$APPIMAGE_DIR"

# Optional positional arg: local AppImage file path or http(s) URL
APPIMAGE_SRC="${1:-}"

if [ -n "$APPIMAGE_SRC" ]; then
  if [[ "$APPIMAGE_SRC" =~ ^https?:// ]]; then
    echo "→ Downloading AppImage from $APPIMAGE_SRC"
    curl -L "$APPIMAGE_SRC" --output "$APPIMAGE_PATH"
  elif [ -f "$APPIMAGE_SRC" ]; then
    echo "→ Using local AppImage at $APPIMAGE_SRC"
    cp "$APPIMAGE_SRC" "$APPIMAGE_PATH"
  else
    echo "Error: AppImage source not found: $APPIMAGE_SRC" >&2
    exit 1
  fi
else
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
fi

chmod +x "$APPIMAGE_PATH"

# Extract the udev template from inside the AppImage so the rule version
# we install always matches what the app on this disk expects.
echo "→ Extracting udev template from the AppImage"
EXTRACT_DIR="$(mktemp -d)"
trap "rm -rf '$EXTRACT_DIR'" EXIT
pushd "$EXTRACT_DIR" >/dev/null
"$APPIMAGE_PATH" --appimage-extract "resources/$TEMPLATE_NAME" >/dev/null 2>&1 || true
TEMPLATE_PATH="$EXTRACT_DIR/squashfs-root/resources/$TEMPLATE_NAME"
popd >/dev/null

if [ ! -f "$TEMPLATE_PATH" ]; then
  BRANCH="${LIVI_TEMPLATE_BRANCH:-main}"
  TEMPLATE_URL="https://raw.githubusercontent.com/f-io/LIVI/${BRANCH}/assets/linux/${TEMPLATE_NAME}"
  echo "→ Template not in AppImage (likely older release). Falling back to ${TEMPLATE_URL}"
  TEMPLATE_PATH="$EXTRACT_DIR/${TEMPLATE_NAME}"
  if ! curl -fL "$TEMPLATE_URL" -o "$TEMPLATE_PATH"; then
    echo "Error: udev template not in AppImage and download from $TEMPLATE_URL failed" >&2
    exit 1
  fi
  echo "   Note: LIVI may prompt for a udev rule upgrade on first launch if the AppImage version differs."
fi

echo "→ Writing $RULE_FILE"
sed "s/__USERNAME__/$USER/g" "$TEMPLATE_PATH" | sudo tee "$RULE_FILE" >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger

echo "→ Enabling seatd"
sudo systemctl enable --now seatd

echo "→ Enabling lingering for PipeWire user services"
sudo loginctl enable-linger "$USER"

echo "→ Configuring tty1 autologin"
sudo raspi-config nonint do_boot_behaviour B2

KIOSK_MARKER="# LIVI-KIOSK-AUTOSTART"
if ! grep -q "$KIOSK_MARKER" "$USER_HOME/.bash_profile" 2>/dev/null; then
  echo "→ Wiring Cage kiosk autostart into ~/.bash_profile"
  cat >> "$USER_HOME/.bash_profile" <<EOF

$KIOSK_MARKER
if [ -z "\$WAYLAND_DISPLAY" ] && [ "\$(tty)" = "/dev/tty1" ]; then
  export ELECTRON_OZONE_PLATFORM_HINT=wayland
  exec cage -- "$APPIMAGE_PATH"
fi
EOF
else
  echo "→ Kiosk autostart already present in ~/.bash_profile, leaving as is"
fi

echo ""
echo "✅ LIVI Lite installation complete."
echo ""
echo "Reboot to launch LIVI in kiosk mode on tty1:"
echo "    sudo reboot"
echo ""
echo "To exit kiosk for debugging, switch to a different VT (Ctrl+Alt+F2)."
echo "To disable kiosk autostart, remove the '$KIOSK_MARKER' block from ~/.bash_profile."
