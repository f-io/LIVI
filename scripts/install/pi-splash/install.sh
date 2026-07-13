#!/usr/bin/env bash
# Install the LIVI animated Plymouth boot splash on Raspberry Pi OS.
# Run as root (or via sudo) on the Pi.
set -euo pipefail

THEME_NAME="livi"
THEME_DIR="/usr/share/plymouth/themes/${THEME_NAME}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOGO_SRC="${SCRIPT_DIR}/assets/images/livi-splash.png"
DEFAULT_VIDEO_1="${SCRIPT_DIR}/assets/videos/default1.h264"
DEFAULT_VIDEO_2="${SCRIPT_DIR}/assets/videos/default2.h264"
APPLY_SPLASH_SRC="${SCRIPT_DIR}/installers/apply-plymouth-splash.sh"
APPLY_SPLASH_DEST="/usr/local/bin/livi-plymouth-apply-splash.sh"

CONFIG_TXT=""
CMDLINE_TXT=""

if [[ $EUID -ne 0 ]]; then
  echo "Run with sudo: sudo $0" >&2
  exit 1
fi

write_boot_splash_id() {
  local config_file="$1"
  local splash_id="$2"

  install -d -m 0755 "$(dirname "${config_file}")"

  if command -v python3 >/dev/null 2>&1; then
    python3 - "${config_file}" "${splash_id}" <<'PY'
import json
import sys
from pathlib import Path

config_path = Path(sys.argv[1])
splash_id = sys.argv[2]

data = {}
if config_path.exists():
    try:
        data = json.loads(config_path.read_text(encoding="utf-8"))
        if not isinstance(data, dict):
            data = {}
    except Exception:
        data = {}

data["bootSplashId"] = splash_id
config_path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
PY
  else
    echo "Warning: python3 not found; bootSplashId was not written to ${config_file}" >&2
  fi
}

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
apt-get install -y plymouth plymouth-themes ffmpeg

echo "[2/6] Writing theme scaffold to ${THEME_DIR}"
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
  TARGET_DIR="${TARGET_HOME}/.config/LIVI/splashscreen/videos"
  install -d -m 0755 -o "${TARGET_USER}" -g "${TARGET_USER}" "${TARGET_DIR}"
  install -m 0644 -o "${TARGET_USER}" -g "${TARGET_USER}" "${DEFAULT_VIDEO_1}" "${TARGET_DIR}/splash1.h264"
  install -m 0644 -o "${TARGET_USER}" -g "${TARGET_USER}" "${DEFAULT_VIDEO_2}" "${TARGET_DIR}/splash2.h264"
  printf '%s\n' "Default" > "${TARGET_HOME}/.config/LIVI/splashscreen/selection.txt"
  chown "${TARGET_USER}:${TARGET_USER}" "${TARGET_HOME}/.config/LIVI/splashscreen/selection.txt"
  write_boot_splash_id "${TARGET_CONFIG_FILE}" "default"
  chown "${TARGET_USER}:${TARGET_USER}" "${TARGET_CONFIG_FILE}"
  "${APPLY_SPLASH_DEST}" "${TARGET_USER}"
else
  echo "Skipping default selection install because no non-root sudo user was detected."
  echo "After install, run:"
  echo "  scripts/install/pi-splash/tools/set-splashscreen.sh \"Default\""
fi

echo
echo "Done. Reboot to see the new animated splash."
