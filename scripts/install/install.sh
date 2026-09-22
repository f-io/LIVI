#!/usr/bin/env bash
set -euo pipefail

# Picks the desktop or headless installer by the host's boot target.
# Override with --desktop / --headless; other args pass through to the flow.

FLOW=""
ARGS=()
for arg in "$@"; do
  case "$arg" in
    --desktop)  FLOW=desktop ;;
    --headless) FLOW=headless ;;
    *)          ARGS+=("$arg") ;;
  esac
done

# get-default is machine state, unlike a session's DISPLAY, so it holds over SSH.
if [ -z "$FLOW" ]; then
  if [ "$(systemctl get-default 2>/dev/null)" = graphical.target ]; then
    FLOW=desktop
  else
    FLOW=headless
  fi
fi

echo "→ Host looks $FLOW, running the $FLOW installer (override with --desktop / --headless)"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$DIR/$FLOW/install.sh"
if [ ! -f "$SCRIPT" ]; then
  RAW="https://raw.githubusercontent.com/${LIVI_REPO:-f-io/LIVI}/${LIVI_INSTALLER_BRANCH:-main}/scripts/install"
  SCRIPT="$(mktemp)"
  curl -fsSL "$RAW/$FLOW/install.sh" -o "$SCRIPT" \
    || { echo "Error: cannot obtain the $FLOW installer" >&2; exit 1; }
fi

exec bash "$SCRIPT" ${ARGS[@]+"${ARGS[@]}"}
