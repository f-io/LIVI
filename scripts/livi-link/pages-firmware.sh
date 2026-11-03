#!/usr/bin/env bash
# Dongle firmware of the nightly and the latest release, for the dongle's update page on GitHub
#
# usage: pages-firmware.sh <dest-dir>
set -euo pipefail
DEST=${1:?usage: pages-firmware.sh <dest-dir>}
REPO=${GITHUB_REPOSITORY:?}

field() {
  sed -n "s/^  \"$2\": \"\{0,1\}\([^\",]*\)\"\{0,1\},\{0,1\}$/\1/p" "$1"
}

page() {
  cat <<HTML
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>$1</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; padding: 0 1.5rem 3rem; background: #0e1116; color: #e5e7eb; line-height: 1.6;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; }
  main { max-width: 960px; margin: 0 auto; }
  h1 { font-family: Menlo, ui-monospace, SFMono-Regular, Consolas, monospace; font-weight: 400;
    font-size: 2rem; color: #f9fafb; margin: 1.5rem 0 0.5rem; padding-bottom: 0.4rem; border-bottom: 1px solid #272a33; }
  table { width: 100%; border-collapse: collapse; font-size: 0.95rem; }
  th, td { text-align: left; padding: 0.4rem 0.6rem; border-bottom: 1px solid #1f2933; }
  th { color: #9ca3af; font-weight: 500; }
  a { color: #93c5fd; }
  p { color: #9ca3af; }
</style>
</head>
<body>
<main>
<h1>$1</h1>
<p>$2</p>
<table>
$3
</table>
</main>
</body>
</html>
HTML
}

tags=()
gh release view nightly --repo "$REPO" >/dev/null 2>&1 && tags+=(nightly)
latest=$(gh api "repos/$REPO/releases/latest" -q .tag_name 2>/dev/null || true)
[ -n "$latest" ] && tags+=("$latest")

channels='<tr><th>Channel</th><th>Version</th><th>Targets</th></tr>'
for tag in "${tags[@]}"; do
  tmp=$(mktemp -d)
  gh release download "$tag" --repo "$REPO" --dir "$tmp" --pattern '*.firmware.json'
  rows='<tr><th>Target</th><th>Version</th><th>Bundle</th><th>Size</th><th>Manifest</th></tr>'
  version=
  count=0
  for json in "$tmp"/*.firmware.json; do
    target=$(basename "$json" .firmware.json)
    file=$(field "$json" file)
    dir=$DEST/$tag/$target
    mkdir -p "$dir"
    gh release download "$tag" --repo "$REPO" --dir "$dir" --pattern "$file"
    mv "$json" "$dir/firmware.json"
    version="$(field "$dir/firmware.json" version) ($(field "$dir/firmware.json" build))"
    kib=$(($(field "$dir/firmware.json" size) / 1024))
    rows+="<tr><td>$target</td><td>$version</td><td><a href=\"$target/$file\">$file</a></td><td>$kib KiB</td><td><a href=\"$target/firmware.json\">firmware.json</a></td></tr>"
    count=$((count + 1))
    echo "$tag/$target: $file"
  done
  rm -rf "$tmp"
  page "LIVI Link firmware · $tag" "<a href=\"../\">All channels</a> · <a href=\"https://github.com/$REPO/releases/tag/$tag\">Release</a>" "$rows" > "$DEST/$tag/index.html"
  channels+="<tr><td><a href=\"$tag/\">$tag</a></td><td>$version</td><td>$count</td></tr>"
done

mkdir -p "$DEST"
page "LIVI Link firmware" "The dongle's update page loads its firmware from here. Older firmware is on the <a href=\"https://github.com/$REPO/releases\">releases</a>." "$channels" > "$DEST/index.html"
