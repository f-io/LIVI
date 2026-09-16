//! Shared LIVI-Link mDNS: wire format + interface-tracking daemon.
//! Used by CPC200 (bin/livi-link, argv[0]=mdnsd) and V821B (bin/livi-netd).
pub mod wire;
pub mod daemon;

pub use wire::{Name, PORT, GROUP, build_answer, parse_query};
