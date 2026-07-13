#!/usr/bin/env bash
set -euo pipefail

THEME_NAME="livi"
THEME_DIR="/usr/share/plymouth/themes/${THEME_NAME}"
SERVICE_PATH="/etc/systemd/system/livi-boot-video.service"
PLAYER_PATH="/usr/local/bin/livi-boot-video-player.sh"
APPLY_HELPER="/usr/local/bin/livi-plymouth-apply-splash.sh"
PLYMOUTH_CONF="/etc/plymouth/plymouthd.conf"
CONFIG_TXT="/boot/firmware/config.txt"
CMDLINE_TXT="/boot/firmware/cmdline.txt"

if [[ $EUID -ne 0 ]]; then
  echo "Run with sudo: sudo $0 [username]" >&2
  exit 1
fi

detect_target_user() {
  local user_arg="${1:-}"
  if [[ -n "${user_arg}" ]]; then
    echo "${user_arg}"
    return 0
  fi
  if [[ -n "${SUDO_USER:-}" && "${SUDO_USER}" != "root" ]]; then
    echo "${SUDO_USER}"
    return 0
  fi
  if [[ -f "${SERVICE_PATH}" ]]; then
    sed -n 's/^User=//p' "${SERVICE_PATH}" | sed -n '1p'
    return 0
  fi
  return 1
}

pick_fallback_theme() {
  local candidate
  for candidate in pix spinner bgrt details text; do
    if [[ -f "/usr/share/plymouth/themes/${candidate}/${candidate}.plymouth" ]]; then
      echo "${candidate}"
      return 0
    fi
  done
  return 1
}

restore_cmdline() {
  if [[ ! -f "${CMDLINE_TXT}" ]]; then
    echo "Skipping cmdline restore: ${CMDLINE_TXT} not found."
    return 0
  fi

  local latest_backup=""
  latest_backup="$(find "$(dirname "${CMDLINE_TXT}")" -maxdepth 1 -type f -name 'cmdline.txt.bak.*' | sort | tail -n 1 || true)"

  if [[ -n "${latest_backup}" ]]; then
    echo "→ Restoring ${CMDLINE_TXT} from ${latest_backup}"
    cp -f "${latest_backup}" "${CMDLINE_TXT}"
    return 0
  fi

  echo "→ No cmdline backup found, removing LIVI splash flags inline"
  local line
  line="$(tr -d '\n' < "${CMDLINE_TXT}")"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])quiet($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])splash($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])plymouth\.ignore-serial-consoles($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])loglevel=0($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])logo\.nologo($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])vt\.global_cursor_default=0($|[[:space:]])/ /g')"
  line="$(echo "${line}" | sed -E 's/(^|[[:space:]])video=[^[:space:]]+($|[[:space:]])/ /g')"
  echo "${line}" | xargs > "${CMDLINE_TXT}"
}

echo "[1/6] Stopping and removing legacy LIVI boot-video service"
if systemctl list-unit-files | grep -q '^livi-boot-video.service'; then
  systemctl disable --now livi-boot-video.service 2>/dev/null || true
fi
rm -f "${SERVICE_PATH}" "${PLAYER_PATH}" /etc/livi-boot-video.env
systemctl daemon-reload

echo "[2/6] Removing LIVI Plymouth theme"
rm -rf "${THEME_DIR}"
rm -f "${APPLY_HELPER}"

echo "[3/6] Restoring default Plymouth theme"
if command -v plymouth-set-default-theme >/dev/null 2>&1; then
  if fallback_theme="$(pick_fallback_theme)"; then
    plymouth-set-default-theme "${fallback_theme}" -R || true
  else
    echo "No fallback Plymouth theme found; leaving the current default as-is."
  fi
fi

echo "[4/6] Restoring ${PLYMOUTH_CONF}"
if [[ -f "${PLYMOUTH_CONF}" ]] && grep -q "Theme=${THEME_NAME}" "${PLYMOUTH_CONF}"; then
  rm -f "${PLYMOUTH_CONF}"
fi

echo "[5/6] Reverting Raspberry Pi boot config"
if [[ -f "${CONFIG_TXT}" ]]; then
  sed -i '' -e '/^disable_splash=1$/d' "${CONFIG_TXT}" 2>/dev/null || \
    sed -i -e '/^disable_splash=1$/d' "${CONFIG_TXT}"
  sed -i '' -e 's/^# disable_fw_kms_setup=1     # disabled by pi-splash for early HDMI$/disable_fw_kms_setup=1/' "${CONFIG_TXT}" 2>/dev/null || \
    sed -i -e 's/^# disable_fw_kms_setup=1     # disabled by pi-splash for early HDMI$/disable_fw_kms_setup=1/' "${CONFIG_TXT}"
fi
restore_cmdline

echo "[6/6] Removing installed splash selection"
TARGET_USER="$(detect_target_user "${1:-}" || true)"
if [[ -n "${TARGET_USER}" ]] && id "${TARGET_USER}" >/dev/null 2>&1; then
  TARGET_HOME="$(getent passwd "${TARGET_USER}" | cut -d: -f6)"
  if [[ -n "${TARGET_HOME}" && -d "${TARGET_HOME}" ]]; then
    rm -rf "${TARGET_HOME}/.config/LIVI/splashscreen" "${TARGET_HOME}/.cache/LIVI"
  fi
fi

echo
echo "LIVI splash components removed."
echo "Reboot to return to the standard Raspberry Pi boot flow."
