#!/usr/bin/env bash
set -euo pipefail

# Builds the faad plugin for the macOS bundle: libfaad2 statically from its release, then
# ext/faad from the gst-plugins-bad source matching the installed GStreamer.framework. The
# framework ships no decoder that plays DAB+ (960 sample frames with SBR and PS), libav
# drops SBR there. Prints the path of libgstfaad.dylib for package-macos.sh.
#   build-faad-macos.sh [OUT_DIR]      (GST_VERSION, GST_ROOT, FAAD2_VERSION overridable)

GST_ROOT="${GST_ROOT:-/Library/Frameworks/GStreamer.framework/Versions/1.0}"
OUT="${1:-$(mktemp -d)}"
FAAD2_VERSION="${FAAD2_VERSION:-2.11.4}"
ARCHS=(arm64 x86_64)

export PKG_CONFIG_PATH="$GST_ROOT/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
GST_VERSION="${GST_VERSION:-$(pkg-config --modversion gstreamer-1.0)}"
GST_URL="https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${GST_VERSION}.tar.xz"
FAAD_URL="https://github.com/knik0/faad2/archive/refs/tags/${FAAD2_VERSION}.tar.gz"

for t in cmake meson ninja glib-mkenums glib-genmarshal gobject-query; do
  command -v "$t" >/dev/null || { echo "$t not on PATH (brew install cmake meson ninja glib)" >&2; exit 1; }
done
# Same glib-2.0.pc override as build-patched-macos.sh: the framework's generator tools do not run.
PC_OVERRIDE="$(mktemp -d)"
sed \
  -e "s|^prefix=.*|prefix=$GST_ROOT|" \
  -e "s|^glib_genmarshal=.*|glib_genmarshal=$(command -v glib-genmarshal)|" \
  -e "s|^gobject_query=.*|gobject_query=$(command -v gobject-query)|" \
  -e "s|^glib_mkenums=.*|glib_mkenums=$(command -v glib-mkenums)|" \
  "$GST_ROOT/lib/pkgconfig/glib-2.0.pc" > "$PC_OVERRIDE/glib-2.0.pc"
export PKG_CONFIG_PATH="$PC_OVERRIDE:$PKG_CONFIG_PATH"

echo "Building faad plugin for GStreamer $GST_VERSION with faad2 $FAAD2_VERSION"

work="$(mktemp -d)"
(
  cd "$work"
  curl -fsSL -o faad2.tar.gz "$FAAD_URL"
  curl -fsSL -o bad.tar.xz "$GST_URL"
  tar -xf faad2.tar.gz
  tar -xf bad.tar.xz
  slices=()
  for arch in "${ARCHS[@]}"; do
    prefix="$PWD/faad2-$arch"
    cmake -S "faad2-${FAAD2_VERSION}" -B "_faad-$arch" \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_OSX_ARCHITECTURES="$arch" \
      -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0 \
      -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
      -DBUILD_SHARED_LIBS=OFF \
      -DFAAD_BUILD_CLI=OFF \
      -DCMAKE_INSTALL_PREFIX="$prefix"
    cmake --build "_faad-$arch" --target install
    # Only the static archive exists, so the plugin links it in.
    find "$prefix" -name '*.dylib' -delete

    cross=()
    if [ "$arch" != "$(uname -m)" ]; then
      cpu_family=$([ "$arch" = arm64 ] && echo aarch64 || echo "$arch")
      cat > "cross-$arch.ini" <<EOF
[binaries]
c = ['clang', '-arch', '$arch']
cpp = ['clang++', '-arch', '$arch']
objc = ['clang', '-arch', '$arch']
objcpp = ['clang++', '-arch', '$arch']
ar = 'ar'
strip = 'strip'
pkg-config = 'pkg-config'

[properties]
needs_exe_wrapper = true

[host_machine]
system = 'darwin'
subsystem = 'macos'
kernel = 'xnu'
cpu_family = '$cpu_family'
cpu = '$arch'
endian = 'little'
EOF
      cross=(--cross-file "$PWD/cross-$arch.ini")
    fi
    libdir="$(dirname "$(find "$prefix" -name libfaad.a | head -1)")"
    meson setup "_build-$arch" "gst-plugins-bad-${GST_VERSION}" ${cross[@]+"${cross[@]}"} \
      -Dpkg_config_path="$PKG_CONFIG_PATH" \
      -Dauto_features=disabled \
      -Dgpl=enabled \
      -Dfaad=enabled \
      -Dtests=disabled \
      -Dexamples=disabled \
      -Ddoc=disabled \
      -Dc_args="-I$prefix/include" \
      -Dc_link_args="-L$libdir"
    meson compile -C "_build-$arch"
    slice="$PWD/libgstfaad-$arch.dylib"
    cp "$(find "_build-$arch" -name libgstfaad.dylib | head -1)" "$slice"
    while read -r rp; do
      case "$rp" in /*|*gst-libs*) install_name_tool -delete_rpath "$rp" "$slice" ;; esac
    done < <(otool -l "$slice" | awk '/LC_RPATH/{getline;getline;print $2}')
    slices+=("$slice")
  done
  mkdir -p "$OUT"
  lipo -create -output "$OUT/libgstfaad.dylib" "${slices[@]}"
)
PLUGIN="$OUT/libgstfaad.dylib"
for arch in "${ARCHS[@]}"; do
  lipo "$PLUGIN" -verify_arch "$arch" || {
    echo "built plugin lacks $arch" >&2
    exit 1
  }
done
if otool -L "$PLUGIN" | grep -q libfaad; then
  echo "built plugin links a shared libfaad, it has to be static" >&2
  exit 1
fi
codesign --force --sign - "$PLUGIN" >/dev/null 2>&1 || true

inspect="$("$GST_ROOT/bin/gst-inspect-1.0" "$PLUGIN" 2>&1)"
grep "faad" <<<"$inspect" >/dev/null || {
  echo "built plugin does not load or register faad:" >&2
  echo "$inspect" >&2
  exit 1
}

echo "faad plugin: $PLUGIN"
