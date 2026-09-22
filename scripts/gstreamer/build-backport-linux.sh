#!/usr/bin/env bash
set -euo pipefail

# forky's prebuilt libraries reference glibc symbols newer than the stable deploy target
# (sqrtf@GLIBC_2.43, ...). Build GStreamer from forky sources against the container's own
# stable libraries instead, so the bundle never needs more glibc than the target has.

REPO="$(pwd)"
PATCHES="$REPO/scripts/gstreamer/patches"
APPLY_PATCHES="${APPLY_PATCHES:-0}"
SOURCE_SUITE="${SOURCE_SUITE:-forky}"
PROVENANCE=/usr/share/gst-build/meson-builds.txt

export DEBIAN_FRONTEND=noninteractive
export DEB_BUILD_OPTIONS="nocheck noautodbgsym parallel=$(nproc)"

cat > /etc/apt/sources.list.d/gst-src.sources <<EOF
Types: deb-src
URIs: http://deb.debian.org/debian
Suites: $SOURCE_SUITE
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
EOF

apt-get update
apt-get install -y --no-install-recommends build-essential fakeroot dpkg-dev

apply_series() {
  local sub="$1"
  while IFS= read -r p; do
    [ -n "$p" ] || continue
    patch -p1 -i "$PATCHES/$sub/$p"
  done < "$PATCHES/$sub/series"
}

# Run in the unpacked source dir: fetch build deps, then apply our patches.
prepare_source() {
  local srcpkg="$1" sub="$2"
  if [[ "$srcpkg" == gst-plugins-bad1.0 ]]; then
    # not packaged in stable; only needed for features we do not build
    sed -i "/libtensorflow-lite-dev/d" debian/control
  fi
  apt-get build-dep -y --arch-only ./
  if [[ "$APPLY_PATCHES" == 1 && -n "$sub" ]]; then
    apply_series "$sub"
  fi
}

fetch_source() {
  local work="$1" srcpkg="$2"
  cd "$work"
  apt-get source -o APT::Sandbox::User=root "$srcpkg"
  cd "$(find . -mindepth 1 -maxdepth 1 -type d | head -1)"
}

# Full Debian packaging; the resulting .debs are installed so later builds and the bundle see them.
build_deb() {
  local srcpkg="$1" sub="${2:-}"
  local work
  work="$(mktemp -d)"
  (
    fetch_source "$work" "$srcpkg"
    prepare_source "$srcpkg" "$sub"
    dpkg-buildpackage -B -uc -us
    local debs=() deb
    for deb in "$work"/*.deb; do
      case "$(basename "$deb")" in
        gstreamer1.0-gtk3_*|gstreamer1.0-qt5_*|gstreamer1.0-qt6_*) ;;
        *) debs+=("$deb") ;;
      esac
    done
    apt-get install -y --no-install-recommends "${debs[@]}"
  )
  rm -rf "$work"
}

# Only the plugins the bundle ships, straight from meson. Skips the rest of the Debian package
# (webrtc, tflite, ...) that stable cannot satisfy and we never bundle.
build_meson() {
  local srcpkg="$1" sub="$2"
  shift 2
  local work multiarch
  multiarch="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"
  work="$(mktemp -d)"
  (
    fetch_source "$work" "$srcpkg"
    prepare_source "$srcpkg" "$sub"
    meson setup _build \
      --prefix=/usr \
      --libdir="lib/$multiarch" \
      -Dtests=disabled \
      -Dexamples=disabled \
      -Ddoc=disabled \
      -Dintrospection=disabled \
      "$@"
    meson compile -C _build
    meson install -C _build
    mkdir -p "$(dirname "$PROVENANCE")"
    echo "$srcpkg $(dpkg-parsechangelog -l debian/changelog -S Version)" >> "$PROVENANCE"
  )
  rm -rf "$work"
}

build_deb gstreamer1.0
build_deb gst-plugins-base1.0 gst-plugins-base
build_deb gst-plugins-good1.0
build_meson gst-plugins-bad1.0 gst-plugins-bad \
  -Dauto_features=disabled \
  -Dgpl=enabled \
  -Dvideoparsers=enabled \
  -Dfaad=enabled \
  -Dv4l2codecs=enabled \
  -Dkms=enabled \
  -Dva=enabled \
  -Dwayland=enabled
build_deb gst-libav1.0

ldconfig
gst-inspect-1.0 --version | head -1
