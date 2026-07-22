#!/usr/bin/env bash
set -euo pipefail

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
#   bash setup-hdmi-pr-display.sh --edid panel.edid [--connector HDMI-A-1]
#   bash setup-hdmi-pr-display.sh --edid panel.edid --no-build   # EDID + cmdline only
#   bash setup-hdmi-pr-display.sh                                 # module patch only
# ============================================================================

CONNECTOR="HDMI-A-1"
EDID_SRC=""
DO_BUILD=1
KSRC=""   # set by fetch_and_sync from the running kernel version
STATE_DIR="/var/lib/livi/hdmi-pr"
FW_EDID="/lib/firmware/edid/livi-display.edid"
CMDLINE="/boot/firmware/cmdline.txt"
MARKER="LIVI HDMI-PR"

usage() {
  sed -n '4,24p' "$0" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --edid) EDID_SRC="${2:-}"; shift 2 ;;
    --connector) CONNECTOR="${2:-}"; shift 2 ;;
    --no-build) DO_BUILD=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 1 ;;
  esac
done

require_pi() {
  if ! grep -qi "raspberry pi" /proc/device-tree/model 2>/dev/null; then
    echo "This script targets a Raspberry Pi" >&2
    exit 1
  fi
}

# The kernel source comes from apt, in the same version as the running kernel, so it
# is the tree the binary was built from. The config comes from the installed headers.
fetch_and_sync() {
  local series pkg tarball vc4src

  series="$(uname -r | cut -d. -f1,2)"
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

  echo "→ Taking the running kernel's config"
  if [[ -f "/lib/modules/$(uname -r)/build/.config" ]]; then
    cp "/lib/modules/$(uname -r)/build/.config" "${KSRC}/.config"
  elif [[ -f "/boot/config-$(uname -r)" ]]; then
    cp "/boot/config-$(uname -r)" "${KSRC}/.config"
  else
    echo "no kernel config found for $(uname -r)" >&2
    exit 1
  fi

  echo "→ Resolving kernel config (non-interactive)"
  make -C "$KSRC" olddefconfig >/dev/null
}

# Patch vc4_hdmi.c, build only the vc4 module, install it for the running kernel.
build_vc4() {
  local kver done_marker f built moddir target ext new_vm run_vm base lv

  kver="$(uname -r)"
  done_marker="${STATE_DIR}/vc4-pr4d-${kver}.done"

  if [[ -f "$done_marker" ]]; then
    echo "→ vc4 PR module already built for ${kver}, skipping"
    return 0
  fi

  if ! lsmod | grep -q '^vc4 '; then
    echo "WARNING: vc4 is not a loaded module, this kernel may build it in." >&2
    echo "         A module swap will not take effect, a full kernel is needed." >&2
  fi

  echo "→ Installing build dependencies"
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends \
    bc bison flex libssl-dev make gcc kmod xz-utils zstd

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
  python3 - "$f" <<'PY'
import sys
path = sys.argv[1]
src = open(path).read()
orig = src

if "livi_mode" not in src:
    a = "\tret = drm_edid_connector_add_modes(connector);\n"
    if a not in src:
        sys.stderr.write("get_modes anchor not found, vc4 layout changed\n"); sys.exit(2)
    src = src.replace(a, a + (
        "\t{\n"
        "\t\tstruct drm_display_mode *livi_mode;\n"
        "\n"
        "\t\tlist_for_each_entry(livi_mode, &connector->probed_modes, head)\n"
        "\t\t\tif (livi_mode->clock && livi_mode->clock < 25000)\n"
        "\t\t\t\tlivi_mode->flags |= DRM_MODE_FLAG_DBLCLK;\n"
        "\t}\n"
    ), 1)

if "mode->clock < 12500 ? 4 : 2" not in src:
    o = "\tu32 pixel_rep = (mode->flags & DRM_MODE_FLAG_DBLCLK) ? 2 : 1;\n"
    if o not in src:
        sys.stderr.write("pixel_rep anchor not found, vc4 layout changed\n"); sys.exit(2)
    src = src.replace(o, (
        "\tu32 pixel_rep = (mode->flags & DRM_MODE_FLAG_DBLCLK) ?\n"
        "\t\t(mode->clock < 12500 ? 4 : 2) : 1;\n"
    ))

if "livi_ret" not in src:
    o = "\treturn drm_atomic_helper_connector_hdmi_check(connector, state);\n"
    if o not in src:
        sys.stderr.write("atomic_check anchor not found, vc4 layout changed\n"); sys.exit(2)
    src = src.replace(o, (
        "\tint livi_ret = drm_atomic_helper_connector_hdmi_check(connector, state);\n"
        "\n"
        "\tif (!livi_ret && new_state->hdmi.tmds_char_rate &&\n"
        "\t    new_state->hdmi.tmds_char_rate < 25000000)\n"
        "\t\tnew_state->hdmi.tmds_char_rate *= 2;\n"
        "\n"
        "\treturn livi_ret;\n"
    ), 1)

import re
src = re.sub(
    r"\t/\* LIVI HDMI-PR: the shared HDMI helper[\s\S]*?tmds_char_rate \*= 2;\n\n",
    "", src)

if src != orig:
    open(path, "w").write(src)
    print("   patched (get_modes + pixel_rep 4x + tmds_char_rate x4)")
else:
    print("   already patched")
PY

  echo "→ Preparing module build"
  make -C "$KSRC" LOCALVERSION="$lv" modules_prepare

  echo "→ Building the vc4 module only (clean rebuild)"
  make -C "$KSRC" LOCALVERSION="$lv" M=drivers/gpu/drm/vc4 clean >/dev/null 2>&1 || true
  make -C "$KSRC" LOCALVERSION="$lv" -j"$(nproc)" M=drivers/gpu/drm/vc4 modules

  built="${KSRC}/drivers/gpu/drm/vc4/vc4.ko"
  [[ -f "$built" ]] || { echo "build produced no vc4.ko" >&2; exit 1; }

  # Never install a module the running kernel would refuse to load
  echo "→ Verifying the module matches the running kernel"
  new_vm="$(modinfo "$built" -F vermagic 2>/dev/null || true)"
  run_vm="$(modinfo vc4 -F vermagic 2>/dev/null || true)"
  if [[ -z "$new_vm" || "$new_vm" != "$run_vm" ]]; then
    echo "ERROR: built module does not match the running kernel, not installing." >&2
    echo "  built:   ${new_vm:-<none>}" >&2
    echo "  running: ${run_vm:-<none>}" >&2
    echo "If you changed kernels, run 'rm -rf ~/linux-source-*' and re-run." >&2
    exit 1
  fi
  echo "   vermagic OK: ${new_vm}"

  moddir="/lib/modules/${kver}/kernel/drivers/gpu/drm/vc4"
  target="$(ls "$moddir"/vc4.ko* 2>/dev/null | head -1 || true)"
  [[ -n "$target" ]] || { echo "no existing vc4.ko* under $moddir" >&2; exit 1; }
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
  echo "   vc4 patched and installed (original backed up to ${target}.livi-bak)"
}

# Force the panel EDID and reference it from cmdline.txt.
install_edid() {
  local size token
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
echo "  wlr-randr                   expect the native panel resolution"
echo "  sudo cat /sys/kernel/debug/dri/1/state | grep -i tmds   expect ~32.5 MHz for a sub-12.5 MHz panel (4x)"
