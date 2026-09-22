// Compiles the Cocoa window code into the cdylib on macOS. The archive is
// linked +whole-archive so its entry points survive, and the GStreamer link
// flags follow it, because GNU ld resolves left to right.

const GST_PKGS: [&str; 3] = ["gstreamer-1.0", "gstreamer-app-1.0", "gstreamer-video-1.0"];

/// The GStreamer.framework ships its own pkgconfig dir, put it first.
fn prefer_framework_pkgconfig() {
    let fw = "/Library/Frameworks/GStreamer.framework/Versions/1.0/lib/pkgconfig";
    let prev = std::env::var("PKG_CONFIG_PATH").unwrap_or_default();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("PKG_CONFIG_PATH", format!("{fw}:{prev}")) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("PKG_CONFIG_ALLOW_CROSS", "1") };
}

fn gst_includes() -> Vec<std::path::PathBuf> {
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

    if !cfg!(target_os = "macos") {
        return;
    }

    prefer_framework_pkgconfig();
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag("-mmacosx-version-min=11.0")
        .includes(gst_includes())
        .link_lib_modifier("+whole-archive")
        .file("../../src/gst_video_mac.mm")
        .compile("gst_video_cpp");
    println!("cargo:rustc-link-lib=framework=Cocoa");
    println!("cargo:rustc-link-lib=framework=QuartzCore");

    emit_gst_link_flags();
}
