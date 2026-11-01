#!/bin/sh
set -eu

target=$1
root=$(cd "$(dirname "$0")/../.." && pwd)

names=$(for m in "$root"/native/crates/*/Cargo.toml "$root"/native/xtask/Cargo.toml; do
  sed -n '/^\[package\]/,/^\[/s/^name = "\(.*\)"/\1/p' "$m"
done)

for dir in "$target"/*/ "$target"/*/*/; do
  [ -e "$dir/.cargo-build-lock" ] || [ -e "$dir/.cargo-lock" ] || continue
  rm -rf "$dir/incremental"
  find "$dir" -maxdepth 1 -type f ! -name '.cargo-*' -delete
  for n in $names; do
    u=$(echo "$n" | tr - _)
    rm -rf "$dir/build/$n" "$dir"/build/"$n"-* "$dir"/deps/"$u"-* "$dir"/deps/lib"$u"-* "$dir"/.fingerprint/"$n"-*
  done
done
