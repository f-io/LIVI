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
  wlr-randr \
  pipewire wireplumber pipewire-pulse \
  gstreamer1.0-plugins-base \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad \
  gstreamer1.0-gl \
  gstreamer1.0-libav \
  gstreamer1.0-tools \
  python3-dbus python3-gi \
  bluez hostapd dnsmasq-base iw rfkill

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

# Raspberry Pi 1080p HEVC needs the v4l2codecs SAND-crop fix. GStreamer 1.26.x
# (< 1.26.11, which the Pi currently ships) detiles the 1088-padded 1080p frame into
# SystemMemory; waylandsink rejects that and the main video layer hard-fails (black).
# Build the crop-fixed plugin from the distro source on-device. Upstream-fixed in
# 1.26.11 / 1.28.x, where this detects the version and skips. Non-fatal.
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
  export LIVI_KIOSK=1
  LIVI_KIOSK_MODE="\${LIVI_KIOSK_MODE:-native}"

  # Default: Cage uses the display's preferred mode.
  # Set LIVI_KIOSK_MODE=WxH@Hz to force a mode (e.g. cap a 4K panel at 1080p).
  if [ "\$LIVI_KIOSK_MODE" != "native" ]; then
    (
      export XDG_RUNTIME_DIR=/run/user/\$(id -u)
      for _ in \$(seq 1 40); do
        if [ -S "\$XDG_RUNTIME_DIR/wayland-0" ]; then
          OUT=\$(WAYLAND_DISPLAY=wayland-0 wlr-randr 2>/dev/null \\
            | awk 'NR==1 {print \$1; exit}')
          [ -n "\$OUT" ] && WAYLAND_DISPLAY=wayland-0 \\
            wlr-randr --output "\$OUT" --mode "\$LIVI_KIOSK_MODE" 2>/dev/null
          break
        fi
        sleep 0.25
      done
    ) &
  fi

  exec cage -- "$APPIMAGE_PATH" >"$APPIMAGE_DIR/LIVI.log" 2>&1
fi
EOF
else
  echo "→ Kiosk autostart already present in ~/.bash_profile, leaving as is"
fi

# Remove the USB re-enumerate service from earlier installs, it cycles port power and disrupts mouse/touch
if [ -f /etc/systemd/system/livi-usb-rescan.service ]; then
  echo "→ Removing legacy USB re-enumerate service"
  sudo systemctl disable --now livi-usb-rescan.service 2>/dev/null || true
  sudo rm -f /etc/systemd/system/livi-usb-rescan.service /usr/local/bin/livi-usb-rescan.sh
  sudo systemctl daemon-reload
fi

echo ""
echo "✅ LIVI Lite installation complete."
echo ""
echo "Reboot to launch LIVI in kiosk mode on tty1:"
echo "    sudo reboot"
echo ""
echo "To exit kiosk for debugging, switch to a different VT (Ctrl+Alt+F2)."
echo "To disable kiosk autostart, remove the '$KIOSK_MARKER' block from ~/.bash_profile."
