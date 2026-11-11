#!/usr/bin/env bash
set -euo pipefail

# Builds the faad plugin for the Android bundle as static archives: libfaad2 from its release,
# then ext/faad from the gst-plugins-bad source matching the GStreamer Android SDK. The SDK
# ships no decoder that plays DAB+ (960 sample frames with SBR and PS). Leaves libgstfaad.a
# and libfaad.a in OUT_DIR for package-android.sh.
#   build-faad-android.sh GST_ROOT [OUT_DIR]   (ANDROID_NDK_HOME, ANDROID_API, FAAD2_VERSION, FAAD2_SHA256 overridable)

GST_ROOT="$(cd "${1:?GStreamer Android SDK arm64 root required}" && pwd)"
OUT="${2:-$(mktemp -d)}"
FAAD2_VERSION="${FAAD2_VERSION:-2.11.4}"
FAAD2_SHA256="${FAAD2_SHA256:-ee479ccbae4a8387ab696e6f21a481bd83fe3881471cafa81b4ae59d7d3aed43}"
ANDROID_API="${ANDROID_API:-24}"
NDK="${ANDROID_NDK_HOME:?ANDROID_NDK_HOME not set}"

for t in cmake meson ninja pkg-config glib-mkenums glib-genmarshal gobject-query; do
  command -v "$t" >/dev/null || { echo "$t not on PATH" >&2; exit 1; }
done

TOOLCHAIN="$(echo "$NDK"/toolchains/llvm/prebuilt/*)"
[[ -x "$TOOLCHAIN/bin/aarch64-linux-android${ANDROID_API}-clang" ]] || {
  echo "NDK at $NDK has no aarch64 clang for API $ANDROID_API" >&2
  exit 1
}

# The SDK's generator tools are target binaries, the host ones stand in for them.
PC_OVERRIDE="$(mktemp -d)"
sed \
  -e "s|^prefix=.*|prefix=$GST_ROOT|" \
  -e "s|^glib_genmarshal=.*|glib_genmarshal=$(command -v glib-genmarshal)|" \
  -e "s|^gobject_query=.*|gobject_query=$(command -v gobject-query)|" \
  -e "s|^glib_mkenums=.*|glib_mkenums=$(command -v glib-mkenums)|" \
  "$GST_ROOT/lib/pkgconfig/glib-2.0.pc" > "$PC_OVERRIDE/glib-2.0.pc"
export PKG_CONFIG_LIBDIR="$PC_OVERRIDE:$GST_ROOT/lib/pkgconfig"
unset PKG_CONFIG_PATH
GST_VERSION="${GST_VERSION:-$(pkg-config --modversion gstreamer-1.0)}"
GST_URL="https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-${GST_VERSION}.tar.xz"
FAAD_URL="https://github.com/knik0/faad2/archive/refs/tags/${FAAD2_VERSION}.tar.gz"

echo "Building faad plugin for GStreamer $GST_VERSION (Android arm64, API $ANDROID_API) with faad2 $FAAD2_VERSION"

work="$(mktemp -d)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
(
  cd "$work"
  curl -fsSL -o faad2.tar.gz "$FAAD_URL"
  curl -fsSL -o bad.tar.xz "$GST_URL"
  bad_sha256=$(curl -fsSL "$GST_URL.sha256sum" | cut -d' ' -f1)
  printf '%s  faad2.tar.gz\n%s  bad.tar.xz\n' "$FAAD2_SHA256" "$bad_sha256" | sha256sum -c -
  tar -xf faad2.tar.gz
  tar -xf bad.tar.xz

  prefix="$PWD/faad2"
  cmake -S "faad2-${FAAD2_VERSION}" -B _faad -G Ninja \
    -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
    -DANDROID_ABI=arm64-v8a \
    -DANDROID_PLATFORM="android-$ANDROID_API" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
    -DBUILD_SHARED_LIBS=OFF \
    -DFAAD_BUILD_CLI=OFF \
    -DCMAKE_INSTALL_PREFIX="$prefix"
  cmake --build _faad --target install
  libdir="$(dirname "$(find "$prefix" -name libfaad.a | head -1)")"

  cat > cross.ini <<EOF
[binaries]
c = '$TOOLCHAIN/bin/aarch64-linux-android${ANDROID_API}-clang'
cpp = '$TOOLCHAIN/bin/aarch64-linux-android${ANDROID_API}-clang++'
ar = '$TOOLCHAIN/bin/llvm-ar'
strip = '$TOOLCHAIN/bin/llvm-strip'
pkg-config = '$(command -v pkg-config)'

[properties]
needs_exe_wrapper = true
pkg_config_libdir = '$PKG_CONFIG_LIBDIR'

[host_machine]
system = 'android'
cpu_family = 'aarch64'
cpu = 'aarch64'
endian = 'little'
EOF
  meson setup _build "gst-plugins-bad-${GST_VERSION}" --cross-file "$PWD/cross.ini" \
    -Ddefault_library=static \
    -Dauto_features=disabled \
    -Dgpl=enabled \
    -Dfaad=enabled \
    -Dtests=disabled \
    -Dexamples=disabled \
    -Ddoc=disabled \
    -Dintrospection=disabled \
    -Dc_args="-I$prefix/include" \
    -Dc_link_args="-L$libdir"
  ninja -C _build ext/faad/libgstfaad.a
  cp _build/ext/faad/libgstfaad.a "$OUT/libgstfaad.a"
  cp "$libdir/libfaad.a" "$OUT/libfaad.a"
)
rm -rf "$work" "$PC_OVERRIDE"

NM="$TOOLCHAIN/bin/llvm-nm"
"$NM" --defined-only "$OUT/libgstfaad.a" 2>/dev/null | grep -q ' T gst_plugin_faad_register$' || {
  echo "libgstfaad.a does not define gst_plugin_faad_register" >&2
  exit 1
}

echo "faad plugin: $OUT/libgstfaad.a"
