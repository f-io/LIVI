#!/bin/sh
set -eu

root=$(git rev-parse --show-toplevel)
mark='^// SPDX-FileCopyrightText:'

has_header() {
  head -n1 "$1" | grep -q "$mark"
}

if [ "${1:-}" = "--check" ]; then
  missing=$(git -C "$root" ls-files '*.rs' '*.slint' ':!native/vendor' | while read -r f; do
    has_header "$root/$f" || echo "$f"
  done)
  [ -z "$missing" ] && exit 0
  echo "missing license header:" >&2
  echo "$missing" >&2
  exit 1
fi

for f in "$@"; do
  has_header "$f" && continue
  tmp=$(mktemp)
  {
    echo "// SPDX-FileCopyrightText: 2025-$(date +%Y) Lasse Heitgres"
    echo "// SPDX-License-Identifier: GPL-3.0-or-later"
    echo "// Additional terms: see NOTICE"
    echo
    cat "$f"
  } > "$tmp"
  cat "$tmp" > "$f"
  rm -f "$tmp"
done
