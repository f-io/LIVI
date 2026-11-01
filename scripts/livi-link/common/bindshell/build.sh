#!/usr/bin/env bash
# Build the static bind-shell for one dongle architecture with zig as the cross compiler.
#   build.sh <arch> <output file>
# arch is the name the provisioner embeds it under (armv7 | riscv32 | mipsel).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

arch=${1:?usage: build.sh <arch> <output file>}
out=${2:?usage: build.sh <arch> <output file>}

# musl keeps the binary self-contained so it runs on a stock rootfs with no shared libs of ours.
case "$arch" in
  armv7)   target=arm-linux-musleabihf ;;
  riscv32) target=riscv32-linux-musl ;;
  mipsel)  target=mipsel-linux-musleabi ;;
  *) echo "build.sh: unknown arch $arch" >&2; exit 1 ;;
esac

mkdir -p "$(dirname "$out")"
zig cc -target "$target" -Os -static -s "$HERE/bindshell.c" -o "$out"

file "$out"
