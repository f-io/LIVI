# LIVI installer library, sourced by scripts/install/*/install.sh.

LIVI_REPO="${LIVI_REPO:-f-io/LIVI}"
LIVI_BRANCH="${LIVI_INSTALLER_BRANCH:-main}"
LIVI_RAW="https://raw.githubusercontent.com/${LIVI_REPO}/${LIVI_BRANCH}"
LIVI_API="https://api.github.com/repos/${LIVI_REPO}"
LIVI_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

LIVI_UDEV_FILE="/etc/udev/rules.d/99-LIVI.rules"
LIVI_UDEV_TEMPLATE="99-LIVI.rules.template"
LIVI_TOUCH_FILTER_TEMPLATE="livi-touch-filter"
LIVI_TOUCH_FILTER_FILE="/usr/local/lib/livi/livi-touch-filter"
LIVI_SUDOERS_FILE="/etc/sudoers.d/99-LIVI-bt"
LIVI_SUDOERS_TEMPLATE="99-LIVI-bt.sudoers.template"
LIVI_BOOT_CONFIG="${LIVI_BOOT_CONFIG:-/boot/firmware/config.txt}"

# I2C for the Apple MFi coprocessor, matching carPlayMfiI2cBus in config.json
LIVI_MFI_I2C_BUS="2"
LIVI_MFI_OVERLAY="dtoverlay=i2c-gpio,bus=${LIVI_MFI_I2C_BUS},i2c_gpio_sda=19,i2c_gpio_scl=26,i2c_gpio_delay_us=5"
LIVI_MODULES_LOAD="${LIVI_MODULES_LOAD:-/etc/modules-load.d/livi-i2c.conf}"
LIVI_NM_POWERSAVE_FILE="/etc/NetworkManager/conf.d/99-LIVI-wifi-powersave.conf"

# Pixel repetition for RGB/VGA panels below HDMI's clock floor
LIVI_HDMI_PR_SCRIPT="setup-hdmi-pr-display.sh"
LIVI_DISPLAYS_DIR="displays"

livi_lower() { printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]'; }

livi_require_regular_user() {
  if [ "$(id -u)" -eq 0 ]; then
    echo "Run as a regular user. sudo is used internally where needed." >&2
    exit 1
  fi
}

# Release assets are named LIVI-<version>-linux-<arch>.AppImage, where <arch> is
# electron-builder's spelling: x86_64 or arm64.
livi_asset_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  printf 'x86_64\n' ;;
    aarch64|arm64) printf 'arm64\n' ;;
    *)
      echo "Error: unsupported architecture $(uname -m). LIVI ships x86_64 and arm64." >&2
      return 1
      ;;
  esac
}

# Package names for the given sections of packages.txt, the single source the app checks too.
livi_packages() {
  local manifest tmp section
  manifest="$LIVI_LIB_DIR/packages.txt"
  if [ ! -f "$manifest" ]; then
    tmp="$(mktemp)"
    curl -fsSL "$LIVI_RAW/scripts/install/packages.txt" -o "$tmp" \
      || { echo "Error: cannot obtain packages.txt" >&2; return 1; }
    manifest="$tmp"
  fi
  for section in "$@"; do
    grep -E "^${section}\|" "$manifest" | cut -d '|' -f2
  done
}

# Wired CarPlay drives the phone over usbmux, which needs pymobiledevice3.
livi_install_pymobiledevice3() {
  echo "→ Installing pymobiledevice3 for wired CarPlay"
  if ! command -v pip3 >/dev/null; then
    echo "   WARNING: pip3 missing, wired CarPlay stays disabled" >&2
    return 0
  fi
  local out
  # pip is loud even with -q, so keep its output for the failure case only.
  if out="$(pip3 install --break-system-packages --ignore-installed -q pymobiledevice3 2>&1)"; then
    echo "   installed"
  else
    echo "   WARNING: install failed, wired CarPlay stays disabled" >&2
    printf '%s\n' "$out" | tail -5 >&2
  fi
}

