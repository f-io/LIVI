#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLED_COMMON="/usr/local/lib/livi/pi-splash/pi-splash-common.sh"
LOCAL_COMMON="${SCRIPT_DIR}/../lib/pi-splash-common.sh"

if [[ -f "${INSTALLED_COMMON}" ]]; then
  # shellcheck source=/usr/local/lib/livi/pi-splash/pi-splash-common.sh
  source "${INSTALLED_COMMON}"
elif [[ -f "${LOCAL_COMMON}" ]]; then
  # shellcheck source=../lib/pi-splash-common.sh
  source "${LOCAL_COMMON}"
else
  echo "Missing pi-splash common library. Run pi-splash/install.sh first." >&2
  exit 1
fi

THEME_NAME="livi"
THEME_DIR="/usr/share/plymouth/themes/${THEME_NAME}"
FRAMES_DIR="${THEME_DIR}/frames"
SCRIPT_PATH="${THEME_DIR}/${THEME_NAME}.script"
PLYMOUTH_PATH="${THEME_DIR}/${THEME_NAME}.plymouth"
SYSTEM_VIDEO_DIR="${LIVI_SPLASH_SYSTEM_VIDEO_DIR}"
SELECTION_ROOT_REL=".config/LIVI/splashscreen"
SELECTION_FILE="selection.txt"
TARGET_FPS="${LIVI_SPLASH_FPS:-10}"
MAX_WIDTH="${LIVI_SPLASH_MAX_WIDTH:-0}"
MAX_HEIGHT="${LIVI_SPLASH_MAX_HEIGHT:-0}"

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
  return 1
}

write_static_script() {
  cat > "${SCRIPT_PATH}" <<'EOF'
Window.SetBackgroundTopColor(0, 0, 0);
Window.SetBackgroundBottomColor(0, 0, 0);

logo = Image("logo.png");

logo_sprite = Sprite();
scaled_for_h = 0;
logo_w = 0;
logo_h = 0;

fun refresh() {
  win_w = Window.GetWidth();
  win_h = Window.GetHeight();
  if (win_w > 0) {
    if (win_h > 0) {
      scale = win_h * 0.75 / logo.GetHeight();
      if (scale > 1) scale = 1;
      if (logo.GetWidth() * scale > win_w * 0.9) scale = (win_w * 0.9) / logo.GetWidth();
      logo_w = logo.GetWidth() * scale;
      logo_h = logo.GetHeight() * scale;
      if (scaled_for_h != win_h) {
        logo_sprite.SetImage(logo.Scale(logo_w, logo_h));
        scaled_for_h = win_h;
      }
      logo_sprite.SetPosition(win_w / 2 - logo_w / 2, win_h / 2 - logo_h / 2, 10);
    }
  }
}
Plymouth.SetRefreshFunction(refresh);
EOF
}

detect_display_size() {
  local cmdline mode width height
  cmdline="/boot/firmware/cmdline.txt"
  [[ -f "${cmdline}" ]] || return 1
  mode="$(tr ' ' '\n' < "${cmdline}" | sed -nE 's/^video=[^:]*:([0-9]+x[0-9]+).*/\1/p' | sed -n '1p')"
  [[ -n "${mode}" ]] || return 1
  width="${mode%x*}"
  height="${mode#*x}"
  [[ "${width}" =~ ^[0-9]+$ && "${height}" =~ ^[0-9]+$ ]] || return 1
  printf '%s %s\n' "${width}" "${height}"
}

detect_display_refresh() {
  local cmdline hz
  cmdline="/boot/firmware/cmdline.txt"
  [[ -f "${cmdline}" ]] || return 1
  hz="$(tr ' ' '\n' < "${cmdline}" | sed -nE 's/^video=[^@]*@([0-9]+).*/\1/p' | sed -n '1p')"
  [[ "${hz}" =~ ^[0-9]+$ ]] || return 1
  printf '%s\n' "${hz}"
}

probe_frame_dimensions() {
  local frame_path="$1"
  local dimensions=""

  dimensions="$(ffprobe -v error -f image2 -show_entries stream=width,height -of csv=p=0:s=x "${frame_path}" 2>/dev/null || true)"
  if [[ -z "${dimensions}" ]]; then
    dimensions="$(ffprobe -v error -show_entries stream=width,height -of csv=p=0:s=x "${frame_path}" 2>/dev/null || true)"
  fi

  [[ -n "${dimensions}" ]] || return 1
  [[ "${dimensions}" == *x* ]] || return 1
  printf '%s\n' "${dimensions}"
}

