// Wired CarPlay control channel: lockdown's com.apple.carkit.service, whose TLS stream
// carries iAP2.

mod carkit;
pub use carkit::{open_carkit, pair_record_path, CarkitChannel, LOCKDOWN_SERVICE};

// Same carkit stream, but sourced from the system usbmuxd instead of our own mux, so the
// phone can sit on any Mac USB port.
#[cfg(unix)]
pub mod usbmuxd;

/// macOS only: the phone's own USB network interface (enX) that carries the AV stream, resolved
/// from its UDID in the IORegistry.
#[cfg(target_os = "macos")]
pub mod mac_network;
