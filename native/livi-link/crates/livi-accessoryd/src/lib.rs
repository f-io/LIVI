#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub mod mgmt;
pub mod sdp;

#[cfg(target_os = "linux")]
mod server;
#[cfg(target_os = "linux")]
pub use server::{Config, NameSource, run};