generate_script() {
  local intro_count="$1"
  local loop_count="$2"
  local frame_width="$3"
  local frame_height="$4"
  local frame_list="$5"
  local frame_hold="$6"
  local total_count="$((intro_count + loop_count))"
  local intro_end=0
  local loop_start="${intro_count}"
  local loop_end="$((total_count - 1))"

  if (( intro_count > 0 )); then
    intro_end=$((intro_count - 1))
  fi

  {
    cat <<EOF
Window.SetBackgroundTopColor(0, 0, 0);
Window.SetBackgroundBottomColor(0, 0, 0);

art_sprite = Sprite();
frame = 0;
frame_tick = 0;
last_frame = -1;
frame_count = ${total_count};
frame_hold = ${frame_hold};
intro_count = ${intro_count};
intro_end = ${intro_end};
loop_count = ${loop_count};
loop_start = ${loop_start};
loop_end = ${loop_end};
frame_w = ${frame_width};
frame_h = ${frame_height};
art_x = 0;
art_y = 0;

EOF

    local idx=0 frame_name
    while IFS= read -r frame_name; do
      printf 'img_%d = Image("frames/%s");\n' "${idx}" "${frame_name}"
      idx=$((idx + 1))
    done <<< "${frame_list}"

    cat <<'EOF'
art_sprite.SetImage(img_0);

fun refresh() {
  win_w = Window.GetWidth();
  win_h = Window.GetHeight();
  if (win_w > 0) {
    if (win_h > 0) {
      art_x = win_w / 2 - frame_w / 2;
      art_y = win_h / 2 - frame_h / 2;

      if (frame != last_frame) {
EOF

    idx=0
    while IFS= read -r frame_name; do
      printf '        if (frame == %d) art_sprite.SetImage(img_%d);\n' "${idx}" "${idx}"
      idx=$((idx + 1))
    done <<< "${frame_list}"

    cat <<'EOF'
        last_frame = frame;
      }
      art_sprite.SetPosition(art_x, art_y, 10);

      frame_tick += 1;
      if (frame_tick >= frame_hold) {
        frame_tick = 0;
        if (intro_count > 0 && frame < intro_end) {
          frame += 1;
        } else {
          if (loop_count > 0) {
            if (frame < loop_start) frame = loop_start;
            if (frame < loop_end) frame += 1;
            else frame = loop_start;
          }
        }
      }
    }
  }
}
Plymouth.SetRefreshFunction(refresh);
EOF
  } > "${SCRIPT_PATH}"
}

TARGET_USER="$(detect_target_user "${1:-}" || true)"
if [[ -z "${TARGET_USER}" ]] || ! id "${TARGET_USER}" >/dev/null 2>&1; then
  echo "Unable to determine target user." >&2
  exit 2
fi

TARGET_HOME="$(getent passwd "${TARGET_USER}" | cut -d: -f6)"
APP_CONFIG_FILE="$(livi_splash_config_file_for_home "${TARGET_HOME}")"
SELECTION_ROOT="${TARGET_HOME}/${SELECTION_ROOT_REL}"

splash_id=""
if splash_id_from_config="$(livi_splash_read_boot_splash_id "${APP_CONFIG_FILE}" 2>/dev/null)" && [[ -n "${splash_id_from_config}" ]]; then
  splash_id="${splash_id_from_config}"
elif [[ -f "${SELECTION_ROOT}/${SELECTION_FILE}" ]]; then
  splash_id="$(tr -d '\n' < "${SELECTION_ROOT}/${SELECTION_FILE}")"
fi
splash_id="$(livi_splash_to_slug "${splash_id:-default}")"
VIDEO_ONE="${SYSTEM_VIDEO_DIR}/${splash_id}1.h264"
VIDEO_TWO="${SYSTEM_VIDEO_DIR}/${splash_id}2.h264"

if [[ ! -d "${THEME_DIR}" || ! -f "${PLYMOUTH_PATH}" ]]; then
  echo "Plymouth theme ${THEME_NAME} is not installed. Run pi-splash/install.sh first." >&2
  exit 3
fi

