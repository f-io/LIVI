// Compiles the gst_video sources with LIVI_GST_HOST_STANDALONE into the
// livi-gst-host binary (Linux only); its C++ main() is the process entry.
fn main() {
    println!("cargo:rerun-if-changed=../../src");
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut includes = Vec::new();
    for pkg in ["gstreamer-1.0", "gstreamer-app-1.0", "gstreamer-video-1.0"] {
        let lib = pkg_config::Config::new().cargo_metadata(false).probe(pkg).unwrap();
        includes.extend(lib.include_paths);
    }

    let mut cpp = cc::Build::new();
    cpp.cpp(true)
        .std("c++17")
        // napi callbacks legitimately ignore their info parameter
        .flag_if_supported("-Wno-unused-parameter")
        .define("LIVI_GST_HOST_STANDALONE", None)
        .includes(&includes)
        .link_lib_modifier("+whole-archive")
        .file("../../src/gst_video.cc")
        .compile("gst_video_host_cpp");

    // Link flags go last: GNU ld resolves left to right, and shared libs
    // emitted before the archives are dropped under --as-needed. Only the first
    // package emits metadata.
    let first = pkg_config::Config::new().probe("gstreamer-1.0").unwrap();
    let mut seen: std::collections::HashSet<std::path::PathBuf> =
        first.link_paths.iter().cloned().collect();

    for pkg in ["gstreamer-app-1.0", "gstreamer-video-1.0"] {
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
