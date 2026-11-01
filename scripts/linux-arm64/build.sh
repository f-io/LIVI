#!/usr/bin/env bash
# Cross-builds the Linux arm64 AppImage on an x86_64 host in a Debian Trixie container.
# Usage: scripts/linux-arm64/build.sh <repo copy> [cache dir]
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "${1:?repo copy}" && pwd)"
cache="${2:-$HOME/LocalDev/linux-arm64-cache}"
image=livi-linux-arm64-build

mkdir -p "$cache/registry" "$cache/git" "$cache/rustup" "$cache/target"
podman build -q -t "$image" -f "$here/Containerfile" "$here" >/dev/null

podman run --rm \
  -v "$repo:/src:Z" \
  -v "$cache/registry:/cargo/registry:Z" \
  -v "$cache/git:/cargo/git:Z" \
  -v "$cache/rustup:/rustup:Z" \
  -v "$cache/target:/target:Z" \
  -e CARGO_TARGET_DIR=/target \
  -w /src/native \
  "$image" bash -c '
    set -euo pipefail
    rustup toolchain install
    rustup target add aarch64-unknown-linux-gnu
    cargo xtask package-linux --arch arm64
  '