# Sets LIVI_CHANNEL and LIVI_RELEASE_API. Skips the prompt when LIVI_CHANNEL is
# already set or an AppImage was passed in, so unattended runs keep working.
livi_pick_channel() {
  local have_src="${1:-}" reply
  LIVI_CHANNEL="$(livi_lower "${LIVI_CHANNEL:-}")"
  if [ -z "$have_src" ] && [ -z "$LIVI_CHANNEL" ]; then
    if [ -t 0 ]; then
      echo ""
      echo "Which build should be installed?"
      echo "  1) release   the latest tagged release"
      echo "  2) nightly   the latest build of main"
      read -r -p "Choice [1]: " reply || true
      case "$(livi_lower "${reply:-}")" in
        2|n|nightly) LIVI_CHANNEL="nightly" ;;
        *)           LIVI_CHANNEL="release" ;;
      esac
    else
      LIVI_CHANNEL="release"
    fi
  fi
  LIVI_CHANNEL="${LIVI_CHANNEL:-release}"

  case "$LIVI_CHANNEL" in
    release) LIVI_RELEASE_API="$LIVI_API/releases/latest" ;;
    nightly) LIVI_RELEASE_API="$LIVI_API/releases/tags/nightly" ;;
    *)
      echo "Error: unknown channel '$LIVI_CHANNEL'. Use release or nightly." >&2
      return 1
      ;;
  esac
}

