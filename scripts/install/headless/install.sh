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
#   - Configures tty1 autologin through a systemd getty drop-in and starts Cage
#     from the livi-kiosk service
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
KIOSK_UNIT="/etc/systemd/system/livi-kiosk.service"
KIOSK_PAM="/etc/pam.d/livi-kiosk"

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
livi_ask_hdmi_pr

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

TOUCH_FILTER="$(livi_fetch_template "$APPIMAGE_PATH" "$LIVI_TOUCH_FILTER_TEMPLATE")" || {
  echo "Error: cannot obtain $LIVI_TOUCH_FILTER_TEMPLATE" >&2
  exit 1
}

livi_install_touch_filter "$TOUCH_FILTER"
livi_write_udev_rule "$UDEV_TEMPLATE"
livi_write_sudoers "$SUDOERS_TEMPLATE"
livi_disable_wifi_powersave
livi_apply_mfi
livi_apply_splash
livi_apply_hdmi_pr "$APPIMAGE_PATH"

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
if grep -q "$KIOSK_MARKER" "$USER_HOME/.bash_profile" 2>/dev/null; then
  echo "→ Removing the old kiosk autostart from ~/.bash_profile"
  cp -p "$USER_HOME/.bash_profile" "$USER_HOME/.bash_profile.livi-bak"
  awk -v marker="$KIOSK_MARKER" '
    $0 == marker { skip = 1; next }
    skip && /^fi$/ { skip = 0; next }
    !skip { print }
  ' "$USER_HOME/.bash_profile.livi-bak" > "$USER_HOME/.bash_profile"
fi

echo "→ Writing $KIOSK_PAM"
sudo tee "$KIOSK_PAM" >/dev/null <<'EOF'
auth      required  pam_permit.so
@include  common-account
@include  common-session
EOF

echo "→ Writing $KIOSK_UNIT"
CAGE_BIN="$(command -v cage || echo /usr/bin/cage)"
SYSTEMCTL_BIN="$(command -v systemctl || echo /usr/bin/systemctl)"
sudo tee "$KIOSK_UNIT" >/dev/null <<EOF
[Unit]
Description=LIVI kiosk
After=systemd-user-sessions.service seatd.service getty@tty1.service
Conflicts=getty@tty1.service

[Service]
Type=simple
User=$USER
PAMName=livi-kiosk
WorkingDirectory=$USER_HOME
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
StandardInput=tty-fail
StandardOutput=append:$APPIMAGE_DIR/LIVI.log
StandardError=inherit
Environment=ELECTRON_OZONE_PLATFORM_HINT=wayland
Environment=LIVI_KIOSK=1
ExecStart=$CAGE_BIN -s -- $APPIMAGE_PATH
ExecStopPost=-$SYSTEMCTL_BIN start getty@tty1.service
Restart=no

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable livi-kiosk.service

echo ""
echo "✅ LIVI headless installation complete."
echo ""
echo "Reboot to launch LIVI in kiosk mode on tty1:"
echo "    sudo reboot"
echo ""
echo "To exit kiosk for debugging, switch to a different VT (Ctrl+Alt+F2)."
echo "Shutting LIVI down from its own menu ends the service and leaves you on tty1."
echo "To disable kiosk autostart, run 'sudo systemctl disable livi-kiosk.service'."
echo "To disable autologin, remove $GETTY_DROPIN and run 'sudo systemctl daemon-reload'."
echo "To get a graphical login back, run 'sudo systemctl set-default graphical.target'."
