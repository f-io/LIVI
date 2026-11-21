# Sourced by build.sh: static gstreamer-full. Video from a URL (playbin3, HLS) does not fit the NOR.
# Needs TOP, HERE, JOBS, CROSS_COMPILE, REALGCC, US_ARCH, log(), apply_patches(), download().
GST_VER=${GST_VER:-1.28.9}
GST_SRC=$TOP/gstreamer-$GST_VER
GST_PREFIX=$TOP/gst-prefix
# gstreamer-rs asks for gstreamer-1.0, which here is gstreamer-full with its plugins.
GST_PC=$TOP/gst-pc

ALSA_VER=1.2.14
ALSA_SHA=be9c88a0b3604367dd74167a2b754a35e142f670292ae47a2fdef27a2ee97a32
FAAD_VER=2.11.2
FAAD_SHA=3fcbd305e4abd34768c62050e18ca0986f7d9c5eca343fb98275418013065c0e
FFMPEG_VER=7.1.2
FFMPEG_SHA=089bc60fb59d6aecc5d994ff530fd0dcb3ee39aa55867849a2bbc4e555f9c304

gst_fetch() {
  local url=$1 out=$2 sha=$3
  echo "$sha  $out" | sha256sum -c --status 2>/dev/null && return 0
  log "fetch $(basename "$out")"
  download "$url" -o "$out"
  echo "$sha  $out" | sha256sum -c --status || { rm -f "$out"; log "$(basename "$out") does not match its hash"; exit 3; }
}

gst_deps() {
  local cc=$TOP/userspace/stage/musl-$US_ARCH/bin/musl-gcc src=$TOP/gst-deps
  rm -rf "$src"
  mkdir -p "$src"

  gst_fetch "https://www.alsa-project.org/files/pub/lib/alsa-lib-$ALSA_VER.tar.bz2" "$TOP/alsa-lib-$ALSA_VER.tar.bz2" "$ALSA_SHA"
  tar -xjf "$TOP/alsa-lib-$ALSA_VER.tar.bz2" -C "$src"
  log "alsa-lib $ALSA_VER"
  # seq and rawmidi stay: gstalsa builds its midi source along.
  # On the device it reads rootfs/usr/share/alsa/alsa.conf, upstream's files stay in the prefix.
  (cd "$src/alsa-lib-$ALSA_VER" && CC=$cc ./configure --host=riscv64-linux-gnu --prefix="$GST_PREFIX" \
    --with-configdir=/usr/share/alsa --enable-static --disable-shared --disable-ucm --disable-topology --disable-hwdep --disable-old-symbols \
    --disable-python --disable-mixer-pymods --without-debug --disable-aload \
    --with-pcm-plugins="plug rate linear route copy dmix dsnoop asym softvol null hooks" --with-ctl-plugins= >/dev/null \
    && make -j"$JOBS" >/dev/null && make install alsaconfigdir="$GST_PREFIX/share/alsa" >/dev/null)

  gst_fetch "https://github.com/knik0/faad2/archive/refs/tags/$FAAD_VER.tar.gz" "$TOP/faad2-$FAAD_VER.tar.gz" "$FAAD_SHA"
  tar -xzf "$TOP/faad2-$FAAD_VER.tar.gz" -C "$src"
  log "faad2 $FAAD_VER"
  cmake -S "$src/faad2-$FAAD_VER" -B "$src/faad2-$FAAD_VER/b" -DCMAKE_SYSTEM_NAME=Linux \
    -DCMAKE_C_COMPILER="$cc" -DCMAKE_BUILD_TYPE=MinSizeRel -DCMAKE_INSTALL_PREFIX="$GST_PREFIX" \
    -DBUILD_SHARED_LIBS=OFF -DFAAD_BUILD_CLI=OFF >/dev/null
  cmake --build "$src/faad2-$FAAD_VER/b" -j"$JOBS" >/dev/null
  cmake --install "$src/faad2-$FAAD_VER/b" >/dev/null

  gst_fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VER.tar.xz" "$TOP/ffmpeg-$FFMPEG_VER.tar.xz" "$FFMPEG_SHA"
  tar -xJf "$TOP/ffmpeg-$FFMPEG_VER.tar.xz" -C "$src"
  log "ffmpeg $FFMPEG_VER (aac, alac)"
  # Only AirPlay's audio: the video decodes in the VE through v4l2codecs.
  (cd "$src/ffmpeg-$FFMPEG_VER" && ./configure --prefix="$GST_PREFIX" --enable-cross-compile --arch=riscv64 \
    --target-os=linux --cc="$cc" --ar="${CROSS_COMPILE}ar" --ranlib="${CROSS_COMPILE}ranlib" \
    --nm="${CROSS_COMPILE}nm" --strip="${CROSS_COMPILE}strip" --pkg-config=false \
    --extra-cflags="-ffunction-sections -fdata-sections" --extra-ldflags=-static --enable-static --disable-shared \
    --enable-small --disable-debug --disable-programs --disable-doc --disable-network --disable-everything \
    --disable-autodetect --disable-swscale --disable-swresample --disable-postproc --disable-avdevice \
    --disable-rvv --disable-iconv --enable-decoder=aac,alac >/dev/null \
    && make -j"$JOBS" >/dev/null && make install >/dev/null)
}

