#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Headless Installer (any systemd + apt host)
# ----------------------------------------
# Kiosk-style install for a host with no desktop session. Works on Raspberry Pi
# OS Lite and on x86 Debian alike. Everything it shares with the desktop
# installer lives in scripts/install/common.sh.
#
#   - Installs Cage (Wayland kiosk compositor), seatd, PipeWire
#   - Writes the udev rule and the sudoers drop-in from the templates inside the
#     AppImage, so first launch needs no pkexec dialog
#   - Configures tty1 autologin through a systemd getty drop-in and a Cage
#     autostart in ~/.bash_profile
#
# Re-runnable. Refuses to run as root (sudo is used internally).

LIVI_LIB="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/../common.sh"
if [ ! -f "$LIVI_LIB" ]; then
  LIVI_LIB="$(mktemp)"
  curl -fsSL \
    "https://raw.githubusercontent.com/${LIVI_REPO:-f-io/LIVI}/${LIVI_INSTALLER_BRANCH:-main}/scripts/install/common.sh" \
    -o "$LIVI_LIB" || { echo "Error: cannot obtain common.sh" >&2; exit 1; }
fi
# shellcheck source=../common.sh
. "$LIVI_LIB"

livi_require_regular_user

if ! command -v apt-get >/dev/null; then
  echo "Error: this installer needs apt. Install the packages from" >&2
  echo "scripts/install/packages.txt by hand, then run the remaining steps." >&2
  exit 1
fi

USER_HOME="$HOME"
APPIMAGE_PATH="$USER_HOME/LIVI/LIVI.AppImage"
APPIMAGE_DIR="$(dirname "$APPIMAGE_PATH")"
GETTY_DROPIN="/etc/systemd/system/getty@tty1.service.d/livi-autologin.conf"

echo "→ Architecture: $(uname -m) → $(livi_asset_arch).AppImage"

echo "→ Installing required packages"
sudo apt-get update
sudo apt-get install -y $(livi_packages core lite | tr '\n' ' ')

livi_install_pymobiledevice3

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

# Optional positional arg: local AppImage file path or http(s) URL
APPIMAGE_SRC="${1:-}"

livi_pick_channel "$APPIMAGE_SRC"
livi_ask_mfi
livi_ask_splash

livi_fetch_appimage "$APPIMAGE_PATH" "$APPIMAGE_SRC"

LIVI_EXTRACT_DIR="$(mktemp -d)"
trap "rm -rf '$LIVI_EXTRACT_DIR'" EXIT

echo "→ Extracting rule templates from the AppImage"
UDEV_TEMPLATE="$(livi_fetch_template "$APPIMAGE_PATH" "$LIVI_UDEV_TEMPLATE")" || {
  echo "Error: cannot obtain $LIVI_UDEV_TEMPLATE" >&2
  exit 1
}
SUDOERS_TEMPLATE="$(livi_fetch_template "$APPIMAGE_PATH" "$LIVI_SUDOERS_TEMPLATE")" || {
  echo "Error: cannot obtain $LIVI_SUDOERS_TEMPLATE" >&2
  exit 1
}

livi_write_udev_rule "$UDEV_TEMPLATE"
livi_write_sudoers "$SUDOERS_TEMPLATE"
livi_apply_mfi
livi_apply_splash

echo "→ Enabling seatd"
sudo systemctl enable --now seatd

echo "→ Enabling lingering for PipeWire user services"
sudo loginctl enable-linger "$USER"

echo "→ Configuring tty1 autologin"
AGETTY_BIN="$(command -v agetty || echo /sbin/agetty)"
sudo mkdir -p "$(dirname "$GETTY_DROPIN")"
sudo tee "$GETTY_DROPIN" >/dev/null <<EOF
[Service]
ExecStart=
ExecStart=-$AGETTY_BIN --autologin $USER --noclear %I \$TERM
EOF
sudo systemctl daemon-reload

PREVIOUS_TARGET="$(systemctl get-default)"
if [ "$PREVIOUS_TARGET" != "multi-user.target" ]; then
  echo "   boot target $PREVIOUS_TARGET → multi-user.target (kiosk owns the screen)"
  sudo systemctl set-default multi-user.target
fi

KIOSK_MARKER="# LIVI-KIOSK-AUTOSTART"
if ! grep -q "$KIOSK_MARKER" "$USER_HOME/.bash_profile" 2>/dev/null; then
  echo "→ Wiring Cage kiosk autostart into ~/.bash_profile"
  cat >> "$USER_HOME/.bash_profile" <<EOF

$KIOSK_MARKER
if [ -z "\$WAYLAND_DISPLAY" ] && [ "\$(tty)" = "/dev/tty1" ]; then
  export ELECTRON_OZONE_PLATFORM_HINT=wayland
  export LIVI_KIOSK=1
  exec cage -- "$APPIMAGE_PATH" >"$APPIMAGE_DIR/LIVI.log" 2>&1
fi
EOF
else
  echo "→ Kiosk autostart already present in ~/.bash_profile, leaving as is"
fi

echo ""
echo "✅ LIVI headless installation complete."
echo ""
echo "Reboot to launch LIVI in kiosk mode on tty1:"
echo "    sudo reboot"
echo ""
echo "To exit kiosk for debugging, switch to a different VT (Ctrl+Alt+F2)."
echo "To disable kiosk autostart, remove the '$KIOSK_MARKER' block from ~/.bash_profile."
echo "To disable autologin, remove $GETTY_DROPIN and run 'sudo systemctl daemon-reload'."
echo "To get a graphical login back, run 'sudo systemctl set-default graphical.target'."
