// Compiles the gst_video C/C++ sources into the cdylib: per-OS source lists,
// GStreamer via pkg-config, N-API headers vendored in ../napi-headers. The C++
// archive is linked +whole-archive so the module entry points survive, and the
// GStreamer link flags are emitted after the archives — GNU ld resolves left to
// right and would otherwise drop the shared libs under --as-needed.

const GST_PKGS: [&str; 3] = ["gstreamer-1.0", "gstreamer-app-1.0", "gstreamer-video-1.0"];

fn gst_includes() -> Vec<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        // The GStreamer.framework ships its own pkgconfig dir; make it win.
        let fw = "/Library/Frameworks/GStreamer.framework/Versions/1.0/lib/pkgconfig";
        let prev = std::env::var("PKG_CONFIG_PATH").unwrap_or_default();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PKG_CONFIG_PATH", format!("{fw}:{prev}")) };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("PKG_CONFIG_ALLOW_CROSS", "1") };
    }
    let mut includes = Vec::new();
    for pkg in GST_PKGS {
        let lib = pkg_config::Config::new().cargo_metadata(false).probe(pkg).unwrap();
        includes.extend(lib.include_paths);
    }
    includes
}

fn emit_gst_link_flags() {
    // Every probe re-emits the package's ld args, and all three carry the same rpath
    let first = pkg_config::Config::new().probe(GST_PKGS[0]).unwrap();
    let mut seen: std::collections::HashSet<std::path::PathBuf> =
        first.link_paths.iter().cloned().collect();

    for pkg in &GST_PKGS[1..] {
        let lib = pkg_config::Config::new().cargo_metadata(false).probe(pkg).unwrap();
        for path in &lib.link_paths {
            if seen.insert(path.clone()) {
                println!("cargo:rustc-link-search=native={}", path.display());
            }
        }
        for name in &lib.libs {
            println!("cargo:rustc-link-lib={name}");
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=../../src");
    napi_build::setup();

    let includes = gst_includes();
    let mut cpp = cc::Build::new();
    cpp.cpp(true)
        .std("c++17")
        // napi callbacks legitimately ignore their info parameter
        .flag_if_supported("-Wno-unused-parameter")
        .includes(&includes)
        .link_lib_modifier("+whole-archive")
        .file("../../src/gst_video.cc");

    if cfg!(target_os = "macos") {
        cpp.flag("-mmacosx-version-min=11.0")
            .file("../../src/gst_video_mac.mm")
            .compile("gst_video_cpp");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=QuartzCore");
    } else {
        cpp.compile("gst_video_cpp");
    }

    emit_gst_link_flags();
}
