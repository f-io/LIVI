#!/usr/bin/env bash
# Install the LIVI animated Plymouth boot splash on Raspberry Pi OS.
# Run as root (or via sudo) on the Pi.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMMON_SRC="${SCRIPT_DIR}/lib/pi-splash-common.sh"

if [[ ! -f "${COMMON_SRC}" ]]; then
  echo "Missing ${COMMON_SRC}" >&2
  exit 1
fi

# shellcheck source=lib/pi-splash-common.sh
source "${COMMON_SRC}"

THEME_NAME="livi"
THEME_DIR="/usr/share/plymouth/themes/${THEME_NAME}"
SYSTEM_ASSETS_DIR="${LIVI_SPLASH_SYSTEM_ASSETS_DIR}"
ASSETS_SRC="${SCRIPT_DIR}/assets"
LOGO_SRC="${ASSETS_SRC}/images/livi-splash.png"
DEFAULT_VIDEO_1="${ASSETS_SRC}/videos/default1.h264"
DEFAULT_VIDEO_2="${ASSETS_SRC}/videos/default2.h264"
APPLY_SPLASH_SRC="${SCRIPT_DIR}/installers/apply-plymouth-splash.sh"
APPLY_SPLASH_DEST="/usr/local/bin/livi-plymouth-apply-splash.sh"
COMMON_DEST="/usr/local/lib/livi/pi-splash/pi-splash-common.sh"

CONFIG_TXT=""
CMDLINE_TXT=""

if [[ $EUID -ne 0 ]]; then
  echo "Run with sudo: sudo $0" >&2
  exit 1
fi

if [[ ! -f "${LOGO_SRC}" ]]; then
  echo "Missing ${LOGO_SRC}" >&2
  echo "Place a transparent-background PNG." >&2
  exit 1
fi

if [[ ! -f "${DEFAULT_VIDEO_1}" || ! -f "${DEFAULT_VIDEO_2}" ]]; then
  echo "Missing default splash videos in ${SCRIPT_DIR}/assets/videos" >&2
  exit 1
fi

if [[ ! -f "${APPLY_SPLASH_SRC}" ]]; then
  echo "Missing ${APPLY_SPLASH_SRC}" >&2
  exit 1
fi

CONFIG_TXT="/boot/firmware/config.txt"
CMDLINE_TXT="/boot/firmware/cmdline.txt"
if [[ ! -f "${CONFIG_TXT}" ]] || [[ ! -f "${CMDLINE_TXT}" ]]; then
  echo "Expected ${CONFIG_TXT} and ${CMDLINE_TXT} (Pi OS Trixie)" >&2
  exit 1
fi

echo "[1/6] Installing Plymouth + ffmpeg"
apt-get update -qq
apt-get install -y plymouth plymouth-themes ffmpeg python3

echo "[2/6] Installing assets and theme scaffold"
install -d -m 0755 "${SYSTEM_ASSETS_DIR}"
cp -a "${ASSETS_SRC}/." "${SYSTEM_ASSETS_DIR}/"
find "${SYSTEM_ASSETS_DIR}" -type d -exec chmod 0755 {} +
find "${SYSTEM_ASSETS_DIR}" -type f -exec chmod 0644 {} +

install -d -m 0755 "$(dirname "${COMMON_DEST}")"
install -m 0644 "${COMMON_SRC}" "${COMMON_DEST}"
install -d -m 0755 "${THEME_DIR}"
install -m 0644 "${LOGO_SRC}" "${THEME_DIR}/logo.png"
install -m 0755 "${APPLY_SPLASH_SRC}" "${APPLY_SPLASH_DEST}"

cat > "${THEME_DIR}/${THEME_NAME}.plymouth" <<EOF
[Plymouth Theme]
Name=LIVI
Description=LIVI animated boot splash
ModuleName=script

[script]
ImageDir=${THEME_DIR}
ScriptFile=${THEME_DIR}/${THEME_NAME}.script
EOF

cat > "${THEME_DIR}/${THEME_NAME}.script" <<'EOF'
Window.SetBackgroundTopColor(0, 0, 0);
Window.SetBackgroundBottomColor(0, 0, 0);

logo = Image("logo.png");
sprite = Sprite();
scaled_for_h = 0;

# Scale the logo to at most 75% of display height (keep aspect, no upscaling, fit width), then center.
fun refresh() {
  win_w = Window.GetWidth();
  win_h = Window.GetHeight();
  if (win_w > 0) {
    if (win_h > 0) {
      scale = win_h * 0.75 / logo.GetHeight();
      if (scale > 1) scale = 1;
      if (logo.GetWidth() * scale > win_w) scale = win_w / logo.GetWidth();
      logo_w = logo.GetWidth() * scale;
      logo_h = logo.GetHeight() * scale;
      if (scaled_for_h != win_h) {
        sprite.SetImage(logo.Scale(logo_w, logo_h));
        scaled_for_h = win_h;
      }
      sprite.SetPosition(win_w / 2 - logo_w / 2, win_h / 2 - logo_h / 2, 10);
    }
  }
}
Plymouth.SetRefreshFunction(refresh);
EOF

