#!/usr/bin/env bash
set -euo pipefail
export PATH="$PATH:/usr/sbin:/sbin"

# ============================================================================
# HDMI-PR mode (Raspberry Pi)
# ============================================================================
# Drives a small low-pixel-clock RGB/VGA panel over HDMI using HDMI pixel
# repetition. A panel below HDMI's 25 MHz clock floor would otherwise be an
# invalid link.
#
# Only the vc4 module is rebuilt.
#
# Usage:
#   bash setup-hdmi-pr-display.sh --edid panel.edid [--connector HDMI-A-2]
#     (default connector: the single connected one, else HDMI-A-1)
#   bash setup-hdmi-pr-display.sh --edid panel.edid --no-build   # EDID + cmdline only
#   bash setup-hdmi-pr-display.sh                                 # module patch only
#   bash setup-hdmi-pr-display.sh --kernel <version>              # build for another kernel
# ============================================================================

CONNECTOR=""   # --connector overrides; otherwise the single connected one, else HDMI-A-1
EDID_SRC=""
DO_BUILD=1
KVER="$(uname -r)"   # --kernel builds for another installed kernel (postinst hook)
CUR_SRC_VER=""       # source version of the current build, set by fetch_rpt_apt
KSRC=""   # set by fetch_and_sync from the running kernel version
STATE_DIR="/var/lib/livi/hdmi-pr"
WORK_ROOT="${HOME}/LIVI/kernel-src"
FW_EDID="/lib/firmware/edid/livi-display.edid"
CMDLINE="/boot/firmware/cmdline.txt"
MARKER="LIVI HDMI-PR"

usage() {
  sed -n '4,20p' "$0" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --edid) EDID_SRC="${2:-}"; shift 2 ;;
    --connector) CONNECTOR="${2:-}"; shift 2 ;;
    --no-build) DO_BUILD=0; shift ;;
    --kernel) KVER="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

