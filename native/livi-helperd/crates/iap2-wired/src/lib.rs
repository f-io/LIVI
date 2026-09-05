// Wired CarPlay control channel: lockdown's com.apple.carkit.service, whose TLS stream
// carries iAP2.

mod carkit;
pub use carkit::{open_carkit, pair_record_path, CarkitChannel, LOCKDOWN_SERVICE};
