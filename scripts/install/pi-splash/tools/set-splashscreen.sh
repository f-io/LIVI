#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
VIDEO_DIR="${ROOT_DIR}/assets/videos"
CONFIG_ROOT="${XDG_CONFIG_HOME:-$HOME/.config}/LIVI"
APP_CONFIG_FILE="${CONFIG_ROOT}/config.json"
TARGET_DIR="${CONFIG_ROOT}/splashscreen"
TARGET_VIDEO_DIR="${TARGET_DIR}/videos"
SELECTION_FILE="${TARGET_DIR}/selection.txt"
APPLY_HELPER="/usr/local/bin/livi-plymouth-apply-splash.sh"

print_help() {
  echo "Usage: ./set-splashscreen.sh \"NAME\""
  echo "Example: ./set-splashscreen.sh \"Range Rover\""
  echo
  echo "Installs the selected startup splash videos for LIVI into:"
  echo "  ${TARGET_DIR}"
  echo
  echo "If the Plymouth helper is installed, the script also asks sudo to rebuild"
  echo "the animated boot splash immediately."
  echo
  echo "Available names:"
  list_names
}

list_names() {
  local file slug pretty
  while IFS= read -r file; do
    slug="${file##*/}"
    slug="${slug%1.h264}"
    pretty="$(slug_to_name "$slug")"
    printf '  %s\n' "$pretty"
  done < <(find "${VIDEO_DIR}" -maxdepth 1 -type f -name '*1.h264' | sort)
}

slug_to_name() {
  case "$1" in
    alfaromeo) echo "Alfa Romeo" ;;
    bmwm) echo "BMW M" ;;
    fordmustang) echo "Ford Mustang" ;;
    landrover) echo "Land Rover" ;;
    rangerover) echo "Range Rover" ;;
    *) echo "$1" | sed -E 's/(^|[^[:alpha:]])([[:alpha:]])/\1\u\2/g' ;;
  esac
}

to_slug() {
  echo "$1" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]-'
}

write_boot_splash_id() {
  local splash_id="$1"
  install -d -m 0755 "${CONFIG_ROOT}"

  if command -v python3 >/dev/null 2>&1; then
    python3 - "${APP_CONFIG_FILE}" "${splash_id}" <<'PY'
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
    return 0
  fi

  echo "Warning: python3 not found; bootSplashId was not written to ${APP_CONFIG_FILE}" >&2
  return 1
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

SLUG="$(to_slug "${INPUT}")"
SPLASH_FILE_1="${VIDEO_DIR}/${SLUG}1.h264"
SPLASH_FILE_2="${VIDEO_DIR}/${SLUG}2.h264"

if [[ ! -f "${SPLASH_FILE_1}" || ! -f "${SPLASH_FILE_2}" ]]; then
  echo "Name \"${INPUT}\" is not available. Try './set-splashscreen.sh --help' for more information." >&2
  exit 2
fi

install -d -m 0755 "${TARGET_VIDEO_DIR}"
install -m 0644 "${SPLASH_FILE_1}" "${TARGET_VIDEO_DIR}/splash1.h264"
install -m 0644 "${SPLASH_FILE_2}" "${TARGET_VIDEO_DIR}/splash2.h264"
printf '%s\n' "${INPUT}" > "${SELECTION_FILE}"
write_boot_splash_id "${SLUG}" || true

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