# livi_fetch_appimage <dest> [source]
# source is an optional local path or http(s) URL, otherwise the picked channel.
livi_fetch_appimage() {
  local dest="$1" src="${2:-}" arch url
  mkdir -p "$(dirname "$dest")"

  if [ -n "$src" ]; then
    if [[ "$src" =~ ^https?:// ]]; then
      echo "→ Downloading AppImage from $src"
      curl -fL "$src" --output "$dest" || { echo "Error: download failed" >&2; return 1; }
    elif [ -f "$src" ]; then
      echo "→ Using local AppImage at $src"
      cp "$src" "$dest"
    else
      echo "Error: AppImage source not found: $src" >&2
      return 1
    fi
  else
    arch="$(livi_asset_arch)" || return 1
    echo "→ Fetching the latest $LIVI_CHANNEL build for $arch"
    url="$(curl -s "$LIVI_RELEASE_API" \
      | grep browser_download_url \
      | grep "${arch}.AppImage" \
      | cut -d '"' -f 4)"
    if [ -z "$url" ]; then
      echo "Error: no ${arch} AppImage in the latest $LIVI_CHANNEL build" >&2
      return 1
    fi
    echo "   Download URL: $url"
    curl -fL "$url" --output "$dest" || { echo "Error: download failed" >&2; return 1; }
  fi

  chmod +x "$dest"
}

# livi_fetch_template <appimage> <name> -> prints the path to the extracted template.
# Taken from inside the AppImage so what is installed matches the app on this disk,
# with the repository copy as the fallback for older releases. Each template gets
# its own directory because --appimage-extract takes one pattern.
livi_fetch_template() {
  local appimage="$1" name="$2" dir path
  dir="${LIVI_EXTRACT_DIR:-$(mktemp -d)}/$name.d"
  mkdir -p "$dir"
  ( cd "$dir" && "$appimage" --appimage-extract "resources/$name" >/dev/null 2>&1 ) || true
  path="$dir/squashfs-root/resources/$name"
  if [ ! -f "$path" ]; then
    echo "   $name not in AppImage (likely older release), falling back to the repository" >&2
    path="$dir/$name"
    curl -fL "$LIVI_RAW/assets/linux/$name" -o "$path" >/dev/null 2>&1 || return 1
  fi
  printf '%s\n' "$path"
}

# The udev rule calls this to tell a real mouse from a touch panel's mouse interface.
livi_install_touch_filter() {
  local script="$1"
  echo "→ Installing $LIVI_TOUCH_FILTER_FILE"
  sudo mkdir -p "$(dirname "$LIVI_TOUCH_FILTER_FILE")"
  sudo install -m 0755 -o root -g root "$script" "$LIVI_TOUCH_FILTER_FILE"
}

# livi_fetch_resource <appimage> <path inside resources> <path in the repository>
# Prints the path to the extracted file. Same order as livi_fetch_template: the
# AppImage on disk first, the repository only for releases that predate the file.
livi_fetch_resource() {
  local appimage="$1" res="$2" repo="$3" dir path
  dir="${LIVI_EXTRACT_DIR:-$(mktemp -d)}/$(printf '%s' "$res" | tr '/' '_').d"
  mkdir -p "$dir"
  ( cd "$dir" && "$appimage" --appimage-extract "resources/$res" >/dev/null 2>&1 ) || true
  path="$dir/squashfs-root/resources/$res"
  if [ ! -f "$path" ]; then
    path="$dir/$(basename "$res")"
    curl -fL "$LIVI_RAW/$repo" -o "$path" >/dev/null 2>&1 || return 1
  fi
  printf '%s\n' "$path"
}

livi_is_raspberry_pi() {
  grep -qi "raspberry pi" /proc/device-tree/model 2>/dev/null
}

# Prints one EDID profile name per line, read from the repository because the
# question comes before the AppImage is on disk.
livi_list_display_profiles() {
  curl -fsSL "$LIVI_API/contents/assets/$LIVI_DISPLAYS_DIR?ref=$LIVI_BRANCH" 2>/dev/null \
    | grep -o '"name"[[:space:]]*:[[:space:]]*"[^"]*\.edid"' \
    | sed 's/.*"\([^"]*\.edid\)"/\1/'
}

# Sets LIVI_HDMI_PR to yes or no and LIVI_HDMI_PR_EDID to the chosen profile.
# Both skip the prompt when already set.
livi_ask_hdmi_pr() {
  local reply profiles count i name
  LIVI_HDMI_PR="$(livi_lower "${LIVI_HDMI_PR:-}")"

  if ! livi_is_raspberry_pi; then
    LIVI_HDMI_PR="no"
    return 0
  fi

  if [ -z "$LIVI_HDMI_PR" ]; then
    if [ -t 0 ]; then
      echo ""
      echo "Is the display an RGB or VGA panel below HDMI's 25 MHz clock floor?"
      echo "  Such a panel cannot be driven over HDMI as it is. LIVI can rebuild the"
      echo "  vc4 driver so it repeats pixels, which lifts the wire clock while the"
      echo "  panel keeps its native resolution. This downloads the kernel source and"
      echo "  compiles one module, so it takes a while."
      read -r -p "Set the display up this way? [y/N]: " reply || true
      case "$(livi_lower "${reply:-}")" in
        y|yes) LIVI_HDMI_PR="yes" ;;
        *)     LIVI_HDMI_PR="no" ;;
      esac
    else
      LIVI_HDMI_PR="no"
    fi
  fi

  case "$LIVI_HDMI_PR" in
    yes|no) ;;
    *)
      echo "Error: unknown LIVI_HDMI_PR '$LIVI_HDMI_PR'. Use yes or no." >&2
      return 1
      ;;
  esac

  [ "$LIVI_HDMI_PR" = "yes" ] || return 0
  [ -z "${LIVI_HDMI_PR_EDID:-}" ] || return 0

  profiles="$(livi_list_display_profiles)"
  if [ -z "$profiles" ]; then
    echo "Error: no display profiles found, cannot continue without one." >&2
    return 1
  fi

  count="$(printf '%s\n' "$profiles" | wc -l | tr -d ' ')"
  echo ""
  echo "Which display?"
  i=1
  printf '%s\n' "$profiles" | while IFS= read -r name; do
    echo "  $i) $name"
    i=$((i + 1))
  done

  if [ ! -t 0 ]; then
    LIVI_HDMI_PR_EDID="$(printf '%s\n' "$profiles" | head -1)"
    return 0
  fi

  read -r -p "Choice [1]: " reply || true
  case "${reply:-1}" in
    ''|*[!0-9]*) reply=1 ;;
  esac
  [ "$reply" -ge 1 ] 2>/dev/null && [ "$reply" -le "$count" ] || reply=1
  LIVI_HDMI_PR_EDID="$(printf '%s\n' "$profiles" | sed -n "${reply}p")"
}

