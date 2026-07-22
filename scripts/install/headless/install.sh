#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Headless Installer (any systemd + apt host)
# ----------------------------------------
# Kiosk-style install for a host with no desktop session. Works on Raspberry Pi
# OS Lite and on x86 Debian alike.
#
#   - Installs Cage (Wayland kiosk compositor), seatd, PipeWire
#   - Asks for the release or the nightly channel and picks the AppImage
#     matching this machine's architecture (LIVI_CHANNEL skips the prompt)
#   - Extracts the bundled udev rule template from the AppImage and writes
#     /etc/udev/rules.d/99-LIVI.rules with the matching version marker so the
#     in-app pkexec dialog stays silent on first launch
#   - Writes /etc/sudoers.d/99-LIVI-bt from the bundled template so the BT/Wi-Fi
#     helper can run as root, which a headless host cannot set up from the app
#   - Configures tty1 autologin through a systemd getty drop-in and a Cage
#     autostart in ~/.bash_profile
#
# Re-runnable. Refuses to run as root (sudo is used internally).

if [[ $EUID -eq 0 ]]; then
  echo "Run as a regular user. sudo is used internally where needed." >&2
  exit 1
fi

if ! command -v apt-get >/dev/null; then
  echo "Error: this installer needs apt. Install the packages from" >&2
  echo "scripts/install/packages.txt by hand, then run the remaining steps." >&2
  exit 1
fi

USER_HOME="$HOME"
APPIMAGE_PATH="$USER_HOME/LIVI/LIVI.AppImage"
APPIMAGE_DIR="$(dirname "$APPIMAGE_PATH")"
RULE_FILE="/etc/udev/rules.d/99-LIVI.rules"
TEMPLATE_NAME="99-LIVI.rules.template"
SUDOERS_FILE="/etc/sudoers.d/99-LIVI-bt"
SUDOERS_TEMPLATE_NAME="99-LIVI-bt.sudoers.template"
GETTY_DROPIN="/etc/systemd/system/getty@tty1.service.d/livi-autologin.conf"

# Release assets are named LIVI-<version>-linux-<arch>.AppImage, where <arch> is
# electron-builder's spelling: x86_64 or arm64.
case "$(uname -m)" in
  x86_64|amd64)   ASSET_ARCH="x86_64" ;;
  aarch64|arm64)  ASSET_ARCH="arm64" ;;
  *)
    echo "Error: unsupported architecture $(uname -m). LIVI ships x86_64 and arm64." >&2
    exit 1
    ;;
esac
echo "→ Architecture: $(uname -m) → ${ASSET_ARCH}.AppImage"

# Package list comes from scripts/install/packages.txt, the single source the app checks too.
MANIFEST_URL="https://raw.githubusercontent.com/f-io/LIVI/main/scripts/install/packages.txt"
livi_packages() {
  local here manifest tmp section
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  manifest="$here/../packages.txt"
  if [ ! -f "$manifest" ]; then
    tmp="$(mktemp)"
    curl -fsSL "$MANIFEST_URL" -o "$tmp" || { echo "Error: cannot obtain packages.txt" >&2; return 1; }
    manifest="$tmp"
  fi
  for section in "$@"; do
    grep -E "^${section}\|" "$manifest" | cut -d '|' -f2
  done
}

echo "→ Installing required packages"
sudo apt-get update
sudo apt-get install -y $(livi_packages core lite | tr '\n' ' ')

# pymobiledevice3 drives wired CarPlay over usbmux/lockdown
pip3 install --break-system-packages --ignore-installed -q pymobiledevice3 \
  || echo "   pymobiledevice3 install failed — wired CarPlay will be disabled"

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

# release = the latest tagged release, nightly = the rolling prerelease the CI
# replaces on every green build of main. LIVI_CHANNEL skips the prompt so
# unattended runs keep working.
lowercase() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