gst_build() {
  if [[ ! -d $GST_SRC ]]; then
    log "fetch gstreamer $GST_VER"
    git clone -q --depth 1 --branch "$GST_VER" https://gitlab.freedesktop.org/gstreamer/gstreamer.git "$GST_SRC"
  fi
  apply_patches "$HERE/gst-patches" "$GST_SRC"

  local stamp sum
  stamp=$GST_PREFIX/.livi-build
  sum=$(cat "$HERE/gst.sh" "$HERE"/gst-patches/*.patch | sha256sum | cut -d' ' -f1)
  if [[ ! -f $stamp || $(cat "$stamp") != "$sum" ]]; then
    log "gstreamer $GST_VER (static gstreamer-full)"
    rm -rf "$GST_SRC/_build" "$GST_PREFIX"
    gst_deps
    cat >"$TOP/gst-cross.txt" <<EOF
[binaries]
c = '$TOP/userspace/stage/musl-$US_ARCH/bin/musl-gcc'
ar = '${CROSS_COMPILE}ar'
strip = '${CROSS_COMPILE}strip'
pkg-config = 'pkg-config'

[built-in options]
c_args = ['-Os', '-ffunction-sections', '-fdata-sections', '-I$GST_PREFIX/include']
c_link_args = ['-static', '-L$GST_PREFIX/lib']

[properties]
needs_exe_wrapper = true
pkg_config_libdir = '$GST_PREFIX/lib/pkgconfig'

[host_machine]
system = 'linux'
cpu_family = 'riscv64'
cpu = 'riscv64'
endian = 'little'
EOF
    (cd "$GST_SRC" && meson setup _build --cross-file "$TOP/gst-cross.txt" --prefix="$GST_PREFIX" \
      -Ddefault_library=static -Dauto_features=disabled -Dbuildtype=minsize -Db_lto=false \
      -Dbase=enabled -Dgood=enabled -Dbad=enabled -Dugly=disabled -Dlibav=enabled -Dgpl=enabled \
      -Dpython=disabled -Dges=disabled -Drtsp_server=disabled -Ddevtools=disabled -Dqt5=disabled -Dqt6=disabled \
      -Dtests=disabled -Dexamples=disabled -Ddoc=disabled -Dintrospection=disabled -Dnls=disabled -Dorc=disabled \
      -Dtools=disabled -Dgst-full=enabled -Dgst-full-target-type=static_library \
      -Dgst-full-libraries=app,video,codecparsers,allocators,audio,pbutils,tag,rtp,controller -Dgst-full-plugins="*" \
      "-Dgst-full-elements=rtp:rtpmp4gdepay,rtpopusdepay,rtpopuspay;rtpmanager:rtpjitterbuffer;alsa:alsasink,alsasrc;audioparsers:aacparse" \
      -Dgstreamer:check=disabled -Dgstreamer:ptp-helper=disabled -Dgstreamer:libunwind=disabled -Dgstreamer:libdw=disabled \
      -Dgst-plugins-base:app=enabled -Dgst-plugins-base:typefind=enabled \
      -Dgst-plugins-base:audioconvert=enabled -Dgst-plugins-base:audioresample=enabled \
      -Dgst-plugins-base:volume=enabled -Dgst-plugins-base:alsa=enabled -Dgst-plugins-base:opus=enabled \
      -Dgst-plugins-good:audioparsers=enabled -Dgst-plugins-good:level=enabled \
      -Dgst-plugins-good:rtp=enabled -Dgst-plugins-good:rtpmanager=enabled \
      -Dgst-plugins-bad:faad=enabled -Dgst-plugins-bad:videoparsers=enabled -Dgst-plugins-bad:v4l2codecs=enabled \
      -Dopus:extra-programs=disabled -Dopus:tests=disabled -Dopus:docs=disabled \
      -Dglib:tests=false -Dglib:xattr=false -Dglib:selinux=disabled -Dglib:libmount=disabled \
      -Dglib:sysprof=disabled -Dglib:introspection=disabled -Dglib:nls=disabled >/dev/null)
    ninja -C "$GST_SRC/_build" -j"$JOBS" install >/dev/null
    echo "$sum" >"$stamp"
    GST_REBUILT=1
  fi

  mkdir -p "$GST_PC"
  printf 'Name: gstreamer-1.0\nDescription: gstreamer-full with its static plugins\nVersion: %s\nRequires: gstreamer-full-1.0\n' \
    "$GST_VER" >"$GST_PC/gstreamer-1.0.pc"
}

gst_env() {
  export PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_ALL_STATIC=1 SYSTEM_DEPS_LINK=static PKG_CONFIG_SYSROOT_DIR=
  export PKG_CONFIG_LIBDIR=$GST_PC:$GST_PREFIX/lib/pkgconfig:$GST_PREFIX/lib/gstreamer-1.0/pkgconfig
}
