#!/usr/bin/env bash
# Dongle firmware and the provisioning tool of the nightly and the latest release, for GitHub Pages.
# The manifests come from the livi-link-<channel> branch that livi-link runs update, the release's
# from its tag once the release is newer than that branch; the files from the "assets" release.
#
# usage: pages-firmware.sh <dest-dir>
set -euo pipefail
DEST=${1:?usage: pages-firmware.sh <dest-dir>}
REPO=${GITHUB_REPOSITORY:?}

field() {
  sed -n "s/^  \"$2\": \"\{0,1\}\([^\",]*\)\"\{0,1\},\{0,1\}$/\1/p" "$1"
}

sha() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

manifest() {
  gh api -H 'Accept: application/vnd.github.raw+json' \
    "repos/$REPO/contents/assets/livi-link/$2/firmware.json?ref=$1" 2>/dev/null
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
  h2 { font-weight: 500; font-size: 1.2rem; color: #f9fafb; margin: 2rem 0 0.5rem; }
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
$3
</main>
</body>
</html>
HTML
}

tags=()
gh release view nightly --repo "$REPO" >/dev/null 2>&1 && tags+=(nightly)
latest='' published=''
if release=$(gh api "repos/$REPO/releases/latest" -q '.tag_name + " " + .published_at' 2>/dev/null); then
  read -r latest published <<<"$release"
fi
[ -n "$latest" ] && tags+=("$latest")

branch_date() {
  gh api "repos/$REPO/commits/livi-link-$1" -q .commit.committer.date 2>/dev/null
}

platform() {
  case $1 in
    linux-x64) echo "Linux x64" ;;
    linux-arm64) echo "Linux arm64" ;;
    macos-x64) echo "macOS Intel" ;;
    macos-arm64) echo "macOS Apple silicon" ;;
    *) echo "$1" ;;
  esac
}

tools() {
  local dir=$DEST/$1/provision rows= json platforms
  mkdir -p "$dir"
  platforms=$(gh api "repos/$REPO/contents/assets/livi-link-provision?ref=$2" \
    -q '.[] | select(.name | endswith(".json")) | .name | rtrimstr(".json")' 2>/dev/null) || platforms=
  for platform in $platforms; do
    json=$dir/$platform.json
    gh api -H 'Accept: application/vnd.github.raw+json' \
      "repos/$REPO/contents/assets/livi-link-provision/$platform.json?ref=$2" > "$json"
    name=$(field "$json" file)
    want=$(field "$json" sha256)
    gh release download assets --repo "$REPO" --pattern "$name-${want:0:16}" --output "$dir/$name"
    [ "$(sha "$dir/$name")" = "$want" ] || { echo "$1/$name does not match its manifest" >&2; exit 1; }
    rm "$json"
    mib=$(($(wc -c < "$dir/$name") / 1048576))
    rows+="<tr><td>$(platform "$platform")</td><td><a href=\"provision/$name\">$name</a></td><td>$mib MiB</td></tr>"
    echo "$1/provision: $name" >&2
  done
  [ -n "$rows" ] || { rmdir "$dir"; return; }
  echo "<h2 id=\"provision\">Provisioning tool</h2><table><tr><th>Platform</th><th>File</th><th>Size</th></tr>$rows</table>"
}

channels='<tr><th>Channel</th><th>Version</th><th>Built</th><th>Targets</th><th>Provisioning tool</th></tr>'
for tag in "${tags[@]}"; do
  ref=$tag
  if [ "$tag" = nightly ]; then
    branch_date nightly >/dev/null && ref=livi-link-nightly
  elif head=$(branch_date release) && [[ "$head" > "$published" ]]; then
    ref=livi-link-release
  fi
  rows='<tr><th>Target</th><th>Version</th><th>Built</th><th>Bundle</th><th>Size</th><th>Manifest</th></tr>'
  version=
  built=
  count=0
  targets=$(gh api "repos/$REPO/contents/assets/livi-link?ref=$ref" -q '.[] | select(.type == "dir") | .name')
  for target in $targets; do
    dir=$DEST/$tag/$target
    mkdir -p "$dir"
    manifest "$ref" "$target" > "$dir/firmware.json" || { rm -rf "$dir"; continue; }
    file=$(field "$dir/firmware.json" file)
    want=$(field "$dir/firmware.json" sha256)
    gh release download assets --repo "$REPO" --pattern "${file%.lfwb}-${want:0:16}.lfwb" --output "$dir/$file"
    [ "$(sha "$dir/$file")" = "$want" ] || { echo "$tag/$target: $file does not match firmware.json" >&2; exit 1; }
    build=$(field "$dir/firmware.json" build)
    version="$(field "$dir/firmware.json" version) ($build)"
    built=$(gh api "repos/$REPO/commits/$build" -q .commit.committer.date 2>/dev/null) || built=
    built=${built:0:10}
    kib=$(($(field "$dir/firmware.json" size) / 1024))
    rows+="<tr><td>$target</td><td>$version</td><td>$built</td><td><a href=\"$target/$file\">$file</a></td><td>$kib KiB</td><td><a href=\"$target/firmware.json\">firmware.json</a></td></tr>"
    count=$((count + 1))
    echo "$tag/$target: $file"
  done
  provision=$(tools "$tag" "$ref")
  page "LIVI Link firmware · $tag" "<a href=\"../\">All channels</a> · <a href=\"https://github.com/$REPO/releases/tag/$tag\">Release</a>" \
    "<h2>Firmware</h2><table>$rows</table>$provision" > "$DEST/$tag/index.html"
  channels+="<tr><td><a href=\"$tag/\">$tag</a></td><td>$version</td><td>$built</td><td>$count</td><td>${provision:+<a href=\"$tag/#provision\">download</a>}</td></tr>"
done

mkdir -p "$DEST"
page "LIVI Link firmware" "Firmware and the provisioning tool for the <a href=\"https://github.com/$REPO/blob/main/LIVI-LINK.md\">LIVI-LINK</a>." "<table>$channels</table>" > "$DEST/index.html"