CHANNEL="$(lowercase "${LIVI_CHANNEL:-}")"
if [ -z "$APPIMAGE_SRC" ] && [ -z "$CHANNEL" ]; then
  if [ -t 0 ]; then
    echo ""
    echo "Which build should be installed?"
    echo "  1) release   the latest tagged release"
    echo "  2) nightly   the latest build of main"
    read -r -p "Choice [1]: " CHANNEL_REPLY || true
    case "$(lowercase "${CHANNEL_REPLY:-}")" in
      2|n|nightly) CHANNEL="nightly" ;;
      *)           CHANNEL="release" ;;
    esac
  else
    CHANNEL="release"
  fi
fi
CHANNEL="${CHANNEL:-release}"

case "$CHANNEL" in
  release) RELEASE_API="https://api.github.com/repos/f-io/LIVI/releases/latest" ;;
  nightly) RELEASE_API="https://api.github.com/repos/f-io/LIVI/releases/tags/nightly" ;;
  *)
    echo "Error: unknown channel '$CHANNEL'. Use release or nightly." >&2
    exit 1
    ;;
esac

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
  echo "→ Fetching the latest $CHANNEL build"
  latest_url=$(curl -s "$RELEASE_API" \
    | grep "browser_download_url" \
    | grep "${ASSET_ARCH}.AppImage" \
    | cut -d '"' -f 4)

  if [ -z "$latest_url" ]; then
    echo "Error: no ${ASSET_ARCH} AppImage in the latest $CHANNEL build" >&2
    exit 1
  fi

  echo "   Download URL: $latest_url"
  if ! curl -L "$latest_url" --output "$APPIMAGE_PATH"; then
    echo "Error: Download failed" >&2
    exit 1
  fi
fi

chmod +x "$APPIMAGE_PATH"

EXTRACT_DIR="$(mktemp -d)"
trap "rm -rf '$EXTRACT_DIR'" EXIT

# Take a template from inside the AppImage so what we install always matches the
# app on this disk.
fetch_template() {
  local name="$1" dir path branch url
  dir="$EXTRACT_DIR/$name.d"
  mkdir -p "$dir"
  ( cd "$dir" && "$APPIMAGE_PATH" --appimage-extract "resources/$name" >/dev/null 2>&1 ) || true
  path="$dir/squashfs-root/resources/$name"
  if [ ! -f "$path" ]; then
    branch="${LIVI_TEMPLATE_BRANCH:-main}"
    url="https://raw.githubusercontent.com/f-io/LIVI/${branch}/assets/linux/${name}"
    echo "   $name not in AppImage (likely older release), falling back to $url" >&2
    path="$dir/$name"
    curl -fL "$url" -o "$path" >/dev/null 2>&1 || return 1
  fi
  printf '%s\n' "$path"
}

echo "→ Extracting rule templates from the AppImage"
UDEV_TEMPLATE="$(fetch_template "$TEMPLATE_NAME")" || {
  echo "Error: cannot obtain $TEMPLATE_NAME" >&2
  exit 1
}
SUDOERS_TEMPLATE="$(fetch_template "$SUDOERS_TEMPLATE_NAME")" || {
  echo "Error: cannot obtain $SUDOERS_TEMPLATE_NAME" >&2
  exit 1
}

echo "→ Writing $RULE_FILE"
sed "s/__USERNAME__/$USER/g" "$UDEV_TEMPLATE" | sudo tee "$RULE_FILE" >/dev/null
sudo udevadm control --reload-rules
sudo udevadm trigger

# Lets the helper run as root without a password, which a headless host needs
# because the in-app pkexec dialog has no agent to display it.
echo "→ Writing $SUDOERS_FILE"
PYTHON_BIN="$(command -v python3 || echo /usr/bin/python3)"
STAGED="$EXTRACT_DIR/sudoers.staged"
sed -e "s/__USERNAME__/$USER/g" -e "s#__PYTHON__#$PYTHON_BIN#g" "$SUDOERS_TEMPLATE" > "$STAGED"
sudo install -m 0440 -o root -g root "$STAGED" "$SUDOERS_FILE.livi-tmp"
if sudo visudo -c -f "$SUDOERS_FILE.livi-tmp" >/dev/null; then
  sudo mv "$SUDOERS_FILE.livi-tmp" "$SUDOERS_FILE"
else
  sudo rm -f "$SUDOERS_FILE.livi-tmp"
  echo "Error: the generated sudoers file failed validation and was not installed" >&2
  exit 1
fi

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