echo "[3/6] Activating theme + rebuilding initramfs"
plymouth-set-default-theme "${THEME_NAME}" -R

# disable_fw_kms_setup=1 means KMS comes up late; let plymouth wait for it
install -d -m 0755 /etc/plymouth
cat > /etc/plymouth/plymouthd.conf <<EOF
[Daemon]
Theme=${THEME_NAME}
ShowDelay=0
DeviceTimeout=30
EOF

echo "[4/6] Patching ${CONFIG_TXT}"
# Pi rainbow off (we run plymouth instead)
if ! grep -qE '^\s*disable_splash=1' "${CONFIG_TXT}"; then
  echo "disable_splash=1" >> "${CONFIG_TXT}"
fi
# Firmware mode-set must run early; otherwise plymouth renders into offline HDMI
sed -i 's/^disable_fw_kms_setup=1$/# disable_fw_kms_setup=1     # disabled by pi-splash for early HDMI/' "${CONFIG_TXT}"

echo "[5/6] Patching ${CMDLINE_TXT}"
cp -a "${CMDLINE_TXT}" "${CMDLINE_TXT}.bak.$(date +%s)"
LINE=$(tr -d '\n' < "${CMDLINE_TXT}")

add_flag() {
  local flag="$1"
  case " ${LINE} " in
    *" ${flag} "*) ;;
    *) LINE="${LINE} ${flag}" ;;
  esac
}

add_flag "quiet"
add_flag "splash"
add_flag "plymouth.ignore-serial-consoles"
add_flag "loglevel=0"

# Auto-detect the active display mode so plymouth doesn't render at EDID
# preferred (often 4K). Falls back to LIVI_SPLASH_VIDEO env if detection fails.
detect_video_mode() {
  [[ -z "${SUDO_USER:-}" ]] && return 1
  command -v wlr-randr >/dev/null || return 1
  local uid runtime
  uid=$(id -u "${SUDO_USER}")
  runtime="/run/user/${uid}"
  [[ -S "${runtime}/wayland-0" ]] || return 1
  local out
  out=$(WAYLAND_DISPLAY=wayland-0 XDG_RUNTIME_DIR="${runtime}" \
        runuser -u "${SUDO_USER}" -- wlr-randr 2>/dev/null) || return 1
  awk '
    /^[^[:space:]]/ { conn=$1 }
    /current/ {
      for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+/) mode=$i
      for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+\.[0-9]+/) hz=int($i+0.5)
      if (conn && mode && hz) { printf "%s:%s@%d", conn, mode, hz; exit }
    }
  ' <<< "${out}"
}

VIDEO_MODE="${LIVI_SPLASH_VIDEO:-$(detect_video_mode || true)}"
if [[ -n "${VIDEO_MODE}" ]]; then
  echo "      video mode = ${VIDEO_MODE}"
  LINE=$(echo "${LINE}" | sed -E 's/[[:space:]]*video=[^[:space:]]+//g')
  LINE="${LINE} video=${VIDEO_MODE}"
else
  echo "      no video mode pinned (plymouth will use EDID preferred)"
fi
add_flag "logo.nologo"
add_flag "vt.global_cursor_default=0"

echo "${LINE}" > "${CMDLINE_TXT}"

echo "[6/6] Installing default animated splash"
systemctl disable --now livi-boot-video.service 2>/dev/null || true
rm -f /etc/systemd/system/livi-boot-video.service /usr/local/bin/livi-boot-video-player.sh /etc/livi-boot-video.env
systemctl daemon-reload

if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
  TARGET_USER="${SUDO_USER}"
  TARGET_HOME="$(getent passwd "${TARGET_USER}" | cut -d: -f6)"
  TARGET_CONFIG_FILE="${TARGET_HOME}/.config/LIVI/config.json"
  install -d -m 0755 -o "${TARGET_USER}" -g "${TARGET_USER}" "${TARGET_HOME}/.config/LIVI"
  install -d -m 0755 -o "${TARGET_USER}" -g "${TARGET_USER}" "${TARGET_HOME}/.config/LIVI/splashscreen"
  printf '%s\n' "default" > "${TARGET_HOME}/.config/LIVI/splashscreen/selection.txt"
  chown "${TARGET_USER}:${TARGET_USER}" "${TARGET_HOME}/.config/LIVI/splashscreen/selection.txt"
  livi_splash_write_boot_splash_id "${TARGET_CONFIG_FILE}" "default"
  chown "${TARGET_USER}:${TARGET_USER}" "${TARGET_CONFIG_FILE}"
  "${APPLY_SPLASH_DEST}" "${TARGET_USER}"
else
  echo "Skipping default selection install because no non-root sudo user was detected."
  echo "After install, run:"
  echo "  scripts/install/pi-splash/tools/set-splashscreen.sh \"Default\""
fi

echo
echo "Done. Reboot to see the new animated splash."
