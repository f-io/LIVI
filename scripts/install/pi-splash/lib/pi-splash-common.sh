#!/usr/bin/env bash

LIVI_SPLASH_SYSTEM_ROOT="${LIVI_SPLASH_SYSTEM_ROOT:-/usr/local/share/livi/pi-splash}"
LIVI_SPLASH_SYSTEM_ASSETS_DIR="${LIVI_SPLASH_SYSTEM_ASSETS_DIR:-${LIVI_SPLASH_SYSTEM_ROOT}/assets}"
LIVI_SPLASH_SYSTEM_VIDEO_DIR="${LIVI_SPLASH_SYSTEM_VIDEO_DIR:-${LIVI_SPLASH_SYSTEM_ASSETS_DIR}/videos}"
LIVI_CONFIG_REL="${LIVI_CONFIG_REL:-.config/LIVI/config.json}"

livi_splash_to_slug() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]-'
}

livi_splash_slug_to_name() {
  case "$1" in
    default) echo "Default" ;;
    alfaromeo) echo "Alfa Romeo" ;;
    bmw) echo "BMW" ;;
    bmwm) echo "BMW M" ;;
    fordmustang) echo "Ford Mustang" ;;
    gmc) echo "GMC" ;;
    landrover) echo "Land Rover" ;;
    mg) echo "MG" ;;
    rangerover) echo "Range Rover" ;;
    *) printf '%s\n' "$1" | awk '{ print toupper(substr($0, 1, 1)) substr($0, 2) }' ;;
  esac
}

livi_splash_is_static_id() {
  [[ "$1" == "default" ]]
}

livi_splash_pair_exists() {
  local video_dir="$1"
  local splash_id="$2"

  [[ -f "${video_dir}/${splash_id}1.h264" && -f "${video_dir}/${splash_id}2.h264" ]]
}

livi_splash_pick_video_dir() {
  local fallback_dir="${1:-}"

  if [[ -d "${LIVI_SPLASH_SYSTEM_VIDEO_DIR}" ]]; then
    printf '%s\n' "${LIVI_SPLASH_SYSTEM_VIDEO_DIR}"
    return 0
  fi

  if [[ -n "${fallback_dir}" && -d "${fallback_dir}" ]]; then
    printf '%s\n' "${fallback_dir}"
    return 0
  fi

  return 1
}

livi_splash_list_slugs() {
  local video_dir="$1"
  local file slug

  [[ -d "${video_dir}" ]] || return 1

  while IFS= read -r file; do
    slug="${file##*/}"
    slug="${slug%1.h264}"
    [[ "${slug}" == _* ]] && continue
    livi_splash_is_static_id "${slug}" && continue
    if livi_splash_pair_exists "${video_dir}" "${slug}"; then
      printf '%s\n' "${slug}"
    fi
  done < <(find "${video_dir}" -maxdepth 1 -type f -name '*1.h264' | sort)
}

livi_splash_config_file_for_home() {
  local target_home="$1"
  printf '%s/%s\n' "${target_home}" "${LIVI_CONFIG_REL}"
}

livi_splash_read_boot_splash_id() {
  local config_file="$1"
  local value=""

  [[ -f "${config_file}" ]] || return 1
  command -v python3 >/dev/null 2>&1 || return 1

  value="$(python3 - "${config_file}" <<'PY'
import json
import sys

try:
    with open(sys.argv[1], "r", encoding="utf-8") as fh:
        data = json.load(fh)
    value = data.get("bootSplashId", "")
    if isinstance(value, str) and value.strip():
        print(value.strip())
except Exception:
    pass
PY
)"

  [[ -n "${value}" ]] || return 1
  printf '%s\n' "${value}"
}

livi_splash_write_boot_splash_id() {
  local config_file="$1"
  local splash_id="$2"

  install -d -m 0755 "$(dirname "${config_file}")"

  if ! command -v python3 >/dev/null 2>&1; then
    echo "Warning: python3 not found; bootSplashId was not written to ${config_file}" >&2
    return 1
  fi

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
}
