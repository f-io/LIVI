//! Entry shell for livi-gst-host. On Linux the process entry is the C++ main()
//! from the whole-archive gst_video objects; elsewhere the binary is a stub.
#![cfg_attr(target_os = "linux", no_main)]

// The element choice, the screen receiver's AEAD, the NAL helpers and the
// stream framing live in these crates; the references keep rustc from dropping
// the otherwise-unused dependencies at link time.
use livi_video_codec as _;
use livi_video_player as _;
#[cfg(target_os = "linux")]
use livi_crypto_node as _;
#[cfg(target_os = "linux")]
use livi_screen_stream as _;
#[cfg(target_os = "linux")]
use livi_host_proto as _;
#[cfg(target_os = "linux")]
use livi_video_fanout as _;
#[cfg(target_os = "linux")]
use livi_video_nal as _;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("livi-gst-host runs on Linux only");
    std::process::exit(1);
}