# Runs the pixel repetition setup for the chosen panel.
livi_apply_hdmi_pr() {
  local appimage="$1" script edid
  [ "${LIVI_HDMI_PR:-no}" = "yes" ] || return 0

  echo "→ Setting up $LIVI_HDMI_PR_EDID"
  script="$(livi_fetch_resource "$appimage" "$LIVI_HDMI_PR_SCRIPT" \
    "scripts/install/pi/$LIVI_HDMI_PR_SCRIPT")" || {
    echo "Error: cannot obtain $LIVI_HDMI_PR_SCRIPT" >&2
    return 1
  }
  edid="$(livi_fetch_resource "$appimage" "$LIVI_DISPLAYS_DIR/$LIVI_HDMI_PR_EDID" \
    "assets/$LIVI_DISPLAYS_DIR/$LIVI_HDMI_PR_EDID")" || {
    echo "Error: cannot obtain $LIVI_HDMI_PR_EDID" >&2
    return 1
  }

  bash "$script" --edid "$edid"
}

# The radio's power saving drops an idle link after a few minutes, which takes the
# host off the network. Applies from the next boot, so no running session is cut.
livi_disable_wifi_powersave() {
  echo "→ Writing $LIVI_NM_POWERSAVE_FILE"
  sudo mkdir -p "$(dirname "$LIVI_NM_POWERSAVE_FILE")"
  printf '[connection]\nwifi.powersave = 2\n' | sudo tee "$LIVI_NM_POWERSAVE_FILE" >/dev/null
}

livi_write_udev_rule() {
  local template="$1"
  echo "→ Writing $LIVI_UDEV_FILE"
  sed "s/__USERNAME__/$USER/g" "$template" | sudo tee "$LIVI_UDEV_FILE" >/dev/null
  sudo udevadm control --reload-rules
  sudo udevadm trigger
}

# Lets the helper run as root without a password, which a headless host needs
# because the in-app pkexec dialog has no agent to display it.
livi_write_sudoers() {
  local template="$1" python_bin staged
  echo "→ Writing $LIVI_SUDOERS_FILE"
  python_bin="$(command -v python3 || echo /usr/bin/python3)"
  staged="$(mktemp)"
  sed -e "s/__USERNAME__/$USER/g" -e "s#__PYTHON__#$python_bin#g" "$template" > "$staged"
  sudo install -m 0440 -o root -g root "$staged" "$LIVI_SUDOERS_FILE.livi-tmp"
  rm -f "$staged"
  if sudo visudo -c -f "$LIVI_SUDOERS_FILE.livi-tmp" >/dev/null; then
    sudo mv "$LIVI_SUDOERS_FILE.livi-tmp" "$LIVI_SUDOERS_FILE"
  else
    sudo rm -f "$LIVI_SUDOERS_FILE.livi-tmp"
    echo "Error: the generated sudoers file failed validation and was not installed" >&2
    return 1
  fi
}

