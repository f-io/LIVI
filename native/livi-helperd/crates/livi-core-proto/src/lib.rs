pub mod config;
pub mod frame;
pub mod gnss;
pub mod input;
pub mod message;
pub mod patch;
pub mod state;
pub mod telemetry;

#[cfg(test)]
mod contract_ts;

/// Raised with every change to the shape of a message or of the state.
pub const PROTOCOL: u32 = 6;

/// "1" when stdin is a pipe from the process that started this one. The kernel
/// closes it when that process dies, even on SIGKILL.
pub const LIFELINE_ENV: &str = "LIVI_LIFELINE";