# A panel without a hotplug line shows "disconnected" even when wired,
# so detection only replaces the default when it is unambiguous.
if [[ -z "$CONNECTOR" ]]; then
  mapfile -t conns < <(for d in /sys/class/drm/card*-*; do
    [[ -f "$d/status" && "$(cat "$d/status" 2>/dev/null)" == "connected" ]] \
      && basename "$d" | sed 's/^card[0-9]*-//'
  done | sort -u)
  if [[ ${#conns[@]} -eq 1 ]]; then
    CONNECTOR="${conns[0]}"
    echo "→ Using connected connector ${CONNECTOR}"
  else
    CONNECTOR="HDMI-A-1"
    if [[ ${#conns[@]} -gt 1 ]]; then
      echo "→ Multiple connected connectors (${conns[*]}), defaulting to ${CONNECTOR}." >&2
      echo "  Use --connector to pick another one." >&2
    fi
  fi
fi

require_pi() {
  if ! grep -qi "raspberry pi" /proc/device-tree/model 2>/dev/null; then
    echo "This script targets a Raspberry Pi" >&2
    exit 1
  fi
}

ensure_rpi_source() {
  if ! command -v rpi-source >/dev/null 2>&1; then
    sudo wget -q https://raw.githubusercontent.com/RPi-Distro/rpi-source/master/rpi-source \
      -O /usr/local/bin/rpi-source
    sudo chmod +x /usr/local/bin/rpi-source
  fi
}

# rpi-update kernel
fetch_rpi_source() {
  KSRC="${HOME}/linux"
  local vc4src="${KSRC}/drivers/gpu/drm/vc4/vc4_hdmi.c"
  if [[ ! -f "$vc4src" ]]; then
    echo "→ Fetching matching kernel source (rpi-source)"
    ensure_rpi_source
    rm -rf "$KSRC" "$HOME"/linux-*
    yes '' | rpi-source --skip-gcc || true
  fi
  [[ -f "$vc4src" ]] || {
    echo "kernel source not found after rpi-source (this custom kernel's source may be unpublished)" >&2
    exit 1
  }
  echo "→ Resolving kernel config (non-interactive)"
  make -C "$KSRC" olddefconfig >/dev/null
}

# Copies the running kernel's config and Module.symvers into KSRC.
sync_config_and_symvers() {
  local kver="$1" sym

  echo "→ Taking the running kernel's config"
  if [[ -f "/lib/modules/${kver}/build/.config" ]]; then
    cp "/lib/modules/${kver}/build/.config" "${KSRC}/.config"
  elif [[ -f "/boot/config-${kver}" ]]; then
    cp "/boot/config-${kver}" "${KSRC}/.config"
  else
    echo "no kernel config found for ${kver}" >&2
    exit 1
  fi
  echo "→ Resolving kernel config (non-interactive)"
  make -C "$KSRC" olddefconfig >/dev/null

  echo "→ Providing Module.symvers from the running kernel"
  sym="/lib/modules/${kver}/build/Module.symvers"
  if [[ ! -f "$sym" ]]; then
    sudo apt-get install -y --no-install-recommends "linux-headers-${kver}" 2>/dev/null || true
  fi
  [[ -f "$sym" ]] || sym="/usr/src/linux-headers-${kver}/Module.symvers"
  if [[ ! -f "$sym" ]]; then
    echo "ERROR: no Module.symvers for ${kver}. Install the kernel headers:" >&2
    echo "       sudo apt-get install linux-headers-${kver}" >&2
    exit 1
  fi
  if ! [[ "$sym" -ef "${KSRC}/Module.symvers" ]]; then
    cp "$sym" "${KSRC}/Module.symvers"
  fi
}

# Raspberry Pi OS apt kernel (+rpt).
fetch_rpt_apt() {
  local kver ver upstream pool work dsc f k vc4src out
  local -a keyrings
  kver="$KVER"
  ver="$(dpkg-query -W -f='${Version}' "linux-image-${kver}")"   # 1:6.18.34-1+rpt1
  ver="${ver#*:}"                                                # 6.18.34-1+rpt1
  CUR_SRC_VER="$ver"
  upstream="${ver%%-*}"                                          # 6.18.34
  pool="https://archive.raspberrypi.com/debian/pool/main/l/linux"
  work="${WORK_ROOT}/cache"
  KSRC="${WORK_ROOT}/linux-${ver}"
  vc4src="${KSRC}/drivers/gpu/drm/vc4/vc4_hdmi.c"

  if [[ ! -f "$vc4src" ]]; then
    command -v dpkg-source >/dev/null 2>&1 || \
      sudo apt-get install -y --no-install-recommends dpkg-dev

    echo "→ Fetching kernel source ${ver} from the archive pool"
    mkdir -p "$work"
    for f in "linux_${ver}.dsc" "linux_${upstream}.orig.tar.xz" "linux_${ver}.debian.tar.xz"; do
      [[ -f "${work}/${f}" ]] || wget -q -O "${work}/${f}" "${pool}/${f}" || {
        rm -f "${work}/${f}"
        echo "cannot fetch ${pool}/${f} (this kernel may have left the pool)" >&2
        exit 1
      }
    done
    dsc="${work}/linux_${ver}.dsc"

    # Aborts only on a BAD signature; dpkg-source verifies the tarball hashes.
    if command -v gpgv >/dev/null 2>&1; then
      keyrings=()
      for k in /usr/share/keyrings/raspberrypi-archive-*.pgp \
               /usr/share/keyrings/raspberrypi-archive-*.gpg; do
        [[ -f "$k" ]] && keyrings+=(--keyring "$k")
      done
      if out="$(gpgv ${keyrings[@]+"${keyrings[@]}"} "$dsc" 2>&1)"; then
        echo "   .dsc signature OK"
      elif grep -q "BAD signature" <<<"$out"; then
        echo "ERROR: ${dsc##*/} carries a BAD signature, refusing to use it" >&2
        exit 1
      else
        echo "   NOTE: .dsc uploader key not in the local keyrings, relying on HTTPS + hash checks"
      fi
    fi

    echo "→ Unpacking (applies the Raspberry Pi patch set, takes a while)"
    rm -rf "$KSRC"
    dpkg-source --no-check -x "$dsc" "$KSRC" >/dev/null
  fi
  [[ -f "$vc4src" ]] || { echo "kernel source not found at $KSRC" >&2; exit 1; }

  sync_config_and_symvers "$kver"
}

# Stock apt kernel
fetch_apt() {
  local kver series pkg tarball vc4src
  kver="$KVER"
  series="$(echo "$kver" | cut -d. -f1,2)"
  pkg="linux-source-${series}"
  tarball="/usr/src/${pkg}.tar.xz"
  KSRC="${HOME}/${pkg}"
  vc4src="${KSRC}/drivers/gpu/drm/vc4/vc4_hdmi.c"

  if [[ ! -f "$vc4src" ]]; then
    if [[ ! -f "$tarball" ]]; then
      echo "→ Installing ${pkg}"
      sudo apt-get install -y --no-install-recommends "$pkg"
    fi
    [[ -f "$tarball" ]] || { echo "kernel source package left no ${tarball}" >&2; exit 1; }
    echo "→ Unpacking ${tarball}"
    rm -rf "$KSRC"
    tar -xf "$tarball" -C "$HOME"
  fi
  [[ -f "$vc4src" ]] || { echo "kernel source not found at $KSRC" >&2; exit 1; }

  sync_config_and_symvers "$kver"
}

fetch_and_sync() {
  local kver
  kver="$KVER"
  if [[ "$kver" == *+rpt-* || "$kver" == *-rpt-* ]]; then
    if dpkg -s "linux-image-${kver}" >/dev/null 2>&1; then
      echo "→ Raspberry Pi OS apt kernel (+rpt) detected, using the archive pool"
      fetch_rpt_apt
    else
      echo "→ Raspberry Pi (+rpt) kernel without an apt package, using rpi-source"
      require_running_kernel
      fetch_rpi_source
    fi
  elif apt-cache show "linux-headers-${kver}" >/dev/null 2>&1; then
    echo "→ Stock apt kernel detected, using apt source + headers"
    fetch_apt
  else
    echo "→ Custom / rpi-update kernel, using rpi-source"
    require_running_kernel
    fetch_rpi_source
  fi
}

# rpi-source can only resolve the running kernel
require_running_kernel() {
  if [[ "$KVER" != "$(uname -r)" ]]; then
    echo "ERROR: --kernel ${KVER} needs an apt kernel, rpi-source only builds the running one" >&2
    exit 1
  fi
}

# Each step checks for its own mark, so an already patched tree is left alone.
# The replacements stay quoted, bash 5.2 would otherwise expand every & to the match.
patch_vc4_hdmi() {
  local f="$1" LC_ALL=C src orig a o n

  src="$(cat "$f"; printf x)"
  src="${src%x}"
  orig="$src"

  if [[ "$src" != *livi_mode* ]]; then
    a=$'\tret = drm_edid_connector_add_modes(connector);\n'
    if [[ "$src" != *"$a"* ]]; then
      echo "get_modes anchor not found, vc4 layout changed" >&2
      exit 2
    fi
    printf -v n '%s\n' \
      $'\t{' \
      $'\t\tstruct drm_display_mode *livi_mode;' \
      '' \
      $'\t\tlist_for_each_entry(livi_mode, &connector->probed_modes, head)' \
      $'\t\t\tif (livi_mode->clock && livi_mode->clock < 25000)' \
      $'\t\t\t\tlivi_mode->flags |= DRM_MODE_FLAG_DBLCLK;' \
      $'\t}'
    src="${src/"$a"/"$a$n"}"
  fi

  if [[ "$src" != *"mode->clock < 12500 ? 4 : 2"* ]]; then
    o=$'\tu32 pixel_rep = (mode->flags & DRM_MODE_FLAG_DBLCLK) ? 2 : 1;\n'
    if [[ "$src" != *"$o"* ]]; then
      echo "pixel_rep anchor not found, vc4 layout changed" >&2
      exit 2
    fi
    printf -v n '%s\n' \
      $'\tu32 pixel_rep = (mode->flags & DRM_MODE_FLAG_DBLCLK) ?' \
      $'\t\t(mode->clock < 12500 ? 4 : 2) : 1;'
    src="${src//"$o"/"$n"}"
  fi

  if [[ "$src" != *livi_ret* ]]; then
    o=$'\treturn drm_atomic_helper_connector_hdmi_check(connector, state);\n'
    if [[ "$src" != *"$o"* ]]; then
      echo "atomic_check anchor not found, vc4 layout changed" >&2
      exit 2
    fi
    printf -v n '%s\n' \
      $'\tint livi_ret = drm_atomic_helper_connector_hdmi_check(connector, state);' \
      '' \
      $'\tif (!livi_ret && new_state->hdmi.tmds_char_rate &&' \
      $'\t    new_state->hdmi.tmds_char_rate < 25000000)' \
      $'\t\tnew_state->hdmi.tmds_char_rate *= 2;' \
      '' \
      $'\treturn livi_ret;'
    src="${src/"$o"/"$n"}"
  fi

  if [[ "$src" == "$orig" ]]; then
    echo "   already patched"
    return 0
  fi
  printf '%s' "$src" > "$f"
  echo "   patched (get_modes + pixel_rep 4x + tmds_char_rate x4)"
}

# Patch vc4_hdmi.c, build only the vc4 module, install it for the running kernel.
build_vc4() {
  local kver done_marker f built moddir target ext new_vm run_vm base lv

  kver="$KVER"
  done_marker="${STATE_DIR}/vc4-pr4d-${kver}.done"

  install_kernel_hook

  if [[ -f "$done_marker" ]]; then
    echo "→ vc4 PR module already built for ${kver}, skipping"
    return 0
  fi

  if [[ "$kver" == "$(uname -r)" ]] && ! lsmod | grep -q '^vc4 '; then
    echo "WARNING: vc4 is not a loaded module, this kernel may build it in." >&2
    echo "         A module swap will not take effect, a full kernel is needed." >&2
  fi

  # apt holds its lock while the kernel hook runs, so only install when missing
  if ! command -v bison >/dev/null || ! command -v flex >/dev/null || \
     ! command -v gcc >/dev/null || ! command -v make >/dev/null || \
     ! dpkg -s libssl-dev >/dev/null 2>&1; then
    echo "→ Installing build dependencies"
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends \
      git bc bison flex libssl-dev make gcc kmod wget xz-utils zstd
  fi

  fetch_and_sync
  f="${KSRC}/drivers/gpu/drm/vc4/vc4_hdmi.c"

  rm -f "$KSRC/.scmversion"
  base="$(make -C "$KSRC" -s kernelrelease 2>/dev/null || true)"
  lv=""
  if [[ "$kver" == "${base}"* ]]; then
    lv="${kver#"$base"}"
    [[ -n "$lv" ]] && echo "→ Using LOCALVERSION '${lv}' to match ${kver}"
  fi

  echo "→ Patching vc4_hdmi.c (DBLCLK <25MHz, 4x pixel_rep <12.5MHz, clock x4)"
  patch_vc4_hdmi "$f"

  echo "→ Preparing module build"
  make -C "$KSRC" LOCALVERSION="$lv" modules_prepare

  # The generic apt linux-source resolves to a different suffix.
  # Force the exact release from the running kernel's headers.
  local hdr="/lib/modules/${kver}/build"
  [[ -f "$hdr/include/generated/utsrelease.h" ]] || hdr="/usr/src/linux-headers-${kver}"
  if [[ -f "$hdr/include/generated/utsrelease.h" ]]; then
    if [[ "$hdr/include/generated/utsrelease.h" -ef "${KSRC}/include/generated/utsrelease.h" ]]; then
      echo "   headers are the kernel source itself, nothing to sync"
    else
      cp "$hdr/include/generated/utsrelease.h" "${KSRC}/include/generated/utsrelease.h"
      if [[ -f "$hdr/include/config/kernel.release" ]] &&
         ! [[ "$hdr/include/config/kernel.release" -ef "${KSRC}/include/config/kernel.release" ]]; then
        cp "$hdr/include/config/kernel.release" "${KSRC}/include/config/kernel.release"
      fi
    fi
  fi

  local jobs mem_kb
  if [[ -n "${JOBS:-}" ]]; then
    jobs="$JOBS"
  else
    jobs="$(nproc)"
    mem_kb="$(awk '/^MemTotal:/{print $2}' /proc/meminfo 2>/dev/null || echo 0)"
    if [[ "$mem_kb" -gt 0 && "$mem_kb" -lt 768000 && "$jobs" -gt 2 ]]; then
      jobs=2
    fi
  fi

  echo "→ Building the vc4 module only (clean rebuild, -j${jobs})"
  make -C "$KSRC" LOCALVERSION="$lv" M=drivers/gpu/drm/vc4 clean >/dev/null 2>&1 || true
  make -C "$KSRC" LOCALVERSION="$lv" -j"$jobs" M=drivers/gpu/drm/vc4 modules

  built="${KSRC}/drivers/gpu/drm/vc4/vc4.ko"
  [[ -f "$built" ]] || { echo "build produced no vc4.ko" >&2; exit 1; }

  moddir="/lib/modules/${kver}/kernel/drivers/gpu/drm/vc4"
  target="$(ls "$moddir"/vc4.ko* 2>/dev/null | head -1 || true)"
  [[ -n "$target" ]] || { echo "no existing vc4.ko* under $moddir" >&2; exit 1; }

  # Never install a module the target kernel would refuse to load
  echo "→ Verifying the module matches kernel ${kver}"
  new_vm="$(modinfo "$built" -F vermagic 2>/dev/null || true)"
  run_vm="$(modinfo "$target" -F vermagic 2>/dev/null || true)"
  if [[ -z "$new_vm" || "$new_vm" != "$run_vm" ]]; then
    echo "ERROR: built module does not match kernel ${kver}, not installing." >&2
    echo "  built:  ${new_vm:-<none>}" >&2
    echo "  target: ${run_vm:-<none>}" >&2
    echo "If you changed kernels, run 'rm -rf ${WORK_ROOT}' and re-run." >&2
    exit 1
  fi
  echo "   vermagic OK: ${new_vm}"

  ext="${target##*vc4.ko}"

  echo "→ Installing vc4.ko (matching existing compression '${ext:-none}')"
  sudo cp -p "$target" "${target}.livi-bak"
  case "$ext" in
    "")    sudo cp "$built" "$target" ;;
    .xz)   xz -c -f "$built" | sudo tee "$target" >/dev/null ;;
    .zst)  zstd -q -19 -c -f "$built" | sudo tee "$target" >/dev/null ;;
    .gz)   gzip -c -f "$built" | sudo tee "$target" >/dev/null ;;
    *)     echo "unknown module compression: ${ext}" >&2; exit 1 ;;
  esac
  sudo depmod -a "$kver"

  sudo mkdir -p "$STATE_DIR"
  sudo touch "$done_marker"
  sudo rm -f "${STATE_DIR}/BUILD-FAILED-${kver}"
  echo "   vc4 patched and installed (original backed up to ${target}.livi-bak)"

  cleanup_kernel_src
}

# Keep only the tarball cache of the version
cleanup_kernel_src() {
  local d f
  [[ -d "$WORK_ROOT" ]] || return 0
  echo "→ Cleaning up kernel sources"
  for d in "$WORK_ROOT"/linux-*; do
    [[ -d "$d" ]] && rm -rf "$d"
  done
  for f in "$WORK_ROOT"/cache/*; do
    [[ -f "$f" ]] || continue
    case "$(basename "$f")" in
      *"${CUR_SRC_VER%%-*}"*) [[ -n "$CUR_SRC_VER" ]] || rm -f "$f" ;;
      *) rm -f "$f" ;;
    esac
  done
}

# Rebuild the module for every future kernel, a failure warns before the reboot.
install_kernel_hook() {
  local self="/usr/local/lib/livi/setup-hdmi-pr-display.sh"
  echo "→ Installing the kernel post-install hook"
  if ! [[ "$0" -ef "$self" ]]; then
    sudo install -m 0755 "$0" "$self"
  fi
  sudo tee /etc/kernel/postinst.d/livi-vc4 >/dev/null <<EOF
#!/bin/sh
# LIVI HDMI-PR: rebuild the patched vc4 for a freshly installed kernel.
version="\$1"
[ -n "\$version" ] || exit 0
[ -d /var/lib/livi/hdmi-pr ] || exit 0
[ -x "$self" ] || exit 0
echo "livi-vc4: building the patched vc4 for \$version" >&2
if ! "$self" --kernel "\$version" </dev/null; then
  touch "/var/lib/livi/hdmi-pr/BUILD-FAILED-\$version"
  echo "" >&2
  echo "**********************************************************************" >&2
  echo "* livi-vc4: PATCH BUILD FAILED for kernel \$version" >&2
  echo "* The display WILL STAY DARK after booting this kernel." >&2
  echo "* Do not reboot. Re-run: sudo $self --kernel \$version" >&2
  echo "**********************************************************************" >&2
fi
exit 0
EOF
  sudo chmod +x /etc/kernel/postinst.d/livi-vc4
}

# Mode of the EDID's first DTD as "WxH@Hz", the mode the panel actually runs.
edid_video_mode() {
  local -a b
  read -r -a b <<< "$(od -An -tu1 -v -j 54 -N 8 "$EDID_SRC" | tr '\n' ' ')"
  local clk=$(( (b[1] << 8 | b[0]) * 10000 ))
  local hact=$(( (b[4] & 0xF0) << 4 | b[2] ))
  local hbl=$((  (b[4] & 0x0F) << 8 | b[3] ))
  local vact=$(( (b[7] & 0xF0) << 4 | b[5] ))
  local vbl=$((  (b[7] & 0x0F) << 8 | b[6] ))
  local total=$(( (hact + hbl) * (vact + vbl) ))
  (( clk == 0 || total == 0 || hact == 0 || vact == 0 )) && return 1
  printf '%sx%s@%s' "$hact" "$vact" "$(( (clk + total / 2) / total ))"
}

# Force the panel EDID and reference it from cmdline.txt.
install_edid() {
  local size token mode line stripped
  [[ -f "$EDID_SRC" ]] || { echo "EDID file not found: $EDID_SRC" >&2; exit 1; }
  size="$(wc -c < "$EDID_SRC")"
  if [[ "$size" != "128" && "$size" != "256" ]]; then
    echo "EDID must be a raw 128 or 256 byte blob, got ${size} bytes." >&2
    echo "Use the binary EDID, not the .h C array." >&2
    exit 1
  fi

  echo "→ Installing panel EDID to ${FW_EDID}"
  sudo mkdir -p "$(dirname "$FW_EDID")"
  sudo cp "$EDID_SRC" "$FW_EDID"

  token="drm.edid_firmware=${CONNECTOR}:edid/$(basename "$FW_EDID")"
  if grep -q "drm.edid_firmware=${CONNECTOR}:" "$CMDLINE"; then
    echo "→ cmdline already forces an EDID on ${CONNECTOR}, leaving it untouched"
  else
    echo "→ Appending '${token}' to ${CMDLINE}"
    sudo cp -p "$CMDLINE" "${CMDLINE}.livi-bak"
    sudo sed -i "1 s|\$| ${token}|" "$CMDLINE"
  fi

  # Console and splash must come up in the panel's mode, not a guessed default.
  if mode="$(edid_video_mode)"; then
    echo "→ Pinning video=${CONNECTOR}:${mode} in ${CMDLINE}"
    line="$(tr -d '\n' < "$CMDLINE")"
    stripped="$(sed -E 's/[[:space:]]*video=[^[:space:]]+//g' <<< "$line")"
    stripped="${stripped} video=${CONNECTOR}:${mode}"
    [ "$stripped" = "$line" ] || echo "$stripped" | sudo tee "$CMDLINE" >/dev/null
  else
    echo "→ EDID has no parsable DTD, not pinning a video mode"
  fi

  # Early KMS (boot splash) runs from the initramfs
  if command -v update-initramfs >/dev/null 2>&1; then
    echo "→ Packing the EDID into the initramfs"
    sudo tee /etc/initramfs-tools/hooks/livi-edid >/dev/null <<'EOF'
#!/bin/sh
[ "$1" = "prereqs" ] && { echo ""; exit 0; }
. /usr/share/initramfs-tools/hook-functions
for f in /lib/firmware/edid/*.edid; do
  [ -f "$f" ] && copy_file firmware "$f"
done
EOF
    sudo chmod +x /etc/initramfs-tools/hooks/livi-edid
    sudo update-initramfs -u
  fi
}

require_pi

if [[ "$DO_BUILD" == 1 ]]; then
  build_vc4
fi

if [[ -n "$EDID_SRC" ]]; then
  install_edid
elif [[ "$DO_BUILD" == 0 ]]; then
  echo "Nothing to do: --no-build given and no --edid provided" >&2
  exit 1
else
  echo "→ No --edid given, skipped the EDID step (run again with --edid to force the panel timing)"
fi

echo
echo "Done. Reboot to apply:"
echo "  sudo reboot"
echo
echo "After reboot, verify:"
echo "  kmsprint | grep -i hdmi     expect the native panel mode (DBLCLK shows as '2x', the 4x is in the wire clock)"
echo "  sudo cat /sys/kernel/debug/dri/1/state | grep -i tmds   expect ~32.5 MHz for a sub-12.5 MHz panel (4x)"