rm -rf "${FRAMES_DIR}"
rm -rf "${THEME_DIR}/spinner"
install -d -m 0755 "${FRAMES_DIR}"

if [[ ! -d "${SYSTEM_VIDEO_DIR}" ]]; then
  echo "Installed splash assets not found at ${SYSTEM_VIDEO_DIR}; keeping static logo."
  write_static_script
  plymouth-set-default-theme "${THEME_NAME}" -R
  exit 0
fi

if ! livi_splash_pair_exists "${SYSTEM_VIDEO_DIR}" "${splash_id}"; then
  echo "Splash \"${splash_id}\" was not found in ${SYSTEM_VIDEO_DIR}; keeping static logo."
  write_static_script
  plymouth-set-default-theme "${THEME_NAME}" -R
  exit 0
fi

if ! command -v ffmpeg >/dev/null 2>&1 || ! command -v ffprobe >/dev/null 2>&1; then
  echo "ffmpeg/ffprobe are required but not installed." >&2
  exit 4
fi

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

display_width=1920
display_height=1080
display_refresh=60
if detected_display="$(detect_display_size)"; then
  display_width="${detected_display%% *}"
  display_height="${detected_display##* }"
fi
if detected_refresh="$(detect_display_refresh)"; then
  display_refresh="${detected_refresh}"
fi

if [[ "${MAX_WIDTH}" -eq 0 ]]; then
  MAX_WIDTH=$((display_width * 90 / 100))
fi
if [[ "${MAX_HEIGHT}" -eq 0 ]]; then
  MAX_HEIGHT=$((display_height * 75 / 100))
fi

frame_hold=$((display_refresh / TARGET_FPS))
if (( frame_hold < 1 )); then
  frame_hold=1
fi

frame_index=1
intro_count=0
loop_count=0
for input_file in "${VIDEO_ONE}" "${VIDEO_TWO}"; do
  stage_dir="${temp_dir}/$(basename "${input_file}" .h264)"
  install -d -m 0755 "${stage_dir}"
  ffmpeg -hide_banner -loglevel error -fflags +genpts -f h264 -r 30 -i "${input_file}" \
    -vf "fps=${TARGET_FPS},scale='min(${MAX_WIDTH},iw)':'min(${MAX_HEIGHT},ih)':force_original_aspect_ratio=decrease:flags=lanczos,format=rgba" \
    -vsync vfr "${stage_dir}/frame-%04d.png"

  start_index="${frame_index}"
  while IFS= read -r frame_path; do
    printf -v frame_name 'frame-%04d.png' "${frame_index}"
    install -m 0644 "${frame_path}" "${FRAMES_DIR}/${frame_name}"
    frame_index=$((frame_index + 1))
  done < <(find "${stage_dir}" -maxdepth 1 -type f -name 'frame-*.png' | sort)

  file_count=$((frame_index - start_index))
  if [[ "${input_file}" == "${VIDEO_ONE}" ]]; then
    intro_count="${file_count}"
  else
    loop_count="${file_count}"
  fi
done

frame_count=$((frame_index - 1))
if (( frame_count == 0 )); then
  echo "No frames were generated from the selected videos; keeping static logo."
  write_static_script
  plymouth-set-default-theme "${THEME_NAME}" -R
  exit 0
fi

first_frame="$(find "${FRAMES_DIR}" -maxdepth 1 -type f -name 'frame-*.png' | sort | sed -n '1p')"
dimensions="$(probe_frame_dimensions "${first_frame}" || true)"
if [[ -z "${dimensions}" ]]; then
  echo "Could not probe frame dimensions; falling back to display-based sizing."
  frame_width="${MAX_WIDTH}"
  frame_height="${MAX_HEIGHT}"
else
  frame_width="${dimensions%x*}"
  frame_height="${dimensions#*x}"
fi
frame_list="$(find "${FRAMES_DIR}" -maxdepth 1 -type f -name 'frame-*.png' -printf '%f\n' | sort)"

generate_script "${intro_count}" "${loop_count}" "${frame_width}" "${frame_height}" "${frame_list}" "${frame_hold}"

echo "Applied animated Plymouth splash: ${splash_id} (intro=${intro_count}, loop=${loop_count}, ${frame_width}x${frame_height}, hold=${frame_hold})"
plymouth-set-default-theme "${THEME_NAME}" -R
