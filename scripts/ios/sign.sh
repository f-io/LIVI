#!/usr/bin/env bash
set -euo pipefail
USAGE="usage: sign.sh <LIVI.ipa> <signing identity> <profile.mobileprovision> [bundle id]"
IPA=${1:?$USAGE}
IDENTITY=${2:?$USAGE}
PROFILE=${3:?$USAGE}
ID=${4:-}

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

security cms -D -i "$PROFILE" -o "$WORK/profile.plist"
plutil -extract Entitlements xml1 -o "$WORK/entitlements.plist" "$WORK/profile.plist"
APP_ID=$(plutil -extract application-identifier raw -o - "$WORK/entitlements.plist")
TEAM=${APP_ID%%.*}
PROFILE_ID=${APP_ID#*.}

if [[ -z "$ID" ]]; then
  [[ "$PROFILE_ID" != *"*"* ]] || { echo "the profile is a wildcard one, pass a bundle id" >&2; exit 1; }
  ID=$PROFILE_ID
elif [[ "$PROFILE_ID" != *"*"* && "$PROFILE_ID" != "$ID" ]]; then
  echo "the profile is for $PROFILE_ID, not $ID" >&2
  exit 1
fi
plutil -replace application-identifier -string "$TEAM.$ID" "$WORK/entitlements.plist"

ditto -x -k "$IPA" "$WORK"
APP="$WORK/Payload/LIVI.app"
plutil -replace CFBundleIdentifier -string "$ID" "$APP/Info.plist"
cp "$PROFILE" "$APP/embedded.mobileprovision"
codesign --force --timestamp=none --sign "$IDENTITY" "$APP/Frameworks/GStreamer.framework"
codesign --force --timestamp=none --sign "$IDENTITY" --entitlements "$WORK/entitlements.plist" "$APP"

OUT="$(cd "$(dirname "$IPA")" && pwd)/$(basename "${IPA%.ipa}")-signed.ipa"
rm -f "$OUT"
(cd "$WORK" && ditto -c -k --keepParent Payload "$OUT")
echo "$OUT"
