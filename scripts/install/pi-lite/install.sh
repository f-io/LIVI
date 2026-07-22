#!/usr/bin/env bash
set -euo pipefail

# ----------------------------------------
# LIVI Lite Installer (Raspberry Pi OS Lite)
# ----------------------------------------
# Kept so older instructions keep working. The headless installer took over and
# is the only one maintained and tested, so this hands every argument to it.
#
#   scripts/install/headless/install.sh
#
# Prefers the copy next to this file, otherwise fetches it from the repository.
# LIVI_INSTALLER_BRANCH picks a branch other than main.

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCAL="$HERE/../headless/install.sh"

if [ -f "$LOCAL" ]; then
  exec bash "$LOCAL" "$@"
fi

BRANCH="${LIVI_INSTALLER_BRANCH:-main}"
URL="https://raw.githubusercontent.com/f-io/LIVI/${BRANCH}/scripts/install/headless/install.sh"

echo "→ This installer moved to scripts/install/headless/install.sh"
echo "   Fetching it from ${BRANCH}"

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT
curl -fsSL "$URL" -o "$TMP" || {
  echo "Error: cannot fetch $URL" >&2
  exit 1
}
bash "$TMP" "$@"