# Sets LIVI_MFI to yes or no. LIVI_MFI skips the prompt.
livi_ask_mfi() {
  local reply
  LIVI_MFI="$(livi_lower "${LIVI_MFI:-}")"
  if [ -z "$LIVI_MFI" ]; then
    if [ -t 0 ]; then
      echo ""
      echo "Is an Apple MFi coprocessor wired to this board?"
      echo "  Needed for native CarPlay. Android Auto and the dongle work without it."
      read -r -p "Wire up I2C for it? [y/N]: " reply || true
      case "$(livi_lower "${reply:-}")" in
        y|yes) LIVI_MFI="yes" ;;
        *)     LIVI_MFI="no" ;;
      esac
    else
      LIVI_MFI="no"
    fi
  fi

  case "$LIVI_MFI" in
    yes|no) ;;
    *)
      echo "Error: unknown LIVI_MFI '$LIVI_MFI'. Use yes or no." >&2
      return 1
      ;;
  esac
}

# LIVI_SPLASH
livi_ask_splash() {
  local reply
  LIVI_SPLASH="$(livi_lower "${LIVI_SPLASH:-}")"
  if [ -z "$LIVI_SPLASH" ]; then
    if [ -t 0 ]; then
      echo ""
      echo "Install the LIVI boot splash?"
      echo "  Replaces the Pi rainbow and the boot text with the LIVI logo."
      read -r -p "Install it? [y/N]: " reply || true
      case "$(livi_lower "${reply:-}")" in
        y|yes) LIVI_SPLASH="yes" ;;
        *)     LIVI_SPLASH="no" ;;
      esac
    else
      LIVI_SPLASH="no"
    fi
  fi

  case "$LIVI_SPLASH" in
    yes|no) ;;
    *)
      echo "Error: unknown LIVI_SPLASH '$LIVI_SPLASH'. Use yes or no." >&2
      return 1
      ;;
  esac
}

# Hands over to scripts/install/pi-splash/install.sh, which needs root and the
# logo next to it. Prefers the checkout, otherwise fetches both.
livi_apply_splash() {
  [ "${LIVI_SPLASH:-no}" = "yes" ] || return 0
  local dir script

  if [ ! -f "$LIVI_BOOT_CONFIG" ]; then
    echo "→ Skipping the boot splash, no $LIVI_BOOT_CONFIG on this host"
    return 0
  fi

  echo "→ Installing the LIVI boot splash"
  dir="$LIVI_LIB_DIR/pi-splash"
  if [ ! -f "$dir/install.sh" ] || [ ! -f "$dir/livi-splash.png" ]; then
    dir="$(mktemp -d)"
    curl -fsSL "$LIVI_RAW/scripts/install/pi-splash/install.sh" -o "$dir/install.sh"       && curl -fsSL "$LIVI_RAW/scripts/install/pi-splash/livi-splash.png" -o "$dir/livi-splash.png"       || { echo "   could not obtain the splash installer, skipping" >&2; return 0; }
  fi

  script="$dir/install.sh"
  sudo bash "$script" || echo "   splash install failed, continuing" >&2
}

livi_apply_mfi() {
  [ "${LIVI_MFI:-no}" = "yes" ] || return 0
  local dev="/dev/i2c-${LIVI_MFI_I2C_BUS}"

  echo "→ Enabling I2C for the MFi coprocessor"

  if [ -f "$LIVI_BOOT_CONFIG" ]; then
    if grep -qF "$LIVI_MFI_OVERLAY" "$LIVI_BOOT_CONFIG"; then
      echo "   overlay already in $LIVI_BOOT_CONFIG"
    else
      printf '%s\n' "$LIVI_MFI_OVERLAY" | sudo tee -a "$LIVI_BOOT_CONFIG" >/dev/null
      echo "   overlay added to $LIVI_BOOT_CONFIG"
    fi
  else
    echo "   no $LIVI_BOOT_CONFIG on this host, skipping the overlay"
  fi

  # The overlay registers the bus, but the helper opens /dev/i2c-N and that node
  # only exists once i2c-dev is loaded.
  echo i2c-dev | sudo tee "$LIVI_MODULES_LOAD" >/dev/null
  sudo modprobe i2c-dev 2>/dev/null || true

  if [ ! -e "$dev" ]; then
    echo "   $dev appears after a reboot"
    return 0
  fi

  echo "   $dev is present"
}
