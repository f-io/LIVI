#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMMON_SRC="${ROOT_DIR}/lib/pi-splash-common.sh"

if [[ ! -f "${COMMON_SRC}" ]]; then
  echo "Missing ${COMMON_SRC}" >&2
  exit 1
fi

# shellcheck source=../lib/pi-splash-common.sh
source "${COMMON_SRC}"

REPO_VIDEO_DIR="${ROOT_DIR}/assets/videos"
SYSTEM_VIDEO_DIR="${LIVI_SPLASH_SYSTEM_VIDEO_DIR}"
CONFIG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}/LIVI"
APP_CONFIG_FILE="${CONFIG_ROOT}/config.json"
TARGET_DIR="${CONFIG_ROOT}/splashscreen"
SELECTION_FILE="${TARGET_DIR}/selection.txt"
APPLY_HELPER="/usr/local/bin/livi-plymouth-apply-splash.sh"

print_help() {
  echo "Usage: ./set-splashscreen.sh \"NAME\""
  echo "Example: ./set-splashscreen.sh \"Range Rover\""
  echo
  echo "Stores the selected startup splash id in:"
  echo "  ${APP_CONFIG_FILE}"
  echo
  echo "If the Plymouth helper is installed, the script also asks sudo to rebuild"
  echo "the boot splash immediately."
  echo
  echo "Available names:"
  list_names
}

list_names() {
  local source_dir slug pretty

  echo "  Default"

  if ! source_dir="$(livi_splash_pick_video_dir "${REPO_VIDEO_DIR}")"; then
    return 0
  fi

  while IFS= read -r slug; do
    pretty="$(livi_splash_slug_to_name "$slug")"
    printf '  %s\n' "$pretty"
  done < <(livi_splash_list_slugs "${source_dir}")
}

INPUT="${1:-}"

if [[ "${INPUT}" == "--help" || "${INPUT}" == "-h" ]]; then
  print_help
  exit 0
fi

if [[ -z "${INPUT}" ]]; then
  echo "Try './set-splashscreen.sh --help' for more information." >&2
  exit 1
fi

SLUG="$(livi_splash_to_slug "${INPUT}")"

if ! livi_splash_is_static_id "${SLUG}"; then
  if ! VIDEO_DIR="$(livi_splash_pick_video_dir "${REPO_VIDEO_DIR}")"; then
    echo "No splash videos found. Run sudo scripts/install/pi-splash/install.sh first." >&2
    exit 2
  fi

  if ! livi_splash_pair_exists "${VIDEO_DIR}" "${SLUG}"; then
    echo "Name \"${INPUT}\" is not available. Try './set-splashscreen.sh --help' for more information." >&2
    exit 2
  fi
fi

install -d -m 0755 "${TARGET_DIR}"
printf '%s\n' "${INPUT}" > "${SELECTION_FILE}"
livi_splash_write_boot_splash_id "${APP_CONFIG_FILE}" "${SLUG}"

echo "Installed LIVI startup splash: ${INPUT}"
echo "Selection saved to: ${SELECTION_FILE}"
echo "bootSplashId saved to: ${APP_CONFIG_FILE}"

if [[ -x "${APPLY_HELPER}" ]]; then
  echo "Rebuilding Plymouth animation..."
  if sudo "${APPLY_HELPER}" "${USER}"; then
    echo "Animated boot splash updated."
  else
    echo "Could not rebuild the Plymouth theme automatically." >&2
    echo "Run manually:" >&2
    echo "  sudo ${APPLY_HELPER} ${USER}" >&2
    exit 3
  fi
else
  echo "Plymouth apply helper is not installed yet."
  echo "Run first:"
  echo "  sudo scripts/install/pi-splash/install.sh"
fi
